use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use chrono::{SecondsFormat, Utc};
use crossbeam_channel::{Sender, Receiver, unbounded};
use dirs::{home_dir};
use monosodium::{auth_sign, auth_verify};
use monosodium::util::{CryptoBuf};
use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use rand::prelude::*;
use rand::distributions::{Uniform};
use schemas::{Revise, deserialize_revision, serialize_revision_into};
use schemas::v1::{DistroInfoV0, GpusV0, MachineConfigV0, SystemSetupV0, Bot2RegistryV0, Registry2BotV0, _NewCiRunV0, RegisterCiRepoV0};
use serde::{Deserialize, Serialize};
use tooling::config::{ApiConfig, ApiAuth, Config};
use tooling::docker::*;
use tooling::ipc::*;
use tooling::query::{Maybe, Open, Query, fail};
use tooling::state::{ImageSpec, ImageManifest, RootManifest, Sysroot};

use std::collections::{VecDeque};
use std::env;
use std::fs::{File, create_dir_all};
use std::io::{Read, Write, Cursor};
use std::path::{PathBuf};
use std::process::{exit};
use std::str;
use std::sync::{Arc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{JoinHandle, sleep, spawn};
use std::time::{Duration};

pub fn runloop(git_head_commit: &[u8]) -> Maybe {
  Context::new(git_head_commit)?._init(false)?.runloop()
}

fn base64_str_to_vec(len_bytes: usize, b64_str: &str) -> Option<Vec<u8>> {
  let mut buf = Vec::with_capacity(len_bytes);
  if base64::decode_config_buf(
      b64_str,
      base64::URL_SAFE,
      &mut buf,
  ).is_err() {
    return None;
  }
  if buf.len() != len_bytes {
    None
  } else {
    Some(buf)
  }
}

fn base64_str_to_buf(len_bytes: usize, b64_str: &str) -> Option<CryptoBuf> {
  base64_str_to_vec(len_bytes, b64_str)
    .map(|buf| CryptoBuf::from_vec(len_bytes, buf))
}

enum BotWsMsg {
  Open(BotWsSender),
  Bin(Vec<u8>),
  Hup,
  Error,
}

struct BotWsConn {
  delay_lo: f64,
  delay_hi: f64,
  loopback_s: Sender<LoopbackMsg>,
  watchdog_s: Sender<WatchdogMsg>,
  reg2bot_s: Sender<BotWsMsg>,
  reg_echo_ctr: Arc<AtomicUsize>,
  reconnect: Arc<Mutex<Reconnect>>,
  registry_s: ws::Sender,
}

impl BotWsConn {
  pub fn new(loopback_s: Sender<LoopbackMsg>, watchdog_s: Sender<WatchdogMsg>, reg2bot_s: Sender<BotWsMsg>, reg_echo_ctr: Arc<AtomicUsize>, reconnect: Arc<Mutex<Reconnect>>, registry_s: ws::Sender) -> BotWsConn {
    BotWsConn{
      delay_lo: 3600.0 - 900.0,
      delay_hi: 3600.0 - 150.0,
      loopback_s,
      watchdog_s,
      reg2bot_s,
      reg_echo_ctr,
      reconnect,
      registry_s,
    }
  }

  fn keepalive_delay_ms(&mut self) -> f64 {
    let delay_s_dist = Uniform::new_inclusive(self.delay_lo, self.delay_hi);
    let delay_ms = thread_rng().sample(&delay_s_dist) * 1000.0;
    delay_ms
  }
}

impl ws::Handler for BotWsConn {
  fn on_open(&mut self, _: ws::Handshake) -> ws::Result<()> {
    eprintln!("TRACE: BotWsConn: on_open: delay interval: {:?} s {:?} s",
        self.delay_lo, self.delay_hi);
    {
      let mut reconn = self.reconnect.lock();
      reconn.open = true;
      reconn.backoff_count = 0;
    }
    let delay_ms = self.keepalive_delay_ms();
    let echo_ctr = self.reg_echo_ctr.fetch_add(1, Ordering::Relaxed) + 1;
    self.registry_s.timeout(delay_ms as _, ws::util::Token(echo_ctr)).unwrap();
    self.reg2bot_s.send(BotWsMsg::Open(BotWsSender{
      registry_s: self.registry_s.clone(),
      secret_token_buf: None,
    })).unwrap();
    Ok(())
  }

  fn on_message(&mut self, msg: ws::Message) -> ws::Result<()> {
    let delay_ms = self.keepalive_delay_ms();
    let echo_ctr = self.reg_echo_ctr.fetch_add(1, Ordering::Relaxed) + 1;
    self.registry_s.timeout(delay_ms as _, ws::util::Token(echo_ctr)).unwrap();
    if let ws::Message::Binary(bin) = msg {
      self.reg2bot_s.send(BotWsMsg::Bin(bin)).unwrap();
    }
    Ok(())
  }

  fn on_shutdown(&mut self) {
    eprintln!("TRACE: BotWsConn: on_shutdown");
    {
      let mut reconn = self.reconnect.lock();
      reconn.open = false;
    }
    self.watchdog_s.send(WatchdogMsg::_WsHup).unwrap();
    self.reg2bot_s.send(BotWsMsg::Hup).unwrap();
  }

  fn on_close(&mut self, _: ws::CloseCode, _: &str) {
    eprintln!("TRACE: BotWsConn: on_close");
    {
      let mut reconn = self.reconnect.lock();
      reconn.open = false;
    }
    self.watchdog_s.send(WatchdogMsg::_WsHup).unwrap();
    self.reg2bot_s.send(BotWsMsg::Hup).unwrap();
  }

  fn on_error(&mut self, _: ws::Error) {
    eprintln!("TRACE: BotWsConn: on_error");
    {
      let mut reconn = self.reconnect.lock();
      reconn.open = false;
    }
    self.watchdog_s.send(WatchdogMsg::_WsHup).unwrap();
    self.reg2bot_s.send(BotWsMsg::Error).unwrap();
  }

  fn on_timeout(&mut self, token: ws::util::Token) -> ws::Result<()> {
    self.loopback_s.send(LoopbackMsg::_Echo{echo_ctr: token.0}).unwrap();
    Ok(())
  }
}

struct BotWsSender {
  registry_s: ws::Sender,
  secret_token_buf: Option<CryptoBuf>,
}

impl BotWsSender {
  pub fn send_auth<'a, T: Revise<'a> + Serialize>(&mut self, auth: Option<&ApiAuth>, msg: &'a T) -> Maybe {
    if self.secret_token_buf.is_none() {
      //if api_cfg.is_none() {
      if auth.is_none() {
        return Err(fail("API authentication config is required"));
      }
      //let secret_token = api_cfg.as_ref().map(|cfg| cfg.auth.secret_token.as_ref()).unwrap();
      let secret_token = auth.map(|a| a.secret_token.as_ref()).unwrap();
      self.secret_token_buf = base64_str_to_buf(32, secret_token);
      if self.secret_token_buf.is_none() {
        return Err(fail("API authentication config is required"));
      }
    }
    let mut bin: Vec<u8> = Vec::with_capacity(64);
    bin.resize(36, 0_u8);
    assert_eq!(36, bin.len());
    serialize_revision_into(&mut bin, msg).unwrap();
    assert!(36 <= bin.len());
    let msg_bin_len = bin.len() - 36;
    assert!(msg_bin_len <= u32::max_value() as usize);
    Cursor::new(&mut bin[32 .. 36])
      .write_u32::<LittleEndian>(msg_bin_len as u32).unwrap();
    let (sig_buf, payload_buf) = bin.split_at_mut(32);
    auth_sign(
        sig_buf,
        payload_buf,
        self.secret_token_buf.as_ref().unwrap().as_ref(),
    )
      .map_err(|_| fail("API message signing failure"))?;
    self.registry_s.send(bin)
      .map_err(|_| fail("websocket transmission failure"))?;
    Ok(())
  }

  pub fn recv_auth<'a, T: Revise<'a> + Deserialize<'a>>(&mut self, auth: Option<&ApiAuth>, bin: &'a [u8]) -> Maybe<T> {
    if self.secret_token_buf.is_none() {
      if auth.is_none() {
        return Err(fail("API authentication config is required"));
      }
      let secret_token = auth.map(|a| a.secret_token.as_ref()).unwrap();
      self.secret_token_buf = base64_str_to_buf(32, secret_token);
      if self.secret_token_buf.is_none() {
        return Err(fail("API authentication config is required"));
      }
    }
    if bin.len() < 36 {
      return Err(fail("API message protocol failure"));
    }
    auth_verify(
        &bin[0 .. 32],
        &bin[32 .. ],
        self.secret_token_buf.as_ref().unwrap().as_ref(),
    )
      .map_err(|_| fail("API message verification failure"))?;
    let msg_bin_len = Cursor::new(&bin[32 .. 36])
      .read_u32::<LittleEndian>().unwrap() as usize;
    if msg_bin_len != bin[36 .. ].len() {
      return Err(fail("API message self-consistency failure"));
    }
    let msg: T = deserialize_revision(&bin[36 .. ])
      .map_err(|_| fail("API message deserialization failure"))?;
    Ok(msg)
  }
}

