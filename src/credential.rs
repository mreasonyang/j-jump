//! Secrets use only the environment or a named OS credential entry; no file fallback.
use crate::{
    Error, Result,
    config::{Config, Paths, Provider},
};
use zeroize::Zeroizing;
pub fn entry_identity(provider: Provider) -> (&'static str, &'static str) {
    provider
        .driver()
        .descriptor()
        .credential
        .as_ref()
        .map(|c| (c.service, c.account))
        .unwrap_or(("", ""))
}
fn entry(provider: Provider) -> Result<keyring::Entry> {
    if !provider.needs_credentials() {
        return Err(Error(
            2,
            "selected provider does not use credentials".into(),
        ));
    }
    let (service, account) = entry_identity(provider);
    keyring::Entry::new(service, account).map_err(|_| {
        Error(
            5,
            "system credential store unavailable; use environment or semantic off".into(),
        )
    })
}
pub fn system() -> Result<Option<Zeroizing<String>>> {
    system_for(Provider::Jev)
}
pub fn system_for(provider: Provider) -> Result<Option<Zeroizing<String>>> {
    match entry(provider)?.get_password() {
        Ok(s) => Ok(Some(Zeroizing::new(s))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(Error(
            5,
            "system credential store locked or unavailable; no plaintext fallback".into(),
        )),
    }
}
pub fn get(source: &str) -> Result<Zeroizing<String>> {
    get_for(Provider::Jev, source)
}
pub fn environment_name(provider: Provider) -> Option<&'static str> {
    provider
        .driver()
        .descriptor()
        .credential
        .as_ref()?
        .env
        .iter()
        .copied()
        .find(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}
pub fn get_for(provider: Provider, source: &str) -> Result<Zeroizing<String>> {
    if !provider.needs_credentials() {
        return Ok(Zeroizing::new(String::new()));
    }
    if let Some(name) = environment_name(provider) {
        let s = Zeroizing::new(
            std::env::var(name)
                .map_err(|_| Error(5, "credential environment value is not valid UTF-8".into()))?,
        );
        validate(&s)?;
        return Ok(s);
    }
    if source == "system" {
        let secret = system_for(provider)?.ok_or(Error(
            5,
            format!(
                "stored {} key missing; use credential set or semantic off",
                provider.label()
            )
            .into(),
        ))?;
        validate(&secret)?;
        return Ok(secret);
    }
    Err(Error(
        5,
        format!(
            "{} key absent; set {} or run credential set",
            provider.label(),
            provider.env_key()
        )
        .into(),
    ))
}
pub fn validate(secret: &str) -> Result<()> {
    if secret.is_empty() || secret.len() > 4096 || secret.chars().any(char::is_control) {
        return Err(Error(
            2,
            "credential must be nonempty, at most 4096 bytes and contain no control characters"
                .into(),
        ));
    }
    Ok(())
}
pub fn set(secret: &str) -> Result<()> {
    set_for(Provider::Jev, secret)
}
pub fn set_for(provider: Provider, secret: &str) -> Result<()> {
    validate(secret)?;
    entry(provider)?.set_password(secret).map_err(|_| {
        Error(
            5,
            "system credential save failed; no plaintext fallback".into(),
        )
    })
}
pub fn delete() -> Result<()> {
    delete_for(Provider::Jev)
}
pub fn delete_for(provider: Provider) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(Error(
            5,
            "system credential deletion failed; environment/provider key not revoked".into(),
        )),
    }
}

