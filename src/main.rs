use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use j_jump::{
    Error, Result, adapter,
    config::{Config, Paths},
    engine::{self, Candidate},
    provider,
    store::Store,
};
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::{self, IsTerminal, Read, Write},
    os::fd::AsRawFd,
    path::PathBuf,
    time::Instant,
};
mod setup_ui;
mod ui;
mod wait_ui;
#[derive(Parser)]
#[command(
    name = "jjump",
    bin_name = "jjump",
    version,
    about = "Private local directory navigation. Optional Jev suggestions always require selection.",
    after_help = "Connect your shell automatically: jjump shell install\nFirst j/ji opens setup; edit later: jjump setup\nZsh: eval \"$(jjump init zsh)\"\nBash: eval \"$(jjump init bash)\"\nFish: jjump init fish | source\nUse j QUERY, j -- PATH, j -, ji QUERY.\nNo key/network is required for local navigation. Exit codes: 2 input, 3 no match, 4 selection required, 5 provider, 6 path, 7 state, 130 cancelled."
)]
struct Cli {
    #[arg(long, global = true, help = "Use an absolute configuration file path")]
    config: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Disable semantic requests for this command"
    )]
    offline: bool,
    #[arg(
        long,
        global = true,
        help = "Request Jev even with local matches; selection required"
    )]
    force_semantic: bool,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Install or remove automatic shell integration (no network)
    Shell {
        #[command(subcommand)]
        action: ShellCommand,
    },
    /// Print the shell integration code for your startup file
    Init {
        shell: String,
        #[arg(long, default_value = "j")]
        cmd: String,
    },
    /// Print the best matching visited directory (used by j and ji)
    Query {
        #[arg(long, hide = true)]
        setup_if_needed: bool,
        #[arg(long)]
        interactive: bool,
        #[arg(long)]
        complete: bool,
        #[arg(long)]
        explicit: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    #[command(hide = true)]
    Record {
        #[arg(long)]
        generation: Option<String>,
        path: PathBuf,
    },
    /// Internal prompt observer; stdout is only its bounded supervisor PID.
    #[command(hide = true)]
    Observe { path: PathBuf },
    #[command(hide = true)]
    ObserveSupervise { path: PathBuf },
    #[command(hide = true)]
    SetupIfNeeded,
    #[command(hide = true)]
    ShellHelp {
        cmd: String,
        #[arg(long)]
        interactive: bool,
    },
    /// Configure step by step in your terminal; first setup saves on completion.
    Setup,
    /// Store or remove the Jev key in the OS credential store
    Credential {
        #[command(subcommand)]
        action: CredentialCommand,
    },
    /// Show or change settings
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Check settings, visit history and Jev readiness (no network)
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Show which visited directories match a query and how they rank
    Explain {
        #[arg(long)]
        json: bool,
        terms: Vec<String>,
    },
    /// Show exactly what a Jev request would send (no network)
    Preview { terms: Vec<String> },
    /// List, back up, restore, prune or delete visit history
    History {
        #[command(subcommand)]
        action: HistoryCommand,
    },
    /// Clear cached Jev answers; history and keys are kept
    Cache {
        #[command(subcommand)]
        action: ClearCommand,
    },
    /// Clear visit history and cached Jev answers
    Data {
        #[command(subcommand)]
        action: ClearCommand,
    },
    /// Show or stop the background Jev helper
    Adapter {
        #[command(subcommand)]
        action: AdapterCommand,
    },
    #[command(hide = true)]
    AdapterServe,
}
#[derive(Subcommand)]
enum ShellCommand {
    /// Back up and connect your shell; open a new terminal afterwards
    Install(ShellOptions),
    /// Remove unchanged J-Jump managed blocks; preserve your other settings
    Uninstall(ShellOptions),
}
#[derive(Args)]
struct ShellOptions {
    /// Shell to connect (default: your login shell from SHELL)
    #[arg(long, value_parser = ["bash", "zsh", "fish"])]
    shell: Option<String>,
    /// Use this absolute startup-file path instead of the default
    #[arg(long)]
    rc: Option<PathBuf>,
    /// Navigation command prefix; install uses j by default
    #[arg(long, default_value = "j")]
    cmd: String,
    /// Stable binary directory, used by the archive installer
    #[arg(long, hide = true)]
    bin_dir: Option<PathBuf>,
}
#[derive(Subcommand)]
enum AdapterCommand {
    Status,
    Stop,
    Restart,
}
#[derive(Subcommand)]
enum CredentialCommand {
    Status,
    Set,
    Delete {
        #[arg(long)]
        apply: bool,
    },
}
#[derive(Subcommand)]
enum ConfigCommand {
    Show {
        #[arg(long)]
        json: bool,
    },
    #[command(after_help = ui::SETTINGS)]
    Set {
        key: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    Reset {
        #[arg(long, conflicts_with = "apply")]
        preview: bool,
        #[arg(long)]
        apply: bool,
    },
    Recover {
        #[arg(long)]
        apply: bool,
    },
}
#[derive(Subcommand)]
enum HistoryCommand {
    /// Write a private, versioned backup of visit records.
    Backup {
        out: PathBuf,
    },
    /// Validate a private backup; replace visits only with --apply.
    Restore {
        file: PathBuf,
        #[arg(long)]
        apply: bool,
    },
    List,
    Prune {
        #[arg(long)]
        apply: bool,
    },
    Clear {
        #[arg(long, conflicts_with = "apply")]
        preview: bool,
        #[arg(long)]
        apply: bool,
    },
    Forget {
        #[arg(long, conflicts_with = "apply")]
        preview: bool,
        #[arg(long)]
        apply: bool,
        path: PathBuf,
    },
}
#[derive(Subcommand)]
enum ClearCommand {
    Clear {
        #[arg(long, conflicts_with = "apply")]
        preview: bool,
        #[arg(long)]
        apply: bool,
    },
}
fn tty() -> Result<std::fs::File> {
    if !io::stdin().is_terminal() {
        return Err(Error(
            4,
            "selection requires an interactive terminal; stdin was not read".into(),
        ));
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| Error(4, "no controlling terminal; no directory selected".into()))
}
struct TerminalSignals {
    fd: libc::c_int,
    original: Option<libc::termios>,
}
impl Drop for TerminalSignals {
    fn drop(&mut self) {
        if let Some(original) = &self.original {
            // Restore only settings owned by this bounded terminal read.
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, original);
            }
        }
    }
}
fn read_secret() -> Result<zeroize::Zeroizing<String>> {
    let mut f = tty()?;
    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(f.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
        return Err(Error(130, "terminal settings unavailable".into()));
    }
    let original = unsafe { mode.assume_init() };
    let mut input = original;
    input.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
    input.c_iflag &= !libc::IXON;
    input.c_cc[libc::VMIN] = 1;
    input.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(f.as_raw_fd(), libc::TCSANOW, &input) } != 0 {
        return Err(Error(130, "hidden input unavailable".into()));
    }
    let _restore = TerminalSignals {
        fd: f.as_raw_fd(),
        original: Some(original),
    };
    write!(
        f,
        "{}",
        ui::tr(
            "Jev API key (hidden; Enter keeps current choice): ",
            "Jev API Key（隐藏输入；Enter 保留当前选择）："
        )
    )
    .and_then(|_| f.flush())
    .map_err(|_| Error(130, "terminal closed".into()))?;
    let mut bytes = zeroize::Zeroizing::new(Vec::with_capacity(4096));
    let mut invalid = false;
    loop {
        let mut byte = zeroize::Zeroizing::new([0]);
        if f.read(&mut *byte)
            .map_err(|_| Error(130, "credential entry cancelled".into()))?
            == 0
        {
            return Err(Error(130, "credential entry cancelled".into()));
        }
        match byte[0] {
            b'\r' | b'\n' => {
                let _ = f.write_all(b"\r\n");
                if invalid {
                    return Err(Error(
                        2,
                        "key must be at most 4096 bytes without control characters".into(),
                    ));
                }
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| Error(2, "key must be valid UTF-8".into()))?;
                return Ok(zeroize::Zeroizing::new(text.to_owned()));
            }
            3 | 4 | 27 => {
                let _ = f.write_all(b"\r\n");
                return Err(Error(130, "credential entry cancelled".into()));
            }
            8 | 127 => {
                bytes.pop();
                while !bytes.is_empty() && std::str::from_utf8(&bytes).is_err() {
                    bytes.pop();
                }
            }
            0..=31 => invalid = true,
            ch => {
                if bytes.len() == 4096 {
                    invalid = true;
                } else {
                    bytes.push(ch);
                }
            }
        }
    }
}
fn ask(prompt: &str) -> Result<String> {
    let mut f = tty()?;
    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(f.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
        return Err(Error(130, "terminal settings unavailable".into()));
    }
    let original = unsafe { mode.assume_init() };
    let raw = original.c_lflag & libc::ICANON == 0;
    let echo = raw && original.c_lflag & libc::ECHO == 0;
    let mut signals = TerminalSignals {
        fd: f.as_raw_fd(),
        original: None,
    };
    if raw && original.c_lflag & libc::ISIG != 0 {
        let mut input = original;
        input.c_lflag &= !libc::ISIG;
        if unsafe { libc::tcsetattr(f.as_raw_fd(), libc::TCSANOW, &input) } != 0 {
            return Err(Error(130, "terminal cancellation setup failed".into()));
        }
        signals.original = Some(original);
    }
    write!(f, "{prompt}")
        .and_then(|_| f.flush())
        .map_err(|_| Error(130, "terminal closed".into()))?;
    // CR is Enter in raw widgets too. Never read ahead into the shell's input.
    let mut text = Vec::new();
    loop {
        let mut byte = [0];
        if f.read(&mut byte)
            .map_err(|_| Error(130, "selection cancelled".into()))?
            == 0
        {
            return Err(Error(130, "selection cancelled".into()));
        }
        match byte[0] {
            b'\r' | b'\n' => {
                if echo {
                    let _ = f.write_all(b"\r\n");
                }
                return String::from_utf8(text)
                    .map(|s| s.trim().to_owned())
                    .map_err(|_| Error(130, "invalid terminal input".into()));
            }
            3 | 4 | 27 => return Err(Error(130, "selection cancelled".into())),
            8 | 127 => {
                if text.pop().is_some() {
                    while std::str::from_utf8(&text).is_err() {
                        text.pop();
                    }
                    if echo {
                        let _ = f.write_all(b"\x08 \x08");
                    }
                }
            }
            0..=31 => {}
            ch => {
                if text.len() >= 4096 {
                    return Err(Error(130, "terminal input too long".into()));
                }
                text.push(ch);
                if echo {
                    let _ = f.write_all(&[ch]);
                    let _ = f.flush();
                }
            }
        }
    }
}
fn pick(candidates: &[Candidate], suggested: Option<&str>) -> Result<PathBuf> {
    ui::load_preferences();
    if candidates.is_empty() {
        return Err(Error(
            3,
            "no directory matches; use ordinary cd to build local history".into(),
        ));
    }
    let mut f = tty()?;
    match std::env::var("J_JUMP_PICKER").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("numbered") => {}
        Ok("fzf") => {
            return j_jump::picker::fuzzy(&f, candidates, suggested, ui::tr("en", "zh") == "zh");
        }
        _ => return Err(Error(2, "J_JUMP_PICKER expects numbered or fzf".into())),
    }
    let (rows, columns) = ui::terminal_size();
    let page_size = rows.saturating_sub(5).clamp(1, 20);
    let mut shown: Vec<_> = candidates.iter().collect();
    let mut filter = String::new();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut page = 0;
    loop {
        let pages = shown.len().div_ceil(page_size).max(1);
        if !filter.is_empty() {
            writeln!(f, "Matching {}: {}", j_jump::display(&filter), shown.len())
                .map_err(|_| Error(130, "terminal closed".into()))?;
        }
        if suggested.is_some() {
            writeln!(f, "Jev suggestion: first; choose explicitly")
                .map_err(|_| Error(130, "terminal closed".into()))?;
        }
        writeln!(f, "{}", ui::tr("Choose a directory", "选择目录"))
            .map_err(|_| Error(130, "terminal closed".into()))?;
        writeln!(
            f,
            "{} {}/{} | {}",
            ui::tr("Page", "页"),
            page + 1,
            pages,
            ui::tr("n next / p previous", "n 下一页 / p 上一页")
        )
        .map_err(|_| Error(130, "terminal closed".into()))?;
        for (i, c) in shown
            .iter()
            .enumerate()
            .skip(page * page_size)
            .take(page_size)
        {
            let label = if Some(c.id.as_str()) == suggested {
                " [Jev]"
            } else {
                ""
            };
            let prefix = format!("{}. ", i + 1);
            let shown = ui::suffix(
                &ui::short_path(&c.path, &home),
                columns.saturating_sub(prefix.len() + label.len()),
            );
            writeln!(f, "{prefix}{shown}{label}")
                .map_err(|_| Error(130, "terminal closed".into()))?;
        }
        writeln!(
            f,
            "{}",
            ui::tr(
                "Number: select; v N: full path",
                "编号选择；v N 查看完整路径"
            )
        )
        .map_err(|_| Error(130, "terminal closed".into()))?;
        writeln!(f, "{}", ui::tr("Empty/q: cancel", "空输入/q 取消"))
            .map_err(|_| Error(130, "terminal closed".into()))?;
        loop {
            let input = ask("> ")?;
            match input.as_str() {
                "" | "q" => {
                    return Err(Error(
                        130,
                        "selection cancelled; directory unchanged".into(),
                    ));
                }
                "n" => {
                    page = (page + 1).min(pages - 1);
                    break;
                }
                "p" => {
                    page = page.saturating_sub(1);
                    break;
                }
                _ => {}
            }
            let (view, number) = input
                .strip_prefix("v ")
                .map_or((false, input.as_str()), |v| (true, v));
            if let Some(i) = number
                .parse::<usize>()
                .ok()
                .filter(|i| *i > 0 && *i <= shown.len())
            {
                if view {
                    writeln!(
                        f,
                        "{}",
                        j_jump::display(&shown[i - 1].path.to_string_lossy())
                    )
                    .map_err(|_| Error(130, "terminal closed".into()))?;
                } else {
                    return Ok(shown[i - 1].path.clone());
                }
            } else if !view && number.parse::<usize>().is_err() {
                filter = input;
                let folded = engine::key(&filter);
                shown = candidates
                    .iter()
                    .filter(|c| engine::key(&c.path.to_string_lossy()).contains(&folded))
                    .collect();
                page = 0;
                break;
            } else {
                writeln!(
                    f,
                    "{} 1..{}.",
                    ui::tr("Enter a number in", "请输入范围内的编号"),
                    shown.len()
                )
                .map_err(|_| Error(130, "terminal closed".into()))?;
            }
        }
    }
}
fn output(path: &std::path::Path) -> Result<()> {
    let path = engine::valid_path(path)?;
    println!(
        "{}",
        path.to_str().ok_or(Error(6, "non UTF-8 path".into()))?
    );
    Ok(())
}
fn inventory(paths: &Paths, cfg: &Config) -> Result<(Store, Vec<Candidate>)> {
    let mut db = Store::open(paths, false)?;
    db.prune_missing(true, true)?;
    let all = db.list()?;
    let policy = paths.policy(cfg, false);
    let all = engine::ranked(
        all.into_iter().filter(|c| policy.allows(&c.path)).collect(),
        j_jump::now(),
    );
    Ok((db, all))
}
/// Shared query-term normalization so `query` and `explain` describe exactly
/// the same candidate set: strip a leading `--` separator, reject empty terms,
/// and strip a trailing child-only `/`.
fn prepare_terms(
    mut terms: Vec<String>,
    mut explicit: bool,
    interactive: bool,
    complete: bool,
) -> Result<(Vec<String>, bool, bool)> {
    if terms.first().is_some_and(|s| s == "--") {
        terms.remove(0);
        explicit = true;
    }
    if (interactive || complete) && terms.len() == 1 && terms[0].trim().is_empty() {
        terms.clear();
    }
    if terms.iter().any(|s| s.trim().is_empty()) {
        return Err(Error(
            2,
            "empty query argument; omit it for HOME or bare picker".into(),
        ));
    }
    let child = !explicit && terms.len() > 1 && terms.last().is_some_and(|s| s == "/");
    if child {
        terms.pop();
    }
    Ok((terms, explicit, child))
}
fn query(
    paths: &Paths,
    offline: bool,
    force_semantic: bool,
    terms: Vec<String>,
    interactive: bool,
    complete: bool,
    explicit: bool,
) -> Result<()> {
    let (terms, explicit, child) = prepare_terms(terms, explicit, interactive, complete)?;
    if !child
        && !interactive
        && !complete
        && let Some(path) = engine::direct(&terms, explicit)?
    {
        return output(&path);
    }
    if !child
        && (interactive || complete)
        && !terms.is_empty()
        && let Some(path) = engine::direct(&terms, explicit)?
    {
        return output(&pick(
            &[Candidate {
                id: "explicit".into(),
                path,
                count: 0,
                last_seen: 0,
                weight: 0.0,
            }],
            None,
        )?);
    }
    let policy_read = paths.policy_lock()?;
    let cfg = paths.load()?;
    ui::configure(&cfg.language);
    let fingerprint = paths.fingerprint()?;
    let mut db = Store::open(paths, false)?;
    let (generation, raw) = db.snapshot()?;
    let now = j_jump::now();
    let cwd = engine::logical_cwd()?;
    let tokens = engine::tokens(&terms);
    // Discovery pass in store order: filtering needs no sort, so a query that
    // matches nothing never pays to order the whole inventory (AC-002).
    let lexical = engine::local(&raw, &tokens, &cwd, child, interactive || complete);
    let policy = paths.policy(&cfg, false);
    let ranked_lexical = engine::ranked_lexical(lexical.clone(), &tokens, now);
    let query = terms.join(" ");
    // Identity binding only exists for a Jev request. Resolving every
    // policy-allowed path before this guard made a no-match query cost scale
    // with the whole inventory, so it is prepared lazily below (AC-002).
    let semantic_wanted =
        !query.trim().is_empty() && !offline && cfg.semantic && io::stdin().is_terminal();
    // Without any credential a request is impossible: route locally, as with Jev off.
    let credential_ready = ui::credential_available(&cfg);
    let semantic_request = semantic_wanted && credential_ready;
    let wants_force = (force_semantic || cfg.semantic_route == "force")
        && !complete
        && !query.trim().is_empty()
        && !offline;
    if wants_force && semantic_wanted && !credential_ready {
        return Err(Error(
            5,
            "forced Jev needs a key; add one with jjump setup or TYPESAFE_API_KEY".into(),
        ));
    }
    if wants_force && !semantic_request {
        return Err(Error(
            5,
            "forced Jev requires semantic enabled and an interactive terminal; use --offline for local navigation".into(),
        ));
    }
    let force_route = wants_force;
    let ordinary_local = !force_route && !interactive && !complete;
    let mut allowed_but_unusable = false;
    if ordinary_local {
        // Ranking uses only the snapshot. Resolve policy and filesystem state
        // in rank order, so broad matches do not stat every path to pick one.
        for candidate in &ranked_lexical {
            if !policy.allows(&candidate.path) {
                continue;
            }
            allowed_but_unusable = true;
            if let Ok(path) = engine::valid_path(&candidate.path) {
                if db.generation()? != generation {
                    return Err(Error(7, "visit snapshot changed; retry query".into()));
                }
                return output(&path);
            }
        }
    }
    // Keep cwd out of ordinary ranking, but if it is the only usable recorded
    // lexical match the query still succeeds locally. Do not turn it into a Jev miss.
    if !force_route
        && !interactive
        && !complete
        && !child
        && raw
            .iter()
            .any(|c| c.path == cwd && policy.allows(&c.path) && engine::matches(&c.path, &tokens))
    {
        if db.generation()? != generation {
            return Err(Error(7, "visit snapshot changed; retry query".into()));
        }
        return output(&cwd);
    }
    if ordinary_local && allowed_but_unusable {
        db.prune_missing(true, true)?;
        return Err(Error(
            6,
            "no matching directory is a usable destination; check permissions and names".into(),
        ));
    }
    drop(policy_read);
    if !force_route
        && !interactive
        && !complete
        && let Some(gone) = lexical.iter().find(|c| !c.path.is_dir())
    {
        db.prune_missing(true, true)?;
        let text = gone.path.to_string_lossy();
        return Err(Error(
            6,
            format!(
                "matching directory {} no longer exists; remove it with: jjump history forget --apply -- {}",
                j_jump::display(&text),
                j_jump::shell_quote(&text)
            )
            .into(),
        ));
    }
    if semantic_wanted && !credential_ready {
        eprintln!("{}", ui::missing_key_notice());
    }
    // Picker and semantic routes still need the full eligible local set.
    let local: Vec<_> = ranked_lexical
        .into_iter()
        .filter(|c| policy.allows(&c.path))
        .collect();
    let semantic_prep = Instant::now();
    // Full inventory is needed only by semantics or explicit bare local browsing.
    let needs_all = semantic_request || ((interactive || complete) && query.trim().is_empty());
    let all: Vec<Candidate> = if needs_all {
        engine::ranked(
            raw.iter()
                .filter(|c| policy.allows(&c.path))
                .filter(|c| !child || (c.path != cwd && c.path.starts_with(&cwd)))
                .cloned()
                .collect(),
            now,
        )
    } else {
        Vec::new()
    };
    let mut candidates = if semantic_request {
        engine::semantic_groups(&all, &local, &tokens, &cwd, paths, &cfg, force_route).0
    } else {
        Vec::new()
    };
    let mut local_choice = false;
    let mut suggested = None;
    let mut semantic_epoch = None;
    let mut identities: Option<std::collections::BTreeMap<PathBuf, PathBuf>> = None;
    if semantic_request && !candidates.is_empty() {
        let offered: Vec<_> = candidates.iter().flat_map(|g| g.members.clone()).collect();
        let ids = engine::identities(&offered);
        let identity_binding = j_jump::digest(
            &serde_json::to_vec(&ids)
                .map_err(|_| Error(7, "cannot bind candidate snapshot".into()))?,
        );
        identities = Some(ids);
        {
            let (payload, sent) = provider::request(&query, &candidates, &cwd, paths, &cfg)?;
            candidates.truncate(sent);
            let prep_elapsed = semantic_prep.elapsed();
            let allow=cfg.consent=="always"||ask(&format!("Send query and {} eligible directory names to Jev ({}, default network)? [y/N] ",candidates.len(),cfg.privacy))?.eq_ignore_ascii_case("y");
            if allow {
                let deadline =
                    Instant::now() + adapter::INTERACTIVE_DEADLINE.saturating_sub(prep_elapsed);
                let _policy_lock = paths.policy_lock()?;
                // Recheck consent/policy immediately before any provider/cache use.
                if paths.fingerprint()? != fingerprint || db.generation()? != generation {
                    return Err(Error(7, "state changed; retry query".into()));
                }
                let fresh_payload = provider::request(&query, &candidates, &cwd, paths, &cfg)?;
                if fresh_payload.0 != payload
                    || candidates.iter().flat_map(|g| &g.members).any(|c| {
                        c.path.canonicalize().ok().as_ref()
                            != identities.as_ref().and_then(|m| m.get(&c.path))
                    })
                {
                    return Err(Error(7, "candidate identity changed; retry query".into()));
                }
                db.db
                    .execute_batch("BEGIN")
                    .map_err(|_| Error(7, "cannot bind request to visit generation".into()))?;
                if db.generation()? != generation {
                    return Err(Error(7, "visit generation changed before request".into()));
                }
                let binding = format!(
                    "{generation}:{fingerprint}:{identity_binding}:{}",
                    j_jump::digest(cwd.as_os_str().as_encoded_bytes())
                );
                let worker_paths = paths.clone();
                let worker_cfg = cfg.clone();
                let worker_fingerprint = fingerprint.clone();
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let abort = std::sync::Arc::new(adapter::Abort::new());
                let worker_abort = abort.clone();
                let worker = std::thread::spawn(move || {
                    let result = provider::ProviderState::open(&worker_paths).and_then(|state| {
                        state.send_until(
                            &payload,
                            &binding,
                            &worker_cfg,
                            &worker_paths,
                            &worker_fingerprint,
                            deadline,
                            &worker_abort,
                        )
                    });
                    let _ = tx.send(result);
                });
                let waited = wait_ui::wait_for_jev(tty()?, rx, deadline);
                abort.abort();
                // Confirm caller-owned locks are released before opening local
                // picks or returning cancellation. HTTP completion is detached.
                let release_by = Instant::now() + std::time::Duration::from_millis(250);
                while !worker.is_finished() && Instant::now() < release_by {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                if worker.is_finished() {
                    let _ = worker.join();
                }
                match waited? {
                    Some(Ok((s, epoch))) => {
                        suggested = Some(s.ok_or(Error(3, "Jev found no reliable match; use jjump --offline query --interactive to browse locally".into()))?);
                        if suggested.is_some() {
                            semantic_epoch = Some(epoch);
                        }
                    }
                    Some(Err(e)) => return Err(e),
                    None => local_choice = true,
                }
                db.db
                    .execute_batch("COMMIT")
                    .map_err(|_| Error(7, "cannot release request snapshot".into()))?;
            } else {
                return Err(Error(
                    130,
                    "request declined; use jjump --offline query --interactive to browse locally"
                        .into(),
                ));
            }
        }
    } else if semantic_request {
        return Err(Error(
            3,
            "no eligible semantic candidates; use ordinary cd to build local history".into(),
        ));
    }
    if !interactive && !complete && suggested.is_none() && !local_choice {
        return Err(Error(
            3,
            "no supported match; try ji to select locally or cd to visit a new directory".into(),
        ));
    }
    let choices = if suggested.is_some() {
        let mut members: Vec<_> = candidates
            .iter()
            .flat_map(|g| {
                g.members.iter().cloned().map(|mut c| {
                    c.id = g.id.clone();
                    c
                })
            })
            .collect();
        members.sort_by_key(|c| Some(c.id.as_str()) != suggested.as_deref());
        members
    } else if local_choice || query.trim().is_empty() {
        all
    } else {
        local
    };
    // Snapshot identity gate. Capture the canonical identity of exactly the
    // candidates offered to the picker (a Jev request already captured the
    // wider set), then re-verify only the picked path after the interactive
    // window. This restores the "directory snapshot or policy changed"
    // protection without resolving the whole inventory (AC-002).
    if identities.is_none() {
        identities = Some(engine::identities(&choices));
    }
    let target = pick(&choices, suggested.as_deref())?;
    if let Some(epoch) = semantic_epoch {
        let _lock = paths.credential_lock(false)?;
        if paths.credential_epoch()? != epoch {
            return Err(Error(
                7,
                "credential changed before selection; retry query".into(),
            ));
        }
    }
    let current = paths.load()?;
    // Fail closed if the gate was not captured for a path that reached a pick.
    let identity_unchanged = identities
        .as_ref()
        .is_some_and(|ids| target.canonicalize().ok().as_ref() == ids.get(&target));
    if fingerprint != paths.fingerprint()?
        || generation != db.generation()?
        || !paths.allowed(&current, &target, false)
        || !identity_unchanged
    {
        return Err(Error(
            7,
            "directory snapshot or policy changed; retry selection".into(),
        ));
    }
    output(&target)
}
fn setup(paths: &Paths) -> Result<()> {
    setup_ui::run(paths)
}
fn credential_status(cfg: &Config) -> serde_json::Value {
    let environment = std::env::var_os("TYPESAFE_API_KEY").is_some_and(|s| !s.is_empty());
    json!({"source":if environment{"environment"}else{cfg.credential.as_str()},"present":if environment{Some(true)}else if cfg.credential=="system"{None}else{Some(false)},"system_store":"not accessed by offline status"})
}
fn config(paths: &Paths, cmd: ConfigCommand, offline: bool) -> Result<()> {
    match cmd {
        ConfigCommand::Show { json } => {
            let cfg = paths.load()?;
            ui::configure(&cfg.language);
            if json {
                println!("{}",serde_json::to_string_pretty(&json!({"schema_version":1,"saved":cfg,"source":if paths.config.exists(){"user config"}else{"defaults"},"effective":{"semantic":cfg.semantic&&!offline,"tracking":cfg.tracking},"overrides":{"offline":offline},"credential":credential_status(&cfg),"automatic_quality_approved":false,"provider_live_test":"not run","proxy":cfg.proxy})).unwrap());
            } else {
                println!(
                    "{}: {}",
                    ui::tr("Configuration", "配置文件"),
                    ui::short_path(&paths.config, &paths.home)
                );
                println!(
                    "{}: {}",
                    ui::tr("Source", "来源"),
                    if paths.config.exists() {
                        ui::tr("user config", "用户配置")
                    } else {
                        ui::tr("defaults", "默认值")
                    }
                );
                println!(
                    "semantic={} (effective={})\ntracking={}\nconsent={}\nprivacy={}\ncandidate_limit={}\nsemantic_route={}\nlanguage={}",
                    cfg.semantic,
                    cfg.semantic && !offline,
                    cfg.tracking,
                    cfg.consent,
                    cfg.privacy,
                    cfg.candidate_limit,
                    cfg.semantic_route,
                    cfg.language
                );
                println!(
                    "exclude={} / no_send={} ({})",
                    cfg.exclude.len(),
                    cfg.no_send.len(),
                    ui::tr("paths hidden", "路径已隐藏")
                );
                if offline {
                    println!(
                        "{}",
                        ui::tr(
                            "--offline/J_JUMP_OFFLINE affects this command only.",
                            "--offline/J_JUMP_OFFLINE 仅限制本次命令。"
                        )
                    );
                }
                if std::env::var_os("J_JUMP_LANG").is_some() {
                    println!(
                        "{}",
                        ui::tr(
                            "Display language overridden by J_JUMP_LANG.",
                            "显示语言被 J_JUMP_LANG 覆盖。"
                        )
                    );
                }
                for line in ui::readiness(&cfg, offline) {
                    println!("{line}");
                }
                println!(
                    "{}",
                    ui::tr(
                        "Edit: jjump setup or jjump config set --help",
                        "修改：jjump setup 或 jjump config set --help"
                    )
                );
            }
            Ok(())
        }
        ConfigCommand::Set { key, value } => {
            let expected = paths.fingerprint()?;
            let mut cfg = paths.load()?;
            ui::field(&mut cfg, &key, &value)?;
            paths.save(&cfg, &expected)?;
            ui::configure(&cfg.language);
            if !cfg.semantic {
                let _ = adapter::stop(paths);
            }
            println!(
                "{}",
                ui::tr(
                    "Saved; changes invalidate prior semantic decisions. Credentials were not changed.",
                    "已保存，旧语义决策已失效；凭证未改变。"
                )
            );
            Ok(())
        }
        ConfigCommand::Reset { preview: _, apply } => {
            let expected = paths.fingerprint()?;
            let old = paths.load()?;
            let cfg = Config {
                tracking: old.tracking,
                exclude: old.exclude,
                no_send: old.no_send,
                ..Config::default()
            };
            println!(
                "{}",
                ui::tr(
                    "Reset disables semantic network; retains exclusions, tracking, visits, cache and credentials.",
                    "恢复默认将关闭语义联网；保留排除规则、记录开关、访问库、缓存和凭证。"
                )
            );
            if apply {
                paths.save(&cfg, &expected)?;
                let _ = adapter::stop(paths);
            }
            println!(
                "{}",
                if apply {
                    ui::tr("Reset complete.", "已恢复默认。")
                } else {
                    ui::tr(
                        "Preview only; nothing changed. Apply with:\njjump config reset --apply",
                        "仅预览，尚未修改。执行：\njjump config reset --apply",
                    )
                }
            );
            Ok(())
        }
        ConfigCommand::Recover { apply } => {
            println!(
                "{}",
                ui::tr(
                    "Recovery disables semantic network and tracking. Visits and credentials remain.\nCorrupt content is not copied. Re-enter exclusions before enabling tracking.",
                    "恢复会关闭语义联网和访问记录，保留访问库和凭证。\n不复制可能含秘密的损坏文件。启用记录前请重新配置排除规则。"
                )
            );
            if apply {
                let expected = paths.fingerprint()?;
                paths.save(
                    &Config {
                        tracking: false,
                        ..Config::default()
                    },
                    &expected,
                )?;
                let _ = adapter::stop(paths);
            }
            println!(
                "{}",
                if apply {
                    ui::tr(
                        "Recovery complete. Tracking remains off.",
                        "恢复完成。访问记录仍关闭。",
                    )
                } else {
                    ui::tr(
                        "Preview only; nothing changed. Apply with:\njjump config recover --apply",
                        "仅预览，尚未修改。执行：\njjump config recover --apply",
                    )
                }
            );
            Ok(())
        }
    }
}
fn offline_env() -> bool {
    std::env::var("J_JUMP_OFFLINE").is_ok_and(|s| s == "1" || s == "true")
}
fn observer_command(
    action: &str,
    path: &std::path::Path,
    config: Option<&std::path::Path>,
) -> Result<std::process::Command> {
    use std::process::Command as Process;
    let mut command = Process::new(
        std::env::current_exe().map_err(|_| Error(7, "observer executable unavailable".into()))?,
    );
    command.env_clear();
    for key in [
        "HOME",
        "PATH",
        "PWD",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "J_JUMP_HOME",
        "J_JUMP_CONFIG",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    if let Some(config) = config {
        command.arg("--config").arg(config);
    }
    command.arg(action).arg("--").arg(path);
    Ok(command)
}
fn observe(path: &std::path::Path, config: Option<&std::path::Path>) -> Result<()> {
    use std::process::Stdio;
    let mut command = observer_command("observe-supervise", path, config)?;
    let supervisor = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error(7, "cannot launch observer supervisor".into()))?;
    // The shell keeps this PID until the supervisor has reaped its sole worker.
    println!("{}", supervisor.id());
    Ok(())
}
fn supervise_record(path: &std::path::Path, config: Option<&std::path::Path>) -> Result<()> {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let started = Instant::now();
    let mut command = observer_command("record", path, config)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error(7, "cannot launch record worker".into()))?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                child
                    .wait()
                    .map_err(|_| Error(7, "cannot reap observer worker".into()))?;
                return Err(Error(7, "cannot inspect observer worker".into()));
            }
        }
        if started.elapsed() >= Duration::from_millis(250) {
            let _ = child.kill();
            // The prompt already returned. Stay alive until this worker is reaped;
            // the shell guards this supervisor PID from launching another write.
            child
                .wait()
                .map_err(|_| Error(7, "cannot reap observer worker".into()))?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn explain(paths: &Paths, terms: Vec<String>, is_preview: bool, machine: bool) -> Result<()> {
    // Parse exactly as a noninteractive `query` does so both describe the same
    // candidate set for the same terms, including the trailing child-only `/`.
    let (terms, _, child) = prepare_terms(terms, false, false, false)?;
    let cfg = paths.load()?;
    let (diagnostic_store, all) = inventory(paths, &cfg)?;
    let inventory_count = diagnostic_store.list()?.len();
    let cwd = engine::logical_cwd()?;
    let tokens = engine::tokens(&terms);
    // Count only destinations `query` could actually emit: it refuses an
    // unenterable directory or an unsafe name, so diagnostics must not report a
    // match the navigation path will always reject (AC-004, AC-005, AC-010).
    fn usable(candidate: &Candidate) -> bool {
        engine::valid_path(&candidate.path).is_ok()
    }
    let local: Vec<_> = engine::ranked_lexical(
        engine::local(&all, &tokens, &cwd, child, false),
        &tokens,
        j_jump::now(),
    )
    .into_iter()
    .filter(usable)
    .collect();
    // `query` treats a matching current directory as a valid success even though
    // it is kept out of ordinary ranking. Count it here too so `explain` reports
    // the same candidate set as navigation.
    let lexical_matches = local.len()
        + usize::from(
            !child
                && all
                    .iter()
                    .any(|c| c.path == cwd && engine::matches(&c.path, &tokens) && usable(c)),
        );
    let scoped: Vec<_> = all
        .iter()
        .filter(|c| !child || (c.path != cwd && c.path.starts_with(&cwd)))
        .filter(|c| usable(c))
        .cloned()
        .collect();
    let (short, truncated) = engine::semantic_groups(
        &scoped,
        &local,
        &tokens,
        &cwd,
        paths,
        &cfg,
        cfg.semantic_route == "force",
    );
    if is_preview {
        let (bytes, _) = provider::request(&terms.join(" "), &short, &cwd, paths, &cfg)?;
        io::stdout()
            .write_all(&bytes)
            .map_err(|_| Error(7, "cannot write preview".into()))?;
        println!();
    } else if machine {
        println!(
            "{}",
            json!({"schema_version":2,"matches":local.iter().map(|c| json!({"path":c.path,"score":engine::effective_score(c,j_jump::now())})).collect::<Vec<_>>(),"score_version":2,"privacy_policy_version":1,"shortlist_version":2,"inventory":inventory_count,"lexical_matches":lexical_matches,"semantic_candidates":short.len(),"truncated":truncated,"local_first":true,"network_requests":0,"automatic_semantic":false})
        );
    } else {
        println!(
            "{} usable matches; {} semantic groups{}",
            lexical_matches,
            short.len(),
            if truncated { " (truncated)" } else { "" }
        );
        for (i, c) in local.iter().enumerate() {
            println!(
                "{}. {}{}",
                i + 1,
                ui::short_path(&c.path, &paths.home),
                if i == 0 { " [j goes here]" } else { "" }
            );
        }
        if local.is_empty() && lexical_matches > 0 {
            println!("{} [j goes here]", ui::short_path(&cwd, &paths.home));
        }
    }
    Ok(())
}
/// Diagnose a state-path rejection. `config_side` is true when the rejected
/// path is the configuration file or its parent directory (which `config
/// recover` itself reads and writes); false when it is the visit store file or
/// the data directory (`config recover` never touches those).
///
/// State-path hardening refuses a linked or non-private state *file* (and an
/// unowned/unshared state directory); it does not refuse an ordinary symbolic
/// link in the state *ancestry*. Each cause names the remedy that can actually
/// succeed in the same state: `config recover` only helps when the rejected
/// path is the configuration itself.
fn state_path_diagnosis(message: &str, config_side: bool) -> Option<(&'static str, &'static str)> {
    if message.contains("size limit") {
        Some((
            "the configuration file exceeds the size limit",
            "move the oversized configuration file aside; config recover cannot read it",
        ))
    } else if message.contains("state file") || message.contains("private state") {
        Some(if config_side {
            (
                "the configuration file is linked, not private, or not owned by you",
                "unlink the configuration file or restore it as an owned, unshared mode-0600 file; config recover reads this file and cannot replace it",
            )
        } else {
            (
                "the visit store file is linked, not private, or not owned by you",
                "unlink the store file or restore it as an owned, unshared mode-0600 file, or move it aside to start a new store; config recover does not repair the visit store",
            )
        })
    } else if message.contains("state directory") || message.contains("state parent") {
        Some(if config_side {
            (
                "the configuration directory is linked, not private, or not owned by you",
                "restore the configuration directory to an owned, unshared mode-0700 directory; config recover saves there and cannot succeed first",
            )
        } else {
            (
                "the state data directory is linked, not private, or not owned by you",
                "restore the state data directory to an owned, unshared mode-0700 directory; config recover does not repair the visit store",
            )
        })
    } else if message.contains("symlink") || message.contains("state ancestry") {
        Some((
            "state-path hardening refused a linked or non-private state file or directory",
            "replace the linked state file or restore private ownership/permissions; an ordinary symlinked ancestor is allowed",
        ))
    } else if message.contains("state paths") || message.contains("HOME must be absolute") {
        Some((
            "the state paths are not absolute",
            "set HOME and J_JUMP_HOME to absolute paths",
        ))
    } else {
        None
    }
}
fn run() -> Result<()> {
    ui::configure("auto");
    if std::env::args_os().skip(1).any(|a| {
        matches!(
            a.to_str(),
            Some(
                "--help"
                    | "-h"
                    | "help"
                    | "config"
                    | "doctor"
                    | "setup"
                    | "shell-help"
                    | "setup-if-needed"
            )
        )
    }) {
        ui::load_preferences();
    }
    let matches = ui::command(Cli::command())
        .try_get_matches()
        .unwrap_or_else(|e| {
            if e.use_stderr() && ui::tr("en", "zh") == "zh" {
                eprintln!("参数无效；请查看 --help。参数诊断如下：");
            }
            e.exit()
        });
    let cli = Cli::from_arg_matches(&matches)
        .map_err(|_| Error(2, "invalid arguments; run jjump --help".into()))?;
    if let Some(Command::Init { ref shell, ref cmd }) = cli.command {
        print!("{}", j_jump::shell::init(shell, cmd)?);
        return Ok(());
    }
    if let Some(Command::Shell { ref action }) = cli.command {
        let (options, uninstall) = match action {
            ShellCommand::Install(options) => (options, false),
            ShellCommand::Uninstall(options) => (options, true),
        };
        return j_jump::shell_install::run(
            options.shell.as_deref(),
            options.rc.as_deref(),
            &options.cmd,
            options.bin_dir.as_deref(),
            uninstall,
        );
    }
    if let Some(Command::Observe { ref path }) = cli.command {
        return observe(path, cli.config.as_deref());
    }
    if let Some(Command::ObserveSupervise { ref path }) = cli.command {
        return supervise_record(path, cli.config.as_deref());
    }
    let paths = Paths::discover(cli.config)?;
    if let Some(Command::AdapterServe) = cli.command {
        return adapter::serve(paths);
    }
    let offline = cli.offline || offline_env();
    match cli.command {
        Some(Command::Shell { .. })
        | Some(Command::Init { .. })
        | Some(Command::Observe { .. })
        | Some(Command::ObserveSupervise { .. })
        | Some(Command::AdapterServe) => unreachable!(),
        Some(Command::Adapter { action }) => {
            match action {
                AdapterCommand::Status => println!("adapter: {}", adapter::status(&paths)?),
                AdapterCommand::Stop => println!("adapter: {}", adapter::stop(&paths)?),
                AdapterCommand::Restart => {
                    let _ = adapter::stop(&paths)?;
                    println!("adapter: stopped; starts on the next authorized semantic request");
                }
            }
            Ok(())
        }
        Some(Command::ShellHelp { cmd, interactive }) => ui::shell_help(&cmd, interactive),
        Some(Command::SetupIfNeeded) => setup_ui::if_needed(&paths),
        Some(Command::Query {
            setup_if_needed,
            interactive,
            complete,
            explicit,
            terms,
        }) => {
            if setup_if_needed && !complete {
                setup_ui::if_needed(&paths)?;
            }
            query(
                &paths,
                offline,
                cli.force_semantic,
                terms,
                interactive,
                complete,
                explicit || std::env::args().any(|a| a == "--"),
            )
        }
        Some(Command::Record { path, generation }) => {
            let cfg = paths.load()?;
            if !cfg.tracking {
                return Ok(());
            }
            let _lock = paths.policy_lock()?;
            let fingerprint = paths.fingerprint()?;
            let cfg = paths.load()?;
            let mut db = Store::open(&paths, true)?;
            let initial = db.event_generation()?;
            if fingerprint != paths.fingerprint()? {
                return Err(Error(7, "policy changed during record".into()));
            }
            db.record(
                &paths,
                &cfg,
                &path,
                generation.as_deref().or(Some(&initial)),
            )
        }
        Some(Command::Setup) if io::stdin().is_terminal() => setup(&paths),
        None => {
            ui::command(Cli::command())
                .print_help()
                .map_err(|_| Error(7, "cannot write help".into()))?;
            println!();
            Ok(())
        }
        Some(Command::Setup) => Err(Error(
            4,
            "setup needs an interactive terminal; use config show/set offline".into(),
        )),
        Some(Command::Credential { action }) => match action {
            CredentialCommand::Status => {
                println!(
                    "{}",
                    json!({"schema_version":1,"environment_present":std::env::var_os("TYPESAFE_API_KEY").is_some_and(|s|!s.is_empty()),"configured_source":paths.load()?.credential,"system_entry":"not accessed; status does not unlock credential store"})
                );
                Ok(())
            }
            CredentialCommand::Set => {
                let _ = tty()?;
                let fingerprint = paths.fingerprint()?;
                let cfg = paths.load()?;
                j_jump::credential::system()?;
                let secret = read_secret()?;
                j_jump::credential::save_config(&paths, &cfg, &fingerprint, &secret)?;
                eprintln!(
                    "Stored in OS credential store. Network settings unchanged; environment override remains higher priority."
                );
                Ok(())
            }
            CredentialCommand::Delete { apply } => {
                if apply {
                    let _credential_lock = paths.credential_lock(true)?;
                    paths.rotate_credential_epoch()?;
                    j_jump::credential::delete()?;
                }
                println!(
                    "J-Jump owned OS credential entry {}. Environment and provider account key are unchanged; revoke at the provider separately.",
                    if apply {
                        "deleted"
                    } else {
                        "would be deleted; add --apply"
                    }
                );
                Ok(())
            }
        },
        Some(Command::Config { action }) => config(&paths, action, offline),
        Some(Command::Doctor { json }) => {
            let cfg = paths.load();
            // A store that opens but cannot be read is not healthy: exercise the
            // read path rather than reporting "ok" while every other command
            // demands repair (AC-011).
            let store_check: Option<Result<()>> = cfg.as_ref().ok().map(|_| {
                let db = Store::open(&paths, false)?;
                db.list()?;
                db.generation()?;
                Ok(())
            });
            let config_ok = cfg.is_ok();
            let store_ok = store_check.as_ref().is_some_and(|r| r.is_ok());
            let ready = cfg.as_ref().is_ok_and(|c| c.semantic)
                && std::env::var_os("TYPESAFE_API_KEY").is_some_and(|s| !s.is_empty())
                && !offline;
            let healthy = config_ok && store_ok;
            let config_error = cfg.as_ref().err().map(|e| e.1.as_ref());
            let store_error = store_check
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .map(|e| e.1.as_ref());
            let (cause, next): (&str, &str) = if healthy {
                (
                    "none",
                    "init your shell; use cd to accumulate visits; setup is optional",
                )
            } else if let Some((cause, next)) = config_error
                .and_then(|message| state_path_diagnosis(message, true))
                .or_else(|| store_error.and_then(|message| state_path_diagnosis(message, false)))
            {
                (cause, next)
            } else if !config_ok {
                (
                    "configuration is invalid or unsupported",
                    "preview: jjump config recover; apply: jjump config recover --apply",
                )
            } else {
                (
                    "the visit store is unreadable or malformed",
                    "config recover does not repair the store; move the unreadable store file aside so a new one can be created, or restore a backup you already hold",
                )
            };
            if json {
                println!(
                    "{}",
                    json!({"schema_version":1,"version":env!("CARGO_PKG_VERSION"),"configuration":if config_ok{"ok"}else{"invalid"},"store":if store_ok{"ok"}else{"unavailable"},"cause":cause,"provider":"jev","provider_configured":cfg.as_ref().is_ok_and(|c|c.semantic),"environment_ready":ready,"credential":cfg.as_ref().ok().map(credential_status),"provider_test":"not run","shell_activation":"not verified by child process","offline":offline,"automatic_quality_approved":false,"next":next})
                );
            } else {
                println!("J-Jump {}", env!("CARGO_PKG_VERSION"));
                println!(
                    "{}: {}",
                    ui::tr("Configuration", "配置"),
                    if config_ok {
                        ui::tr("ok", "正常")
                    } else {
                        ui::tr("invalid", "无效")
                    }
                );
                println!(
                    "{}: {}",
                    ui::tr("Visit store", "访问库"),
                    if store_ok {
                        ui::tr("ok", "正常")
                    } else {
                        ui::tr("unavailable", "不可用")
                    }
                );
                if !healthy {
                    println!("{}: {cause}", ui::tr("Cause", "原因"));
                    if ui::tr("en", "zh") == "zh" {
                        println!(
                            "{}",
                            if !config_ok {
                                "配置读取失败；若为格式错误，请先预览恢复。\n权限、所有权或链接问题须先按下方说明处理。"
                            } else {
                                "访问库不可用；config recover 只修配置，不能修复访问库。\n保留备份，再处理文件权限或从已有备份恢复。"
                            }
                        );
                    }
                }
                if let Ok(c) = &cfg {
                    for line in ui::readiness(c, offline) {
                        println!("{line}");
                    }
                }
                println!("{}: {next}", ui::tr("Next", "下一步"));
                if healthy && ui::tr("en", "zh") == "zh" {
                    println!(
                        "请在当前终端确认 Shell 接入，并用 cd 积累访问记录。\n可选设置：jjump setup"
                    );
                }
            }
            if healthy {
                Ok(())
            } else {
                Err(Error(7, next.into()))
            }
        }
        Some(Command::Explain { terms, json }) => explain(&paths, terms, false, json),
        Some(Command::Preview { terms }) => explain(&paths, terms, true, true),
        Some(Command::History { action }) => {
            let cfg = paths.load()?;
            let mut db = Store::open(&paths, false)?;
            match action {
                HistoryCommand::Backup { out } => {
                    if !out.is_absolute() || out.exists() || std::fs::symlink_metadata(&out).is_ok()
                    {
                        return Err(Error(2, "backup requires a new absolute filename".into()));
                    }
                    let parent = out
                        .parent()
                        .ok_or(Error(2, "invalid backup location".into()))?;
                    j_jump::config::private_dir(parent)?;
                    let records = db.list()?;
                    let backup = json!({"schema_version":3,"records":records});
                    j_jump::config::atomic_write(
                        &out,
                        &serde_json::to_vec(&backup)
                            .map_err(|_| Error(7, "cannot encode backup".into()))?,
                    )?;
                    println!(
                        "Private visit backup saved; contains directory names. Keep it private; deletion is not secure erasure."
                    );
                    Ok(())
                }
                HistoryCommand::Restore { file, apply } => {
                    let bytes = j_jump::config::private_read(&file, 64 * 1024 * 1024)?;
                    #[derive(serde::Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Backup {
                        schema_version: u32,
                        records: Vec<Candidate>,
                    }
                    let backup: Backup = serde_json::from_slice(&bytes)
                        .map_err(|_| Error(7, "invalid visit backup".into()))?;
                    if backup.schema_version != 3 || backup.records.len() > 10000 {
                        return Err(Error(7, "unsupported backup schema or capacity".into()));
                    }
                    let mut seen = std::collections::HashSet::new();
                    if backup.records.iter().any(|r| {
                        !r.path.is_absolute()
                            || r.path
                                .to_str()
                                .is_none_or(|s| s.contains(['\r', '\n', '\0']) || s.len() > 16384)
                            || r.count == 0
                            || r.count > 2147483647
                            || r.last_seen < 0
                            || !r.weight.is_finite()
                            || !(0.0..=2147483647.0).contains(&r.weight)
                            || !seen.insert(r.path.clone())
                    }) {
                        return Err(Error(
                            7,
                            "backup contains invalid or duplicate visit records".into(),
                        ));
                    }
                    if apply {
                        let tx = db
                            .db
                            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                            .map_err(|_| Error(7, "store busy; restore not applied".into()))?;
                        tx.execute("DELETE FROM visits", [])
                            .map_err(|_| Error(7, "restore failed".into()))?;
                        for r in &backup.records {
                            tx.execute(
                                "INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?1,?2,?3,?4,?5)",
                                rusqlite::params![
                                    r.path.to_str(),
                                    engine::key(&r.path.to_string_lossy()),
                                    r.count,
                                    r.last_seen,
                                    r.weight
                                ],
                            )
                            .map_err(|_| Error(7, "restore failed; previous store retained".into()))?;
                        }
                        tx.execute("UPDATE meta SET epoch=lower(hex(randomblob(16))),generation=0,revision=0 WHERE id=1",[]).map_err(|_|Error(7,"restore epoch failed".into()))?;
                        tx.commit()
                            .map_err(|_| Error(7, "restore commit failed".into()))?;
                    }
                    println!(
                        "{} records {}; a new epoch invalidates previous snapshots. Current privacy rules still filter restored entries.",
                        backup.records.len(),
                        if apply {
                            "restored"
                        } else {
                            "validated; add --apply to replace visits"
                        }
                    );
                    Ok(())
                }
                HistoryCommand::Prune { apply } => {
                    let n = db.prune_missing(apply, false)?;
                    println!(
                        "{n} missing visits {}",
                        if apply { "deleted" } else { "would be deleted" }
                    );
                    // Entries whose parent is also gone may sit on an unmounted drive and
                    // are kept; name them so they can be removed deliberately.
                    let kept: Vec<_> = db
                        .list()?
                        .into_iter()
                        .filter(|c| {
                            std::fs::metadata(&c.path)
                                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                                && c.path.parent().is_some_and(|parent| {
                                    std::fs::metadata(parent)
                                        .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                                })
                        })
                        .collect();
                    if !kept.is_empty() {
                        println!(
                            "{} missing visits kept because their parent folder is also missing (possibly an unmounted drive); remove one with: jjump history forget --apply -- PATH",
                            kept.len()
                        );
                        for c in kept {
                            println!("  {}", j_jump::display(&c.path.to_string_lossy()));
                        }
                    }
                    Ok(())
                }
                HistoryCommand::List => {
                    db.prune_missing(true, true)?;
                    let policy = paths.policy(&cfg, false);
                    let mut shown = false;
                    for c in db.list()?.into_iter().filter(|c| policy.allows(&c.path)) {
                        shown = true;
                        println!(
                            "{}\t{}",
                            c.count,
                            j_jump::display(&c.path.to_string_lossy())
                        );
                    }
                    if !shown && io::stderr().is_terminal() {
                        eprintln!(
                            "{}",
                            ui::tr(
                                "No visited directories yet; use ordinary cd first.",
                                "尚无可用访问记录，请先用普通 cd 访问目录。"
                            )
                        );
                    }
                    Ok(())
                }
                HistoryCommand::Clear { preview: _, apply } => {
                    let n = db.clear(None, apply)?;
                    println!(
                        "{n} visit records {}; prior semantic snapshots invalidated on apply. New visits may be recorded again. Not secure erasure.",
                        if apply { "deleted" } else { "would be deleted" }
                    );
                    Ok(())
                }
                HistoryCommand::Forget {
                    preview: _,
                    apply,
                    path,
                } => {
                    if !path.is_absolute() {
                        return Err(Error(2, "forget requires absolute logical path".into()));
                    }
                    let n = db.clear(path.to_str(), apply)?;
                    println!(
                        "{n} visit records {}; new visits may be recorded again.",
                        if apply { "deleted" } else { "would be deleted" }
                    );
                    Ok(())
                }
            }
        }
        Some(Command::Cache {
            action: ClearCommand::Clear { preview: _, apply },
        }) => {
            let state = provider::ProviderState::open(&paths)?;
            let n = state.clear(apply)?;
            println!(
                "{n} cached responses {}; visits and outage-circuit state retained. Not secure erasure.",
                if apply { "deleted" } else { "would be deleted" }
            );
            Ok(())
        }
        Some(Command::Data {
            action: ClearCommand::Clear { preview: _, apply },
        }) => {
            paths.load()?;
            let mut db = Store::open(&paths, false)?;
            let state = provider::ProviderState::open(&paths)?;
            let n = db.clear(None, apply)?;
            let m = state.clear(apply)?;
            println!(
                "{n} visits and {m} cached responses {}; credentials, config, outage-circuit state and shell/third-party data retained. Not secure erasure.",
                if apply { "deleted" } else { "would be deleted" }
            );
            Ok(())
        }
    }
}
pub(crate) fn main() {
    // Restrictive before SQLite can create journals; affects only this child.
    unsafe {
        libc::umask(0o077);
    }
    if let Err(e) = run() {
        ui::load_preferences();
        if e.0 != 130 {
            eprintln!("{}", ui::error_text(&e));
        }
        std::process::exit(e.0);
    }
}
