//! Private, lazy semantic transport. Policy, credential ownership and response validation stay in the caller.
use crate::{
    Error, Result,
    config::{Paths, private_dir},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroize;

fn local_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|_| Error(5, "local HTTP client unavailable".into()))
}

/// Explicit local service check. Offline doctor never calls this. Only static
/// metadata and a synthetic decision are sent, through the driver's transport.
pub fn diagnose(cfg: &crate::config::Config) -> Result<serde_json::Value> {
    diagnose_for(cfg, crate::providers::DiagnosticKind::Check)
}
pub fn diagnose_for(
    cfg: &crate::config::Config,
    kind: crate::providers::DiagnosticKind,
) -> Result<serde_json::Value> {
    let driver = cfg.provider.driver();
    // The driver owns metadata and probe semantics. The adapter only executes
    // bounded requests on the already validated local origin.
    let mut responses = Vec::new();
    let deadline = Instant::now() + INTERACTIVE_DEADLINE;
    let mut step = driver.diagnostic(cfg, kind, &responses)?;
    let connection = driver.connection(cfg, None)?;
    if connection.transport != crate::providers::Transport::Loopback {
        return Err(Error(5, "diagnostic transport is not local".into()));
    }
    let origin = reqwest::Url::parse(&connection.url)
        .map_err(|_| Error(5, "invalid diagnostic origin".into()))?;
    let client = local_client()?;
    loop {
        match step {
            crate::providers::Diagnostic::Complete(mut report) => {
                report["provider"] = cfg.provider.id().into();
                report["requests"] = responses.len().into();
                return Ok(report);
            }
            crate::providers::Diagnostic::Request { path, body } => {
                if responses.len() >= 8
                    || !path.starts_with('/')
                    || path.starts_with("//")
                    || body.as_ref().is_some_and(|b| b.len() > 65536)
                {
                    return Err(Error(5, "diagnostic request bounds".into()));
                }
                let url = origin
                    .join(path)
                    .map_err(|_| Error(5, "invalid diagnostic path".into()))?;
                if url.origin() != origin.origin() {
                    return Err(Error(5, "diagnostic origin changed".into()));
                }
                let request = if let Some(body) = body {
                    client
                        .post(url)
                        .header("Content-Type", "application/json")
                        .body(body)
                } else {
                    client.get(url)
                };
                let raw = read_response(request.timeout(remaining(deadline)?).send())?;
                responses.push(crate::provider::strict_json(raw.as_bytes())?);
                step = driver.diagnostic(cfg, kind, &responses)?;
                remaining(deadline)?;
            }
        }
    }
}

const VERSION: u8 = 7;
// A runtime directory may only be reclaimed once it has had no live owner for
// at least this long. A shorter window risks racing a concurrent invocation
// that has created its directory but not yet taken its instance lock.
const RUNTIME_STALE: Duration = Duration::from_secs(600);
// Bound the work done on adapter start; reclamation is not on the CLI hot path.
// The match bound must comfortably exceed the number of runtime identities a
// machine accumulates, otherwise a fixed readdir order can starve stale
// directories; the entry bound caps iteration over unrelated temporary files.
const RUNTIME_SCAN_LIMIT: usize = 4096;
const RUNTIME_ENTRY_LIMIT: usize = 65536;
// The only entries an adapter ever places in its own runtime directory.
const RUNTIME_FILES: [&str; 3] = ["instance.lock", "jev.sock", "profile.id"];
// Conventional proxy settings enter the detached adapter; unrelated shell
// environment and credentials remain excluded.
const PROXY_ENV_KEYS: [&str; 9] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "REQUEST_METHOD",
];
// JSON string escaping can expand a bounded 256 KiB response by up to six times.
const MAX_FRAME: usize = 2_000_000;
const IDLE: Duration = Duration::from_secs(300);
const MAX_ACTIVE: usize = 8;
/// Abandoned requests whose HTTP exchange may still be finishing in the background.
const MAX_DETACHED: usize = 8;
pub const INTERACTIVE_DEADLINE: Duration = Duration::from_secs(10);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
// Leave time for a correlated IPC reply and caller-side validation after
// reqwest's network timeout. This is part of the caller's absolute deadline.
const REPLY_MARGIN: Duration = Duration::from_millis(80);

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Wire {
    Ping {
        version: u8,
    },
    Stop {
        version: u8,
    },
    Send {
        version: u8,
        id: String,
        fingerprint: String,
        credential_epoch: String,
        account_id: String,
        remaining_ms: u64,
        deadline_tick_ns: u64,
        key: String,
        payload: String,
    },
}
impl Drop for Wire {
    fn drop(&mut self) {
        if let Wire::Send { key, .. } = self {
            key.zeroize();
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    version: u8,
    id: String,
    status: String,
    body: String,
}

/// Lets the CLI give up on a pending exchange when the user chooses local picks.
/// Shutting the socket down ends the blocked read at once, and the adapter, which
/// sees the hangup, frees its send slot for the next request.
#[derive(Default)]
pub struct Abort {
    flag: AtomicBool,
    stream: std::sync::Mutex<Option<UnixStream>>,
}
impl Abort {
    pub fn new() -> Self {
        Self::default()
    }
    fn register(&self, stream: &UnixStream) {
        if let (Ok(clone), Ok(mut slot)) = (stream.try_clone(), self.stream.lock()) {
            *slot = Some(clone);
            if self.flag.load(Ordering::SeqCst)
                && let Some(s) = slot.as_ref()
            {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }
    pub fn abort(&self) {
        self.flag.store(true, Ordering::SeqCst);
        if let Ok(slot) = self.stream.lock()
            && let Some(s) = slot.as_ref()
        {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
    pub fn aborted(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterState {
    Running,
    Stopping,
    Stopped,
}
impl std::fmt::Display for AdapterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
        })
    }
}
fn proxy_env_identity() -> Vec<u8> {
    let mut identity = Vec::new();
    for key in PROXY_ENV_KEYS {
        identity.extend_from_slice(key.as_bytes());
        identity.push(0);
        if let Some(value) = std::env::var_os(key) {
            identity.extend_from_slice(value.as_encoded_bytes());
        }
        identity.push(0);
    }
    identity
}
fn runtime_base() -> &'static Path {
    // macOS has a short Unix-socket path limit. Keep the authenticated endpoint
    // in a short, private per-user/profile directory rather than a long XDG path.
    #[cfg(target_os = "macos")]
    {
        Path::new("/private/tmp")
    }
    #[cfg(target_os = "linux")]
    {
        Path::new("/tmp")
    }
}
/// Pure path computation. Only the adapter's own start creates the directory, so
/// a mere connect/status probe never materialises a new runtime directory.
fn profile_id(paths: &Paths) -> String {
    let protocol = format!("\0adapter-v{VERSION}");
    crate::digest(
        &[
            paths.config.as_os_str().as_encoded_bytes(),
            b"\0",
            paths.cache.as_os_str().as_encoded_bytes(),
            protocol.as_bytes(),
        ]
        .concat(),
    )
}
fn runtime_path(paths: &Paths) -> PathBuf {
    let protocol = format!("\0adapter-v{VERSION}");
    let mut profile = [
        paths.config.as_os_str().as_encoded_bytes(),
        b"\0",
        paths.cache.as_os_str().as_encoded_bytes(),
        protocol.as_bytes(),
    ]
    .concat();
    profile.extend_from_slice(&proxy_env_identity());
    let name = format!(
        "jj-{}-{}",
        unsafe { libc::geteuid() },
        &crate::digest(&profile)[..24]
    );
    runtime_base().join(name)
}
fn runtime(paths: &Paths) -> Result<PathBuf> {
    let dir = runtime_path(paths);
    private_dir(&dir)?;
    Ok(dir)
}
fn socket_path(paths: &Paths) -> PathBuf {
    runtime_path(paths).join("jev.sock")
}
fn socket(paths: &Paths) -> Result<PathBuf> {
    Ok(runtime(paths)?.join("jev.sock"))
}
/// Reclaim runtime directories under `base` for `uid` that provably have no live
/// owner. `keep` is the caller's own live directory and is never touched. A
/// directory is reclaimed only when it is owned by `uid`, private, older than
/// RUNTIME_STALE, holds no held instance lock, has no listening socket, and
/// contains no entries other than the files an adapter owns.
fn reclaim_runtime_in(base: &Path, keep: &Path, uid: u32) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    let prefix = format!("jj-{uid}-");
    let mut examined = 0usize;
    for (seen, entry) in entries.flatten().enumerate() {
        if examined >= RUNTIME_SCAN_LIMIT || seen >= RUNTIME_ENTRY_LIMIT {
            break;
        }
        let dir = entry.path();
        if dir == keep
            || !dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        {
            continue;
        }
        examined += 1;
        reclaim_one(&dir, uid);
    }
}
fn reclaim_one(dir: &Path, uid: u32) {
    if !stale_ownerless_dir(dir, uid) {
        return;
    }
    // Refuse anything an adapter would not have written, rather than deleting
    // unrecognised content.
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    for item in listing.flatten() {
        if !item
            .file_name()
            .to_str()
            .is_some_and(|name| RUNTIME_FILES.contains(&name))
        {
            return;
        }
    }
    // A socket that still accepts a connection means a live adapter, even if the
    // lock could not be observed.
    let sock = dir.join("jev.sock");
    if let Ok(m) = fs::symlink_metadata(&sock) {
        if !m.file_type().is_socket() || m.uid() != uid || m.mode() & 0o077 != 0 {
            return;
        }
        if UnixStream::connect(&sock).is_ok() {
            return;
        }
    }
    // Hold the instance lock across removal so a concurrent serve cannot adopt
    // this directory after the liveness checks but before it is removed. A
    // starting adapter also takes this lock before it binds, so whoever wins the
    // lock owns the directory and the loser exits without touching it.
    let Ok(lock) = crate::config::private_file(&dir.join("instance.lock")) else {
        return;
    };
    if fs2::FileExt::try_lock_exclusive(&lock).is_err() {
        return;
    }
    for name in RUNTIME_FILES {
        let _ = fs::remove_file(dir.join(name));
    }
    let _ = fs::remove_dir(dir);
}
fn stale_ownerless_dir(dir: &Path, uid: u32) -> bool {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return false;
    };
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != uid
        || meta.mode() & 0o077 != 0
    {
        return false;
    }
    meta.modified()
        .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age >= RUNTIME_STALE))
}
/// Reclaim stale runtime directories for the current user, keeping the live one
/// belonging to this adapter. Called once per adapter start.
fn reclaim_stale_runtime(keep: &Path) {
    reclaim_runtime_in(runtime_base(), keep, unsafe { libc::geteuid() });
}

