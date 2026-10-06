//! Built-in semantic drivers. Navigation and transport consume these contracts;
//! only this registry knows which implementations are shipped.
mod clef;
mod jev;
mod tev1;

use crate::{Error, Result, config::Config};
use serde_json::Value;

pub struct Text(pub &'static str, pub &'static str);
impl Text {
    pub fn get(&self, chinese: bool) -> &'static str {
        if chinese { self.1 } else { self.0 }
    }
}
pub struct CredentialSpec {
    pub env: &'static [&'static str],
    pub service: &'static str,
    pub account: &'static str,
    pub source: fn(&Config) -> &str,
    pub set_source: fn(&mut Config, &str),
}
pub struct Field {
    pub key: &'static str,
    pub discover: bool,
    /// Existing fields retain flat storage; new fields use provider namespaces.
    pub nested: bool,
    pub label: Text,
    pub help: Text,
    pub default: &'static str,
    pub get: fn(&Config) -> String,
    pub set: fn(&mut Config, &str) -> Result<()>,
}
pub struct Descriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub summary: Text,
    pub missing: Text,
    pub fields: &'static [Field],
    pub credential: Option<CredentialSpec>,
}
#[derive(Clone, Copy)]
pub struct Capabilities {
    pub max_candidates: usize,
    pub max_body_bytes: usize,
    /// Conservative serialized-byte bound, including instructions. UTF-8 bytes
    /// upper-bound byte-level tokenization; the rest is reserved for the template.
    pub context_bytes: Option<usize>,
    pub diverse_shortlist: bool,
    pub cacheable: bool,
}
pub const CLOUD: Capabilities = Capabilities {
    max_candidates: 254,
    max_body_bytes: 65536,
    context_bytes: None,
    diverse_shortlist: false,
    cacheable: true,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    OfficialHttps,
    Loopback,
}
pub struct Connection {
    pub url: String,
    pub context: String,
    pub transport: Transport,
}
/// Only privacy-filtered task JSON enters a driver; it has no candidate paths,
/// database handle, credentials or network client.
pub trait ProviderDriver: Sync {
    fn descriptor(&self) -> &'static Descriptor;
    fn model(&self) -> &'static str;
    fn selected_model<'a>(&self, _cfg: &'a Config) -> &'a str {
        self.model()
    }
    fn accepts_model(&self, model: &str) -> bool {
        model == self.model()
    }
    fn capabilities(&self) -> Capabilities {
        CLOUD
    }
    fn connection(&self, cfg: &Config, context: Option<&str>) -> Result<Connection>;
    fn prepare(&self, mut task: Value, model: &str) -> Result<Vec<u8>> {
        if !self.accepts_model(model) {
            return Err(Error(5, "unsupported request model".into()));
        }
        task["model"] = model.into();
        serde_json::to_vec(&task).map_err(|_| Error(5, "cannot encode provider task".into()))
    }
    fn decode(&self, value: Value) -> Result<Value> {
        Ok(value)
    }
    fn config_notice(&self, _cfg: &Config, _chinese: bool) -> Option<String> {
        None
    }
    fn setup_help(&self, _cfg: &Config, _chinese: bool) -> Option<String> {
        None
    }
    fn recovery(&self, _cfg: &Config, _error: &Error, _chinese: bool) -> Option<String> {
        None
    }
    fn diagnostic(
        &self,
        _cfg: &Config,
        _kind: DiagnosticKind,
        _responses: &[Value],
    ) -> Result<Diagnostic> {
        Err(Error(2,"provider-check is available for local model drivers; cloud requests use the normal interactive route".into()))
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Check,
    Models,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOption {
    pub id: String,
    pub size_bytes: u64,
}
pub enum Diagnostic {
    Request {
        path: &'static str,
        body: Option<Vec<u8>>,
    },
    Complete(Value),
}
pub static DRIVERS: &[&dyn ProviderDriver] = &[
    &jev::JEV,
    &clef::CLEF,
    &tev1::TEV1,
    #[cfg(test)]
    &tests::TINY,
];
pub fn find(id: &str) -> Option<&'static dyn ProviderDriver> {
    DRIVERS.iter().copied().find(|d| d.descriptor().id == id)
}
pub fn ids() -> String {
    DRIVERS
        .iter()
        .map(|d| d.descriptor().id)
        .collect::<Vec<_>>()
        .join("|")
}
pub fn for_model(model: &str) -> Result<&'static dyn ProviderDriver> {
    DRIVERS
        .iter()
        .copied()
        .find(|d| d.accepts_model(model))
        .ok_or(Error(5, "unsupported request model".into()))
}
pub fn field(key: &str) -> Option<&'static Field> {
    DRIVERS
        .iter()
        .flat_map(|d| d.descriptor().fields)
        .find(|f| f.key == key)
}
pub fn validate_settings(cfg: &Config) -> Result<()> {
    for (id, settings) in &cfg.providers {
        let d = find(id).ok_or(Error(7, "unknown provider settings".into()))?;
        for (key, value) in settings {
            let field = d
                .descriptor()
                .fields
                .iter()
                .find(|f| f.key == key && f.nested)
                .ok_or(Error(7, "unknown provider setting".into()))?;
            (field.set)(&mut cfg.clone(), value)?;
        }
    }
    for d in DRIVERS {
        for f in d.descriptor().fields {
            (f.set)(&mut cfg.clone(), &(f.get)(cfg))?;
        }
    }
    Ok(())
}

