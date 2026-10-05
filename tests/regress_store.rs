//! Regressions for iteration 0027 workstream W3: visit-store corruption
//! fail-closed behaviour (AC-003), store contention (AC-008) and state-path
//! hardening (AC-007).
use j_jump::{
    config::{Config, Paths, private_dir},
    store::Store,
};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    process::Command,
};

/// Path to the binary under test. `JJ_TEST_BIN` lets the same tests be pointed
/// at another build (used to prove each AC-003 shape genuinely discriminates
/// against the pre-fix binary); it defaults to this crate's own binary.
fn bin() -> String {
    std::env::var("JJ_TEST_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_jjump").to_string())
}

struct Bank {
    _t: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state: PathBuf,
    target: PathBuf,
}
impl Bank {
    fn new() -> Self {
        let t = tempfile::tempdir().unwrap();
        // Resolve symlinks in the base. On macOS the default temp dir lives
        // under `/var/folders/...` and `/var` is a link to `/private/var`; a
        // state tree under a linked ancestor would be rejected by the *old*
        // ancestry check for a reason unrelated to the defect under test, so
        // every fixture here must sit on a path with no symbolic links above it.
        let root = t.path().canonicalize().unwrap();
        let home = root.join("home");
        let state = root.join("state");
        let target = home.join("work");
        fs::create_dir_all(&target).unwrap();
        Self {
            _t: t,
            root,
            home,
            state,
            target,
        }
    }
    fn db(&self) -> PathBuf {
        self.state.join("data/visits.db")
    }
    fn paths(&self) -> Paths {
        Paths {
            config: self.state.join("config/config.json"),
            data: self.state.join("data"),
            cache: self.state.join("cache"),
            home: self.home.clone(),
        }
    }
    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(bin())
            .args(args)
            .current_dir(&self.target)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("J_JUMP_HOME", &self.state)
            .env("PWD", &self.target)
            .output()
            .unwrap()
    }
    fn record(&self) -> std::process::Output {
        self.command(&["record", "--", self.target.to_str().unwrap()])
    }
    /// Total recorded visits across every path.
    fn rows(&self) -> i64 {
        let db = rusqlite::Connection::open(self.db()).unwrap();
        db.query_row("SELECT sum(count) FROM visits", [], |r| r.get(0))
            .unwrap()
    }
}

