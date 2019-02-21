use crate::query::{Maybe, fail};
use crate::registry::{RegistryChannel};
use crate::state::{ImageSpec, Toolchain, Sysroot};

//use chrono::prelude::*;
use crossbeam_channel::{bounded};
use schemas::v1::{
  CudaToolkitVersionV0,
  DistroIdV0,
  DistroCodenameV0,
  SystemSetupV0,
};
use tempfile::{NamedTempFile, TempDir, tempdir};

use std::env::{current_dir};
use std::fs::{File, create_dir_all};
use std::io::{BufRead, Read, Write, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::str::{from_utf8};

#[derive(Debug)]
pub enum Dir {
  Path(PathBuf),
  Temp(TempDir),
}

impl Dir {
  pub fn path(&self) -> &Path {
    match self {
      &Dir::Path(ref path) => path,
      &Dir::Temp(ref tmp_dir) => tmp_dir.path(),
    }
  }
}

#[derive(Debug)]
pub struct GitCheckoutSpec {
  pub remote_url: String,
  pub dir: Dir,
}

impl GitCheckoutSpec {
  pub fn with_current_dir() -> Maybe<GitCheckoutSpec> {
    let cwd = current_dir().map_err(|_| fail("failed to get current dir"))?;
    Ok(GitCheckoutSpec{
      // TODO
      remote_url: "".to_string(),
      dir: Dir::Path(cwd),
    })
  }

  pub fn with_local_dir(path: &Path) -> Maybe<GitCheckoutSpec> {
    Ok(GitCheckoutSpec{
      // TODO
      remote_url: "".to_string(),
      dir: Dir::Path(path.into()),
    })
  }

  pub fn with_remote_url(remote_url: String) -> Maybe<GitCheckoutSpec> {
    Ok(GitCheckoutSpec{
      remote_url,
      dir: Dir::Temp(tempdir().map_err(|_| fail("failed to create temp dir"))?),
    })
  }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Version {
  Exact,
  AtLeast,
  Any,
}

#[derive(Default)]
struct TaskSpecBuilder {
  name: String,
  toolchain: Option<Toolchain>,
  require_docker: bool,
  require_nvidia_docker: bool,
  require_distro: Option<(Version, DistroCodenameV0)>,
  require_cuda: Option<(Version, Option<CudaToolkitVersionV0>)>,
  require_gpu_arch: Option<()>,
  allow_errors: bool,
  sh: Vec<String>,
}

impl TaskSpecBuilder {
  fn into_task(self) -> Maybe<TaskSpec> {
    Ok(TaskSpec{
      name: self.name,
      toolchain: self.toolchain,
      require_docker: self.require_docker,
      require_nvidia_docker: self.require_nvidia_docker,
      require_distro: self.require_distro
        .ok_or_else(|| fail("missing require_distro"))?,
      require_cuda: self.require_cuda,
      allow_errors: self.allow_errors,
      sh: self.sh,
    })
  }
}

#[derive(Debug)]
pub struct TaskSpec {
  pub name: String,
  pub toolchain: Option<Toolchain>,
  pub require_docker: bool,
  pub require_nvidia_docker: bool,
  pub require_distro: (Version, DistroCodenameV0),
  pub require_cuda: Option<(Version, Option<CudaToolkitVersionV0>)>,
  pub allow_errors: bool,
  pub sh: Vec<String>,
}

impl TaskSpec {
  pub fn image_candidate(&self) -> Option<ImageSpec> {
    if !self.require_docker {
      return None;
    }
    Some(ImageSpec{
      cuda: match self.require_cuda {
        None => None,
        Some((ver, v)) => match (ver, v) {
          (Version::Exact, Some(v)) => {
            Some(v)
          }
          (Version::AtLeast, Some(v)) => {
            Some(v)
          }
          (Version::Any, None) => {
            Some(CudaToolkitVersionV0::Cuda10_0)
          }
          _ => return None,
        },
      },
      distro_codename: self.require_distro.1,
      distro_id: self.require_distro.1.to_id(),
      docker: self.require_docker,
      nvidia_docker: self.require_nvidia_docker,
      toolchain: self.toolchain.clone(),
    })
  }

  pub fn image_candidates(&self) -> Vec<ImageSpec> {
    // TODO
    unimplemented!();
  }
}

pub enum DockerOutput {
  Stdout,
  Chan(RegistryChannel),
}

pub enum DockerRunStatus {
  Success,
  Failure,
}

pub struct DockerImage {
  // TODO
  pub imagespec: ImageSpec,
  pub hash_digest: String,
  //pub base_image: String,
}

impl DockerImage {
  pub fn _build(&self, fresh: bool) -> Maybe {
    let toolchain_image_dir = self.imagespec.to_toolchain_image_dir();
    let toolchain_template_dir = self.imagespec.to_toolchain_docker_template_dir();
    let distro_toolchain_template_dir = toolchain_template_dir.join(self.imagespec.distro_codename.to_desc_str());
    {
      let src_file = File::open(distro_toolchain_template_dir.join("Dockerfile.template"))
        .or_else(|_| File::open(toolchain_template_dir.join("Dockerfile.default_template")))
        .map_err(|_| fail("failed to open Dockerfile template"))?;
      let mut reader = BufReader::new(src_file);
      let mut src_buf = String::new();
      reader.read_to_string(&mut src_buf)
        .map_err(|_| fail("failed to read Dockerfile template"))?;
      create_dir_all(toolchain_image_dir.join(&self.hash_digest)).ok();
      let dst_file = File::create(toolchain_image_dir.join(&self.hash_digest).join("Dockerfile")).unwrap();
      let mut writer = BufWriter::new(dst_file);
      writeln!(&mut writer, "# automatically generated for: gup/{}", self.hash_digest)
        .map_err(|_| fail("failed to write Dockerfile"))?;
      writeln!(&mut writer, "")
        .map_err(|_| fail("failed to write Dockerfile"))?;
      let base_docker_image = self.imagespec.to_docker_base_image()
        .ok_or_else(|| fail("no docker base image candidate"))?;
      writeln!(&mut writer, "FROM {}", base_docker_image)
        .map_err(|_| fail("failed to write Dockerfile"))?;
      writeln!(&mut writer, "")
        .map_err(|_| fail("failed to write Dockerfile"))?;
      writer.write_all(src_buf.as_bytes())
        .map_err(|_| fail("failed to write Dockerfile"))?;
    }
    let mut cmd = Command::new("docker");
    cmd
      .arg("build")
    ;
    if fresh {
      cmd
        .arg("--no-cache")
        .arg("--pull")
      ;
    }
    cmd
      .arg("-t")
      .arg(format!("gup/{}", self.hash_digest))
      .arg(toolchain_image_dir.join(&self.hash_digest))
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
    ;
    let mut proc = cmd.spawn()
      .map_err(|_| fail("failed to run `docker build`"))?;
    //println!("### BEGIN MONITOR ###");
    let mon_h = ConsoleMonitor::sink(proc.stdout.take().unwrap(), proc.stderr.take().unwrap());
    // FIXME: check status.
    proc.wait().ok();
    mon_h.join().ok();
    //println!("### END MONITOR ###");
    Ok(())
  }

  pub fn _run_checkout(&self, checkout: &GitCheckoutSpec, sysroot: &Sysroot) -> Maybe {
    unimplemented!();
  }

  pub fn _run_checkout_ssh(&self, checkout: &GitCheckoutSpec, key_path: String, sysroot: &Sysroot) -> Maybe {
    unimplemented!();
  }

  pub fn _run_taskspec(&self, checkout: &GitCheckoutSpec, sysroot: &Sysroot) -> Maybe<Vec<TaskSpec>> {
    unimplemented!();
  }

  pub fn _run_taskspec_direct(&self, gup_py_path: &PathBuf, sysroot: &Sysroot) -> Maybe<Vec<TaskSpec>> {
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir();
    let mut cmd = Command::new("docker");
    cmd
      .arg("run")
    ;
    if self.imagespec.nvidia_docker {
      cmd.arg("--runtime").arg("nvidia");
    } else {
      cmd.arg("--runtime").arg("runc");
    }
    cmd
      .arg("--rm")
      .arg("--interactive")
      .arg("--log-driver").arg("none")
      //.arg("--tty")
      .arg("--attach").arg("stdin")
      .arg("--attach").arg("stdout")
      .arg("--attach").arg("stderr")
      .arg("--volume").arg(format!("{}:/python:ro", sysroot.base_dir.join("python3.6/site-packages").display()))
      .arg("--volume").arg(format!("{}:/gup.py:ro", gup_py_path.display()))
      .arg("--volume").arg(format!("{}:/entry.sh:ro", toolchain_dir.join("_run_taskspec_direct.sh").display()))
      .arg("--env").arg("PYTHONPATH=/python")
      .arg("--env").arg("CI=1")
      .arg(format!("gup/{}", self.hash_digest))
      .arg("/entry.sh")
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
    ;
    let mut proc = cmd.spawn()
      .map_err(|_| fail("failed to run `docker run`"))?;
    let tasks = if let Some(ref mut stdout) = proc.stdout {
      match _taskspecs(stdout) {
        Err(e) => {
          proc.wait().ok();
          return Err(e);
        }
        Ok(tasks) => tasks,
      }
    } else {
      Vec::new()
    };
    if let Some(ref mut stderr) = proc.stderr {
      let mut buf = String::new();
      stderr.read_to_string(&mut buf).unwrap();
      if !(buf.is_empty() || buf == "\n") {
        return Err(fail("docker taskspec run returned nonempty stderr"));
      }
      /*println!("### BEGIN STDERR ###");
      println!("{}", buf);
      println!("### END STDERR ###");*/
    }
    // FIXME: check status.
    proc.wait().ok();
    Ok(tasks)
  }

  pub fn run(&self, checkout: &GitCheckoutSpec, task: &TaskSpec, mut output: Option<DockerOutput>) -> Maybe<DockerRunStatus> {
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir();
    // FIXME
    //let distro_toolchain_dir = toolchain_dir.join(self.imagespec.distro_codename.to_desc_str());
    //eprintln!("TRACE: docker image: toolchain dir: {}", toolchain_dir.display());
    let mut task_file = NamedTempFile::new()
      .map_err(|_| fail("failed to create temporary script file"))?;
    {
      writeln!(task_file, "#!/bin/sh")
        .map_err(|_| fail("failed to write to script file"))?;
      if task.allow_errors {
        writeln!(task_file, "set -ux")
      } else {
        writeln!(task_file, "set -eux")
      }
        .map_err(|_| fail("failed to write to script file"))?;
      for sh in task.sh.iter() {
        writeln!(task_file, "{}", sh)
          .map_err(|_| fail("failed to write to script file"))?;
      }
      task_file.flush()
        .map_err(|_| fail("failed to write to script file"))?;
    }
    let mut cmd = Command::new("docker");
    cmd
      .arg("run")
    ;
    if self.imagespec.nvidia_docker {
      cmd.arg("--runtime").arg("nvidia");
    } else {
      cmd.arg("--runtime").arg("runc");
    }
    cmd
      .arg("--rm")
      .arg("--interactive")
      .arg("--log-driver").arg("none")
      //.arg("--tty")
      .arg("--attach").arg("stdin")
      .arg("--attach").arg("stdout")
      .arg("--attach").arg("stderr")
      .arg("--volume").arg(format!("{}:/checkout:ro", checkout.dir.path().display()))
      .arg("--volume").arg(format!("{}:/task:ro", task_file.path().display()))
      .arg("--volume").arg(format!("{}:/entry.sh:ro", toolchain_dir.join("run.sh").display()))
      .arg("--env").arg("CI=1")
      .arg(format!("gup/{}", self.hash_digest))
      .arg("/entry.sh")
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
    ;
    let mut proc = cmd.spawn()
      .expect("failed to run `docker run`");
    //println!("### BEGIN MONITOR ###");
    let mon_h = match output {
      None => {
        ConsoleMonitor::sink(proc.stdout.take().unwrap(), proc.stderr.take().unwrap())
      }
      Some(DockerOutput::Stdout) => {
        ConsoleMonitor::serialize_to_stdout(proc.stdout.take().unwrap(), proc.stderr.take().unwrap())
      }
      Some(DockerOutput::Chan(mut chan)) => {
        ConsoleMonitor::serialize_to_channel(proc.stdout.take().unwrap(), proc.stderr.take().unwrap(), Some(&mut chan))
      }
    };
    let maybe_status = proc.wait();
    mon_h.join().ok();
    //println!("### END MONITOR ###");
    let status = maybe_status
      .map_err(|_| fail("failed to wait for `docker run`"))?;
    match status.success() {
      false => Ok(DockerRunStatus::Failure),
      true  => Ok(DockerRunStatus::Success),
    }
  }

  pub fn run_mut(&self, checkout: &GitCheckoutSpec, task: &TaskSpec, mut output: Option<DockerOutput>) -> Maybe<DockerRunStatus> {
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir();
    // FIXME
    //let distro_toolchain_dir = toolchain_dir.join(self.imagespec.distro_codename.to_desc_str());
    //eprintln!("TRACE: docker image: toolchain dir: {}", toolchain_dir.display());
    let mut task_file = NamedTempFile::new()
      .map_err(|_| fail("failed to create temporary script file"))?;
    {
      writeln!(task_file, "#!/bin/sh")
        .map_err(|_| fail("failed to write to script file"))?;
      if task.allow_errors {
        writeln!(task_file, "set -ux")
      } else {
        writeln!(task_file, "set -eux")
      }
        .map_err(|_| fail("failed to write to script file"))?;
      for sh in task.sh.iter() {
        writeln!(task_file, "{}", sh)
          .map_err(|_| fail("failed to write to script file"))?;
      }
      task_file.flush()
        .map_err(|_| fail("failed to write to script file"))?;
    }
    let mut cmd = Command::new("docker");
    cmd
      .arg("run")
    ;
    if self.imagespec.nvidia_docker {
      cmd.arg("--runtime").arg("nvidia");
    } else {
      cmd.arg("--runtime").arg("runc");
    }
    cmd
      .arg("--rm")
      .arg("--interactive")
      .arg("--log-driver").arg("none")
      //.arg("--tty")
      .arg("--attach").arg("stdin")
      .arg("--attach").arg("stdout")
      .arg("--attach").arg("stderr")
      .arg("--volume").arg(format!("{}:/checkout:rw", checkout.dir.path().display()))
      .arg("--volume").arg(format!("{}:/task:ro", task_file.path().display()))
      .arg("--volume").arg(format!("{}:/entry.sh:ro", toolchain_dir.join("run_mut.sh").display()))
      .arg("--env").arg("CI=1")
      .arg(format!("gup/{}", self.hash_digest))
      .arg("/entry.sh")
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
    ;
    let mut proc = cmd.spawn()
      .expect("failed to run `docker run`");
    //println!("### BEGIN MONITOR ###");
    let mon_h = match output {
      None => {
        ConsoleMonitor::sink(proc.stdout.take().unwrap(), proc.stderr.take().unwrap())
      }
      Some(DockerOutput::Stdout) => {
        ConsoleMonitor::serialize_to_stdout(proc.stdout.take().unwrap(), proc.stderr.take().unwrap())
      }
      Some(DockerOutput::Chan(mut chan)) => {
        ConsoleMonitor::serialize_to_channel(proc.stdout.take().unwrap(), proc.stderr.take().unwrap(), Some(&mut chan))
      }
    };
    let maybe_status = proc.wait();
    mon_h.join().ok();
    //println!("### END MONITOR ###");
    let status = maybe_status
      .map_err(|_| fail("failed to wait for `docker run`"))?;
    match status.success() {
      false => Ok(DockerRunStatus::Failure),
      true  => Ok(DockerRunStatus::Success),
    }
  }
}

pub struct DockerPreImage {
}

fn _taskspecs<R: Read>(stdout: &mut R) -> Maybe<Vec<TaskSpec>> {
  let mut tasks = Vec::new();
  let mut builder: Option<TaskSpecBuilder> = None;
  let buf = BufReader::new(stdout);
  for line in buf.lines() {
    let line = line.map_err(|_| fail("failed to understand task spec"))?;
    let line_toks: Vec<_> = line.splitn(2, "#-guppy:").collect();
    if line_toks.len() == 2 && line_toks[0].is_empty() {
      //eprintln!("DEBUG: directive? line toks: {:?}", line_toks);
      let directive_toks: Vec<_> = line_toks[1].splitn(2, ":").collect();
      match directive_toks[0] {
        "task" => {
          panic!("must specify a directive version");
        }
        "v0.task" => {
          let task_toks: Vec<_> = directive_toks[1].split_whitespace().collect();
          match task_toks[0] {
            "begin" => {
              if builder.is_some() {
                // TODO: fail.
                return Err(fail("todo1"));
              }
              builder = Some(TaskSpecBuilder::default());
            }
            "end" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo2"));
              }
              // FIXME
              let builder = builder.take().unwrap();
              tasks.push(builder.into_task()?);
            }
            "name" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo3"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:name takes 1 argument"));
              }
              builder.as_mut().unwrap()
                .name = task_toks[1].to_string();
            }
            "toolchain" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:toolchain takes 1 argument"));
              }
              let toolchain = match task_toks[1] {
                "rust_nightly" => Toolchain::RustNightly,
                _ => return Err(fail("v0.task: unsupported toolchain")),
              };
              builder.as_mut().unwrap()
                .toolchain = Some(toolchain);
            }
            "require_docker" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo4"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:require_docker takes 1 argument"));
              }
              builder.as_mut().unwrap()
                .require_docker = task_toks[1].parse()
                  .map_err(|_| fail("v0.task:require_docker takes boolean argument"))?;
            }
            "require_nvidia_docker" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo5"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:require_nvidia_docker takes 1 argument"));
              }
              builder.as_mut().unwrap()
                .require_nvidia_docker = task_toks[1].parse()
                  .map_err(|_| fail("v0.task:require_nvidia_docker takes boolean argument"))?;
            }
            "require_distro" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo6"));
              }
              if task_toks.len() <= 2 {
                return Err(fail("v0.task:require_distro takes 2 arguments"));
              }
              let distro_id = match task_toks[1] {
                "alpine" => DistroIdV0::Alpine,
                "centos" => DistroIdV0::Centos,
                "debian" => DistroIdV0::Debian,
                "ubuntu" => DistroIdV0::Ubuntu,
                _ => return Err(fail("v0.task: unsupported distro")),
              };
              let mut ver = Version::Exact;
              let mut ver_pat = None;
              if task_toks[2].starts_with("==") {
                ver = Version::Exact;
                ver_pat = Some("==");
              } else if task_toks[2].starts_with(">=") {
                ver = Version::AtLeast;
                ver_pat = Some(">=");
              }
              let code_str = if let Some(pat) = ver_pat {
                let code_toks: Vec<_> = task_toks[2].splitn(2, pat).collect();
                // FIXME: length check.
                code_toks[1]
              } else {
                task_toks[2]
              };
              let code = match (distro_id, code_str) {
                (DistroIdV0::Alpine, "3.8") => DistroCodenameV0::Alpine3_8,
                (DistroIdV0::Alpine, "3.9") => DistroCodenameV0::Alpine3_9,
                (DistroIdV0::Centos, "6") => DistroCodenameV0::Centos6,
                (DistroIdV0::Centos, "7") => DistroCodenameV0::Centos7,
                (DistroIdV0::Debian, "wheezy") => DistroCodenameV0::DebianWheezy,
                (DistroIdV0::Debian, "7") |
                (DistroIdV0::Debian, "wheezy") => DistroCodenameV0::DebianWheezy,
                (DistroIdV0::Debian, "8") |
                (DistroIdV0::Debian, "jessie") => DistroCodenameV0::DebianJessie,
                (DistroIdV0::Debian, "9") |
                (DistroIdV0::Debian, "stretch") => DistroCodenameV0::DebianStretch,
                (DistroIdV0::Debian, "10") |
                (DistroIdV0::Debian, "buster") => DistroCodenameV0::DebianBuster,
                (DistroIdV0::Ubuntu, "14.04") |
                (DistroIdV0::Ubuntu, "trusty") => DistroCodenameV0::UbuntuTrusty,
                (DistroIdV0::Ubuntu, "16.04") |
                (DistroIdV0::Ubuntu, "xenial") => DistroCodenameV0::UbuntuXenial,
                (DistroIdV0::Ubuntu, "18.04") |
                (DistroIdV0::Ubuntu, "bionic") => DistroCodenameV0::UbuntuBionic,
                _ => return Err(fail("v0.task: unsupported distro version")),
              };
              builder.as_mut().unwrap()
                .require_distro = Some((ver, code));
            }
            "require_cuda" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo7"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:require_cuda takes 1 argument"));
              }
              let mut ver = Version::Exact;
              let mut ver_pat = None;
              if task_toks[1] == "*" {
                ver = Version::Any;
              } else if task_toks[1].starts_with("==") {
                ver = Version::Exact;
                ver_pat = Some("==");
              } else if task_toks[1].starts_with(">=") {
                ver = Version::AtLeast;
                ver_pat = Some(">=");
              }
              let maybe_code = if ver == Version::Any {
                None
              } else {
                let code_str = if let Some(pat) = ver_pat {
                  let ver_toks: Vec<_> = task_toks[1].splitn(2, pat).collect();
                  // FIXME: length check.
                  ver_toks[1]
                } else {
                  task_toks[1]
                };
                let code = match code_str {
                  "6.5" => CudaToolkitVersionV0::Cuda6_5,
                  "7.0" => CudaToolkitVersionV0::Cuda7_0,
                  "7.5" => CudaToolkitVersionV0::Cuda7_5,
                  "8.0" => CudaToolkitVersionV0::Cuda8_0,
                  "9.0" => CudaToolkitVersionV0::Cuda9_0,
                  "9.1" => CudaToolkitVersionV0::Cuda9_1,
                  "9.2" => CudaToolkitVersionV0::Cuda9_2,
                  "10.0" => CudaToolkitVersionV0::Cuda10_0,
                  _ => return Err(fail("v0.task: unsupported cuda version")),
                };
                Some(code)
              };
              builder.as_mut().unwrap()
                .require_cuda = Some((ver, maybe_code));
            }
            "require_gpu_arch" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo8"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:require_gpu_arch takes 1 argument"));
              }
              // TODO
              match task_toks[1] {
                "*" => {}
                _ => return Err(fail("todo")),
              }
            }
            "allow_errors" => {
              if builder.is_none() {
                // TODO: fail.
                return Err(fail("todo"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:allow_errors takes 1 argument"));
              }
              builder.as_mut().unwrap()
                .allow_errors = task_toks[1].parse()
                  .map_err(|_| fail("v0.task:allow_errors takes boolean argument"))?;
            }
            _ => return Err(fail("todo")),
          }
        }
        _ => return Err(fail("todo")),
      }
    } else {
      //eprintln!("DEBUG: sh? line toks: {:?}", line_toks);
      if builder.is_none() {
        // TODO: fail.
        return Err(fail("todo9"));
      }
      builder.as_mut().unwrap()
        .sh.push(line);
    }
  }
  if builder.is_some() {
    // TODO: fail.
    return Err(fail("todo10"));
  }
  Ok(tasks)
}

