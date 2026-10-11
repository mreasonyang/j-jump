//! A bounded, pure JSON interpreter. Only transport code can resolve credentials.
use super::*;
use serde_json::json;

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

mod definition;
mod expr;
mod shape;
mod wire;
pub use definition::{Auth, Definition, Limits, Probe, ProbeStep};
pub use expr::Expr;
use expr::{Env, measure};
pub use shape::Shape;
pub use wire::Wire;

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
mod tests;
