use self::config_toml::{
  ApiConfig as ApiToml,
  MachineConfig as MachineToml,
  CiConfig as CiToml,
};

use crate::query::{Maybe, Query, fail};

use schemas::v1::{
  GpusV0,
  MachineConfigV0, LocalMachineV0, LocalDeviceV0,
  CiConfigV0, CiRepoV0, CiEventPolicyV0, UserHandleV0, UserDomainV0,
};
use url::{Url};

use std::fs::{File, create_dir_all};
use std::io::{Write, BufWriter};
use std::path::{Path, PathBuf};

mod config_toml {
  use crate::query::{Maybe, fail};

  use std::fs::{File};
  use std::io::{Read, BufReader};
  use std::os::unix::fs::{PermissionsExt};
  use std::path::{Path};

  #[derive(Debug, Default, Deserialize)]
  pub struct ApiAuth {
    pub api_id: Option<String>,
    pub secret_token: Option<String>,
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct ApiConfig {
    pub auth: Option<ApiAuth>,
  }

  impl ApiConfig {
    pub fn open(path: &Path) -> Maybe<ApiConfig> {
      let file = match File::open(path) {
        Err(_) => return Err(fail("failed to open api config")),
        Ok(f) => f,
      };
      let meta = match file.metadata() {
        Err(_) => return Err(fail("failed to get api config metadata")),
        Ok(m) => m,
      };
      match meta.permissions().mode() & 0o600 {
        0o400 | 0o600 => {}
        _ => return Err(fail("api config: file permissions are too open")),
      }
      let mut text = String::new();
      let mut reader = BufReader::new(file);
      match reader.read_to_string(&mut text) {
        Err(_) => return Err(fail("failed to read api config")),
        Ok(_) => {}
      }
      match toml::from_str(&text) {
        Err(e) => Err(fail(format!("api config is not valid toml: {:?}", e))),
        Ok(x) => Ok(x),
      }
    }
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct LocalMachine {
    pub parallel_tasks: Option<u32>,
    pub gpus: Option<Vec<String>>,
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct MachineConfig {
    pub local_machine: Option<LocalMachine>,
  }

  impl MachineConfig {
    pub fn open(path: &Path) -> Maybe<MachineConfig> {
      let file = match File::open(path) {
        Err(_) => return Err(fail("failed to open config file")),
        Ok(f) => f,
      };
      let mut text = String::new();
      let mut reader = BufReader::new(file);
      match reader.read_to_string(&mut text) {
        Err(_) => return Err(fail("failed to read config file")),
        Ok(_) => {}
      }
      match toml::from_str(&text) {
        Err(e) => Err(fail(format!("config file is not valid toml: {:?}", e))),
        Ok(x) => Ok(x),
      }
    }
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct CiRepo {
    pub remote_url: Option<String>,
    pub commit_policy: Option<String>,
    pub pr_policy: Option<String>,
    pub allowed_users: Option<Vec<String>>,
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct CiConfig {
    pub repos: Option<Vec<CiRepo>>,
  }

  impl CiConfig {
    pub fn open(path: &Path) -> Maybe<CiConfig> {
      let file = match File::open(path) {
        Err(_) => return Err(fail("failed to open config file")),
        Ok(f) => f,
      };
      let mut text = String::new();
      let mut reader = BufReader::new(file);
      match reader.read_to_string(&mut text) {
        Err(_) => return Err(fail("failed to read config file")),
        Ok(_) => {}
      }
      match toml::from_str(&text) {
        Err(e) => Err(fail(format!("config file is not valid toml: {:?}", e))),
        Ok(x) => Ok(x),
      }
    }
  }
}

#[derive(Debug)]
pub struct ApiAuth {
  pub api_id: String,
  pub secret_token: String,
}

#[derive(Debug)]
pub struct ApiConfig {
  pub auth: ApiAuth,
}

impl ApiConfig {
  pub fn open_default() -> Maybe<ApiConfig> {
    let default_path = PathBuf::from("/etc/guppybot/api");
    ApiConfig::open(&default_path)
  }

  pub fn open(path: &Path) -> Maybe<ApiConfig> {
    let api = ApiToml::open(path)?;
    let auth = api.auth.unwrap_or_default();
    let auth = ApiAuth{
      api_id: auth.api_id.ok_or_else(|| fail("api config: auth: missing api_id"))?,
      secret_token: auth.secret_token.ok_or_else(|| fail("api config: auth: missing secret_token"))?,
    };
    Ok(ApiConfig{
      auth,
    })
  }
}

impl Query for MachineConfigV0 {
  fn query() -> Maybe<MachineConfigV0> {
    let cfg = MachineToml::open(&PathBuf::from("/etc/guppybot/machine"))?;
    let local_machine = cfg.local_machine.unwrap_or_default();
    let local_machine = LocalMachineV0{
      parallel_tasks: local_machine.parallel_tasks.unwrap_or_else(|| 1),
      gpus: local_machine.gpus.unwrap_or_default()
        .iter().map(|dev_str| LocalDeviceV0::PciSlot(dev_str.to_string()))
        .collect(),
    };
    Ok(MachineConfigV0{
      local_machine,
    })
  }
}

impl Query for CiConfigV0 {
  fn query() -> Maybe<CiConfigV0> {
    let cfg = CiToml::open(&PathBuf::from("/etc/guppybot/ci"))?;
    let mut repos = Vec::new();
    for repo in cfg.repos.unwrap_or_default().iter() {
      let remote_url = Url::parse(
          repo.remote_url.as_ref()
            .ok_or_else(|| fail("ci repo: missing remote url"))?
      )
        .map_err(|_| fail("ci repo: error parsing remote url"))?
        .into_string();
      let commit_policy = match repo.commit_policy.as_ref().map(|s| s.as_str()) {
        Some("nobody") => Ok(CiEventPolicyV0::Nobody),
        Some("allowed_users") => Ok(CiEventPolicyV0::AllowedUsers),
        Some("everybody_except_ci_changes") => Ok(CiEventPolicyV0::EverybodyExceptCiChanges),
        Some("everybody") => Ok(CiEventPolicyV0::Everybody),
        _ => Err(fail("ci repo: error parsing commit event policy")),
      }?;
      let pr_policy = match repo.pr_policy.as_ref().map(|s| s.as_str()) {
        Some("nobody") => Ok(CiEventPolicyV0::Nobody),
        Some("allowed_users") => Ok(CiEventPolicyV0::AllowedUsers),
        Some("everybody_except_ci_changes") => Ok(CiEventPolicyV0::EverybodyExceptCiChanges),
        Some("everybody") => Ok(CiEventPolicyV0::Everybody),
        _ => Err(fail("ci repo: error parsing pr event policy")),
      }?;
      let empty = Vec::new();
      let mut allowed_users = Vec::new();
      for user_str in repo.allowed_users.as_ref().unwrap_or_else(|| &empty).iter() {
        let user_toks: Vec<_> = user_str.splitn(2, ":").collect();
        allowed_users.push(match user_toks.len() {
          0 => return Err(fail("ci repo: invalid user format")),
          1 => UserHandleV0{
            username: user_toks[0].to_string(),
            domain: UserDomainV0::GuppybotOrg,
          },
          2 => UserHandleV0{
            username: user_toks[0].to_string(),
            domain: match user_toks[1] {
              "guppybot.org" => Ok(UserDomainV0::GuppybotOrg),
              "github.com" => Ok(UserDomainV0::GithubCom),
              _ => Err(fail("ci repo: currently unsupported user domain")),
            }?,
          },
          _ => unreachable!(),
        });
      }
      repos.push(CiRepoV0{
        remote_url,
        commit_policy,
        pr_policy,
        allowed_users,
      });
    }
    Ok(CiConfigV0{
      repos,
    })
  }
}

pub struct Config {
  pub config_dir: PathBuf,
}

impl Default for Config {
  fn default() -> Config {
    Config{
      config_dir: PathBuf::from("/etc/guppybot"),
    }
  }
}

impl Config {
  pub fn install_default(&self, gpus: &GpusV0) -> Maybe {
    create_dir_all(&self.config_dir)
      .map_err(|_| fail("failed to create configuration directory"))?;

    let _ = File::open(self.config_dir.join("api"))
      .or_else(|_| {
        let mut api_file = File::create(self.config_dir.join("api"))
          .map_err(|_| fail("failed to create api config file"))?;
        {
          let mut api_writer = BufWriter::new(&mut api_file);
          writeln!(&mut api_writer, "# automatically generated by guppybot")
            .map_err(|_| fail("failed to write to api config file"))?;
          writeln!(&mut api_writer, "")
            .map_err(|_| fail("failed to write to api config file"))?;
          writeln!(&mut api_writer, "[auth]")
            .map_err(|_| fail("failed to write to api config file"))?;
          writeln!(&mut api_writer, "api_id = \"YOUR_API_ID\"")
            .map_err(|_| fail("failed to write to api config file"))?;
          writeln!(&mut api_writer, "secret_token = \"YOUR_SECRET_TOKEN\"")
            .map_err(|_| fail("failed to write to api config file"))?;
        }
        Ok(api_file)
      })?;

    let _ = File::open(self.config_dir.join("machine"))
      .or_else(|_| {
        let mut machine_file = File::create(self.config_dir.join("machine"))
          .map_err(|_| fail("failed to create machine config file"))?;
        {
          let mut machine_writer = BufWriter::new(&mut machine_file);
          writeln!(&mut machine_writer, "# automatically generated by guppybot")
            .map_err(|_| fail("failed to write to machine config file"))?;
          writeln!(&mut machine_writer, "")
            .map_err(|_| fail("failed to write to machine config file"))?;
          writeln!(&mut machine_writer, "[local_machine]")
            .map_err(|_| fail("failed to write to machine config file"))?;
          write!(&mut machine_writer, "gpus = [")
            .map_err(|_| fail("failed to write to machine config file"))?;
          for (record_nr, record) in gpus.pci_records.iter().enumerate() {
            match record.slot.domain {
              Some(domain) => {
                write!(&mut machine_writer, "\"{:08x}:{:02x}:{:02x}.{:02x}\"",
                    domain, record.slot.bus, record.slot.device, record.slot.function)
                  .map_err(|_| fail("failed to write to machine config file"))?;
              }
              None => {
                write!(&mut machine_writer, "\"{:02x}:{:02x}.{:02x}\"",
                    record.slot.bus, record.slot.device, record.slot.function)
                  .map_err(|_| fail("failed to write to machine config file"))?;
              }
            }
            if record_nr < gpus.pci_records.len() - 1 {
              write!(&mut machine_writer, ", ")
                .map_err(|_| fail("failed to write to machine config file"))?;
            }
          }
          writeln!(&mut machine_writer, "]")
            .map_err(|_| fail("failed to write to machine config file"))?;
        }
        Ok(machine_file)
      })?;

    Ok(())
  }
}
