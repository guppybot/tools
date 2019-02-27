use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use crossbeam_channel::{Sender, Receiver, unbounded};
use minisodium::{auth_sign, auth_verify};
use minisodium::util::{CryptoBuf};
use schemas::v1::{DistroInfoV0, GpusV0, MachineConfigV0, SystemSetupV0, Bot2RegistryV0, Registry2BotV0};
use serde::{Serialize};
use serde::de::{DeserializeOwned};
use tooling::config::{ApiConfig, ApiAuth};
use tooling::ipc::*;
use tooling::query::{Maybe, Query, fail};
use tooling::state::{RootManifest, Sysroot};

use std::fs::{File};
use std::io::{Read, Write, Cursor};
use std::path::{PathBuf};
use std::thread::{JoinHandle, spawn};

pub fn runloop() -> Maybe {
  Context::new()?.runloop()
}

fn base64_str_to_buf(len_bytes: usize, b64_str: &str) -> Option<CryptoBuf> {
  let mut buf = Vec::with_capacity(len_bytes);
  if base64::decode_config_buf(
      b64_str,
      base64::URL_SAFE,
      &mut buf,
  ).is_err() {
    return None;
  }
  //Some(buf)
  Some(CryptoBuf::from_vec(len_bytes, buf))
}

enum BotWsMsg {
  Open(BotWsSender),
  Bin(Vec<u8>),
  Hup,
}

struct BotWsConn {
  reg2bot_s: Sender<BotWsMsg>,
  registry_s: ws::Sender,
}

impl ws::Handler for BotWsConn {
  fn on_shutdown(&mut self) {
    // TODO
    eprintln!("TRACE: BotWsConn: on_shutdown");
    self.reg2bot_s.send(BotWsMsg::Hup).unwrap();
  }

  fn on_open(&mut self, _: ws::Handshake) -> ws::Result<()> {
    self.reg2bot_s.send(BotWsMsg::Open(BotWsSender{
      registry_s: self.registry_s.clone(),
      secret_token_buf: None,
    })).unwrap();
    Ok(())
  }

  fn on_message(&mut self, msg: ws::Message) -> ws::Result<()> {
    if let ws::Message::Binary(bin) = msg {
      self.reg2bot_s.send(BotWsMsg::Bin(bin)).unwrap();
    }
    Ok(())
  }

  fn on_close(&mut self, _: ws::CloseCode, _: &str) {
    // TODO
    eprintln!("TRACE: BotWsConn: on_close");
    self.reg2bot_s.send(BotWsMsg::Hup).unwrap();
  }

  fn on_error(&mut self, _: ws::Error) {
    // TODO
    eprintln!("TRACE: BotWsConn: on_error");
  }

  fn on_timeout(&mut self, _: ws::util::Token) -> ws::Result<()> {
    // TODO
    Ok(())
  }
}

struct BotWsSender {
  registry_s: ws::Sender,
  secret_token_buf: Option<CryptoBuf>,
}

