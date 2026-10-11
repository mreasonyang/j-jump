//! Expressions: a bounded, pure JSON evaluator with no ambient access.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use std::collections::BTreeMap;

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
pub(super) struct Env<'a> {
    pub(super) root: &'a Value,
    pub(super) item: Option<&'a Value>,
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
pub(super) struct Size {
    pub(super) bytes: usize,
    pub(super) height: usize,
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
pub(super) fn measure(v: &Value) -> Result<Size> {
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
    pub(super) fn check(&self, depth: usize, nodes: &mut usize) -> Result<()> {
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
    pub(super) fn eval(&self, env: Env, fuel: &mut usize, depth: usize) -> Result<(Value, Size)> {
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
