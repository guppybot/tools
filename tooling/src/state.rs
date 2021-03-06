use crate::assets::{SYSROOT_TAR_GZ};
use crate::config::{ApiAuth};
use crate::docker::{DockerImage};
use crate::query::{Maybe, fail};

use byteorder::{ReadBytesExt, WriteBytesExt};
use monosodium::{generic_hash};
use monosodium::util::{CryptoBuf};
use schemas::v1::{
  CudaVersionV0,
  DistroIdV0::{self, *},
  DistroCodenameV0::{self, *},
};

use std::fmt::{Write as FmtWrite};
use std::fs::{File, OpenOptions, Permissions, create_dir_all, set_permissions};
use std::io::{BufRead, Read, Seek, Write, BufReader, BufWriter, SeekFrom};
use std::os::unix::fs::{PermissionsExt};
use std::path::{PathBuf};
use std::process::{Command};

pub struct Index {
}

impl Index {
  pub fn load(sysroot: &Sysroot) -> Maybe<Index> {
    // TODO
    unimplemented!();
  }

  pub fn clone(sysroot: &Sysroot) -> Maybe<Index> {
    // TODO
    unimplemented!();
  }

  pub fn update(&self) -> Maybe {
    // TODO
    unimplemented!();
  }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Toolchain {
  //Custom(String),
  Builtin,
  Default,
  Python2,
  Python3,
  RustNightly,
}

impl Toolchain {
  pub fn from_desc_str_nocustom(s: &str) -> Option<Toolchain> {
    match s {
      "_builtin" => Some(Toolchain::Builtin),
      "default" => Some(Toolchain::Default),
      "python2" => Some(Toolchain::Python2),
      "python3" => Some(Toolchain::Python3),
      "rust_nightly" => Some(Toolchain::RustNightly),
      _ => None,
    }
  }

  pub fn from_desc_str_no_builtin(s: &str) -> Option<Toolchain> {
    match Toolchain::from_desc_str_nocustom(s) {
      Some(Toolchain::Builtin) | None => None,
      Some(t) => Some(t),
    }
  }

  pub fn to_desc_string(&self) -> String {
    match self {
      //&Toolchain::Custom(ref s) => s,
      &Toolchain::Builtin => "_builtin",
      &Toolchain::Default => "default",
      &Toolchain::Python2 => "python2",
      &Toolchain::Python3 => "python3",
      &Toolchain::RustNightly => "rust_nightly",
    }.to_string()
  }
}

#[derive(Default)]
pub struct ImageSpecBuilder {
  pub cuda: Option<CudaVersionV0>,
  pub distro_codename: Option<DistroCodenameV0>,
  pub distro_id: Option<DistroIdV0>,
  pub docker: bool,
  pub nvidia_docker: bool,
  pub toolchain: Option<Toolchain>,
}

impl ImageSpecBuilder {
  fn into_imagespec(self) -> Maybe<ImageSpec> {
    Ok(ImageSpec{
      cuda: self.cuda,
      distro_codename: self.distro_codename.ok_or_else(|| fail("imagespec: missing distro codename"))?,
      distro_id: self.distro_id.ok_or_else(|| fail("imagespec: missing distro id"))?,
      docker: self.docker,
      nvidia_docker: self.nvidia_docker,
      toolchain: self.toolchain,
    })
  }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImageSpec {
  pub cuda: Option<CudaVersionV0>,
  pub distro_codename: DistroCodenameV0,
  pub distro_id: DistroIdV0,
  pub docker: bool,
  pub nvidia_docker: bool,
  pub toolchain: Option<Toolchain>,
}

impl ImageSpec {
  pub fn builtin_default() -> ImageSpec {
    ImageSpec{
      cuda: None,
      distro_codename: DistroCodenameV0::Alpine3_8,
      distro_id: DistroIdV0::Alpine,
      docker: true,
      nvidia_docker: false,
      toolchain: Some(Toolchain::Builtin),
    }
  }

