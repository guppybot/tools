extern crate git_version;
extern crate guppybot;

fn main() {
  guppybot::run_main(git_version::GIT_HEAD_COMMIT_HASH);
}
