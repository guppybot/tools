extern crate guppyctl;

static GUPPYBOT_BIN: &'static [u8] = include_bytes!("../guppybot");

fn main() {
  guppyctl::cli::dispatch(GUPPYBOT_BIN);
}