/// AC-003: unreadable content is never replaced, including zero bytes.
///
/// `Bank::new` resolves the fixture root, so the state path has no symbolic
/// ancestor. That matters: with a linked ancestor the pre-fix binary rejects
/// the path for an unrelated reason and the test would not discriminate. Run
/// with `JJ_TEST_BIN` pointed at the pre-fix binary, this test fails because
/// the zero-byte store is silently replaced (rc 0), which is the defect.
#[test]
fn empty_store_file_fails_closed_and_is_preserved() {
    let b = Bank::new();
    assert_eq!(b.record().status.code(), Some(0));
    assert_eq!(b.rows(), 1);
    fs::write(b.db(), b"").unwrap();
    for _ in 0..3 {
        let out = b.command(&["record", "--", b.target.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(7), "{out:?}");
        assert!(out.stdout.is_empty());
        // The file is neither recreated nor deleted.
        assert!(b.db().exists());
        assert_eq!(fs::metadata(b.db()).unwrap().len(), 0);
    }
    // A read-only command fails closed too instead of reporting an empty history.
    let out = b.command(&["query", "work"]);
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    assert_eq!(fs::metadata(b.db()).unwrap().len(), 0);
}

/// AC-003: the other corruption shapes keep their fail-closed behaviour.
#[test]
fn other_corrupt_store_shapes_fail_closed() {
    let b = Bank::new();
    assert_eq!(b.record().status.code(), Some(0));
    let good = fs::read(b.db()).unwrap();
    assert!(good.len() > 100);
    let shapes: Vec<Vec<u8>> = vec![
        good[..100].to_vec(),
        (0..4096u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect(),
        [
            b"SQLite format 3\0".as_slice(),
            &(0..200u32)
                .map(|i| (i.wrapping_mul(40503) >> 7) as u8)
                .collect::<Vec<_>>(),
        ]
        .concat(),
    ];
    for shape in shapes {
        fs::write(b.db(), &shape).unwrap();
        let out = b.command(&["record", "--", b.target.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(7), "{out:?}");
        assert_eq!(fs::read(b.db()).unwrap(), shape, "bytes must be preserved");
    }
}

/// AC-007: an ordinary state directory reached through a symbolic-linked
/// ancestor is accepted. The base is resolved first, so the only link in play
/// is the `alias` this test creates.
#[test]
fn symlinked_ancestor_is_accepted() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let real = base.join("real");
    fs::create_dir(&real).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    let child = real.join("state");
    fs::create_dir(&child).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = base.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    // The link is an ancestor; the leaf `state` is a real private directory.
    private_dir(&alias.join("state")).unwrap();
    let paths = Paths {
        config: alias.join("state/config/config.json"),
        data: alias.join("state/data"),
        cache: alias.join("state/cache"),
        home: real.clone(),
    };
    Store::open(&paths, false).unwrap();
    // End to end: a whole CLI run under a symbolic-linked state root works.
    let bank = Bank::new();
    assert_eq!(bank.record().status.code(), Some(0));
    let linked = bank.root.join("linked-state");
    std::os::unix::fs::symlink(&bank.state, &linked).unwrap();
    let out = Command::new(bin())
        .args(["record", "--", bank.target.to_str().unwrap()])
        .current_dir(&bank.target)
        .env_clear()
        .env("HOME", &bank.home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("J_JUMP_HOME", &linked)
        .env("PWD", &bank.target)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(bank.rows(), 2);
}

/// AC-007: a state directory or state file that IS a link is still refused.
/// The base is resolved so a linked ancestor cannot be the cause of the error.
#[test]
fn linked_state_directory_and_file_are_refused() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let real = base.join("real");
    fs::create_dir(&real).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    let dirlink = base.join("dirlink");
    std::os::unix::fs::symlink(&real, &dirlink).unwrap();
    assert!(private_dir(&dirlink).is_err());

    let bank = Bank::new();
    assert_eq!(bank.record().status.code(), Some(0));
    let db = bank.db();
    let moved = bank.state.join("data/real.db");
    fs::rename(&db, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &db).unwrap();
    private_dir(&bank.paths().data).unwrap();
    assert!(Store::open(&bank.paths(), false).is_err());
}

/// AC-007: a non-private (group/other readable) state directory is refused.
/// The base is resolved so only the mode can be the cause.
#[test]
fn non_private_state_directory_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().canonicalize().unwrap().join("loose");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_dir(&dir).is_err());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(private_dir(&dir).is_err());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    private_dir(&dir).unwrap();
}

/// AC-007: a state directory owned by another account is refused.
///
/// Fabricating a foreign owner needs `chown`, so this test is explicitly
/// `#[ignore]`d rather than quietly passing on an unprivileged machine: run
/// `cargo test -- --ignored` as root to execute it. If it is forced without
/// root it fails loudly instead of reporting a pass.
#[test]
#[ignore = "requires root to chown the state directory to a foreign uid"]
fn wrong_owner_state_directory_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().canonicalize().unwrap().join("owned");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "this test chowns to uid 1; run it as root with --ignored"
    );
    let c = std::ffi::CString::new(dir.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::chown(c.as_ptr(), 1, 1) }, 0);
    assert_eq!(fs::metadata(&dir).unwrap().uid(), 1);
    assert!(private_dir(&dir).is_err());
}

/// AC-007: the configuration directory is validated whenever it exists, even
/// when it holds no `config.json`. Previously `Paths::load` returned the
/// default before touching the directory, so `doctor` reported a healthy state
/// while every other command failed there. An absent directory is a genuine
/// first run and must stay usable.
#[test]
fn config_directory_is_validated_without_a_config_file() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let home = base.join("home");
    fs::create_dir(&home).unwrap();
    let state = base.join("state");
    fs::create_dir(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    let cfg = state.join("config");
    fs::create_dir(&cfg).unwrap();
    fs::set_permissions(&cfg, fs::Permissions::from_mode(0o755)).unwrap();
    let paths = |config: PathBuf| Paths {
        config,
        data: state.join("data"),
        cache: state.join("cache"),
        home: home.clone(),
    };
    let p = paths(cfg.join("config.json"));
    assert!(!p.config.exists());
    assert!(
        p.load().is_err(),
        "0755 config dir with no file reported usable"
    );
    fs::set_permissions(&cfg, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        p.load().is_ok(),
        "0700 config dir with no file must stay usable"
    );

    // A symbolic-linked configuration directory is refused without a file too.
    let real = base.join("realcfg");
    fs::create_dir(&real).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir(&cfg).unwrap();
    std::os::unix::fs::symlink(&real, &cfg).unwrap();
    assert!(
        paths(cfg.join("config.json")).load().is_err(),
        "symlinked config dir with no file reported usable"
    );
    fs::remove_file(&cfg).unwrap();

    // No configuration directory at all: a brand-new user stays healthy.
    let first = paths(state.join("absent/config.json"));
    assert!(!first.config.parent().unwrap().exists());
    assert!(first.load().is_ok(), "a genuine first run must stay usable");
}

