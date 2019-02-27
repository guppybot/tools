pub use self::{Ack::*};

use crate::query::{Maybe, fail};

use byteorder::{ReadBytesExt, WriteBytesExt, NativeEndian};
use schemas::v1::{MachineConfigV0};
use serde::{Deserialize, Serialize};

use std::fs;
use std::io::{Read, Write, Cursor};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{PathBuf};

#[derive(Serialize, Deserialize, Debug)]
pub enum Ack<T> {
  Done(T),
  Pending,
  Stopped,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Ctl2Bot {
  _QueryApiAuthConfig,
  _DumpApiAuthConfig{
    api_id: String,
    secret_token: String,
  },
  _RetryApiAuth,
  _AckRetryApiAuth,
  _UndoApiAuth,
  EchoApiId,
  EchoMachineId,
  PrintConfig,
  RegisterCiMachine{
    repo_url: String,
  },
  RegisterCiRepo{
    repo_url: String,
  },
  RegisterMachine,
  ReloadConfig,
  UnregisterCiMachine,
  UnregisterCiRepo,
  UnregisterMachine,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Bot2Ctl {
  _QueryApiAuthConfig(Option<QueryApiAuthConfig>),
  _DumpApiAuthConfig(Option<()>),
  _RetryApiAuth(Option<()>),
  _AckRetryApiAuth(Ack<()>),
  _UndoApiAuth(Option<()>),
  EchoApiId(Option<EchoApiId>),
  EchoMachineId(Option<EchoMachineId>),
  PrintConfig(Option<PrintConfig>),
  RegisterCiMachine(Option<RegisterCiMachine>),
  AckRegisterCiMachine(Ack<RegisterCiMachine>),
  RegisterCiRepo(Option<RegisterCiRepo>),
  AckRegisterCiRepo(Ack<RegisterCiRepo>),
  RegisterMachine(Option<()>),
  AckRegisterMachine(Ack<()>),
  ReloadConfig(Option<()>),
  UnregisterCiMachine(Option<()>),
  UnregisterCiRepo(Option<()>),
  UnregisterMachine(Option<()>),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryApiAuthConfig {
  pub api_id: Option<String>,
  pub secret_token: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EchoApiId {
  pub api_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EchoMachineId {
  pub machine_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PrintConfig {
  pub api_id: String,
  pub machine_cfg: MachineConfigV0,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterCiMachine {
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterCiRepo {
  pub repo_url: String,
  pub webhook_payload_url: String,
  pub webhook_secret: String,
  pub webhook_settings_url: String,
}

/*#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterMachine {
}*/

/*#[derive(Serialize, Deserialize, Debug)]
pub struct ReloadConfig {
  pub api_id: String,
  //pub secret_token: String,
  pub machine_cfg: MachineConfigV0,
}*/

pub struct CtlListener {
  inner: UnixListener,
}

impl CtlListener {
  //pub fn open(socket_path: &PathBuf) -> Maybe<CommChannel> {
  pub fn open_default() -> Maybe<CtlListener> {
    let socket_path = PathBuf::from("/var/run/guppybot.sock");
    let inner = UnixListener::bind(&socket_path)
      .or_else(|_| {
        fs::remove_file(&socket_path).ok();
        UnixListener::bind(&socket_path)
      })
      .map_err(|_| fail("Unable to serve the guppybot daemon"))?;
    Ok(CtlListener{inner})
  }

  pub fn accept(&self) -> Maybe<CtlChannel> {
    let (stream, _) = match self.inner.accept() {
      Err(_) => return Err(fail("Unable to accept connections to the guppybot daemon")),
      Ok(stream) => stream,
    };
    let mut buf = Vec::with_capacity(4096);
    for _ in 0 .. 4096 {
      buf.push(0);
    }
    let chan = CtlChannel{buf, inner: stream};
    Ok(chan)
  }
}

pub struct CtlChannel {
  buf: Vec<u8>,
  inner: UnixStream,
}

impl CtlChannel {
  pub fn open_default() -> Maybe<CtlChannel> {
    let socket_path = PathBuf::from("/var/run/guppybot.sock");
    let mut buf = Vec::with_capacity(4096);
    for _ in 0 .. 4096 {
      buf.push(0);
    }
    let inner = UnixStream::connect(&socket_path)
      .map_err(|_| fail("Unable to connect to the guppybot daemon"))?;
    Ok(CtlChannel{buf, inner})
  }

  pub fn send<T: Serialize>(&mut self, msg: &T) -> Maybe {
    let msg_len = {
      let mut cursor = Cursor::new(&mut self.buf as &mut [u8]);
      assert_eq!(0, cursor.position());
      match bincode::serialize_into(&mut cursor, msg) {
        Err(_) => return Err(fail("unix socket: serialize error")),
        Ok(_) => {}
      }
      cursor.position()
    };
    if msg_len > 4092 {
      return Err(fail(format!("unix socket: oversized packet ({})", msg_len + 4)));
    }
    match self.inner.write_u32::<NativeEndian>(msg_len as u32) {
      Err(_) => return Err(fail("unix socket: write error")),
      Ok(_) => {}
    }
    match self.inner.write_all(&self.buf[ .. msg_len as usize]) {
      Err(_) => return Err(fail("unix socket: write error")),
      Ok(_) => {}
    }
    Ok(())
  }

  pub fn recv<'a, T: Deserialize<'a> + 'static>(&'a mut self) -> Maybe<T> {
    let msg_len = match self.inner.read_u32::<NativeEndian>() {
      Err(_) => return Err(fail("unix socket: read error")),
      Ok(x) => x,
    };
    if msg_len > 4092 {
      return Err(fail(format!("unix socket: oversized packet ({})", msg_len + 4)));
    }
    match self.inner.read_exact(&mut self.buf[ .. msg_len as usize]) {
      Err(_) => return Err(fail("unix socket: read error")),
      Ok(_) => {}
    }
    let msg = match bincode::deserialize(&self.buf[ .. msg_len as usize]) {
      Err(_) => return Err(fail("unix socket: deserialize error")),
      Ok(x) => x,
    };
    Ok(msg)
  }

  pub fn hup(self) {
  }
}
