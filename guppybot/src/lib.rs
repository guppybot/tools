extern crate ctrlc;
extern crate schemas;
extern crate tooling;

use std::process::{exit};

pub mod daemon;

pub fn run_main() -> ! {
  let code = match daemon::runloop() {
    Err(_) => 1,
    Ok(_) => 0,
  };
  exit(code)
}
