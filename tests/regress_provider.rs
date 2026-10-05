//! Offline regressions for the explicit picker outcomes (AC-006) and the provider cache
//! schema identity (AC-012b). No network, credential or live provider is used:
//! every helper is a fake `fzf` shell script and every cache is a temp fixture.
use j_jump::{config::Paths, engine::Candidate, picker, provider::ProviderState};
use std::{
    io::Read as _,
    os::{
        fd::FromRawFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

// Tests in one binary share the process environment; serialise the ones that
// mutate PATH/TERM.
static ENV: Mutex<()> = Mutex::new(());

struct EnvGuard {
    saved: Vec<(String, Option<std::ffi::OsString>)>,
}
impl EnvGuard {
    fn new() -> Self {
        Self { saved: Vec::new() }
    }
    fn set(&mut self, key: &str, value: &str) {
        self.saved.push((key.into(), std::env::var_os(key)));
        unsafe { std::env::set_var(key, value) };
    }
    fn remove(&mut self, key: &str) {
        self.saved.push((key.into(), std::env::var_os(key)));
        unsafe { std::env::remove_var(key) };
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(value) => unsafe { std::env::set_var(&key, value) },
                None => unsafe { std::env::remove_var(&key) },
            }
        }
    }
}

/// A controlling-terminal stand-in so `picker::fuzzy` can reach its helper path.
struct Pty {
    file: std::fs::File,
    master: std::os::fd::RawFd,
}
impl Pty {
    fn open() -> Self {
        use std::os::fd::FromRawFd;
        let (mut master, mut slave) = (-1, -1);
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(rc, 0, "openpty failed");
        Self {
            file: unsafe { std::fs::File::from_raw_fd(slave) },
            master,
        }
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        unsafe { libc::close(self.master) };
    }
}

fn candidate(id: &str, path: &str) -> Candidate {
    Candidate {
        id: id.into(),
        path: PathBuf::from(path),
        count: 1,
        last_seen: 0,
        weight: 1.0,
    }
}
fn install_fake_fzf(dir: &Path, body: &str) {
    let script = dir.join("fzf");
    std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[derive(PartialEq)]
enum Outcome {
    Selected,
    Refused,
    Failed,
    Cancelled,
}

#[test]
fn fake_fzf_matrix_never_auto_selects() {
    let _serial = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let fake = base.join("fake");
    std::fs::create_dir(&fake).unwrap();
    let none = base.join("none");
    std::fs::create_dir(&none).unwrap();
    let marker = base.join("EXECUTED");
    let original_path = std::env::var("PATH").unwrap_or_default();
    let pty = Pty::open();
    let candidates = vec![candidate("a", "/tmp/jj-alpha")];

    // (label, helper body, expected outcome when the helper runs)
    let cases: &[(&str, &str, Outcome)] = &[
        (
            "exit 0 with the offered row selects",
            "printf '0\t/tmp/jj-alpha\\0'",
            Outcome::Selected,
        ),
        (
            "exit 0 with a forged row is refused",
            "printf '0\\tforged\\0'",
            Outcome::Refused,
        ),
        ("exit 0 with no output is refused", ":", Outcome::Refused),
        (
            "exit 1 no-match fails explicitly",
            "exit 1",
            Outcome::Failed,
        ),
        ("exit 2 initialization failure", "exit 2", Outcome::Failed),
        ("exit 3 fails explicitly", "exit 3", Outcome::Failed),
        ("exit 127 fails explicitly", "exit 127", Outcome::Failed),
        ("exit 130 is a cancellation", "exit 130", Outcome::Cancelled),
        (
            "SIGINT is a cancellation",
            "kill -INT $$",
            Outcome::Cancelled,
        ),
        ("SIGKILL fails explicitly", "kill -KILL $$", Outcome::Failed),
    ];
    for (label, body, expected) in cases {
        install_fake_fzf(&fake, body);
        let mut env = EnvGuard::new();
        env.set("PATH", &format!("{}:{}", fake.display(), original_path));
        env.set("TERM", "xterm");
        env.remove("J_JUMP_PICKER");
        let result = picker::fuzzy(&pty.file, &candidates, None, false);
        match (expected, &result) {
            (Outcome::Selected, Ok(path)) => {
                assert_eq!(path, Path::new("/tmp/jj-alpha"), "{label}");
            }
            (Outcome::Failed, Err(error)) => {
                assert_eq!(error.0, 5, "{label}: {error}");
            }
            (Outcome::Refused | Outcome::Cancelled, Err(error)) => {
                assert_eq!(error.0, 130, "{label}: {error}");
                // A refused or cancelled helper never yields a destination.
                assert!(result.is_err(), "{label}");
            }
            _ => panic!("{label}: unexpected {result:?}"),
        }
    }

    // A missing helper fails explicitly without any selection.
    {
        let mut env = EnvGuard::new();
        env.set("PATH", &none.display().to_string());
        env.set("TERM", "xterm");
        env.remove("J_JUMP_PICKER");
        assert!(matches!(
            picker::fuzzy(&pty.file, &candidates, None, false),
            Err(j_jump::Error(5, _))
        ));
    }
    // TERM=dumb fails explicitly before the helper is ever spawned.
    {
        install_fake_fzf(&fake, &format!("touch {}", marker.display()));
        let mut env = EnvGuard::new();
        env.set("PATH", &format!("{}:{}", fake.display(), original_path));
        env.set("TERM", "dumb");
        env.remove("J_JUMP_PICKER");
        assert!(matches!(
            picker::fuzzy(&pty.file, &candidates, None, false),
            Err(j_jump::Error(5, _))
        ));
        assert!(!marker.exists(), "dumb terminal must not run the helper");
    }
}

/// Spawn the real CLI on a controlling PTY so the observable selection path is
/// exercised, not just an internal return value.
fn spawn_cli_on_pty(
    args: &[&str],
    env: &[(&str, &str)],
    cwd: &Path,
) -> (std::fs::File, std::process::Child, Arc<Mutex<Vec<u8>>>) {
    let (mut master, mut slave) = (-1, -1);
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0,
        "openpty failed"
    );
    let out_fd = unsafe { libc::dup(slave) };
    let err_fd = unsafe { libc::dup(slave) };
    assert!(out_fd >= 0 && err_fd >= 0, "pty dup failed");
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_jjump"));
    cmd.args(args).current_dir(cwd).env_clear();
    for (key, value) in env {
        cmd.env(key, value);
    }
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.stdin(unsafe { Stdio::from_raw_fd(slave) })
        .stdout(unsafe { Stdio::from_raw_fd(out_fd) })
        .stderr(unsafe { Stdio::from_raw_fd(err_fd) });
    let child = cmd.spawn().expect("spawn jjump");
    let master = unsafe { std::fs::File::from_raw_fd(master) };
    let mut reader = master.try_clone().unwrap();
    let output = Arc::new(Mutex::new(Vec::new()));
    let sink = output.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => sink.lock().unwrap().extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    (master, child, output)
}

