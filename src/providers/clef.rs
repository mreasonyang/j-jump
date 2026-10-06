use super::*;
pub struct Clef;
pub static CLEF: Clef = Clef;
pub fn valid_account(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
static DESCRIPTION: Descriptor = Descriptor {
    id: "clef-flash",
    label: "Clef-Flash",
    summary: Text(
        "Cloudflare Workers AI; needs an Account ID and API token.",
        "Cloudflare Workers AI，需要 Account ID 和 API Token。",
    ),
    missing: Text(
        "Clef-Flash skipped: configure a Cloudflare Account ID and API token in jjump setup, or CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_AUTH_TOKEN.",
        "已跳过 Clef-Flash：请在 jjump setup 中配置 Cloudflare Account ID 和 API Token，或设置 CLOUDFLARE_ACCOUNT_ID、CLOUDFLARE_AUTH_TOKEN。",
    ),
    fields: &[Field {
        key: "cloudflare_account_id",
        discover: false,
        nested: false,
        label: Text("Cloudflare Account ID", "Cloudflare Account ID"),
        help: Text(
            "32 hexadecimal characters; CLOUDFLARE_ACCOUNT_ID overrides the saved ID. Enter keeps the current value, including an empty value for later setup.",
            "32 位十六进制字符；CLOUDFLARE_ACCOUNT_ID 优先于保存的 ID。Enter 保留当前值，也可暂留空值稍后配置。",
        ),
        default: "",
        get: |c| c.cloudflare_account_id.clone(),
        set: |c, s| {
            if !s.is_empty() && !valid_account(s) {
                return Err(Error(
                    2,
                    "cloudflare_account_id expects a 32-character hexadecimal Account ID".into(),
                ));
            }
            c.cloudflare_account_id = s.into();
            Ok(())
        },
    }],
    credential: Some(CredentialSpec {
        env: &["CLOUDFLARE_AUTH_TOKEN", "CLOUDFLARE_API_TOKEN"],
        service: "j-jump.cloudflare",
        account: "workers-ai-api-token",
        source: |c| &c.cloudflare_credential,
        set_source: |c, s| c.cloudflare_credential = s.into(),
    }),
};
impl ProviderDriver for Clef {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &'static str {
        "clef-flash"
    }
    fn connection(&self, cfg: &Config, context: Option<&str>) -> Result<Connection> {
        let account = if let Some(s) = context {
            s.to_owned()
        } else {
            match std::env::var("CLOUDFLARE_ACCOUNT_ID") {
                Ok(s) if !s.is_empty() => s,
                Err(std::env::VarError::NotUnicode(_)) => {
                    return Err(Error(
                        5,
                        "Clef-Flash Cloudflare Account ID environment value is not valid UTF-8"
                            .into(),
                    ));
                }
                _ => cfg.cloudflare_account_id.clone(),
            }
        };
        if !valid_account(&account) {
            return Err(Error(5,"Clef-Flash requires a 32-character hexadecimal Cloudflare Account ID; use jjump setup or CLOUDFLARE_ACCOUNT_ID".into()));
        }
        Ok(Connection {
            url: format!(
                "https://api.cloudflare.com/client/v4/accounts/{account}/ai/run/@cf/cloudflare/clef-flash"
            ),
            context: account,
            transport: Transport::OfficialHttps,
        })
    }
    fn decode(&self, mut v: Value) -> Result<Value> {
        if !v.as_object().is_some_and(|m| {
            m.keys()
                .all(|k| ["result", "success", "errors", "messages"].contains(&k.as_str()))
        }) || v["success"] != true
            || !v["errors"].as_array().is_some_and(Vec::is_empty)
            || v.get("messages").is_some_and(|m| !m.is_array())
        {
            return Err(Error(
                5,
                "Clef-Flash returned an invalid or unsuccessful Cloudflare response".into(),
            ));
        }
        Ok(v["result"].take())
    }
    fn config_notice(&self, cfg: &Config, chinese: bool) -> Option<String> {
        if self.connection(cfg, None).is_ok() {
            return None;
        }
        let mut message = Text(
            "Clef-Flash blocked: Cloudflare Account ID missing or invalid.",
            "Clef-Flash 暂不可用：Cloudflare Account ID 缺失或无效。",
        )
        .get(chinese)
        .to_owned();
        if std::env::var_os("CLOUDFLARE_ACCOUNT_ID").is_some_and(|v| !v.is_empty()) {
            message.push(' ');
            message.push_str(Text("CLOUDFLARE_ACCOUNT_ID is invalid and overrides the saved Account ID. Correct it or unset it to use the saved account.","CLOUDFLARE_ACCOUNT_ID 无效，但仍覆盖已保存的 Account ID。请更正它，或清除它以使用已保存的账号。").get(chinese));
        }
        Some(message)
    }
}