enum LoopbackMsg {
  _Echo{
    echo_ctr: usize,
  },
  _Echo2,
  StartCiTask{
    api_key: Vec<u8>,
    ci_run_key: Vec<u8>,
    task_nr: u64,
    task_name: Option<String>,
    taskspec: Option<Vec<u8>>,
  },
  AppendCiTaskData{
    api_key: Vec<u8>,
    ci_run_key: Vec<u8>,
    task_nr: u64,
    part_nr: u64,
    key: String,
    data: Vec<u8>,
  },
  DoneCiTask{
    api_key: Vec<u8>,
    ci_run_key: Vec<u8>,
    task_nr: u64,
    failed: bool,
  },
}

enum WatchdogMsg {
  _WsHup,
}

enum WorkerLbMsg {
  CiTask{
    api_key: Vec<u8>,
    ci_run_key: Vec<u8>,
    task_nr: u64,
    checkout: GitCheckoutSpec,
    task: TaskSpec,
  },
}

enum Event {
  RegisterCiMachine,
  CancelRegisterCiMachine,
  RegisterCiRepo(RegisterCiRepoV0),
  CancelRegisterCiRepo,
}

struct Shared {
  sysroot: Sysroot,
  config: Config,
  root_manifest: RootManifest,
}

struct Reconnect {
  min_backoff_delay_lo: f64,
  min_backoff_delay_hi: f64,
  max_backoff_delay_lo: f64,
  max_backoff_delay_hi: f64,
  open: bool,
  backoff_count: i64,
  backoff_delay_lo: f64,
  backoff_delay_hi: f64,
}

struct Context {
  shared: Arc<RwLock<Shared>>,
  system_setup: SystemSetupV0,
  api_cfg: Option<ApiConfig>,
  machine_cfg: Option<MachineConfigV0>,
  loopback_r: Receiver<LoopbackMsg>,
  loopback_s: Sender<LoopbackMsg>,
  watchdog_r: Receiver<WatchdogMsg>,
  watchdog_s: Sender<WatchdogMsg>,
  workerlb_r: Receiver<WorkerLbMsg>,
  workerlb_s: Sender<WorkerLbMsg>,
  ctlchan_r: Receiver<CtlChannel>,
  ctlchan_s: Sender<CtlChannel>,
  reg2bot_r: Receiver<BotWsMsg>,
  reg2bot_s: Sender<BotWsMsg>,
  reg_conn_join_h: Option<JoinHandle<()>>,
  reg_sender: Option<BotWsSender>,
  reg_echo_ctr: Arc<AtomicUsize>,
  reconnect: Arc<Mutex<Reconnect>>,
  auth_maybe: bool,
  auth: bool,
  machine_reg_maybe: bool,
  machine_reg: bool,
  evbuf: VecDeque<Event>,
}

