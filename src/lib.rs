pub mod adapter;
pub mod config;
pub mod credential;
pub mod engine;
pub mod picker;
pub mod provider;
pub mod shell;
pub mod shell_install;
pub mod store;

use std::fmt;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub struct Error(pub i32, pub std::borrow::Cow<'static, str>);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "J{}: {}", self.0, self.1)
    }
}
impl std::error::Error for Error {}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
pub fn display(text: &str) -> String {
    text.chars().flat_map(|c| if c.is_control() || matches!(c, '\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') { c.escape_unicode().collect::<Vec<_>>() } else { vec![c] }).collect()
}
/// Quote for a POSIX shell so a printed remedy can be pasted as-is.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
