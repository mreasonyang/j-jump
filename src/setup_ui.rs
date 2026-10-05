//! Configuration stays in a single draft until an explicit, fingerprint-bound save.
use super::{ask, ui};
use j_jump::{
    Error, Result, adapter,
    config::{Config, Paths},
    store::Store,
};
use zeroize::Zeroizing;

fn cancelled() -> Error {
    Error(
        130,
        "setup cancelled; previous configuration retained".into(),
    )
}
fn value(cfg: &mut Config, key: &str, prompt: &str) -> Result<bool> {
    loop {
        let input = ask(prompt)?;
        let input = if [
            "semantic",
            "tracking",
            "consent",
            "privacy",
            "credential",
            "language",
            "proxy",
            "semantic_route",
        ]
        .contains(&key)
        {
            input.to_ascii_lowercase()
        } else {
            input
        };
        match input.as_str() {
            "q" => return Err(cancelled()),
            "b" => return Ok(false),
            "" => return Ok(true),
            _ => {}
        }
        let mut next = cfg.clone();
        let result = if key == "semantic" {
            match input.as_str() {
                "jev" => {
                    next.semantic = true;
                    Ok(())
                }
                "off" => {
                    next.semantic = false;
                    Ok(())
                }
                _ => Err(Error(2, "choose off or jev".into())),
            }
        } else {
            ui::field(&mut next, key, &input)
        };
        match result {
            Ok(()) => {
                *cfg = next;
                return Ok(true);
            }
            Err(e) => eprintln!("{}", ui::error_text(&e)),
        }
    }
}
fn semantic(cfg: &mut Config, start: usize) -> Result<bool> {
    let mut step = start;
    loop {
        let (key, prompt) = match step {
            0 => (
                "semantic",
                format!(
                    "{} [off/jev; {}]: ",
                    ui::tr("Semantic provider", "语义服务"),
                    if cfg.semantic { "jev" } else { "off" }
                ),
            ),
            1 => (
                "consent",
                format!(
                    "{} [ask/always; {}]: ",
                    ui::tr("Network permission", "联网许可"),
                    cfg.consent
                ),
            ),
            _ => {
                eprintln!(
                    "{}",
                    ui::tr(
                        "strict: names; balanced: limited context; full: paths.\nQuery and names can be private. Jev may charge for requests.",
                        "strict：目录名；balanced：有限上下文；full：路径。\n查询和名称也可能敏感；Jev 可能按请求计费。"
                    )
                );
                (
                    "privacy",
                    format!(
                        "{} [strict/balanced/full; {}]: ",
                        ui::tr("Fields", "发送字段"),
                        cfg.privacy
                    ),
                )
            }
        };
        if !value(cfg, key, &prompt)? {
            if step == 0 {
                return Ok(false);
            }
            step -= 1;
            continue;
        }
        if (step == 0 && !cfg.semantic) || step == 2 {
            return Ok(true);
        }
        step += 1;
    }
}
fn key(cfg: &mut Config, pending: &mut Option<Zeroizing<String>>) -> Result<bool> {
    loop {
        let env = std::env::var_os("TYPESAFE_API_KEY").is_some_and(|s| !s.is_empty());
        eprintln!(
            "{}",
            ui::tr(
                "Enter a Key here; it is stored when setup finishes. No connection test is sent.",
                "在这里输入 Key；完成设置时写入系统凭证库，不自动联网测试。"
            )
        );
        if env {
            eprintln!(
                "{}",
                ui::tr(
                    "TYPESAFE_API_KEY is present and overrides any stored key.",
                    "已检测到 TYPESAFE_API_KEY，它优先于系统存储的 Key。"
                )
            );
        }
        eprintln!(
            "{}: {}",
            ui::tr("Key choice", "Key 选择"),
            if pending.is_some() {
                ui::tr("new key pending save", "新 Key 等待保存")
            } else if cfg.credential == "system" {
                ui::tr(
                    "keep configured OS key (not checked)",
                    "保留已配置的系统 Key（尚未检查）",
                )
            } else if env {
                ui::tr("environment", "环境变量")
            } else {
                ui::tr("no key configured", "尚未配置 Key")
            }
        );
        let default = if pending.is_some() || cfg.credential == "system" || env {
            "keep"
        } else {
            "enter"
        };
        let answer = ask(&format!(
            "{} [enter/keep/environment/skip/b/q; {}]: ",
            ui::tr("API key", "API Key"),
            default
        ))?;
        let answer = answer.to_ascii_lowercase();
        match if answer.is_empty() {
            default
        } else {
            answer.as_str()
        } {
            "enter" => match super::read_secret() {
                Ok(secret) if secret.is_empty() => {
                    // Enter keeps the current choice; with no key that means none for now.
                    if pending.is_none() && cfg.credential != "system" && !env {
                        cfg.credential = "environment".into();
                        eprintln!(
                            "{}",
                            ui::tr(
                                "No key entered; Jev stays unavailable until you add one.",
                                "未输入 Key；添加之前 Jev 暂不可用。"
                            )
                        );
                    }
                    return Ok(true);
                }
                Ok(secret) => match j_jump::credential::validate(&secret) {
                    Ok(()) => {
                        *pending = Some(secret);
                        return Ok(true);
                    }
                    Err(e) => eprintln!("{}", ui::error_text(&e)),
                },
                Err(e) if e.0 == 2 => eprintln!("{}", ui::error_text(&e)),
                Err(e) => return Err(e),
            },
            "keep" if pending.is_some() || cfg.credential == "system" || env => return Ok(true),
            "environment" if env => {
                *pending = None;
                cfg.credential = "environment".into();
                return Ok(true);
            }
            "skip" => {
                *pending = None;
                cfg.credential = "environment".into();
                return Ok(true);
            }
            "b" => return Ok(false),
            "q" => return Err(cancelled()),
            _ => eprintln!(
                "{}",
                ui::tr(
                    "Choose enter, an available source, skip, b or q.",
                    "请输入 enter、可用来源、skip、b 或 q。"
                )
            ),
        }
    }
}
fn network(cfg: &mut Config, pending: &mut Option<Zeroizing<String>>) -> Result<bool> {
    loop {
        if !semantic(cfg, 0)? {
            return Ok(false);
        }
        if !cfg.semantic {
            *pending = None;
            return Ok(true);
        }
        if key(cfg, pending)? {
            return Ok(true);
        }
    }
}
fn readiness(cfg: &Config, pending: &Option<Zeroizing<String>>) {
    if pending.is_some() {
        eprintln!(
            "{}",
            ui::tr(
                "API key: hidden draft, saved to OS store when setup finishes.",
                "API Key：隐藏的草稿，完成设置时保存到系统凭证库。"
            )
        );
    }
    let mut effective = cfg.clone();
    if pending.is_some() {
        effective.credential = "system".into();
    }
    for line in ui::readiness(&effective, super::offline_env()) {
        if line == "jjump setup" {
            eprintln!(
                "{}",
                ui::tr(
                    "Back → 6 API key to enter a Key here.",
                    "返回 → 6 API Key，在这里填写 Key。"
                )
            );
        } else if pending.is_none()
            || !line.starts_with("OS credential:") && !line.starts_with("系统凭证：")
        {
            eprintln!("{line}");
        }
    }
}
fn roots(cfg: &mut Config, key: &str) -> Result<bool> {
    let label = if key == "no_send" {
        eprintln!(
            "{}",
            ui::tr(
                "These folders and their subfolders stay searchable locally. Their names and paths are not sent to Jev.\nWhile you are inside one, Jev requests are disabled. File contents are never uploaded.\nExample: add /work/private-client to keep that client's directory information local.",
                "这些文件夹及其子文件夹仍可在本地查找和跳转，但名称、路径不会发送给 Jev。\n你在这些文件夹里时，也不会调用 Jev。J-Jump 始终不会上传文件内容。\n例如：添加 /work/保密客户，让这个客户的目录信息只留在本机。"
            )
        );
        ui::tr(
            "Folders whose directory information stays local",
            "不向 Jev 发送目录信息的文件夹",
        )
    } else {
        eprintln!(
            "{}",
            ui::tr(
                "These folders and their subfolders are not recorded or included in local search. Direct paths still work.",
                "不记录这些文件夹及其子文件夹的访问，也不把它们列入本地搜索；仍可直接输入路径跳转。"
            )
        );
        ui::tr(
            "Folders excluded from history and search",
            "不记录、不搜索的文件夹",
        )
    };
    loop {
        let list = if key == "exclude" {
            &cfg.exclude
        } else {
            &cfg.no_send
        };
        eprintln!(
            "{}: {} {}",
            label,
            list.len(),
            ui::tr("roots (paths hidden)", "条路径（已隐藏）")
        );
        eprintln!(
            "{}",
            ui::tr(
                "Add one absolute path, or clear to remove all.\nEnter: done; b: back; q: discard setup.",
                "输入一个绝对路径以添加；clear 清空本组。\nEnter 完成；b 返回；q 放弃本次设置。"
            )
        );
        let input = ask("> ")?;
        match input.to_ascii_lowercase().as_str() {
            "" => return Ok(true),
            "b" => return Ok(false),
            "q" => return Err(cancelled()),
            "clear" => {
                if key == "exclude" {
                    cfg.exclude.clear();
                } else {
                    cfg.no_send.clear();
                }
            }
            _ => {
                let mut next = list.clone();
                next.push(input.into());
                match ui::field(cfg, key, &serde_json::to_string(&next).unwrap()) {
                    Ok(()) => {}
                    Err(e) => eprintln!("{}", ui::error_text(&e)),
                }
            }
        }
    }
}
fn advanced(cfg: &mut Config) -> Result<bool> {
    let mut step: usize = 0;
    loop {
        let done = match step {
            0 => roots(cfg, "exclude")?,
            1 => roots(cfg, "no_send")?,
            2 => value(
                cfg,
                "candidate_limit",
                &format!(
                    "{} [1..64; {}]: ",
                    ui::tr("Candidate limit", "候选数量"),
                    cfg.candidate_limit
                ),
            )?,
            _ => value(
                cfg,
                "language",
                &format!(
                    "{} [auto/en/zh; {}]: ",
                    ui::tr("Language", "显示语言"),
                    cfg.language
                ),
            )?,
        };
        if done {
            if step == 3 {
                return Ok(true);
            }
            step += 1;
        } else {
            if step == 0 {
                return Ok(false);
            }
            step -= 1;
        }
    }
}
fn reset(cfg: &mut Config) {
    *cfg = Config {
        tracking: cfg.tracking,
        exclude: cfg.exclude.clone(),
        no_send: cfg.no_send.clone(),
        ..Config::default()
    };
    eprintln!(
        "{}",
        ui::tr(
            "Draft reset: semantic off; privacy roots and tracking retained.\nVisits, cache and credentials unchanged. Not saved yet.",
            "草稿已恢复默认：关闭语义，保留排除规则和记录开关。\n访问记录、缓存和凭证不变；尚未保存。"
        )
    );
}
fn summary(cfg: &Config) {
    eprintln!("{}", ui::tr("Draft (not saved):", "草稿（尚未保存）："));
    eprintln!(
        "semantic={}\nconsent={}\nprivacy={}\ntracking={}\ncandidate_limit={}\nlanguage={}\n{}: {} / {}: {}",
        cfg.semantic,
        cfg.consent,
        cfg.privacy,
        cfg.tracking,
        cfg.candidate_limit,
        cfg.language,
        ui::tr("Excluded from history/search", "不记录、不搜索的文件夹"),
        cfg.exclude.len(),
        ui::tr(
            "Directory information stays local",
            "不向 Jev 发送目录信息的文件夹"
        ),
        cfg.no_send.len()
    );
}
/// Only explicit shell navigation may offer setup; do not consume redirected input.
pub fn if_needed(paths: &Paths) -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Ok(());
    }
    // An existing object belongs to the regular config/recovery path. In
    // particular, a dangling link is not an invitation to overwrite config.
    if !matches!(std::fs::symlink_metadata(&paths.config), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    eprintln!(
        "{}",
        ui::tr(
            "Welcome to J-Jump. Configure it here, then continue your navigation.",
            "欢迎使用 J-Jump。完成这里的配置后，将继续刚才的导航。",
        )
    );
    configure(paths, true)
}

