//! Iteration 0029: successful visits remain learnable at the 10000-row cap.
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn bin() -> String {
    std::env::var("JJ_TEST_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_jjump").to_string())
}

struct Fixture {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    neutral: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let neutral = home.join("neutral");
        fs::create_dir_all(&neutral).unwrap();
        let fixture = Self {
            _tmp: tmp,
            home,
            state: root.join("state"),
            neutral,
        };
        fixture.success(&fixture.neutral, &["doctor"]);
        fixture
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("HOME", &self.home)
            .env("PWD", cwd)
            .env("J_JUMP_HOME", &self.state)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .output()
            .unwrap()
    }

    fn success(&self, cwd: &Path, args: &[&str]) -> Output {
        let output = self.run(cwd, args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        output
    }

    fn db(&self) -> Connection {
        Connection::open(self.state.join("data/visits.db")).unwrap()
    }

    fn seed_hot_inventory(&self) {
        let mut db = self.db();
        let tx = db.transaction().unwrap();
        let now = j_jump::now();
        for i in 0..10_000 {
            // Synthetic retained history: eviction must use its stored score,
            // without probing or deleting directories during prompt recording.
            let path = self.hot_path(i).to_str().unwrap().to_owned();
            tx.execute(
                "INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?1,?2,100,?3,100)",
                params![path, j_jump::engine::key(&path), now],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    fn hot_path(&self, index: usize) -> PathBuf {
        self.home.join(format!("hot-{index:05}"))
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.home.join(name);
        fs::create_dir(&path).unwrap();
        path
    }

    fn record(&self, path: &Path) {
        self.success(path, &["record", "--", path.to_str().unwrap()]);
    }

    fn count(&self, path: &Path) -> Option<u32> {
        self.db()
            .query_row(
                "SELECT count FROM visits WHERE path=?1",
                [path.to_str().unwrap()],
                |row| row.get(0),
            )
            .optional()
            .unwrap()
    }

    fn size(&self) -> i64 {
        self.db()
            .query_row("SELECT count(*) FROM visits", [], |row| row.get(0))
            .unwrap()
    }

    fn version(&self) -> (String, i64, i64) {
        self.db()
            .query_row(
                "SELECT epoch,generation,revision FROM meta WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn snapshot(&self) -> (String, Vec<(i64, String, u32, i64)>) {
        let db = self.db();
        let generation = db
            .query_row(
                "SELECT epoch || ':' || generation || ':' || revision FROM meta WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let rows = db
            .prepare("SELECT id,path,count,last_seen FROM visits ORDER BY path")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        (generation, rows)
    }
}

#[test]
fn full_hot_store_admits_new_path_and_accumulates_repeated_visits() {
    let fixture = Fixture::new();
    fixture.seed_hot_inventory();
    let arrived = fixture.directory("new-project");
    let before = fixture.version();
    for count in 1..=5 {
        fixture.record(&arrived);
        assert_eq!(fixture.count(&arrived), Some(count));
        assert_eq!(fixture.size(), 9_000);
        assert_eq!(
            fixture.version(),
            (before.0.clone(), before.1, before.2 + i64::from(count) + 1)
        );
    }
    // Equal-score incumbents keep the existing ascending path eviction rule.
    assert_eq!(fixture.count(&fixture.hot_path(0)), None);
    assert_eq!(fixture.count(&fixture.hot_path(1001)), Some(100));
    let output = fixture.success(&fixture.neutral, &["--offline", "query", "new-pro"]);
    assert_eq!(output.stdout, format!("{}\n", arrived.display()).as_bytes());
}

#[test]
fn later_distinct_arrival_evicts_the_weakest_other_path() {
    let fixture = Fixture::new();
    fixture.seed_hot_inventory();
    let first = fixture.directory("first-arrival");
    let second = fixture.directory("second-arrival");
    fixture.record(&first);
    assert_eq!(fixture.count(&first), Some(1));
    fixture.record(&second);
    assert_eq!(fixture.count(&second), Some(1));
    assert_eq!(fixture.count(&first), Some(1));
    assert_eq!(fixture.size(), 9_001);
    assert_eq!(fixture.count(&fixture.hot_path(0)), None);
    assert_eq!(fixture.count(&fixture.hot_path(1001)), Some(100));
}

#[test]
fn refused_observations_do_not_admit_or_evict_at_capacity() {
    let fixture = Fixture::new();
    fixture.seed_hot_inventory();
    let refused = fixture.directory("refused-arrival");
    let before = fixture.snapshot();
    fixture.success(
        &refused,
        &[
            "record",
            "--generation",
            "stale-generation",
            "--",
            refused.to_str().unwrap(),
        ],
    );
    assert_eq!(fixture.snapshot(), before);
    fixture.success(&fixture.neutral, &["config", "set", "tracking", "off"]);
    fixture.record(&refused);
    assert_eq!(fixture.snapshot(), before);
    fixture.success(&fixture.neutral, &["config", "set", "tracking", "on"]);
    let exclusion = serde_json::to_string(&[&refused]).unwrap();
    fixture.success(&fixture.neutral, &["config", "set", "exclude", &exclusion]);
    fixture.record(&refused);
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn saturated_existing_visit_preserves_capacity_and_counter() {
    let fixture = Fixture::new();
    fixture.seed_hot_inventory();
    let existing = fixture.hot_path(0);
    fs::create_dir(&existing).unwrap();
    fixture
        .db()
        .execute(
            "UPDATE visits SET count=2147483647 WHERE path=?1",
            [existing.to_str().unwrap()],
        )
        .unwrap();
    fixture.record(&existing);
    assert_eq!(fixture.count(&existing), Some(2_147_483_647));
    assert_eq!(fixture.size(), 10_000);
    assert_eq!(fixture.count(&fixture.hot_path(1)), Some(100));
}
