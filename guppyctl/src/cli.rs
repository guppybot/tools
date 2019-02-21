use clap::{App, Arg, ArgMatches, SubCommand};
use curl::easy::{Easy, List};
use minisodium::{sign_verify};
use schemas::wire_protocol::{DistroInfoV0, GpusV0, MachineConfigV0, CiConfigV0};
use semver::{Version};
use serde_json::{Value as JsonValue};
use tempfile::{NamedTempFile};
use tooling::config::{Config, ApiConfig};
use tooling::deps::{DockerDeps, Docker, NvidiaDocker2};
use tooling::docker::{GitCheckoutSpec, DockerRunStatus};
use tooling::ipc::*;
use tooling::query::{Maybe, Query, fail};
use tooling::state::{ImageManifest, ImageSpec, RootManifest, Sysroot};
//use url::{Url};

use std::env::{current_dir};
use std::fs::{File, create_dir_all};
use std::io::{Write, stdin, stdout};
use std::path::{PathBuf};
use std::process::{Command, exit};

pub fn _dispatch(guppybot_bin: &[u8]) -> ! {
  let mut app = App::new("guppyctl")
    .version("beta")
    .subcommand(SubCommand::with_name("echo-api-id")
      .about("Print the registered API identifier")
    )
    .subcommand(SubCommand::with_name("echo-machine-id")
      .about("Print the registered machine identifier")
    )
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
    .subcommand(SubCommand::with_name("register-ci-machine")
      .about("Register this machine to provide CI for a repository")
      .arg(Arg::with_name("REPOSITORY_URL")
        .index(1)
        .required(true)
        .help("The URL to the repository.")
      )
    )
    .subcommand(SubCommand::with_name("register-ci-repo")
      .about("Register a repository with guppybot.org CI")
      .arg(Arg::with_name("REPOSITORY_URL")
        .index(1)
        .required(true)
        .help("The URL to the repository.")
      )
    )
    .subcommand(SubCommand::with_name("register-machine")
      .about("Register this machine with guppybot.org")
    )
    .subcommand(SubCommand::with_name("reload-config")
      .about("Reload configuration")
    )
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
    .subcommand(SubCommand::with_name("unregister-ci-machine")
      .about("Unregister this machine from providing CI for a repository")
    )
    .subcommand(SubCommand::with_name("unregister-ci-repo")
      .about("Unregister a repository from guppybot.org CI")
    )
    .subcommand(SubCommand::with_name("unregister-machine")
      .about("Unregister this machine from guppybot.org")
    )
    /*.subcommand(SubCommand::with_name("x-check-deps")
      .about("Experimental. Check if dependencies are correctly installed")
    )*/
    .subcommand(SubCommand::with_name("x-install-deps")
      .about("Experimental. Install dependencies with the system package manager")
    )
  ;
  let code = match app.clone().get_matches().subcommand() {
    ("install-self", Some(matches)) => {
      let alt_sysroot_path = matches.value_of("DEBUG_ALT_SYSROOT")
        .map(|s| PathBuf::from(s));
      match install_self(alt_sysroot_path, guppybot_bin) {
        Err(e) => {
          eprintln!("install-self: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("print-config", Some(_matches)) => {
      match print_config() {
        Err(e) => {
          eprintln!("print-config: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("register-ci-machine", Some(matches)) => {
      let repo_url = matches.value_of("REPOSITORY_URL");
      match register_ci_machine(repo_url) {
        Err(e) => {
          eprintln!("register-ci-machine: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("register-ci-repo", Some(matches)) => {
      let repo_url = matches.value_of("REPOSITORY_URL");
      match register_ci_repo(repo_url) {
        Err(e) => {
          eprintln!("register-ci-repo: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("register-machine", Some(matches)) => {
      match register_machine() {
        Err(e) => {
          eprintln!("register-machine: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("reload-config", Some(matches)) => {
      match reload_config() {
        Err(e) => {
          eprintln!("reload-config: {:?}", e);
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
          eprintln!("run: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("unregister-ci-machine", Some(matches)) => {
      // TODO
      unimplemented!();
    }
    ("unregister-ci-repo", Some(matches)) => {
      // TODO
      unimplemented!();
    }
    ("unregister-machine", Some(matches)) => {
      // TODO
      unimplemented!();
    }
    /*("x-check-deps", Some(matches)) => {
      unimplemented!();
    }*/
    ("x-install-deps", Some(_matches)) => {
      match install_deps() {
        Err(e) => {
          eprintln!("x-install-deps: {:?}", e);
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

fn _ensure_api_auth() -> Maybe {
  let mut old_api_id = None;
  let mut old_secret_token = None;
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::_QueryApiAuth)?;
  match chan.recv()? {
    Bot2Ctl::_QueryApiAuth(Some(res)) => {
      old_api_id = res.api_id;
      old_secret_token = res.secret_token;
    }
    Bot2Ctl::_QueryApiAuth(None) => {}
    _ => return Err(fail("IPC protocol error")),
  };
  let mut new_api_id = None;
  let mut new_secret_token = None;
  if old_api_id.is_none() {
    let mut line = String::new();
    print!("API ID: ");
    stdout().flush().unwrap();
    match stdin().read_line(&mut line) {
      Err(_) => return Err(fail("API authentication requires an API ID")),
      Ok(_) => {}
    }
    new_api_id = Some(line);
  }
  if old_secret_token.is_none() {
    let mut line = String::new();
    print!("Secret token: ");
    stdout().flush().unwrap();
    match stdin().read_line(&mut line) {
      Err(_) => return Err(fail("API authentication requires a secret token")),
      Ok(_) => {}
    }
    new_secret_token = Some(line);
  }
  let api_id = old_api_id.or_else(|| new_api_id);
  if api_id.is_none() {
    return Err(fail("missing API authentication details: API ID"));
  }
  let secret_token = old_secret_token.or_else(|| new_secret_token);
  if secret_token.is_none() {
    return Err(fail("missing API authentication details: secret token"));
  }
  let api_id = api_id.unwrap();
  let secret_token = secret_token.unwrap();
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::_TryApiAuth{api_id, secret_token})?;
  match chan.recv()? {
    Bot2Ctl::_TryApiAuth(Some(_)) => {}
    Bot2Ctl::_TryApiAuth(None) => {
      return Err(fail("API authentication failed"));
    }
    _ => return Err(fail("IPC protocol error")),
  }
  Ok(())
}

pub fn install_deps() -> Maybe {
  let distro_info = DistroInfoV0::query()?;
  DockerDeps::check(&distro_info)?
    .install_missing()?;
  if Docker::check(&distro_info)? {
    Docker::install(&distro_info)?;
  }
  if NvidiaDocker2::check(&distro_info)? {
    NvidiaDocker2::install(&distro_info)?;
  }
  Ok(())
}

pub fn install_self(alt_sysroot_path: Option<PathBuf>, _guppybot_bin: &[u8]) -> Maybe {
  // FIXME: reenable the daemon installation.
  /*let mut bot_file = File::create("/usr/local/lib/guppybot")
    .map_err(|_| fail("Failed to create guppybot daemon file: are you root?"))?;
  bot_file.write_all(guppybot_bin)
    .map_err(|_| fail("Failed to write guppybot daemon file: are you root?"))?;*/
  let gpus = GpusV0::query()?;
  let config = Config::default();
  config.install_default(&gpus)?;
  let sysroot = match alt_sysroot_path {
    Some(base_dir) => Sysroot{base_dir},
    None => Sysroot::default(),
  };
  sysroot.install()?;
  println!("Self-installation complete!");
  println!("Guppybot-related files have been installed to:");
  println!();
  println!("    {}", config.config_dir.display());
  println!("    {}", sysroot.base_dir.display());
  println!();
  Ok(())
}

pub fn print_config() -> Maybe {
  // TODO
  let api_cfg = ApiConfig::open_default().ok();
  let machine_cfg = MachineConfigV0::query().ok();
  let ci_cfg = CiConfigV0::query().ok();
  println!("API config: {:?}", api_cfg);
  println!("Machine config: {:?}", machine_cfg);
  println!("CI config: {:?}", ci_cfg);
  Ok(())
}

pub fn register_ci_machine(repo_url: Option<&str>) -> Maybe {
  if repo_url.is_none() {
    return Err(fail("missing repository URL"));
  }
  let repo_url = repo_url.unwrap().to_string();
  _ensure_api_auth()?;
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::RegisterCiMachine{repo_url})?;
  let res = match chan.recv()? {
    Bot2Ctl::RegisterCiMachine(res) => res,
    _ => return Err(fail("IPC protocol error")),
  };
  if res.is_none() {
    return Err(fail("failed to register machine with CI repo"));
  }
  // TODO
  Ok(())
}

pub fn register_ci_repo(repo_url: Option<&str>) -> Maybe {
  if repo_url.is_none() {
    return Err(fail("missing repository URL"));
  }
  let repo_url = repo_url.unwrap().to_string();
  _ensure_api_auth()?;
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::RegisterCiRepo{repo_url})?;
  let res = match chan.recv()? {
    Bot2Ctl::RegisterCiRepo(res) => res,
    _ => return Err(fail("IPC protocol error")),
  };
  if res.is_none() {
    return Err(fail("failed to register CI repo"));
  }
  let res = res.unwrap();
  println!("Almost done! There is one remaining manual configuration step.");
  println!("");
  println!("guppybot.org has prepared the following webhook configuration for the");
  println!("repository:");
  println!("");
  //println!("    Payload URL:  https://guppybot.org/x/github/longshot");
  println!("    Payload URL:  {}", res.webhook_payload_url);
  println!("    Content type: application/json");
  println!("    Secret:       {}", res.webhook_secret);
  println!("    Events:       Send me everything (optional)");
  println!("");
  println!("Please add a webhook with the above configuration in your repository");
  println!("settings, probably at the following URL:");
  println!("");
  //println!("    https://github.com/asdf/qwerty/settings/hooks");
  println!("    {}", res.webhook_settings_url);
  println!("");
  Ok(())
}

pub fn register_machine() -> Maybe {
  _ensure_api_auth()?;
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::RegisterMachine)?;
  let res = match chan.recv()? {
    Bot2Ctl::RegisterMachine(res) => res,
    _ => return Err(fail("IPC protocol error")),
  };
  if res.is_none() {
    // TODO
  }
  Ok(())
}

pub fn reload_config() -> Maybe {
  // TODO
  Ok(())
}

fn _run(mutable: bool, gup_py_path: PathBuf, working_dir: Option<PathBuf>) -> Maybe<DockerRunStatus> {
  let sysroot = Sysroot::default();

  let root_manifest = RootManifest::load(&sysroot)
    .or_else(|_| RootManifest::fresh(&sysroot))?;

  let mut image_manifest = ImageManifest::load(&sysroot, &root_manifest)?;

  let checkout = match working_dir {
    None => GitCheckoutSpec::with_current_dir()?,
    Some(ref path) => GitCheckoutSpec::with_local_dir(path)?,
  };

  let builtin_imagespec = ImageSpec::builtin_default();
  let builtin_image = image_manifest.lookup_docker_image(&builtin_imagespec, &sysroot, &root_manifest)?;
  let gup_py_path = gup_py_path.canonicalize()
    .map_err(|_| fail("failed to get canonical absolute path, required for docker"))?;
  assert!(gup_py_path.is_absolute());
  let tasks = builtin_image._run_taskspec_direct(&gup_py_path, &sysroot)?;
  let num_tasks = tasks.len();
  match num_tasks {
    0 => {}
    1 => println!("Running 1 task..."),
    _ => println!("Running {} tasks...", num_tasks),
  }
  stdout().flush().unwrap();
  for (task_idx, task) in tasks.iter().enumerate() {
    // FIXME: sanitize the task name.
    print!("Running task {}/{} ({})...", task_idx + 1, num_tasks, task.name);
    stdout().flush().unwrap();
    let image = match task.image_candidate() {
      None => {
        println!(" NOT STARTED: No matching image candidate.");
        stdout().flush().unwrap();
        return Ok(DockerRunStatus::Failure);
      }
      Some(im) => im,
    };
    let docker_image = image_manifest.lookup_docker_image(&image, &sysroot, &root_manifest)?;
    let status = match mutable {
      false => docker_image.run(&checkout, task, None),
      true  => docker_image.run_mut(&checkout, task, None),
    }?;
    if let DockerRunStatus::Failure = status {
      // FIXME: report on the task that failed.
      println!(" FAILED!");
      stdout().flush().unwrap();
      return Ok(status);
    }
    println!(" done.");
    stdout().flush().unwrap();
  }

  Ok(DockerRunStatus::Success)
}

pub fn run(mutable: bool, gup_py_path: PathBuf, working_dir: Option<PathBuf>) -> Maybe {
  match _run(mutable, gup_py_path, working_dir)? {
    DockerRunStatus::Success => {
      println!("All tasks ran successfully.");
      Ok(())
    }
    DockerRunStatus::Failure => {
      println!("Some tasks failed.");
      Err(fail("Some tasks failed"))
    }
  }
}