pub fn run(paths: &Paths) -> Result<()> {
    configure(paths, false)
}

fn configure(paths: &Paths, inline: bool) -> Result<()> {
    let _ = super::tty()?;
    let expected = paths.fingerprint()?;
    let mut cfg = paths.load()?;
    let mut pending = None;
    ui::configure(&cfg.language);
    eprintln!(
        "{}",
        ui::tr(
            "J-Jump stores directory visits, never commands.\nLocal navigation needs no key or network.\nFirst setup saves when you finish the last step; later edits use Save in the menu.\nEnter keeps the current value; b goes back; q cancels before completion.\nEnable Jev to enter an API key here (hidden; OS store).",
            "J-Jump 只记录目录访问，不记录命令。\n本地导航不需要凭证或网络。\n首次完成最后一步后自动保存；以后修改设置，在菜单中选择保存。\nEnter 保留当前值；b 返回；完成前可按 q 退出不保存。\n启用 Jev 后可在这里输入 API Key（隐藏输入，系统存储）。"
        )
    );
    match Store::open(paths, false).and_then(|db| db.list()) {
        Ok(rows) if rows.is_empty() => eprintln!(
            "{}",
            ui::tr(
                "History is empty; visit directories with cd first.",
                "尚无访问记录，请先用 cd 访问目录。"
            )
        ),
        Ok(_) => {}
        Err(_) => eprintln!(
            "{}",
            ui::tr(
                "History unavailable; direct paths still work.\nRun jjump doctor for recovery.",
                "访问库不可用；直接路径仍可使用。\n请运行 jjump doctor 查看恢复方法。"
            )
        ),
    }
    if expected == "absent" {
        let mut stage = 0;
        loop {
            match stage {
                0 => {
                    if network(&mut cfg, &mut pending)? {
                        stage = 1;
                    }
                }
                1 => {
                    let prompt = format!(
                        "{} [on/off; {}]: ",
                        ui::tr("Local visit tracking", "本地访问记录"),
                        if cfg.tracking { "on" } else { "off" }
                    );
                    if value(&mut cfg, "tracking", &prompt)? {
                        stage = 2;
                    } else {
                        stage = 0;
                    }
                }
                _ => {
                    let input = ask(ui::tr(
                        "Advanced settings [Enter=finish/edit/reset]: ",
                        "高级设置 [Enter完成/edit编辑/reset默认]：",
                    ))?;
                    match input.to_ascii_lowercase().as_str() {
                        "" => break,
                        "b" => stage = 1,
                        "q" => return Err(cancelled()),
                        "edit" => {
                            if advanced(&mut cfg)? {
                                break;
                            }
                        }
                        "reset" => {
                            reset(&mut cfg);
                            pending = None;
                            break;
                        }
                        _ => eprintln!(
                            "{}",
                            ui::tr(
                                "Choose edit, reset, b, q or Enter.",
                                "请输入 edit、reset、b、q 或 Enter。"
                            )
                        ),
                    }
                }
            }
        }
    }
    let mut menu = expected != "absent";
    loop {
        ui::configure(&cfg.language);
        if menu {
            summary(&cfg);
            eprintln!(
                "{}",
                ui::tr(
                    "1 Semantic and network\n2 Visit tracking\n3 Roots and advanced settings\n4 Display language\n5 Reset draft\n6 API key\n7 Offline readiness\ns Save; q Exit without saving",
                    "1 语义和联网\n2 访问记录\n3 排除路径与高级设置\n4 显示语言\n5 草稿恢复默认\n6 API Key\n7 离线就绪检查\ns 保存；q 退出不保存"
                )
            );
            match ask("> ")?.to_ascii_lowercase().as_str() {
                "1" => {
                    network(&mut cfg, &mut pending)?;
                    continue;
                }
                "2" => {
                    let prompt = format!(
                        "{} [on/off; {}]: ",
                        ui::tr("Local visit tracking", "本地访问记录"),
                        if cfg.tracking { "on" } else { "off" }
                    );
                    value(&mut cfg, "tracking", &prompt)?;
                    continue;
                }
                "3" => {
                    advanced(&mut cfg)?;
                    continue;
                }
                "4" => {
                    let prompt = format!(
                        "{} [auto/en/zh; {}]: ",
                        ui::tr("Language", "显示语言"),
                        cfg.language
                    );
                    value(&mut cfg, "language", &prompt)?;
                    continue;
                }
                "5" => {
                    reset(&mut cfg);
                    pending = None;
                    continue;
                }
                "6" => {
                    key(&mut cfg, &mut pending)?;
                    continue;
                }
                "7" => {
                    readiness(&cfg, &pending);
                    continue;
                }
                "q" | "" => return Err(cancelled()),
                "s" => {}
                _ => {
                    eprintln!(
                        "{}",
                        ui::tr("Choose 1..7, s or q.", "请输入 1..7、s 或 q。")
                    );
                    continue;
                }
            }
        }
        match save(
            paths,
            &cfg,
            &expected,
            inline,
            pending.as_deref().map(String::as_str),
        ) {
            Ok(()) => return Ok(()),
            Err(e) if e.0 == 5 => {
                eprintln!("{}", ui::error_text(&e));
                eprintln!(
                    "{}",
                    ui::tr(
                        "Settings retained. Use 6 to edit the Key or s to retry saving.",
                        "设置已保留。选择 6 修改 Key，或选择 s 重试保存。"
                    )
                );
                menu = true;
            }
            Err(e) => return Err(e),
        }
    }
}