  pub fn to_hash(&self, root_manifest: &RootManifest) -> CryptoBuf {
    let desc = self.to_desc();
    let mut hash_buf = CryptoBuf::zero_bytes(32);
    generic_hash(hash_buf.as_mut(), desc.as_bytes(), root_manifest.root_key_buf.as_ref()).unwrap();
    hash_buf
  }

  pub fn to_hash_digest(&self, root_manifest: &RootManifest) -> String {
    let hash_buf = self.to_hash(root_manifest);
    hex::encode(&hash_buf)
  }

  pub fn to_desc(&self) -> String {
    let mut buf = String::new();
    if let Some(cuda) = self.cuda {
      write!(&mut buf, " cuda={}", cuda.to_desc_str()).unwrap();
    }
    write!(&mut buf, " distro_codename={}", self.distro_codename.to_desc_str()).unwrap();
    write!(&mut buf, " distro_id={}", self.distro_id.to_desc_str()).unwrap();
    if self.docker {
      write!(&mut buf, " docker").unwrap();
    }
    if self.nvidia_docker {
      write!(&mut buf, " nvidia_docker").unwrap();
    }
    if let Some(ref toolchain) = self.toolchain {
      write!(&mut buf, " toolchain={}", toolchain.to_desc_string()).unwrap();
    }
    buf
  }

  pub fn to_toolchain_docker_template_dir(&self, sysroot: &Sysroot) -> PathBuf {
    match &self.toolchain {
      &None => sysroot.base_dir.join("docker").join("default"),
      &Some(ref tc) => sysroot.base_dir.join("docker").join(tc.to_desc_string()),
    }
  }

  pub fn to_toolchain_image_dir(&self, sysroot: &Sysroot) -> PathBuf {
    match &self.toolchain {
      &None => sysroot.base_dir.join("images").join("default"),
      &Some(ref tc) => sysroot.base_dir.join("images").join(tc.to_desc_string()),
    }
  }

