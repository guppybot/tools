extern crate base64;
extern crate bincode;
extern crate byteorder;
extern crate chrono;
#[macro_use] extern crate crossbeam_channel;
extern crate ctrlc;
extern crate dirs;
extern crate monosodium;
extern crate parking_lot;
extern crate rand;
extern crate schemas;
extern crate toml;
extern crate tooling;
extern crate ws;

use std::process::{exit};

pub mod daemon;

pub fn run_main(git_head_commit: &[u8]) -> ! {
  monosodium::init_sodium();
  let code = match daemon::runloop(git_head_commit) {
    Err(_) => 1,
    Ok(_) => 0,
  };
  exit(code)
}
