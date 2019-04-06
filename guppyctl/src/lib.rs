extern crate clap;
extern crate crossbeam_utils;
//extern crate curl;
extern crate dirs;
extern crate monosodium;
extern crate schemas;
extern crate semver;
extern crate serde_json;
extern crate tempfile;
extern crate tooling;
//extern crate url;

pub(crate) mod cli;

pub fn run_main(git_head_commit: &[u8], guppybot_bin: &[u8]) -> ! {
  monosodium::init_sodium();
  cli::_dispatch(git_head_commit, guppybot_bin)
}
