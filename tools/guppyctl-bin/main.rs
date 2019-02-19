extern crate guppyctl;

static GUPPYBOT_BIN: &'static [u8] = include_bytes!("../../build/guppybot");

fn main() {
  guppyctl::run_main(GUPPYBOT_BIN);
}
