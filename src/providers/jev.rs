use super::*;
pub struct Jev;
pub static JEV: Jev = Jev;
static DESCRIPTION: Descriptor = Descriptor {
    id: "jev",
    label: "Jev",
    summary: Text(
        "official Jev API; needs a Jev key.",
        "官方 Jev API，需要 Jev Key。",
    ),
    missing: Text(
        "Jev skipped: no key configured. Add one with jjump setup or TYPESAFE_API_KEY.",
        "已跳过 Jev：尚未配置 Key。可用 jjump setup 或 TYPESAFE_API_KEY 添加。",
    ),
    fields: &[],
    credential: Some(CredentialSpec {
        env: &["TYPESAFE_API_KEY"],
        service: "j-jump.jev",
        account: "typesafe-api-key",
        source: |c| &c.credential,
        set_source: |c, s| c.credential = s.into(),
    }),
};
impl ProviderDriver for Jev {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &'static str {
        "jev-1.13.0"
    }
    fn connection(&self, _: &Config, context: Option<&str>) -> Result<Connection> {
        if context.is_some_and(|s| !s.is_empty()) {
            return Err(Error(5, "invalid provider binding".into()));
        }
        Ok(Connection {
            url: "https://api.typesafe.ai/v1/systemone".into(),
            context: String::new(),
            transport: Transport::OfficialHttps,
        })
    }
}