  fn _to_nvidia_docker_base_image(&self) -> Option<String> {
    match (self.distro_codename, self.cuda.map(|v| (v.major, v.minor))) {
      (Centos6, Some((7, 0))) => {
        Some("nvidia/cuda:7.0-devel-centos6".to_string())
      }
      (Centos6, Some((7, 5))) => {
        Some("nvidia/cuda:7.5-devel-centos6".to_string())
      }
      (Centos6, Some((8, 0))) => {
        Some("nvidia/cuda:8.0-devel-centos6".to_string())
      }
      (Centos6, Some((9, 0))) => {
        Some("nvidia/cuda:9.0-devel-centos6".to_string())
      }
      (Centos6, Some((9, 1))) => {
        Some("nvidia/cuda:9.1-devel-centos6".to_string())
      }
      (Centos6, Some((9, 2))) => {
        Some("nvidia/cuda:9.2-devel-centos6".to_string())
      }
      (Centos6, Some((10, 0))) => {
        Some("nvidia/cuda:10.0-devel-centos6".to_string())
      }
      (Centos6, Some((10, 1))) => {
        Some("nvidia/cuda:10.1-devel-centos6".to_string())
      }
      (Centos7, None) => {
        Some("nvidia/driver:396.37-centos7".to_string())
      }
      (Centos7, Some((7, 0))) => {
        Some("nvidia/cuda:7.0-devel-centos7".to_string())
      }
      (Centos7, Some((7, 5))) => {
        Some("nvidia/cuda:7.5-devel-centos7".to_string())
      }
      (Centos7, Some((8, 0))) => {
        Some("nvidia/cuda:8.0-devel-centos7".to_string())
      }
      (Centos7, Some((9, 0))) => {
        Some("nvidia/cuda:9.0-devel-centos7".to_string())
      }
      (Centos7, Some((9, 1))) => {
        Some("nvidia/cuda:9.1-devel-centos7".to_string())
      }
      (Centos7, Some((9, 2))) => {
        Some("nvidia/cuda:9.2-devel-centos7".to_string())
      }
      (Centos7, Some((10, 0))) => {
        Some("nvidia/cuda:10.0-devel-centos7".to_string())
      }
      (Centos7, Some((10, 1))) => {
        Some("nvidia/cuda:10.1-devel-centos7".to_string())
      }
      (UbuntuTrusty, Some((6, 5))) => {
        Some("nvidia/cuda:6.5-devel-ubuntu14.04".to_string())
      }
      (UbuntuTrusty, Some((7, 0))) => {
        Some("nvidia/cuda:7.0-devel-ubuntu14.04".to_string())
      }
      (UbuntuTrusty, Some((7, 5))) => {
        Some("nvidia/cuda:7.5-devel-ubuntu14.04".to_string())
      }
      (UbuntuTrusty, Some((8, 0))) => {
        Some("nvidia/cuda:8.0-devel-ubuntu14.04".to_string())
      }
      (UbuntuXenial, None) => {
        Some("nvidia/driver:396.37-ubuntu16.04".to_string())
      }
      (UbuntuXenial, Some((8, 0))) => {
        Some("nvidia/cuda:8.0-devel-ubuntu16.04".to_string())
      }
      (UbuntuXenial, Some((9, 0))) => {
        Some("nvidia/cuda:9.0-devel-ubuntu16.04".to_string())
      }
      (UbuntuXenial, Some((9, 1))) => {
        Some("nvidia/cuda:9.1-devel-ubuntu16.04".to_string())
      }
      (UbuntuXenial, Some((9, 2))) => {
        Some("nvidia/cuda:9.2-devel-ubuntu16.04".to_string())
      }
      (UbuntuXenial, Some((10, 0))) => {
        Some("nvidia/cuda:10.0-devel-ubuntu16.04".to_string())
      }
      (UbuntuXenial, Some((10, 1))) => {
        Some("nvidia/cuda:10.1-devel-ubuntu16.04".to_string())
      }
      (UbuntuBionic, Some((9, 2))) => {
        Some("nvidia/cuda:9.2-devel-ubuntu18.04".to_string())
      }
      (UbuntuBionic, Some((10, 0))) => {
        Some("nvidia/cuda:10.0-devel-ubuntu18.04".to_string())
      }
      (UbuntuBionic, Some((10, 1))) => {
        Some("nvidia/cuda:10.1-devel-ubuntu18.04".to_string())
      }
      _ => None,
    }
  }

  fn _to_distro_docker_base_image(&self) -> Option<String> {
    match self.distro_codename {
      Alpine3_8 => {
        Some("alpine:3.8".to_string())
      }
      Alpine3_9 => {
        Some("alpine:3.9".to_string())
      }
      Centos6 => {
        Some("centos:centos6".to_string())
      }
      Centos7 => {
        Some("centos:centos7".to_string())
      }
      DebianWheezy => {
        Some("debian:wheezy".to_string())
      }
      DebianJessie => {
        Some("debian:jessie".to_string())
      }
      DebianStretch => {
        Some("debian:stretch".to_string())
      }
      DebianBuster => {
        Some("debian:buster".to_string())
      }
      UbuntuTrusty => {
        Some("ubuntu:14.04".to_string())
      }
      UbuntuXenial => {
        Some("ubuntu:16.04".to_string())
      }
      UbuntuBionic => {
        Some("ubuntu:18.04".to_string())
      }
      _ => None,
    }
  }

  pub fn to_docker_base_image(&self) -> Option<String> {
    if self.nvidia_docker {
      self._to_nvidia_docker_base_image()
    } else {
      if self.cuda.is_some() {
        eprintln!("WARNING: specified cuda but not nvidia docker");
        return None;
      }
      self._to_distro_docker_base_image()
    }
  }

