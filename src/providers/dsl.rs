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
const LIMIT: &str = "provider DSL resource limit exceeded";
fn bad(reason: impl std::fmt::Display) -> Error {
    Error(2, format!("invalid provider definition: {reason}").into())
}
fn fail(reason: impl std::fmt::Display) -> Error {
    Error(5, format!("provider DSL {reason}").into())
}
fn ty(op: &str) -> Error {
    fail(format_args!("{op} operand has the wrong type"))
}
fn limit() -> Error {
    Error(5, LIMIT.into())
}
/// Core may retry with fewer candidates only for exhausted evaluation budgets.
pub fn is_resource_limit(e: &Error) -> bool {
    e.0 == 5 && e.1.starts_with("provider DSL resource limit")
}
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
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
/// Phase root plus the innermost collection item; `/item` paths read the item.
#[derive(Clone, Copy)]
struct Env<'a> {
    root: &'a Value,
    item: Option<&'a Value>,
}
impl<'a> Env<'a> {
    fn get(self, path: &str) -> Option<&'a Value> {
        match (self.item, path.strip_prefix("/item")) {
            (Some(item), Some("")) => Some(item),
            (Some(item), Some(rest)) if rest.starts_with('/') => item.pointer(rest),
            _ => self.root.pointer(path),
        }
    }
    fn bind<'b>(self, item: &'b Value) -> Env<'b>
    where
        'a: 'b,
    {
        Env {
            root: self.root,
            item: Some(item),
        }
    }
}
/// Serialized length and nesting of a value, composed bottom-up so each output
/// byte is measured once rather than once per enclosing expression.
#[derive(Clone, Copy, Default)]
struct Size {
    bytes: usize,
    height: usize,
    items: usize,
}
impl Size {
    fn container() -> Self {
        Self {
            bytes: 2,
            ..Self::default()
        }
    }
    fn push(&mut self, key: Option<&str>, child: Size) -> Result<()> {
        self.bytes +=
            child.bytes + key.map_or(0, |k| json_len(k) + 1) + usize::from(self.items > 0);
        self.height = self.height.max(child.height + 1);
        self.items += 1;
        self.check()?;
        Ok(())
    }
    fn check(self) -> Result<Self> {
        if self.items > MAX_ITEMS || self.height > MAX_DEPTH || self.bytes > MAX_VALUE {
            return Err(limit());
        }
        Ok(self)
    }
}
struct Count(usize);
impl std::io::Write for Count {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0 += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn json_len<T: Serialize + ?Sized>(v: &T) -> usize {
    let mut c = Count(0);
    let _ = serde_json::to_writer(&mut c, v);
    c.0
}
fn measure(v: &Value) -> Result<Size> {
    let mut s = Size::container();
    match v {
        Value::Array(a) => {
            for v in a {
                s.push(None, measure(v)?)?;
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                s.push(Some(k), measure(v)?)?;
            }
        }
        v => s.bytes = json_len(v),
    }
    s.check()
}
fn measured(v: Value) -> Result<(Value, Size)> {
    let s = measure(&v)?;
    Ok((v, s))
}
fn truth(v: Value, op: &str) -> Result<bool> {
    v.as_bool().ok_or_else(|| ty(op))
}
fn uint(v: Value, op: &str) -> Result<u64> {
    v.as_u64().ok_or_else(|| ty(op))
}
/// `X.Y.Z[-pre][+build]`; returns the numeric core and whether it is a pre-release.
fn version(text: &str) -> Option<([u64; 3], bool)> {
    let (text, build) = text.split_once('+').unwrap_or((text, "-"));
    let (core, pre) = text
        .split_once('-')
        .map_or((text, None), |(c, p)| (c, Some(p)));
    if build.is_empty() || pre == Some("") {
        return None;
    }
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some((v, pre.is_some()))
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
            return Err(bad("expression tree exceeds 4096 nodes or depth 32"));
        }
        if let Self::Ref { path, .. } = self
            && (!path.starts_with('/') || path.len() > 1024)
        {
            return Err(bad("ref path must start with / and be at most 1024 bytes"));
        }
        for child in self.children() {
            child.check(depth + 1, nodes)?;
        }
        Ok(())
    }
    pub fn evaluate(&self, root: &Value) -> Result<Value> {
        measure(root)?;
        let mut fuel = FUEL;
        Ok(self.eval(Env { root, item: None }, &mut fuel, 0)?.0)
    }
    /// Costs one unit per visited node plus one per 64 output bytes.
    fn eval(&self, env: Env, fuel: &mut usize, depth: usize) -> Result<(Value, Size)> {
        if *fuel == 0 || depth > MAX_DEPTH {
            return Err(limit());
        }
        *fuel -= 1;
        let mut run = |e: &Expr| e.eval(env, fuel, depth + 1);
        let (v, size) = match self {
            Self::Literal { value } => measured(value.clone())?,
            Self::Ref { path, default } => {
                measured(env.get(path).or(default.as_ref()).cloned().ok_or_else(|| {
                    fail(format_args!("ref {} has no value", crate::display(path)))
                })?)?
            }
            Self::Object { fields } => {
                let (mut out, mut s) = (Map::new(), Size::container());
                for (k, e) in fields {
                    let (v, c) = run(e)?;
                    s.push(Some(k), c)?;
                    out.insert(k.clone(), v);
                }
                (Value::Object(out), s)
            }
            Self::Array { items } => {
                let (mut out, mut s) = (Vec::with_capacity(items.len()), Size::container());
                for e in items {
                    let (v, c) = run(e)?;
                    s.push(None, c)?;
                    out.push(v);
                }
                (Value::Array(out), s)
            }
            Self::Entries { input } => {
                let Value::Object(m) = run(input)?.0 else {
                    return Err(ty("entries"));
                };
                measured(Value::Array(
                    m.into_iter()
                        .map(|(k, v)| {
                            Value::Object(Map::from_iter([
                                ("key".into(), Value::String(k)),
                                ("value".into(), v),
                            ]))
                        })
                        .collect(),
                ))?
            }
            Self::Map { input, body } => {
                let Value::Array(a) = run(input)?.0 else {
                    return Err(ty("map"));
                };
                let (mut out, mut s) = (Vec::with_capacity(a.len()), Size::container());
                for item in &a {
                    let (v, c) = body.eval(env.bind(item), fuel, depth + 1)?;
                    s.push(None, c)?;
                    out.push(v);
                }
                (Value::Array(out), s)
            }
            Self::Filter { input, condition } => {
                let Value::Array(a) = run(input)?.0 else {
                    return Err(ty("filter"));
                };
                let (mut out, mut s) = (Vec::new(), Size::container());
                for item in a {
                    if truth(
                        condition.eval(env.bind(&item), fuel, depth + 1)?.0,
                        "filter",
                    )? {
                        s.push(None, measure(&item)?)?;
                        out.push(item);
                    }
                }
                (Value::Array(out), s)
            }
            Self::VersionAtLeast { input, minimum } => {
                let (v, pre) = run(input)?
                    .0
                    .as_str()
                    .and_then(version)
                    .ok_or_else(|| ty("version_at_least"))?;
                measured(json!(v > *minimum || v == *minimum && !pre))?
            }
            Self::Any { items } => {
                let mut yes = false;
                for e in items {
                    yes |= truth(run(e)?.0, "any")?;
                }
                measured(json!(yes))?
            }
            Self::MapValues { input, body } => {
                let Value::Object(m) = run(input)?.0 else {
                    return Err(ty("map_values"));
                };
                let (mut out, mut s) = (Map::new(), Size::container());
                for (k, item) in &m {
                    let (v, c) = body.eval(env.bind(item), fuel, depth + 1)?;
                    s.push(Some(k), c)?;
                    out.insert(k.clone(), v);
                }
                (Value::Object(out), s)
            }
            Self::Index { input, key, value } => {
                let Value::Array(a) = run(input)?.0 else {
                    return Err(ty("index"));
                };
                let (mut out, mut s) = (Map::new(), Size::container());
                for item in &a {
                    let env = env.bind(item);
                    let Value::String(k) = key.eval(env, fuel, depth + 1)?.0 else {
                        return Err(ty("index"));
                    };
                    let (v, c) = value.eval(env, fuel, depth + 1)?;
                    s.push(Some(&k), c)?;
                    if out.insert(k, v).is_some() {
                        return Err(fail("index produced a duplicate key"));
                    }
                }
                (Value::Object(out), s)
            }
            Self::Stringify { input } => measured(Value::String(run(input)?.0.to_string()))?,
            Self::Text { input } => measured(match run(input)?.0 {
                Value::String(s) => Value::String(s),
                v => Value::String(v.to_string()),
            })?,
            Self::If {
                condition,
                then,
                otherwise,
            } => {
                if truth(run(condition)?.0, "if")? {
                    run(then)?
                } else {
                    run(otherwise)?
                }
            }
            Self::Eq { left, right } => measured(Value::Bool(run(left)?.0 == run(right)?.0))?,
            Self::Le { left, right } => measured(Value::Bool(
                uint(run(left)?.0, "le")? <= uint(run(right)?.0, "le")?,
            ))?,
            Self::Add { left, right } => measured(json!(
                uint(run(left)?.0, "add")?
                    .checked_add(uint(run(right)?.0, "add")?)
                    .ok_or_else(|| fail("add overflowed"))?
            ))?,
            Self::Length { input } => measured(json!(match &run(input)?.0 {
                Value::Array(a) => a.len(),
                Value::Object(a) => a.len(),
                Value::String(a) => a.len(),
                _ => return Err(ty("length")),
            }))?,
            Self::All { items } => {
                let mut yes = true;
                for e in items {
                    yes &= truth(run(e)?.0, "all")?;
                }
                measured(json!(yes))?
            }
            Self::Not { input } => measured(json!(!truth(run(input)?.0, "not")?))?,
        };
        *fuel = fuel
            .checked_sub(size.bytes.div_ceil(64))
            .ok_or_else(limit)?;
        Ok((v, size))
    }
}
/// Deliberately small shape language, with strict objects by default.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub properties: BTreeMap<String, Shape>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub required: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub extra: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub items: Option<Box<Shape>>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub values: Option<Box<Shape>>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub choices: Vec<Value>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub any: Vec<Shape>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub min: Option<usize>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub max: Option<usize>,
}
impl Shape {
    fn definition_check(&self, depth: usize) -> Result<()> {
        let kind = self.kind.as_str();
        let object_only = !self.properties.is_empty()
            || !self.required.is_empty()
            || self.values.is_some()
            || self.extra;
        let bare = kind.is_empty()
            && !object_only
            && self.items.is_none()
            && self.choices.is_empty()
            && self.min.is_none()
            && self.max.is_none();
        if let Some((_, reason)) = [
            (depth > MAX_DEPTH, "shape nesting exceeds depth 32"),
            (
                ![
                    "", "object", "array", "string", "uint", "number", "boolean", "null",
                ]
                .contains(&kind),
                "shape kind must be object, array, string, uint, number, boolean or null",
            ),
            (
                !self.any.is_empty() && !bare,
                "shape any cannot be combined with other keywords",
            ),
            (
                (self.min.is_some() || self.max.is_some())
                    && !["string", "array", "object"].contains(&kind),
                "shape min/max require kind string, array or object",
            ),
            (
                self.min.zip(self.max).is_some_and(|(a, b)| a > b),
                "shape min exceeds max",
            ),
            (
                object_only && kind != "object",
                "shape properties, required, values and extra require kind object",
            ),
            (
                self.extra && self.values.is_some(),
                "shape extra cannot be combined with values",
            ),
            (
                self.required
                    .iter()
                    .any(|k| !self.properties.contains_key(k)),
                "shape required names must be declared in properties",
            ),
            (
                self.items.is_some() && kind != "array",
                "shape items requires kind array",
            ),
        ]
        .into_iter()
        .find(|c| c.0)
        {
            return Err(bad(reason));
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
    fn matches(&self, v: &Value) -> bool {
        if !self.any.is_empty() {
            return self.any.iter().any(|s| s.matches(v));
        }
        if !self.choices.is_empty() && !self.choices.contains(v) {
            return false;
        }
        let len = match (self.kind.as_str(), v) {
            ("", _) => None,
            ("object", Value::Object(a)) => {
                if self.required.iter().any(|k| !a.contains_key(k))
                    || !a.iter().all(|(k, v)| {
                        self.properties
                            .get(k)
                            .or(self.values.as_deref())
                            .map_or(self.extra, |s| s.matches(v))
                    })
                {
                    return false;
                }
                Some(a.len())
            }
            ("array", Value::Array(a)) => {
                if !self
                    .items
                    .as_ref()
                    .is_none_or(|s| a.iter().all(|v| s.matches(v)))
                {
                    return false;
                }
                Some(a.len())
            }
            ("string", Value::String(s)) => Some(s.len()),
            ("uint", v) if v.is_u64() => None,
            ("number", Value::Number(_)) | ("boolean", Value::Bool(_)) | ("null", Value::Null) => {
                None
            }
            _ => return false,
        };
        len.is_none_or(|n| self.min.is_none_or(|m| n >= m) && self.max.is_none_or(|m| n <= m))
    }
    pub fn validate(&self, v: &Value) -> Result<()> {
        if self.matches(v) {
            Ok(())
        } else {
            Err(fail("value does not match its declared shape"))
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub request: Expr,
    pub response: Expr,
    pub questions: Expr,
    #[serde(default, skip_serializing_if = "is_default")]
    pub request_shape: Shape,
    #[serde(default, skip_serializing_if = "is_default")]
    pub response_shape: Shape,
    #[serde(default, skip_serializing_if = "is_default")]
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
        let root = Value::Object(Map::from_iter([
            ("task".into(), task),
            ("model".into(), model.into()),
        ]));
        let v = self.request.evaluate(&root)?;
        if !self.request_shape.matches(&v) {
            return Err(fail("request does not match request_shape"));
        }
        if v["model"] != model {
            return Err(fail("request model must equal the selected model"));
        }
        // The inverse mapping must retain every question and every sent choice.
        let lost = || fail("questions mapping must recover every question and choice");
        let recovered = self.questions(&v)?;
        let expected = root["task"]["questions"]
            .as_object()
            .ok_or_else(|| fail("task questions must be an object"))?;
        let actual = recovered.as_object().ok_or_else(lost)?;
        if expected.len() != actual.len() {
            return Err(lost());
        }
        for (name, q) in expected {
            let a = actual
                .get(name)
                .and_then(|q| q["criteria"].as_object())
                .ok_or_else(lost)?;
            let b = q["criteria"]
                .as_object()
                .ok_or_else(|| fail("task criteria must be an object"))?;
            if a.len() != b.len() || !b.keys().all(|k| a.contains_key(k)) || !b.contains_key("none")
            {
                return Err(lost());
            }
        }
        serde_json::to_vec(&v).map_err(|_| fail("request cannot be encoded"))
    }
    pub fn decode(&self, response: Value) -> Result<Value> {
        let root = Value::Object(Map::from_iter([("response".into(), response)]));
        measure(&root)?;
        if !self.response_shape.matches(&root["response"]) {
            return Err(fail("response does not match response_shape"));
        }
        let (env, mut fuel) = (
            Env {
                root: &root,
                item: None,
            },
            FUEL,
        );
        for check in &self.response_checks {
            if check.eval(env, &mut fuel, 0)?.0 != true {
                return Err(fail("response check failed"));
            }
        }
        Ok(self.response.eval(env, &mut fuel, 0)?.0)
    }
    pub fn questions(&self, request: &Value) -> Result<Value> {
        if !self.request_shape.matches(request) {
            return Err(fail("request does not match request_shape"));
        }
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
                Transport::Https
            },
        })
    }
    fn prepare(&self, task: Value, model: &str) -> Result<Vec<u8>> {
        if model != self.model {
            return Err(fail("request model must equal the selected model"));
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
            return Err(fail("probe received more responses than steps"));
        }
        let mut fuel = FUEL;
        for (i, response) in responses.iter().enumerate() {
            let step = &p.steps[i];
            if !step.checks.is_empty() {
                let root = json!({"response":response,"responses":responses,"model":self.model});
                measure(&root)?;
                for check in &step.checks {
                    if check
                        .eval(
                            Env {
                                root: &root,
                                item: None,
                            },
                            &mut fuel,
                            0,
                        )?
                        .0
                        != true
                    {
                        return Err(fail("probe check failed"));
                    }
                }
            }
            if let Some(choice) = &step.expect_choice {
                let task = step
                    .task
                    .clone()
                    .ok_or_else(|| fail("probe expect_choice requires task"))?;
                let request = self.prepare(task, &self.model)?;
                let response = serde_json::to_vec(response)
                    .map_err(|_| fail("probe response cannot be encoded"))?;
                if crate::provider::validate_for(cfg, &response, &request)?.as_ref() != Some(choice)
                {
                    return Err(fail("probe choice differs from expect_choice"));
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
            return Err(fail("probe report must be an object"));
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
    fn full_task() -> Value {
        let mut criteria: Map<String, Value> = (0..254)
            .map(|i| {
                (
                    format!("c{i}"),
                    json!({"name":format!("synthetic-directory-name-{i:04}"),"parent":format!("/synthetic/projects/workspace/parent-directory-{i:04}/x"),"usage":"low"}),
                )
            })
            .collect();
        criteria.insert("none".into(), json!("No match"));
        json!({"state":{"query":"backend","cwd":"src"},"questions":{"destination":{"type":"choice","instructions":"Choose a match.","criteria":criteria}}})
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
            json!({"op":"eq","left":{"op":"length","input":{"op":"map","input":{"op":"ref","path":"/response/metadata"},"body":{"op":"literal","value":0}}},"right":{"op":"literal","value":1000}}),
        );
        w.response_checks = vec![check.clone(); 100];
        w.check().unwrap();
        let response = json!({"metadata":vec!["x".repeat(128);1000]});
        assert_eq!(check.evaluate(&json!({"response":response})).unwrap(), true);
        w.response_checks.truncate(10);
        w.decode(response.clone()).unwrap();
        w.response_checks = vec![check; 100];
        assert!(is_resource_limit(&w.decode(response).unwrap_err()));
    }
    #[test]
    fn evaluation_cost_tracks_output_not_phase_root() {
        // Binding an item no longer copies the phase root, so unrelated input
        // size does not multiply per-element cost.
        let tiny = expr(
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"literal","value":null}}),
        );
        tiny.evaluate(&json!({"a":vec!["x".repeat(128);1024]}))
            .unwrap();
        let small = expr(
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"ref","path":"/item"}}),
        );
        assert_eq!(
            small
                .evaluate(&json!({"a":[1,2,3],"big":vec!["x".repeat(200);1000]}))
                .unwrap(),
            json!([1, 2, 3])
        );
        // Copies and per-element work are still charged, so amplification is bounded.
        for e in [
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"length","input":{"op":"ref","path":"/a"}}}),
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"literal","value":null}}}),
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"filter","input":{"op":"ref","path":"/a"},"condition":{"op":"literal","value":false}}}),
            json!({"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"index","input":{"op":"ref","path":"/a"},"key":{"op":"text","input":{"op":"ref","path":"/item"}},"value":{"op":"literal","value":null}}}),
        ] {
            let a: Vec<_> = (0..1024).map(|i| format!("{i:0128}")).collect();
            let err = expr(e).evaluate(&json!({ "a": a })).unwrap_err();
            assert!(is_resource_limit(&err), "{}", err.1);
        }
    }
    #[test]
    fn composed_sizes_match_serialization() {
        let v = json!({"a\"\n":["é\u{1}",1.5,-3,u64::MAX,null,true,{}],"b":[[],{"k":"\u{2028}"}]});
        assert_eq!(
            measure(&v).unwrap().bytes,
            serde_json::to_vec(&v).unwrap().len()
        );
        let e = expr(
            json!({"op":"object","fields":{"x\"":{"op":"map","input":{"op":"ref","path":"/a"},"body":{"op":"array","items":[{"op":"ref","path":"/item"},{"op":"literal","value":"\t"}]}},"y":{"op":"index","input":{"op":"ref","path":"/a"},"key":{"op":"text","input":{"op":"ref","path":"/item"}},"value":{"op":"stringify","input":{"op":"ref","path":"/item"}}}}}),
        );
        let root = json!({"a":["p","q\"",3]});
        let (v, size) = e
            .eval(
                Env {
                    root: &root,
                    item: None,
                },
                &mut FUEL.clone(),
                0,
            )
            .unwrap();
        assert_eq!(size.bytes, serde_json::to_vec(&v).unwrap().len());
        assert_eq!(size.height, 3);
    }
    #[test]
    fn item_binding_is_innermost_and_other_paths_read_the_root() {
        let e = expr(
            json!({"op":"map","input":{"op":"ref","path":"/rows"},"body":{"op":"map","input":{"op":"ref","path":"/item/cells"},"body":{"op":"array","items":[{"op":"ref","path":"/item"},{"op":"ref","path":"/model"},{"op":"ref","path":"/itemx"}]}}}),
        );
        assert_eq!(
            e.evaluate(&json!({"rows":[{"cells":[1,2]},{"cells":[3]}],"model":"m","itemx":0}))
                .unwrap(),
            json!([[[1, "m", 0], [2, "m", 0]], [[3, "m", 0]]])
        );
        let outside = expr(json!({"op":"ref","path":"/item"}))
            .evaluate(&json!({"task":{}}))
            .unwrap_err();
        assert_eq!(outside.0, 5);
        assert!(!is_resource_limit(&outside));
    }
    #[test]
    fn shipped_openai_wire_prepares_full_candidate_set() {
        for id in ["openai", "jev", "clef", "tev1"] {
            let b = super::super::wire(id)
                .prepare(full_task(), "synthetic-model")
                .unwrap();
            let q = super::super::wire(id)
                .questions(&serde_json::from_slice(&b).unwrap())
                .unwrap();
            assert_eq!(
                q["destination"]["criteria"].as_object().unwrap().len(),
                255,
                "{id}"
            );
        }
    }
    fn https() -> Definition {
        Definition {
            transport: "https".into(),
            url: "https://example.invalid/decision".into(),
            auth: Auth::Bearer {
                env: "TEST_API_KEY".into(),
            },
            ..definition()
        }
    }
    fn reason(d: &Definition) -> String {
        let e = d.validate().unwrap_err();
        assert_eq!(e.0, 2, "{}", e.1);
        e.1.strip_prefix("invalid provider definition: ")
            .unwrap()
            .to_owned()
    }
    #[test]
    fn definition_failures_are_specific_usage_errors() {
        let v = serde_json::to_value(definition()).unwrap();
        let parse = |v: &Value| Definition::parse(&serde_json::to_vec(v).unwrap()).unwrap_err();
        for (bytes, want) in [
            (b"{".to_vec(), "not strict JSON or duplicate key"),
            (
                br#"{"version":1,"version":1}"#.to_vec(),
                "not strict JSON or duplicate key",
            ),
            (vec![b' '; 32769], "definition exceeds 32 KiB"),
        ] {
            let e = Definition::parse(&bytes).unwrap_err();
            assert_eq!(
                (e.0, e.1.as_ref()),
                (2, format!("invalid provider definition: {want}").as_str())
            );
        }
        for (k, x, want) in [
            ("version", json!(2), "unsupported version"),
            ("unknown", json!(true), "unknown field `unknown`"),
            (
                "id",
                json!("../jev"),
                "id must be 1-64 lowercase letters, digits or -",
            ),
            ("model", json!("bad\nmodel"), "model must be 1-256 bytes"),
            (
                "transport",
                json!("shell"),
                "url must be https, or http to 127.0.0.1/[::1]",
            ),
            (
                "url",
                json!("file:///tmp/test"),
                "url must be absolute with a host",
            ),
            ("limits", json!({"candidates":0}), "limits out of range"),
            (
                "response_models",
                json!(vec!["m"; 17]),
                "response_models must be at most 16",
            ),
        ] {
            let mut v = v.clone();
            v[k] = x;
            let e = parse(&v);
            assert_eq!(e.0, 2, "{k}");
            assert!(
                e.1.starts_with(&format!("invalid provider definition: {want}")),
                "{k}: {}",
                e.1
            );
        }
        let mut w = v.clone();
        w["wire"]["request"] = json!({"op":"shell","command":"whoami"});
        assert!(parse(&w).1.contains("unknown variant `shell`"));
        w["wire"]["request"] = json!({"op":"ref","path":"task"});
        assert!(
            parse(&w)
                .1
                .ends_with("ref path must start with / and be at most 1024 bytes")
        );
        let mut deep = json!({"op":"literal","value":true});
        for _ in 0..33 {
            deep = json!({"op":"not","input":deep});
        }
        w["wire"]["request"] = deep;
        assert!(
            parse(&w)
                .1
                .ends_with("expression tree exceeds 4096 nodes or depth 32")
        );
        w["wire"]["request"] = v["wire"]["request"].clone();
        w["wire"]["request_shape"] = json!({"kind":"text"});
        assert!(parse(&w).1.contains("shape kind must be"));
    }
    #[test]
    fn runtime_failures_are_value_free_and_limits_are_recognizable() {
        let secret = "SYNTHETIC-SECRET-VALUE";
        let root = json!({"s":secret,"n":{"k":secret}});
        for e in [
            json!({"op":"not","input":{"op":"ref","path":"/s"}}),
            json!({"op":"map","input":{"op":"ref","path":"/n"},"body":{"op":"literal","value":0}}),
            json!({"op":"ref","path":"/missing"}),
            json!({"op":"version_at_least","input":{"op":"ref","path":"/s"},"minimum":[1,0,0]}),
            json!({"op":"index","input":{"op":"array","items":[{"op":"ref","path":"/s"},{"op":"ref","path":"/s"}]},"key":{"op":"ref","path":"/item"},"value":{"op":"literal","value":0}}),
            json!({"op":"add","left":{"op":"literal","value":u64::MAX},"right":{"op":"literal","value":1}}),
        ] {
            let err = expr(e).evaluate(&root).unwrap_err();
            assert_eq!(err.0, 5);
            assert!(err.1.starts_with("provider DSL "), "{}", err.1);
            assert!(
                !err.1.contains(secret) && !is_resource_limit(&err),
                "{}",
                err.1
            );
        }
        let w = super::super::wire("openai");
        let err = w.decode(json!({"model":secret})).unwrap_err();
        assert_eq!(err.1, "provider DSL response does not match response_shape");
        let mut deep = json!(0);
        for _ in 0..32 {
            deep = json!([deep]);
        }
        let mut fuel = Expr::Literal { value: Value::Null };
        for _ in 0..4 {
            fuel = Expr::Map {
                input: Box::new(Expr::Literal {
                    value: json!(vec![0; 1024]),
                }),
                body: Box::new(fuel),
            };
        }
        for (e, root) in [
            (
                Expr::Literal {
                    value: json!(vec![0; 1025]),
                },
                json!({}),
            ),
            (
                Expr::Literal {
                    value: json!("x".repeat(262145)),
                },
                json!({}),
            ),
            (
                Expr::Array {
                    items: vec![Expr::Literal {
                        value: deep.clone(),
                    }],
                },
                json!({}),
            ),
            (expr(json!({"op":"ref","path":"/d"})), json!({"d":[deep]})),
            (fuel, json!({})),
        ] {
            let err = e.evaluate(&root).unwrap_err();
            assert_eq!((err.0, err.1.as_ref()), (5, LIMIT));
            assert!(is_resource_limit(&err));
        }
        assert!(!is_resource_limit(&Error(
            5,
            "provider response exceeds limit".into()
        )));
        assert!(!is_resource_limit(&Error(2, LIMIT.into())));
    }
    #[test]
    fn partial_limits_use_field_defaults() {
        let mut v = serde_json::to_value(https()).unwrap();
        v["limits"] = json!({"cache":true});
        let d = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(
            d.limits,
            Limits {
                cache: true,
                ..Limits::default()
            }
        );
        v["limits"] = json!({"candidates":23,"context_bytes":1600});
        let d = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!((d.limits.candidates, d.limits.body_bytes), (23, 65536));
        v["limits"] = json!({});
        assert_eq!(
            Definition::parse(&serde_json::to_vec(&v).unwrap())
                .unwrap()
                .limits,
            Limits::default()
        );
    }
    #[test]
    fn serialization_is_compact_and_roundtrips() {
        let text = serde_json::to_string(&definition()).unwrap();
        assert!(
            !text.contains("\"limits\"")
                && !text.contains("\"extra\"")
                && !text.contains("\"probe\"")
        );
        for text in [
            include_str!("../../examples/providers/openrouter-jev.json"),
            include_str!("../../examples/providers/vercel-openai.json"),
            include_str!("../../examples/providers/local-tev1.json"),
            include_str!("../../examples/providers/openrouter-microsoft.json"),
            include_str!("../../examples/providers/openrouter-openai.json"),
            include_str!("../../examples/providers/openrouter-clef.json"),
        ] {
            let d = Definition::parse(text.as_bytes()).unwrap();
            let minified =
                serde_json::to_vec(&serde_json::from_str::<Value>(text).unwrap()).unwrap();
            let b = serde_json::to_vec(&d).unwrap();
            assert!(
                b.len() <= minified.len(),
                "{} > {}",
                b.len(),
                minified.len()
            );
            assert_eq!(serde_json::from_slice::<Definition>(&b).unwrap(), d);
        }
        for text in [
            include_str!("definitions/jev.json"),
            include_str!("definitions/clef.json"),
            include_str!("definitions/openai.json"),
            include_str!("definitions/tev1.json"),
        ] {
            let w: Wire = serde_json::from_str(text).unwrap();
            let minified =
                serde_json::to_vec(&serde_json::from_str::<Value>(text).unwrap()).unwrap();
            let b = serde_json::to_vec(&w).unwrap();
            assert!(b.len() <= minified.len());
            assert_eq!(serde_json::from_slice::<Wire>(&b).unwrap(), w);
        }
        let mut d = definition();
        d.limits = Limits {
            candidates: 23,
            body_bytes: 4096,
            context_bytes: Some(1600),
            diverse: true,
            cache: false,
        };
        d.response_models = vec!["alias".into()];
        let b = serde_json::to_vec(&d).unwrap();
        assert_eq!(serde_json::from_slice::<Definition>(&b).unwrap(), d);
        // The size limit binds both the source bytes and the compact form.
        d.wire.response = Expr::Object {
            fields: (0..800)
                .map(|i| (format!("field{i}"), Expr::Literal { value: Value::Null }))
                .collect(),
        };
        assert_eq!(reason(&d), "definition exceeds 32 KiB");
        let mut padded = serde_json::to_vec_pretty(&definition()).unwrap();
        padded.resize(32769, b' ');
        assert_eq!(
            Definition::parse(&padded).unwrap_err().1,
            "invalid provider definition: definition exceeds 32 KiB"
        );
    }
    #[test]
    fn shape_keywords_must_apply_to_their_kind() {
        let check = |v: Value| {
            serde_json::from_value::<Shape>(v)
                .unwrap()
                .definition_check(0)
        };
        for v in [
            json!({"kind":"uint","min":1}),
            json!({"max":3}),
            json!({"kind":"boolean","min":0,"max":1}),
            json!({"kind":"array","properties":{"a":{}}}),
            json!({"kind":"string","required":["a"]}),
            json!({"kind":"array","values":{}}),
            json!({"extra":true}),
            json!({"kind":"object","items":{}}),
            json!({"kind":"string","items":{}}),
            json!({"kind":"object","values":{},"extra":true}),
            json!({"kind":"object","any":[{}]}),
            json!({"choices":[1],"any":[{}]}),
            json!({"min":1,"any":[{"kind":"string"}]}),
            json!({"any":[{"kind":"string","items":{}}]}),
            json!({"kind":"object","properties":{"a":{"kind":"null","max":1}}}),
        ] {
            let e = check(v.clone()).unwrap_err();
            assert_eq!(e.0, 2, "{v}");
            assert!(
                e.1.starts_with("invalid provider definition: shape "),
                "{v}"
            );
        }
        for v in [
            json!({}),
            json!({"kind":"string","min":1,"max":3,"choices":["ab"]}),
            json!({"kind":"array","min":1,"items":{"kind":"uint"}}),
            json!({"kind":"object","max":2,"required":["a"],"properties":{"a":{}},"values":{"kind":"null"}}),
            json!({"kind":"object","extra":true}),
            json!({"kind":"","any":[{"kind":"null"},{"kind":"array","max":0}]}),
            json!({"kind":"uint","choices":[1,2]}),
        ] {
            check(v.clone()).unwrap_or_else(|e| panic!("{v}: {}", e.1));
        }
    }
    #[test]
    fn probe_paths_reject_whitespace_controls_and_other_origins() {
        let mut d = definition();
        d.probe = Some(Probe {
            steps: vec![ProbeStep {
                path: "/api/version".into(),
                task: None,
                expect_choice: None,
                checks: vec![],
            }],
            report: Expr::Literal { value: json!({}) },
        });
        d.validate().unwrap();
        for path in [
            "/\t/remote.invalid",
            "/\n/remote.invalid",
            "/\r/remote.invalid",
            "/api version",
            "/api\u{7f}",
            "/api\u{0}",
            "/api\u{85}",
            "/api\u{3000}",
            "api/version",
        ] {
            d.probe.as_mut().unwrap().steps[0].path = path.into();
            assert!(
                reason(&d).starts_with("probe step path must be a same-origin"),
                "{path:?}"
            );
        }
        d.probe.as_mut().unwrap().steps[0].path = "/api/%20tags".into();
        d.validate().unwrap();
    }
    #[test]
    fn auth_header_rejects_framing_hop_by_hop_and_long_names() {
        let mut d = https();
        for name in [
            "te",
            "Trailer",
            "upgrade",
            "keep-alive",
            "proxy-connection",
            "proxy-authenticate",
            "expect",
            "host",
            "content-type",
        ] {
            d.auth = Auth::Header {
                env: "TEST_API_KEY".into(),
                name: name.into(),
            };
            assert_eq!(
                reason(&d),
                "auth header name is reserved for transport",
                "{name}"
            );
        }
        for (name, ok) in [
            ("x".repeat(64), true),
            ("x".repeat(65), false),
            ("bad header".into(), false),
        ] {
            d.auth = Auth::Header {
                env: "TEST_API_KEY".into(),
                name,
            };
            assert_eq!(d.validate().is_ok(), ok);
        }
    }
    #[test]
    fn version_prereleases_rank_below_their_release() {
        let at_least = |v: &str| {
            expr(json!({"op":"version_at_least","input":{"op":"literal","value":v},"minimum":[0,35,0]}))
                .evaluate(&json!({}))
                .map(|v| v == true)
        };
        for (v, want) in [
            ("0.35.0", true),
            ("0.35.0+build.7", true),
            ("0.35.0-rc1", false),
            ("0.35.0-rc.1+build", false),
            ("0.35.1-rc1", true),
            ("0.36.0-rc1", true),
            ("1.0.0", true),
            ("0.34.99", false),
            ("0.34.9-rc1", false),
        ] {
            assert_eq!(at_least(v).unwrap(), want, "{v}");
        }
        for v in [
            "0.35", "0.35.0.1", "0.35.0-", "0.35.0+", "v0.35.0", "garbage", "",
        ] {
            assert!(at_least(v).is_err(), "{v}");
        }
    }
    #[test]
    fn builtin_credential_variables_are_reserved() {
        let reserved: Vec<&str> = DRIVERS
            .iter()
            .filter_map(|d| d.descriptor().credential.as_ref())
            .flat_map(|c| c.env.iter().copied())
            .collect();
        for env in [
            "TYPESAFE_API_KEY",
            "OPENAI_API_KEY",
            "CLOUDFLARE_AUTH_TOKEN",
            "CLOUDFLARE_API_TOKEN",
        ] {
            assert!(reserved.contains(&env));
        }
        let mut d = https();
        for env in reserved {
            for auth in [
                Auth::Bearer { env: env.into() },
                Auth::Header {
                    env: env.into(),
                    name: "x-api-key".into(),
                },
            ] {
                d.auth = auth;
                assert_eq!(
                    reason(&d),
                    "credential variable reserved for built-in provider"
                );
            }
        }
        d.auth = Auth::Bearer {
            env: "OPENROUTER_API_KEY".into(),
        };
        d.validate().unwrap();
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
