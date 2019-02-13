use crate::*;

use clap::{App, Arg, ArgMatches, SubCommand};

use std::env::{current_dir};
use std::fs::{File, create_dir_all};
use std::io::{Write};
use std::path::{PathBuf};
use std::process::{exit};

pub fn dispatch(guppybot_bin: &[u8]) -> ! {
  let mut app = App::new("guppyctl")
    .version("beta")
    .subcommand(SubCommand::with_name("install-self")
      .about("Install guppybot")
      .arg(Arg::with_name("DEBUG_ALT_SYSROOT")
        .long("debug-alt-sysroot")
        .takes_value(true)
        .help("Debug option: alternative sysroot path. The default sysroot\npath is '/var/lib/guppybot'.")
      )
    )
    .subcommand(SubCommand::with_name("print-config")
      .about("Print the currently loaded configuration")
    )
    /*.subcommand(SubCommand::with_name("reload-config")
      .about("Reload configuration")
    )*/
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
    )*/
    .subcommand(SubCommand::with_name("x-install-deps")
      .about("Experimental. Install dependencies")
    )
  ;
  let code = match app.clone().get_matches().subcommand() {
    ("install-self", Some(matches)) => {
      let alt_sysroot_path = matches.value_of("DEBUG_ALT_SYSROOT")
        .map(|s| PathBuf::from(s));
      match install_self(alt_sysroot_path, guppybot_bin) {
        Err(e) => {
          eprintln!("{:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("print-config", Some(_matches)) => {
      match print_config() {
        Err(e) => {
          eprintln!("{:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("run", Some(matches)) => {
      let gup_py_path = PathBuf::from(matches.value_of("FILE")
        .unwrap_or_else(|| "gup.py"));
      let mutable = matches.is_present("MUTABLE");
      let working_dir = matches.value_of("WORKING_DIR")
        .map(|s| PathBuf::from(s))
        .or_else(|| current_dir().ok());
      match run(mutable, gup_py_path, working_dir) {
        Err(e) => {
          eprintln!("{:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    /*("x-check-deps", Some(matches)) => {
      unimplemented!();
    }*/
    ("x-install-deps", Some(_matches)) => {
      match install_deps() {
        Err(e) => {
          eprintln!("{:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    _ => {
      app.print_help().unwrap();
      println!();
      0
    }
  };
  exit(code)
}