  pub fn to_mincache_imagespec(&self) -> ImageSpec {
    // TODO
    unimplemented!();
  }
}

#[derive(Debug)]
pub struct ImageManifest {
  pub imagespecs: Vec<ImageSpec>,
}

impl ImageManifest {
  fn parse<R: Read>(file: &mut R, root_manifest: &RootManifest) -> Maybe<ImageManifest> {
    let mut imagespecs = vec![];
    let mut reader = BufReader::new(file);
    for line in reader.lines() {
      let line = line.unwrap();
      let line_parts: Vec<_> = line.split_whitespace().collect();
      let mut line_parts_iter = line_parts.iter();
      let im_hash = match line_parts_iter.next() {
        None => return Err(fail("bad images manifest (missing hash)")),
        Some(im_hash_str) => {
          hex::decode(im_hash_str)
            .map_err(|_| fail("bad images manifest (hash decode)"))?
        }
      };
      if im_hash.len() != 32 {
        return Err(fail("bad images manifest (hash length)"));
      }
      let mut builder = ImageSpecBuilder::default();
      for part in line_parts_iter {
        let part_toks: Vec<_> = part.splitn(2, "=").collect();
        if part_toks.is_empty() {
          return Err(fail("bug: bad images manifest"));
        }
        match part_toks.len() {
          1 => {
            match part_toks[0] {
              "docker" => {
                builder.docker = true;
              }
              "nvidia_docker" => {
                builder.nvidia_docker = true;
              }
              _ => return Err(fail("bug: bad images manifest")),
            }
          }
          2 => {
            match part_toks[0] {
              "cuda" => {
                let v = match part_toks[1] {
                  "v6_5" => CudaVersionV0{major: 6, minor: 5},
                  "v7_0" => CudaVersionV0{major: 7, minor: 0},
                  "v7_5" => CudaVersionV0{major: 7, minor: 5},
                  "v8_0" => CudaVersionV0{major: 8, minor: 0},
                  "v9_0" => CudaVersionV0{major: 9, minor: 0},
                  "v9_1" => CudaVersionV0{major: 9, minor: 1},
                  "v9_2" => CudaVersionV0{major: 9, minor: 2},
                  "v10_0" => CudaVersionV0{major: 10, minor: 0},
                  "v10_1" => CudaVersionV0{major: 10, minor: 1},
                  _ => return Err(fail("bug: bad images manifest")),
                };
                builder.cuda = Some(v);
              }
              "distro_codename" => {
                let v = match part_toks[1] {
                  "alpine_3_8" => Alpine3_8,
                  "alpine_3_9" => Alpine3_9,
                  "centos_6" => Centos6,
                  "centos_7" => Centos7,
                  "debian_wheezy" => DebianWheezy,
                  "debian_jessie" => DebianJessie,
                  "debian_stretch" => DebianStretch,
                  "debian_buster" => DebianBuster,
                  "ubuntu_trusty" => UbuntuTrusty,
                  "ubuntu_xenial" => UbuntuXenial,
                  "ubuntu_bionic" => UbuntuBionic,
                  _ => return Err(fail("bug: bad images manifest")),
                };
                builder.distro_codename = Some(v);
              }
              "distro_id" => {
                let v = match part_toks[1] {
                  "alpine" => Alpine,
                  "centos" => Centos,
                  "debian" => Debian,
                  "ubuntu" => Ubuntu,
                  _ => return Err(fail("bug: bad images manifest")),
                };
                builder.distro_id = Some(v);
              }
              "toolchain" => {
                match Toolchain::from_desc_str_nocustom(part_toks[1]) {
                  None => return Err(fail("bug: bad images manifest")),
                  Some(toolchain) => {
                    builder.toolchain = Some(toolchain);
                  }
                }
              }
              _ => return Err(fail("bug: bad images manifest")),
            }
          }
          _ => unreachable!(),
        }
      }
      let image = builder.into_imagespec()?;
      match image.to_hash(root_manifest) == CryptoBuf::from_vec(32, im_hash) {
        false => return Err(fail("bad images manifest (bad hash)")),
        true  => {}
      }
      imagespecs.push(image);
    }
    Ok(ImageManifest{imagespecs})
  }

