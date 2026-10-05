//! Regression coverage for the iteration 0027 defects owned by the query
//! engine, CLI output and diagnostics workstream (AC-002, AC-004, AC-005,
//! AC-007, AC-010, AC-011), plus iteration 0029 cwd fallback coverage.

use serde_json::Value;
use std::{
    fs,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{PermissionsExt, symlink},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Output, Stdio},
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_jjump");

/// Restore owner permissions on an intentionally unenterable fixture directory
/// so the temporary tree can always be cleaned up.
struct RestoreMode(PathBuf);
impl Drop for RestoreMode {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let cwd = home.join("neutral");
        fs::create_dir_all(&cwd).unwrap();
        Self {
            root: root.clone(),
            home,
            cwd,
            state: root.join("state"),
            _tmp: tmp,
        }
    }
    fn command(&self, cwd: &Path) -> Command {
        let mut command =
            Command::new(std::env::var_os("JJ_TEST_BIN").unwrap_or_else(|| BIN.into()));
        command.current_dir(cwd);
        command.env_clear();
        command.env("HOME", &self.home);
        command.env("PWD", cwd);
        command.env("J_JUMP_HOME", &self.state);
        command.env("LANG", "en_US.UTF-8");
        command.env("PATH", std::env::var_os("PATH").unwrap_or_default());
        if let Some(tmp) = std::env::var_os("TMPDIR") {
            command.env("TMPDIR", tmp);
        }
        command
    }
    fn run_in(&self, cwd: &Path, args: &[&str]) -> Output {
        self.command(cwd).args(args).output().expect("run jjump")
    }
    fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.cwd, args)
    }
    /// Run against an alternate `J_JUMP_HOME`, for tests that need several
    /// independent state trees in one fixture.
    fn run_state(&self, state: &Path, args: &[&str]) -> Output {
        self.command(&self.cwd)
            .env("J_JUMP_HOME", state)
            .args(args)
            .output()
            .expect("run jjump")
    }
    /// A private state tree containing a healthy, freshly created visit store.
    fn state_dir(&self, name: &str) -> PathBuf {
        let state = self.root.join(name);
        fs::create_dir_all(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        let out = self.run_state(&state, &["doctor", "--json"]);
        assert!(
            out.status.success(),
            "cannot seed a healthy state at {}: {}",
            state.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        state
    }
    fn dir(&self, name: &str) -> PathBuf {
        let path = self.home.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
    /// Replace the visit store with the given records through the supported
    /// backup/restore path, so the test never depends on the store schema.
    fn seed(&self, records: &[(PathBuf, u32)]) {
        let items: Vec<String> = records
            .iter()
            .enumerate()
            .map(|(i, (path, count))| {
                format!(
                    r#"{{"id":"d{i}","path":{},"count":{count},"last_seen":100,"weight":{count}}}"#,
                    json(path)
                )
            })
            .collect();
        let file = self.root.join("seed.json");
        fs::write(
            &file,
            format!(r#"{{"schema_version":3,"records":[{}]}}"#, items.join(",")),
        )
        .unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let out = self.run(&["history", "restore", file.to_str().unwrap(), "--apply"]);
        assert!(
            out.status.success(),
            "seeding failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Minimal pseudo-terminal child so a test can drive the interactive picker and
/// change the filesystem inside the selection window.
struct Pty {
    master: fs::File,
    child: std::process::Child,
}

impl Pty {
    fn spawn(f: &Fixture, args: &[&str]) -> Self {
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
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
        let master_file = unsafe { fs::File::from_raw_fd(master) };
        let slave_file = unsafe { fs::File::from_raw_fd(slave) };
        let mut command = f.command(&f.cwd);
        command.env("J_JUMP_PICKER", "numbered");
        command.env("TERM", "xterm");
        command.args(args);
        command.stdin(Stdio::from(slave_file.try_clone().unwrap()));
        command.stdout(Stdio::from(slave_file.try_clone().unwrap()));
        command.stderr(Stdio::from(slave_file.try_clone().unwrap()));
        drop(slave_file);
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self {
            master: master_file,
            child: command.spawn().expect("spawn pty child"),
        }
    }
    fn read(&mut self, needle: &[u8], timeout: Duration) -> Vec<u8> {
        let fd = self.master.as_raw_fd();
        let deadline = Instant::now() + timeout;
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while Instant::now() < deadline {
            let mut timeout = libc::timeval {
                tv_sec: 0,
                tv_usec: 20_000,
            };
            let mut set: libc::fd_set = unsafe { std::mem::zeroed() };
            unsafe { libc::FD_SET(fd, &mut set) };
            let ready = unsafe {
                libc::select(
                    fd + 1,
                    &mut set,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut timeout,
                )
            };
            if ready <= 0 {
                continue;
            }
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
            if out.windows(needle.len()).any(|w| w == needle) {
                return out;
            }
        }
        panic!(
            "timed out waiting for {:?}; saw {:?}",
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&out)
        );
    }
    fn send(&mut self, bytes: &[u8]) {
        let fd = self.master.as_raw_fd();
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(written as usize, bytes.len(), "short pty write");
    }
    /// Drain the remaining output and reap the child.
    fn finish(mut self) -> (ExitStatus, Vec<u8>) {
        let fd = self.master.as_raw_fd();
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let mut timeout = libc::timeval {
                tv_sec: 0,
                tv_usec: 20_000,
            };
            let mut set: libc::fd_set = unsafe { std::mem::zeroed() };
            unsafe { libc::FD_SET(fd, &mut set) };
            let ready = unsafe {
                libc::select(
                    fd + 1,
                    &mut set,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut timeout,
                )
            };
            if ready <= 0 {
                continue;
            }
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
        let status = self.child.wait().expect("wait pty child");
        (status, out)
    }
}

fn json(path: &Path) -> String {
    let text = path.to_str().expect("UTF-8 fixture path");
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn parse(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "expected JSON on stdout, got {:?} / stderr {:?}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// AC-004: a directory the process cannot enter is never a successful
/// destination; a valid alternative wins, otherwise the command fails closed.
#[test]
fn unenterable_directory_is_never_a_success() {
    let f = Fixture::new();
    let denied = f.dir("alpha-denied");
    let open = f.dir("alpha-open");
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o000)).unwrap();
    let _guard = RestoreMode(denied.clone());
    f.seed(&[(denied.clone(), 50), (open.clone(), 1)]);

    let out = f.run(&["query", "alpha"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{}\n", open.display()),
        "the enterable alternative must win over the higher-scoring unenterable match"
    );

    // With no enterable match the command fails with empty stdout.
    let only = f.dir("beta-denied");
    fs::set_permissions(&only, fs::Permissions::from_mode(0o000)).unwrap();
    let _guard2 = RestoreMode(only.clone());
    f.seed(&[(only.clone(), 50)]);
    let out = f.run(&["query", "beta"]);
    assert!(!out.status.success(), "unenterable-only query must fail");
    assert!(
        out.stdout.is_empty(),
        "stdout must stay empty: {:?}",
        out.stdout
    );
}

/// Iteration 0029 AC-002: cwd is a fallback only after all other matching
/// destinations have proved unusable, regardless of cwd's higher score.
#[test]
fn current_directory_fallback_after_unusable_matches() {
    let f = Fixture::new();
    let cwd = f.dir("alpha-current");
    let denied = f.dir("alpha-denied");
    let open = f.dir("alpha-open");
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o600)).unwrap();
    let _guard = RestoreMode(denied.clone());
    assert!(
        j_jump::engine::valid_path(&denied).is_err(),
        "fixture must actually be unenterable by this process"
    );
    f.seed(&[(cwd.clone(), 100), (denied.clone(), 50), (open.clone(), 1)]);

    let out = f.run_in(&cwd, &["query", "alpha"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        out.stdout,
        format!("{}\n", open.display()).as_bytes(),
        "a valid non-cwd match must win even when cwd has the higher score"
    );

    f.seed(&[(cwd.clone(), 100), (denied, 50)]);
    for args in [
        vec!["query", "alpha"],
        vec!["--offline", "query", "alpha"],
        vec!["--offline", "--force-semantic", "query", "alpha"],
    ] {
        let out = f.run_in(&cwd, &args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert_eq!(out.stdout, format!("{}\n", cwd.display()).as_bytes());
        assert!(out.stderr.is_empty(), "{args:?}: {out:?}");
    }

    // Picker modes still require a terminal; the local no-op must not bypass
    // their explicit-selection contract.
    for mode in ["--interactive", "--complete"] {
        let out = f.run_in(&cwd, &["query", mode, "alpha"]);
        assert_eq!(out.status.code(), Some(4), "{mode}: {out:?}");
        assert!(out.stdout.is_empty(), "{mode}: {out:?}");
    }
}

/// Iteration 0029 AC-002: the cwd fallback never broadens the query scope or
/// bypasses history and exclusion requirements.
#[test]
fn current_directory_fallback_preserves_scope_and_policy() {
    let f = Fixture::new();
    let cwd = f.dir("alpha-current");
    let denied = f.dir("alpha-denied");
    let child = f.dir("alpha-current/alpha-child");
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o600)).unwrap();
    let _denied_guard = RestoreMode(denied.clone());
    let _child_guard = RestoreMode(child.clone());
    f.seed(&[
        (cwd.clone(), 100),
        (denied.clone(), 50),
        (child.clone(), 25),
    ]);

    let child_only = f.run_in(&cwd, &["--offline", "query", "alpha", "/"]);
    assert_eq!(child_only.status.code(), Some(6), "{child_only:?}");
    assert!(child_only.stdout.is_empty());

    let excluded = serde_json::to_string(&[&cwd]).unwrap();
    assert!(
        f.run(&["config", "set", "exclude", &excluded])
            .status
            .success()
    );
    let out = f.run_in(&cwd, &["--offline", "query", "alpha"]);
    assert_eq!(out.status.code(), Some(6), "excluded cwd: {out:?}");
    assert!(out.stdout.is_empty());

    assert!(f.run(&["config", "set", "exclude", "[]"]).status.success());
    f.seed(&[(denied.clone(), 50), (child, 25)]);
    let out = f.run_in(&cwd, &["--offline", "query", "alpha"]);
    assert_eq!(out.status.code(), Some(6), "unrecorded cwd: {out:?}");
    assert!(out.stdout.is_empty());

    // A recorded cwd that does not match the query cannot rescue the denied
    // candidate, and the failure retains the existing local diagnostic.
    f.seed(&[(cwd.clone(), 100), (denied, 50)]);
    let out = f.run_in(&cwd, &["query", "denied"]);
    assert_eq!(out.status.code(), Some(6), "nonmatching cwd: {out:?}");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no matching directory is a usable"));
}

/// AC-002 support: the ASCII fast path in `engine::key` must be byte-identical
/// to the general normalise + case-fold path.
#[test]
fn key_ascii_fast_path_matches_case_folding() {
    use unicode_casefold::{Locale, UnicodeCaseFold, Variant};
    use unicode_normalization::UnicodeNormalization;
    for byte in 0..=127u8 {
        let text = (byte as char).to_string();
        let expected: String = text
            .nfc()
            .case_fold_with(Variant::Simple, Locale::NonTurkic)
            .collect();
        assert_eq!(j_jump::engine::key(&text), expected, "ASCII byte {byte}");
    }
    assert_eq!(j_jump::engine::key("/Work/API"), "/work/api");
    // Non-ASCII still normalises and case-folds.
    assert_eq!(j_jump::engine::key("Cafe\u{301}"), "café");
    assert_eq!(j_jump::engine::key("CAFÉ"), "café");
}

/// M1: a recorded path retargeted inside the interactive selection window must
/// not be emitted; the snapshot identity gate must fire.
#[test]
fn retargeted_directory_is_rejected_during_selection() {
    let f = Fixture::new();
    let real_a = f.dir("real-a");
    let real_b = f.dir("real-b");
    let link = f.home.join("link");
    symlink(&real_a, &link).unwrap();
    f.seed(&[(link.clone(), 5)]);

    let mut pty = Pty::spawn(&f, &["query", "--interactive", "link"]);
    let prompt = pty.read(b"> ", Duration::from_secs(10));
    assert!(
        String::from_utf8_lossy(&prompt).contains("link"),
        "picker did not offer the candidate: {:?}",
        String::from_utf8_lossy(&prompt)
    );
    // Retarget the selected directory before the selection is submitted.
    fs::remove_file(&link).unwrap();
    symlink(&real_b, &link).unwrap();
    pty.send(b"1\n");
    let (status, _) = pty.finish();
    assert_eq!(
        status.code(),
        Some(7),
        "a retargeted destination must fail closed"
    );
}

/// M1 companion: an unchanged selection still succeeds through the picker.
#[test]
fn unchanged_interactive_selection_still_succeeds() {
    let f = Fixture::new();
    let real = f.dir("real-c");
    let link = f.home.join("link-c");
    symlink(&real, &link).unwrap();
    f.seed(&[(link.clone(), 5)]);

    let mut pty = Pty::spawn(&f, &["query", "--interactive", "link-c"]);
    pty.read(b"> ", Duration::from_secs(10));
    pty.send(b"1\n");
    let (status, output) = pty.finish();
    assert_eq!(
        status.code(),
        Some(0),
        "picker output: {:?}",
        String::from_utf8_lossy(&output)
    );
    let text = String::from_utf8_lossy(&output);
    let lines: Vec<&str> = text.split(['\n', '\r']).collect();
    assert!(
        lines.contains(&link.to_str().unwrap()),
        "selected path not emitted on its own line: {text:?}"
    );
}

/// AC-004: normally accessible directories are unchanged.
#[test]
fn normal_directories_are_unaffected() {
    let f = Fixture::new();
    let normal = f.dir("gamma/normal");
    f.seed(&[(normal.clone(), 3)]);
    let out = f.run(&["query", "normal"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{}\n", normal.display())
    );
    assert!(f.run(&["doctor", "--json"]).status.success());
}

/// AC-005: a destination name with a control character is refused, not escaped,
/// and human-facing display keeps the existing escape.
#[test]
fn control_characters_are_refused_not_escaped() {
    let f = Fixture::new();
    let cases = [("esc", '\u{1b}'), ("tab", '\t'), ("bel", '\u{7}')];
    let records: Vec<_> = cases
        .iter()
        .map(|(label, ch)| {
            let dir = f.dir(&format!("ctl-{label}-{ch}-end"));
            (dir, 5)
        })
        .collect();
    f.seed(&records);

    for (label, ch) in cases {
        let out = f.run(&["query", &format!("ctl-{label}")]);
        assert!(!out.status.success(), "{label}: must fail");
        assert!(out.stdout.is_empty(), "{label}: stdout must stay empty");
        assert!(
            !out.stdout.contains(&(ch as u8)),
            "{label}: a raw control byte reached stdout"
        );
    }

    // history list still escapes control characters for humans.
    let list = String::from_utf8_lossy(&f.run(&["history", "list"]).stdout).into_owned();
    assert!(
        list.contains("\\u{1b}"),
        "display must keep escaping: {list}"
    );
    assert!(
        list.contains("\\u{9}"),
        "display must keep escaping: {list}"
    );
    assert!(
        !list.contains('\u{1b}'),
        "no raw escape byte in display output"
    );
}

/// AC-010: explain parses terms exactly as query, and truncated reports a real
/// candidate-cap cut rather than an unrelated inventory/shortlist difference.
#[test]
fn explain_agrees_with_query_and_truncation_is_meaningful() {
    let f = Fixture::new();
    // A child-only query resolves against the process cwd, so the match lives
    // under it (the process cwd is the fixture neutral directory).
    let api = f.cwd.join("api");
    fs::create_dir_all(&api).unwrap();
    f.seed(&[(api.clone(), 5)]);

    let q = f.run(&["query", "api", "/"]);
    assert!(
        q.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&q.stderr)
    );
    assert_eq!(
        String::from_utf8(q.stdout).unwrap(),
        format!("{}\n", api.display())
    );

    let e = parse(&f.run(&["explain", "--json", "api", "/"]));
    assert_eq!(
        e["lexical_matches"], 1,
        "explain must strip the child-only `/`"
    );
    assert_eq!(e["semantic_candidates"], 1);
    assert_eq!(e["truncated"], false);

    // A query that matches only the current directory still succeeds; explain
    // must report the same candidate set.
    let cwd = f.cwd.clone();
    f.seed(&[(cwd.clone(), 4)]);
    let q = f.run(&["query", "neutral"]);
    assert!(
        q.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&q.stderr)
    );
    assert_eq!(
        String::from_utf8(q.stdout).unwrap(),
        format!("{}\n", cwd.display())
    );
    let e = parse(&f.run(&["explain", "--json", "neutral"]));
    assert_eq!(
        e["lexical_matches"], 1,
        "a current-directory match must be counted like query"
    );

    // Twelve matching candidates: the cap only cuts at candidate_limit 8.
    let records: Vec<_> = (0..12)
        .map(|i| (f.dir(&format!("m/match-{i:02}")), i as u32 + 1))
        .collect();
    f.seed(&records);

    for (limit, candidates, truncated) in [(8, 8, true), (32, 12, false), (64, 12, false)] {
        assert!(
            f.run(&["config", "set", "candidate_limit", &limit.to_string()])
                .status
                .success()
        );
        let e = parse(&f.run(&["explain", "--json", "match"]));
        assert_eq!(e["inventory"], 12);
        assert_eq!(e["lexical_matches"], 12);
        assert_eq!(e["semantic_candidates"], candidates, "limit {limit}");
        assert_eq!(e["truncated"], truncated, "limit {limit}");
    }
}

/// AC-010 intent (round 2): `explain` must not count destinations `query` would
/// refuse, such as an unenterable directory or a name with a control character.
#[test]
fn explain_counts_only_destinations_query_accepts() {
    let f = Fixture::new();
    let denied = f.dir("zzz-denied");
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o000)).unwrap();
    let _guard = RestoreMode(denied.clone());
    let control = f.dir(&format!("zzz-ctl-{}\u{1b}-end", "x"));
    let usable = f.dir("zzz-ok");
    f.seed(&[
        (denied.clone(), 50),
        (control.clone(), 5),
        (usable.clone(), 1),
    ]);

    // The one enterable, safe destination wins, and explain agrees.
    let q = f.run(&["query", "zzz"]);
    assert!(
        q.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&q.stderr)
    );
    assert_eq!(
        String::from_utf8(q.stdout).unwrap(),
        format!("{}\n", usable.display())
    );
    let e = parse(&f.run(&["explain", "--json", "zzz"]));
    assert_eq!(e["inventory"], 3);
    assert_eq!(e["lexical_matches"], 1);
    assert_eq!(e["semantic_candidates"], 1);

    // With only refused destinations, query fails and explain reports none.
    f.seed(&[(denied.clone(), 50), (control.clone(), 5)]);
    let q = f.run(&["query", "zzz"]);
    assert!(
        !q.status.success(),
        "query must refuse an unusable-only match"
    );
    assert!(
        q.stdout.is_empty(),
        "stdout must stay empty: {:?}",
        q.stdout
    );
    let e = parse(&f.run(&["explain", "--json", "zzz"]));
    assert_eq!(e["inventory"], 2);
    assert_eq!(
        e["lexical_matches"], 0,
        "query returns nothing, so explain must report no match"
    );
    assert_eq!(e["semantic_candidates"], 0);
}

