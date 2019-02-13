extern crate walkdir;

use std::fs;
use std::path::{PathBuf};
use std::process::{Command};

fn main() {
  println!("cargo:rerun-if-changed=build.rs");
  // TODO: walk dir in sysroot.
  let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
    .canonicalize().unwrap();
  eprintln!("TRACE: cargo manifest dir: {:?}", manifest_dir);
  let sysroot_dir = manifest_dir.join("..").join("sysroot")
    .canonicalize().unwrap();
  eprintln!("TRACE: sysroot dir (guess): {:?}", sysroot_dir);
  let assets_dir = manifest_dir.join("..").join("build_assets")
    .canonicalize().unwrap();
  eprintln!("TRACE: assets dir (guess): {:?}", assets_dir);
  fs::remove_file(assets_dir.join("sysroot.tar.gz")).ok();
  let out = Command::new("tar")
    .current_dir(&sysroot_dir)
    .arg("--numeric-owner")
    .arg("--owner=0")
    .arg("--group=0")
    .arg("-czf")
    .arg(assets_dir.join("sysroot.tar.gz"))
    .arg(".")
    .output().unwrap();
  if !out.status.success() {
    panic!("tar failed with exit status: {:?}", out.status);
  }
}
