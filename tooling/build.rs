extern crate walkdir;

use walkdir::{WalkDir};

use std::fs;
use std::path::{PathBuf};
use std::process::{Command};

fn main() {
  println!("cargo:rerun-if-changed=build.rs");
  for entry in WalkDir::new("../sysroot") {
    let entry = entry.unwrap();
    println!("cargo:rerun-if-changed={}", entry.path().display());
  }
  let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
    .canonicalize().unwrap();
  eprintln!("TRACE: cargo manifest dir: {:?}", manifest_dir);
  let sysroot_dir = manifest_dir.join("..").join("sysroot")
    .canonicalize().unwrap();
  eprintln!("TRACE: sysroot dir (guess): {:?}", sysroot_dir);
  let build_dir = manifest_dir.join("..").join("build")
    .canonicalize().unwrap();
  eprintln!("TRACE: build dir (guess): {:?}", build_dir);
  fs::remove_file(build_dir.join("sysroot.tar.gz")).ok();
  let out = Command::new("tar")
    .current_dir(&sysroot_dir)
    .arg("--numeric-owner")
    .arg("--owner=0")
    .arg("--group=0")
    .arg("--exclude='*.swp'")
    .arg("--exclude='*.swo'")
    .arg("-czf")
    .arg(build_dir.join("sysroot.tar.gz"))
    .arg(".")
    .output().unwrap();
  if !out.status.success() {
    panic!("tar failed with exit status: {:?}", out.status);
  }
}
