//! Strictly validated definitions: endpoint, auth, limits, wire and probes.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Auth {
    None,
    Bearer { env: String },
    Header { env: String, name: String },
}
fn default_candidates() -> usize {
    254
}
fn default_body_bytes() -> usize {
    65536
}
fn is_default_candidates(n: &usize) -> bool {
    *n == default_candidates()
}
fn is_default_body_bytes(n: &usize) -> bool {
    *n == default_body_bytes()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(
        default = "default_candidates",
        skip_serializing_if = "is_default_candidates"
    )]
    pub candidates: usize,
    #[serde(
        default = "default_body_bytes",
        skip_serializing_if = "is_default_body_bytes"
    )]
    pub body_bytes: usize,
    #[serde(default, skip_serializing_if = "is_default")]
    pub context_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub diverse: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cache: bool,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            candidates: default_candidates(),
            body_bytes: default_body_bytes(),
            context_bytes: None,
            diverse: false,
            cache: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProbeStep {
    pub path: String,
    /// A privacy-free synthetic task, adapted by the selected wire definition.
    #[serde(default, skip_serializing_if = "is_default")]
    pub task: Option<Value>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub expect_choice: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub checks: Vec<Expr>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub steps: Vec<ProbeStep>,
    pub report: Expr,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub id: String,
    pub model: String,
    pub url: String,
    pub transport: String,
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "is_default")]
    pub limits: Limits,
    #[serde(default, skip_serializing_if = "is_default")]
    pub response_models: Vec<String>,
    pub wire: Wire,
    #[serde(default, skip_serializing_if = "is_default")]
    pub probe: Option<Probe>,
}
const RESERVED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "content-type",
    "proxy-authorization",
    "proxy-authenticate",
    "proxy-connection",
    "te",
    "trailer",
    "upgrade",
    "keep-alive",
    "expect",
];
impl Definition {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(bad("definition exceeds 32 KiB"));
        }
        let value = crate::provider::strict_json(bytes)
            .map_err(|_| bad("not strict JSON or duplicate key"))?;
        if value.get("version").is_some_and(|v| v != 1) {
            return Err(bad("unsupported version"));
        }
        let d: Self = serde_json::from_slice(bytes).map_err(bad)?;
        d.validate()?;
        Ok(d)
    }
    pub fn validate(&self) -> Result<()> {
        let text = |s: &str| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control);
        let range = |n: usize| (256..=65536).contains(&n);
        if let Some((_, reason)) = [
            (
                serde_json::to_vec(self).map_or(true, |b| b.len() > MAX_BYTES),
                "definition exceeds 32 KiB",
            ),
            (self.version != 1, "unsupported version"),
            (
                !(1..=64).contains(&self.id.len())
                    || !self
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "id must be 1-64 lowercase letters, digits or -",
            ),
            (
                !text(&self.model),
                "model must be 1-256 bytes without control characters",
            ),
            (
                !(1..=254).contains(&self.limits.candidates)
                    || !range(self.limits.body_bytes)
                    || self.limits.context_bytes.is_some_and(|n| !range(n)),
                "limits out of range: candidates 1-254, body_bytes and context_bytes 256-65536",
            ),
            (
                self.response_models.len() > 16 || !self.response_models.iter().all(|s| text(s)),
                "response_models must be at most 16 names of 1-256 bytes without control characters",
            ),
        ]
        .into_iter()
        .find(|c| c.0)
        {
            return Err(bad(reason));
        }
        self.wire.check()?;
        let u = reqwest::Url::parse(&self.url)
            .ok()
            .filter(|u| {
                u.username().is_empty()
                    && u.password().is_none()
                    && u.query().is_none()
                    && u.fragment().is_none()
                    && u.host_str().is_some()
                    && u.port_or_known_default() != Some(0)
            })
            .ok_or_else(|| {
                bad("url must be absolute with a host and nonzero port, without credentials, query or fragment")
            })?;
        match self.transport.as_str() {
            "https" if u.scheme() == "https" => (),
            "loopback"
                if u.scheme() == "http"
                    && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))
                    && self.auth == Auth::None => {}
            _ => {
                return Err(bad(
                    "url must be https, or http to 127.0.0.1/[::1] with auth none (transport https or loopback)",
                ));
            }
        }
        if let Some(env) = self.env() {
            if env.is_empty()
                || env.len() > 128
                || !env
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                || env.as_bytes()[0].is_ascii_digit()
            {
                return Err(bad(
                    "auth env must be 1-128 uppercase letters, digits or _, not starting with a digit",
                ));
            }
            if DRIVERS
                .iter()
                .filter_map(|d| d.descriptor().credential.as_ref())
                .any(|c| c.env.contains(&env))
            {
                return Err(bad("credential variable reserved for built-in provider"));
            }
        }
        if let Auth::Header { name, .. } = &self.auth {
            let h = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .ok()
                .filter(|_| name.len() <= 64)
                .ok_or_else(|| bad("auth header name must be an HTTP token of at most 64 bytes"))?;
            if RESERVED_HEADERS.contains(&h.as_str()) {
                return Err(bad("auth header name is reserved for transport"));
            }
        }
        if let Some(p) = &self.probe {
            if self.transport != "loopback" || p.steps.is_empty() || p.steps.len() > 7 {
                return Err(bad("probe requires loopback transport and 1-7 steps"));
            }
            for step in &p.steps {
                if !step.path.starts_with('/')
                    || step.path.starts_with("//")
                    || step.path.contains(['?', '#', '\\'])
                    || step
                        .path
                        .chars()
                        .any(|c| c.is_control() || c.is_whitespace())
                    || !u.join(&step.path).is_ok_and(|t| t.origin() == u.origin())
                {
                    return Err(bad(
                        "probe step path must be a same-origin absolute path without query, fragment, backslash, whitespace or control characters",
                    ));
                }
                if step.expect_choice.is_some() && step.task.is_none() {
                    return Err(bad("probe expect_choice requires task"));
                }
                if let Some(task) = &step.task {
                    self.wire.prepare(task.clone(), &self.model).map_err(|e| {
                        bad(format_args!(
                            "probe task: {}",
                            e.1.trim_start_matches("provider DSL ")
                        ))
                    })?;
                }
                let mut n = 0;
                for c in &step.checks {
                    c.check(0, &mut n)?;
                }
            }
            p.report.check(0, &mut 0)?;
        }
        Ok(())
    }
    pub fn env(&self) -> Option<&str> {
        match &self.auth {
            Auth::None => None,
            Auth::Bearer { env } | Auth::Header { env, .. } => Some(env),
        }
    }
    pub fn identity(&self) -> String {
        crate::digest(&serde_json::to_vec(self).expect("serializable definition"))
    }
}
