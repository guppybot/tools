use crate::query::{Maybe, fail};
use crate::state::{ImageSpec, Toolchain, Sysroot};

//use chrono::prelude::*;
use crossbeam_channel::{Sender, bounded};
use curl::easy::{Easy as CurlEasy, List as CurlList};
use schemas::v1::{
  CudaVersionV0,
  DistroIdV0,
  DistroCodenameV0,
  SystemSetupV0,
};
use tempfile::{NamedTempFile, TempDir, tempdir};
use url::{Url};

use std::env::{current_dir};
use std::fs::{File, create_dir_all};
use std::io::{BufRead, Read, Write, BufReader, BufWriter, Cursor};
use std::path::{Path, PathBuf, Component};
use std::process::{Command, Stdio};
use std::thread;
use std::str::{from_utf8};
use std::sync::{Arc};

#[derive(Clone, Debug)]
pub enum Dir {
  Path(PathBuf),
  Temp(Arc<TempDir>),
}

impl Dir {
  pub fn path(&self) -> &Path {
    match self {
      &Dir::Path(ref path) => path,
      &Dir::Temp(ref tmp_dir) => tmp_dir.path(),
    }
  }
}

#[derive(Clone, Debug)]
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
      dir: Dir::Temp(Arc::new(tempdir().map_err(|_| fail("failed to create temp dir"))?)),
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
  require_cuda: Option<(Version, Option<CudaVersionV0>)>,
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

#[derive(Clone, Debug)]
pub struct TaskSpec {
  pub name: String,
  pub toolchain: Option<Toolchain>,
  pub require_docker: bool,
  pub require_nvidia_docker: bool,
  pub require_distro: (Version, DistroCodenameV0),
  pub require_cuda: Option<(Version, Option<CudaVersionV0>)>,
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
            Some(CudaVersionV0{major: 10, minor: 0})
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
  Buffer{buf_sz: usize, consumer: Box<Fn(u64, Vec<u8>) + Send>},
}

