extern crate base64;
extern crate bincode;
extern crate byteorder;
#[macro_use] extern crate crossbeam_channel;
extern crate ctrlc;
extern crate monosodium;
extern crate schemas;
extern crate toml;
extern crate tooling;
extern crate ws;

use std::process::{exit};

pub mod daemon;

pub fn run_main() -> ! {
  let code = match daemon::runloop() {
    Err(_) => 1,
    Ok(_) => 0,
  };
  exit(code)
}
