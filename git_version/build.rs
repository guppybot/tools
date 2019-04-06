use std::fs;
use std::io::{BufRead, Write, Cursor};
use std::path::{PathBuf};
use std::process::{Command};

fn main() {
  println!("cargo:rerun-if-changed=build.rs");
  println!("cargo:rerun-if-changed=../.git/logs/HEAD");
  let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
    .canonicalize().unwrap();
  eprintln!("TRACE: cargo manifest dir: {:?}", manifest_dir);
  let build_dir = manifest_dir.join("..").join("build")
    .canonicalize().unwrap();
  eprintln!("TRACE: build dir (guess): {:?}", build_dir);
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
