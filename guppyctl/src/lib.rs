extern crate clap;
extern crate crossbeam_utils;
extern crate curl;
extern crate monosodium;
extern crate schemas;
extern crate semver;
extern crate serde_json;
extern crate tempfile;
extern crate tooling;
//extern crate url;

pub(crate) mod cli;

pub fn run_main(guppybot_bin: &[u8]) -> ! {
  monosodium::init_sodium();
  cli::_dispatch(guppybot_bin)
}