  pub fn load(sysroot: &Sysroot, root_manifest: &RootManifest) -> Maybe<ImageManifest> {
    let manifest_path = sysroot.base_dir.join("images").join(".manifest");
    File::open(&manifest_path)
      .map_err(|_| fail("failed to open image manifest"))
      .and_then(|mut manifest_file| {
        ImageManifest::parse(&mut manifest_file, root_manifest)
      })
      .or_else(|_| {
        eprintln!("WARNING: images manifest is missing or corrupt, clearing");
        File::create(&manifest_path)
          .map_err(|_| fail("failed to load image manifest"))?;
        Ok(ImageManifest{imagespecs: Vec::new()})
      })
  }

  pub fn dump(&self, sysroot: &Sysroot, root_manifest: &RootManifest) -> Maybe {
    let manifest_path = sysroot.base_dir.join("images").join(".manifest");
    let manifest_file = File::create(manifest_path)
      .map_err(|_| fail("failed to open image manifest"))?;
    let mut buf = BufWriter::new(manifest_file);
    for image in self.imagespecs.iter() {
      writeln!(&mut buf, "{}{}", image.to_hash_digest(root_manifest), image.to_desc())
        .map_err(|_| fail("failed to write image manifest"))?;
    }
    Ok(())
  }

  pub fn lookup_docker_image(&mut self, lookup_image: &ImageSpec, sysroot: &Sysroot, root_manifest: &RootManifest) -> Maybe<DockerImage> {
    // FIXME
    for image in self.imagespecs.iter() {
      if image == lookup_image {
        //eprintln!("TRACE: lookup docker image: found match");
        return Ok(DockerImage{
          imagespec: image.clone(),
          hash_digest: image.to_hash_digest(root_manifest),
        });
      }
    }
    let new_docker_image = DockerImage{
      imagespec: lookup_image.clone(),
      hash_digest: lookup_image.to_hash_digest(root_manifest),
    };
    new_docker_image._build(false, sysroot)?;
    self.imagespecs.push(lookup_image.clone());
    self.dump(sysroot, root_manifest)?;
    Ok(new_docker_image)
  }
}

pub struct RootManifest {
  root_key_buf: CryptoBuf,
  auth_bit: bool,
  mach_reg_bit: bool,
}

impl RootManifest {
  fn parse<R: Read>(file: &mut R) -> Maybe<RootManifest> {
    let mut root_key_buf = vec![0; 32];
    file.read_exact(&mut root_key_buf)
      .map_err(|_| fail("failed to read root manifest"))?;
    let root_key_buf = CryptoBuf::from_vec(32, root_key_buf);
    let flag_bits: u8 = file.read_u8().ok().unwrap_or_else(|| 0_u8);
    let auth_bit = match flag_bits & 0x01 {
      0    => false,
      0x01 => true,
      _ => unreachable!(),
    };
    let mach_reg_bit = match flag_bits & 0x02 {
      0    => false,
      0x02 => true,
      _ => unreachable!(),
    };
    Ok(RootManifest{
      root_key_buf,
      auth_bit,
      mach_reg_bit,
    })
  }

  pub fn load(sysroot: &Sysroot) -> Maybe<RootManifest> {
    let manifest_path = sysroot.base_dir.join("root");
    let mut manifest_file = File::open(&manifest_path)
      .map_err(|_| fail("failed to open root manifest"))?;
    RootManifest::parse(&mut manifest_file)
  }

  pub fn fresh(sysroot: &Sysroot) -> Maybe<RootManifest> {
    let root_key_buf = CryptoBuf::random_bytes(32);
    let manifest_path = sysroot.base_dir.join("root");
    let mut manifest_file = File::create(&manifest_path)
      .map_err(|_| fail("failed to create root manifest"))?;
    manifest_file.set_permissions(Permissions::from_mode(0o600))
      .map_err(|_| fail("failed to set permissions on root manifest"))?;
    manifest_file.write_all(root_key_buf.as_ref())
      .map_err(|_| fail("failed to write root manifest"))?;
    manifest_file.write_u8(0_u8)
      .map_err(|_| fail("failed to write root manifest"))?;
    Ok(RootManifest{
      root_key_buf,
      auth_bit: false,
      mach_reg_bit: false,
    })
  }