struct MonitorJoin {
  joins: Vec<thread::JoinHandle<()>>,
}

impl MonitorJoin {
  pub fn join(mut self) -> Maybe {
    let mut err_ct = 0;
    for h in self.joins.drain(..) {
      let r = h.join().map_err(|_| fail("failed to join worker"));
      if r.is_err() {
        err_ct += 1;
      }
    }
    if err_ct > 0 {
      Err(fail(format!("failed to join {}/3 workers", err_ct)))
    } else {
      Ok(())
    }
  }
}

struct ConsoleMonitor {
}

impl ConsoleMonitor {
  pub fn sink<Stdout, Stderr>(stdout: Stdout, stderr: Stderr) -> MonitorJoin
  where Stdout: Read + Send + 'static, Stderr: Read + Send + 'static {
    let (stdout_tx, mon_rx) = bounded(64);
    let stderr_tx = stdout_tx.clone();
    let joins = vec![
      thread::spawn(move || {
        let buf = BufReader::with_capacity(64, stdout);
        for line in buf.lines() {
          let _line = line.unwrap();
          stdout_tx.send(()).unwrap();
        }
      }),
      thread::spawn(move || {
        let buf = BufReader::with_capacity(64, stderr);
        for line in buf.lines() {
          let _line = line.unwrap();
          stderr_tx.send(()).unwrap();
        }
      }),
      thread::spawn(move || {
        loop {
          match mon_rx.recv() {
            Err(_) => break,
            Ok(_) => {}
          }
        }
      }),
    ];
    MonitorJoin{joins}
  }

