use crate::{
    Error, Result,
    config::{Config, Paths, private_dir, private_file},
    engine::{Candidate, decayed_weight, effective_score, key},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Duration,
};
pub struct Store {
    pub db: Connection,
    budget: Duration,
}
fn db_err(e: rusqlite::Error) -> Error {
    if is_busy(&e) {
        return Error(7, "visit store busy; retry shortly".into());
    }
    Error(
        7,
        "visit store unavailable or corrupt; run doctor; exact paths still work".into(),
    )
}
const WEIGHT_CAP: f64 = 2_147_483_647.0;

fn read_weight(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<f64> {
    let weight: f64 = row.get(index)?;
    if !weight.is_finite() || !(0.0..=WEIGHT_CAP).contains(&weight) {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Real,
            Box::new(Error(7, "invalid visit weight".into())),
        ));
    }
    Ok(weight)
}

fn read_candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    Ok(Candidate {
        id: format!("d{}", row.get::<_, i64>(0)?),
        path: row.get::<_, String>(1)?.into(),
        count: row.get(2)?,
        last_seen: row.get(3)?,
        weight: read_weight(row, 4)?,
    })
}
/// Inspect a SQLite sidecar without creating it.
///
/// A concurrent writer removes its rollback journal the moment it commits, so
/// the inode can be unlinked between our probe and the check. An unlinked file
/// (`nlink == 0`) is not a hard link and is not a privacy problem; only a
/// surviving link count above one is refused.
fn private_sidecar(path: &Path) -> Result<()> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Error(7, "cannot open private state file".into())),
    };
    let m = file
        .metadata()
        .map_err(|_| Error(7, "cannot inspect state file".into()))?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() > 1
    {
        return Err(Error(
            7,
            "state file must be private, owned and not linked".into(),
        ));
    }
    Ok(())
}
/// Total time a write may spend waiting for another writer before it gives up.
///
/// The prompt-hook worker is SIGKILLed by its supervisor at 250 ms, so its wait
/// must stay a small fraction of that: a worker killed mid-commit is a silent
/// loss. A direct command is not time-boxed and may wait longer.
fn write_budget(hook: bool) -> Duration {
    if hook {
        Duration::from_millis(30)
    } else {
        Duration::from_millis(600)
    }
}
/// Longest single wait inside the retry loop; the remaining budget caps it.
const ATTEMPT_CAP: Duration = Duration::from_millis(25);
/// Run `op` inside an immediate write transaction, retrying while `budget`
/// remains. Each attempt's `busy_timeout` is the remaining budget, so the total
/// wait is bounded by `budget` and no attempt is started that could outlive it.
fn with_immediate_tx<T>(
    db: &mut Connection,
    budget: Duration,
    op: impl Fn(&rusqlite::Transaction<'_>) -> Result<T>,
) -> Result<T> {
    let deadline = std::time::Instant::now() + budget;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        db.busy_timeout(remaining.min(ATTEMPT_CAP).max(Duration::from_millis(1)))
            .map_err(db_err)?;
        match db.transaction_with_behavior(TransactionBehavior::Immediate) {
            Ok(tx) => {
                let out = op(&tx)?;
                tx.commit().map_err(db_err)?;
                return Ok(out);
            }
            Err(e) if is_busy(&e) => {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left < Duration::from_millis(1) {
                    return Err(db_err(e));
                }
                let jitter = (std::process::id() as u64 * 31 + left.as_millis() as u64) % 3;
                std::thread::sleep(Duration::from_millis(1 + jitter).min(left));
            }
            Err(e) => return Err(db_err(e)),
        }
    }
}
fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked,
                ..
            },
            _
        )
    )
}
/// True when `path` exists as a regular file with no content. A symbolic link is
/// never a regular file here, so a linked store is left to `private_file`'s
/// `O_NOFOLLOW` open to refuse.
fn is_empty_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == 0)
}
impl Store {
    pub fn open(paths: &Paths, hook: bool) -> Result<Self> {
        private_dir(&paths.data)?;
        let path = paths.data.join("visits.db");
        // SQLite never leaves a zero-length database behind, so bytes that are
        // present but empty are unreadable content, not a new store. Wait
        // briefly first in case a concurrent first-use is still writing the
        // header, then fail closed without replacing the file.
        if is_empty_file(&path) {
            for _ in 0..5 {
                std::thread::sleep(Duration::from_millis(10));
                if !is_empty_file(&path) {
                    break;
                }
            }
            if is_empty_file(&path) {
                return Err(Error(
                    7,
                    "visit store is empty or corrupt; run doctor; exact paths still work".into(),
                ));
            }
        }
        let _f = private_file(&path)?;
        for suffix in ["-wal", "-shm", "-journal"] {
            private_sidecar(&paths.data.join(format!("visits.db{suffix}")))?;
        }
        let mut db = Connection::open(path).map_err(db_err)?;
        let budget = write_budget(hook);
        db.busy_timeout(budget).map_err(db_err)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db_err)?;
        if version == 0 {
            with_immediate_tx(&mut db, budget, |tx| {
                let current: i64 = tx
                    .pragma_query_value(None, "user_version", |r| r.get(0))
                    .map_err(db_err)?;
                if current == 0 {
                    let objects: i64 = tx
                        .query_row(
                            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                            [],
                            |r| r.get(0),
                        )
                        .map_err(db_err)?;
                    if objects != 0 {
                        return Err(Error(
                            7,
                            "unversioned visit store is unsupported; use a fresh J_JUMP_HOME; existing data unchanged".into(),
                        ));
                    }
                    tx.execute_batch("CREATE TABLE visits(id INTEGER PRIMARY KEY AUTOINCREMENT,path TEXT NOT NULL UNIQUE,key TEXT NOT NULL,count INTEGER NOT NULL,last_seen INTEGER NOT NULL,weight REAL NOT NULL CHECK(typeof(weight) IN ('real','integer') AND weight >= 0 AND weight <= 2147483647)); CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),epoch TEXT NOT NULL,generation INTEGER NOT NULL,revision INTEGER NOT NULL); INSERT INTO meta VALUES(1,lower(hex(randomblob(16))),0,0); PRAGMA user_version=4;").map_err(db_err)?;
                } else if current != 4 {
                    return Err(Error(
                        7,
                        "unsupported visit store schema; use a fresh J_JUMP_HOME; existing data unchanged".into(),
                    ));
                }
                Ok(())
            })?;
        } else if version != 4 {
            return Err(Error(
                7,
                "unsupported visit store schema; use a fresh J_JUMP_HOME; existing data unchanged"
                    .into(),
            ));
        }
        Ok(Self { db, budget })
    }
    pub fn generation(&self) -> Result<String> {
        self.db
            .query_row(
                "SELECT epoch || ':' || generation || ':' || revision FROM meta WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(db_err)
    }
    pub fn snapshot(&self) -> Result<(String, Vec<Candidate>)> {
        let tx = self.db.unchecked_transaction().map_err(db_err)?;
        let generation = self.generation()?;
        let rows = self.list()?;
        tx.commit().map_err(db_err)?;
        Ok((generation, rows))
    }
    pub fn event_generation(&self) -> Result<String> {
        self.db
            .query_row(
                "SELECT epoch || ':' || generation FROM meta WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(db_err)
    }
    pub fn list(&self) -> Result<Vec<Candidate>> {
        let mut s = self
            .db
            .prepare("SELECT id,path,count,last_seen,weight FROM visits")
            .map_err(db_err)?;
        let rows = s.query_map([], read_candidate).map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
    pub fn record(
        &mut self,
        paths: &Paths,
        config: &Config,
        path: &Path,
        expected: Option<&str>,
    ) -> Result<()> {
        if !config.tracking {
            return Ok(());
        }
        let p = crate::engine::valid_path(path)?;
        let logical = std::fs::metadata(&p)
            .map_err(|_| Error(6, "record directory no longer exists".into()))?;
        let actual =
            std::fs::metadata(".").map_err(|_| Error(6, "current directory unavailable".into()))?;
        if logical.dev() != actual.dev() || logical.ino() != actual.ino() {
            return Err(Error(
                6,
                "logical PWD does not identify current directory".into(),
            ));
        }
        if !paths.allowed(config, &p, false) {
            return Ok(());
        }
        let s = p
            .to_str()
            .ok_or(Error(6, "non UTF-8 path rejected".into()))?;
        let recorded = with_immediate_tx(&mut self.db, self.budget, |tx| {
            let generation: String = tx
                .query_row(
                    "SELECT epoch || ':' || generation FROM meta WHERE id=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if expected.is_some_and(|g| g != generation) {
                return Ok(false);
            }
            let now = crate::now();
            let previous = tx
                .query_row(
                    "SELECT count,last_seen,weight FROM visits WHERE path=?1",
                    [s],
                    |row| {
                        Ok((
                            row.get::<_, u32>(0)?,
                            row.get::<_, i64>(1)?,
                            read_weight(row, 2)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_err)?;
            let (count, last_seen, weight) = match previous {
                Some((count, last_seen, weight)) => (
                    count.saturating_add(1).min(2_147_483_647),
                    last_seen.max(now),
                    (decayed_weight(weight, last_seen, now) + 1.0).min(WEIGHT_CAP),
                ),
                None => (1, now, 1.0),
            };
            tx.execute("INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(path) DO UPDATE SET count=excluded.count,last_seen=excluded.last_seen,weight=excluded.weight",params![s,key(s),count,last_seen,weight]).map_err(db_err)?;
            tx.execute("UPDATE meta SET revision=revision+1 WHERE id=1", [])
                .map_err(db_err)?;
            Ok(true)
        })?;
        // Pruning cannot roll back the just committed visit.
        if !recorded
            || self
                .db
                .query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap_or(0)
                <= 10000
        {
            return Ok(());
        }
        let now = crate::now();
        let _ = with_immediate_tx(&mut self.db, self.budget, |tx| {
            let size: i64 = tx
                .query_row("SELECT count(*) FROM visits", [], |r| r.get(0))
                .map_err(db_err)?;
            if size > 10000 {
                // Admit this visit even when every incumbent has a higher score.
                // A later distinct visit may evict it, but it can now accumulate
                // frequency while retained. One clock keeps all comparisons stable.
                let mut rows = {
                    let mut stmt = tx
                        .prepare(
                            "SELECT id,path,count,last_seen,weight FROM visits WHERE path != ?1",
                        )
                        .map_err(db_err)?;
                    stmt.query_map([s], |r| Ok((r.get::<_, i64>(0)?, read_candidate(r)?)))
                        .map_err(db_err)?
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(db_err)?
                };
                rows.sort_by(|a, b| {
                    effective_score(&a.1, now)
                        .total_cmp(&effective_score(&b.1, now))
                        .then(a.1.last_seen.cmp(&b.1.last_seen))
                        .then(a.1.path.cmp(&b.1.path))
                });
                for row in rows.into_iter().take((size - 9000) as usize) {
                    tx.execute("DELETE FROM visits WHERE id=?1", [row.0])
                        .map_err(db_err)?;
                }
            }
            tx.execute("UPDATE meta SET revision=revision+1 WHERE id=1", [])
                .map_err(db_err)?;
            Ok(())
        });
        Ok(())
    }
    /// Only ENOENT is missing; inaccessible volumes and permission errors stay.
    pub fn prune_missing(&mut self, apply: bool, stale_only: bool) -> Result<usize> {
        let cutoff = crate::now() - 90 * 86400;
        let mut statement = self
            .db
            .prepare(
                "SELECT id,path,count,last_seen,weight FROM visits WHERE NOT ?1 OR last_seen<?2",
            )
            .map_err(db_err)?;
        let rows = statement
            .query_map(params![stale_only, cutoff], read_candidate)
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        drop(statement);
        let ids: Vec<_> = rows
            .into_iter()
            .filter(|c| {
                std::fs::metadata(&c.path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                    && c.path
                        .parent()
                        .is_some_and(|p| crate::engine::valid_path(p).is_ok())
            })
            .map(|c| (c.id[1..].parse::<i64>().unwrap(), c.last_seen))
            .collect();
        if apply && !ids.is_empty() {
            with_immediate_tx(&mut self.db, self.budget, |tx| {
                for (id, last) in &ids {
                    tx.execute(
                        "DELETE FROM visits WHERE id=?1 AND last_seen=?2",
                        params![id, last],
                    )
                    .map_err(db_err)?;
                }
                tx.execute(
                    "UPDATE meta SET generation=generation+1,revision=revision+1 WHERE id=1",
                    [],
                )
                .map_err(db_err)?;
                Ok(())
            })?;
        }
        Ok(ids.len())
    }
    pub fn clear(&mut self, path: Option<&str>, apply: bool) -> Result<i64> {
        with_immediate_tx(&mut self.db, self.budget, |tx| {
            let count = tx
                .query_row(
                    "SELECT count(*) FROM visits WHERE ?1 IS NULL OR path=?1",
                    [path],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if apply {
                tx.execute("DELETE FROM visits WHERE ?1 IS NULL OR path=?1", [path])
                    .map_err(db_err)?;
                tx.execute(
                    "UPDATE meta SET generation=generation+1,revision=revision+1 WHERE id=1",
                    [],
                )
                .map_err(db_err)?;
            }
            Ok(count)
        })
    }
}
