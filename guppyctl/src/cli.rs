use clap::{App, Arg, ArgMatches, SubCommand};
use crossbeam_utils::{Backoff};
use curl::easy::{Easy, List};
//use monosodium::{sign_verify};
use schemas::wire_protocol::{DistroInfoV0, GpusV0, MachineConfigV0};
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
use std::time::{Instant};

pub fn _dispatch(guppybot_bin: &[u8]) -> ! {
  let mut app = App::new("guppyctl")
    .version("beta")
    .subcommand(SubCommand::with_name("auth")
      .about("Authenticate with guppybot.org")
    )
    /*.subcommand(SubCommand::with_name("echo-api-id")
      .about("Print the registered API identifier")
    )
    .subcommand(SubCommand::with_name("echo-machine-id")
      .about("Print the registered machine identifier")
    )*/
    .subcommand(SubCommand::with_name("install-self")
      .about("Install guppybot")
      .arg(Arg::with_name("DEBUG_ALT_SYSROOT")
        .long("debug-alt-sysroot")
        .takes_value(true)
        .help("Debug option: alternative sysroot path. The default sysroot\npath is '/var/lib/guppybot'.")
      )
    )
    /*.subcommand(SubCommand::with_name("print-config")
      .about("Print the currently loaded configuration")
    )*/
    /*.subcommand(SubCommand::with_name("register-ci-group-machine")
      .about("Register this machine to provide CI for a group")
      .arg(Arg::with_name("GROUP_ID")
        .index(1)
        .required(true)
        .help("The group ID.")
      )
    )
    .subcommand(SubCommand::with_name("register-ci-group-repo")
      .about("Register a repository with a CI group")
      .arg(Arg::with_name("GROUP_ID")
        .index(1)
        .required(true)
        .help("The group ID.")
      )
      .arg(Arg::with_name("REPOSITORY_URL")
        .index(2)
        .required(true)
        .help("The URL to the repository.")
      )
    )*/
    .subcommand(SubCommand::with_name("register-ci-machine")
      .about("Register this machine to run CI tasks for a repository")
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
      .about("")
    )
    .subcommand(SubCommand::with_name("run-local")
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
      /*.arg(Arg::with_name("WORKING_DIR")
        .index(1)
        .required(false)
        .help("The local working directory. If not provided, the default\nis the current directory.")
      )*/
      .arg(Arg::with_name("WORKING_DIR")
        .short("d")
        .long("dir")
        .takes_value(true)
        .help("The local working directory. If not provided, the default\nis the current directory.")
      )
    )
    .subcommand(SubCommand::with_name("unauth")
      .about("Deauthenticate with guppybot.org")
    )
    /*.subcommand(SubCommand::with_name("unregister-ci-machine")
      .about("Unregister this machine from providing CI for a repository")
    )
    .subcommand(SubCommand::with_name("unregister-ci-repo")
      .about("Unregister a repository from guppybot.org CI")
    )
    .subcommand(SubCommand::with_name("unregister-machine")
      .about("Unregister this machine from guppybot.org")
    )*/
    /*.subcommand(SubCommand::with_name("x-check-deps")
      .about("Experimental. Check if dependencies are correctly installed")
    )*/
    .subcommand(SubCommand::with_name("x-install-deps")
      .about("Experimental. Install dependencies with the system package manager")
    )
  ;
  let code = match app.clone().get_matches().subcommand() {
    ("auth", Some(_matches)) => {
      match auth() {
        Err(e) => {
          eprintln!("auth: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("unauth", Some(_matches)) => {
      match unauth() {
        Err(e) => {
          eprintln!("unauth: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
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
    ("register-ci-group-machine", Some(matches)) => {
      match register_ci_group_machine() {
        Err(e) => {
          eprintln!("register-ci-group-machine: {:?}", e);
          1
        }
        Ok(_) => 0,
      }
    }
    ("register-ci-group-repo", Some(matches)) => {
      match register_ci_group_repo() {
        Err(e) => {
          eprintln!("register-ci-group-repo: {:?}", e);
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
    ("run-local", Some(matches)) => {
      let gup_py_path = PathBuf::from(matches.value_of("FILE")
        .unwrap_or_else(|| "gup.py"));
      let mutable = matches.is_present("MUTABLE");
      let working_dir = matches.value_of("WORKING_DIR")
        .map(|s| PathBuf::from(s))
        .or_else(|| current_dir().ok());
      match run_local(mutable, gup_py_path, working_dir) {
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

fn _query_api_auth_config() -> Maybe<(Option<String>, Option<String>)> {
  let mut old_api_id = None;
  let mut old_secret_token = None;
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::_QueryApiAuthConfig)?;
  match chan.recv()? {
    Bot2Ctl::_QueryApiAuthConfig(Some(res)) => {
      old_api_id = res.api_id;
      old_secret_token = res.secret_token;
    }
    Bot2Ctl::_QueryApiAuthConfig(None) => {}
    _ => return Err(fail("IPC protocol error")),
  };
  chan.hup();
  Ok((old_api_id, old_secret_token))
}

fn _retry_api_auth(old_api_id: Option<String>, old_secret_token: Option<String>) -> Maybe {
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
  let api_id = old_api_id.or_else(|| new_api_id.clone());
  if api_id.is_none() {
    return Err(fail("missing API authentication details: API ID"));
  }
  let secret_token = old_secret_token.or_else(|| new_secret_token.clone());
  if secret_token.is_none() {
    return Err(fail("missing API authentication details: secret token"));
  }
  if new_api_id.is_some() || new_secret_token.is_some() {
    let api_id = api_id.unwrap();
    let secret_token = secret_token.unwrap();
    let mut chan = CtlChannel::open_default()?;
    chan.send(&Ctl2Bot::_DumpApiAuthConfig{api_id, secret_token})?;
    match chan.recv()? {
      Bot2Ctl::_DumpApiAuthConfig(Some(_)) => {}
      Bot2Ctl::_DumpApiAuthConfig(None) => {
        return Err(fail("failed to write new API auth config"));
      }
      _ => return Err(fail("IPC protocol error")),
    }
    chan.hup();
  }
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::_RetryApiAuth)?;
  match chan.recv()? {
    Bot2Ctl::_RetryApiAuth(Some(_)) => {}
    Bot2Ctl::_RetryApiAuth(None) => {
      return Err(fail("API authentication failed"));
    }
    _ => return Err(fail("IPC protocol error")),
  }
  chan.hup();
  let backoff = Backoff::new();
  loop {
    let mut chan = CtlChannel::open_default()?;
    chan.send(&Ctl2Bot::_AckRetryApiAuth)?;
    let msg = chan.recv()?;
    chan.hup();
    match msg {
      Bot2Ctl::_AckRetryApiAuth(Done(_)) => {
        break;
      }
      Bot2Ctl::_AckRetryApiAuth(Pending) => {
        backoff.snooze();
        continue;
      }
      Bot2Ctl::_AckRetryApiAuth(Stopped) => {
        return Err(fail("API authentication failed"));
      }
      _ => return Err(fail("IPC protocol error")),
    }
  }
  Ok(())
}

fn _query_api_auth_state() -> Maybe<(bool, bool)> {
  let mut auth = false;
  let mut auth_bit = false;
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::_QueryApiAuthState)?;
  match chan.recv()? {
    Bot2Ctl::_QueryApiAuthState(Some(rep)) => {
      auth = rep.auth;
      auth_bit = rep.auth_bit;
    }
    Bot2Ctl::_QueryApiAuthState(None) => {
      return Err(fail("failed to query API authentication state"));
    }
    _ => return Err(fail("IPC protocol error")),
  };
  chan.hup();
  Ok((auth, auth_bit))
}

fn _ensure_api_auth() -> Maybe {
  let auth = _query_api_auth_state()
    .and_then(|(auth, auth_bit)| match (auth, auth_bit) {
      (true,  true) => Ok(true),
      (false, true) => Ok(false),
      _ => Err(fail("not authenticated"))
    })?;
  if !auth {
    let (api_id, secret_token) = _query_api_auth_config()?;
    _retry_api_auth(api_id, secret_token)?;
    println!("Successfully authenticated.");
  }
  Ok(())
}

pub fn auth() -> Maybe {
  let (api_id, secret_token) = _query_api_auth_config()?;
  _retry_api_auth(api_id, secret_token)?;
  println!("Successfully authenticated.");
  Ok(())
}

pub fn unauth() -> Maybe {
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::_UndoApiAuth)?;
  match chan.recv()? {
    Bot2Ctl::_UndoApiAuth(Some(_)) => {}
    Bot2Ctl::_UndoApiAuth(None) => {
      return Err(fail("failed to unauthenticate"));
    }
    _ => return Err(fail("IPC protocol error")),
  }
  chan.hup();
  println!("Unauthenticated.");
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
  //let ci_cfg = CiConfigV0::query().ok();
  println!("API config: {:?}", api_cfg);
  println!("Machine config: {:?}", machine_cfg);
  //println!("CI config: {:?}", ci_cfg);
  Ok(())
}

pub fn register_ci_group_machine() -> Maybe {
  // TODO
  Ok(())
}

pub fn register_ci_group_repo() -> Maybe {
  // TODO
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
  let rep = match chan.recv()? {
    Bot2Ctl::RegisterCiMachine(rep) => rep,
    _ => return Err(fail("IPC protocol error")),
  };
  chan.hup();
  if rep.is_none() {
    return Err(fail("failed to register machine with CI repo"));
  }
  let backoff = Backoff::new();
  loop {
    let mut chan = CtlChannel::open_default()?;
    chan.send(&Ctl2Bot::AckRegisterCiMachine)?;
    let msg = chan.recv()?;
    chan.hup();
    match msg {
      Bot2Ctl::AckRegisterCiMachine(Done(_)) => {
        break;
      }
      Bot2Ctl::AckRegisterCiMachine(Pending) => {
        backoff.snooze();
        continue;
      }
      Bot2Ctl::AckRegisterCiMachine(Stopped) => {
        return Err(fail("failed to register CI machine"));
      }
      _ => return Err(fail("IPC protocol error")),
    }
  }
  println!("Successfully registered machine for repository CI.");
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
  chan.hup();
  if res.is_none() {
    return Err(fail("failed to register CI repo"));
  }
  let backoff = Backoff::new();
  let mut rep = None;
  loop {
    let mut chan = CtlChannel::open_default()?;
    chan.send(&Ctl2Bot::AckRegisterCiRepo)?;
    let msg = chan.recv()?;
    chan.hup();
    match msg {
      Bot2Ctl::AckRegisterCiRepo(Done(r)) => {
        rep = Some(r);
        break;
      }
      Bot2Ctl::AckRegisterCiRepo(Pending) => {
        backoff.snooze();
        continue;
      }
      Bot2Ctl::AckRegisterCiRepo(Stopped) => {
        return Err(fail("failed to register CI repo"));
      }
      _ => return Err(fail("IPC protocol error")),
    }
  }
  let rep = rep.unwrap();
  println!("Almost done! There is one remaining manual configuration step.");
  println!("");
  println!("guppybot.org has prepared the following webhook configuration for the");
  println!("repository:");
  println!("");
  println!("    Payload URL:  {}", rep.webhook_payload_url);
  println!("    Content type: application/json");
  println!("    Secret:       {}", rep.webhook_secret);
  println!("    Events:       Send me everything");
  println!("");
  println!("Please add a webhook with the above configuration in your repository");
  println!("settings, probably at the following URL:");
  println!("");
  println!("    {}", rep.webhook_settings_url.unwrap_or_else(|| "".to_string()));
  println!("");
  Ok(())
}

pub fn register_machine() -> Maybe {
  _ensure_api_auth()?;
  let mut chan = CtlChannel::open_default()?;
  chan.send(&Ctl2Bot::RegisterMachine)?;
  let msg = chan.recv()?;
  chan.hup();
  match msg {
    Bot2Ctl::RegisterMachine(Some(_)) => {}
    Bot2Ctl::RegisterMachine(None) => {
      return Err(fail("failed to register machine"));
    }
    _ => return Err(fail("IPC protocol error")),
  }
  let backoff = Backoff::new();
  loop {
    let mut chan = CtlChannel::open_default()?;
    chan.send(&Ctl2Bot::AckRegisterMachine)?;
    let msg = chan.recv()?;
    chan.hup();
    match msg {
      Bot2Ctl::AckRegisterMachine(Done(_)) => {
        break;
      }
      Bot2Ctl::AckRegisterMachine(Pending) => {
        backoff.snooze();
        continue;
      }
      Bot2Ctl::AckRegisterMachine(Stopped) => {
        return Err(fail("failed to register machine"));
      }
      _ => return Err(fail("IPC protocol error")),
    }
  }
  println!("Successfully registered machine.");
  Ok(())
}

pub fn reload_config() -> Maybe {
  // TODO
  Ok(())
}

fn _run_local(mutable: bool, gup_py_path: PathBuf, working_dir: Option<PathBuf>) -> Maybe<DockerRunStatus> {
  let run_start = Instant::now();

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
    let task_start = Instant::now();
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
      false => docker_image.run(&checkout, task, &sysroot, None),
      true  => docker_image.run_mut(&checkout, task, &sysroot, None),
    }?;
    if let DockerRunStatus::Failure = status {
      // FIXME: display task timing.
      // FIXME: report on the task that failed.
      let task_end = Instant::now();
      println!(" FAILED!");
      stdout().flush().unwrap();
      return Ok(status);
    }
    let task_end = Instant::now();
    let task_dur = task_end - task_start;
    let task_ms = task_dur.subsec_millis() as u64;
    let task_s = task_dur.as_secs() + task_ms / 500;
    let task_m = task_s / 60;
    let task_h = task_m / 60;
    if task_h > 0 {
      println!(" done (elapsed: {}h {:02}m {:02}s).", task_h, task_m % 60, task_s % 60);
    } else if task_m > 0 {
      println!(" done (elapsed: {}m {:02}s).", task_m, task_s % 60);
    } else {
      println!(" done (elapsed: {}s).", task_s);
    }
    stdout().flush().unwrap();
  }

  let run_end = Instant::now();
  print!("All tasks ran successfully");
  let run_dur = run_end - run_start;
  let run_ms = run_dur.subsec_millis() as u64;
  let run_s = run_dur.as_secs() + run_ms / 500;
  let run_m = run_s / 60;
  let run_h = run_m / 60;
  if run_h > 0 {
    println!(" (elapsed: {}h {:02}m {:02}s).", run_h, run_m % 60, run_s % 60);
  } else if run_m > 0 {
    println!(" (elapsed: {}m {:02}s).", run_m, run_s % 60);
  } else {
    println!(" (elapsed: {}s).", run_s);
  }

  Ok(DockerRunStatus::Success)
}

pub fn run_local(mutable: bool, gup_py_path: PathBuf, working_dir: Option<PathBuf>) -> Maybe {
  match _run_local(mutable, gup_py_path, working_dir)? {
    DockerRunStatus::Success => {
      Ok(())
    }
    DockerRunStatus::Failure => {
      println!("Some tasks failed.");
      Err(fail("Some tasks failed"))
    }
  }
}
