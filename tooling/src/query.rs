use schemas::v1::*;

use std::fmt::{Debug};
use std::ffi::{OsStr};
use std::fs::{File};
use std::io::{BufRead, BufReader, Cursor};
use std::path::{PathBuf};
use std::process::{Command};
use std::str::{from_utf8};

#[derive(PartialEq, Eq, Debug)]
pub struct Failure {
  pub excuses: Vec<String>,
}

impl Default for Failure {
  fn default() -> Failure {
    Failure{excuses: Vec::new()}
  }
}

impl Failure {
  pub fn new<S: Into<String>>(msg: S) -> Failure {
    Failure{excuses: vec![msg.into()]}
  }

  pub fn push<S: Into<String>>(mut self, msg: S) -> Failure {
    self.excuses.push(msg.into());
    self
  }
}

pub fn fail<S: Into<String>>(msg: S) -> Failure {
  Failure::new(msg)
}

pub type Maybe<T=()> = Result<T, Failure>;

pub fn quorum<T: Eq + Debug>(mut xs: Vec<Maybe<T>>) -> Maybe<T> {
  let mut x0 = None;
  for x in xs.drain(..) {
    match x0 {
      None => x0 = Some(x),
      Some(ref x0) => if x.is_ok() && x0 != &x {
        return Err(fail(format!("disagreement: `{:?}` vs `{:?}`", x0, x)));
      },
    }
  }
  x0.unwrap_or_else(|| Err(fail("BUG: empty quorum")))
}

pub fn which<S: AsRef<OsStr>>(cmd: S) -> Maybe<PathBuf> {
  let output = Command::new("which").arg(cmd).output()
    .map_err(|_| fail("failed to run `which`"))?;
  let out = from_utf8(&output.stdout).map_err(|_| fail("output of `which` is not utf8"))?;
  if out.is_empty() {
    Err(fail("path not found"))
  } else {
    Ok(PathBuf::from(out))
  }
}

pub trait Query {
  fn query() -> Maybe<Self> where Self: Sized;
}

//pub type DistroId = DistroIdV0;
//pub type DistroCodename = DistroCodenameV0;
//pub type DistroInfo = DistroInfoV0;

fn query_distro_id_lsb_release() -> Maybe<DistroIdV0> {
  let output = Command::new("lsb_release").arg("-is").output()
    .map_err(|_| fail("failed to run `lsb_release -is`"))?;
  if !output.status.success() {
    return Err(fail(format!("`lsb_release` failed with exit status {:?}", output.status.code())));
  }
  match from_utf8(&output.stdout) {
    Ok("debian\n") => Ok(DistroIdV0::Debian),
    Ok("ubuntu\n") => Ok(DistroIdV0::Ubuntu),
    _ => Err(fail(format!("unsupported `lsb_release` output"))),
  }
}

fn query_distro_id_os_release() -> Maybe<DistroIdV0> {
  let file = File::open("/etc/os-release")
    .map_err(|_| fail("failed to open /etc/os-release"))?;
  let mut reader = BufReader::new(file);
  let mut line = String::new();
  loop {
    line.clear();
    reader.read_line(&mut line)
      .map_err(|_| fail("failed to read /etc/os-release"))?;
    if line.is_empty() {
      break;
    }
    if line.contains("CentOS") {
      return Ok(DistroIdV0::Centos);
    } else if line.contains("Debian") {
      return Ok(DistroIdV0::Debian);
    } else if line.contains("Fedora") {
      return Ok(DistroIdV0::Fedora);
    } else if line.contains("Red Hat") {
      return Ok(DistroIdV0::RedHat);
    } else if line.contains("Ubuntu") {
      return Ok(DistroIdV0::Ubuntu);
    }
  }
  Err(fail("unsupported or missing /etc/os-release"))
}

impl Query for DistroIdV0 {
  fn query() -> Maybe<DistroIdV0> {
    query_distro_id_lsb_release()
      .or_else(|_| query_distro_id_os_release())
  }
}

impl Query for DistroCodenameV0 {
  fn query() -> Maybe<DistroCodenameV0> {
    let output = Command::new("lsb_release").arg("-cs").output()
      .map_err(|_| fail("failed to run `lsb_release -cs`"))?;
    if !output.status.success() {
      return Err(fail(format!("`lsb_release` failed with exit status {:?}", output.status.code())));
    }
    match from_utf8(&output.stdout) {
      Ok("wheezy\n") => Ok(DistroCodenameV0::DebianWheezy),
      Ok("jessie\n") => Ok(DistroCodenameV0::DebianJessie),
      Ok("stretch\n") => Ok(DistroCodenameV0::DebianStretch),
      Ok("buster\n") => Ok(DistroCodenameV0::DebianBuster),
      Ok("trusty\n") => Ok(DistroCodenameV0::UbuntuTrusty),
      Ok("xenial\n") => Ok(DistroCodenameV0::UbuntuXenial),
      Ok("bionic\n") => Ok(DistroCodenameV0::UbuntuBionic),
      x => return Err(fail(format!("`lsb_release` returned unsupported output: {:?}", x))),
    }
  }
}