/// AC-007: the same distinction through `doctor` — present but unusable is
/// reported with a real cause, absent stays healthy.
#[test]
fn doctor_reports_a_present_but_unusable_config_directory() {
    let bad = Bank::new();
    let cfg = bad.state.join("config");
    fs::create_dir_all(&cfg).unwrap();
    fs::set_permissions(&cfg, fs::Permissions::from_mode(0o755)).unwrap();
    let out = bad.command(&["doctor", "--json"]);
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("configuration directory"), "{text}");
    assert!(!text.contains(r#""cause":"none""#), "{text}");

    let first = Bank::new();
    let ok = first.command(&["doctor", "--json"]);
    assert_eq!(ok.status.code(), Some(0), "{ok:?}");
    assert!(
        String::from_utf8_lossy(&ok.stdout).contains(r#""cause":"none""#),
        "first run must report no cause"
    );
}

/// AC-007: non-private mode and hard links are refused on the visit store file
/// itself (`private_file` -> `check_file`), not only on its directory.
#[test]
fn state_file_mode_and_link_checks_are_enforced() {
    let b = Bank::new();
    assert_eq!(b.record().status.code(), Some(0));
    fs::set_permissions(b.db(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Store::open(&b.paths(), false).is_err(), "0644 db accepted");
    fs::set_permissions(b.db(), fs::Permissions::from_mode(0o640)).unwrap();
    assert!(Store::open(&b.paths(), false).is_err(), "0640 db accepted");
    fs::set_permissions(b.db(), fs::Permissions::from_mode(0o600)).unwrap();
    Store::open(&b.paths(), false).unwrap();

    let alias = b.state.join("data/hardlinked.db");
    fs::hard_link(b.db(), &alias).unwrap();
    assert_eq!(fs::metadata(b.db()).unwrap().nlink(), 2);
    assert!(
        Store::open(&b.paths(), false).is_err(),
        "nlink=2 db accepted"
    );
    fs::remove_file(&alias).unwrap();
    assert_eq!(fs::metadata(b.db()).unwrap().nlink(), 1);
    Store::open(&b.paths(), false).unwrap();
}

/// AC-007: every state opener applies the same leaf checks. Each file the tool
/// opens is tampered with in turn and the open must fail: `private_read` for the
/// configuration and credential epoch, `private_file` for the config lock and
/// the visit store, `private_sidecar` for SQLite's journal, and `private_dir`
/// for the containing directories.
#[test]
fn every_state_opener_applies_the_leaf_checks() {
    let b = Bank::new();
    let paths = b.paths();
    paths.save(&Config::default(), "absent").unwrap();
    assert!(paths.load().is_ok());

    // config.json through private_read: non-private mode, then hard link.
    fs::set_permissions(&paths.config, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(paths.load().is_err(), "0644 config accepted");
    fs::set_permissions(&paths.config, fs::Permissions::from_mode(0o600)).unwrap();
    let config_alias = b.state.join("config/hardlinked.json");
    fs::hard_link(&paths.config, &config_alias).unwrap();
    assert!(paths.load().is_err(), "nlink=2 config accepted");
    fs::remove_file(&config_alias).unwrap();

    // config.lock through private_file: a symbolic link is refused.
    let lock = b.state.join("config/config.lock");
    let _ = fs::remove_file(&lock);
    let target = b.state.join("config/elsewhere");
    fs::write(&target, b"").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&target, &lock).unwrap();
    assert!(paths.policy_lock().is_err(), "linked config.lock accepted");
    let _ = fs::remove_file(&lock);

    // credential.epoch through private_read: a non-private mode is refused.
    let epoch = b.state.join("config/credential.epoch");
    fs::write(&epoch, b"0".repeat(32)).unwrap();
    fs::set_permissions(&epoch, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(paths.credential_epoch().is_err(), "0644 epoch accepted");
    fs::set_permissions(&epoch, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(paths.credential_epoch().unwrap().len(), 32);

    // SQLite journal through private_sidecar: mode and hard link are refused.
    assert_eq!(b.record().status.code(), Some(0));
    let journal = b.state.join("data/visits.db-journal");
    fs::write(&journal, b"").unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        Store::open(&b.paths(), false).is_err(),
        "0644 journal accepted"
    );
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    Store::open(&b.paths(), false).unwrap();
    let journal_alias = b.state.join("data/journal-alias");
    fs::hard_link(&journal, &journal_alias).unwrap();
    assert!(
        Store::open(&b.paths(), false).is_err(),
        "nlink=2 journal accepted"
    );
    fs::remove_file(&journal_alias).unwrap();
    // A journal that a concurrent writer has already unlinked (nlink 0) is not a
    // privacy failure and must not be rejected.
    Store::open(&b.paths(), false).unwrap();
    fs::remove_file(&journal).unwrap();
}

/// AC-008: a hook write must give up inside the supervisor's 250 ms kill window.
/// A worker SIGKILLed mid-commit silently loses the visit, so the bounded retry
/// must leave time for the supervisor to reap it. The 200 ms limit includes
/// process startup and leaves 50 ms of headroom. Best of two runs tolerates one
/// scheduling hiccup while still rejecting the direct-command wait budget.
#[test]
fn hook_write_gives_up_inside_the_supervisor_budget() {
    use std::time::{Duration, Instant};
    let b = Bank::new();
    assert_eq!(b.record().status.code(), Some(0));
    let mut best = Duration::MAX;
    for _ in 0..2 {
        let holder = rusqlite::Connection::open(b.db()).unwrap();
        holder.busy_timeout(Duration::from_millis(500)).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let start = Instant::now();
        let out = b.record();
        let elapsed = start.elapsed();
        holder.execute_batch("ROLLBACK").unwrap();
        assert_eq!(out.status.code(), Some(7), "{out:?}");
        best = best.min(elapsed);
    }
    assert!(
        best < Duration::from_millis(200),
        "hook write waited {best:?}, past the supervisor kill point"
    );
    assert_eq!(b.rows(), 1, "a failed hook write must not change the store");
}

/// AC-008: concurrent writers must not be rejected by the privacy check while
/// SQLite is between rollback-journal files, and every successful record must
/// be retained.
#[test]
fn concurrent_records_retain_every_success() {
    let b = Bank::new();
    assert_eq!(b.record().status.code(), Some(0));
    let mut children = Vec::new();
    for _ in 0..12 {
        children.push(
            Command::new(bin())
                .args(["record", "--", b.target.to_str().unwrap()])
                .current_dir(&b.target)
                .env_clear()
                .env("HOME", &b.home)
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("J_JUMP_HOME", &b.state)
                .env("PWD", &b.target)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let mut succeeded = 1;
    for child in children {
        let out = child.wait_with_output().unwrap();
        let code = out.status.code();
        assert!(matches!(code, Some(0) | Some(7)), "{out:?}");
        assert!(out.stdout.is_empty());
        if code == Some(0) {
            succeeded += 1;
        } else {
            let text = String::from_utf8_lossy(&out.stderr);
            assert!(
                !text.contains("must be private"),
                "journal race resurfaced: {text}"
            );
        }
    }
    assert_eq!(b.rows(), succeeded);
}

/// A directly opened store still records and reports its schema.
#[test]
fn healthy_store_still_works() {
    let b = Bank::new();
    assert_eq!(b.record().status.code(), Some(0));
    let store = Store::open(&b.paths(), false).unwrap();
    assert_eq!(store.generation().unwrap().split(':').count(), 3);
    assert_eq!(b.rows(), 1);
    let cfg = Config::default();
    assert!(b.paths().allowed(&cfg, &b.target, false));
}
