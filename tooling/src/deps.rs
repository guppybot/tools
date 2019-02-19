use crate::query::{Maybe, fail};

use schemas::v1::{
  DistroIdV0::*,
  DistroCodenameV0::*,
  DistroInfoV0,
};

use std::fs::{File};
use std::io::{Write};
use std::process::{Command, Stdio};
use std::str::{from_utf8};

pub enum Pkg {
  Deb(String),
}

pub struct DockerDeps {
  pub missing_pkgs: Vec<Pkg>,
}

fn query_deb<S: AsRef<str>>(deb_name: S) -> Maybe<bool> {
  let output = Command::new("dpkg-query").arg("-W").arg(deb_name.as_ref()).output()
    .map_err(|_| fail("failed to run `dpkg-query`"))?;
  if !output.status.success() {
    return Err(fail(format!("`dpkg-query` failed with exit status {:?}", output.status.code())));
  }
  let out = from_utf8(&output.stdout)
    .map_err(|_| fail("output of `dpkg-query` is not utf-8"))?;
  let parts: Vec<_> = out.splitn(2, "\t").collect();
  match parts.len() {
    0 => Ok(false),
    _ => {
      // FIXME: the first token can also include the arch, e.g. "amd64".
      if parts[0] == deb_name.as_ref() {
        Ok(true)
      } else {
        Err(fail(format!("`dpkg-query` returned an unexpected package: '{}'", parts[0])))
      }
    }
  }
}

fn add_deb_if_missing<S: AsRef<str>>(missing_pkgs: &mut Vec<Pkg>, deb_name: S) -> Maybe {
  if query_deb(&deb_name)? {
    missing_pkgs.push(Pkg::Deb(deb_name.as_ref().to_owned()));
  }
  Ok(())
}

impl DockerDeps {
  fn check_debian(_distro_info: &DistroInfoV0) -> Maybe<DockerDeps> {
    let mut missing_pkgs = Vec::new();
    add_deb_if_missing(&mut missing_pkgs, "apt-transport-https")?;
    add_deb_if_missing(&mut missing_pkgs, "ca-certificates")?;
    add_deb_if_missing(&mut missing_pkgs, "curl")?;
    add_deb_if_missing(&mut missing_pkgs, "gnupg2")?;
    Ok(DockerDeps{missing_pkgs})
  }

  fn check_ubuntu() -> Maybe<DockerDeps> {
    Err(fail("TODO: docker dependencies on ubuntu"))
  }

  pub fn check(distro_info: &DistroInfoV0) -> Maybe<DockerDeps> {
    match distro_info.id {
      Debian => DockerDeps::check_debian(distro_info),
      Ubuntu => DockerDeps::check_ubuntu(),
      _ => Err(fail("docker dependencies: unsupported distro")),
    }
  }

  pub fn install_missing(self) -> Maybe {
    for pkg in self.missing_pkgs.iter() {
      match pkg {
        &Pkg::Deb(ref deb_name) => {
          let output = Command::new("apt-get").arg("install").arg("-y").arg(deb_name).output()
            .map_err(|_| fail("failed to run `apt-get`"))?;
          if !output.status.success() {
            return Err(fail(format!("`apt-get` failed with exit status: {:?}", output.status.code())));
          }
        }
      }
    }
    Ok(())
  }
}

pub struct Docker;

impl Docker {
  pub fn check(distro_info: &DistroInfoV0) -> Maybe<bool> {
    match distro_info.id {
      Debian => query_deb("docker-ce"),
      _ => Err(fail("install nvidia-docker2: unsupported distro")),
    }
  }

  fn install_debian(distro_info: &DistroInfoV0) -> Maybe {
    let curl_cmd = Command::new("curl")
      .arg("-fsSL")
      .arg("https://download.docker.com/linux/debian/gpg")
      .stdout(Stdio::piped())
      .spawn()
      .map_err(|_| fail("failed to run `curl`"))?;
    let output = Command::new("apt-key").arg("add").arg("-")
      .stdin(curl_cmd.stdout.unwrap())
      .output()
      .map_err(|_| fail("failed to run `apt-key`"))?;
    if !output.status.success() {
      return Err(fail(format!("`apt-key` failed with exit status: {:?}", output.status.code())));
    }
    {
      let debian_codename = match distro_info.codename {
        Some(DebianWheezy) => "wheezy",
        Some(DebianJessie) => "jessie",
        Some(DebianStretch) => "stretch",
        Some(DebianBuster) => "buster",
        _ => panic!("bug"),
      };
      let mut apt_source_file = File::create("/etc/apt/sources.list.d/guppybot_docker.list")
        .map_err(|_| fail("failed to create apt source list file"))?;
      writeln!(&mut apt_source_file)
        .and_then(|_| writeln!(&mut apt_source_file, "# automatically added by `guppyctl install`"))
        .and_then(|_| writeln!(&mut apt_source_file, "deb [arch=amd64] https://download.docker.com/linux/debian {} stable", debian_codename))
        .map_err(|_| fail("failed to write to apt source list file"))?;
    }
    let output = Command::new("apt-get").arg("update").output()
      .map_err(|_| fail("failed to run `apt-get update`"))?;
    if !output.status.success() {
      return Err(fail(format!("`apt-get update` failed with exit status: {:?}", output.status.code())));
    }
    let output = Command::new("apt-get").arg("install").arg("-y").arg("docker-ce").output()
      .map_err(|_| fail("failed to run `apt-get install`"))?;
    if !output.status.success() {
      return Err(fail(format!("`apt-get install` failed with exit status: {:?}", output.status.code())));
    }
    Ok(())
  }