  pub fn key_buf(&self) -> &CryptoBuf {
    &self.root_key_buf
  }

  pub fn key_as_base64(&self) -> String {
    let mut key_strbuf = String::new();
    base64::encode_config_buf(
        &self.root_key_buf,
        base64::URL_SAFE,
        &mut key_strbuf,
    );
    key_strbuf
  }

  fn write_flag_bits(&self, sysroot: &Sysroot) -> Maybe {
    let auth_mask: u8 = match self.auth_bit {
      false => 0,
      true  => 0x01,
    };
    let mach_reg_mask: u8 = match self.mach_reg_bit {
      false => 0,
      true  => 0x02,
    };
    let flag_bits = auth_mask | mach_reg_mask;
    let manifest_path = sysroot.base_dir.join("root");
    let mut manifest_file = OpenOptions::new()
      .read(true).write(true)
      .append(false).truncate(false).create(false)
      .open(&manifest_path)
      .map_err(|_| fail("failed to open root manifest"))?;
    manifest_file.seek(SeekFrom::Start(32))
      .map_err(|_| fail("failed to write root manifest"))?;
    manifest_file.write_u8(flag_bits)
      .map_err(|_| fail("failed to write root manifest"))?;
    Ok(())
  }

  pub fn auth_bit(&self) -> bool {
    self.auth_bit
  }

  pub fn set_auth_bit(&mut self, bit: bool, sysroot: &Sysroot) -> Maybe<bool> {
    let prev = self.auth_bit;
    self.auth_bit = bit;
    self.write_flag_bits(sysroot)?;
    Ok(prev)
  }

  pub fn mach_reg_bit(&self) -> bool {
    self.mach_reg_bit
  }

  pub fn set_mach_reg_bit(&mut self, bit: bool, sysroot: &Sysroot) -> Maybe<bool> {
    let prev = self.mach_reg_bit;
    self.mach_reg_bit = bit;
    self.write_flag_bits(sysroot)?;
    Ok(prev)
  }
}

pub struct Sysroot {
  pub base_dir: PathBuf,
  pub sock_dir: PathBuf,
}

impl Default for Sysroot {
  fn default() -> Sysroot {
    Sysroot{
      base_dir: PathBuf::from("/var/lib/guppybot"),
      sock_dir: PathBuf::from("/var/run"),
    }
  }
}

impl Sysroot {
  pub fn install(&self) -> Maybe {
    create_dir_all(&self.base_dir)
      .map_err(|_| fail("failed to create sysroot (default: /var/lib/guppybot): are you root?"))?;
    {
      let mut sysroot_tar = File::create(self.base_dir.join("sysroot.tar.gz"))
        .map_err(|_| fail("failed to unpack sysroot: are you root?"))?;
      sysroot_tar.write_all(SYSROOT_TAR_GZ)
        .map_err(|_| fail("failed to unpack sysroot: are you root?"))?;
    }
    let out = Command::new("tar")
      .current_dir(&self.base_dir)
      .arg("--no-same-owner")
      .arg("-xzf")
      .arg(self.base_dir.join("sysroot.tar.gz"))
      .output()
      .map_err(|_| fail("failed to run `tar`"))?;
    if !out.status.success() {
      return Err(fail(format!("`tar` failed with exit status: {:?}", out.status)));
    }
    set_permissions(&self.base_dir, Permissions::from_mode(0o700))
      .map_err(|_| fail("failed to install sysroot: are you root?"))?;
    create_dir_all(self.base_dir.join("images"))
      .map_err(|_| fail("failed to install sysroot: are you root?"))?;
    RootManifest::load(self)
      .or_else(|_| RootManifest::fresh(self))
      .map_err(|_| fail("failed to install root manifest: are you root?"))?;
    Ok(())
  }

  pub fn ensure_tmp_dir(&self) -> Maybe<PathBuf> {
    let tmp_dir = self.base_dir.join("tmp");
    create_dir_all(&tmp_dir)
      .map_err(|_| fail("failed to create tmp directory in sysroot"))?;
    Ok(tmp_dir)
  }
}
