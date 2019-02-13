use crate::query::{Maybe, fail};

use byteorder::{ReadBytesExt, WriteBytesExt, NativeEndian};
use serde::{Deserialize, Serialize};

use std::fs;
use std::io::{Read, Write, Cursor};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::{PathBuf};

pub struct CtlListener {
  buf: Vec<u8>,
  inner: UnixListener,
}

impl CtlListener {
  //pub fn open(socket_path: &PathBuf) -> Maybe<CommChannel> {
  pub fn open_default() -> Maybe<CtlListener> {
    let socket_path = PathBuf::from("/var/lib/guppybot/.sock");
    let mut buf = Vec::with_capacity(4096);
    for _ in 0 .. 4096 {
      buf.push(0);
    }
    let inner = UnixListener::bind(&socket_path)
      .or_else(|_| {
        fs::remove_file(&socket_path).ok();
        UnixListener::bind(&socket_path)
      })
      .map_err(|_| fail("failed to connect to unix socket"))?;
    Ok(CtlListener{buf, inner})
  }

  pub fn accept(&self) -> Maybe<CtlChannel> {
    let (stream, _) = match self.inner.accept() {
      Err(_) => return Err(fail("unix socket: failed to accept connection")),
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
    let socket_path = PathBuf::from("/var/lib/guppybot/.sock");
    let mut buf = Vec::with_capacity(4096);
    for _ in 0 .. 4096 {
      buf.push(0);
    }
    let inner = UnixStream::connect(&socket_path)
      .map_err(|_| fail("failed to connect to unix socket"))?;
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
}
