//! Human terminal presentation. Machine JSON and path stdout remain separate.
use j_jump::{
    Error, Result,
    config::{Config, Paths},
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

static CHINESE: AtomicBool = AtomicBool::new(false);
pub fn tr<'a>(en: &'a str, zh: &'a str) -> &'a str {
    if CHINESE.load(Ordering::Relaxed) {
        zh
    } else {
        en
    }
}
pub fn configure(saved: &str) {
    let override_lang = std::env::var("J_JUMP_LANG").ok();
    let chosen = override_lang.as_deref().unwrap_or(saved);
    let chinese = match chosen {
        "zh" => true,
        "en" => false,
        _ => ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("zh")),
    };
    CHINESE.store(chinese, Ordering::Relaxed);
}
pub fn explicit_config() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--config" {
            return args.next().map(PathBuf::from);
        }
        if let Some(value) = arg.to_str().and_then(|s| s.strip_prefix("--config=")) {
            return Some(PathBuf::from(value));
        }
    }
    None
}
pub fn load_preferences() {
    configure("auto");
    if let Ok(paths) = Paths::discover(explicit_config())
        && let Ok(cfg) = paths.load()
    {
        configure(&cfg.language);
    }
}
pub fn short_path(path: &Path, home: &Path) -> String {
    let text = if let Ok(rest) = path.strip_prefix(home) {
        if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", rest.display())
        }
    } else {
        path.to_string_lossy().into_owned()
    };
    j_jump::display(&text)
}
pub fn terminal_size() -> (usize, usize) {
    use std::os::fd::AsRawFd;
    if let Ok(tty) = super::tty() {
        let mut size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } == 0 {
            return (
                (size.ws_row as usize).max(8),
                (size.ws_col as usize).max(20),
            );
        }
    }
    (24, 80)
}
fn width(c: char) -> usize {
    match c as u32 {
        0x300..=0x36f | 0xfe00..=0xfe0f => 0,
        0x1100..=0x115f
        | 0x2e80..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe10..=0xfe6f
        | 0xff01..=0xff60
        | 0x1f300..=0x1faff
        | 0x20000..=0x3ffff => 2,
        _ => 1,
    }
}
pub fn suffix(text: &str, columns: usize) -> String {
    if text.chars().map(width).sum::<usize>() <= columns {
        return text.into();
    }
    let mut count = 1;
    let mut chars = Vec::new();
    for c in text.chars().rev() {
        count += width(c);
        if count > columns {
            break;
        }
        chars.push(c);
    }
    format!("…{}", chars.into_iter().rev().collect::<String>())
}
pub fn readiness(cfg: &Config, offline: bool) -> Vec<String> {
    let mut lines = vec![
        tr(
            "Shell activation: verify in your terminal.",
            "Shell 接入：请在当前终端验证。",
        )
        .into(),
    ];
    if !cfg.semantic {
        lines.push(
            tr(
                "Semantic help: off (no network).",
                "语义查找：已关闭，不联网。",
            )
            .into(),
        );
        return lines;
    }
    if offline {
        lines.push(
            tr(
                "This command is offline; no provider request.",
                "本次命令处于离线模式，不调用语义服务。",
            )
            .into(),
        );
    }
    lines.push(format!(
        "{}: {}",
        tr("Semantic provider", "语义服务"),
        cfg.provider.label()
    ));
    if cfg.provider.driver().connection(cfg, None).is_err() {
        lines.push(missing_key_notice(cfg));
        lines.push("jjump setup".into());
    }
    if !cfg.provider.needs_credentials() {
        for f in cfg.provider.driver().descriptor().fields {
            lines.push(format!(
                "{}: {}",
                f.label.get(tr("en", "zh") == "zh"),
                j_jump::display(&(f.get)(cfg))
            ));
        }
        lines.push(
            tr(
                "Credentials: not required; OS credential store is not accessed.",
                "无需凭据，不访问系统凭证库。",
            )
            .into(),
        );
        lines.push(
            tr(
                "Local service and model: not checked. Run jjump provider-check explicitly.",
                "本机服务与模型：尚未检查。请显式运行 jjump provider-check。",
            )
            .into(),
        );
    } else if let Some(notice) = invalid_credential_notice(cfg) {
        lines.push(notice);
    } else if let Some(name) = j_jump::credential::environment_name(cfg.provider) {
        lines.push(format!("{}: {name}.", tr("Credential source", "凭证来源")));
    } else if cfg.credential_source() == "system" {
        lines.push(
            tr(
                "OS credential: configured, not checked.",
                "系统凭证：已配置，尚未检查。",
            )
            .into(),
        );
    } else {
        lines.push(format!(
            "{} {}",
            cfg.provider.label(),
            tr(
                "blocked: no credential configured.",
                "暂不可用：尚未配置凭证。"
            )
        ));
        lines.push("jjump setup".into());
    }
    if cfg.provider.needs_credentials() {
        lines.push(format!(
            "{} {}",
            cfg.provider.label(),
            tr("connection: not tested here.", "连接：本次尚未实测。")
        ));
    }
    lines
}
pub fn field(cfg: &mut Config, key: &str, value: &str) -> Result<()> {
    fn boolean(value: &str) -> Result<bool> {
        match value {
            "on" | "true" => Ok(true),
            "off" | "false" => Ok(false),
            _ => Err(Error(
                2,
                "expected on or off; configuration unchanged".into(),
            )),
        }
    }
    match key {
        "semantic" => cfg.semantic = boolean(value)?,
        "provider" => j_jump::providers::select(cfg, j_jump::config::Provider::parse(value)?)?,
        key if j_jump::providers::field(key).is_some() => {
            (j_jump::providers::field(key).unwrap().set)(cfg, value)?;
        }
        "tracking" => cfg.tracking = boolean(value)?,
        "consent" if ["ask", "always"].contains(&value) => cfg.consent = value.into(),
        "privacy" if ["strict", "balanced", "full"].contains(&value) => cfg.privacy = value.into(),
        "language" if ["auto", "en", "zh"].contains(&value) => cfg.language = value.into(),
        "semantic_route" if ["local_first", "force"].contains(&value) => {
            cfg.semantic_route = value.into()
        }
        "candidate_limit" => {
            cfg.candidate_limit = value
                .parse::<usize>()
                .ok()
                .filter(|v| (1..=j_jump::config::CANDIDATE_LIMIT_MAX).contains(v))
                .ok_or(Error(
                    2,
                    "candidate_limit expects 1..254; configuration unchanged".into(),
                ))?
        }
        "exclude" | "no_send" => {
            let roots: Vec<PathBuf> = serde_json::from_str(value).map_err(|_| {
                Error(
                    2,
                    "roots expect a JSON array of absolute paths; configuration unchanged".into(),
                )
            })?;
            if roots.iter().any(|p| {
                !p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
            }) {
                return Err(Error(
                    2,
                    "roots must be absolute paths without '..'; configuration unchanged".into(),
                ));
            }
            if key == "exclude" {
                cfg.exclude = roots;
            } else {
                cfg.no_send = roots;
            }
        }
        "consent" => {
            return Err(Error(
                2,
                "consent expects ask or always; configuration unchanged".into(),
            ));
        }
        "privacy" => {
            return Err(Error(
                2,
                "privacy expects strict, balanced or full; configuration unchanged".into(),
            ));
        }
        "language" => return Err(Error(2, "language expects auto, en or zh".into())),
        "semantic_route" => {
            return Err(Error(
                2,
                "semantic_route expects local_first or force".into(),
            ));
        }
        "automatic" => {
            return Err(Error(
                5,
                "automatic semantic navigation requires genuine quality evidence; unavailable"
                    .into(),
            ));
        }
        _ => {
            return Err(Error(
                2,
                "unknown setting; run jjump config set --help".into(),
            ));
        }
    }
    Ok(())
}
pub const SETTINGS: &str = "tracking/semantic: on|off\nconsent: ask|always\nprivacy: strict|balanced|full\ncandidate_limit: 1..254\nsemantic_route: local_first|force\nlanguage: auto|en|zh\nexclude/no_send: JSON array of absolute paths\nExample: jjump config set semantic off\nEnter your API key in jjump setup; never put it in config.";
pub fn command(mut cmd: clap::Command) -> clap::Command {
    let fields = j_jump::providers::DRIVERS
        .iter()
        .flat_map(|d| d.descriptor().fields)
        .map(|f| format!("{}: {}", f.key, f.help.get(tr("en", "zh") == "zh")))
        .collect::<Vec<_>>()
        .join("\n");
    let settings = format!(
        "{}\nprovider: {}\n{}",
        SETTINGS,
        j_jump::providers::ids(),
        fields
    );
    cmd = cmd.mut_subcommand("config", |c| {
        c.mut_subcommand("set", |s| s.after_help(settings.clone()))
    });
    if tr("en", "zh") == "en" {
        return cmd;
    }
    cmd = cmd.about("本地目录导航；可选语义建议须确认。")
        .after_help("首次 j/ji 自动引导；后续修改：jjump setup\nZsh: eval \"$(jjump init zsh)\"\nBash: eval \"$(jjump init bash)\"\nFish: jjump init fish | source\n本地导航不需要凭证或网络。\n用法：j 查询词；j -- 路径；j -；ji 查询词\n退出码：2 输入；3 无匹配；4 需选择；5 服务；6 路径；7 状态；130 取消。")
        .mut_subcommands(|sub| {
            let desc = match sub.get_name() {
                "init" => "生成 Shell 接入代码，不修改启动文件",
                "query" => "解析目录；成功时只输出绝对路径",
                "record" => "内部访问记录命令",
                "setup" => "逐步配置，完成后保存",
                "config" => "查看或修改设置，不存储密钥",
                "credential" => "管理 J-Jump 系统凭证",
                "doctor" => "离线检查配置、目录库和下一步",
                "provider-check" => "显式检查本机服务、模型和合成决策",
                "explain" => "离线解释本地排序",
                "preview" => "离线预览发送给所选服务商的 JSON",
                "history" => "管理本地访问记录",
                "cache" => "仅清理语义缓存",
                "data" => "清理访问记录和缓存",
                "adapter" => "管理本地传输进程",
                _ => return sub,
            };
            sub.about(desc)
        });
    cmd = cmd.mut_subcommand("provider-check", |c| {
        c.mut_arg("models", |a| a.help("仅查询已安装的兼容模型，不加载模型"))
            .mut_arg("json", |a| a.help("输出 JSON 诊断"))
    });
    cmd = cmd
        .mut_arg("config", |a| a.help("使用指定的绝对配置路径"))
        .mut_arg("offline", |a| a.help("本次命令禁止语义请求"))
        .mut_arg("force_semantic", |a| {
            a.help("有本地匹配时也请求所选服务商；仍须选择")
        })
        .mut_subcommand("config", |c| {
            c.mut_subcommand("set", |c| {
                c.about("修改一个字段；错误时不保存").after_help(settings)
            })
        });
    cmd
}
pub fn shell_help(name: &str, interactive: bool) -> Result<()> {
    // Also reject control characters when this internal entry is invoked directly.
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(Error(2, "invalid command name".into()));
    }
    println!("{}: {name}", tr("Directory navigation", "目录导航"));
    println!(
        "{name} QUERY        {}",
        tr("find a visited directory", "查找已访问目录")
    );
    println!(
        "{name} -- PATH      {}",
        tr("use a literal path", "使用字面路径")
    );
    if interactive {
        println!(
            "{name}                 {}",
            tr("choose locally; no network", "本地选择，不联网")
        );
    }
    if !interactive {
        println!(
            "{name}                 HOME\n{name} -               {}",
            tr("previous directory", "上一目录")
        );
    }
    println!(
        "{}\njjump setup\njjump doctor\njjump --help",
        tr("Settings and help:", "设置与帮助：")
    );
    Ok(())
}
/// A semantic request is possible only with an environment key or a configured OS entry.
pub fn credential_available(cfg: &Config) -> bool {
    cfg.provider.driver().connection(cfg, None).is_ok()
        && (!cfg.provider.needs_credentials()
            || ((j_jump::credential::environment_name(cfg.provider).is_some()
                || cfg.credential_source() == "system")
                && invalid_credential_notice(cfg).is_none()))
}
pub fn account_override_notice(cfg: &Config) -> Option<String> {
    cfg.provider
        .driver()
        .config_notice(cfg, tr("en", "zh") == "zh")
}
pub fn invalid_credential_notice(cfg: &Config) -> Option<String> {
    let name = j_jump::credential::environment_name(cfg.provider)?;
    if j_jump::credential::get_for(cfg.provider, "environment").is_ok() {
        return None;
    }
    Some(format!(
        "{name} {}",
        tr(
            "is invalid. Use a UTF-8 key/token of 1..4096 bytes without control characters, or unset it to use the saved key.",
            "无效。请使用 1..4096 字节且不含控制字符的 UTF-8 Key/Token，或清除它以使用已保存的凭据。"
        )
    ))
}
pub fn consent_prompt(cfg: &Config, count: usize) -> String {
    let fields = match cfg.privacy.as_str() {
        "strict" => tr("directory names", "目录名"),
        "balanced" => tr(
            "directory and parent names, usage, current directory name",
            "目录名、父目录名、使用频度和当前目录名",
        ),
        _ => tr(
            "directory full paths, usage, current full path",
            "目录的完整路径、使用频度和当前完整路径",
        ),
    };
    format!(
        "{} {} {} {} ({})?\n{}: {}. [y/N] ",
        tr("Send query and", "发送查询和"),
        count,
        tr("directory options to", "个目录选项给"),
        cfg.provider.label(),
        cfg.privacy,
        tr("Fields", "发送字段"),
        fields
    )
}
pub fn missing_key_notice(cfg: &Config) -> String {
    account_override_notice(cfg)
        .or_else(|| invalid_credential_notice(cfg))
        .unwrap_or_else(|| {
            cfg.provider
                .driver()
                .descriptor()
                .missing
                .get(tr("en", "zh") == "zh")
                .into()
        })
}
pub fn error_text(e: &Error) -> String {
    if tr("en", "zh") == "en" {
        if e.1.contains("authentication rejected") {
            return format!(
                "{e}\nCheck the selected provider's key/token and permissions. Environment keys override saved keys; edit jjump setup or your environment."
            );
        }
        return e.to_string();
    }
    let provider = j_jump::providers::DRIVERS
        .iter()
        .map(|d| d.descriptor().label)
        .find(|label| e.1.contains(label))
        .unwrap_or("语义服务");
    let detail: std::borrow::Cow<'_, str> = if e.1.contains("ollama_url expects") {
        "Ollama 地址只允许 http://127.0.0.1:端口 或 http://[::1]:端口，不允许远程主机、凭据、路径或查询参数。".into()
    } else if e.1.contains("ollama_model expects") {
        "模型标签只允许 tev1:4b、tev1:4b-q8_0、tev1:4b-q4_K_M 或 tev1:4b-bf16；需要 GGUF 权重。"
            .into()
    } else if e
        .1
        .contains("provider-check is unavailable in offline mode")
    {
        "离线模式不能检查服务或查询模型；本地导航仍然可用。".into()
    } else if e.1.contains("payload exceeds its byte/context budget") {
        "请求超过所选模型的上下文预算；请缩短查询或减少发送上下文。".into()
    } else if e.1.contains("local runtime version is unsupported") {
        "本机运行时版本过旧；请更新至所选服务要求的版本。".into()
    } else if e.1.contains("local model is missing") {
        "本机尚未安装所选模型；请通过模型运行时安装后重试。".into()
    } else if e.1.contains("compatible GGUF weights") {
        "本机 System One 需要兼容的 GGUF 权重，不支持 MLX/Safetensors。".into()
    } else if e.1.contains("does not use credentials") {
        "所选服务无需凭据；未访问或修改其他服务的凭据。".into()
    } else if e.1.contains("authentication rejected") {
        format!("{provider} 拒绝了凭据；请检查 Key/Token 和权限。环境凭据优先于已保存的凭据；可在 jjump setup 或环境变量中更正。\n本地浏览：jjump --offline query --interactive").into()
    } else if e.0 == 5 && e.1.contains("rate limit or quota") {
        format!("{provider} 请求受到限流或配额限制；请稍后重试或检查服务商配额。\n本地浏览：jjump --offline query --interactive").into()
    } else if e.0 == 5 && (e.1.contains("timed out") || e.1.contains("deadline")) {
        format!(
            "{provider} 请求超时；请检查网络后重试。\n本地浏览：jjump --offline query --interactive"
        )
        .into()
    } else if e.0 == 5
        && (e.1.contains("connection failed")
            || e.1.contains("transport failed")
            || e.1.contains("response body failed"))
    {
        format!("{provider} 请求连接失败；请检查网络后重试。\n本地浏览：jjump --offline query --interactive").into()
    } else if e.0 == 5
        && (e.1.contains("invalid JSON")
            || e.1.contains("schema mismatch")
            || e.1.contains("Cloudflare response"))
    {
        format!("{provider} 响应格式无效；未选择目录。可稍后重试，或本地浏览：jjump --offline query --interactive").into()
    } else if e.1.starts_with("provider expects") {
        format!("provider：请选择 {}。", j_jump::providers::ids()).into()
    } else if e.1.starts_with("choose off or") {
        format!("请选择 off（仅本地）或 {}。", j_jump::providers::ids()).into()
    } else if e.1.starts_with("cloudflare_account_id expects") {
        "Cloudflare Account ID 必须是 32 位十六进制字符；请填写 Account ID，而非 Token 或邮箱。"
            .into()
    } else if e.1.starts_with("credential must") || e.1.starts_with("key must") {
        "Key/Token 必须为 UTF-8，最多 4096 字节，且不能包含换行、制表符等控制字符。".into()
    } else if e.1.starts_with("privacy expects") {
        "privacy 允许 strict、balanced、full。".into()
    } else if e.1.starts_with("consent expects") {
        "consent 允许 ask、always。".into()
    } else if e.1.starts_with("candidate_limit expects") {
        "candidate_limit 必须为 1..254 的整数。".into()
    } else if e.1.starts_with("language expects") {
        "language 允许 auto、en、zh。".into()
    } else if e.1.starts_with("unknown setting") {
        "未知设置；请运行 jjump config set --help。".into()
    } else if e.1.contains("concurrently") || e.1.starts_with("configuration changed") {
        "配置已被其他进程修改，请重新打开 setup。".into()
    } else {
        e.1.as_ref().into()
    };
    let explanation = match e.0 {
        2 => "输入无效；请按提示更正，配置未保存。",
        3 if e.1.contains("found no reliable match") => "所选服务没有可靠建议；未选择目录。",
        3 => "没有匹配目录；可用 ji 选择，或先用 cd 访问。",
        4 => "此操作需要交互终端。",
        5 => "语义服务未完成；未选择目录。",
        6 => "目录不可用；请检查名称和访问权限。",
        7 if e.1.contains("busy") => "访问库正忙；请稍后重试。",
        7 => "配置或数据状态异常；请运行 jjump doctor。",
        130 => "已取消；当前操作未选择目录或保存草稿。",
        _ => "操作未完成。",
    };
    format!("J{}: {}\n{}", e.0, explanation, detail)
}
