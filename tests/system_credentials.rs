/// Explicit opt-in local integration: uses only a new test-owned credential entry.
/// No project/user credential is read, no network request is made.
#[test]
#[ignore = "requires explicit local OS credential-store acceptance"]
fn isolated_os_credential_roundtrip() {
    let service = format!("j-jump.test.{}", std::process::id());
    let entry = keyring::Entry::new(&service, "isolated-test").unwrap();
    struct Cleanup<'a>(&'a keyring::Entry);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = self.0.delete_credential();
        }
    }
    let _cleanup = Cleanup(&entry);
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    entry.set_password("synthetic-test-value-1").unwrap();
    assert_eq!(entry.get_password().unwrap(), "synthetic-test-value-1");
    entry.set_password("synthetic-test-value-2").unwrap();
    assert_eq!(entry.get_password().unwrap(), "synthetic-test-value-2");
    entry.delete_credential().unwrap();
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
}
