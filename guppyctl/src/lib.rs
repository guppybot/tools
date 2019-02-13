extern crate curl;
extern crate minisodium;
extern crate schemas;
extern crate semver;
extern crate serde_json;
extern crate tempfile;
extern crate tooling;

use curl::easy::{Easy, List};
use minisodium::{sign_verify};
use schemas::wire_protocol::{DistroInfoV0};
use semver::{Version};
use serde_json::{Value as JsonValue};
use tempfile::{NamedTempFile};
use tooling::deps::{DockerDeps, Docker, NvidiaDocker2};
use tooling::docker::{GitCheckoutSpec};
use tooling::query::{Maybe, Query, fail};
use tooling::state::{ImageManifest, ImageSpec, RootManifest, Sysroot};

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

pub fn install_self(_guppybot_bin: &[u8]) -> Maybe {
  // FIXME: reenable the daemon installation.
  /*let mut bot_file = File::create("/usr/local/lib/guppybot")
    .map_err(|_| fail("Failed to create guppybot daemon file: are you root?"))?;
  bot_file.write_all(guppybot_bin)
    .map_err(|_| fail("Failed to write guppybot daemon file: are you root?"))?;*/
  let sysroot = Sysroot::default();
  sysroot.install()?;
  println!("Self-installation complete!");
  println!("Guppybot-related files have been installed to:");
  println!();
  println!("    {}", sysroot.base_dir.display());
  println!();
  Ok(())
}

pub fn run(mutable: bool, gup_py_path: PathBuf, working_dir: Option<PathBuf>) -> Maybe {
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
  for (task_idx, task) in tasks.iter().enumerate() {
    let image = match task.image_candidate() {
      None => {
        eprintln!("TRACE: task {}/{}: no image candidate", task_idx + 1, num_tasks);
        continue;
      }
      Some(im) => im,
    };
    //eprintln!("TRACE: task {}/{}: image: {:?}", task_idx + 1, num_tasks, image);
    let docker_image = image_manifest.lookup_docker_image(&image, &sysroot, &root_manifest)?;
    match mutable {
      false => docker_image.run(&checkout, task, None)?,
      true  => docker_image.run_mut(&checkout, task, None)?,
    }
  }

  Ok(())
}
