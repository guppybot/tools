use self::config_toml::{Api as ApiToml, Config as ConfigToml};

use crate::query::{Maybe, fail};

use std::path::{Path, PathBuf};

mod config_toml {
  use crate::query::{Maybe, fail};

  use std::fs::{File};
  use std::io::{Read, BufReader};
  use std::os::unix::fs::{PermissionsExt};
  use std::path::{Path};

  #[derive(Debug, Default, Deserialize)]
  pub struct ApiAuth {
    pub api_key: Option<String>,
    pub secret_token: Option<String>,
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct Api {
    pub auth: Option<ApiAuth>,
  }

  impl Api {
    pub fn open(path: &Path) -> Maybe<Api> {
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
  pub struct Machine {
    pub devices: Option<Vec<String>>,
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct Repos {
    pub global: Option<GlobalRepoConfig>,
    pub github: Option<Vec<GithubRepo>>,
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct GlobalRepoConfig {
    pub enable_cache: Option<bool>,
    pub cache_enabled_repos: Option<Vec<String>>,
  }

  #[derive(Debug, Default, Deserialize, Clone)]
  pub struct GithubRepo {
    pub url: String,
    pub commits: Option<String>,
    pub prs: Option<String>,
    allowed_users: Option<Vec<String>>,
    blocked_users: Option<Vec<String>>,
  }

  #[derive(Debug, Default, Deserialize)]
  pub struct Config {
    pub machine: Option<Machine>,
    pub repos: Option<Repos>,
  }

  impl Config {
    pub fn open(path: &Path) -> Maybe<Config> {
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
  pub api_key: String,
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
      api_key: auth.api_key.ok_or_else(|| fail("api config: auth: missing api_key"))?,
      secret_token: auth.secret_token.ok_or_else(|| fail("api config: auth: missing secret_token"))?,
    };
    Ok(ApiConfig{
      auth,
    })
  }
}

#[derive(Debug)]
pub struct Config {
  pub machine: Machine,
  pub repos: Repos,
}

#[derive(Debug)]
pub struct Machine {
  pub devices: Vec<Device>,
}

#[derive(Debug)]
pub enum Device {
  PciSlot(String),
  //Uuid(String),
}

#[derive(Debug)]
pub struct Repos {
  pub enable_cache: bool,
  pub cache_enabled_repos: Vec<String>,
  pub gh_repos: Vec<GithubRepo>,
}

#[derive(Debug)]
pub struct GithubRepo {
  pub url: String,
  /*pub commits: Option<String>,
  pub prs: Option<String>,
  allowed_users: Option<Vec<String>>,
  blocked_users: Option<Vec<String>>,*/
}

impl Config {
  pub fn open_default() -> Maybe<Config> {
    let default_path = PathBuf::from("/etc/guppybot/config");
    Config::open(&default_path)
  }

  pub fn open(path: &Path) -> Maybe<Config> {
    let cfg = ConfigToml::open(path)?;
    let machine = cfg.machine.unwrap_or_default();
    let machine = Machine{
      devices: machine.devices.unwrap_or_default()
        .iter().map(|dev_str| Device::PciSlot(dev_str.to_string()))
        .collect(),
    };
    let repos = Repos{
      enable_cache: cfg.repos.as_ref()
        .and_then(|repos| repos.global.as_ref())
        .and_then(|global| global.enable_cache)
        .unwrap_or_else(|| false),
      cache_enabled_repos: cfg.repos.as_ref()
        .and_then(|repos| repos.global.as_ref())
        .and_then(|global| global.cache_enabled_repos.clone())
        .unwrap_or_default(),
      gh_repos: {
        let gh_repos = cfg.repos.as_ref()
          .and_then(|repos| repos.github.clone())
          .unwrap_or_default();
        gh_repos.into_iter().map(|gh_repo| {
          GithubRepo{url: gh_repo.url}
        }).collect()
      },
    };
    Ok(Config{
      machine,
      repos,
    })
  }
}