/// AC-011: doctor must not report a store with a valid header but malformed
/// content as healthy while other commands demand repair.
#[test]
fn doctor_detects_a_malformed_store() {
    let f = Fixture::new();
    let dir = f.dir("work/store-target");
    f.seed(&[(dir, 3)]);
    assert!(
        f.run(&["doctor", "--json"]).status.success(),
        "healthy baseline"
    );

    let db = f.state.join("data/visits.db");
    let mut bytes = fs::read(&db).unwrap();
    assert!(bytes.len() > 4096, "fixture store too small to corrupt");
    for byte in bytes.iter_mut().skip(4096) {
        *byte = 0xff;
    }
    fs::write(&db, &bytes).unwrap();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o600)).unwrap();

    let doctor = f.run(&["doctor", "--json"]);
    assert!(
        !doctor.status.success(),
        "doctor must fail on a malformed store"
    );
    let text = String::from_utf8_lossy(&doctor.stdout).into_owned();
    assert!(
        !text.contains("\"store\":\"ok\""),
        "doctor reported a healthy store: {text}"
    );
    assert!(text.contains("\"store\":\"unavailable\""), "{text}");
    // A normal command agrees that the store needs repair.
    assert!(!f.run(&["history", "list"]).status.success());
}

/// AC-007: a genuinely unusable state path is reported with its real cause and
/// doctor does not recommend a remedy that fails in the same state.
#[test]
fn doctor_reports_state_path_cause_without_an_inapplicable_remedy() {
    let f = Fixture::new();
    let config_dir = f.state.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let real = f.root.join("real-config.json");
    fs::write(&real, r#"{"schema_version":1}"#).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
    // A linked state *file* is still refused after the hardening is narrowed.
    symlink(&real, config_dir.join("config.json")).unwrap();

    let doctor = f.run(&["doctor", "--json"]);
    assert!(!doctor.status.success());
    let value = parse(&doctor);
    assert_ne!(value["configuration"], "ok");
    let cause = value["cause"].as_str().expect("cause").to_owned();
    assert!(
        cause.contains("configuration file"),
        "the cause must name the rejected file: {cause}"
    );
    let next = value["next"].as_str().expect("next").to_owned();
    assert!(
        !next.contains("run config recover"),
        "must not recommend an inapplicable remedy: {next}"
    );
    assert!(
        next.contains("unlink") || next.contains("mode-0600"),
        "guidance must be actionable: {next}"
    );
    assert!(
        next.contains("cannot"),
        "recover genuinely cannot succeed here, so the advice must say so: {next}"
    );
    // The remedy doctor used to recommend really does fail in this state.
    assert!(!f.run(&["config", "recover", "--apply"]).status.success());
}

/// AC-007 (round 2): the recover advice must match what `config recover --apply`
/// really does. Each case is exercised end to end: run doctor, read the advice,
/// run recover, then run doctor again and compare.
#[test]
fn doctor_recover_advice_matches_observed_behaviour() {
    let f = Fixture::new();
    let check = |state: &Path, label: &str, cause_needle: &str| {
        let doctor = f.run_state(state, &["doctor", "--json"]);
        assert!(
            !doctor.status.success(),
            "{label}: expected an unhealthy doctor"
        );
        let value = parse(&doctor);
        let cause = value["cause"].as_str().expect("cause").to_owned();
        let next = value["next"].as_str().expect("next").to_owned();
        assert!(
            cause.contains(cause_needle),
            "{label}: cause {cause:?} does not mention {cause_needle:?}"
        );
        let recover = f.run_state(state, &["config", "recover", "--apply"]);
        let after = f.run_state(state, &["doctor", "--json"]);
        if next.contains("jjump config recover --apply") {
            assert!(
                recover.status.success(),
                "{label}: advice says to run config recover, but it failed: {next}"
            );
            assert!(
                after.status.success(),
                "{label}: advice says to run config recover, but doctor is still unhealthy after it: {next}"
            );
        } else if next.contains("does not repair") {
            assert!(
                recover.status.success(),
                "{label}: advice says recover does not repair (rather than cannot run), but it failed to run: {next}"
            );
            assert!(
                !after.status.success(),
                "{label}: advice says recover does not repair, but doctor became healthy after it: {next}"
            );
        } else if next.contains("cannot") {
            assert!(
                !recover.status.success(),
                "{label}: advice says config recover cannot succeed, but it exited 0: {next}"
            );
        } else {
            panic!("{label}: advice names no config-recover outcome: {next}");
        }
    };

    // Corrupt configuration content: config recover genuinely repairs it.
    let state = f.state_dir("adv-invalid");
    let cfg_dir = state.join("config");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::set_permissions(&cfg_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let cfg = cfg_dir.join("config.json");
    fs::write(&cfg, b"{not json").unwrap();
    fs::set_permissions(&cfg, fs::Permissions::from_mode(0o600)).unwrap();
    check(&state, "invalid config content", "invalid");

    // Linked configuration file: recover reads the same file, so it cannot run.
    let state = f.state_dir("adv-linked-config");
    let cfg_dir = state.join("config");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::set_permissions(&cfg_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let real = state.join("real-config.json");
    fs::write(&real, r#"{"schema_version":1}"#).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&real, cfg_dir.join("config.json")).unwrap();
    check(&state, "linked config file", "configuration file");

    // Oversized configuration file: recover cannot read it either.
    let state = f.state_dir("adv-oversized-config");
    let cfg_dir = state.join("config");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::set_permissions(&cfg_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let cfg = cfg_dir.join("config.json");
    fs::write(
        &cfg,
        format!(r#"{{"schema_version":1,"pad":"{}"}}"#, "x".repeat(70_000)),
    )
    .unwrap();
    fs::set_permissions(&cfg, fs::Permissions::from_mode(0o600)).unwrap();
    check(&state, "oversized config file", "size limit");

    // Non-private configuration directory: recover saves there, so it cannot run.
    let state = f.state_dir("adv-config-dir");
    let cfg_dir = state.join("config");
    fs::create_dir_all(&cfg_dir).unwrap();
    let cfg = cfg_dir.join("config.json");
    fs::write(&cfg, r#"{"schema_version":1}"#).unwrap();
    fs::set_permissions(&cfg, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&cfg_dir, fs::Permissions::from_mode(0o755)).unwrap();
    check(
        &state,
        "non-private config directory",
        "configuration directory",
    );

    // Linked store file: recover runs but never touches the visit store.
    let state = f.state_dir("adv-linked-store");
    let db = state.join("data/visits.db");
    let real = state.join("real-store.db");
    fs::rename(&db, &real).unwrap();
    symlink(&real, &db).unwrap();
    check(&state, "linked store file", "store file");

    // Non-private data directory: recover runs but does not repair the store.
    let state = f.state_dir("adv-data-dir");
    fs::set_permissions(state.join("data"), fs::Permissions::from_mode(0o755)).unwrap();
    check(&state, "non-private data directory", "data directory");

    // Corrupt store content: recover runs but does not repair the store.
    let state = f.state_dir("adv-corrupt-store");
    let db = state.join("data/visits.db");
    let mut bytes = fs::read(&db).unwrap();
    assert!(bytes.len() > 4096, "fixture store too small to corrupt");
    for byte in bytes.iter_mut().skip(4096) {
        *byte = 0xff;
    }
    fs::write(&db, &bytes).unwrap();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o600)).unwrap();
    check(&state, "corrupt store", "unreadable");
}

/// AC-007: an ordinary symbolic-linked state *ancestor* still operates normally.
#[test]
fn symlinked_state_ancestor_operates_normally() {
    let f = Fixture::new();
    let real = f.root.join("real-state");
    fs::create_dir_all(&real).unwrap();
    let link = f.root.join("linked-state");
    symlink(&real, &link).unwrap();

    let out = f
        .command(&f.cwd)
        .env("J_JUMP_HOME", link.join("store"))
        .arg("doctor")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// AC-002: a query that matches nothing must not resolve (or even order) the
/// whole inventory, so its cost stays close to an explicit-path query as the
/// inventory grows.
#[test]
fn no_match_query_cost_does_not_scale_with_inventory() {
    let f = Fixture::new();
    let work = f.home.join("work");
    fs::create_dir_all(&work).unwrap();
    let best = |args: &[&str]| {
        (0..5)
            .map(|_| {
                let started = Instant::now();
                let _ = f.run(args);
                started.elapsed().as_secs_f64()
            })
            .fold(f64::MAX, f64::min)
    };
    let measured = |count: usize| {
        let records: Vec<_> = (0..count)
            .map(|i| {
                let dir = work.join(format!("d{i:06}"));
                fs::create_dir_all(&dir).unwrap();
                (dir, (i % 9) as u32 + 1)
            })
            .collect();
        f.seed(&records);
        // `doctor` reads the whole store without matching anything, so it is a
        // load-robust baseline for the unavoidable store read. The difference
        // is what discovery adds on top of it.
        (
            best(&["query", "zzz-no-such-directory"]),
            best(&["doctor", "--json"]),
            best(&["query", "--", work.to_str().unwrap()]),
        )
    };
    let (small, small_doctor, _small_explicit) = measured(1000);
    let (large, large_doctor, large_explicit) = measured(10_000);

    // Measured on a debug build: discovery costs ~0.5 ms over `doctor` at 1000
    // and ~4 ms at 10000. Re-instating an inventory-wide canonicalise (the
    // 0.0.14 defect) adds ~150 us/row here, i.e. ~50 ms at 10000, which these
    // bounds must reject.
    assert!(
        small < small_doctor + 0.015,
        "no-match {small:.3}s vs store-read {small_doctor:.3}s at 1000"
    );
    assert!(
        large < large_doctor + 0.030,
        "discovery must not resolve the inventory: no-match {large:.3}s vs store-read {large_doctor:.3}s at 10000"
    );
    assert!(
        large < large_explicit + 0.150,
        "no-match {large:.3}s vs explicit {large_explicit:.3}s at 10000"
    );
}
