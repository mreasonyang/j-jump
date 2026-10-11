//! Bounded, private prompt observation. One profile owner serializes writes;
//! changed prompts enqueue even while its previous worker is still running.
use crate::{
    Error, Result,
    config::{Paths, atomic_write, private_dir, private_file, private_read},
    store::Store,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::UnixDatagram,
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

// Bound foreground queue coordination; database waits happen only in the worker.
const SUBMIT_BUDGET: Duration = Duration::from_millis(300);
const MAX_PENDING: usize = 128;
const MAX_EVENT: usize = 32768;

fn unavailable() -> Error {
    Error(
        7,
        "prompt observation unavailable; retry on the next prompt".into(),
    )
}

/// Shared by submissions and record attempts; destructive history operations
/// take the exclusive lock. Ordinary visit writers never block submissions.
pub fn history_lock(data: &Path, exclusive: bool, deadline: Instant) -> Result<File> {
    private_dir(data)?;
    let lock = private_file(&data.join("observation.lock"))?;
    loop {
        let result = if exclusive {
            fs2::FileExt::try_lock_exclusive(&lock)
        } else {
            fs2::FileExt::try_lock_shared(&lock)
        };
        if result.is_ok() {
            return Ok(lock);
        }
        if Instant::now() >= deadline {
            return Err(Error(7, "visit history changing; retry shortly".into()));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Caller holds the exclusive history lock (or the shared lock and submit lock
/// on first use). Rotate before destructive SQL so a crash cannot replay visits
/// from before a successful deletion. A failed deletion may invalidate queued
/// visits conservatively, but never restores deleted history.
pub fn rotate_epoch(data: &Path) -> Result<()> {
    let path = data.join("observation.epoch");
    if fs::symlink_metadata(&path).is_ok() {
        private_read(&path, 32)?;
    }
    let mut random = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .map_err(|_| unavailable())?;
    atomic_write(&path, &crate::digest(&random).as_bytes()[..32])
}

fn epoch(data: &Path) -> Result<String> {
    let bytes = private_read(&data.join("observation.epoch"), 32)?;
    if bytes.len() != 32 || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return Err(unavailable());
    }
    String::from_utf8(bytes).map_err(|_| unavailable())
}

// Bind existing stores without opening SQLite. Replacing the database file
// must invalidate old queued events even if the private epoch file remains.
fn store_id(data: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(data.join("visits.db")) {
        Ok(m)
            if m.is_file()
                && m.uid() == unsafe { libc::geteuid() }
                && m.mode() & 0o077 == 0
                && m.nlink() == 1 =>
        {
            Ok(Some(format!("{}:{}", m.dev(), m.ino())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        _ => Err(unavailable()),
    }
}

/// Retry only contention, after releasing both privacy/history guards. There is
/// no commit timeout: killing a worker during fsync can silently discard a visit.
/// Each unsuccessful SQLite transaction rolls back before the next attempt;
/// Store::record never returns a pruning error after its visit has committed.
pub fn record_event(
    paths: &Paths,
    path: &Path,
    expected: &str,
    expected_store: Option<&str>,
) -> Result<()> {
    loop {
        let attempt = (|| {
            let _history = history_lock(&paths.data, false, Instant::now())?;
            let actual_store = store_id(&paths.data)?;
            if epoch(&paths.data)? != expected
                || expected_store.is_some_and(|id| actual_store.as_deref() != Some(id))
            {
                return Ok(());
            }
            let _policy = paths.policy_lock()?;
            let cfg = paths.load()?;
            if !cfg.tracking {
                return Ok(());
            }
            let mut db = Store::open(paths, true)?;
            db.record(paths, &cfg, path, None)
        })();
        match attempt {
            Err(Error(7, ref message))
                if matches!(
                    message.as_ref(),
                    "visit store busy; retry shortly"
                        | "visit history changing; retry shortly"
                        | "privacy configuration changing; retry operation"
                ) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    path: PathBuf,
    cwd: PathBuf,
    epoch: String,
    store_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerState {
    pid: u32,
    outstanding: usize,
}

struct Inbox {
    dir: PathBuf,
    socket: PathBuf,
}
impl Inbox {
    fn new(paths: &Paths) -> Result<Self> {
        // This event format uses a history fence instead of a DB generation.
        // Never submit it to a still-live supervisor from the previous format.
        let dir = paths.cache.join("observer-v2");
        private_dir(&dir)?;
        // Unix socket names must fit even with a long HOME/XDG profile path.
        // The socket is private and checked before use; all lock/PID files live
        // in the private profile. No directory names are written to the inbox.
        let identity = serde_json::to_vec(&(2, &paths.config, &paths.data, &paths.cache))
            .map_err(|_| unavailable())?;
        let base = if cfg!(target_os = "macos") {
            "/private/tmp"
        } else {
            "/tmp"
        };
        let socket = Path::new(base)
            .join(format!(
                ".jjump-observe-{}-{}",
                unsafe { libc::geteuid() },
                &crate::digest(&identity)[..32]
            ))
            .join("inbox.sock");
        Ok(Self { dir, socket })
    }
    fn check_socket(&self) -> Result<bool> {
        let dir = self.socket.parent().ok_or_else(unavailable)?;
        if fs::symlink_metadata(dir).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            return Ok(false);
        }
        private_dir(dir)?;
        match fs::symlink_metadata(&self.socket) {
            Ok(m)
                if m.file_type().is_socket()
                    && m.uid() == unsafe { libc::geteuid() }
                    && m.mode() & 0o077 == 0 =>
            {
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            _ => Err(unavailable()),
        }
    }
    fn state(&self) -> Result<OwnerState> {
        let bytes = private_read(&self.dir.join("owner.json"), 128)?;
        let state: OwnerState = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        if state.pid <= 1 || state.outstanding > MAX_PENDING {
            return Err(unavailable());
        }
        Ok(state)
    }
    fn set_state(&self, state: Option<&OwnerState>) -> Result<()> {
        let mut file = private_file(&self.dir.join("owner.json"))?;
        file.set_len(0).map_err(|_| unavailable())?;
        if let Some(state) = state {
            let bytes = serde_json::to_vec(state).map_err(|_| unavailable())?;
            file.write_all(&bytes).map_err(|_| unavailable())?;
        }
        Ok(())
    }
    fn remove_socket(&self) -> Result<()> {
        if self.check_socket()? {
            fs::remove_file(&self.socket).map_err(|_| unavailable())?;
        }
        if let Some(dir) = self.socket.parent() {
            match fs::remove_dir(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(unavailable()),
            }
        }
        Ok(())
    }
}

fn submission_lock(inbox: &Inbox, deadline: Instant) -> Result<File> {
    let lock = private_file(&inbox.dir.join("submit.lock"))?;
    loop {
        if fs2::FileExt::try_lock_exclusive(&lock).is_ok() {
            return Ok(lock);
        }
        if Instant::now() >= deadline {
            return Err(Error(
                7,
                "prompt observation coordination busy; retry on the next prompt".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn command(paths: &Paths, action: &str) -> Result<Command> {
    let mut command = Command::new(std::env::current_exe().map_err(|_| unavailable())?);
    command.env_clear();
    for key in [
        "HOME",
        "PATH",
        "PWD",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "J_JUMP_HOME",
        "J_JUMP_CONFIG",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .arg("--config")
        .arg(&paths.config)
        .arg(action)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

pub fn observe(path: &Path, config: Option<&Path>) -> Result<()> {
    let deadline = Instant::now() + SUBMIT_BUDGET;
    let paths = Paths::discover(config.map(Path::to_path_buf))?;
    let cfg = paths.load()?;
    if !cfg.tracking {
        return Ok(());
    }
    let _policy = paths.policy_lock()?;
    let fingerprint = paths.fingerprint()?;
    let path = crate::engine::valid_path(path)?;
    let cwd = std::env::current_dir().map_err(|_| unavailable())?;
    let logical = fs::metadata(&path).map_err(|_| unavailable())?;
    let actual = fs::metadata(&cwd).map_err(|_| unavailable())?;
    if (logical.dev(), logical.ino()) != (actual.dev(), actual.ino()) {
        return Err(Error(
            6,
            "logical PWD does not identify current directory".into(),
        ));
    }
    if fingerprint != paths.fingerprint()? {
        return Err(unavailable());
    }
    let _history = history_lock(&paths.data, false, deadline)?;
    let inbox = Inbox::new(&paths)?;
    // Submission and empty-queue retirement share this lock. A successful send
    // cannot land in the gap between an owner's final receive and its exit.
    let _submission = submission_lock(&inbox, deadline)?;
    if fs::symlink_metadata(paths.data.join("observation.epoch"))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        rotate_epoch(&paths.data)?;
    }
    let bytes = serde_json::to_vec(&Event {
        path,
        cwd,
        epoch: epoch(&paths.data)?,
        store_id: store_id(&paths.data)?,
    })
    .map_err(|_| unavailable())?;
    if bytes.len() > MAX_EVENT {
        return Err(unavailable());
    }
    let owner = private_file(&inbox.dir.join("owner.lock"))?;
    if fs2::FileExt::try_lock_exclusive(&owner).is_ok() {
        inbox.remove_socket()?;
        inbox.set_state(None)?;
        private_dir(inbox.socket.parent().ok_or_else(unavailable)?)?;
        let socket = UnixDatagram::bind(&inbox.socket).map_err(|_| unavailable())?;
        fs::set_permissions(&inbox.socket, fs::Permissions::from_mode(0o600))
            .map_err(|_| unavailable())?;
        socket.set_nonblocking(true).map_err(|_| unavailable())?;
        let socket_fd = socket.as_raw_fd();
        let owner_fd = owner.as_raw_fd();
        let mut supervisor = command(&paths, "observe-supervise")?;
        supervisor
            .arg("--socket-fd")
            .arg(socket_fd.to_string())
            .arg("--owner-fd")
            .arg(owner_fd.to_string());
        // Only these two already-open descriptors cross exec. The inherited
        // owner lock prevents a second supervisor even during startup.
        unsafe {
            supervisor.pre_exec(move || {
                if libc::setsid() == -1
                    || libc::fcntl(socket_fd, libc::F_SETFD, 0) == -1
                    || libc::fcntl(owner_fd, libc::F_SETFD, 0) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        match supervisor.spawn() {
            Ok(child) => inbox.set_state(Some(&OwnerState {
                pid: child.id(),
                outstanding: 0,
            }))?,
            Err(_) => {
                let _ = inbox.remove_socket();
                return Err(unavailable());
            }
        }
    }
    if !inbox.check_socket()? {
        return Err(unavailable());
    }
    let mut state = inbox.state()?;
    if state.outstanding == MAX_PENDING {
        return Err(Error(
            7,
            "prompt observation queue full; retry on the next prompt".into(),
        ));
    }
    let sender = UnixDatagram::unbound().map_err(|_| unavailable())?;
    sender
        .set_write_timeout(Some(
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
        ))
        .map_err(|_| unavailable())?;
    // Reserve capacity before sending. Count the kernel inbox, in-memory FIFO
    // and active worker together, independently of OS socket buffer settings.
    state.outstanding += 1;
    inbox.set_state(Some(&state))?;
    if let Err(error) = sender.send_to(&bytes, &inbox.socket) {
        state.outstanding -= 1;
        let _ = inbox.set_state(Some(&state));
        if matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ) {
            return Err(Error(
                7,
                "prompt observation inbox full; retry on the next prompt".into(),
            ));
        }
        return Err(unavailable());
    }
    // Acceptance means the complete event is in the owned inbox. The shell
    // may deduplicate this PWD immediately, without waiting for its DB write.
    println!("{}", state.pid);
    Ok(())
}

struct Worker {
    child: Child,
}
impl Worker {
    fn start(paths: &Paths, event: Event) -> Result<Self> {
        let mut command = command(paths, "observe-record")?;
        command.arg("--epoch").arg(event.epoch);
        if let Some(id) = event.store_id {
            command.arg("--store-id").arg(id);
        }
        let child = command
            .arg("--")
            .arg(&event.path)
            .current_dir(event.cwd)
            .env("PWD", &event.path)
            .spawn()
            .map_err(|_| unavailable())?;
        Ok(Self { child })
    }
    fn finished(&mut self) -> Result<bool> {
        Ok(self.child.try_wait().map_err(|_| unavailable())?.is_some())
    }
    fn reap(&mut self) -> Result<()> {
        let _ = self.child.kill();
        self.child.wait().map_err(|_| unavailable())?;
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Even error paths retain ownership until the only worker is reaped.
        let _ = self.reap();
    }
}

fn receive(socket: &UnixDatagram, pending: &mut VecDeque<Event>) -> Result<()> {
    let mut bytes = [0u8; MAX_EVENT + 1];
    while pending.len() < MAX_PENDING {
        match socket.recv(&mut bytes) {
            Ok(len) if len <= MAX_EVENT => {
                if let Ok(event) = serde_json::from_slice(&bytes[..len]) {
                    pending.push_back(event);
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => return Err(unavailable()),
        }
    }
    Ok(())
}

pub fn supervise(paths: Paths, socket_fd: i32, owner_fd: i32) -> Result<()> {
    if socket_fd < 3 || owner_fd < 3 || socket_fd == owner_fd {
        return Err(unavailable());
    }
    for fd in [socket_fd, owner_fd] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(unavailable());
        }
    }
    let socket = unsafe { UnixDatagram::from_raw_fd(socket_fd) };
    let _owner = unsafe { File::from_raw_fd(owner_fd) };
    let inbox = Inbox::new(&paths)?;
    let expected = private_file(&inbox.dir.join("owner.lock"))?
        .metadata()
        .map_err(|_| unavailable())?;
    let actual = _owner.metadata().map_err(|_| unavailable())?;
    if (actual.dev(), actual.ino()) != (expected.dev(), expected.ino())
        || socket
            .local_addr()
            .map_err(|_| unavailable())?
            .as_pathname()
            != Some(&inbox.socket)
    {
        return Err(unavailable());
    }
    let mut pending = VecDeque::new();
    let mut worker: Option<Worker> = None;
    let mut completed = false;
    loop {
        receive(&socket, &mut pending)?;
        if let Some(active) = worker.as_mut()
            && active.finished()?
        {
            completed = true;
        }
        if completed && let Ok(_submission) = submission_lock(&inbox, Instant::now()) {
            let mut state = inbox.state()?;
            state.outstanding = state.outstanding.saturating_sub(1);
            inbox.set_state(Some(&state))?;
            worker = None;
            completed = false;
        }
        if worker.is_none() && !completed {
            if let Some(event) = pending.pop_front() {
                // A removed/unenterable directory affects only that event.
                worker = Worker::start(&paths, event).ok();
                completed = worker.is_none();
                continue;
            }
            if let Ok(_submission) = submission_lock(&inbox, Instant::now()) {
                receive(&socket, &mut pending)?;
                if pending.is_empty() {
                    inbox.remove_socket()?;
                    inbox.set_state(None)?;
                    // Release the owner before submissions resume.
                    drop(_owner);
                    return Ok(());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
