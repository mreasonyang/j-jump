//! Source format assertions checked before normalization.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub(super) fn definition_check(&self, depth: usize) -> Result<()> {
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
    pub(super) fn matches(&self, v: &Value) -> bool {
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
