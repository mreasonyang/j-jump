use super::*;
pub struct Tev1;
pub static TEV1: Tev1 = Tev1;
const DEFAULT_URL: &str = "http://127.0.0.1:11434";
fn base(cfg: &Config) -> String {
    cfg.providers
        .get("tev1")
        .and_then(|m| m.get("ollama_url"))
        .cloned()
        .unwrap_or_else(|| DEFAULT_URL.into())
}
fn validate_url(value: &str) -> Result<String> {
    let u = reqwest::Url::parse(value)
        .map_err(|_| Error(2, "ollama_url expects a loopback HTTP origin".into()))?;
    let valid = u.scheme() == "http"
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none()
        && u.path() == "/"
        && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))
        && u.port_or_known_default().is_some_and(|p| p > 0);
    if !valid {
        return Err(Error(2,"ollama_url expects http://127.0.0.1:PORT or http://[::1]:PORT; no path, credentials, query or remote host".into()));
    }
    Ok(u.as_str().trim_end_matches('/').into())
}
static DESCRIPTION: Descriptor = Descriptor {
    id: "tev1",
    label: "Tev1 4B",
    summary: Text(
        "local Ollama >=0.35 with tev1:4b GGUF; no API key. Manage Ollama and model downloads yourself.",
        "本机 Ollama >=0.35 和 tev1:4b GGUF；无需 API Key。请自行启动 Ollama 并下载模型。",
    ),
    missing: Text(
        "Tev1 local configuration is invalid; use jjump setup.",
        "Tev1 本机配置无效；请运行 jjump setup。",
    ),
    fields: &[Field {
        key: "ollama_url",
        nested: true,
        label: Text("Ollama URL", "Ollama 地址"),
        help: Text(
            "Local loopback HTTP only; direct connection without proxies. Run jjump provider-check to test service and model explicitly.",
            "仅支持本机 loopback HTTP，直连且不经过代理。使用 jjump provider-check 显式检查服务和模型。",
        ),
        default: DEFAULT_URL,
        get: base,
        set: |c, s| {
            let v = validate_url(s)?;
            c.providers
                .entry("tev1".into())
                .or_default()
                .insert("ollama_url".into(), v);
            Ok(())
        },
    }],
    credential: None,
};
impl ProviderDriver for Tev1 {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &'static str {
        "tev1:4b"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_candidates: 23,
            context_bytes: Some(1600),
            diverse_shortlist: true,
            // A mutable Ollama tag does not bind an immutable weight digest. Do
            // not persist model answers across local model replacement.
            cacheable: false,
            ..CLOUD
        }
    }
    fn connection(&self, cfg: &Config, context: Option<&str>) -> Result<Connection> {
        let url = validate_url(&base(cfg))?;
        if context.is_some_and(|s| s != url) {
            return Err(Error(5, "local endpoint binding changed".into()));
        }
        Ok(Connection {
            url: format!("{url}/v1/systemone"),
            context: url,
            transport: Transport::Loopback,
        })
    }
    fn prepare(&self, mut task: Value) -> Result<Vec<u8>> {
        task["model"] = self.model().into();
        let criteria = task["questions"]["destination"]["criteria"]
            .as_object_mut()
            .ok_or(Error(5, "criteria missing".into()))?;
        for value in criteria.values_mut() {
            if !value.is_string() {
                *value = Value::String(value.to_string());
            }
        }
        task["keep_alive"] = "5m".into();
        serde_json::to_vec(&task).map_err(|_| Error(5, "cannot encode local task".into()))
    }
    fn diagnostic(&self, responses: &[Value]) -> Result<Diagnostic> {
        let probe = || {
            self.prepare(serde_json::json!({"state":"Select the option named ready.","questions":{"destination":{"type":"choice","instructions":"Select ready.","criteria":{"ready":"ready","none":"No match"}}}}))
        };
        if responses.is_empty() {
            return Ok(Diagnostic::Request {
                path: "/api/version",
                body: None,
            });
        }
        let v = responses[0]["version"]
            .as_str()
            .ok_or(Error(5, "local runtime version missing".into()))?;
        let parts: Vec<u64> = v
            .split('.')
            .take(3)
            .map(|x| x.split('-').next().unwrap_or("").parse().unwrap_or(0))
            .collect();
        if parts.len() != 3 || (parts[0], parts[1], parts[2]) < (0, 35, 0) {
            return Err(Error(5,"local runtime version is unsupported; update the runtime required by the selected driver".into()));
        }
        if responses.len() == 1 {
            return Ok(Diagnostic::Request {
                path: "/api/tags",
                body: None,
            });
        }
        let model = responses[1]["models"]
            .as_array()
            .and_then(|ms| {
                ms.iter()
                    .find(|m| m["name"] == self.model() || m["model"] == self.model())
            })
            .ok_or(Error(
                5,
                "local model is missing; install the selected model with your local runtime".into(),
            ))?;
        if model["details"]["format"] != "gguf" {
            return Err(Error(
                5,
                "local System One needs compatible GGUF weights; MLX is unsupported".into(),
            ));
        }
        if responses.len() == 2 {
            return Ok(Diagnostic::Request {
                path: "/v1/systemone",
                body: Some(probe()?),
            });
        }
        let body = serde_json::to_vec(&responses[2])
            .map_err(|_| Error(5, "invalid diagnostic response".into()))?;
        if crate::provider::validate(&body, &probe()?)?.as_deref() != Some("ready") {
            return Err(Error(
                5,
                "local model did not pass the synthetic decision check".into(),
            ));
        }
        Ok(Diagnostic::Complete(
            serde_json::json!({"configuration":"ok","service":"reachable","runtime_version":v,
            "model":self.model(),"model_digest":model["digest"],"model_format":"gguf","synthetic_request":"passed",
            "directory_quality":"not measured","credentials":"not used","transport":"loopback direct"}),
        ))
    }
}