impl Context {
  pub fn new(git_head_commit: &[u8]) -> Maybe<Context> {
    let args: Vec<_> = env::args().collect();
    let arg0 = args[0].clone();
    let mut prev_arg = None;
    let mut user_arg = false;
    let mut user_prefix_arg = None;
    for arg in args.into_iter() {
      match prev_arg {
        Some("--user-prefix") => {
          user_prefix_arg = Some(PathBuf::from(&arg));
        }
        _ => {}
      }
      prev_arg = None;
      if arg == "--help" || arg == "-h" {
        println!("usage: {} [-h|--help] [-V|--version] [-U|--user] [--user-prefix <USER_PREFIX>]", arg0);
        exit(0);
      } else if arg == "--version" || arg == "-V" {
        println!("guppybot (git: {})", str::from_utf8(git_head_commit).unwrap());
        exit(0);
      } else if arg == "--user" || arg == "-U" {
        user_arg = true;
      } else if arg == "--user-prefix" {
        prev_arg = Some("--user-prefix");
      }
    }
    let sysroot = match user_arg {
      false => Sysroot::default(),
      true  => {
        let base_dir = user_prefix_arg.clone().or_else(|| home_dir())
          .ok_or_else(|| fail("Failed to find user home directory"))?
          .join(".guppybot")
          .join("lib");
        create_dir_all(&base_dir).ok();
        let sock_dir = user_prefix_arg.clone().or_else(|| home_dir())
          .ok_or_else(|| fail("Failed to find user home directory"))?
          .join(".guppybot")
          .join("run");
        create_dir_all(&sock_dir).ok();
        Sysroot{base_dir, sock_dir}
      }
    };
    let config = match user_arg {
      false => Config::default(),
      true  => {
        let config_dir = user_prefix_arg.clone().or_else(|| home_dir())
          .ok_or_else(|| fail("Failed to find user home directory"))?
          .join(".guppybot")
          .join("conf");
        create_dir_all(&config_dir).ok();
        Config{config_dir}
      }
    };
    eprintln!("TRACE: sysroot");
    let root_manifest = RootManifest::load(&sysroot)?;
    eprintln!("TRACE: root manifest");
    let system_setup = SystemSetupV0::query()?;
    eprintln!("TRACE: system setup: {:?}", system_setup);
    let api_cfg = ApiConfig::open(&config).ok();
    eprintln!("TRACE: api cfg: {:?}", api_cfg);
    let machine_cfg = MachineConfigV0::open(&config).ok();
    eprintln!("TRACE: machine cfg: {:?}", machine_cfg);
    let (loopback_s, loopback_r) = unbounded();
    let (watchdog_s, watchdog_r) = unbounded();
    let (workerlb_s, workerlb_r) = unbounded();
    let (ctlchan_s, ctlchan_r) = unbounded();
    let (reg2bot_s, reg2bot_r) = unbounded();
    Ok(Context{
      shared: Arc::new(RwLock::new(Shared{
        sysroot,
        config,
        root_manifest,
      })),
      system_setup,
      api_cfg,
      machine_cfg,
      loopback_r,
      loopback_s,
      watchdog_r,
      watchdog_s,
      workerlb_r,
      workerlb_s,
      ctlchan_r,
      ctlchan_s,
      reg2bot_r,
      reg2bot_s,
      reg_conn_join_h: None,
      reg_sender: None,
      reg_echo_ctr: Arc::new(AtomicUsize::new(0)),
      reconnect: Arc::new(Mutex::new(Reconnect{
        min_backoff_delay_lo: 7.5,
        min_backoff_delay_hi: 15.0,
        max_backoff_delay_lo: 1800.0 - 300.0,
        max_backoff_delay_hi: 1800.0 + 300.0,
        open: false,
        backoff_count: 0,
        backoff_delay_lo: 0.0,
        backoff_delay_hi: 0.0,
      })),
      auth_maybe: false,
      auth: false,
      machine_reg_maybe: false,
      machine_reg: false,
      evbuf: VecDeque::new(),
    })
  }

  fn _init(&mut self, force: bool) -> Maybe<&mut Context> {
    let already_open = {
      let reconn = self.reconnect.lock();
      reconn.open
    };
    if !already_open {
      if self._reconnect_reg().is_none() {
        eprintln!("TRACE: guppybot: init: failed to connect to registry");
        return Ok(self);
      }
    }
    if (force || !self.auth) && self.shared.read().root_manifest.auth_bit() {
      if self._retry_api_auth().is_none() {
        eprintln!("TRACE: guppybot: init: failed to authenticate with registry");
        return Ok(self);
      }
    }
    if (force || !self.machine_reg) && self.shared.read().root_manifest.mach_reg_bit() {
      match self.prepare_register_machine() {
        None => {
          eprintln!("TRACE: guppybot: init: failed to register machine with registry");
          return Ok(self);
        }
        Some((system_setup, machine_cfg)) => {
          if self.finish_register_machine(system_setup, machine_cfg).is_none() {
            eprintln!("TRACE: guppybot: init: failed to register machine with registry");
            return Ok(self);
          }
        }
      }
    }
    Ok(self)
  }

  fn _query_api_auth_config(&mut self) -> Option<QueryApiAuthConfig> {
    self.api_cfg.as_ref()
      .map(|api_cfg| {
        QueryApiAuthConfig{
          api_id: Some(api_cfg.auth.api_key.clone()),
          secret_token: Some(api_cfg.auth.secret_token.clone()),
        }
      })
  }

  fn _dump_api_auth_config(&mut self) -> Option<()> {
    None
  }

  fn _query_api_auth_state(&mut self) -> Option<QueryApiAuthState> {
    Some(QueryApiAuthState{
      auth: self.auth,
      auth_bit: self.shared.read().root_manifest.auth_bit(),
    })
  }

