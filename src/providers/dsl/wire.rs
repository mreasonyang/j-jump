//! Request construction, response normalization and question recovery.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};

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
