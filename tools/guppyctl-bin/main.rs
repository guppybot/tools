extern crate git_version;
extern crate guppyctl;

static GUPPYBOT_BIN: &'static [u8] = include_bytes!("../../build/guppybot");

fn main() {
  guppyctl::run_main(git_version::GIT_HEAD_COMMIT_HASH, GUPPYBOT_BIN);
}