  fn _reconnect_reg(&mut self) -> Option<()> {
    if self.api_cfg.is_none() {
      return None;
    }
    if self.reg_conn_join_h.is_some() {
      eprintln!("TRACE: guppybot: reconnecting to registry");
    }
    let api_cfg = self.api_cfg.as_ref().unwrap();
    let loopback_s = self.loopback_s.clone();
    let watchdog_s = self.watchdog_s.clone();
    let reg2bot_s = self.reg2bot_s.clone();
    let reg_echo_ctr = self.reg_echo_ctr.clone();
    let reconnect = self.reconnect.clone();
    self.reg_conn_join_h = Some(spawn(move || {
      eprintln!("TRACE: guppybot: connecting to registry...");
      match ws::connect("wss://guppybot.org:443/w/v1/", |registry_s| {
        BotWsConn::new(
          loopback_s.clone(),
          watchdog_s.clone(),
          reg2bot_s.clone(),
          reg_echo_ctr.clone(),
          reconnect.clone(),
          registry_s,
        )
      }) {
        Err(_) => {
          eprintln!("TRACE: guppybot: failed to connect to registry");
        }
        Ok(_) => {}
      }
    }));
    select! {
      // FIXME: need timeout case.
      recv(self.reg2bot_r) -> msg => match msg {
        Ok(BotWsMsg::Open(s)) => {
          self.reg_sender = Some(s);
        }
        _ => return None,
      }
    }
    if self.reg_sender.is_none() {
      return None;
    }
    Some(())
  }

  fn _retry_api_auth(&mut self) -> Option<()> {
    self.auth_maybe = false;
    self.auth = false;
    if self.api_cfg.is_none() {
      return None;
    }
    if self.reg_sender.is_none() {
      return None;
    }
    let api_cfg = self.api_cfg.as_ref().unwrap();
    if self.reg_sender.as_mut().unwrap()
      .send_auth(
          self.api_cfg.as_ref().map(|api| &api.auth),
          &Bot2RegistryV0::Auth{
            api_key: match base64_str_to_vec(48, &api_cfg.auth.api_key) {
              None => return None,
              Some(buf) => buf,
            },
          }
      ).is_err()
    {
      return None;
    }
    self.auth_maybe = true;
    Some(())
  }

  fn _undo_api_auth(&mut self) -> Option<()> {
    None
  }

  fn register_ci_machine(&mut self, repo_url: String) -> Option<()> {
    if self.api_cfg.is_none() {
      return None;
    }
    if self.reg_sender.is_none() {
      return None;
    }
    let api_cfg = self.api_cfg.as_ref().unwrap();
    if self.reg_sender.as_mut().unwrap()
      .send_auth(
          self.api_cfg.as_ref().map(|api| &api.auth),
          &Bot2RegistryV0::RegisterCiMachine{
            api_key: match base64_str_to_vec(48, &api_cfg.auth.api_key) {
              None => return None,
              Some(buf) => buf,
            },
            machine_key: self.shared.read().root_manifest.key_buf().as_vec().clone(),
            repo_url: repo_url.clone(),
          }
      ).is_err()
    {
      return None;
    }
    Some(())
  }

  fn register_ci_repo(&mut self, repo_url: String) -> Option<()> {
    if self.api_cfg.is_none() {
      return None;
    }
    if self.reg_sender.is_none() {
      return None;
    }
    let api_cfg = self.api_cfg.as_ref().unwrap();
    if self.reg_sender.as_mut().unwrap()
      .send_auth(
          self.api_cfg.as_ref().map(|api| &api.auth),
          &Bot2RegistryV0::RegisterCiRepo{
            api_key: match base64_str_to_vec(48, &api_cfg.auth.api_key) {
              None => return None,
              Some(buf) => buf,
            },
            group_key: None,
            repo_url: repo_url.clone(),
          }
      ).is_err()
    {
      return None;
    }
    Some(())
  }

  fn prepare_register_machine(&mut self) -> Option<(SystemSetupV0, MachineConfigV0)> {
    if self.machine_cfg.is_none() {
      return None;
    }
    let machine_cfg = self.machine_cfg.clone().unwrap();
    Some((self.system_setup.clone(), machine_cfg))
  }

  fn finish_register_machine(&mut self, system_setup: SystemSetupV0, machine_cfg: MachineConfigV0) -> Option<()> {
    self.machine_reg_maybe = false;
    self.machine_reg = false;
    if self.api_cfg.is_none() {
      return None;
    }
    if self.reg_sender.is_none() {
      return None;
    }
    let api_cfg = self.api_cfg.as_ref().unwrap();
    if self.reg_sender.as_mut().unwrap()
      .send_auth(
          self.api_cfg.as_ref().map(|api| &api.auth),
          &Bot2RegistryV0::RegisterMachine{
            api_key: match base64_str_to_vec(48, &api_cfg.auth.api_key) {
              None => return None,
              Some(buf) => buf,
            },
            machine_key: self.shared.read().root_manifest.key_buf().as_vec().clone(),
            system_setup: system_setup,
            machine_cfg: machine_cfg,
          }
      ).is_err()
    {
      return None;
    }
    self.machine_reg_maybe = true;
    Some(())
  }
}

