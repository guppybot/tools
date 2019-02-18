extern crate curl;
extern crate minisodium;
extern crate schemas;
extern crate semver;
extern crate serde_json;
extern crate tempfile;
extern crate tooling;
extern crate url;

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
use url::{Url};

use std::fs::{File};
use std::io::{Write, stdin, stdout};
use std::path::{PathBuf};
use std::process::{Command};

pub mod cli;

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
  let api_cfg = ApiConfig::open_default().ok();
  let machine_cfg = MachineConfigV0::query().ok();
  let ci_cfg = CiConfigV0::query().ok();
  println!("API config: {:?}", api_cfg);
  println!("Machine config: {:?}", machine_cfg);
  println!("CI config: {:?}", ci_cfg);
  Ok(())
}

pub fn register_ci_machine() -> Maybe {
  // TODO
  Ok(())
}

pub fn register_ci_repo(repo_url: Option<&str>) -> Maybe {
  if repo_url.is_none() {
    return Err(fail("missing repo URL"));
  }
  let repo_url = repo_url.unwrap().to_string();
  let mut chan = CtlChannel::open_default()?;
  let send_msg = Ctl2Bot::RegisterCiRepo{repo_url};
  chan.send(&send_msg)?;
  let recv_msg: Bot2Ctl = chan.recv()?;
  let res = match recv_msg {
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
  let mut chan = CtlChannel::open_default()?;
  let send_msg = Ctl2Bot::RegisterMachine;
  chan.send(&send_msg)?;
  let recv_msg: Bot2Ctl = chan.recv()?;
  let res = match recv_msg {
    Bot2Ctl::RegisterMachine(res) => res,
    _ => return Err(fail("IPC protocol error")),
  };
  if res.is_none() {
    // TODO
  }
  Ok(())
}

/*pub fn reload_config() -> Maybe {
}*/

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
  for (task_idx, task) in tasks.iter().enumerate() {
    let image = match task.image_candidate() {
      None => {
        eprintln!("TRACE: task {}/{}: no image candidate", task_idx + 1, num_tasks);
        return Ok(DockerRunStatus::Failure);
      }
      Some(im) => im,
    };
    //eprintln!("TRACE: task {}/{}: image: {:?}", task_idx + 1, num_tasks, image);
    let docker_image = image_manifest.lookup_docker_image(&image, &sysroot, &root_manifest)?;
    let status = match mutable {
      false => docker_image.run(&checkout, task, None),
      true  => docker_image.run_mut(&checkout, task, None),
    }?;
    if let DockerRunStatus::Failure = status {
      // FIXME: report on the task that failed.
      return Ok(status);
    }
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
      println!("Some tasks ran unsuccessfully.");
      Err(fail("Some tasks ran unsuccessfully"))
    }
  }
}