fn check_socket(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(m)
            if m.file_type().is_socket()
                && m.uid() == unsafe { libc::geteuid() }
                && m.mode() & 0o077 == 0 =>
        {
            Ok(true)
        }
        Ok(_) => Err(Error(
            7,
            "adapter socket is not private or has wrong owner/type".into(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Error(7, "cannot inspect adapter socket".into())),
    }
}
fn peer_uid(stream: &UnixStream) -> Result<()> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "macos")]
    {
        let (mut uid, mut gid) = (0, 0);
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0
            || uid != unsafe { libc::geteuid() }
        {
            return Err(Error(7, "adapter peer identity mismatch".into()));
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = std::mem::MaybeUninit::<libc::ucred>::uninit();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                cred.as_mut_ptr().cast(),
                &mut len,
            )
        } != 0
            || len as usize != std::mem::size_of::<libc::ucred>()
            || unsafe { cred.assume_init().uid } != unsafe { libc::geteuid() }
        {
            return Err(Error(7, "adapter peer identity mismatch".into()));
        }
    }
    Ok(())
}
fn connect(paths: &Paths) -> Result<Option<UnixStream>> {
    let path = socket_path(paths);
    if !check_socket(&path)? {
        return Ok(None);
    }
    match UnixStream::connect(&path) {
        Ok(s) => {
            peer_uid(&s)?;
            Ok(Some(s))
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(Error(7, "private adapter connection failed".into())),
    }
}
fn write_frame(stream: &mut UnixStream, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(Error(5, "adapter frame exceeds limit".into()));
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| stream.write_all(bytes))
        .map_err(|_| {
            Error(
                5,
                "adapter send outcome unknown; browse locally: jjump --offline query --interactive"
                    .into(),
            )
        })
}
fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut size = [0u8; 4];
    stream.read_exact(&mut size).map_err(|_| {
        Error(
            5,
            "adapter reply unavailable; browse locally: jjump --offline query --interactive".into(),
        )
    })?;
    let len = u32::from_be_bytes(size) as usize;
    if len > MAX_FRAME {
        return Err(Error(5, "adapter reply exceeds limit".into()));
    }
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).map_err(|_| {
        Error(
            5,
            "adapter reply incomplete; browse locally: jjump --offline query --interactive".into(),
        )
    })?;
    Ok(bytes)
}
fn remaining(deadline: Instant) -> Result<Duration> {
    let d = deadline.saturating_duration_since(Instant::now());
    if d.is_zero() {
        Err(Error(5, "Provider total request deadline elapsed".into()))
    } else {
        Ok(d)
    }
}
fn monotonic_ns() -> Result<u64> {
    let mut ts = std::mem::MaybeUninit::<libc::timespec>::uninit();
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) } != 0 {
        return Err(Error(5, "monotonic request clock unavailable".into()));
    }
    let ts = unsafe { ts.assume_init() };
    (ts.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(ts.tv_nsec as u64))
        .ok_or(Error(5, "monotonic request clock overflow".into()))
}
fn timeouts(stream: &UnixStream, deadline: Instant) -> Result<()> {
    let d = remaining(deadline)?;
    stream
        .set_read_timeout(Some(d))
        .and_then(|_| stream.set_write_timeout(Some(d)))
        .map_err(|_| Error(5, "cannot bound adapter operation".into()))
}
fn launch(paths: &Paths) -> Result<()> {
    let exe =
        std::env::current_exe().map_err(|_| Error(7, "adapter executable unavailable".into()))?;
    let mut cmd = Command::new(exe);
    detach_session(&mut cmd);
    cmd.env_clear();
    for key in [
        "HOME",
        "J_JUMP_HOME",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    for key in PROXY_ENV_KEYS {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.arg("--config")
        .arg(&paths.config)
        .arg("adapter-serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|_| Error(7, "cannot launch private adapter".into()))?;
    Ok(())
}

fn detach_session(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // The calling CLI may run inside a short-lived PTY. Its exit must not send
    // SIGHUP to the retained adapter, even though all three stdio fds are null.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}
/// Start only the private adapter; unavailable transport fails closed.
pub fn ready(paths: &Paths, deadline: Instant, abort: &Abort) -> Result<UnixStream> {
    if let Some(s) = connect(paths)? {
        return Ok(s);
    }
    let unavailable = || {
        Error(
            5,
            "adapter unavailable; run jjump adapter restart and retry, or use jjump --offline query --interactive".into(),
        )
    };
    if remaining(deadline)?.as_millis() < 50 {
        return Err(unavailable());
    }
    let startup_end = Instant::now() + remaining(deadline)?.min(Duration::from_secs(1));
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let launch_paths = paths.clone();
    std::thread::Builder::new()
        .spawn(move || {
            let _ = tx.send(launch(&launch_paths));
        })
        .map_err(|_| unavailable())?;
    loop {
        if abort.aborted() {
            return Err(Error(130, "request cancelled".into()));
        }
        let left = startup_end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(unavailable());
        }
        match rx.recv_timeout(left.min(Duration::from_millis(40))) {
            Ok(result) => {
                result?;
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err(unavailable()),
        }
    }
    while Instant::now() < startup_end {
        if abort.aborted() {
            return Err(Error(130, "request cancelled".into()));
        }
        if let Some(s) = connect(paths)? {
            return Ok(s);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(unavailable())
}
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn exchange(
    stream: UnixStream,
    id: &str,
    fingerprint: &str,
    credential_epoch: &str,
    key: &mut String,
    payload: &[u8],
    deadline: Instant,
    abort: &Abort,
) -> Result<Vec<u8>> {
    exchange_for(
        stream,
        id,
        fingerprint,
        credential_epoch,
        "",
        key,
        payload,
        deadline,
        abort,
    )
}
#[allow(clippy::too_many_arguments)]
pub fn exchange_for(
    mut stream: UnixStream,
    id: &str,
    fingerprint: &str,
    credential_epoch: &str,
    account_id: &str,
    key: &mut String,
    payload: &[u8],
    deadline: Instant,
    abort: &Abort,
) -> Result<Vec<u8>> {
    abort.register(&stream);
    if id.len() != 32
        || fingerprint.len() > 128
        || credential_epoch.len() > 64
        || account_id.len() > 256
        || key.len() > 4096
        || payload.len() > 65536
    {
        return Err(Error(5, "invalid adapter request bounds".into()));
    }
    timeouts(&stream, deadline)?;
    let payload = std::str::from_utf8(payload)
        .map_err(|_| Error(5, "invalid adapter request encoding".into()))?;
    let mut frame = serde_json::to_vec(&Wire::Send {
        version: VERSION,
        id: id.into(),
        fingerprint: fingerprint.into(),
        credential_epoch: credential_epoch.into(),
        account_id: account_id.into(),
        remaining_ms: remaining(deadline)?.as_millis() as u64,
        deadline_tick_ns: monotonic_ns()? + remaining(deadline)?.as_nanos() as u64,
        key: std::mem::take(key),
        payload: payload.into(),
    })
    .map_err(|_| Error(5, "cannot encode adapter request".into()))?;
    let result = (|| {
        timeouts(&stream, deadline)?;
        write_frame(&mut stream, &frame)?;
        timeouts(&stream, deadline)?;
        let reply: Reply = serde_json::from_slice(&read_frame(&mut stream)?)
            .map_err(|_| Error(5, "invalid adapter reply".into()))?;
        if reply.version != VERSION || reply.id != id || reply.body.len() > 262144 {
            return Err(Error(5, "adapter reply binding mismatch".into()));
        }
        if reply.status != "ok" {
            return Err(match reply.status.as_str() {
                "bounds" => Error(
                    5,
                    "adapter request bounds invalid; browse locally: jjump --offline query --interactive".into(),
                ),
                "deadline" => Error(
                    5,
                    "adapter deadline binding invalid; browse locally: jjump --offline query --interactive".into(),
                ),
                "fingerprint" => Error(
                    5,
                    "adapter configuration binding changed; browse locally: jjump --offline query --interactive".into(),
                ),
                "credential" => Error(
                    5,
                    "adapter credential binding changed; browse locally: jjump --offline query --interactive".into(),
                ),
                "policy" => Error(5, "adapter policy disabled; browse locally: jjump --offline query --interactive".into()),
                "dispatch" => Error(
                    5,
                    "adapter dispatch claim unavailable; browse locally: jjump --offline query --interactive".into(),
                ),
                "stopping" => Error(5, "adapter is stopping; browse locally: jjump --offline query --interactive".into()),
                "busy" => Error(
                    5,
                    "adapter request already in flight; browse locally: jjump --offline query --interactive".into(),
                ),
                "settings_busy" => Error(5, "J-Jump settings were changing; retry".into()),
                "credential_busy" => Error(5, "the provider key was changing; retry".into()),
                "connect" => Error(
                    5,
                    "Provider adapter connection failed; browse locally: jjump --offline query --interactive".into(),
                ),
                "connect_timeout" => Error(
                    5,
                    "Provider adapter connection timed out; browse locally: jjump --offline query --interactive".into(),
                ),
                "timeout" => Error(
                    5,
                    "Provider adapter response timed out; browse locally: jjump --offline query --interactive".into(),
                ),
                "body_timeout" => Error(
                    5,
                    "Provider adapter response body timed out; browse locally: jjump --offline query --interactive".into(),
                ),
                "body" => Error(
                    5,
                    "Provider adapter response body failed; browse locally: jjump --offline query --interactive".into(),
                ),
                "transport" => Error(
                    5,
                    "Provider adapter transport failed; browse locally: jjump --offline query --interactive".into(),
                ),
                "auth" => Error(5, "Provider authentication rejected; browse locally: jjump --offline query --interactive".into()),
                "rate_limit" => Error(
                    5,
                    "Provider rate limit or quota reached; browse locally: jjump --offline query --interactive".into(),
                ),
                "rejected" => Error(5, "Provider request rejected; browse locally: jjump --offline query --interactive".into()),
                _ => Error(5, "Provider adapter request failed; browse locally: jjump --offline query --interactive".into()),
            });
        }
        Ok(reply.body.into_bytes())
    })();
    frame.zeroize();
    result
}
/// A reachable adapter that cannot answer is shutting down, not an error.
fn control(paths: &Paths, message: Wire) -> Result<AdapterState> {
    let Some(mut stream) = connect(paths)? else {
        return Ok(AdapterState::Stopped);
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_millis(100)))
        .ok();
    let bytes = serde_json::to_vec(&message)
        .map_err(|_| Error(7, "cannot encode adapter control".into()))?;
    if write_frame(&mut stream, &bytes).is_err() {
        return Ok(AdapterState::Stopping);
    }
    let reply = read_frame(&mut stream)
        .ok()
        .and_then(|frame| serde_json::from_slice::<Reply>(&frame).ok());
    Ok(match reply {
        Some(reply) if reply.version == VERSION && reply.status == "ok" => AdapterState::Running,
        _ => AdapterState::Stopping,
    })
}
pub fn status(paths: &Paths) -> Result<AdapterState> {
    control(paths, Wire::Ping { version: VERSION })
}
pub fn stop(paths: &Paths) -> Result<AdapterState> {
    let others = stop_others(paths);
    Ok(match control(paths, Wire::Stop { version: VERSION })? {
        AdapterState::Stopped if others == 0 => AdapterState::Stopped,
        _ => AdapterState::Stopping,
    })
}
/// Map a provider response to the bounded body or a static correlated error.
fn read_response(
    response: std::result::Result<reqwest::blocking::Response, reqwest::Error>,
) -> Result<String> {
    let response = response.map_err(|e| {
        if e.is_connect() && e.is_timeout() {
            Error(5, "adapter connect timeout".into())
        } else if e.is_timeout() {
            Error(5, "adapter timeout".into())
        } else if e.is_connect() {
            Error(5, "adapter connect".into())
        } else {
            Error(5, "adapter transport".into())
        }
    })?;
    match response.status().as_u16() {
        401 | 403 => return Err(Error(5, "adapter auth".into())),
        429 => return Err(Error(5, "adapter rate limit".into())),
        _ if !response.status().is_success() => {
            return Err(Error(5, "adapter rejected".into()));
        }
        _ => {}
    }
    if response.content_length().is_some_and(|n| n > 262144) {
        return Err(Error(5, "Provider response exceeds limit".into()));
    }
    let mut body = Vec::new();
    response.take(262145).read_to_end(&mut body).map_err(|e| {
        if matches!(
            e.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ) {
            Error(5, "adapter body timeout".into())
        } else {
            Error(5, "adapter body".into())
        }
    })?;
    if body.len() > 262144 {
        return Err(Error(5, "Provider response exceeds limit".into()));
    }
    String::from_utf8(body).map_err(|_| Error(5, "Provider response encoding invalid".into()))
}
/// Ask every other helper this user runs to stop: helpers are namespaced by
/// profile and proxy environment, so one started under another proxy setting is
/// not reachable through `stop`. Only private, user-owned runtime directories
/// and sockets whose peer is this user are contacted. Returns how many answered.
pub fn stop_others(paths: &Paths) -> usize {
    let uid = unsafe { libc::geteuid() };
    let prefix = format!("jj-{uid}-");
    let own = runtime_path(paths);
    let Ok(entries) = fs::read_dir(runtime_base()) else {
        return 0;
    };
    let mut stopped = 0;
    for entry in entries.flatten().take(RUNTIME_ENTRY_LIMIT) {
        let dir = entry.path();
        if dir == own
            || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix))
            || !fs::symlink_metadata(&dir)
                .is_ok_and(|m| m.is_dir() && m.uid() == uid && m.mode() & 0o077 == 0)
        {
            continue;
        }
        if crate::config::private_read(&dir.join("profile.id"), 128)
            .ok()
            .as_deref()
            != Some(profile_id(paths).as_bytes())
        {
            continue;
        }
        let socket = dir.join("jev.sock");
        if !check_socket(&socket).unwrap_or(false) {
            continue;
        }
        let Ok(mut stream) = UnixStream::connect(&socket) else {
            continue;
        };
        if peer_uid(&stream).is_err() {
            continue;
        }
        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
        let Ok(bytes) = serde_json::to_vec(&Wire::Stop { version: VERSION }) else {
            continue;
        };
        if write_frame(&mut stream, &bytes).is_ok() && read_frame(&mut stream).is_ok() {
            stopped += 1;
        }
    }
    stopped
}
/// Shared locks are contended only by a brief settings or key change; wait a
/// moment rather than failing a request that would succeed shortly after.
fn shared_lock(take: impl Fn() -> Result<std::fs::File>) -> Result<std::fs::File> {
    let until = Instant::now() + Duration::from_millis(150);
    loop {
        match take() {
            Err(e)
                if Instant::now() < until
                    && matches!(
                        e.1.as_ref(),
                        "privacy configuration changing; retry operation"
                            | "credential change or request is in progress"
                    ) =>
            {
                std::thread::sleep(Duration::from_millis(5))
            }
            other => return other,
        }
    }
}
/// True once the client has closed its end: a peek sees end-of-file.
fn client_gone(stream: &UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    let mut byte = [0u8; 1];
    let n = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            byte.as_mut_ptr().cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    n == 0
        || (n < 0
            && !matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EAGAIN | libc::EINTR)
            ))
}
fn process(
    mut stream: UnixStream,
    paths: Paths,
    client: reqwest::blocking::Client,
    stopping: Arc<AtomicBool>,
    sending: Arc<AtomicBool>,
    detached: Arc<AtomicUsize>,
    endpoint: &str,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(5250)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(5250)));
    if peer_uid(&stream).is_err() {
        return;
    }
    let mut correlation = String::new();
    let reply = (|| -> Result<Reply> {
        let mut bytes = read_frame(&mut stream)?;
        let decoded =
            serde_json::from_slice(&bytes).map_err(|_| Error(5, "invalid adapter request".into()));
        bytes.zeroize();
        let mut message: Wire = decoded?;
        if let Wire::Send { id, .. } = &message
            && id.len() == 32
            && id.bytes().all(|b| b.is_ascii_hexdigit())
        {
            correlation = id.clone();
        }
        match &mut message {
            Wire::Ping { version } if *version == VERSION => Ok(Reply {
                version: VERSION,
                id: String::new(),
                status: "ok".into(),
                body: String::new(),
            }),
            Wire::Stop { version } if *version == VERSION => {
                stopping.store(true, Ordering::SeqCst);
                Ok(Reply {
                    version: VERSION,
                    id: String::new(),
                    status: "ok".into(),
                    body: String::new(),
                })
            }
            Wire::Send {
                version,
                id,
                fingerprint,
                credential_epoch,
                account_id,
                remaining_ms,
                deadline_tick_ns,
                key,
                payload,
            } if *version == VERSION => {
                let id = id.clone();
                let _policy_lock = shared_lock(|| paths.policy_lock())?;
                let cfg = paths.load()?;
                let authenticated = cfg.provider.needs_credentials();
                let _credential_lock = if authenticated {
                    Some(shared_lock(|| paths.credential_lock(false))?)
                } else {
                    None
                };
                let clock_remaining = deadline_tick_ns.saturating_sub(monotonic_ns()?);
                if id.len() != 32
                    || *remaining_ms == 0
                    || *remaining_ms > INTERACTIVE_DEADLINE.as_millis() as u64
                    || (authenticated && key.is_empty())
                    || (!authenticated && (!key.is_empty() || credential_epoch != "none"))
                    || key.len() > 4096
                    || payload.len() > 65536
                {
                    key.zeroize();
                    return Err(Error(5, "adapter bounds".into()));
                }
                if !(1_000_000..=INTERACTIVE_DEADLINE.as_nanos() as u64).contains(&clock_remaining)
                {
                    key.zeroize();
                    return Err(Error(5, "adapter deadline".into()));
                }
                if paths.fingerprint()? != *fingerprint {
                    key.zeroize();
                    return Err(Error(5, "adapter fingerprint".into()));
                }
                if authenticated && paths.credential_epoch()? != *credential_epoch {
                    key.zeroize();
                    return Err(Error(5, "adapter credential".into()));
                }
                let cfg = paths.load()?;
                if !cfg.semantic {
                    key.zeroize();
                    return Err(Error(5, "adapter policy".into()));
                }
                let connection = cfg.provider.driver().connection(&cfg, Some(account_id))?;
                let selected_endpoint = connection.url;
                let client = if connection.transport == crate::providers::Transport::Loopback {
                    local_client()?
                } else {
                    client.clone()
                };
                if crate::provider::strict_json(payload.as_bytes())?["model"]
                    != cfg.provider.driver().selected_model(&cfg)
                    && endpoint == crate::provider::ENDPOINT
                {
                    return Err(Error(5, "adapter policy".into()));
                }
                if !crate::provider::dispatch_authorized(&paths, &id)? {
                    key.zeroize();
                    return Err(Error(5, "adapter dispatch".into()));
                }
                if stopping.load(Ordering::SeqCst) {
                    key.zeroize();
                    return Err(Error(5, "adapter stopping".into()));
                }
                // A cancelled CLI can return before the old IPC handler observes
                // its hangup (the handler polls every 40 ms). Allow that slot to
                // finish handing over before refusing the next request. This
                // waits only for local ownership; it never replays an HTTP send.
                let handover = Instant::now() + Duration::from_millis(80);
                loop {
                    if detached.load(Ordering::SeqCst) >= MAX_DETACHED {
                        return Err(Error(5, "adapter busy".into()));
                    }
                    if sending
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        break;
                    }
                    if Instant::now() >= handover || monotonic_ns()? >= *deadline_tick_ns {
                        return Err(Error(5, "adapter busy".into()));
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                struct SendGuard(Arc<AtomicBool>);
                impl Drop for SendGuard {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::SeqCst);
                    }
                }
                let guard = SendGuard(sending.clone());
                let mut secret = std::mem::take(key);
                let live = deadline_tick_ns.saturating_sub(monotonic_ns()?);
                let available = Duration::from_nanos(live.min(*remaining_ms * 1_000_000));
                let Some(request_budget) = available.checked_sub(REPLY_MARGIN) else {
                    secret.zeroize();
                    return Err(Error(5, "adapter deadline".into()));
                };
                if request_budget.is_zero() {
                    secret.zeroize();
                    return Err(Error(5, "adapter deadline".into()));
                }
                // 0 = running, 1 = finished, 2 = abandoned by the client.
                let state = Arc::new(std::sync::atomic::AtomicU8::new(0));
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let (http, url, body, worker_state, worker_detached) = (
                    client.clone(),
                    if endpoint == crate::provider::ENDPOINT {
                        selected_endpoint
                    } else {
                        endpoint.to_owned()
                    },
                    payload.clone(),
                    state.clone(),
                    detached.clone(),
                );
                std::thread::spawn(move || {
                    let request = http.post(&url);
                    let request = if authenticated {
                        request.bearer_auth(&secret)
                    } else {
                        request
                    };
                    let response = request
                        .header("Content-Type", "application/json")
                        .timeout(request_budget)
                        .body(body)
                        .send();
                    secret.zeroize();
                    let outcome = read_response(response);
                    if worker_state
                        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                        .is_err()
                    {
                        worker_detached.fetch_sub(1, Ordering::SeqCst);
                    }
                    let _ = tx.send(outcome);
                });
                let outcome = loop {
                    match rx.recv_timeout(Duration::from_millis(40)) {
                        Ok(outcome) => break outcome,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            if client_gone(&stream) {
                                // The user chose local picks. Free the send slot now;
                                // the exchange finishes in the background and is dropped.
                                detached.fetch_add(1, Ordering::SeqCst);
                                if state
                                    .compare_exchange(0, 2, Ordering::SeqCst, Ordering::SeqCst)
                                    .is_ok()
                                {
                                    drop(guard);
                                    return Err(Error(5, "adapter abandoned".into()));
                                }
                                detached.fetch_sub(1, Ordering::SeqCst);
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            break Err(Error(5, "adapter transport".into()));
                        }
                    }
                };
                drop(guard);
                Ok(Reply {
                    version: VERSION,
                    id,
                    status: "ok".into(),
                    body: outcome?,
                })
            }
            _ => Err(Error(5, "adapter protocol mismatch".into())),
        }
    })();
    let response = reply.unwrap_or_else(|error| Reply {
        version: VERSION,
        id: correlation,
        status: match error.1.as_ref() {
            "adapter bounds" => "bounds",
            "adapter deadline" => "deadline",
            "adapter fingerprint" => "fingerprint",
            "adapter credential" => "credential",
            "adapter policy" => "policy",
            "adapter dispatch" => "dispatch",
            "adapter stopping" => "stopping",
            "adapter busy" => "busy",
            "privacy configuration changing; retry operation" => "settings_busy",
            "credential change or request is in progress" => "credential_busy",
            "adapter connect" => "connect",
            "adapter connect timeout" => "connect_timeout",
            "adapter timeout" => "timeout",
            "adapter body timeout" => "body_timeout",
            "adapter body" => "body",
            "adapter transport" => "transport",
            "adapter auth" => "auth",
            "adapter rate limit" => "rate_limit",
            "adapter rejected" => "rejected",
            _ => "error",
        }
        .into(),
        body: String::new(),
    });
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = write_frame(&mut stream, &bytes);
    }
}
pub fn serve(paths: Paths) -> Result<()> {
    if !paths.load()?.semantic {
        return Err(Error(5, "semantic networking is disabled".into()));
    }
    let dir = runtime(&paths)?;
    let lock = crate::config::private_file(&dir.join("instance.lock"))?;
    if fs2::FileExt::try_lock_exclusive(&lock).is_err() {
        return Ok(());
    }
    // This adapter is now the sole owner of `dir`; reclaim directories left
    // behind by earlier identities that have no live owner.
    reclaim_stale_runtime(&dir);
    let path = socket(&paths)?;
    if check_socket(&path)? {
        fs::remove_file(&path)
            .map_err(|_| Error(7, "cannot remove stale adapter socket".into()))?;
    }
    crate::config::atomic_write(&dir.join("profile.id"), profile_id(&paths).as_bytes())?;
    let listener =
        UnixListener::bind(&path).map_err(|_| Error(7, "cannot bind adapter socket".into()))?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::listen(listener.as_raw_fd(), MAX_ACTIVE as libc::c_int) } != 0 {
        return Err(Error(7, "cannot bound adapter pending connections".into()));
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|_| Error(7, "cannot privatize adapter socket".into()))?;
    let original = fs::symlink_metadata(&path)
        .map_err(|_| Error(7, "cannot inspect adapter socket".into()))?;
    listener
        .set_nonblocking(true)
        .map_err(|_| Error(7, "cannot bound adapter accept".into()))?;
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(240))
        .build()
        .map_err(|_| Error(5, "TLS client unavailable".into()))?;
    let active = Arc::new(AtomicUsize::new(0));
    let stopping = Arc::new(AtomicBool::new(false));
    let sending = Arc::new(AtomicBool::new(false));
    let detached = Arc::new(AtomicUsize::new(0));
    let mut last = Instant::now();
    let mut last_policy_check = Instant::now();
    while !stopping.load(Ordering::SeqCst)
        && (last.elapsed() < IDLE || active.load(Ordering::SeqCst) > 0)
    {
        if last_policy_check.elapsed() >= Duration::from_secs(1) {
            if !paths.load().is_ok_and(|cfg| cfg.semantic) {
                stopping.store(true, Ordering::SeqCst);
                break;
            }
            last_policy_check = Instant::now();
        }
        match listener.accept() {
            Ok((stream, _)) => {
                last = Instant::now();
                // On macOS an accepted socket can inherit the nonblocking listener flag.
                // A framed request larger than the first kernel read then fails with
                // WouldBlock before the rest of its bytes arrive.
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_ACTIVE {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (a, p, c, s, busy, gone) = (
                    active.clone(),
                    paths.clone(),
                    client.clone(),
                    stopping.clone(),
                    sending.clone(),
                    detached.clone(),
                );
                std::thread::spawn(move || {
                    process(stream, p, c, s, busy, gone, crate::provider::ENDPOINT);
                    a.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(_) => break,
        }
    }
    if let Ok(current) = fs::symlink_metadata(&path)
        && current.ino() == original.ino()
        && current.dev() == original.dev()
    {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn abandoned_exchange_frees_the_send_slot_at_once() {
        let (_temp, paths, fingerprint) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        // A provider that stalls well past the moment the user gives up.
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_http_request(&mut stream);
            std::thread::sleep(Duration::from_millis(1200));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        });
        let (caller, worker) = UnixStream::pair().unwrap();
        let sending = Arc::new(AtomicBool::new(false));
        let detached = Arc::new(AtomicUsize::new(0));
        let (p, url, busy, gone) = (
            paths.clone(),
            endpoint.clone(),
            sending.clone(),
            detached.clone(),
        );
        let task = std::thread::spawn(move || {
            process(
                worker,
                p,
                reqwest::blocking::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap(),
                Arc::new(AtomicBool::new(false)),
                busy,
                gone,
                &url,
            )
        });
        let abort = Arc::new(Abort::new());
        let client_abort = abort.clone();
        let client = std::thread::spawn(move || {
            let mut key = "synthetic-test-key".to_owned();
            exchange(
                caller,
                "00000000000000000000000000000001",
                &fingerprint,
                "absent",
                &mut key,
                b"{}",
                Instant::now() + Duration::from_secs(3),
                &client_abort,
            )
        });
        let until = |what: &str, check: &dyn Fn() -> bool, limit: Duration| {
            let end = Instant::now() + limit;
            while !check() {
                assert!(Instant::now() < end, "{what}");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        until(
            "request in flight",
            &|| sending.load(Ordering::SeqCst),
            Duration::from_secs(2),
        );
        std::thread::sleep(Duration::from_millis(150));
        let abandoned = Instant::now();
        abort.abort();
        assert!(client.join().unwrap().is_err());
        until(
            "abandonment must free the send slot",
            &|| !sending.load(Ordering::SeqCst),
            Duration::from_millis(300),
        );
        assert!(abandoned.elapsed() < Duration::from_millis(400));
        assert_eq!(detached.load(Ordering::SeqCst), 1);
        task.join().unwrap();
        server.join().unwrap();
        until(
            "the background exchange is accounted for when it ends",
            &|| detached.load(Ordering::SeqCst) == 0,
            Duration::from_secs(3),
        );
    }

    #[test]
    fn retained_child_starts_in_its_own_session() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("1");
        detach_session(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        assert_eq!(
            unsafe { libc::getsid(child.id() as libc::pid_t) },
            child.id() as libc::pid_t
        );
        child.kill().unwrap();
        child.wait().unwrap();
    }

    fn fixture() -> (tempfile::TempDir, Paths, String) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let paths = Paths {
            config: root.join("config/config.json"),
            data: root.join("data"),
            cache: root.join("cache"),
            home: root,
        };
        let cfg = Config {
            semantic: true,
            ..Config::default()
        };
        paths.save(&cfg, "absent").unwrap();
        let fingerprint = paths.fingerprint().unwrap();
        drop(crate::provider::ProviderState::open(&paths).unwrap());
        let db = rusqlite::Connection::open(paths.cache.join("semantic-drivers.db")).unwrap();
        for n in 0..2 {
            db.execute(
                "INSERT INTO dispatch VALUES(?1,'MayHaveSent',?2)",
                rusqlite::params![format!("{n:032x}"), crate::now()],
            )
            .unwrap();
        }
        (temp, paths, fingerprint)
    }
    #[test]
    fn new_adapter_uses_a_different_socket_from_version_one() {
        let (_temp, paths, _) = fixture();
        let old_profile = [
            paths.config.as_os_str().as_encoded_bytes(),
            b"\0",
            paths.cache.as_os_str().as_encoded_bytes(),
        ]
        .concat();
        let old_name = format!(
            "jj-{}-{}",
            unsafe { libc::geteuid() },
            &crate::digest(&old_profile)[..24]
        );
        assert_ne!(
            runtime(&paths).unwrap().file_name().unwrap(),
            std::ffi::OsStr::new(&old_name)
        );
    }
    #[test]
    fn cloud_provider_requests_cross_ipc_with_their_wire_format_and_key() {
        for provider in [
            crate::config::Provider::ClefFlash,
            crate::config::Provider::OpenAI,
        ] {
            let (_temp, paths, _) = fixture();
            let mut cfg = paths.load().unwrap();
            cfg.provider = provider;
            cfg.cloudflare_account_id = "00000000000000000000000000000001".into();
            paths.save(&cfg, &paths.fingerprint().unwrap()).unwrap();
            let fingerprint = paths.fingerprint().unwrap();
            let connection = provider.driver().connection(&cfg, None).unwrap();
            let account = connection.context;
            let route = reqwest::Url::parse(&connection.url)
                .unwrap()
                .path()
                .to_owned();
            let model = provider.driver().model();
            let task = serde_json::json!({"state":{"query":"backend"},
                "questions":{"destination":{"type":"choice","instructions":"Choose", "criteria":{"d1":"server", "none":"No match"}}}});
            let payload = provider.driver().prepare(task, model).unwrap();
            let expected_payload = payload.clone();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}{}", listener.local_addr().unwrap(), route);
            let server = std::thread::spawn(move || {
                let (mut http, _) = listener.accept().unwrap();
                let request = read_http_request(&mut http);
                let end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
                let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                assert!(headers.starts_with(&format!("post {route} ")));
                assert!(headers.contains("authorization: bearer synthetic-provider-key"));
                assert!(headers.contains("content-type: application/json"));
                assert_eq!(&request[end + 4..], &expected_payload);
                let response = if provider == crate::config::Provider::OpenAI {
                    serde_json::json!({"model":model,"answers":[{"name":"destination","type":"choice","choice":"d1",
                        "probabilities":[{"value":"d1","probability":0.9},{"value":"none","probability":0.1}],"confidence":0.9}],
                        "usage":{"input_tokens":1,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},"output_tokens":0,
                            "output_tokens_details":{"reasoning_tokens":0},"total_tokens":1}})
                } else {
                    serde_json::json!({"success":true,"errors":[],"messages":[],"result":{
                        "model":model,"answers":{"destination":{"type":"choice","choice":"d1","probabilities":{"d1":0.9,"none":0.1},"confidence":0.9}},
                        "usage":{"input_tokens":1,"output_tokens":0}}})
                }.to_string();
                write!(
                    http,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            });
            let (caller, helper) = UnixStream::pair().unwrap();
            let task = std::thread::spawn(move || {
                process(
                    helper,
                    paths,
                    reqwest::blocking::Client::builder()
                        .no_proxy()
                        .build()
                        .unwrap(),
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(AtomicUsize::new(0)),
                    &url,
                )
            });
            let mut key = "synthetic-provider-key".into();
            let body = exchange_for(
                caller,
                "00000000000000000000000000000001",
                &fingerprint,
                "absent",
                &account,
                &mut key,
                &payload,
                Instant::now() + Duration::from_secs(3),
                &Abort::new(),
            )
            .unwrap();
            assert!(key.is_empty());
            assert_eq!(
                crate::provider::validate(&body, &payload).unwrap(),
                Some("d1".into())
            );
            task.join().unwrap();
            server.join().unwrap();
        }
    }

    fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
        // A read may contain only headers. Closing with an unread request body
        // can reset the connection on Linux and hide the fixture's HTTP status.
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buf[..n]);
            if let Some(end) = data.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = std::str::from_utf8(&data[..end]).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|n| n.parse().ok())
                    })
                    .unwrap();
                if data.len() >= end + 4 + length {
                    return data;
                }
            }
        }
    }
    fn consume_http(stream: &mut TcpStream) {
        read_http_request(stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\n{}")
            .unwrap();
    }
    #[test]
    fn http_fixture_reads_the_body_before_replying() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let task = std::thread::spawn(move || {
            read_http_request(&mut server);
            sender.send(()).unwrap();
        });
        client
            .write_all(b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\n{")
            .unwrap();
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        client.write_all(b"}").unwrap();
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        task.join().unwrap();
    }

    #[test]
    fn separate_ipc_calls_reuse_one_http_connection() {
        let (_temp, paths, fingerprint) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            consume_http(&mut stream);
            consume_http(&mut stream);
        });
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap();
        for n in 0..2 {
            let (caller, worker) = UnixStream::pair().unwrap();
            let (p, c, url) = (paths.clone(), client.clone(), endpoint.clone());
            let task = std::thread::spawn(move || {
                process(
                    worker,
                    p,
                    c,
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(AtomicUsize::new(0)),
                    &url,
                )
            });
            let mut key = "synthetic-test-key".to_owned();
            let id = format!("{n:032x}");
            let body = exchange(
                caller,
                &id,
                &fingerprint,
                "absent",
                &mut key,
                b"{}",
                Instant::now() + Duration::from_millis(1000),
                &Abort::new(),
            )
            .unwrap();
            assert_eq!(body, b"{}");
            assert!(key.is_empty());
            task.join().unwrap();
        }
        server.join().unwrap();
    }
    #[test]
    fn interactive_deadline_accepts_a_response_after_five_seconds() {
        let (_temp, paths, fingerprint) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(5250));
            consume_http(&mut stream);
        });
        let (caller, worker) = UnixStream::pair().unwrap();
        let task = std::thread::spawn(move || {
            process(
                worker,
                paths,
                reqwest::blocking::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
                &endpoint,
            )
        });
        let mut key = "synthetic-test-key".to_owned();
        let body = exchange(
            caller,
            "00000000000000000000000000000000",
            &fingerprint,
            "absent",
            &mut key,
            b"{}",
            Instant::now() + Duration::from_secs(8),
            &Abort::new(),
        )
        .unwrap();
        assert_eq!(body, b"{}");
        task.join().unwrap();
        server.join().unwrap();
    }
    #[test]
    fn adapter_rejects_a_deadline_above_ten_seconds_before_network() {
        let (_temp, paths, fingerprint) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        let (caller, worker) = UnixStream::pair().unwrap();
        let task = std::thread::spawn(move || {
            process(
                worker,
                paths,
                reqwest::blocking::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
                &endpoint,
            )
        });
        let mut key = "synthetic-test-key".to_owned();
        let error = exchange(
            caller,
            "00000000000000000000000000000000",
            &fingerprint,
            "absent",
            &mut key,
            b"{}",
            Instant::now() + Duration::from_secs(11),
            &Abort::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.1,
            "adapter request bounds invalid; browse locally: jjump --offline query --interactive"
        );
        assert!(key.is_empty());
        task.join().unwrap();
        assert!(listener.accept().is_err());
    }
    #[test]
    fn policy_change_and_socket_substitution_fail_closed() {
        let (_temp, paths, fingerprint) = fixture();
        let mut changed = paths.load().unwrap();
        changed.semantic = false;
        paths.save(&changed, &fingerprint).unwrap();
        let (caller, worker) = UnixStream::pair().unwrap();
        let p = paths.clone();
        let task = std::thread::spawn(move || {
            process(
                worker,
                p,
                reqwest::blocking::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
                "http://127.0.0.1:1",
            )
        });
        let mut key = "synthetic-test-key".to_owned();
        let result = exchange(
            caller,
            "00000000000000000000000000000001",
            &fingerprint,
            "absent",
            &mut key,
            b"{}",
            Instant::now() + Duration::from_millis(500),
            &Abort::new(),
        );
        assert_eq!(
            result.unwrap_err().1,
            "adapter configuration binding changed; browse locally: jjump --offline query --interactive"
        );
        task.join().unwrap();
        let path = socket(&paths).unwrap();
        std::os::unix::fs::symlink(&paths.config, &path).unwrap();
        assert!(connect(&paths).is_err());
    }
    #[test]
    fn shared_lock_waits_out_a_brief_change_but_not_a_long_one() {
        let (_temp, paths, _fingerprint) = fixture();
        let hold = |millis: u64| {
            let lock =
                crate::config::private_file(&paths.config.parent().unwrap().join("config.lock"))
                    .unwrap();
            fs2::FileExt::lock_exclusive(&lock).unwrap();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(millis));
                drop(lock);
            })
        };
        let brief = hold(40);
        std::thread::sleep(Duration::from_millis(5));
        assert!(shared_lock(|| paths.policy_lock()).is_ok());
        brief.join().unwrap();
        let long = hold(600);
        std::thread::sleep(Duration::from_millis(5));
        let error = shared_lock(|| paths.policy_lock()).unwrap_err();
        assert_eq!(error.1, "privacy configuration changing; retry operation");
        long.join().unwrap();
    }

    #[test]
    fn credential_rotation_rejects_queued_request_before_network() {
        let (_temp, paths, fingerprint) = fixture();
        {
            let _lock = paths.credential_lock(true).unwrap();
            paths.rotate_credential_epoch().unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        let (caller, worker) = UnixStream::pair().unwrap();
        let p = paths.clone();
        let task = std::thread::spawn(move || {
            process(
                worker,
                p,
                reqwest::blocking::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
                &endpoint,
            )
        });
        let mut key = "synthetic-test-key".to_owned();
        assert_eq!(
            exchange(
                caller,
                "00000000000000000000000000000001",
                &fingerprint,
                "absent",
                &mut key,
                b"{}",
                Instant::now() + Duration::from_millis(500),
                &Abort::new(),
            )
            .unwrap_err()
            .1,
            "adapter credential binding changed; browse locally: jjump --offline query --interactive"
        );
        task.join().unwrap();
        assert!(listener.accept().is_err());
    }
    #[test]
    fn slow_reply_does_not_extend_caller_deadline() {
        let (_temp, paths, fingerprint) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_http_request(&mut stream);
            std::thread::sleep(Duration::from_millis(600));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        });
        let (caller, worker) = UnixStream::pair().unwrap();
        let (p, url) = (paths.clone(), endpoint.clone());
        let task = std::thread::spawn(move || {
            process(
                worker,
                p,
                reqwest::blocking::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
                &url,
            )
        });
        let mut key = "synthetic-test-key".to_owned();
        let started = Instant::now();
        let error = exchange(
            caller,
            "00000000000000000000000000000001",
            &fingerprint,
            "absent",
            &mut key,
            b"{}",
            started + Duration::from_millis(250),
            &Abort::new(),
        )
        .unwrap_err();
        assert_eq!(
            error.1,
            "Provider adapter response timed out; browse locally: jjump --offline query --interactive"
        );
        assert!(started.elapsed() < Duration::from_millis(350));
        task.join().unwrap();
        server.join().unwrap();
    }
    #[test]
    fn slow_body_returns_correlated_stage_error_before_caller_deadline() {
        let (_temp, paths, fingerprint) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_http_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{")
                .unwrap();
            std::thread::sleep(Duration::from_millis(600));
            let _ = stream.write_all(b"}");
        });
        let (caller, worker) = UnixStream::pair().unwrap();
        let (p, url) = (paths.clone(), endpoint.clone());
        let task = std::thread::spawn(move || {
            process(
                worker,
                p,
                reqwest::blocking::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
                &url,
            )
        });
        let mut key = "synthetic-test-key".to_owned();
        let started = Instant::now();
        let error = exchange(
            caller,
            "00000000000000000000000000000001",
            &fingerprint,
            "absent",
            &mut key,
            b"{}",
            started + Duration::from_millis(250),
            &Abort::new(),
        )
        .unwrap_err();
        assert!(
            matches!(
                error.1.as_ref(),
                "Provider adapter response body timed out; browse locally: jjump --offline query --interactive"
                    | "Provider adapter response body failed; browse locally: jjump --offline query --interactive"
            ),
            "{}",
            error.1
        );
        assert!(started.elapsed() < Duration::from_millis(350));
        task.join().unwrap();
        server.join().unwrap();
    }
    #[test]
    fn provider_rejection_codes_are_static_and_correlated() {
        for (status, expected) in [
            (
                401,
                "Provider authentication rejected; browse locally: jjump --offline query --interactive",
            ),
            (
                429,
                "Provider rate limit or quota reached; browse locally: jjump --offline query --interactive",
            ),
        ] {
            let (_temp, paths, fingerprint) = fixture();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                read_http_request(&mut stream);
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} Rejected\r\nContent-Length: 19\r\n\r\nprivate server text"
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            });
            let (caller, worker) = UnixStream::pair().unwrap();
            let (p, url) = (paths.clone(), endpoint.clone());
            let task = std::thread::spawn(move || {
                process(
                    worker,
                    p,
                    reqwest::blocking::Client::builder()
                        .no_proxy()
                        .build()
                        .unwrap(),
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(AtomicUsize::new(0)),
                    &url,
                )
            });
            let mut key = "synthetic-test-key".to_owned();
            let error = exchange(
                caller,
                "00000000000000000000000000000001",
                &fingerprint,
                "absent",
                &mut key,
                b"{}",
                Instant::now() + Duration::from_millis(1000),
                &Abort::new(),
            )
            .unwrap_err();
            assert_eq!(error.1, expected);
            assert!(!error.1.contains("private server text"));
            task.join().unwrap();
            server.join().unwrap();
        }
    }

    fn private_dir_with(dir: &Path) {
        fs::create_dir(dir).unwrap();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fn backdate(path: &Path, age: Duration) {
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .checked_sub(age)
            .unwrap();
        let ts = libc::timespec {
            tv_sec: when.as_secs() as libc::time_t,
            tv_nsec: when.subsec_nanos() as libc::c_long,
        };
        let times = [ts, ts];
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(
            unsafe { libc::utimensat(libc::AT_FDCWD, name.as_ptr(), times.as_ptr(), 0) },
            0
        );
    }
    fn stale() -> Duration {
        RUNTIME_STALE + Duration::from_secs(1)
    }

    #[test]
    fn runtime_reclamation_removes_only_ownerless_directories() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let uid = unsafe { libc::geteuid() };
        let keep = base.join(format!("jj-{uid}-current"));
        private_dir_with(&keep);
        let stale_dir = base.join(format!("jj-{uid}-stale"));
        private_dir_with(&stale_dir);
        crate::config::private_file(&stale_dir.join("instance.lock")).unwrap();
        backdate(&stale_dir, stale());
        let recent = base.join(format!("jj-{uid}-recent"));
        private_dir_with(&recent);
        let other_user = base.join(format!("jj-{}-other", uid + 1));
        private_dir_with(&other_user);
        backdate(&other_user, stale());
        let unexpected = base.join(format!("jj-{uid}-unexpected"));
        private_dir_with(&unexpected);
        fs::write(unexpected.join("keep-me"), b"x").unwrap();
        backdate(&unexpected, stale());

        reclaim_runtime_in(&base, &keep, uid);

        assert!(!stale_dir.exists(), "ownerless stale directory must go");
        assert!(keep.exists(), "the caller's live directory must stay");
        assert!(recent.exists(), "a recently touched directory must stay");
        assert!(
            other_user.exists(),
            "a directory named for another user must stay"
        );
        assert!(
            unexpected.exists(),
            "unrecognised content must never be deleted"
        );
    }

    #[test]
    fn runtime_reclamation_keeps_a_held_lock_and_a_live_socket() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let uid = unsafe { libc::geteuid() };
        let keep = base.join(format!("jj-{uid}-current"));
        private_dir_with(&keep);
        let locked = base.join(format!("jj-{uid}-locked"));
        private_dir_with(&locked);
        let lock = crate::config::private_file(&locked.join("instance.lock")).unwrap();
        fs2::FileExt::try_lock_exclusive(&lock).unwrap();
        backdate(&locked, stale());
        let listening = base.join(format!("jj-{uid}-listening"));
        private_dir_with(&listening);
        let listener = UnixListener::bind(listening.join("jev.sock")).unwrap();
        fs::set_permissions(
            listening.join("jev.sock"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        backdate(&listening, stale());

        reclaim_runtime_in(&base, &keep, uid);

        assert!(locked.exists(), "a held instance lock means a live owner");
        assert!(
            listening.exists(),
            "a socket that accepts a connection means a live owner"
        );
        drop(listener);
        assert!(fs2::FileExt::try_lock_exclusive(&lock).is_ok());
    }

    #[test]
    fn runtime_reclamation_removes_a_directory_with_only_a_dead_socket() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let uid = unsafe { libc::geteuid() };
        let keep = base.join(format!("jj-{uid}-current"));
        private_dir_with(&keep);
        let dead = base.join(format!("jj-{uid}-dead"));
        private_dir_with(&dead);
        {
            let listener = UnixListener::bind(dead.join("jev.sock")).unwrap();
            fs::set_permissions(dead.join("jev.sock"), fs::Permissions::from_mode(0o600)).unwrap();
            drop(listener);
        }
        backdate(&dead, stale());

        reclaim_runtime_in(&base, &keep, uid);

        assert!(!dead.exists(), "a dead socket has no live owner");
    }
}