fn save(
    paths: &Paths,
    cfg: &Config,
    expected: &str,
    inline: bool,
    pending: Option<&str>,
) -> Result<()> {
    let mut committed = cfg.clone();
    if let Some(secret) = pending {
        j_jump::credential::save_config(paths, cfg, expected, secret)?;
        committed.credential = "system".into();
    } else {
        paths.save(cfg, expected)?;
    }
    if !cfg.semantic {
        let _ = adapter::stop(paths);
    }
    ui::configure(&cfg.language);
    eprintln!(
        "{}",
        ui::tr(
            "Saved. Configuration is now active.",
            "已保存。配置已生效。"
        )
    );
    if committed.semantic && !ui::credential_available(&committed) {
        eprintln!("{}", ui::missing_key_notice());
    }
    if inline {
        eprintln!(
            "{}",
            ui::tr("Continuing your navigation.", "继续刚才的导航。")
        );
        return Ok(());
    }
    for line in ui::readiness(&committed, super::offline_env()) {
        eprintln!("{line}");
    }
    eprintln!(
        "{}",
        ui::tr(
            "Check activation: try j -- / then j - in your terminal.\nUse ordinary cd to build visit history.",
            "请在当前终端验证：j -- /，再用 j - 返回。\n用普通 cd 积累访问记录。"
        )
    );
    Ok(())
}
