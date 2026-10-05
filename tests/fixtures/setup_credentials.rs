//! Test-only in-memory credential backend. Never shipped in the archive.
#[path = "../../src/main.rs"]
mod cli;
use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use std::{
    any::Any,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;
#[derive(Default)]
struct State {
    secret: Mutex<Option<Zeroizing<Vec<u8>>>>,
    failed: AtomicBool,
}
struct Memory(Arc<State>);
impl CredentialApi for Memory {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        if std::env::var_os("FIXTURE_FAIL_ONCE").is_some()
            && !self.0.failed.swap(true, Ordering::SeqCst)
        {
            return Err(keyring::Error::NoEntry);
        }
        *self.0.secret.lock().unwrap() = Some(Zeroizing::new(secret.to_vec()));
        eprintln!("FIXTURE: credential stored in memory");
        Ok(())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.0
            .secret
            .lock()
            .unwrap()
            .as_ref()
            .map(|v| v.to_vec())
            .ok_or(keyring::Error::NoEntry)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        *self.0.secret.lock().unwrap() = None;
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl CredentialBuilderApi for Memory {
    fn build(&self, _: Option<&str>, _: &str, _: &str) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(Self(self.0.clone())))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn main() {
    keyring::set_default_credential_builder(Box::new(Memory(Arc::default())));
    cli::main();
}