impl Query for DistroInfoV0 {
  fn query() -> Maybe<DistroInfoV0> {
    let id = DistroIdV0::query()?;
    let codename = match id {
      DistroIdV0::Debian |
      DistroIdV0::Ubuntu => {
        Some(DistroCodenameV0::query()?)
      }
      _ => None,
    };
    Ok(DistroInfoV0{
      id,
      codename,
    })
  }
}

//pub type DriverVersion = DriverVersionV0;

impl Query for DriverVersionV0 {
  fn query() -> Maybe<DriverVersionV0> {
    let file = File::open("/proc/driver/nvidia/version")
      .map_err(|_| fail("failed to open /proc/driver/nvidia/version"))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
      line.clear();
      reader.read_line(&mut line)
        .map_err(|_| fail("failed to read /proc/driver/nvidia/version"))?;
      if line.is_empty() {
        break;
      }
      if line.contains("NVRM version:") {
        for tok in line.split_whitespace() {
          let toks2: Vec<_> = tok.split(".").collect();
          if toks2.len() == 2 {
            match (toks2[0].parse::<u32>(), toks2[1].parse::<u32>()) {
              (Ok(major), Ok(minor)) => {
                return Ok(DriverVersionV0{major, minor});
              }
              _ => {}
            }
          }
        }
      }
    }
    Err(fail("no version in /proc/driver/nvidia/version"))
  }
}

//pub type PciRecord = PciRecordV0;
//pub type Gpus = GpusV0;

impl Query for GpusV0 {
  fn query() -> Maybe<GpusV0> {
    let output = Command::new("lspci").arg("-vmmn").output()
      .map_err(|_| fail("failed to run `lspci`"))?;
    if !output.status.success() {
      return Err(fail(format!("`lspci` failed with exit status {:?}", output.status.code())));
    }
    let mut records = Vec::new();
    let mut record = PciRecordV0::default();
    for line in Cursor::new(output.stdout).lines() {
      let line = line.unwrap();
      if line.is_empty() {
        if record.is_gpu() {
          records.push(record.clone());
        }
        continue;
      }
      let mut line_parts = line.splitn(2, '\t');
      match line_parts.next() {
        Some("Slot:") => {
          record = PciRecordV0::default();
          let slot_parts: Vec<_> = line_parts.next().unwrap().splitn(3, ":").collect();
          match slot_parts.len() {
            2 => {
              record.slot.domain = None;
              record.slot.bus = u8::from_str_radix(slot_parts[0], 16).unwrap();
              let dev_func_parts: Vec<_> = slot_parts[1].splitn(2, ".").collect();
              record.slot.device = u8::from_str_radix(dev_func_parts[0], 16).unwrap();
              record.slot.function = u8::from_str_radix(dev_func_parts[1], 16).unwrap();
            }
            3 => {
              record.slot.domain = Some(u32::from_str_radix(slot_parts[0], 16).unwrap());
              record.slot.bus = u8::from_str_radix(slot_parts[1], 16).unwrap();
              let dev_func_parts: Vec<_> = slot_parts[2].splitn(2, ".").collect();
              record.slot.device = u8::from_str_radix(dev_func_parts[0], 16).unwrap();
              record.slot.function = u8::from_str_radix(dev_func_parts[1], 16).unwrap();
            }
            _ => panic!(),
          }
        }
        Some("Class:") => {
          match u16::from_str_radix(line_parts.next().unwrap(), 16) {
            Ok(class) => {
              record.class = class;
            }
            _ => {}
          }
        }
        Some("Vendor:") => {
          match u16::from_str_radix(line_parts.next().unwrap(), 16) {
            Ok(vendor) => {
              record.vendor = vendor;
            }
            _ => {}
          }
        }
        Some("Device:") => {
          match u16::from_str_radix(line_parts.next().unwrap(), 16) {
            Ok(device) => {
              record.device = device;
            }
            _ => {}
          }
        }
        Some("SVendor:") => {
          match u16::from_str_radix(line_parts.next().unwrap(), 16) {
            Ok(svendor) => {
              record.svendor = Some(svendor);
            }
            _ => {}
          }
        }
        Some("SDevice:") => {
          match u16::from_str_radix(line_parts.next().unwrap(), 16) {
            Ok(sdevice) => {
              record.sdevice = Some(sdevice);
            }
            _ => {}
          }
        }
        Some("Rev:") => {
          match u8::from_str_radix(line_parts.next().unwrap(), 16) {
            Ok(rev) => {
              record.rev = Some(rev);
            }
            _ => {}
          }
        }
        _ => {}
      }
    }
    Ok(GpusV0{pci_records: records})
  }
}

//pub type SystemSetup = SystemSetupV0;

impl Query for SystemSetupV0 {
  fn query() -> Maybe<SystemSetupV0> {
    Ok(SystemSetupV0{
      distro_info: DistroInfoV0::query()?,
      driver_version: DriverVersionV0::query()?,
      num_cpus: num_cpus::get_physical(),
      gpus: GpusV0::query()?,
    })
  }
}
