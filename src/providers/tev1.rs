use super::*;
pub struct Tev1;
pub static TEV1: Tev1 = Tev1;
const RECOMMENDED_MODEL: &str = "tev1:4b-q8_0";
const MODELS: &[&str] = &["tev1:4b", "tev1:4b-q8_0", "tev1:4b-q4_K_M", "tev1:4b-bf16"];
fn selected(cfg: &Config) -> &str {
    cfg.providers
        .get("tev1")
        .and_then(|m| m.get("ollama_model"))
        .map(String::as_str)
        .unwrap_or("tev1:4b")
}
fn download_model(cfg: &Config) -> &str {
    if selected(cfg) == "tev1:4b" {
        RECOMMENDED_MODEL
    } else {
        selected(cfg)
    }
}
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
        "local Tev1 4B; no API key. Setup explains Ollama, GGUF model preparation and connection checks.",
        "本机 Tev1 4B，无需 API Key。向导提供 Ollama、GGUF 模型准备和连接检查指引。",
    ),
    missing: Text(
        "Tev1 local configuration is invalid; use jjump setup.",
        "Tev1 本机配置无效；请运行 jjump setup。",
    ),
    fields: &[
        Field {
            key: "ollama_url",
            discover: false,
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
        },
        Field {
            key: "ollama_model",
            discover: true,
            nested: true,
            label: Text("Tev1 4B model", "Tev1 4B 模型"),
            help: Text(
                "Enter an installed GGUF tag: tev1:4b-q8_0 (recommended), tev1:4b-q4_K_M, tev1:4b-bf16, or an existing tev1:4b. Type list to contact your local service and select an installed compatible model. No model is downloaded automatically.",
                "输入已安装的 GGUF 标签：tev1:4b-q8_0（推荐）、tev1:4b-q4_K_M、tev1:4b-bf16，或已有的 tev1:4b。输入 list 连接本机服务并选择已安装的兼容模型，不会自动下载。",
            ),
            default: RECOMMENDED_MODEL,
            get: |c| selected(c).into(),
            set: |c, s| {
                if !MODELS.contains(&s) {
                    return Err(Error(2, "ollama_model expects a Tev1 4B GGUF tag: tev1:4b-q8_0, tev1:4b-q4_K_M, tev1:4b-bf16 or tev1:4b".into()));
                }
                c.providers
                    .entry("tev1".into())
                    .or_default()
                    .insert("ollama_model".into(), s.into());
                Ok(())
            },
        },
    ],
    credential: None,
};
impl ProviderDriver for Tev1 {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTION
    }
    fn model(&self) -> &'static str {
        "tev1:4b"
    }
    fn selected_model<'a>(&self, cfg: &'a Config) -> &'a str {
        selected(cfg)
    }
    fn accepts_model(&self, model: &str) -> bool {
        MODELS.contains(&model)
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
    fn prepare(&self, mut task: Value, model: &str) -> Result<Vec<u8>> {
        if !self.accepts_model(model) {
            return Err(Error(5, "unsupported request model".into()));
        }
        task["model"] = model.into();
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
    fn setup_help(&self, cfg: &Config, chinese: bool) -> Option<String> {
        let url = base(cfg);
        let host = url.trim_start_matches("http://");
        let tag = download_model(cfg);
        let steps = if chinese {
            format!(
                "准备本机 Tev1 4B：\n1. 安装 Ollama 0.35 或以上版本：https://ollama.com/download\n2. 打开 Ollama 应用，或在另一终端启动服务（保持运行）：\n   env OLLAMA_HOST='{host}' ollama serve\n3. 在另一终端下载 GGUF 模型（推荐 Q8_0 约 4.5 GB，需网络和磁盘空间）：\n   env OLLAMA_HOST='{url}' ollama pull {tag}\n4. 回到向导，填写地址并选择对应模型；不需要复制或覆盖模型别名。\n5. 显式检查会发送一条合成请求，可能加载模型，占用内存，最长等待 10 秒。\nJ-Jump 不安装或启动 Ollama，不自动下载模型；setup、doctor 不会自动发送请求。"
            )
        } else {
            format!(
                "Prepare local Tev1 4B:\n1. Install Ollama 0.35 or later: https://ollama.com/download\n2. Open the Ollama app, or start the service in another terminal (keep it running):\n   env OLLAMA_HOST='{host}' ollama serve\n3. Download GGUF weights in another terminal (recommended Q8_0 is about 4.5 GB; requires network and disk space):\n   env OLLAMA_HOST='{url}' ollama pull {tag}\n4. Return here, set the address and choose that model. No alias copy or overwrite is needed.\n5. An explicit check sends one synthetic request and may load the model into memory; deadline 10 seconds.\nJ-Jump does not install/start Ollama or automatically download models. Setup and doctor never send requests automatically."
            )
        };
        Some(steps)
    }
    fn recovery(&self, cfg: &Config, error: &Error, chinese: bool) -> Option<String> {
        let url = base(cfg);
        let tag = download_model(cfg);
        let reason = if error.1.contains("version") {
            Text(
                "Ollama version is missing or unsupported. Install/update Ollama 0.35 or later: https://ollama.com/download",
                "Ollama 版本缺失或过旧。请安装或更新至 0.35 以上：https://ollama.com/download",
            )
        } else if error.1.contains("connect") {
            Text(
                "Cannot connect to the selected local service. Open Ollama or start it at this address, then retry.",
                "无法连接所选本机服务。请打开 Ollama，或在此地址启动服务，然后重试。",
            )
        } else if error.1.contains("timeout") || error.1.contains("deadline") {
            Text(
                "The local check exceeded 10 seconds. Cold loading can take longer; check free memory and retry after the model loads.",
                "本机检查超过 10 秒。冷启动可能较慢；请检查可用内存，待模型加载后重试。",
            )
        } else if error.1.contains("missing") || error.1.contains("GGUF") {
            Text(
                "The selected model is missing or is not compatible GGUF. Download a supported tag and select that exact tag; MLX/Safetensors cannot serve this request.",
                "所选模型未安装或不是兼容的 GGUF。请下载支持的标签并选择该标签；MLX/Safetensors 无法处理此请求。",
            )
        } else {
            Text(
                "The local service returned an unsupported response or failed its decision check. Verify the address, update Ollama, and use a compatible Tev1 4B GGUF model.",
                "本机服务响应不兼容或未通过决策检查。请核对地址、更新 Ollama，并使用兼容的 Tev1 4B GGUF 模型。",
            )
        };
        Some(format!("{}\n{}: {url}\n{}: {}\n{}\n  env OLLAMA_HOST='{}' ollama serve\n{}\n  env OLLAMA_HOST='{url}' ollama pull {tag}\n{} {tag}\n{}",
            reason.get(chinese), Text("Address", "地址").get(chinese), Text("Selected model", "当前模型").get(chinese), selected(cfg),
            Text("Start the service in another terminal if it is stopped:", "服务未启动时，在另一终端运行：").get(chinese), url.trim_start_matches("http://"),
            Text("Install compatible weights if needed:", "需要安装兼容权重时：").get(chinese),
            Text("Choose this tag in setup, or use: jjump config set ollama_model", "在向导中选择该标签，或执行：jjump config set ollama_model").get(chinese),
            Text("Retry the check here or run jjump provider-check. No settings have been saved by this check.", "在这里重试检查，或运行 jjump provider-check。检查本身不会保存设置。").get(chinese)))
    }
    fn diagnostic(
        &self,
        cfg: &Config,
        kind: DiagnosticKind,
        responses: &[Value],
    ) -> Result<Diagnostic> {
        let model_id = self.selected_model(cfg);
        let probe = || {
            self.prepare(serde_json::json!({"state":"Select the option named ready.","questions":{"destination":{"type":"choice","instructions":"Select ready.","criteria":{"ready":"ready","none":"No match"}}}}), model_id)
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
        let core = v.split(['-', '+']).next().unwrap_or("");
        let parts = core
            .split('.')
            .map(str::parse::<u64>)
            .collect::<std::result::Result<Vec<_>, _>>();
        if !parts.is_ok_and(|p| p.len() == 3 && (p[0], p[1], p[2]) >= (0, 35, 0)) {
            return Err(Error(
                5,
                "local runtime version is unsupported; update Ollama to 0.35 or later".into(),
            ));
        }
        if responses.len() == 1 {
            return Ok(Diagnostic::Request {
                path: "/api/tags",
                body: None,
            });
        }
        let models = responses[1]["models"]
            .as_array()
            .ok_or(Error(5, "invalid local model metadata".into()))?;
        if kind == DiagnosticKind::Models {
            let mut options: Vec<ModelOption> = models
                .iter()
                .filter_map(|m| {
                    let id = m["name"].as_str().or_else(|| m["model"].as_str())?;
                    (self.accepts_model(id) && m["details"]["format"] == "gguf").then(|| {
                        ModelOption {
                            id: id.into(),
                            size_bytes: m["size"].as_u64().unwrap_or(0),
                        }
                    })
                })
                .collect();
            options.sort_by(|a, b| a.id.cmp(&b.id));
            options.dedup_by(|a, b| a.id == b.id);
            return Ok(Diagnostic::Complete(
                serde_json::json!({"runtime_version":v,"models":options,"synthetic_request":"not run","credentials":"not used"}),
            ));
        }
        let model = models
            .iter()
            .find(|m| m["name"] == model_id || m["model"] == model_id)
            .ok_or(Error(
                5,
                "local model is missing; select an installed compatible model or download it first"
                    .into(),
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
            "model":model_id,"model_digest":model["digest"],"model_format":"gguf","synthetic_request":"passed",
            "directory_quality":"not measured","credentials":"not used","transport":"loopback direct"}),
        ))
    }
}
