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
    fn wire(&self) -> Option<&'static super::BuiltinWire> {
        Some(&super::BuiltinWire {
            id: "openai",
            request_error: "OpenAI Decisions schema mismatch",
            response_error: "OpenAI Decisions schema mismatch",
        })
    }
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
            transport: Transport::Https,
        })
    }
}