  fn install_ubuntu() -> Maybe {
    Err(fail("TODO: install docker on ubuntu"))
  }

  pub fn install(distro_info: &DistroInfoV0) -> Maybe {
    match distro_info.id {
      Debian => Docker::install_debian(distro_info),
      Ubuntu => Docker::install_ubuntu(),
      _ => Err(fail("install docker: unsupported distro")),
    }
  }
}

pub struct NvidiaDocker2;

impl NvidiaDocker2 {
  pub fn check(distro_info: &DistroInfoV0) -> Maybe<bool> {
    match distro_info.id {
      Debian => query_deb("nvidia-docker2"),
      _ => Err(fail("install nvidia-docker2: unsupported distro")),
    }
  }

  fn install_debian(distro_info: &DistroInfoV0) -> Maybe {
    let curl_cmd = Command::new("curl")
      .arg("-fsSL")
      .arg("https://nvidia.github.io/nvidia-docker/gpgkey")
      .stdout(Stdio::piped())
      .spawn()
      .map_err(|_| fail("failed to run `curl`"))?;
    let output = Command::new("apt-key").arg("add").arg("-")
      .stdin(Stdio::from(curl_cmd.stdout.unwrap()))
      .output()
      .map_err(|_| fail("failed to run `apt-key`"))?;
    if !output.status.success() {
      return Err(fail(format!("`apt-key` failed with exit status: {:?}", output.status.code())));
    }
    let debian_version = match distro_info.codename {
      Some(DebianWheezy) => {
        return Err(fail("wheezy not supported by nvidia-docker"));
      }
      Some(DebianBuster) => {
        return Err(fail("buster not supported by nvidia-docker"));
      }
      Some(DebianJessie) => "8",
      Some(DebianStretch) => "9",
      _ => panic!("bug"),
    };
    let curl_cmd = Command::new("curl")
      .arg("-fsSL")
      .arg(format!("https://nvidia.github.io/nvidia-docker/debian{}/nvidia-docker.list", debian_version))
      .stdout(Stdio::piped())
      .spawn()
      .map_err(|_| fail("failed to run `curl`"))?;
    let output = Command::new("tee").arg("/etc/apt/sources.list.d/guppybot_nvidia-docker.list")
      .stdin(Stdio::from(curl_cmd.stdout.unwrap()))
      .output()
      .map_err(|_| fail("failed to run `tee`"))?;
    if !output.status.success() {
      return Err(fail(format!("`tee` failed with exit status: {:?}", output.status.code())));
    }
    let output = Command::new("apt-get").arg("update").output()
      .map_err(|_| fail("failed to run `apt-get update`"))?;
    if !output.status.success() {
      return Err(fail(format!("`apt-get update` failed with exit status: {:?}", output.status.code())));
    }
    // TODO: nvidia-docker2 installation may overwrite "/etc/docker/daemon.json",
    // save it somewhere before installing.
    // TODO: need to pin nvidia-docker2 to the docker-ce version.
    let output = Command::new("apt-get").arg("install").arg("-y").arg("nvidia-docker2").output()
      .map_err(|_| fail("failed to run `apt-get install`"))?;
    if !output.status.success() {
      return Err(fail(format!("`apt-get install` failed with exit status: {:?}", output.status.code())));
    }
    Ok(())
  }

  fn install_ubuntu() -> Maybe {
    Err(fail("TODO: install nvidia-docker2 on ubuntu"))
  }

  pub fn install(distro_info: &DistroInfoV0) -> Maybe {
    match distro_info.id {
      Debian => NvidiaDocker2::install_debian(distro_info),
      Ubuntu => NvidiaDocker2::install_ubuntu(),
      _ => Err(fail("install nvidia-docker2: unsupported distro")),
    }
  }
}