fn handle_workerlb_ci_task(
    shared: RwLockReadGuard<Shared>,
    loopback_s: &Sender<LoopbackMsg>,
    api_key: Vec<u8>,
    ci_run_key: Vec<u8>,
    task_nr: u64,
    checkout: GitCheckoutSpec,
    task: TaskSpec,
) {
  eprintln!("TRACE: guppybot: worker: ci task: {}", task_nr);
  loopback_s.send(LoopbackMsg::StartCiTask{
    api_key: api_key.clone(),
    ci_run_key: ci_run_key.clone(),
    task_nr,
    task_name: Some(task.name.clone()),
    taskspec: None,
  }).unwrap();
  eprintln!("TRACE: guppybot: worker:   get imagespec...");
  let image = match task.image_candidate() {
    None => {
      loopback_s.send(LoopbackMsg::DoneCiTask{
        api_key: api_key.clone(),
        ci_run_key: ci_run_key.clone(),
        task_nr,
        failed: true,
      }).unwrap();
      return;
    }
    Some(image) => image,
  };
  eprintln!("TRACE: guppybot: worker:   load manifest...");
  let mut image_manifest = match ImageManifest::load(&shared.sysroot, &shared.root_manifest) {
    Err(_) => {
      loopback_s.send(LoopbackMsg::DoneCiTask{
        api_key: api_key.clone(),
        ci_run_key: ci_run_key.clone(),
        task_nr,
        failed: true,
      }).unwrap();
      return;
    }
    Ok(manifest) => manifest,
  };
  eprintln!("TRACE: guppybot: worker:   lookup docker image...");
  let docker_image = match image_manifest.lookup_docker_image(
      &image,
      &shared.sysroot,
      &shared.root_manifest,
  ) {
    Err(_) => {
      loopback_s.send(LoopbackMsg::DoneCiTask{
        api_key: api_key.clone(),
        ci_run_key: ci_run_key.clone(),
        task_nr,
        failed: true,
      }).unwrap();
      return;
    }
    Ok(im) => im,
  };
  eprintln!("TRACE: guppybot: worker:   run...");
  let output = {
    let loopback_s = loopback_s.clone();
    let api_key = api_key.clone();
    let ci_run_key = ci_run_key.clone();
    DockerOutput::Buffer{buf_sz: 512, consumer: Box::new(move |part_nr, data| loopback_s.send(LoopbackMsg::AppendCiTaskData{
      api_key: api_key.clone(),
      ci_run_key: ci_run_key.clone(),
      task_nr,
      part_nr,
      key: "Console".to_string(),
      data,
    }).unwrap())}
  };
  let status = match docker_image.run(&checkout, &task, &shared.sysroot, Some(output)) {
    Err(_) => {
      loopback_s.send(LoopbackMsg::DoneCiTask{
        api_key: api_key.clone(),
        ci_run_key: ci_run_key.clone(),
        task_nr,
        failed: true,
      }).unwrap();
      return;
    }
    Ok(status) => {
      eprintln!("TRACE: guppybot: worker:   status: {:?}", status);
      status
    }
  };
  match status {
    DockerRunStatus::Failure => {
      loopback_s.send(LoopbackMsg::DoneCiTask{
        api_key,
        ci_run_key,
        task_nr,
        failed: true,
      }).unwrap();
    }
    DockerRunStatus::Success => {
      loopback_s.send(LoopbackMsg::DoneCiTask{
        api_key,
        ci_run_key,
        task_nr,
        failed: false,
      }).unwrap();
    }
  }
}