impl BotWsSender {
  pub fn send_auth<T: Serialize>(&mut self, auth: Option<&ApiAuth>, msg: &T) -> Maybe {
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
    bin.resize(32, 0_u8);
    bin.write_u32::<LittleEndian>(0).unwrap();
    assert_eq!(36, bin.len());
    bincode::serialize_into(&mut bin, msg).unwrap();
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

  pub fn recv_auth<T: DeserializeOwned>(&mut self, auth: Option<&ApiAuth>, bin: &[u8]) -> Maybe<T> {
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
    let msg: T = bincode::deserialize_from(Cursor::new(&bin[36 .. ]))
      .map_err(|_| fail("API message deserialization failure"))?;
    Ok(msg)
  }
}

pub struct Context {
  system_setup: SystemSetupV0,
  sysroot: Sysroot,
  root_manifest: RootManifest,
  api_cfg: Option<ApiConfig>,
  machine_cfg: Option<MachineConfigV0>,
  ctlchan_r: Receiver<CtlChannel>,
  ctlchan_s: Sender<CtlChannel>,
  reg2bot_r: Receiver<BotWsMsg>,
  reg2bot_s: Sender<BotWsMsg>,
  reg_conn_join_h: Option<JoinHandle<()>>,
  reg_sender: Option<BotWsSender>,
  auth_maybe: bool,
  auth_lock: Option<File>,
}

impl Context {
  pub fn new() -> Maybe<Context> {
    let system_setup = SystemSetupV0::query()?;
    eprintln!("TRACE: system setup: {:?}", system_setup);
    let sysroot = Sysroot::default();
    eprintln!("TRACE: sysroot");
    let root_manifest = RootManifest::load(&sysroot)?;
    eprintln!("TRACE: root manifest");
    let api_cfg = ApiConfig::open_default().ok();
    eprintln!("TRACE: api cfg: {:?}", api_cfg);
    let machine_cfg = MachineConfigV0::query().ok();
    eprintln!("TRACE: machine cfg: {:?}", machine_cfg);
    let (ctlchan_s, ctlchan_r) = unbounded();
    let (reg2bot_s, reg2bot_r) = unbounded();
    Ok(Context{
      system_setup,
      sysroot,
      root_manifest,
      api_cfg,
      machine_cfg,
      ctlchan_r,
      ctlchan_s,
      reg2bot_r,
      reg2bot_s,
      reg_conn_join_h: None,
      reg_sender: None,
      auth_maybe: false,
      auth_lock: None,
    })
  }
}

impl Context {
  fn _query_api_auth_config(&mut self) -> Option<QueryApiAuthConfig> {
    self.api_cfg.as_ref()
      .map(|api_cfg| {
        QueryApiAuthConfig{
          api_id: Some(api_cfg.auth.api_id.clone()),
          secret_token: Some(api_cfg.auth.secret_token.clone()),
        }
      })
  }

  fn _dump_api_auth_config(&mut self) -> Option<()> {
    // TODO
    None
  }