#[derive(Debug)]
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
  pub fn _build(&self, fresh: bool, sysroot: &Sysroot) -> Maybe {
    let toolchain_image_dir = self.imagespec.to_toolchain_image_dir(sysroot);
    let toolchain_template_dir = self.imagespec.to_toolchain_docker_template_dir(sysroot);
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
    let remote_url = Url::parse(&checkout.remote_url)
      .map_err(|_| fail("invalid remote URL"))?;
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir(sysroot);
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
      .arg("--volume").arg(format!("{}:/entry.sh:ro", toolchain_dir.join("_run_checkout.sh").display()))
      .arg("--env").arg(format!("GUPPY_GIT_REMOTE_URL={}", remote_url.as_str()))
      .arg("--env").arg("CI=1")
      .arg(format!("gup/{}", self.hash_digest))
      .arg("/entry.sh")
      .stdout(Stdio::null())
      .stderr(Stdio::piped())
    ;
    let mut proc = cmd.spawn()
      .map_err(|_| fail("checkout: failed to run `docker run`"))?;
    if let Some(ref mut stderr) = proc.stderr {
      let mut buf = String::new();
      stderr.read_to_string(&mut buf).unwrap();
      if !(buf.is_empty() || buf == "\n") {
        proc.wait().ok();
        return Err(fail("checkout: `docker run` returned nonempty stderr"));
      }
    }
    let status = proc.wait()
      .map_err(|_| fail("checkout: failed to wait for `docker run`"))?;
    match status.success() {
      false => Err(fail("checkout: `docker run` exited with nonzero status")),
      true  => Ok(())
    }
  }

  pub fn _run_checkout_ssh(&self, checkout: &GitCheckoutSpec, key_path: String, sysroot: &Sysroot) -> Maybe {
    unimplemented!();
  }

  pub fn _run_spec(&self, checkout: &GitCheckoutSpec, sysroot: &Sysroot) -> Maybe<(Vec<u8>, Vec<TaskSpec>)> {
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir(sysroot);
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
      .arg("--volume").arg(format!("{}:/_python:ro", sysroot.base_dir.join("python3.6/site-packages").display()))
      .arg("--volume").arg(format!("{}:/checkout:ro", checkout.dir.path().display()))
      .arg("--volume").arg(format!("{}:/entry.sh:ro", toolchain_dir.join("_run_taskspec.sh").display()))
      .arg("--env").arg("PYTHONPATH=/_python")
      .arg("--env").arg("CI=1")
      .arg(format!("gup/{}", self.hash_digest))
      .arg("/entry.sh")
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
    ;
    let mut proc = cmd.spawn()
      .map_err(|_| fail("taskspec: failed to run `docker run`"))?;
    let (out, tasks) = if let Some(ref mut stdout) = proc.stdout {
      match _taskspecs(stdout, sysroot) {
        Err(e) => {
          proc.wait().ok();
          return Err(e);
        }
        Ok((out, tasks)) => (out, tasks),
      }
    } else {
      (Vec::new(), Vec::new())
    };
    if let Some(ref mut stderr) = proc.stderr {
      let mut buf = String::new();
      stderr.read_to_string(&mut buf).unwrap();
      if !(buf.is_empty() || buf == "\n") {
        proc.wait().ok();
        return Err(fail("taskspec: `docker run` returned nonempty stderr"));
      }
    }
    let status = proc.wait()
      .map_err(|_| fail("taskspec: failed to wait for `docker run`"))?;
    match status.success() {
      false => Err(fail("taskspec: `docker run` exited with nonzero status")),
      true  => Ok((out, tasks))
    }
  }

  pub fn _run_taskspec_direct(&self, gup_py_path: &PathBuf, sysroot: &Sysroot) -> Maybe<Vec<TaskSpec>> {
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir(sysroot);
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
      .arg("--volume").arg(format!("{}:/_python:ro", sysroot.base_dir.join("python3.6/site-packages").display()))
      .arg("--volume").arg(format!("{}:/gup.py:ro", gup_py_path.display()))
      .arg("--volume").arg(format!("{}:/entry.sh:ro", toolchain_dir.join("_run_taskspec_direct.sh").display()))
      .arg("--env").arg("PYTHONPATH=/_python")
      .arg("--env").arg("CI=1")
      .arg(format!("gup/{}", self.hash_digest))
      .arg("/entry.sh")
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
    ;
    let mut proc = cmd.spawn()
      .map_err(|_| fail("failed to run `docker run`"))?;
    let tasks = if let Some(ref mut stdout) = proc.stdout {
      match _taskspecs(stdout, sysroot) {
        Err(e) => {
          proc.wait().ok();
          return Err(e);
        }
        Ok((_, tasks)) => tasks,
      }
    } else {
      Vec::new()
    };
    if let Some(ref mut stderr) = proc.stderr {
      let mut buf = String::new();
      stderr.read_to_string(&mut buf).unwrap();
      if !(buf.is_empty() || buf == "\n") {
        println!("{}", buf);
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

  pub fn run(&self, checkout: &GitCheckoutSpec, task: &TaskSpec, sysroot: &Sysroot, output: Option<DockerOutput>) -> Maybe<DockerRunStatus> {
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir(sysroot);
    // FIXME
    //let distro_toolchain_dir = toolchain_dir.join(self.imagespec.distro_codename.to_desc_str());
    //eprintln!("TRACE: docker image: toolchain dir: {}", toolchain_dir.display());
    let mut task_file = NamedTempFile::new()
      .map_err(|_| fail("failed to create temporary script file"))?;
    {
      writeln!(task_file, "#!/bin/sh")
        .map_err(|_| fail("failed to write to script file"))?;
      writeln!(task_file, "set -u")
        .map_err(|_| fail("failed to write to script file"))?;
      writeln!(task_file, "set -x")
        .map_err(|_| fail("failed to write to script file"))?;
      writeln!(task_file, "set -o pipefail")
        .map_err(|_| fail("failed to write to script file"))?;
      if !task.allow_errors {
        writeln!(task_file, "set -e")
          .map_err(|_| fail("failed to write to script file"))?;
      }
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
      .arg("--volume").arg(format!("{}:/mutable_cache:ro", sysroot.base_dir.join("mutable_cache").display()))
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
      Some(DockerOutput::Buffer{buf_sz, consumer}) => {
        ConsoleMonitor::serialize_to_buffer(proc.stdout.take().unwrap(), proc.stderr.take().unwrap(), buf_sz, consumer)
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

  pub fn run_mut(&self, checkout: &GitCheckoutSpec, task: &TaskSpec, sysroot: &Sysroot, output: Option<DockerOutput>) -> Maybe<DockerRunStatus> {
    let toolchain_dir = self.imagespec.to_toolchain_docker_template_dir(sysroot);
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
      .arg("--volume").arg(format!("{}:/mutable_cache:ro", sysroot.base_dir.join("mutable_cache").display()))
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
      Some(DockerOutput::Buffer{buf_sz, consumer}) => {
        ConsoleMonitor::serialize_to_buffer(proc.stdout.take().unwrap(), proc.stderr.take().unwrap(), buf_sz, consumer)
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

fn _taskspecs<R: Read>(stdout: &mut R, sysroot: &Sysroot) -> Maybe<(Vec<u8>, Vec<TaskSpec>)> {
  let mut tasks = Vec::new();
  let mut task_builder: Option<TaskSpecBuilder> = None;
  let mut raw_out = Vec::with_capacity(4096);
  stdout.read_to_end(&mut raw_out)
    .map_err(|_| fail("failed to read gup.py output"))?;
  let mut cursor = Cursor::new(&raw_out);
  for line in cursor.lines() {
    let line = line.map_err(|_| fail("failed to understand gup.py output"))?;
    let line_toks: Vec<_> = line.splitn(2, "#-guppy:").collect();
    if line_toks.len() == 2 && line_toks[0].is_empty() {
      //eprintln!("DEBUG: directive? line toks: {:?}", line_toks);
      let directive_toks: Vec<_> = line_toks[1].splitn(2, ":").collect();
      match directive_toks[0] {
        "v0.mutable_cache" => {
          // FIXME: use `split_ascii_whitespace` as soon as stabilized:
          // https://github.com/rust-lang/rust/pull/58047
          let cache_toks: Vec<_> = directive_toks[1].split_whitespace().collect();
          match cache_toks[0] {
            "append" => {
              if cache_toks.len() <= 2 {
                return Err(fail("gup.py: v0.mutable_cache:append takes at least 2 arguments"));
              }
              let mut file_path = sysroot.base_dir.join("mutable_cache");
              for comp in PathBuf::from(cache_toks[1]).components() {
                match comp {
                  Component::Normal(c) => {
                    file_path.push(c);
                  }
                  _ => {
                    return Err(fail("gup.py: v0.mutable_cache:append: invalid path"));
                  }
                }
              }
              match cache_toks[2] {
                "fetch_only" => {
                  if cache_toks.len() <= 3 {
                    return Err(fail("gup.py: v0.mutable_cache:append: fetch_only missing url argument"));
                  }
                  match File::open(&file_path) {
                    Ok(_) => {}
                    Err(_) => {
                      let mut new_file = match File::create(&file_path) {
                        Err(_) => return Err(fail("gup.py: v0.mutable_cache:append: failed to open new file")),
                        Ok(f) => f,
                      };
                      let mut writer = BufWriter::new(new_file);
                      {
                        let mut headers = CurlList::new();
                        headers.append("Accept: application/octet-stream").unwrap();
                        let mut ez = CurlEasy::new();
                        ez.http_headers(headers).unwrap();
                        ez.follow_location(true).unwrap();
                        ez.url(cache_toks[3]).unwrap();
                        {
                          let mut xfer = ez.transfer();
                          xfer.write_function(|data| {
                            match writer.write_all(data) {
                              Err(e) => panic!("gup.py: v0.mutable_cache:append: fetch_once: write error: {:?}", e),
                              Ok(_) => {}
                            }
                            Ok(data.len())
                          }).unwrap();
                          xfer.perform().unwrap();
                        }
                      }
                    }
                  }
                }
                "copy_only" => {
                }
                "symlink_only" => {
                }
                _ => {}
              }
            }
            _ => return Err(fail("gup.py syntax error")),
          }
        }
        "v0.pre_run" | "v0.run_prelude" => {
          // TODO
        }
        "v0.post_run" => {
          // TODO
        }
        "task" => {
          panic!("gup.py syntax error: must specify a directive version");
        }
        "v0.task" => {
          // FIXME: use `split_ascii_whitespace` as soon as stabilized:
          // https://github.com/rust-lang/rust/pull/58047
          let task_toks: Vec<_> = directive_toks[1].split_whitespace().collect();
          match task_toks[0] {
            "begin" => {
              if task_builder.is_some() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              task_builder = Some(TaskSpecBuilder::default());
            }
            "end" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              let task_builder = task_builder.take().unwrap();
              tasks.push(task_builder.into_task()?);
            }
            "name" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:name takes 1 argument"));
              }
              let mut iter_state = 0;
              let mut iter = directive_toks[1].chars();
              loop {
                let c = iter.as_str().chars().next().unwrap();
                match iter_state {
                  0 => if c.is_ascii_whitespace() {
                    iter_state = 1;
                  },
                  1 => if !c.is_ascii_whitespace() {
                    iter_state = 2;
                    break;
                  },
                  _ => unreachable!(),
                }
                if iter.next().is_none() {
                  break;
                }
              }
              if iter_state != 2 {
                panic!("bug");
              }
              task_builder.as_mut().unwrap()
                .name = iter.as_str().to_string();
            }
            "toolchain" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:toolchain takes 1 argument"));
              }
              let toolchain = match Toolchain::from_desc_str_no_builtin(task_toks[1]) {
                Some(toolchain) => toolchain,
                None => return Err(fail("v0.task: unsupported toolchain")),
              };
              task_builder.as_mut().unwrap()
                .toolchain = Some(toolchain);
            }
            "require_docker" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:require_docker takes 1 argument"));
              }
              task_builder.as_mut().unwrap()
                .require_docker = task_toks[1].parse()
                  .map_err(|_| fail("v0.task:require_docker takes boolean argument"))?;
            }
            "require_nvidia_docker" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:require_nvidia_docker takes 1 argument"));
              }
              task_builder.as_mut().unwrap()
                .require_nvidia_docker = task_toks[1].parse()
                  .map_err(|_| fail("v0.task:require_nvidia_docker takes boolean argument"))?;
            }
            "require_distro" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
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
              task_builder.as_mut().unwrap()
                .require_distro = Some((ver, code));
            }
            "require_cuda" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
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
                  "6.5" => CudaVersionV0{major: 6, minor: 5},
                  "7.0" => CudaVersionV0{major: 7, minor: 0},
                  "7.5" => CudaVersionV0{major: 7, minor: 5},
                  "8.0" => CudaVersionV0{major: 8, minor: 0},
                  "9.0" => CudaVersionV0{major: 9, minor: 0},
                  "9.1" => CudaVersionV0{major: 9, minor: 1},
                  "9.2" => CudaVersionV0{major: 9, minor: 2},
                  "10.0" => CudaVersionV0{major: 10, minor: 0},
                  "10.1" => CudaVersionV0{major: 10, minor: 1},
                  _ => return Err(fail("v0.task: unsupported cuda version")),
                };
                Some(code)
              };
              task_builder.as_mut().unwrap()
                .require_cuda = Some((ver, maybe_code));
            }
            "require_gpu_arch" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:require_gpu_arch takes 1 argument"));
              }
              // TODO
              match task_toks[1] {
                "*" => {}
                _ => return Err(fail("gup.py syntax error")),
              }
            }
            "allow_errors" => {
              if task_builder.is_none() {
                // TODO: fail.
                return Err(fail("gup.py syntax error"));
              }
              if task_toks.len() <= 1 {
                return Err(fail("v0.task:allow_errors takes 1 argument"));
              }
              task_builder.as_mut().unwrap()
                .allow_errors = task_toks[1].parse()
                  .map_err(|_| fail("v0.task:allow_errors takes boolean argument"))?;
            }
            _ => return Err(fail("gup.py syntax error")),
          }
        }
        _ => return Err(fail("gup.py syntax error")),
      }
    } else {
      //eprintln!("DEBUG: sh? line toks: {:?}", line_toks);
      if task_builder.is_none() {
        return Err(fail("gup.py syntax error"));
      }
      task_builder.as_mut().unwrap()
        .sh.push(line);
    }
  }
  if task_builder.is_some() {
    return Err(fail("gup.py syntax error"));
  }
  Ok((raw_out, tasks))
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

  pub fn serialize_to_buffer<Stdout, Stderr>(stdout: Stdout, stderr: Stderr, buf_sz: usize, consumer: Box<Fn(u64, Vec<u8>) + Send>) -> MonitorJoin
  where Stdout: Read + Send + 'static, Stderr: Read + Send + 'static {
    // TODO
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
        let mut buf: Vec<u8> = Vec::with_capacity(buf_sz);
        let mut occ_sz: usize = 0;
        let mut part_nr: u64 = 1;
        loop {
          match mon_rx.recv() {
            Err(_) => break,
            Ok(line) => {
              buf.extend_from_slice(line.as_bytes());
              buf.push(b'\n');
              occ_sz += line.len() + 1;
              if occ_sz >= buf_sz {
                (consumer)(part_nr, buf.clone());
                buf.clear();
                occ_sz = 0;
                part_nr += 1;
              }
            }
          }
        }
        if occ_sz > 0 {
          (consumer)(part_nr, buf.clone());
          buf.clear();
          occ_sz = 0;
          part_nr += 1;
        }
        assert_eq!(0, occ_sz);
      }),
    ];
    MonitorJoin{joins}
  }
}
