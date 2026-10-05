//! Iteration 0030: persisted decay, current schema and weighted retention.
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const HALF_LIFE: f64 = 604_800.0;
type Visit = (u32, i64, f64);
type Version = (String, i64, i64);

fn bin() -> String {
    std::env::var("JJ_TEST_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_jjump").to_string())
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn current(count: u32, last_seen: i64) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        let target = home.join("decay-target");
        let state = root.join("state");
        fs::create_dir_all(&target).unwrap();
        j_jump::config::private_dir(&state.join("data")).unwrap();
        let fixture = Self {
            _temp: temp,
            home,
            state,
            target,
        };
        let db = fixture.db();
        fs::set_permissions(fixture.db_path(), fs::Permissions::from_mode(0o600)).unwrap();
        db.execute_batch(
            "CREATE TABLE visits(id INTEGER PRIMARY KEY AUTOINCREMENT,path TEXT NOT NULL UNIQUE,key TEXT NOT NULL,count INTEGER NOT NULL,last_seen INTEGER NOT NULL,weight REAL NOT NULL CHECK(weight>=0 AND weight<=2147483647));
             CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),epoch TEXT NOT NULL,generation INTEGER NOT NULL,revision INTEGER NOT NULL DEFAULT 0);
             INSERT INTO meta VALUES(1,'fixture-epoch',7,11);
             PRAGMA user_version=4;",
        )
        .unwrap();
        db.execute(
            "INSERT INTO visits(id,path,key,count,last_seen,weight) VALUES(77,?1,?2,?3,?4,?3)",
            params![
                fixture.target.to_str().unwrap(),
                j_jump::engine::key(fixture.target.to_str().unwrap()),
                count,
                last_seen
            ],
        )
        .unwrap();
        fixture
    }

    fn db_path(&self) -> PathBuf {
        self.state.join("data/visits.db")
    }

    fn db(&self) -> Connection {
        Connection::open(self.db_path()).unwrap()
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::new(bin());
        command
            .current_dir(cwd)
            .env_clear()
            .env("HOME", &self.home)
            .env("PWD", cwd)
            .env("J_JUMP_HOME", &self.state)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(&self.target).args(args).output().unwrap()
    }

    fn success(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        output
    }

    fn check(&self) {
        self.success(&["doctor"]);
    }

    fn record(&self) {
        self.success(&["record", "--", self.target.to_str().unwrap()]);
    }

    fn visit(&self) -> Visit {
        self.db()
            .query_row(
                "SELECT count,last_seen,weight FROM visits WHERE path=?1",
                [self.target.to_str().unwrap()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn version(&self) -> Version {
        self.db()
            .query_row(
                "SELECT epoch,generation,revision FROM meta WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn schema(&self) -> i64 {
        self.db()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    fn set_visit(&self, count: u32, last_seen: i64, weight: f64) {
        self.db()
            .execute(
                "UPDATE visits SET count=?1,last_seen=?2,weight=?3 WHERE path=?4",
                params![count, last_seen, weight, self.target.to_str().unwrap()],
            )
            .unwrap();
    }
}

#[test]
fn old_and_unknown_store_versions_are_rejected_unchanged() {
    for version in [0, 1, 2, 3, 99] {
        let fixture = Fixture::current(22, 33);
        fixture
            .db()
            .pragma_update(None, "user_version", version)
            .unwrap();
        let before = fs::read(fixture.db_path()).unwrap();
        let out = fixture.run(&["doctor"]);
        assert_eq!(out.status.code(), Some(7), "version {version}: {out:?}");
        assert_eq!(fs::read(fixture.db_path()).unwrap(), before);
        assert_eq!(fixture.schema(), version);
    }
}

#[test]
fn concurrent_fresh_openers_initialize_one_complete_store() {
    let fixture = Fixture::current(1, 0);
    fs::remove_file(fixture.db_path()).unwrap();
    let children: Vec<_> = (0..8)
        .map(|_| {
            fixture
                .command(&fixture.target)
                .arg("doctor")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{out:?}");
    }
    assert_eq!(fixture.schema(), 4);
    let version = fixture.version();
    assert_eq!((version.1, version.2), (0, 0));
    fixture.check();
    assert_eq!(fixture.version(), version);
    fixture.record();
    assert_eq!(fixture.visit().0, 1);
}

#[test]
fn concurrent_successful_records_increment_count_weight_and_revision_together() {
    let future = j_jump::now() + 7 * 86400;
    let fixture = Fixture::current(100, future);
    fixture.check();
    fixture.set_visit(100, future, 10.0);
    let before = fixture.version();
    let children: Vec<_> = (0..12)
        .map(|_| {
            fixture
                .command(&fixture.target)
                .args(["record", "--", fixture.target.to_str().unwrap()])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut succeeded = 0_u32;
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.stdout.is_empty());
        match output.status.code() {
            Some(0) => succeeded += 1,
            Some(7) => assert!(
                String::from_utf8_lossy(&output.stderr).contains("visit store busy; retry"),
                "{output:?}"
            ),
            _ => panic!("{output:?}"),
        }
    }
    assert!(succeeded > 0, "at least one writer must acquire the store");
    assert_eq!(
        fixture.visit(),
        (100 + succeeded, future, 10.0 + f64::from(succeeded))
    );
    assert_eq!(
        fixture.version(),
        (before.0, before.1, before.2 + i64::from(succeeded))
    );
}

#[test]
fn expired_visit_weight_does_not_revive_lifetime_count() {
    let old_last = j_jump::now() - 180 * 86400;
    let fixture = Fixture::current(1000, old_last);
    fixture.record();
    let first = fixture.visit();
    let expected = 1000.0 * (-((first.1 - old_last) as f64) / HALF_LIFE).exp2() + 1.0;
    assert_eq!(first.0, 1001);
    assert!((first.2 - expected).abs() < 1e-12);
    assert!(first.2 < 1.001);
    fixture.record();
    let second = fixture.visit();
    let expected = first.2 * (-((second.1 - first.1) as f64) / HALF_LIFE).exp2() + 1.0;
    assert_eq!(second.0, 1002);
    assert!((second.2 - expected).abs() < 1e-12);
    assert!(second.2 < 2.001);
}

#[test]
fn rollback_keeps_weight_anchor_monotonic_and_saturates_independently() {
    let future = j_jump::now() + 7 * 86400;
    let fixture = Fixture::current(100, future);
    fixture.check();
    fixture.set_visit(100, future, 16.0);
    fixture.record();
    assert_eq!(fixture.visit(), (101, future, 17.0));
    fixture.set_visit(100, future, 0.0);
    fixture.record();
    assert_eq!(fixture.visit(), (101, future, 1.0));
    fixture.set_visit(2_147_483_647, future, 2_147_483_647.0);
    fixture.record();
    assert_eq!(fixture.visit(), (2_147_483_647, future, 2_147_483_647.0));
}

#[test]
fn refused_events_do_not_decay_or_increment_weight() {
    let fixture = Fixture::current(100, 100);
    fixture.check();
    fixture.set_visit(100, 100, 7.0);
    let before = (fixture.visit(), fixture.version());
    fixture.success(&[
        "record",
        "--generation",
        "stale",
        "--",
        fixture.target.to_str().unwrap(),
    ]);
    assert_eq!((fixture.visit(), fixture.version()), before);
    fixture.success(&["config", "set", "tracking", "off"]);
    fixture.record();
    assert_eq!((fixture.visit(), fixture.version()), before);
    fixture.success(&["config", "set", "tracking", "on"]);
    let exclusion = serde_json::to_string(&[&fixture.target]).unwrap();
    fixture.success(&["config", "set", "exclude", &exclusion]);
    fixture.record();
    assert_eq!((fixture.visit(), fixture.version()), before);
}

#[test]
fn corrupt_present_weights_fail_closed_without_falling_back_to_count() {
    for weight in [-1.0, f64::INFINITY, 2_147_483_648.0] {
        let fixture = Fixture::current(100, 123);
        fixture.check();
        let db = fixture.db();
        db.execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        db.execute("UPDATE visits SET weight=?1", [weight]).unwrap();
        let before = (fixture.visit(), fixture.version());
        let query = fixture
            .command(&fixture.home)
            .args(["--offline", "query", "decay-targ"])
            .output()
            .unwrap();
        assert_eq!(query.status.code(), Some(7), "{query:?}");
        assert!(query.stdout.is_empty());
        assert_eq!(
            fixture
                .run(&["record", "--", fixture.target.to_str().unwrap()])
                .status
                .code(),
            Some(7)
        );
        assert_eq!((fixture.visit(), fixture.version()), before);
    }
}

#[test]
fn eviction_uses_stored_weight_and_preserves_ancient_score_order() {
    let fixture = Fixture::current(1, 0);
    fixture.check();
    let mut db = fixture.db();
    let tx = db.transaction().unwrap();
    tx.execute("DELETE FROM visits", []).unwrap();
    let weak = fixture.home.join("zz-high-lifetime-low-weight");
    for i in 0..10_000 {
        let (path, count, weight) = if i == 0 {
            (weak.clone(), 1_000_000, 0.25)
        } else {
            (fixture.home.join(format!("history-{i:05}")), 8, 8.0)
        };
        tx.execute(
            "INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?1,?2,?3,0,?4)",
            params![
                path.to_str().unwrap(),
                j_jump::engine::key(path.to_str().unwrap()),
                count,
                weight
            ],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    fixture.record();
    assert_eq!(fixture.visit().0, 1);
    assert_eq!(fixture.visit().2, 1.0);
    let missing: Option<i64> = db
        .query_row(
            "SELECT id FROM visits WHERE path=?1",
            [weak.to_str().unwrap()],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(missing, None);
    let size: i64 = db
        .query_row("SELECT count(*) FROM visits", [], |row| row.get(0))
        .unwrap();
    assert_eq!(size, 9_000);
}