  fn _reconnect_ws(&mut self) -> Option<()> {
    if self.api_cfg.is_none() {
      return None;
    }
    if self.reg_conn_join_h.is_some() {
      eprintln!("warning: reauthenticating on an existing connection");
    }
    let api_cfg = self.api_cfg.as_ref().unwrap();
    let reg2bot_s = self.reg2bot_s.clone();
    self.reg_conn_join_h = Some(spawn(move || {
      match ws::connect("wss://guppybot.org:443/w/", |registry_s| {
        BotWsConn{
          reg2bot_s: reg2bot_s.clone(),
          registry_s,
        }
      }) {
        Err(_) => {
          eprintln!("Failed to connect to guppybot.org");
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
    self.auth_lock = None;
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
            api_id: api_cfg.auth.api_id.clone(),
          }
      ).is_err()
    {
      return None;
    }
    self.auth_maybe = true;
    Some(())
  }

  fn _undo_api_auth(&mut self) -> Option<()> {
    // TODO
    None
  }

  fn register_ci_machine(&mut self) -> Option<()> {
    // TODO
    None
  }

  fn register_ci_repo(&mut self, repo_url: String) -> Option<RegisterCiRepo> {
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
            api_id: api_cfg.auth.api_id.clone(),
            repo_url: repo_url.clone(),
          }
      ).is_err()
    {
      return None;
    }
    // FIXME: record this as a pending request.
    let settings_url = format!("{}/settings/hooks", repo_url);
    Some(RegisterCiRepo{
      repo_url,
      webhook_payload_url: "https://guppybot.org/x/github/longshot".to_string(),
      webhook_secret: "AAAEEEIIIOOOUUU".to_string(),
      webhook_settings_url: settings_url,
    })
  }

  fn register_machine(&mut self) -> Option<()> {
    // TODO
    if self.api_cfg.is_none() {
      return None;
    }
    if self.machine_cfg.is_none() {
      return None;
    }
    if self.reg_sender.is_none() {
      return None;
    }
    let api_cfg = self.api_cfg.as_ref().unwrap();
    let machine_cfg = self.machine_cfg.clone().unwrap();
    if self.reg_sender.as_mut().unwrap()
      .send_auth(
          self.api_cfg.as_ref().map(|api| &api.auth),
          &Bot2RegistryV0::RegisterMachine{
            api_id: api_cfg.auth.api_id.clone(),
            machine_cfg,
            root_manifest_id: self.root_manifest.key_as_base64(),
            system_setup: self.system_setup.clone(),
          }
      ).is_err()
    {
      return None;
    }
    Some(())
  }

  pub fn runloop(&mut self) -> Maybe {
    let ctlchan_s = self.ctlchan_s.clone();
    let ctl_server_join_h = spawn(move || {
      let ctl_server = match CtlListener::open_default() {
        Err(_) => panic!("failed to open unix socket listener"),
        Ok(server) => server,
      };
      loop {
        match ctl_server.accept() {
          Err(_) => continue,
          Ok(mut chan) => {
            // TODO
            ctlchan_s.send(chan).unwrap();
          }
        }
      }
    });
    let mut reg_conn_join_h: Option<JoinHandle<()>> = None;
    let mut reg_sender: Option<BotWsSender> = None;
    loop {
      select! {
        recv(self.ctlchan_r) -> chan => match chan {
          Err(_) => {}
          Ok(mut chan) => {
            // TODO
            eprintln!("TRACE: guppybot: accept ipc conn");
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
                    api_id,
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
              Ctl2Bot::_RetryApiAuth => {
                self._reconnect_ws();
                Bot2Ctl::_RetryApiAuth(self._retry_api_auth())
              }
              Ctl2Bot::_AckRetryApiAuth => {
                let ack = match (self.auth_maybe, self.auth_lock.is_some()) {
                  (true,  true)  => Ack::Done(()),
                  (false, false) |
                  (true,  false) => Ack::Pending,
                  _ => Ack::Stopped,
                };
                Bot2Ctl::_AckRetryApiAuth(ack)
              }
              Ctl2Bot::_UndoApiAuth => {
                // TODO
                Bot2Ctl::_UndoApiAuth(None)
              }
              Ctl2Bot::EchoApiId => {
                // TODO
                Bot2Ctl::EchoApiId(None)
              }
              Ctl2Bot::EchoMachineId => {
                // TODO
                Bot2Ctl::EchoMachineId(None)
              }
              Ctl2Bot::PrintConfig => {
                // TODO
                Bot2Ctl::PrintConfig(None)
              }
              Ctl2Bot::RegisterCiMachine{repo_url} => {
                // TODO
                Bot2Ctl::RegisterCiMachine(None)
              }
              Ctl2Bot::RegisterCiRepo{repo_url} => {
                Bot2Ctl::RegisterCiRepo(self.register_ci_repo(repo_url))
              }
              Ctl2Bot::RegisterMachine => {
                // TODO
                Bot2Ctl::RegisterMachine(self.register_machine())
              }
              Ctl2Bot::ReloadConfig => {
                // TODO
                self.api_cfg = ApiConfig::open_default().ok();
                self.machine_cfg = MachineConfigV0::query().ok();
                Bot2Ctl::ReloadConfig(Some(()))
              }
              Ctl2Bot::UnregisterCiMachine => {
                // TODO
                Bot2Ctl::UnregisterCiMachine(None)
              }
              Ctl2Bot::UnregisterCiRepo => {
                // TODO
                Bot2Ctl::UnregisterCiRepo(None)
              }
              Ctl2Bot::UnregisterMachine => {
                // TODO
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
            eprintln!("TRACE: guppybot: recv ws bin message");
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
              Registry2BotV0::Auth(Some(_)) => {
                if !self.auth_maybe {
                  continue;
                }
                let mut lock_f = match File::create(self.sysroot.base_dir.join(".auth.lock")) {
                  Err(_) => {
                    // Cleanup auth state.
                    self.auth_maybe = false;
                    self.auth_lock = None;
                    continue;
                  }
                  Ok(f) => f,
                };
                self.auth_lock = Some(lock_f);
              }
              Registry2BotV0::Auth(None) => {
                // Cleanup auth state.
                self.auth_maybe = false;
                self.auth_lock = None;
              }
              _ => {}
            }
          }
          Ok(BotWsMsg::Hup) => {
            // TODO
          }
          _ => {}
        }
      }
    }
    ctl_server_join_h.join().ok();
    if let Some(h) = reg_conn_join_h {
      h.join().ok();
    }
    Ok(())
  }
}
