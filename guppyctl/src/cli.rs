use crate::*;

use clap::{App, Arg, ArgMatches, SubCommand};
use tooling::ipc::{CtlChannel};
use tooling::state::{ImageManifest, Sysroot};

use std::env::{current_dir};
use std::fs::{File, create_dir_all};
use std::io::{Write};
use std::path::{PathBuf};

pub fn dispatch(guppybot_bin: &[u8]) {
  let mut app = App::new("guppyctl")
    .version("beta")
    .subcommand(SubCommand::with_name("run")
      .about("Run a local gup.py script in a local working directory")
      .arg(Arg::with_name("FILE")
        .short("f")
        .long("file")
        .takes_value(true)
        .help("Alternative path to local script file. The default path\nis '<WORKING_DIR>/gup.py'.")
      )
      .arg(Arg::with_name("MUTABLE")
        .short("m")
        .long("mut")
        .takes_value(false)
        .help("Make the local working directory mutable, allowing the\ngup.py script to modify the host filesystem.")
      )
      .arg(Arg::with_name("WORKING_DIR")
        .index(1)
        .required(false)
        .help("The local working directory. If not provided, the default\nis the current directory.")
      )
    )
    /*.subcommand(SubCommand::with_name("x-check-deps")
      .about("Experimental. Check if dependencies are correctly installed")
    )
    .subcommand(SubCommand::with_name("x-install-deps")
      .about("Experimental. Install dependencies")
    )*/
  ;
  match app.clone().get_matches().subcommand() {
    ("run", Some(matches)) => {
      let gup_py_path = PathBuf::from(matches.value_of("FILE")
        .unwrap_or_else(|| "gup.py"));
      let mutable = matches.is_present("MUTABLE");
      let working_dir = matches.value_of("WORKING_DIR")
        .map(|s| PathBuf::from(s))
        .or_else(|| current_dir().ok());
      match run(mutable, gup_py_path, working_dir) {
        Err(e) => panic!("{:?}", e),
        Ok(_) => {}
      }
    }
    /*("x-check-deps", Some(matches)) => {
      unimplemented!();
    }
    ("x-install-deps", Some(matches)) => {
      unimplemented!();
    }*/
    _ => {
      app.print_help().unwrap();
      println!();
    }
  }
}