/// Explicit user selection initializes new provider fields, and retains the
/// effective values of an old profile before switching away. Loading is read-only.
pub fn select(cfg: &mut Config, provider: crate::config::Provider) -> Result<()> {
    if cfg.provider != provider {
        for f in cfg
            .provider
            .driver()
            .descriptor()
            .fields
            .iter()
            .filter(|f| f.nested)
        {
            let value = (f.get)(cfg);
            (f.set)(cfg, &value)?;
        }
        cfg.provider = provider;
        if !cfg.providers.contains_key(provider.id()) {
            for f in provider
                .driver()
                .descriptor()
                .fields
                .iter()
                .filter(|f| f.nested)
            {
                (f.set)(cfg, f.default)?;
            }
        }
    }
    Ok(())
}

// Associated constants are source conveniences, not an enum dispatch mechanism.
#[allow(non_upper_case_globals)]
impl crate::config::Provider {
    pub const Jev: Self = Self::builtin("jev");
    pub const ClefFlash: Self = Self::builtin("clef-flash");
    pub const Tev1: Self = Self::builtin("tev1");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Paths, Provider},
        engine::{Candidate, Group},
        provider,
    };
    pub struct Tiny;
    pub static TINY: Tiny = Tiny;
    static DESC: Descriptor = Descriptor {
        id: "tiny-test",
        label: "Tiny test",
        summary: Text("test", "test"),
        missing: Text("test", "test"),
        fields: &[],
        credential: None,
    };
    impl ProviderDriver for Tiny {
        fn descriptor(&self) -> &'static Descriptor {
            &DESC
        }
        fn model(&self) -> &'static str {
            "tiny-test-1"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                max_candidates: 2,
                cacheable: false,
                ..CLOUD
            }
        }
        fn connection(&self, _: &Config, _: Option<&str>) -> Result<Connection> {
            Ok(Connection {
                url: "http://127.0.0.1:1/decision".into(),
                context: String::new(),
                transport: Transport::Loopback,
            })
        }
        fn diagnostic(&self, _: &Config, _: DiagnosticKind, _: &[Value]) -> Result<Diagnostic> {
            Ok(Diagnostic::Complete(
                serde_json::json!({"custom_probe":"ready"}),
            ))
        }
    }
    #[test]
    fn registration_alone_extends_configuration_budget_validation_and_runtime() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().canonicalize().unwrap();
        let paths = Paths {
            home: home.clone(),
            config: home.join("config/c.json"),
            data: home.join("data"),
            cache: home.join("cache"),
        };
        let cfg = Config {
            provider: Provider::parse("tiny-test").unwrap(),
            semantic: true,
            ..Config::default()
        };
        cfg.validate().unwrap();
        let reread: Config = serde_json::from_slice(&serde_json::to_vec(&cfg).unwrap()).unwrap();
        assert_eq!(reread.provider, cfg.provider);
        assert!(!cfg.provider.needs_credentials());
        assert_eq!(cfg.credential_source(), "none");
        let diagnostic = crate::adapter::diagnose(&cfg).unwrap();
        assert_eq!(diagnostic["custom_probe"], "ready");
        assert_eq!(diagnostic["requests"], 0);
        let groups: Vec<_> = (0..4)
            .map(|i| {
                let path = home.join(format!("dir{i}"));
                std::fs::create_dir(&path).unwrap();
                Group {
                    id: format!("c{i}"),
                    name: format!("dir{i}"),
                    parent: String::new(),
                    usage: "low",
                    members: vec![Candidate {
                        id: format!("c{i}"),
                        path,
                        count: 1,
                        last_seen: 1,
                        weight: 1.0,
                    }],
                }
            })
            .collect();
        let (request, n) = provider::request("directory", &groups, &home, &paths, &cfg).unwrap();
        assert_eq!(n, 2);
        let body=serde_json::to_vec(&serde_json::json!({"model":"tiny-test-1","answers":{"destination":{"type":"choice","choice":"c1","probabilities":{"c0":0.1,"c1":0.8,"none":0.1},"confidence":0.5}}})).unwrap();
        assert_eq!(
            provider::ProviderState::open(&paths)
                .unwrap()
                .decide(&request, "snapshot", &cfg, || Ok(body))
                .unwrap(),
            Some("c1".into())
        );
    }
}
