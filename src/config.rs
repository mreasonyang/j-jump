use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub const CANDIDATE_LIMIT_MAX: usize = 254;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Provider {
    #[default]
    #[serde(rename = "jev")]
    Jev,
    #[serde(rename = "clef-flash")]
    ClefFlash,
}
impl Provider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Jev => "jev",
            Self::ClefFlash => "clef-flash",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Jev => "Jev",
            Self::ClefFlash => "Clef-Flash",
        }
    }
    pub fn env_key(self) -> &'static str {
        match self {
            Self::Jev => "TYPESAFE_API_KEY",
            Self::ClefFlash => "CLOUDFLARE_AUTH_TOKEN",
        }
    }
}
fn environment_source() -> String {
    "environment".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub language: String,
    pub credential: String,
    #[serde(default)]
    pub provider: Provider,
    #[serde(default)]
    pub cloudflare_account_id: String,
    #[serde(default = "environment_source")]
    pub cloudflare_credential: String,
    pub tracking: bool,
    pub semantic: bool,
    pub consent: String,
    pub privacy: String,
    pub exclude: Vec<PathBuf>,
    pub no_send: Vec<PathBuf>,
    pub candidate_limit: usize,
    pub proxy: String,
    pub semantic_route: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 3,
            language: "auto".into(),
            credential: "environment".into(),
            provider: Provider::Jev,
            cloudflare_account_id: String::new(),
            cloudflare_credential: environment_source(),
            tracking: true,
            semantic: false,
            consent: "ask".into(),
            privacy: "strict".into(),
            exclude: vec![],
            no_send: vec![],
            candidate_limit: CANDIDATE_LIMIT_MAX,
            proxy: "system".into(),
            semantic_route: "local_first".into(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub home: PathBuf,
}
impl Paths {
    pub fn discover(explicit: Option<PathBuf>) -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or(Error(
                7,
                "HOME must be absolute; fix the environment".into(),
            ))?;
        let root = env::var_os("J_JUMP_HOME").map(PathBuf::from);
        let (config, data, cache) = if let Some(root) = root {
            (root.join("config"), root.join("data"), root.join("cache"))
        } else if cfg!(target_os = "macos") {
            (
                home.join("Library/Application Support/j-jump/config"),
                home.join("Library/Application Support/j-jump/data"),
                home.join("Library/Caches/j-jump"),
            )
        } else {
            (
                xdg("XDG_CONFIG_HOME", &home.join(".config")).join("j-jump"),
                xdg("XDG_DATA_HOME", &home.join(".local/share")).join("j-jump"),
                xdg("XDG_CACHE_HOME", &home.join(".cache")).join("j-jump"),
            )
        };
        let config = explicit
            .or_else(|| env::var_os("J_JUMP_CONFIG").map(PathBuf::from))
            .unwrap_or(config.join("config.json"));
        if !config.is_absolute() || !data.is_absolute() || !cache.is_absolute() {
            return Err(Error(7, "state paths must be absolute".into()));
        }
        Ok(Self {
            config,
            data,
            cache,
            home,
        })
    }
    pub fn load(&self) -> Result<Config> {
        let dir = self
            .config
            .parent()
            .ok_or(Error(7, "invalid config parent".into()))?;
        let bytes = match private_read(&self.config, 65536) {
            Ok(b) => b,
            Err(_e)
                if fs::symlink_metadata(&self.config)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                // A genuine first run has no configuration directory either,
                // and that stays healthy. A directory that does exist must
                // still be private and owned even when it holds no
                // configuration file: every other command refuses it there, so
                // returning a default as healthy would report a usable state
                // for a path nothing can use.
                match fs::symlink_metadata(dir) {
                    Ok(_) => private_dir(dir)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(Error(7, "cannot inspect state directory".into())),
                }
                return Ok(Config::default());
            }
            Err(e) => return Err(e),
        };
        private_dir(dir)?;
        let config: Config = serde_json::from_slice(&bytes).map_err(|_| {
            Error(
                7,
                "invalid configuration; run doctor or config recover; tracking stopped".into(),
            )
        })?;
        config.validate()?;
        Ok(config)
    }
    pub fn save(&self, config: &Config, expected: &str) -> Result<()> {
        config.validate()?;
        let dir = self
            .config
            .parent()
            .ok_or(Error(7, "invalid config path".into()))?;
        private_dir(dir)?;
        let lock = private_file(&dir.join("config.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|_| Error(7, "configuration busy; retry setup".into()))?;
        if self.fingerprint()? != expected {
            return Err(Error(
                7,
                "configuration changed concurrently; restart setup".into(),
            ));
        }
        atomic_write(
            &self.config,
            &serde_json::to_vec_pretty(config)
                .map_err(|_| Error(7, "cannot encode configuration".into()))?,
        )
    }
    pub fn fingerprint(&self) -> Result<String> {
        if fs::symlink_metadata(&self.config)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok("absent".into());
        }
        Ok(crate::digest(&private_read(&self.config, 65536)?))
    }
    pub fn policy_lock(&self) -> Result<std::fs::File> {
        let dir = self
            .config
            .parent()
            .ok_or(Error(7, "invalid config path".into()))?;
        private_dir(dir)?;
        let lock = private_file(&dir.join("config.lock"))?;
        fs2::FileExt::try_lock_shared(&lock)
            .map_err(|_| Error(7, "privacy configuration changing; retry operation".into()))?;
        Ok(lock)
    }
    pub fn credential_lock(&self, exclusive: bool) -> Result<std::fs::File> {
        let dir = self
            .config
            .parent()
            .ok_or(Error(7, "invalid config path".into()))?;
        private_dir(dir)?;
        let lock = private_file(&dir.join("credential.lock"))?;
        let result = if exclusive {
            fs2::FileExt::try_lock_exclusive(&lock)
        } else {
            fs2::FileExt::try_lock_shared(&lock)
        };
        result.map_err(|_| Error(7, "credential change or request is in progress".into()))?;
        Ok(lock)
    }
    pub fn credential_epoch(&self) -> Result<String> {
        let dir = self
            .config
            .parent()
            .ok_or(Error(7, "invalid config path".into()))?;
        private_dir(dir)?;
        let path = dir.join("credential.epoch");
        if fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            return Ok("absent".into());
        }
        let bytes = private_read(&path, 64)?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| Error(7, "credential epoch invalid".into()))?;
        if text.len() != 32 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error(7, "credential epoch invalid".into()));
        }
        Ok(text.into())
    }
    /// Caller must hold the exclusive credential lock before changing the OS entry.
    pub fn rotate_credential_epoch(&self) -> Result<()> {
        let dir = self
            .config
            .parent()
            .ok_or(Error(7, "invalid config path".into()))?;
        private_dir(dir)?;
        let mut random = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(|_| Error(7, "credential epoch entropy unavailable".into()))?;
        let text: String = random.iter().map(|b| format!("{b:02x}")).collect();
        atomic_write(&dir.join("credential.epoch"), text.as_bytes())
    }
    pub fn allowed(&self, config: &Config, path: &Path, remote: bool) -> bool {
        self.policy(config, remote).allows(path)
    }
    pub fn policy(&self, config: &Config, remote: bool) -> Policy {
        let mut roots = vec![
            self.home.join(".ssh"),
            self.home.join(".gnupg"),
            self.home.join(".aws"),
            self.home.join(".azure"),
            self.home.join(".kube"),
            self.home.join("Library/Keychains"),
            xdg("XDG_CONFIG_HOME", &self.home.join(".config")).join("gcloud"),
            self.data.clone(),
            self.cache.clone(),
        ];
        if let Some(p) = self.config.parent() {
            roots.push(p.to_path_buf());
        }
        roots.extend(config.exclude.iter().cloned());
        if remote {
            roots.extend(config.no_send.iter().cloned());
        }
        let originals = roots.clone();
        for root in originals {
            if let Ok(real) = root.canonicalize()
                && !roots.contains(&real)
            {
                roots.push(real);
            }
        }
        Policy {
            roots,
            home: if remote {
                None
            } else {
                Some(
                    self.home
                        .canonicalize()
                        .unwrap_or_else(|_| self.home.clone()),
                )
            },
        }
    }
}
pub struct Policy {
    roots: Vec<PathBuf>,
    home: Option<PathBuf>,
}
impl Policy {
    pub fn allows(&self, path: &Path) -> bool {
        if !path.is_absolute() || !path.is_dir() {
            return false;
        }
        let Ok(real) = path.canonicalize() else {
            return false;
        };
        if self.home.as_ref().is_some_and(|h| *h == real) {
            return false;
        }
        !self.roots.iter().any(|root| {
            if path.starts_with(root) || real.starts_with(root) {
                return true;
            }
            // On case-insensitive volumes spelling may differ. Compare actual directory
            // identity at a case-folded component boundary rather than guessing by text.
            let folded_root = crate::engine::key(&root.to_string_lossy());
            for candidate in [path, real.as_path()] {
                let folded = PathBuf::from(crate::engine::key(&candidate.to_string_lossy()));
                if folded.starts_with(&folded_root) {
                    let prefix: PathBuf = candidate
                        .components()
                        .take(root.components().count())
                        .collect();
                    if let (Ok(a), Ok(b)) = (fs::metadata(root), fs::metadata(prefix))
                        && a.dev() == b.dev()
                        && a.ino() == b.ino()
                    {
                        return true;
                    }
                }
            }
            false
        })
    }
}