/// The observable contract: when the helper does not exit cleanly the numbered
/// picker is offered and a number selects; a genuine cancellation ends in J130
/// with nothing selected. Every case is driven through the real binary.
#[test]
fn helper_outcome_selects_or_cancels_end_to_end() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let home = root.join("home");
    std::fs::create_dir(&home).unwrap();
    let target = home.join("jj-target");
    std::fs::create_dir(&target).unwrap();
    let state = root.join("state");
    let base_path = std::env::var("PATH").unwrap_or_default();
    let fake = root.join("fake");
    std::fs::create_dir(&fake).unwrap();
    let none = root.join("none");
    std::fs::create_dir(&none).unwrap();
    let fake_path = format!("{}:{}", fake.display(), base_path);
    let target_text = target.to_str().unwrap().to_owned();

    let recorded = std::process::Command::new(env!("CARGO_BIN_EXE_jjump"))
        .arg("record")
        .arg("--")
        .arg(&target)
        .current_dir(&target)
        .env_clear()
        .env("HOME", &home)
        .env("J_JUMP_HOME", &state)
        .output()
        .expect("run record");
    assert!(
        recorded.status.success(),
        "record failed: {}",
        String::from_utf8_lossy(&recorded.stderr)
    );

    let cases: &[(&str, Option<&str>, &str, &str, i32)] = &[
        ("exit 1", Some("exit 1"), &fake_path, "xterm", 5),
        ("exit 127", Some("exit 127"), &fake_path, "xterm", 5),
        ("exit 2", Some("exit 2"), &fake_path, "xterm", 5),
        ("exit 130", Some("exit 130"), &fake_path, "xterm", 130),
        ("SIGINT", Some("kill -INT $$"), &fake_path, "xterm", 130),
        ("missing helper", None, "", "xterm", 5),
        ("TERM=dumb", Some("exit 1"), &fake_path, "dumb", 5),
    ];
    for (label, body, path, term, expected) in cases {
        if let Some(body) = body {
            install_fake_fzf(&fake, body);
        }
        let path = if path.is_empty() {
            none.display().to_string()
        } else {
            path.to_string()
        };
        let env = [
            ("HOME", home.display().to_string()),
            ("J_JUMP_HOME", state.display().to_string()),
            ("PATH", path),
            ("TERM", term.to_string()),
            ("J_JUMP_PICKER", "fzf".into()),
        ];
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let (_master, mut child, output) =
            spawn_cli_on_pty(&["query", "--interactive"], &env, &home);
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let status = child.wait().unwrap();
        std::thread::sleep(Duration::from_millis(80));
        let text = String::from_utf8_lossy(&output.lock().unwrap()).to_string();
        assert_eq!(status.code(), Some(*expected), "{label}: {text:?}");
        assert!(!text.contains("Choose a directory"), "{label}: {text:?}");
        assert!(
            !text.contains(&target_text),
            "{label}: no directory may be emitted: {text:?}"
        );
    }
}

