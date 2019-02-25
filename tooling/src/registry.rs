use crate::query::{Maybe, fail};

use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use crossbeam_channel::{Sender, Receiver, unbounded};
use minisodium::{auth_sign, auth_verify};
use minisodium::util::{CryptoBuf};
use serde::{Serialize};
use serde::de::{DeserializeOwned};

use std::io::{Cursor};
use std::thread::{JoinHandle, spawn};

pub enum Chan2Raw {
}

pub enum Raw2Chan {
  Registry(ws::Sender),
  SignedBin(Vec<u8>),
}

pub struct RawWsConn {
  chan2raw_r: Receiver<Chan2Raw>,
  raw2chan_s: Sender<Raw2Chan>,
  registry_s: ws::Sender,
}

impl RawWsConn {
  pub fn new(chan2raw_r: Receiver<Chan2Raw>, raw2chan_s: Sender<Raw2Chan>, registry_s: ws::Sender) -> RawWsConn {
    // TODO
    raw2chan_s.send(Raw2Chan::Registry(registry_s.clone())).unwrap();
    RawWsConn{
      chan2raw_r,
      raw2chan_s,
      registry_s,
    }
  }
}

impl ws::Handler for RawWsConn {
  fn on_shutdown(&mut self) {
    // TODO
    eprintln!("TRACE: RawWsConn: on_shutdown");
  }

  fn on_open(&mut self, _: ws::Handshake) -> ws::Result<()> {
    // TODO
    Ok(())
  }

  fn on_message(&mut self, msg: ws::Message) -> ws::Result<()> {
    if let ws::Message::Binary(bin) = msg {
      // TODO
      self.raw2chan_s.send(Raw2Chan::SignedBin(bin)).unwrap();
    }
    Ok(())
  }

  fn on_close(&mut self, _: ws::CloseCode, _: &str) {
    // TODO
    eprintln!("TRACE: RawWsConn: on_close");
  }

  fn on_error(&mut self, _: ws::Error) {
    // TODO
    eprintln!("TRACE: RawWsConn: on_error");
  }

  fn on_timeout(&mut self, _: ws::util::Token) -> ws::Result<()> {
    // TODO
    Ok(())
  }
}

pub struct RegistryChannel {
  secret_token_buf: CryptoBuf,
  chan2raw_s: Sender<Chan2Raw>,
  raw2chan_r: Receiver<Raw2Chan>,
  registry_s: ws::Sender,
  join_h: JoinHandle<()>,
}

impl RegistryChannel {
  // TODO: open this with api authentication.
  //pub fn open_default() -> Maybe<RegistryChannel> {
  pub fn open(secret_token_buf: CryptoBuf) -> Maybe<RegistryChannel> {
    let (chan2raw_s, chan2raw_r) = unbounded();
    let (raw2chan_s, raw2chan_r) = unbounded();
    let join_h = spawn(move || {
      match ws::connect("wss://guppybot.org:443/w/", |registry_s| {
        RawWsConn::new(
            chan2raw_r.clone(),
            raw2chan_s.clone(),
            registry_s,
        )
      }) {
        Err(_) => {
          // TODO
          eprintln!("failed to connect to guppybot.org");
        }
        Ok(_) => {}
      }
    });
    match raw2chan_r.recv() {
      Ok(Raw2Chan::Registry(registry_s)) => {
        Ok(RegistryChannel{
          secret_token_buf,
          chan2raw_s,
          raw2chan_r,
          registry_s,
          join_h,
        })
      }
      Ok(_) | Err(_) => Err(fail("internal channel error")),
    }
  }

  pub fn send<T: Serialize>(&mut self, msg: &T) -> Maybe {
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
    auth_sign(sig_buf, payload_buf, self.secret_token_buf.as_ref())
      .map_err(|_| fail("API message signing failure"))?;
    self.registry_s.send(bin)
      .map_err(|_| fail("websocket transmission failure"))?;
    Ok(())
  }

  pub fn recv<T: DeserializeOwned>(&mut self) -> Maybe<T> {
    match self.raw2chan_r.recv() {
      Ok(Raw2Chan::SignedBin(bin)) => {
        if bin.len() < 36 {
          return Err(fail("API message protocol failure"));
        }
        if auth_verify(&bin[0 .. 32], &bin[32 .. ], self.secret_token_buf.as_ref())
            .is_err()
        {
          return Err(fail("API message verification failure"));
        }
        let msg_bin_len = Cursor::new(&bin[32 .. 36])
          .read_u32::<LittleEndian>().unwrap() as usize;
        if msg_bin_len != bin[36 .. ].len() {
          return Err(fail("API message self-consistency failure"));
        }
        let msg: T = bincode::deserialize_from(Cursor::new(&bin[36 .. ]))
          .map_err(|_| fail("API message deserialization failure"))?;
        Ok(msg)
      }
      Ok(_) | Err(_) => Err(fail("internal channel error")),
    }
  }

  pub fn hup(self) {
  }
}