impl Config {
    pub fn credential_source(&self) -> &str {
        match self.provider {
            Provider::Jev => &self.credential,
            Provider::ClefFlash => &self.cloudflare_credential,
        }
    }
    pub fn set_credential_source(&mut self, source: &str) {
        match self.provider {
            Provider::Jev => self.credential = source.into(),
            Provider::ClefFlash => self.cloudflare_credential = source.into(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !["auto", "en", "zh"].contains(&self.language.as_str()) {
            return Err(Error(7, "language expects auto, en or zh".into()));
        }
        if !["environment", "system"].contains(&self.credential.as_str())
            || !["environment", "system"].contains(&self.cloudflare_credential.as_str())
            || (!self.cloudflare_account_id.is_empty()
                && !crate::provider::valid_account_id(&self.cloudflare_account_id))
            || self.schema_version != 3
            || !["ask", "always"].contains(&self.consent.as_str())
            || !["strict", "balanced", "full"].contains(&self.privacy.as_str())
            || !(1..=CANDIDATE_LIMIT_MAX).contains(&self.candidate_limit)
            || !["local_first", "force"].contains(&self.semantic_route.as_str())
            || self.proxy != "system"
            || self.exclude.iter().chain(&self.no_send).any(|p| {
                !p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
            })
        {
            return Err(Error(
                7,
                "unsupported configuration field/version/range; current format required; run config recover to preview a reset".into(),
            ));
        }
        Ok(())
    }
}
fn xdg(name: &str, default_path: &Path) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| default_path.to_path_buf())
}
/// Ensure the state directory exists and is private.
///
/// Only the state directory itself is required to be a real, owned, mode-0700
/// directory. Symbolic links in its ancestry are accepted: on macOS `/tmp` and
/// `/var` are links to `/private/...`, and `$HOME` or `$XDG_CONFIG_HOME` may be
/// links, so rejecting every linked ancestor made the tool unusable there. A
/// linked, non-private or wrongly-owned state directory is still refused, and
/// linked state *files* remain refused by `private_file`/`private_read`.
pub fn private_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m)
            if !m.is_dir()
                || m.file_type().is_symlink()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o077 != 0 =>
        {
            return Err(Error(
                7,
                format!("state directory must be owned by you and mode 0700, without symlinks: {}; inspect ownership and use chmod 700 -- '{}'", crate::display(&path.to_string_lossy()), path.to_string_lossy().replace('\'', "'\\''" )).into(),
            ));
        }
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(Error(7, "cannot inspect state directory".into())),
    }
    let parent = path
        .parent()
        .ok_or(Error(7, "invalid state directory".into()))?;
    if !parent.exists() {
        private_dir(parent)?;
    }
    use std::os::unix::fs::DirBuilderExt;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => private_dir(path),
        Err(_) => Err(Error(7, "cannot create private state directory".into())),
    }
}
pub fn private_file(path: &Path) -> Result<std::fs::File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error(7, "cannot open private state file".into()))?;
    check_file(&file)?;
    Ok(file)
}
fn check_file(file: &std::fs::File) -> Result<()> {
    let m = file
        .metadata()
        .map_err(|_| Error(7, "cannot inspect state file".into()))?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err(Error(
            7,
            "state file must be private, owned and not linked".into(),
        ));
    }
    Ok(())
}
pub fn private_read(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error(7, "cannot read private state file".into()))?;
    check_file(&file)?;
    let mut b = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut b)
        .map_err(|_| Error(7, "cannot read state".into()))?;
    if b.len() as u64 > max {
        return Err(Error(7, "state size limit exceeded".into()));
    }
    Ok(b)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)
            .map_err(|_| Error(7, "cannot create atomic state draft".into()))?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| Error(7, "cannot save state; previous state retained".into()))?;
        fs::rename(&temp, path).map_err(|_| Error(7, "cannot commit state".into()))?;
        std::fs::File::open(path.parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(|_| Error(7, "cannot sync state directory".into()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