impl Context {
  pub fn runloop(&mut self) -> Maybe {
    let shared = self.shared.clone();
    let loopback_s = self.loopback_s.clone();
    let watchdog_r = self.watchdog_r.clone();
    let reconnect = self.reconnect.clone();
    let watchdog_join_h = spawn(move || {
      loop {
        select! {
          recv(watchdog_r) -> msg => match msg {
            Err(_) => continue,
            Ok(WatchdogMsg::_WsHup) => {
              let delay_s_dist = {
                let mut reconn = reconnect.lock();
                if reconn.open {
                  continue;
                }
                match reconn.backoff_count {
                  0 => {
                    reconn.backoff_delay_lo = reconn.min_backoff_delay_lo;
                    reconn.backoff_delay_hi = reconn.min_backoff_delay_hi;
                  }
                  _ => {
                    reconn.backoff_delay_lo = reconn.max_backoff_delay_lo.min(2.0 * reconn.backoff_delay_lo);
                    reconn.backoff_delay_hi = reconn.max_backoff_delay_hi.min(2.0 * reconn.backoff_delay_hi);
                  }
                }
                reconn.backoff_count += 1;
                Uniform::new_inclusive(reconn.backoff_delay_lo, reconn.backoff_delay_hi)
              };
              let delay_ms = thread_rng().sample(&delay_s_dist) * 1000.0;
              sleep(Duration::from_millis(delay_ms as _));
              let reconn = reconnect.lock();
              if !reconn.open {
                loopback_s.send(LoopbackMsg::_Echo2).unwrap();
              }
            }
          }
        }
      }
    });
    let loopback_s = self.loopback_s.clone();
    let workerlb_r = self.workerlb_r.clone();
    let worker_join_h = spawn(move || {
      let shared = shared;
      let loopback_s = loopback_s;
      loop {
        match workerlb_r.recv() {
          Err(_) => continue,
          Ok(WorkerLbMsg::CiTask{api_key, ci_run_key, task_nr, checkout, task}) => {
            handle_workerlb_ci_task(
                shared.read(),
                &loopback_s,
                api_key, ci_run_key, task_nr, checkout, task,
            );
          }
        }
      }
    });
    let shared = self.shared.clone();
    let ctlchan_s = self.ctlchan_s.clone();
    let ctl_server_join_h = spawn(move || {
      let ctl_server = {
        let shared = shared.read();
        let &Shared{ref sysroot, ..} = &*shared;
        CtlListener::open(sysroot)
      };
      let ctl_server = match ctl_server {
        Err(_) => panic!("failed to open unix socket listener"),
        Ok(server) => server,
      };
      loop {
        match ctl_server.accept() {
          Err(_) => continue,
          Ok(mut chan) => {
            ctlchan_s.send(chan).unwrap();
          }
        }
      }
    });
    loop {
      select! {
        recv(self.loopback_r) -> msg => match msg {
          Err(_) => {}
          Ok(LoopbackMsg::_Echo{echo_ctr}) => {
            if echo_ctr == 0 {
              eprintln!("TRACE: guppybot: warning: got zero-valued echo");
            }
            let reg_echo_ctr = self.reg_echo_ctr.load(Ordering::Relaxed);
            if echo_ctr != reg_echo_ctr {
            } else if echo_ctr == reg_echo_ctr {
              //eprintln!("TRACE: guppybot: ping...");
              if self.reg_sender.is_none() {
                continue;
              }
              if self.reg_sender.as_mut().unwrap()
                .send_auth(
                    self.api_cfg.as_ref().map(|api| &api.auth),
                    &Bot2RegistryV0::_Ping{
                      // FIXME
                      api_key: vec![],
                      machine_key: self.shared.read().root_manifest.key_buf().as_vec().clone(),
                    }
                ).is_err()
              {
                continue;
              }
            } else {
              unreachable!();
            }
          }
          Ok(LoopbackMsg::_Echo2) => {
            eprintln!("TRACE: guppybot: trying to reconnect...");
            self._init(true).ok();
          }
          Ok(LoopbackMsg::StartCiTask{api_key, ci_run_key, task_nr, task_name, taskspec}) => {
            if self.reg_sender.is_none() {
              continue;
            }
            if self.reg_sender.as_mut().unwrap()
              .send_auth(
                  self.api_cfg.as_ref().map(|api| &api.auth),
                  &Bot2RegistryV0::_StartCiTask{
                    api_key,
                    machine_key: self.shared.read().root_manifest.key_buf().as_vec().clone(),
                    ci_run_key,
                    task_nr,
                    task_name,
                    taskspec,
                    ts: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, false)),
                  }
              ).is_err()
            {
              continue;
            }
          }
          Ok(LoopbackMsg::AppendCiTaskData{api_key, ci_run_key, task_nr, part_nr, key, data}) => {
            if self.reg_sender.is_none() {
              continue;
            }
            if self.reg_sender.as_mut().unwrap()
              .send_auth(
                  self.api_cfg.as_ref().map(|api| &api.auth),
                  &Bot2RegistryV0::_AppendCiTaskData{
                    api_key,
                    ci_run_key,
                    task_nr,
                    part_nr,
                    ts: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, false)),
                    key,
                    data,
                  }
              ).is_err()
            {
              continue;
            }
          }
          Ok(LoopbackMsg::DoneCiTask{api_key, ci_run_key, task_nr, failed}) => {
            if self.reg_sender.is_none() {
              continue;
            }
            if self.reg_sender.as_mut().unwrap()
              .send_auth(
                  self.api_cfg.as_ref().map(|api| &api.auth),
                  &Bot2RegistryV0::_DoneCiTask{
                    api_key,
                    ci_run_key,
                    task_nr,
                    failed,
                    ts: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, false)),
                  }
              ).is_err()
            {
              continue;
            }
          }
        },
        recv(self.ctlchan_r) -> chan => match chan {
          Err(_) => {}
          Ok(mut chan) => {
            //eprintln!("TRACE: guppybot: accept ipc conn");
            let recv_msg: Ctl2Bot = match chan.recv() {
              Err(_) => continue,
              Ok(msg) => msg,
            };
            //eprintln!("TRACE:   recv: {:?}", recv_msg);
            let send_msg = match recv_msg {
              Ctl2Bot::_QueryApiAuthConfig => {
                Bot2Ctl::_QueryApiAuthConfig(self._query_api_auth_config())
              }
              Ctl2Bot::_DumpApiAuthConfig{api_id, secret_token} => {
                // FIXME: get rid of unwraps.
                let new_api_cfg = ApiConfig{
                  auth: ApiAuth{
                    api_key: api_id,
                    secret_token,
                  },
                };
                let cfg_path = PathBuf::from("/etc/guppybot/api");
                let mut cfg_file = File::create(&cfg_path).unwrap();
                writeln!(&mut cfg_file, "# automatically generated by guppybot").unwrap();
                writeln!(&mut cfg_file, "").unwrap();
                writeln!(&mut cfg_file, "{}", toml::ser::to_string_pretty(&new_api_cfg).unwrap()).unwrap();
                Bot2Ctl::_DumpApiAuthConfig(Some(()))
              }
              Ctl2Bot::_QueryApiAuthState => {
                Bot2Ctl::_QueryApiAuthState(self._query_api_auth_state())
              }
              Ctl2Bot::_RetryApiAuth => {
                self._reconnect_reg();
                Bot2Ctl::_RetryApiAuth(self._retry_api_auth())
              }
              Ctl2Bot::_AckRetryApiAuth => {
                let ack = match (self.auth_maybe, self.auth) {
                  (true,  true)  => Ack::Done(()),
                  (false, false) |
                  (true,  false) => Ack::Pending,
                  _ => Ack::Stopped,
                };
                Bot2Ctl::_AckRetryApiAuth(ack)
              }
              Ctl2Bot::_UndoApiAuth => {
                Bot2Ctl::_UndoApiAuth(None)
              }
              Ctl2Bot::EchoApiId => {
                Bot2Ctl::EchoApiId(None)
              }
              Ctl2Bot::EchoMachineId => {
                Bot2Ctl::EchoMachineId(None)
              }
              Ctl2Bot::PrintConfig => {
                Bot2Ctl::PrintConfig(None)
              }
              Ctl2Bot::RegisterCiGroupMachine{group_id} => {
                unimplemented!();
              }
              Ctl2Bot::RegisterCiGroupRepo{group_id, repo_url} => {
                unimplemented!();
              }
              Ctl2Bot::RegisterCiMachine{repo_url} => {
                Bot2Ctl::RegisterCiMachine(self.register_ci_machine(repo_url))
              }
              Ctl2Bot::AckRegisterCiMachine => {
                match self.evbuf.pop_front() {
                  Some(Event::RegisterCiMachine) => {
                    Bot2Ctl::AckRegisterCiMachine(Done(()))
                  }
                  Some(Event::CancelRegisterCiMachine) => {
                    Bot2Ctl::AckRegisterCiMachine(Stopped)
                  }
                  Some(e) => {
                    self.evbuf.push_front(e);
                    Bot2Ctl::AckRegisterCiMachine(Pending)
                  }
                  None => {
                    Bot2Ctl::AckRegisterCiMachine(Pending)
                  }
                }
              }
              Ctl2Bot::RegisterCiRepo{repo_url} => {
                Bot2Ctl::RegisterCiRepo(self.register_ci_repo(repo_url))
              }
              Ctl2Bot::AckRegisterCiRepo => {
                match self.evbuf.pop_front() {
                  Some(Event::RegisterCiRepo(rep)) => {
                    Bot2Ctl::AckRegisterCiRepo(Done(RegisterCiRepo{
                      repo_web_url: rep.repo_web_url,
                      webhook_payload_url: rep.webhook_payload_url,
                      webhook_settings_url: rep.webhook_settings_url,
                      webhook_secret: rep.webhook_secret,
                    }))
                  }
                  Some(Event::CancelRegisterCiRepo) => {
                    Bot2Ctl::AckRegisterCiRepo(Stopped)
                  }
                  Some(e) => {
                    self.evbuf.push_front(e);
                    Bot2Ctl::AckRegisterCiRepo(Pending)
                  }
                  None => {
                    Bot2Ctl::AckRegisterCiRepo(Pending)
                  }
                }
              }
              Ctl2Bot::RegisterMachine => {
                Bot2Ctl::RegisterMachine(self.prepare_register_machine())
              }
              Ctl2Bot::ConfirmRegisterMachine{system_setup, machine_cfg} => {
                let rep = self.finish_register_machine(system_setup, machine_cfg);
                Bot2Ctl::ConfirmRegisterMachine(rep)
              }
              Ctl2Bot::AckRegisterMachine => {
                let ack = match (self.machine_reg_maybe, self.machine_reg) {
                  (true,  true)  => Ack::Done(()),
                  (false, false) |
                  (true,  false) => Ack::Pending,
                  _ => Ack::Stopped,
                };
                Bot2Ctl::AckRegisterMachine(ack)
              }
              Ctl2Bot::ReloadConfig => {
                let shared = self.shared.read();
                self.api_cfg = ApiConfig::open(&shared.config).ok();
                self.machine_cfg = MachineConfigV0::open(&shared.config).ok();
                Bot2Ctl::ReloadConfig(Some(()))
              }
              Ctl2Bot::UnregisterCiMachine => {
                Bot2Ctl::UnregisterCiMachine(None)
              }
              Ctl2Bot::UnregisterCiRepo => {
                Bot2Ctl::UnregisterCiRepo(None)
              }
              Ctl2Bot::UnregisterMachine => {
                Bot2Ctl::UnregisterMachine(None)
              }
              _ => {
                eprintln!("TRACE:   unhandled msg case, skipping");
                continue;
              }
            };
            //eprintln!("TRACE:   send: {:?}", send_msg);
            chan.send(&send_msg)?;
            chan.hup();
            //eprintln!("TRACE:   done");
          }
        },
        recv(self.reg2bot_r) -> recv_msg => match recv_msg {
          Ok(BotWsMsg::Bin(bin)) => {
            //eprintln!("TRACE: guppybot: recv ws bin message");
            if self.reg_sender.is_none() {
              continue;
            }
            let api_cfg = self.api_cfg.as_ref().unwrap();
            let msg: Registry2BotV0 = match self.reg_sender.as_mut().unwrap()
              .recv_auth(
                  self.api_cfg.as_ref().map(|api| &api.auth),
                  &bin,
              )
            {
              Err(_) => continue,
              Ok(msg) => msg,
            };
            match msg {
              Registry2BotV0::_Pong => {
                //eprintln!("TRACE: guppybot: pong");
              }
              Registry2BotV0::_NewCiRun{api_key, ci_run_key, repo_clone_url, originator, ref_full, commit_hash, runspec} => {
                let mut api_id = String::new();
                base64::encode_config_buf(
                    &api_key,
                    base64::URL_SAFE,
                    &mut api_id,
                );
                let mut ci_run_id = String::new();
                base64::encode_config_buf(
                    &ci_run_key,
                    base64::URL_SAFE,
                    &mut ci_run_id,
                );
                // FIXME: logging verbosity.
                eprintln!("TRACE: guppybot: new ci run:");
                eprintln!("TRACE: guppybot:   api id: {:?}", api_id);
                eprintln!("TRACE: guppybot:   ci run id: {:?}", ci_run_id);
                eprintln!("TRACE: guppybot:   repo clone url: {:?}", repo_clone_url);
                eprintln!("TRACE: guppybot:   originator: {:?}", originator);
                eprintln!("TRACE: guppybot:   ref full: {:?}", ref_full);
                eprintln!("TRACE: guppybot:   commit hash: {:?}", commit_hash);
                if self.api_cfg.is_none() {
                  continue;
                }
                if self.reg_sender.is_none() {
                  continue;
                }
                // FIXME: if "local_machine.task_workers" is zero, redirect to a
                // remote machine, if one is available, otherwise reject.
                // FIXME: better error handling.
                let shared = self.shared.read();
                let checkout = match GitCheckoutSpec::with_remote_url(repo_clone_url) {
                  Err(_) => {
                    eprintln!("TRACE: guppybot: new ci run: git checkout spec failed");
                    continue;
                  }
                  Ok(x) => x,
                };
                let mut image_manifest = match ImageManifest::load(&shared.sysroot, &shared.root_manifest) {
                  Err(_) => {
                    eprintln!("TRACE: guppybot: new ci run: image manifest load failed");
                    continue;
                  }
                  Ok(x) => x,
                };
                let builtin_imagespec = ImageSpec::builtin_default();
                let builtin_image = match image_manifest.lookup_docker_image(&builtin_imagespec, &shared.sysroot, &shared.root_manifest) {
                  Err(_) => {
                    eprintln!("TRACE: guppybot: new ci run: image lookup failed");
                    continue;
                  }
                  Ok(x) => x,
                };
                match builtin_image._run_checkout(&checkout, &shared.sysroot) {
                  Err(e) => {
                    eprintln!("TRACE: guppybot: new ci run: checkout failed: {:?}", e);
                    continue;
                  }
                  Ok(_) => {}
                }
                let (_spec_out, tasks) = match builtin_image._run_spec(&checkout, &shared.sysroot) {
                  Err(e) => {
                    eprintln!("TRACE: guppybot: new ci run: taskspec failed: {:?}", e);
                    continue;
                  }
                  Ok(x) => x,
                };
                let task_count = tasks.len() as u64;
                eprintln!("TRACE: guppybot: new ci run: confirmed:");
                eprintln!("TRACE: guppybot:   task count: {}", task_count);
                let api_cfg = self.api_cfg.as_ref().unwrap();
                if self.reg_sender.as_mut().unwrap()
                  .send_auth(
                      self.api_cfg.as_ref().map(|api| &api.auth),
                      &Bot2RegistryV0::_NewCiRun(Some(_NewCiRunV0::Accept{
                        //api_key: api_cfg.auth.api_key.clone(),
                        api_key: api_key.clone(),
                        ci_run_key: ci_run_key.clone(),
                        task_count: Some(task_count),
                        failed_early: false,
                        ts: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, false)),
                      }))
                  ).is_err()
                {
                  continue;
                }
                for task_idx in 0 .. task_count {
                  let task_nr = task_idx + 1;
                  assert!(task_nr != 0);
                  self.workerlb_s.send(WorkerLbMsg::CiTask{
                    api_key: api_key.clone(),
                    ci_run_key: ci_run_key.clone(),
                    task_nr,
                    checkout: checkout.clone(),
                    task: tasks[task_idx as usize].clone(),
                  });
                }
              }
              Registry2BotV0::_StartCiTask(Some(_)) => {
              }
              Registry2BotV0::_StartCiTask(None) => {
              }
              Registry2BotV0::_AppendCiTaskData(Some(_)) => {
              }
              Registry2BotV0::_AppendCiTaskData(None) => {
              }
              Registry2BotV0::_DoneCiTask(Some(_)) => {
              }
              Registry2BotV0::_DoneCiTask(None) => {
              }
              Registry2BotV0::Auth(Some(_)) => {
                let mut shared = self.shared.write();
                let &mut Shared{ref sysroot, ref mut root_manifest, ..} = &mut *shared;
                if !self.auth_maybe {
                  self.auth = false;
                  match root_manifest.set_auth_bit(false, sysroot) {
                    Err(_) => continue,
                    Ok(_) => {}
                  }
                  continue;
                }
                if !root_manifest.auth_bit() {
                  match root_manifest.set_auth_bit(true, sysroot) {
                    Err(_) => {
                      self.auth = false;
                      match root_manifest.set_auth_bit(false, sysroot) {
                        Err(_) => continue,
                        Ok(_) => {}
                      }
                      continue;
                    }
                    Ok(_) => {}
                  }
                }
                self.auth = true;
              }
              Registry2BotV0::Auth(None) => {
                let mut shared = self.shared.write();
                let &mut Shared{ref sysroot, ref mut root_manifest, ..} = &mut *shared;
                self.auth_maybe = false;
                self.auth = false;
                match root_manifest.set_auth_bit(false, sysroot) {
                  Err(_) => continue,
                  Ok(_) => {}
                }
              }
              Registry2BotV0::RegisterCiMachine(Some(())) => {
                self.evbuf.push_back(Event::RegisterCiMachine);
              }
              Registry2BotV0::RegisterCiMachine(None) => {
                self.evbuf.push_back(Event::CancelRegisterCiMachine);
              }
              Registry2BotV0::RegisterCiRepo(Some(rep)) => {
                self.evbuf.push_back(Event::RegisterCiRepo(rep));
              }
              Registry2BotV0::RegisterCiRepo(None) => {
                self.evbuf.push_back(Event::CancelRegisterCiRepo);
              }
              Registry2BotV0::RegisterMachine(Some(_)) => {
                let mut shared = self.shared.write();
                let &mut Shared{ref sysroot, ref mut root_manifest, ..} = &mut *shared;
                if !self.machine_reg_maybe {
                  self.machine_reg = false;
                  match root_manifest.set_mach_reg_bit(false, sysroot) {
                    Err(_) => continue,
                    Ok(_) => {}
                  }
                  continue;
                }
                if !root_manifest.mach_reg_bit() {
                  match root_manifest.set_mach_reg_bit(true, sysroot) {
                    Err(_) => {
                      self.machine_reg = false;
                      match root_manifest.set_mach_reg_bit(false, sysroot) {
                        Err(_) => continue,
                        Ok(_) => {}
                      }
                      continue;
                    }
                    Ok(_) => {}
                  }
                }
                self.machine_reg = true;
              }
              Registry2BotV0::RegisterMachine(None) => {
                let mut shared = self.shared.write();
                let &mut Shared{ref sysroot, ref mut root_manifest, ..} = &mut *shared;
                self.machine_reg_maybe = false;
                self.machine_reg = false;
                match root_manifest.set_mach_reg_bit(false, sysroot) {
                  Err(_) => continue,
                  Ok(_) => {}
                }
              }
              _ => {}
            }
          }
          Ok(BotWsMsg::Hup) | Ok(BotWsMsg::Error) => {
            // FIXME: try to reconnect/reauth.
            if let Some(h) = self.reg_conn_join_h.take() {
              h.join().ok();
            }
          }
          _ => {}
        }
      }
    }
    watchdog_join_h.join().ok();
    worker_join_h.join().ok();
    ctl_server_join_h.join().ok();
    if let Some(h) = self.reg_conn_join_h.take() {
      h.join().ok();
    }
    Ok(())
  }
}
