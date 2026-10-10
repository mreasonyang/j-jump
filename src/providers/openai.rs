//! Official Decisions API wire format, normalized into the shared choice contract.
use super::*;

pub struct OpenAI;
pub static OPENAI: OpenAI = OpenAI;
const MODEL: &str = "gpt-6-luna";
fn source(cfg: &Config) -> &str {
    cfg.providers
        .get("openai")
        .and_then(|s| s.get("credential"))
        .map(String::as_str)
        .unwrap_or("environment")
}
static DESCRIPTION: Descriptor = Descriptor {
    id: "openai",
    label: "OpenAI",
    summary: Text(
        "OpenAI Decisions API (gpt-6-luna); needs an OpenAI API key.",
        "OpenAI Decisions API（gpt-6-luna），需要 OpenAI API Key。",
    ),
    missing: Text(
        "OpenAI skipped: no key configured. Add one with jjump setup or OPENAI_API_KEY.",
        "已跳过 OpenAI：尚未配置 Key。可用 jjump setup 或 OPENAI_API_KEY 添加。",
    ),
    fields: &[],
    credential: Some(CredentialSpec {
        env: &["OPENAI_API_KEY"],
        service: "j-jump.openai",
        account: "openai-api-key",
        setting: Some("credential"),
        source,
        set_source: |cfg, value| {
            cfg.providers
                .entry("openai".into())
                .or_default()
                .insert("credential".into(), value.into());
        },
    }),
};
impl ProviderDriver for OpenAI {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &'static str {
        MODEL
    }
    fn connection(&self, _: &Config, context: Option<&str>) -> Result<Connection> {
        if context.is_some_and(|s| !s.is_empty()) {
            return Err(Error(5, "invalid provider binding".into()));
        }
        Ok(Connection {
            url: "https://api.openai.com/v1/decisions".into(),
            context: String::new(),
            transport: Transport::OfficialHttps,
        })
    }
    fn prepare(&self, task: Value, model: &str) -> Result<Vec<u8>> {
        if !self.accepts_model(model) {
            return Err(Error(5, "unsupported request model".into()));
        }
        super::wire("openai").prepare(task, model)
    }
    fn questions(&self, request: &Value) -> Result<Value> {
        super::wire("openai").questions(request)
    }
    fn decode(&self, value: Value) -> Result<Value> {
        super::wire("openai").decode(value)
    }
}