trait Store {
    fn get(&self) -> Result<Option<Zeroizing<String>>>;
    fn put(&self, secret: &str) -> Result<()>;
    fn remove(&self) -> Result<()>;
}
struct SystemStore(Provider);
impl Store for SystemStore {
    fn get(&self) -> Result<Option<Zeroizing<String>>> {
        system_for(self.0)
    }
    fn put(&self, secret: &str) -> Result<()> {
        set_for(self.0, secret)
    }
    fn remove(&self) -> Result<()> {
        delete_for(self.0)
    }
}
/// Save a pending secret at setup completion or explicit save, compensating a config failure.
pub fn save_config(paths: &Paths, cfg: &Config, expected: &str, secret: &str) -> Result<()> {
    save_using(paths, cfg, expected, secret, &SystemStore(cfg.provider))
}
fn save_using(
    paths: &Paths,
    cfg: &Config,
    expected: &str,
    secret: &str,
    store: &impl Store,
) -> Result<()> {
    validate(secret)?;
    let mut next = cfg.clone();
    next.set_credential_source("system");
    next.validate()?;
    let _lock = paths.credential_lock(true)?;
    if paths.fingerprint()? != expected {
        return Err(Error(
            7,
            "configuration changed; reopen setup before saving".into(),
        ));
    }
    let old = store.get()?;
    paths.rotate_credential_epoch()?;
    store.put(secret)?;
    if let Err(error) = paths.save(&next, expected) {
        let restored = match old {
            Some(old) => store.put(&old),
            None => store.remove(),
        };
        if restored.is_err() {
            return Err(Error(
                7,
                "credential rollback failed; OS entry needs reconciliation before retrying setup"
                    .into(),
            ));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    struct Memory<'a> {
        value: RefCell<Option<Zeroizing<String>>>,
        puts: Cell<usize>,
        fail_at: usize,
        race: Option<&'a Paths>,
    }
    impl Store for Memory<'_> {
        fn get(&self) -> Result<Option<Zeroizing<String>>> {
            Ok(self.value.borrow().clone())
        }
        fn put(&self, secret: &str) -> Result<()> {
            let n = self.puts.get() + 1;
            self.puts.set(n);
            if n == self.fail_at {
                return Err(Error(5, "fixture write refused".into()));
            }
            *self.value.borrow_mut() = Some(Zeroizing::new(secret.to_owned()));
            if n == 1
                && let Some(paths) = self.race
            {
                paths.save(&Config::default(), "absent")?;
            }
            Ok(())
        }
        fn remove(&self) -> Result<()> {
            *self.value.borrow_mut() = None;
            Ok(())
        }
    }
    fn fixture() -> (tempfile::TempDir, Paths) {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let p = Paths {
            config: root.join("private/config.json"),
            data: root.join("data"),
            cache: root.join("cache"),
            home: root,
        };
        (t, p)
    }
    #[test]
    fn save_and_replace_without_plaintext_config() {
        let (_t, paths) = fixture();
        let store = Memory {
            value: RefCell::new(None),
            puts: Cell::new(0),
            fail_at: 0,
            race: None,
        };
        for secret in ["synthetic-first-value", "synthetic-replacement-value"] {
            save_using(
                &paths,
                &Config::default(),
                &paths.fingerprint().unwrap(),
                secret,
                &store,
            )
            .unwrap();
            assert!(
                store
                    .value
                    .borrow()
                    .as_ref()
                    .is_some_and(|v| v.as_str() == secret)
            );
            assert_eq!(paths.load().unwrap().credential, "system");
            assert!(
                !std::fs::read_to_string(&paths.config)
                    .unwrap()
                    .contains(secret)
            );
        }
    }
    #[test]
    fn invalid_conflict_and_store_failure_do_not_commit() {
        for (secret, conflict, failure) in [
            ("", false, 0),
            ("bad\nkey", false, 0),
            ("synthetic", true, 0),
            ("synthetic", false, 1),
        ] {
            let (_t, paths) = fixture();
            let store = Memory {
                value: RefCell::new(Some(Zeroizing::new("original".into()))),
                puts: Cell::new(0),
                fail_at: failure,
                race: None,
            };
            if conflict {
                paths.save(&Config::default(), "absent").unwrap();
            }
            assert!(save_using(&paths, &Config::default(), "absent", secret, &store).is_err());
            assert!(
                store
                    .value
                    .borrow()
                    .as_ref()
                    .is_some_and(|v| v.as_str() == "original")
            );
            assert_eq!(paths.config.exists(), conflict);
            assert_eq!(store.puts.get(), failure);
        }
    }
    #[test]
    fn config_failure_restores_old_or_deletes_new_entry() {
        for old in [None, Some("original")] {
            let (_t, paths) = fixture();
            let store = Memory {
                value: RefCell::new(old.map(|s| Zeroizing::new(s.into()))),
                puts: Cell::new(0),
                fail_at: 0,
                race: Some(&paths),
            };
            assert!(save_using(&paths, &Config::default(), "absent", "new", &store).is_err());
            assert!(store.value.borrow().as_deref().map(String::as_str) == old);
            assert_eq!(paths.load().unwrap(), Config::default());
        }
    }
    #[test]
    fn compensation_failure_is_explicit() {
        let (_t, paths) = fixture();
        let store = Memory {
            value: RefCell::new(Some(Zeroizing::new("original".into()))),
            puts: Cell::new(0),
            fail_at: 2,
            race: Some(&paths),
        };
        let error = save_using(&paths, &Config::default(), "absent", "new", &store).unwrap_err();
        assert_eq!(error.0, 7);
        assert!(error.1.contains("rollback failed"));
        assert_eq!(paths.load().unwrap(), Config::default());
    }
}