fn paths_fixture() -> (tempfile::TempDir, Paths) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let home = root.join("home");
    std::fs::create_dir(&home).unwrap();
    let paths = Paths {
        config: root.join("private/config.json"),
        data: root.join("data"),
        cache: root.join("cache"),
        home,
    };
    (temp, paths)
}
fn cache_path(paths: &Paths) -> PathBuf {
    paths.cache.join("semantic-cache.db")
}
fn user_version(paths: &Paths) -> i64 {
    let db = rusqlite::Connection::open(cache_path(paths)).unwrap();
    db.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

#[test]
fn cache_creation_stamps_the_schema_identity() {
    let (_t, paths) = paths_fixture();
    drop(ProviderState::open(&paths).unwrap());
    assert_eq!(user_version(&paths), 3);
}

#[test]
fn unknown_cache_schema_version_is_refused() {
    for version in [1_i64, 999] {
        let (_t, paths) = paths_fixture();
        drop(ProviderState::open(&paths).unwrap());
        {
            let db = rusqlite::Connection::open(cache_path(&paths)).unwrap();
            db.pragma_update(None, "user_version", version).unwrap();
        }
        let before = std::fs::read(cache_path(&paths)).unwrap();
        assert!(ProviderState::open(&paths).is_err());
        assert_eq!(std::fs::read(cache_path(&paths)).unwrap(), before);
    }
}

#[test]
fn stranger_table_at_version_999_is_refused_and_preserved() {
    // The reported defect: user_version 999 with a stranger table was accepted.
    let (_t, paths) = paths_fixture();
    drop(ProviderState::open(&paths).unwrap());
    {
        let db = rusqlite::Connection::open(cache_path(&paths)).unwrap();
        db.execute_batch("CREATE TABLE stranger(x INTEGER); PRAGMA user_version=999;")
            .unwrap();
    }
    assert!(ProviderState::open(&paths).is_err());
    let db = rusqlite::Connection::open(cache_path(&paths)).unwrap();
    let stranger: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='stranger'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stranger, 1, "a refused cache must not be dropped or reused");
}

#[test]
fn stranger_table_at_version_zero_is_refused() {
    let (_t, paths) = paths_fixture();
    drop(ProviderState::open(&paths).unwrap());
    {
        let db = rusqlite::Connection::open(cache_path(&paths)).unwrap();
        db.execute_batch("CREATE TABLE stranger(x INTEGER); PRAGMA user_version=0;")
            .unwrap();
    }
    assert!(ProviderState::open(&paths).is_err());
}

#[test]
fn mismatched_owned_table_is_refused() {
    let (_t, paths) = paths_fixture();
    drop(ProviderState::open(&paths).unwrap());
    {
        let db = rusqlite::Connection::open(cache_path(&paths)).unwrap();
        db.execute_batch(
            "DROP TABLE cache; CREATE TABLE cache(k TEXT PRIMARY KEY); PRAGMA user_version=0;",
        )
        .unwrap();
    }
    assert!(ProviderState::open(&paths).is_err());
}

#[test]
fn pre_identity_cache_is_rejected_without_mutation() {
    // A populated database without the current identity must not be adopted.
    let (_t, paths) = paths_fixture();
    drop(ProviderState::open(&paths).unwrap());
    {
        let db = rusqlite::Connection::open(cache_path(&paths)).unwrap();
        db.execute_batch("INSERT INTO cache VALUES('kept',X'00',1,1); PRAGMA user_version=0;")
            .unwrap();
    }
    let before = std::fs::read(cache_path(&paths)).unwrap();
    assert!(ProviderState::open(&paths).is_err());
    assert_eq!(std::fs::read(cache_path(&paths)).unwrap(), before);
    assert_eq!(user_version(&paths), 0);
    let db = rusqlite::Connection::open(cache_path(&paths)).unwrap();
    let kept: i64 = db
        .query_row("SELECT count(*) FROM cache WHERE k='kept'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        kept, 1,
        "rejection must not discard existing cache evidence"
    );
}
