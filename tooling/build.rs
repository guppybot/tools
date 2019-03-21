extern crate walkdir;

use walkdir::{WalkDir};

use std::fs;
use std::io::{BufRead, Write, Cursor};
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
    panic!("`tar` failed with exit status: {:?}", out.status);
  }
  let out = Command::new("git")
    .current_dir(&manifest_dir)
    .arg("log")
    .arg("-n").arg("1")
    .arg("--format=%H")
    .output().unwrap();
  if !out.status.success() {
    panic!("`git log` failed with exit status: {:?}", out.status);
  }
  match Cursor::new(out.stdout).lines().next() {
    None => panic!("`git log` did not print the commit hash"),
    Some(line) => {
      let line = line.unwrap();
      let mut file = fs::File::create(build_dir.join("commit_hash")).unwrap();
      write!(&mut file, "{}", line).unwrap();
    }
  }
}