  pub fn serialize_to_stdout<Stdout, Stderr>(stdout: Stdout, stderr: Stderr) -> MonitorJoin
  where Stdout: Read + Send + 'static, Stderr: Read + Send + 'static {
    let (stdout_tx, mon_rx) = bounded(64);
    let stderr_tx = stdout_tx.clone();
    let joins = vec![
      thread::spawn(move || {
        let buf = BufReader::with_capacity(64, stdout);
        for line in buf.lines() {
          let line = line.unwrap();
          stdout_tx.send(line).unwrap();
        }
      }),
      thread::spawn(move || {
        let buf = BufReader::with_capacity(64, stderr);
        for line in buf.lines() {
          let line = line.unwrap();
          stderr_tx.send(line).unwrap();
        }
      }),
      thread::spawn(move || {
        loop {
          match mon_rx.recv() {
            Err(_) => break,
            Ok(line) => println!("{}", line),
          }
        }
      }),
    ];
    MonitorJoin{joins}
  }

  pub fn serialize_to_channel<Stdout, Stderr>(stdout: Stdout, stderr: Stderr, chan: Option<&mut RegistryChannel>) -> MonitorJoin
  where Stdout: Read + Send + 'static, Stderr: Read + Send + 'static {
    let (stdout_tx, mon_rx) = bounded(64);
    let stderr_tx = stdout_tx.clone();
    let joins = vec![
      thread::spawn(move || {
        let buf = BufReader::with_capacity(64, stdout);
        for line in buf.lines() {
          let line = line.unwrap();
          stdout_tx.send(line).unwrap();
        }
      }),
      thread::spawn(move || {
        let buf = BufReader::with_capacity(64, stderr);
        for line in buf.lines() {
          let line = line.unwrap();
          stderr_tx.send(line).unwrap();
        }
      }),
      thread::spawn(move || {
        loop {
          match mon_rx.recv() {
            Err(_) => break,
            Ok(line) => {
              // FIXME
              println!("{}", line);
            }
          }
        }
      }),
    ];
    MonitorJoin{joins}
  }
}
