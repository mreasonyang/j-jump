//! A bounded, pure JSON interpreter. Only transport code can resolve credentials.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use std::collections::BTreeMap;

const MAX_BYTES: usize = 32768;
const MAX_VALUE: usize = 262144;
const MAX_ITEMS: usize = 1024;
const MAX_DEPTH: usize = 32;
const FUEL: usize = 100_000;
fn invalid() -> Error {
    Error(
        5,
        "provider DSL schema, expression or resource limit failed".into(),
    )
}

fn present_value<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    Literal {
        value: Value,
    },
    Ref {
        path: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "present_value"
        )]
        default: Option<Value>,
    },
    Object {
        fields: BTreeMap<String, Expr>,
    },
    Array {
        items: Vec<Expr>,
    },
    Entries {
        input: Box<Expr>,
    },
    Map {
        input: Box<Expr>,
        body: Box<Expr>,
    },
    Filter {
        input: Box<Expr>,
        condition: Box<Expr>,
    },
    VersionAtLeast {
        input: Box<Expr>,
        minimum: [u64; 3],
    },
    Any {
        items: Vec<Expr>,
    },
    MapValues {
        input: Box<Expr>,
        body: Box<Expr>,
    },
    Index {
        input: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
    },
    Stringify {
        input: Box<Expr>,
    },
    Text {
        input: Box<Expr>,
    },
    If {
        condition: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
    Eq {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Le {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Add {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Length {
        input: Box<Expr>,
    },
    All {
        items: Vec<Expr>,
    },
    Not {
        input: Box<Expr>,
    },
}
impl Expr {
    fn children(&self) -> Vec<&Expr> {
        match self {
            Self::Literal { .. } | Self::Ref { .. } => vec![],
            Self::Object { fields } => fields.values().collect(),
            Self::Array { items } | Self::All { items } | Self::Any { items } => {
                items.iter().collect()
            }
            Self::VersionAtLeast { input, .. }
            | Self::Entries { input }
            | Self::Stringify { input }
            | Self::Text { input }
            | Self::Length { input }
            | Self::Not { input } => vec![input],
            Self::Filter {
                input,
                condition: body,
            }
            | Self::Map { input, body }
            | Self::MapValues { input, body } => vec![input, body],
            Self::Index { input, key, value } => vec![input, key, value],
            Self::If {
                condition,
                then,
                otherwise,
            } => vec![condition, then, otherwise],
            Self::Eq { left, right } | Self::Le { left, right } | Self::Add { left, right } => {
                vec![left, right]
            }
        }
    }
    fn check(&self, depth: usize, nodes: &mut usize) -> Result<()> {
        *nodes += 1;
        if depth > MAX_DEPTH || *nodes > 4096 {
            return Err(invalid());
        }
        if let Self::Ref { path, .. } = self
            && (!path.starts_with('/') || path.len() > 1024)
        {
            return Err(invalid());
        }
        for child in self.children() {
            child.check(depth + 1, nodes)?;
        }
        Ok(())
    }
    pub fn evaluate(&self, root: &Value) -> Result<Value> {
        bounded(root)?;
        let mut fuel = FUEL;
        let v = self.eval(root, &mut fuel, 0)?;
        bounded(&v)?;
        Ok(v)
    }
    fn eval(&self, root: &Value, fuel: &mut usize, depth: usize) -> Result<Value> {
        if *fuel == 0 || depth > MAX_DEPTH {
            return Err(invalid());
        }
        *fuel -= 1;
        let mut run = |e: &Expr| e.eval(root, fuel, depth + 1);
        let v = match self {
            Self::Literal { value } => value.clone(),
            Self::Ref { path, default } => root
                .pointer(path)
                .cloned()
                .or_else(|| default.clone())
                .ok_or_else(invalid)?,
            Self::Object { fields } => Value::Object(
                fields
                    .iter()
                    .map(|(k, e)| Ok((k.clone(), run(e)?)))
                    .collect::<Result<_>>()?,
            ),
            Self::Array { items } => {
                Value::Array(items.iter().map(&mut run).collect::<Result<_>>()?)
            }
            Self::Entries { input } => Value::Array(
                run(input)?
                    .as_object()
                    .ok_or_else(invalid)?
                    .iter()
                    .map(|(k, v)| json!({"key":k,"value":v}))
                    .collect(),
            ),
            Self::Map { input, body } => {
                let input = run(input)?;
                let a = input.as_array().ok_or_else(invalid)?;
                if a.len() > MAX_ITEMS {
                    return Err(invalid());
                }
                let mut out = Vec::new();
                for item in a {
                    let mut scope = bind_scope(root, fuel)?;
                    scope["item"] = item.clone();
                    out.push(body.eval(&scope, fuel, depth + 1)?);
                }
                Value::Array(out)
            }
            Self::Filter { input, condition } => {
                let input = run(input)?;
                let a = input.as_array().ok_or_else(invalid)?;
                let mut out = Vec::new();
                for item in a {
                    let mut scope = bind_scope(root, fuel)?;
                    scope["item"] = item.clone();
                    if condition
                        .eval(&scope, fuel, depth + 1)?
                        .as_bool()
                        .ok_or_else(invalid)?
                    {
                        out.push(item.clone());
                    }
                }
                Value::Array(out)
            }
            Self::VersionAtLeast { input, minimum } => {
                let v = run(input)?;
                let text = v.as_str().ok_or_else(invalid)?;
                let parts = text
                    .split(['-', '+'])
                    .next()
                    .unwrap_or("")
                    .split('.')
                    .map(str::parse::<u64>)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|_| invalid())?;
                if parts.len() != 3 {
                    return Err(invalid());
                }
                json!(parts.as_slice() >= minimum.as_slice())
            }
            Self::Any { items } => {
                let mut yes = false;
                for e in items {
                    yes |= run(e)?.as_bool().ok_or_else(invalid)?;
                }
                json!(yes)
            }
            Self::MapValues { input, body } => {
                let input = run(input)?;
                let a = input.as_object().ok_or_else(invalid)?;
                let mut out = Map::new();
                for (k, item) in a {
                    let mut scope = bind_scope(root, fuel)?;
                    scope["item"] = item.clone();
                    out.insert(k.clone(), body.eval(&scope, fuel, depth + 1)?);
                }
                Value::Object(out)
            }
            Self::Index { input, key, value } => {
                let input = run(input)?;
                let a = input.as_array().ok_or_else(invalid)?;
                let mut out = Map::new();
                for item in a {
                    let mut scope = bind_scope(root, fuel)?;
                    scope["item"] = item.clone();
                    let k = key
                        .eval(&scope, fuel, depth + 1)?
                        .as_str()
                        .ok_or_else(invalid)?
                        .to_owned();
                    let v = value.eval(&scope, fuel, depth + 1)?;
                    if out.insert(k, v).is_some() {
                        return Err(invalid());
                    }
                }
                Value::Object(out)
            }
            Self::Stringify { input } => Value::String(run(input)?.to_string()),
            Self::Text { input } => {
                let v = run(input)?;
                Value::String(
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string()),
                )
            }
            Self::If {
                condition,
                then,
                otherwise,
            } => {
                if run(condition)?.as_bool().ok_or_else(invalid)? {
                    run(then)?
                } else {
                    run(otherwise)?
                }
            }
            Self::Eq { left, right } => Value::Bool(run(left)? == run(right)?),
            Self::Le { left, right } => Value::Bool(
                run(left)?.as_u64().ok_or_else(invalid)?
                    <= run(right)?.as_u64().ok_or_else(invalid)?,
            ),
            Self::Add { left, right } => json!(
                run(left)?
                    .as_u64()
                    .ok_or_else(invalid)?
                    .checked_add(run(right)?.as_u64().ok_or_else(invalid)?)
                    .ok_or_else(invalid)?
            ),
            Self::Length { input } => {
                let v = run(input)?;
                json!(match &v {
                    Value::Array(a) => a.len(),
                    Value::Object(a) => a.len(),
                    Value::String(a) => a.len(),
                    _ => return Err(invalid()),
                })
            }
            Self::All { items } => {
                let mut yes = true;
                for e in items {
                    yes &= run(e)?.as_bool().ok_or_else(invalid)?;
                }
                json!(yes)
            }
            Self::Not { input } => json!(!run(input)?.as_bool().ok_or_else(invalid)?),
        };
        bounded(&v)?;
        let cost = serde_json::to_vec(&v)
            .map_err(|_| invalid())?
            .len()
            .div_ceil(64);
        *fuel = fuel.checked_sub(cost).ok_or_else(invalid)?;
        Ok(v)
    }
}
fn bind_scope(root: &Value, fuel: &mut usize) -> Result<Value> {
    // Account for copied input, so nested maps of tiny literals cannot bypass
    // the output-byte budget by repeatedly cloning a large phase root.
    let cost = serde_json::to_vec(root)
        .map_err(|_| invalid())?
        .len()
        .div_ceil(1024);
    *fuel = fuel.checked_sub(cost).ok_or_else(invalid)?;
    Ok(root.clone())
}
fn bounded(v: &Value) -> Result<()> {
    fn walk(v: &Value, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        match v {
            Value::Array(a) => a.len() <= MAX_ITEMS && a.iter().all(|v| walk(v, depth + 1)),
            Value::Object(a) => a.len() <= MAX_ITEMS && a.values().all(|v| walk(v, depth + 1)),
            _ => true,
        }
    }
    if !walk(v, 0) || serde_json::to_vec(v).map_err(|_| invalid())?.len() > MAX_VALUE {
        return Err(invalid());
    }
    Ok(())
}
/// Deliberately small shape language, with strict objects by default.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub properties: BTreeMap<String, Shape>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub extra: bool,
    #[serde(default)]
    pub items: Option<Box<Shape>>,
    #[serde(default)]
    pub values: Option<Box<Shape>>,
    #[serde(default)]
    pub choices: Vec<Value>,
    #[serde(default)]
    pub any: Vec<Shape>,
    #[serde(default)]
    pub min: Option<usize>,
    #[serde(default)]
    pub max: Option<usize>,
}
impl Shape {
    fn definition_check(&self, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH
            || ![
                "", "object", "array", "string", "uint", "number", "boolean", "null",
            ]
            .contains(&self.kind.as_str())
            || self.min.zip(self.max).is_some_and(|(a, b)| a > b)
            || self
                .required
                .iter()
                .any(|k| !self.properties.contains_key(k))
        {
            return Err(invalid());
        }
        for s in self
            .properties
            .values()
            .chain(self.items.as_deref())
            .chain(self.values.as_deref())
            .chain(&self.any)
        {
            s.definition_check(depth + 1)?;
        }
        Ok(())
    }
    pub fn validate(&self, v: &Value) -> Result<()> {
        if !self.any.is_empty() {
            return if self.any.iter().any(|s| s.validate(v).is_ok()) {
                Ok(())
            } else {
                Err(invalid())
            };
        }
        if !self.choices.is_empty() && !self.choices.contains(v) {
            return Err(invalid());
        }
        let len = match self.kind.as_str() {
            "" => None,
            "object" => {
                let a = v.as_object().ok_or_else(invalid)?;
                if self.required.iter().any(|k| !a.contains_key(k)) {
                    return Err(invalid());
                }
                for (k, v) in a {
                    if let Some(s) = self.properties.get(k).or(self.values.as_deref()) {
                        s.validate(v)?;
                    } else if !self.extra {
                        return Err(invalid());
                    }
                }
                Some(a.len())
            }
            "array" => {
                let a = v.as_array().ok_or_else(invalid)?;
                if let Some(s) = &self.items {
                    for v in a {
                        s.validate(v)?;
                    }
                }
                Some(a.len())
            }
            "string" => Some(v.as_str().ok_or_else(invalid)?.len()),
            "uint" if v.as_u64().is_some() => None,
            "number" if v.is_number() => None,
            "boolean" if v.is_boolean() => None,
            "null" if v.is_null() => None,
            _ => return Err(invalid()),
        };
        if len.is_some_and(|n| self.min.is_some_and(|m| n < m) || self.max.is_some_and(|m| n > m)) {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub request: Expr,
    pub response: Expr,
    pub questions: Expr,
    #[serde(default)]
    pub request_shape: Shape,
    #[serde(default)]
    pub response_shape: Shape,
    #[serde(default)]
    pub response_checks: Vec<Expr>,
}
impl Wire {
    pub fn check(&self) -> Result<()> {
        let mut nodes = 0;
        for e in [&self.request, &self.response, &self.questions]
            .into_iter()
            .chain(&self.response_checks)
        {
            e.check(0, &mut nodes)?;
        }
        self.request_shape.definition_check(0)?;
        self.response_shape.definition_check(0)
    }
    pub fn prepare(&self, task: Value, model: &str) -> Result<Vec<u8>> {
        let v = self
            .request
            .evaluate(&json!({"task":&task,"model":model}))?;
        self.request_shape.validate(&v)?;
        if v["model"] != model {
            return Err(invalid());
        }
        // The inverse mapping must retain every question and every sent choice.
        let recovered = self.questions(&v)?;
        let expected = task["questions"].as_object().ok_or_else(invalid)?;
        let actual = recovered.as_object().ok_or_else(invalid)?;
        if expected.len() != actual.len() {
            return Err(invalid());
        }
        for (name, q) in expected {
            let a = actual
                .get(name)
                .and_then(|q| q["criteria"].as_object())
                .ok_or_else(invalid)?;
            let b = q["criteria"].as_object().ok_or_else(invalid)?;
            if a.len() != b.len() || !b.keys().all(|k| a.contains_key(k)) || !b.contains_key("none")
            {
                return Err(invalid());
            }
        }
        let bytes = serde_json::to_vec(&v).map_err(|_| invalid())?;
        Ok(bytes)
    }
    pub fn decode(&self, response: Value) -> Result<Value> {
        bounded(&response)?;
        self.response_shape.validate(&response)?;
        let root = json!({"response":response});
        bounded(&root)?;
        let mut fuel = FUEL;
        for check in &self.response_checks {
            if check.eval(&root, &mut fuel, 0)? != true {
                return Err(invalid());
            }
        }
        self.response.eval(&root, &mut fuel, 0)
    }
    pub fn questions(&self, request: &Value) -> Result<Value> {
        self.request_shape.validate(request)?;
        self.questions.evaluate(&json!({"request":request}))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Auth {
    None,
    Bearer { env: String },
    Header { env: String, name: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub candidates: usize,
    pub body_bytes: usize,
    #[serde(default)]
    pub context_bytes: Option<usize>,
    #[serde(default)]
    pub diverse: bool,
    #[serde(default)]
    pub cache: bool,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            candidates: 254,
            body_bytes: 65536,
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
    #[serde(default)]
    pub task: Option<Value>,
    #[serde(default)]
    pub expect_choice: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub response_models: Vec<String>,
    pub wire: Wire,
    #[serde(default)]
    pub probe: Option<Probe>,
}
impl Definition {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        let value = crate::provider::strict_json(bytes)?;
        let d: Self = serde_json::from_value(value).map_err(|_| invalid())?;
        d.validate()?;
        Ok(d)
    }
    pub fn validate(&self) -> Result<()> {
        if serde_json::to_vec(self).map_err(|_| invalid())?.len() > MAX_BYTES
            || self.version != 1
            || self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || self.model.is_empty()
            || self.model.len() > 256
            || self.model.chars().any(char::is_control)
            || !(1..=254).contains(&self.limits.candidates)
            || !(256..=65536).contains(&self.limits.body_bytes)
            || self
                .limits
                .context_bytes
                .is_some_and(|n| !(256..=65536).contains(&n))
            || self.response_models.len() > 16
            || self
                .response_models
                .iter()
                .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        {
            return Err(invalid());
        }
        self.wire.check()?;
        let u = reqwest::Url::parse(&self.url).map_err(|_| invalid())?;
        if !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || u.host_str().is_none()
            || u.port_or_known_default() == Some(0)
        {
            return Err(invalid());
        }
        match self.transport.as_str() {
            "https" if u.scheme() == "https" => (),
            "loopback"
                if u.scheme() == "http"
                    && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))
                    && self.auth == Auth::None => {}
            _ => return Err(invalid()),
        }
        if let Some(env) = self.env()
            && (env.is_empty()
                || env.len() > 128
                || !env
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                || env.as_bytes()[0].is_ascii_digit())
        {
            return Err(invalid());
        }
        if let Auth::Header { name, .. } = &self.auth {
            let h =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
            if [
                "host",
                "content-length",
                "connection",
                "transfer-encoding",
                "content-type",
                "proxy-authorization",
            ]
            .contains(&h.as_str())
            {
                return Err(invalid());
            }
        }
        if let Some(p) = &self.probe {
            if self.transport != "loopback" || p.steps.is_empty() || p.steps.len() > 7 {
                return Err(invalid());
            }
            for step in &p.steps {
                if !step.path.starts_with('/')
                    || step.path.starts_with("//")
                    || step.path.contains(['?', '#', '\\'])
                    || step.expect_choice.is_some() && step.task.is_none()
                {
                    return Err(invalid());
                }
                if let Some(task) = &step.task {
                    bounded(task)?;
                    self.wire.prepare(task.clone(), &self.model)?;
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
static DESCRIPTION: Descriptor = Descriptor {
    id: "dsl",
    label: "Custom API (DSL)",
    summary: Text(
        "Imported declarative decision API; configure with config import-provider.",
        "导入的声明式决策 API；使用 config import-provider 配置。",
    ),
    missing: Text(
        "Custom API key missing; use credential set or the definition's environment variable.",
        "自定义 API 缺少凭据；请使用 credential set 或定义中的环境变量。",
    ),
    fields: &[],
    credential: Some(CredentialSpec {
        env: &[],
        service: "j-jump.dsl",
        account: "api-key",
        setting: Some("credential"),
        source: |c| {
            c.providers
                .get("dsl")
                .and_then(|m| m.get("credential"))
                .map(String::as_str)
                .unwrap_or("environment")
        },
        set_source: |c, s| {
            c.providers
                .entry("dsl".into())
                .or_default()
                .insert("credential".into(), s.into());
        },
    }),
};
pub struct Unconfigured;
pub static UNCONFIGURED: Unconfigured = Unconfigured;
impl ProviderDriver for Unconfigured {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &str {
        "unconfigured"
    }
    fn connection(&self, _: &Config, _: Option<&str>) -> Result<Connection> {
        Err(Error(
            2,
            "import a provider definition with config import-provider FILE".into(),
        ))
    }
}
impl ProviderDriver for Definition {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &str {
        &self.model
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_candidates: self.limits.candidates,
            max_body_bytes: self.limits.body_bytes,
            context_bytes: self.limits.context_bytes,
            diverse_shortlist: self.limits.diverse,
            cacheable: self.limits.cache && self.transport == "https",
        }
    }
    fn connection(&self, _: &Config, context: Option<&str>) -> Result<Connection> {
        self.validate()?;
        if context.is_some_and(|s| s != self.identity()) {
            return Err(Error(5, "provider definition binding changed".into()));
        }
        Ok(Connection {
            url: self.url.clone(),
            context: self.identity(),
            transport: if self.transport == "loopback" {
                Transport::Loopback
            } else {
                Transport::OfficialHttps
            },
        })
    }
    fn prepare(&self, task: Value, model: &str) -> Result<Vec<u8>> {
        if model != self.model {
            return Err(invalid());
        }
        self.wire.prepare(task, model)
    }
    fn decode(&self, value: Value) -> Result<Value> {
        let mut v = self.wire.decode(value)?;
        if v["model"] != self.model
            && v["model"]
                .as_str()
                .is_some_and(|s| self.response_models.iter().any(|m| m == s))
        {
            v["model"] = self.model.clone().into();
        }
        Ok(v)
    }
    fn questions(&self, request: &Value) -> Result<Value> {
        self.wire.questions(request)
    }
    fn setup_help(&self, _: &Config, chinese: bool) -> Option<String> {
        Some(format!(
            "{}: {}\n{}: {}\n{}: {}",
            Text("Definition", "定义").get(chinese),
            crate::display(&self.id),
            Text("Endpoint", "接口").get(chinese),
            crate::display(&self.url),
            Text("Model", "模型").get(chinese),
            crate::display(&self.model)
        ))
    }
    fn diagnostic(
        &self,
        cfg: &Config,
        kind: DiagnosticKind,
        responses: &[Value],
    ) -> Result<Diagnostic> {
        if kind == DiagnosticKind::Models {
            return Err(Error(
                2,
                "this DSL definition does not declare model discovery".into(),
            ));
        }
        let p = self
            .probe
            .as_ref()
            .ok_or(Error(2, "this definition has no local probe plan".into()))?;
        if responses.len() > p.steps.len() {
            return Err(invalid());
        }
        let mut fuel = FUEL;
        for (i, response) in responses.iter().enumerate() {
            let step = &p.steps[i];
            let root = json!({"response":response,"responses":responses,"model":self.model});
            for check in &step.checks {
                bounded(&root)?;
                if check.eval(&root, &mut fuel, 0)? != true {
                    return Err(invalid());
                }
            }
            if let Some(choice) = &step.expect_choice {
                let request = self.prepare(step.task.clone().ok_or_else(invalid)?, &self.model)?;
                if crate::provider::validate_for(
                    cfg,
                    &serde_json::to_vec(response).map_err(|_| invalid())?,
                    &request,
                )?
                .as_ref()
                    != Some(choice)
                {
                    return Err(invalid());
                }
            }
        }
        if let Some(step) = p.steps.get(responses.len()) {
            return Ok(Diagnostic::Request {
                path: step.path.clone(),
                body: step
                    .task
                    .as_ref()
                    .map(|t| self.prepare(t.clone(), &self.model))
                    .transpose()?,
            });
        }
        let report = p
            .report
            .evaluate(&json!({"responses":responses,"model":self.model}))?;
        if !report.is_object() {
            return Err(invalid());
        }
        Ok(Diagnostic::Complete(report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task() -> Value {
        json!({"state":{"query":"backend"},"questions":{"destination":{"type":"choice","instructions":"Choose a match.","criteria":{"c1":{"name":"alpha"},"none":"No match"}}}})
    }
    fn answer(model: &str) -> Value {
        json!({"model":model,"answers":{"destination":{"type":"choice","choice":"c1","probabilities":{"c1":0.9,"none":0.1},"confidence":0.9}}})
    }
    fn definition() -> Definition {
        Definition {
            version: 1,
            id: "synthetic-api".into(),
            model: "synthetic-model".into(),
            url: "http://127.0.0.1:11434/decision".into(),
            transport: "loopback".into(),
            auth: Auth::None,
            limits: Limits::default(),
            response_models: vec![],
            wire: super::super::wire("jev").clone(),
            probe: None,
        }
    }
    fn config(d: Definition) -> Config {
        Config {
            provider: crate::config::Provider::parse("dsl").unwrap(),
            api_definition: Some(d),
            semantic: true,
            ..Config::default()
        }
    }
    fn expr(v: Value) -> Expr {
        serde_json::from_value(v).unwrap()
    }
    #[test]
    fn shipped_wire_roundtrips() {
        for id in ["jev", "clef", "tev1", "openai"] {
            let w = super::super::wire(id);
            let b = w.prepare(task(), "synthetic-model").unwrap();
            let q = w.questions(&serde_json::from_slice(&b).unwrap()).unwrap();
            assert_eq!(
                q["destination"]["criteria"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                vec!["c1", "none"]
            );
            assert!(!String::from_utf8(b).unwrap().contains("api-key"));
        }
    }
    #[test]
    fn arbitrary_wire_and_same_model_are_selected_by_config() {
        let mut d = definition();
        d.model = "gpt-6-luna".into(); // Deliberately shares a built-in model name.
        d.wire.request = expr(
            json!({"op":"object","fields":{"model":{"op":"ref","path":"/model"},"payload":{"op":"ref","path":"/task"}}}),
        );
        d.wire.questions = expr(json!({"op":"ref","path":"/request/payload/questions"}));
        d.wire.response = expr(json!({"op":"ref","path":"/response/result"}));
        let cfg = config(d);
        cfg.validate().unwrap();
        let b = cfg.driver().prepare(task(), "gpt-6-luna").unwrap();
        assert_eq!(
            crate::provider::validate_for(
                &cfg,
                &serde_json::to_vec(&json!({"result":answer("gpt-6-luna")})).unwrap(),
                &b
            )
            .unwrap()
            .as_deref(),
            Some("c1")
        );
        assert!(
            crate::provider::validate(&serde_json::to_vec(&answer("gpt-6-luna")).unwrap(), &b)
                .is_err()
        );
    }
    #[test]
    fn canonical_validation_cannot_be_disabled() {
        let cfg = config(definition());
        let b = cfg.driver().prepare(task(), "synthetic-model").unwrap();
        for (path, v) in [
            ("/model", json!("wrong")),
            ("/answers/destination/choice", json!("forged")),
            ("/answers/destination/probabilities/c1", json!(1.5)),
            ("/answers/destination/confidence", json!(-1)),
        ] {
            let mut a = answer("synthetic-model");
            *a.pointer_mut(path).unwrap() = v;
            assert!(
                crate::provider::validate_for(&cfg, &serde_json::to_vec(&a).unwrap(), &b).is_err(),
                "{path}"
            );
        }
        for t in ["refusal", "choice"] {
            let mut a = answer("synthetic-model");
            if t == "refusal" {
                a["answers"]["destination"] = json!({"type":"refusal"});
            } else {
                a["answers"]["destination"] = json!({"type":"choice","choice":"none","probabilities":{"c1":0.1,"none":0.9},"confidence":0.9});
            }
            assert_eq!(
                crate::provider::validate_for(&cfg, &serde_json::to_vec(&a).unwrap(), &b).unwrap(),
                None
            );
        }
    }
    #[test]
    fn explicit_aliases_and_definition_identity() {
        let mut d = definition();
        d.response_models.push("versioned-model".into());
        let old = d.identity();
        let cfg = config(d.clone());
        let b = d.prepare(task(), &d.model).unwrap();
        assert_eq!(
            crate::provider::validate_for(
                &cfg,
                &serde_json::to_vec(&answer("versioned-model")).unwrap(),
                &b
            )
            .unwrap()
            .as_deref(),
            Some("c1")
        );
        assert!(
            crate::provider::validate_for(
                &cfg,
                &serde_json::to_vec(&answer("fallback-model")).unwrap(),
                &b
            )
            .is_err()
        );
        d.response_models.clear();
        assert_ne!(old, d.identity());
        assert!(d.connection(&cfg, Some(&old)).is_err());
    }
    #[test]
    fn malformed_definition_import_fails_closed() {
        let v = serde_json::to_value(definition()).unwrap();
        for (k, x) in [
            ("version", json!(2)),
            ("unknown", json!(true)),
            ("id", json!("../jev")),
            ("transport", json!("shell")),
            ("url", json!("file:///tmp/test")),
            ("model", json!("bad\nmodel")),
        ] {
            let mut v = v.clone();
            v[k] = x;
            assert!(
                Definition::parse(&serde_json::to_vec(&v).unwrap()).is_err(),
                "{k}"
            );
        }
        assert!(Definition::parse(br#"{"version":1,"version":1}"#).is_err());
        assert!(Definition::parse(&vec![b' '; 32769]).is_err());
        assert!(serde_json::from_value::<Expr>(json!({"op":"shell","command":"whoami"})).is_err());
        assert!(
            serde_json::from_value::<Expr>(json!({"op":"ref","path":"/task","env":"HOME"}))
                .is_err()
        );
    }
    #[test]
    fn transport_and_auth_boundaries() {
        for url in [
            "http://example.invalid/api",
            "http://localhost:11434/api",
            "http://127.0.0.2/api",
            "http://127.0.0.1@remote.invalid/api",
            "http://127.0.0.1:0/api",
            "http://127.0.0.1/api?key=test",
            "http://127.0.0.1/api#fragment",
        ] {
            let mut d = definition();
            d.url = url.into();
            assert!(d.validate().is_err(), "{url}");
        }
        let mut d = definition();
        d.url = "http://[::1]:11434/api".into();
        d.validate().unwrap();
        d.auth = Auth::Bearer {
            env: "TEST_API_KEY".into(),
        };
        assert!(d.validate().is_err());
        d.transport = "https".into();
        d.url = "https://example.invalid/decision".into();
        d.validate().unwrap();
        for env in ["", "HOME=test", "lower_case", "1KEY"] {
            d.auth = Auth::Bearer { env: env.into() };
            assert!(d.validate().is_err());
        }
        for name in [
            "host",
            "CONTENT-LENGTH",
            "connection",
            "proxy-authorization",
            "bad\nheader",
        ] {
            d.auth = Auth::Header {
                env: "TEST_API_KEY".into(),
                name: name.into(),
            };
            assert!(d.validate().is_err());
        }
        d.auth = Auth::Header {
            env: "TEST_API_KEY".into(),
            name: "x-api-key".into(),
        };
        d.validate().unwrap();
    }
    #[test]
    fn duplicate_index_keys_and_missing_references_fail() {
        let e = expr(
            json!({"op":"index","input":{"op":"ref","path":"/array"},"key":{"op":"ref","path":"/item/id"},"value":{"op":"ref","path":"/item"}}),
        );
        assert!(
            e.evaluate(&json!({"array":[{"id":"x"},{"id":"x"}]}))
                .is_err()
        );
        assert!(
            expr(json!({"op":"ref","path":"/secret"}))
                .evaluate(&json!({}))
                .is_err()
        );
        assert_eq!(
            expr(json!({"op":"ref","path":"/missing","default":null}))
                .evaluate(&json!({}))
                .unwrap(),
            Value::Null
        );
    }
    #[test]
    fn bounded_depth_collections_and_amplification() {
        let e = Expr::Literal {
            value: json!(vec![0; 1025]),
        };
        assert!(e.evaluate(&json!({})).is_err());
        let e = Expr::Literal {
            value: json!("x".repeat(262145)),
        };
        assert!(e.evaluate(&json!({})).is_err());
        let mut e = Expr::Literal { value: json!(true) };
        for _ in 0..33 {
            e = Expr::Not { input: Box::new(e) };
        }
        assert!(e.check(0, &mut 0).is_err());
        assert!(e.evaluate(&json!({})).is_err());
        let e = expr(
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"ref","path":"/a"}}),
        );
        assert!(
            e.evaluate(&json!({"a":vec!["x".repeat(128);1024]}))
                .is_err()
        );
    }
    #[test]
    fn inverse_mapping_cannot_drop_or_invent_choices() {
        let mut d = definition();
        d.wire.questions = Expr::Literal {
            value: json!({"destination":{"criteria":{"none":"No match"}}}),
        };
        assert!(d.prepare(task(), &d.model).is_err());
    }
    #[test]
    fn response_assertions_share_one_work_budget() {
        let mut w = super::super::wire("jev").clone();
        let check = expr(
            json!({"op":"eq","left":{"op":"length","input":{"op":"map","input":{"op":"ref","path":"/response/metadata"},"body":{"op":"literal","value":0}}},"right":{"op":"literal","value":100}}),
        );
        w.response_checks = vec![check.clone(); 100];
        w.check().unwrap();
        let response = json!({"metadata":vec!["x".repeat(128);100]});
        assert_eq!(check.evaluate(&json!({"response":response})).unwrap(), true);
        assert!(w.decode(response).is_err());
    }
    #[test]
    fn repeated_large_input_copies_consume_evaluation_budget() {
        let tiny = expr(
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"literal","value":null}}),
        );
        assert!(
            tiny.evaluate(&json!({"a":vec!["x".repeat(128);1024]}))
                .is_err()
        );
    }
    #[test]
    fn shape_validates_nested_keys_types_bounds_and_overflow() {
        let s: Shape = serde_json::from_value(
            json!({"kind":"object","required":["n"],"properties":{"n":{"kind":"uint"}}}),
        )
        .unwrap();
        s.validate(&json!({"n":1})).unwrap();
        for v in [json!({}), json!({"n":-1}), json!({"n":1,"extra":true})] {
            assert!(s.validate(&v).is_err());
        }
        let e = expr(
            json!({"op":"add","left":{"op":"literal","value":u64::MAX},"right":{"op":"literal","value":1}}),
        );
        assert!(e.evaluate(&json!({})).is_err());
    }
    #[test]
    fn gateway_examples_are_valid_and_compact_in_config() {
        for text in [
            include_str!("../../examples/providers/openrouter-jev.json"),
            include_str!("../../examples/providers/vercel-openai.json"),
            include_str!("../../examples/providers/local-tev1.json"),
            include_str!("../../examples/providers/openrouter-microsoft.json"),
            include_str!("../../examples/providers/openrouter-openai.json"),
            include_str!("../../examples/providers/openrouter-clef.json"),
        ] {
            let d = Definition::parse(text.as_bytes()).unwrap();
            let b = d.prepare(task(), &d.model).unwrap();
            assert!(!b.is_empty());
            let cfg = config(d);
            cfg.validate().unwrap();
            let serialized = serde_json::to_vec(&cfg).unwrap();
            assert!(serialized.len() < 65536);
            assert_eq!(serde_json::from_slice::<Config>(&serialized).unwrap(), cfg);
        }
    }
    #[test]
    fn oversized_serialized_config_is_refused_without_replacing_saved_state() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let paths = crate::config::Paths {
            config: home.join("config/config.json"),
            data: home.join("data"),
            cache: home.join("cache"),
            home,
        };
        paths.save(&Config::default(), "absent").unwrap();
        let before = std::fs::read(&paths.config).unwrap();
        let mut d = definition();
        d.wire.response = Expr::Object {
            fields: (0..750)
                .map(|i| (format!("field{i}"), Expr::Literal { value: Value::Null }))
                .collect(),
        };
        d.validate().unwrap();
        let cfg = config(d);
        assert!(serde_json::to_vec_pretty(&cfg).unwrap().len() > 65536);
        assert!(paths.save(&cfg, &paths.fingerprint().unwrap()).is_err());
        assert_eq!(std::fs::read(&paths.config).unwrap(), before);
        paths.load().unwrap();
    }
    #[test]
    fn definition_digest_isolates_cache_and_circuit() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let paths = crate::config::Paths {
            config: home.join("config/config.json"),
            data: home.join("data"),
            cache: home.join("cache"),
            home,
        };
        let mut d = definition();
        d.transport = "https".into();
        d.url = "https://example.invalid/decision".into();
        d.limits.cache = true;
        let mut cfg = config(d);
        let request = cfg.driver().prepare(task(), "synthetic-model").unwrap();
        let state = crate::provider::ProviderState::open(&paths).unwrap();
        let calls = std::cell::Cell::new(0);
        let send = || {
            calls.set(calls.get() + 1);
            Ok(serde_json::to_vec(&answer("synthetic-model")).unwrap())
        };
        state
            .decide(&request, "synthetic-credential", &cfg, send)
            .unwrap();
        state
            .decide(&request, "synthetic-credential", &cfg, || {
                panic!("validated cache should be reused")
            })
            .unwrap();
        cfg.api_definition.as_mut().unwrap().id = "different-definition".into();
        state
            .decide(&request, "synthetic-credential", &cfg, send)
            .unwrap();
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn local_preset_checks_version_weights_and_synthetic_choice() {
        let d =
            Definition::parse(include_bytes!("../../examples/providers/local-tev1.json")).unwrap();
        let cfg = config(d.clone());
        for v in ["0.34.9", "0.35", "garbage"] {
            assert!(
                d.diagnostic(&cfg, DiagnosticKind::Check, &[json!({"version":v})])
                    .is_err()
            );
        }
        let responses = vec![
            json!({"version":"0.35.1"}),
            json!({"models":[{"name":d.model,"details":{"format":"gguf"}}]}),
        ];
        assert!(matches!(
            d.diagnostic(&cfg, DiagnosticKind::Check, &responses)
                .unwrap(),
            Diagnostic::Request { .. }
        ));
        let mut wrong = responses.clone();
        wrong[1]["models"][0]["details"]["format"] = json!("mlx");
        assert!(d.diagnostic(&cfg, DiagnosticKind::Check, &wrong).is_err());
        wrong[1]["models"] = json!([]);
        assert!(d.diagnostic(&cfg, DiagnosticKind::Check, &wrong).is_err());
    }
    #[test]
    fn gateway_responses_normalize_only_declared_metadata() {
        let d = Definition::parse(include_bytes!(
            "../../examples/providers/openrouter-jev.json"
        ))
        .unwrap();
        let cfg = config(d.clone());
        let b = d.prepare(task(), &d.model).unwrap();
        let mut response = answer(&d.model);
        response["usage"] = json!({"input_tokens":10,"output_tokens":0,"cost":0.001});
        response["id"] = json!("synthetic-request");
        response["provider"] = json!("TypeSafe");
        assert_eq!(
            crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
                .unwrap()
                .as_deref(),
            Some("c1")
        );
        response["model"] = json!("typesafe/jev-1.13-20260917");
        assert_eq!(
            crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
                .unwrap()
                .as_deref(),
            Some("c1")
        );
        response["unexpected"] = json!(true);
        assert!(d.decode(response).is_err());
        let d = Definition::parse(include_bytes!(
            "../../examples/providers/vercel-openai.json"
        ))
        .unwrap();
        let cfg = config(d.clone());
        let b = d.prepare(task(), &d.model).unwrap();
        let mut response = json!({"model":d.model,"answers":[{"name":"destination","type":"choice","choice":"c1","confidence":0.9,"probabilities":[{"value":"c1","probability":0.9},{"value":"none","probability":0.1}]}],"usage":{"input_tokens":10,"output_tokens":0,"total_tokens":10},"provider_metadata":{"gateway":{"cost":"synthetic"}}});
        assert_eq!(
            crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
                .unwrap()
                .as_deref(),
            Some("c1")
        );
        response["usage"]["input_tokens_details"] =
            json!({"cached_tokens":0,"cache_write_tokens":0});
        response["usage"]["output_tokens_details"] = json!({"reasoning_tokens":0});
        assert_eq!(
            crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
                .unwrap()
                .as_deref(),
            Some("c1")
        );
        response["usage"]["input_tokens_details"]["cached_tokens"] = json!(11);
        assert!(d.decode(response.clone()).is_err());
        response["usage"]["input_tokens_details"]["cached_tokens"] = json!(0);
        response["answers"][0]["probabilities"]
            .as_array_mut()
            .unwrap()
            .push(json!({"value":"c1","probability":0.9}));
        assert!(
            crate::provider::validate_for(&cfg, &serde_json::to_vec(&response).unwrap(), &b)
                .is_err()
        );
    }
    #[test]
    fn finite_probe_paths_and_expected_choice_are_validated() {
        let mut d = definition();
        d.probe = Some(Probe {
            steps: vec![ProbeStep {
                path: "/decision".into(),
                task: Some(task()),
                expect_choice: Some("c1".into()),
                checks: vec![],
            }],
            report: Expr::Literal {
                value: json!({"synthetic_request":"passed"}),
            },
        });
        d.validate().unwrap();
        let cfg = config(d.clone());
        assert!(matches!(
            d.diagnostic(&cfg, DiagnosticKind::Check, &[]).unwrap(),
            Diagnostic::Request { .. }
        ));
        assert!(matches!(
            d.diagnostic(&cfg, DiagnosticKind::Check, &[answer(&d.model)])
                .unwrap(),
            Diagnostic::Complete(_)
        ));
        assert!(
            d.diagnostic(&cfg, DiagnosticKind::Check, &[answer("wrong")])
                .is_err()
        );
        for path in [
            "//remote.invalid",
            "/api?query",
            "/api#fragment",
            "/\\remote",
        ] {
            d.probe.as_mut().unwrap().steps[0].path = path.into();
            assert!(d.validate().is_err());
        }
    }
}
