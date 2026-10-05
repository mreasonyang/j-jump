//! Persistent shell integration. Never source user startup files or open product state.
use crate::{Error, Result, digest, display, shell, shell_quote};
use fs2::FileExt;
use std::{
    env,
    fs::{self, File, OpenOptions, Permissions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const BEGIN: &str = "# >>> j-jump shell integration >>>\n";
const END: &str = "# <<< j-jump shell integration <<<\n";
const CHECKSUM: &str = "# j-jump block sha256: ";
const MAX_RC: u64 = 1024 * 1024;

fn error(message: impl Into<String>) -> Error {
    Error(2, message.into().into())
}
fn file_error(path: &Path, cause: impl std::fmt::Display) -> Error {
    error(format!(
        "shell file {}: {cause}",
        display(&path.to_string_lossy())
    ))
}
fn text_path(path: &Path) -> Result<&str> {
    let text = path
        .to_str()
        .ok_or_else(|| file_error(path, "path must be UTF-8"))?;
    if text.chars().any(char::is_control) {
        return Err(file_error(path, "path cannot contain control characters"));
    }
    Ok(text)
}
fn absolute(path: PathBuf) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(file_error(&path, "use an absolute path"));
    }
    text_path(&path)?;
    Ok(path)
}
fn directory(variable: &str, fallback: PathBuf) -> Result<PathBuf> {
    absolute(
        env::var_os(variable)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or(fallback),
    )
}
fn selected_shell(explicit: Option<&str>) -> Result<String> {
    let login = env::var_os("SHELL").unwrap_or_default();
    let name = explicit.or_else(|| Path::new(&login).file_name().and_then(|v| v.to_str()));
    match name {
        Some("bash" | "zsh" | "fish") => Ok(name.unwrap().to_owned()),
        _ => Err(error(
            "cannot identify Bash, Zsh or Fish; use jjump shell install --shell bash|zsh|fish",
        )),
    }
}
fn targets(shell: &str, rc: Option<&Path>, home: &Path) -> Result<Vec<(PathBuf, bool)>> {
    if let Some(rc) = rc {
        return Ok(vec![(absolute(rc.to_path_buf())?, false)]);
    }
    match shell {
        "zsh" => Ok(vec![(
            directory("ZDOTDIR", home.to_path_buf())?.join(".zshrc"),
            false,
        )]),
        "fish" => Ok(vec![(
            directory("XDG_CONFIG_HOME", home.join(".config"))?.join("fish/config.fish"),
            false,
        )]),
        "bash" => {
            // Bash login terminals read the first existing profile, while ordinary
            // interactive terminals read .bashrc. Keep both entry points connected.
            let profile = [".bash_profile", ".bash_login", ".profile"]
                .into_iter()
                .map(|name| home.join(name))
                .find(|path| fs::symlink_metadata(path).is_ok())
                .unwrap_or_else(|| home.join(".bash_profile"));
            Ok(vec![(home.join(".bashrc"), false), (profile, true)])
        }
        _ => unreachable!(),
    }
}
fn resolved_target(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            // Edit the real dotfile, keeping a user's dotfiles symlink intact.
            fs::canonicalize(path).map_err(|e| file_error(path, e))
        }
        Ok(_) => Ok(path.to_path_buf()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(e) => Err(file_error(path, e)),
    }
}

#[derive(Clone)]
struct Snapshot {
    bytes: Option<Vec<u8>>,
    identity: Option<(u64, u64, u32)>,
    permissions: Option<Permissions>,
}
fn snapshot(path: &Path) -> Result<Snapshot> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Snapshot {
                bytes: None,
                identity: None,
                permissions: None,
            });
        }
        Err(e) => return Err(file_error(path, e)),
    };
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > MAX_RC {
        return Err(file_error(
            path,
            "expected a regular file with one link, no larger than 1 MiB",
        ));
    }
    let bytes = fs::read(path).map_err(|e| file_error(path, e))?;
    Ok(Snapshot {
        bytes: Some(bytes),
        identity: Some((metadata.dev(), metadata.ino(), metadata.mode())),
        permissions: Some(metadata.permissions()),
    })
}
fn unchanged(path: &Path, expected: &Snapshot) -> Result<()> {
    let current = snapshot(path)?;
    if current.identity != expected.identity || current.bytes != expected.bytes {
        return Err(file_error(
            path,
            "changed during shell integration; retry after finishing your edit",
        ));
    }
    Ok(())
}
fn managed_range(text: &str) -> Result<Option<std::ops::Range<usize>>> {
    if !text.contains(BEGIN.trim_end()) && !text.contains(END.trim_end()) {
        return Ok(None);
    }
    if text.matches(BEGIN).count() != 1 || text.matches(END).count() != 1 {
        return Err(error(
            "shell integration markers are incomplete or duplicated; inspect the startup file manually",
        ));
    }
    let begin = text.find(BEGIN).unwrap();
    let end = text.find(END).unwrap();
    if begin == 0 || !text[..begin].ends_with('\n') || end < begin + BEGIN.len() {
        return Err(error(
            "shell integration block was edited; preserve it and inspect the startup file manually",
        ));
    }
    let contents = &text[begin + BEGIN.len()..end];
    let (checksum, body) = contents
        .split_once('\n')
        .ok_or_else(|| error("shell integration block lacks a checksum"))?;
    if checksum.strip_prefix(CHECKSUM) != Some(digest(body.as_bytes()).as_str()) {
        return Err(error(
            "shell integration block was edited; nothing was removed or overwritten",
        ));
    }
    Ok(Some(begin - 1..end + END.len()))
}
fn managed_block(body: &str) -> String {
    format!(
        "\n{BEGIN}{CHECKSUM}{}\n{body}{END}",
        digest(body.as_bytes())
    )
}
fn active_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}
fn manual_init(text: &str, shell: &str) -> bool {
    active_lines(text).any(|line| {
        let activation = if shell == "fish" {
            line.contains("| source")
        } else {
            line.starts_with("eval ")
        };
        activation
            && ["jjump", "j-jump"]
                .iter()
                .any(|binary| line.contains(&format!("{binary} init {shell}")))
    })
}
fn declares_command(text: &str, name: &str) -> bool {
    active_lines(text).any(|line| {
        let words: Vec<_> = line.split_whitespace().collect();
        let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        compact.starts_with(&format!("alias{name}="))
            || compact.starts_with(&format!("{name}()"))
            || (words.first() == Some(&"function")
                && words
                    .get(1)
                    .is_some_and(|word| word.trim_end_matches(['(', ')', '{']) == name))
    })
}
fn executable(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
fn path_command(name: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| executable(&dir.join(name))))
}
fn binary_directory(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = explicit {
        let dir = absolute(dir.to_path_buf())?;
        let expected = env::current_exe().map_err(|e| error(e.to_string()))?;
        if !executable(&dir.join("jjump"))
            || fs::canonicalize(dir.join("jjump")).ok() != fs::canonicalize(expected).ok()
        {
            return Err(file_error(&dir, "jjump must be this running executable"));
        }
        return Ok(dir);
    }
    let current = env::current_exe().map_err(|e| error(e.to_string()))?;
    let identity = fs::canonicalize(&current).map_err(|e| file_error(&current, e))?;
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths).filter(|dir| dir.is_absolute()) {
            if executable(&dir.join("jjump"))
                && fs::canonicalize(dir.join("jjump")).ok().as_ref() == Some(&identity)
            {
                // Prefer the stable Homebrew bin symlink over a versioned Cellar path.
                return absolute(dir);
            }
        }
    }
    let dir = current
        .parent()
        .ok_or_else(|| error("cannot locate the jjump binary directory"))?;
    if !executable(&dir.join("jjump")) {
        return Err(error("install jjump before connecting your shell"));
    }
    absolute(dir.to_path_buf())
}
fn fish_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}
fn startup_body(shell: &str, name: &str, bin: &Path) -> Result<String> {
    let dir = text_path(bin)?;
    if dir.contains(':') {
        return Err(file_error(
            bin,
            "binary directory cannot contain a colon because it must be added to PATH",
        ));
    }
    let binary = dir.to_owned() + "/jjump";
    if shell == "fish" {
        let (dir, binary, name) = (fish_quote(dir), fish_quote(&binary), fish_quote(name));
        Ok(format!(
            "if test -x {binary}\n    if not test \"$PATH[1]\" = {dir}\n        set -gx PATH {dir} $PATH\n    end\n    {binary} init fish --cmd {name} | source\nend\n"
        ))
    } else {
        let (dir, binary, name) = (shell_quote(dir), shell_quote(&binary), shell_quote(name));
        Ok(format!(
            "if [ -x {binary} ]; then\n    case \"$PATH\" in\n        {dir}|{dir}:*) ;;\n        *) export PATH={dir}:\"$PATH\" ;;\n    esac\n    eval \"$({binary} init {shell} --cmd {name})\"\nfi\n"
        ))
    }
}
fn login_body(home: &Path) -> Result<String> {
    let rc = shell_quote(text_path(&home.join(".bashrc"))?);
    Ok(format!(
        "if [ -n \"${{BASH_VERSION-}}\" ] && [ -r {rc} ]; then\n    . {rc}\nfi\n"
    ))
}

struct Edit {
    path: PathBuf,
    before: Snapshot,
    after: Vec<u8>,
}
fn draft(
    path: &Path,
    body: &str,
    shell: &str,
    name: &str,
    login: bool,
    uninstall: bool,
) -> Result<Option<Edit>> {
    let before = snapshot(path)?;
    let original = before.bytes.as_deref().unwrap_or_default();
    let text = std::str::from_utf8(original)
        .map_err(|_| file_error(path, "startup file is not UTF-8; use manual integration"))?;
    let range = managed_range(text).map_err(|e| file_error(path, e))?;
    if !uninstall && !login {
        let outside = match &range {
            Some(range) => format!("{}{}", &text[..range.start], &text[range.end..]),
            None => text.to_owned(),
        };
        if manual_init(&outside, shell) && range.is_none() {
            println!(
                "Existing shell integration preserved: {}",
                display(&path.to_string_lossy())
            );
            return Ok(None);
        }
        if [name.to_owned(), format!("{name}i")]
            .iter()
            .any(|command| declares_command(&outside, command) || path_command(command))
        {
            return Err(file_error(
                path,
                format!(
                    "command conflict; choose another prefix with jjump shell install --shell {shell} --cmd NAME (for example --cmd jump)"
                ),
            ));
        }
    }
    let after = if uninstall {
        let Some(range) = range else {
            return Ok(None);
        };
        format!("{}{}", &text[..range.start], &text[range.end..])
    } else if let Some(range) = range {
        format!(
            "{}{}{}",
            &text[..range.start],
            managed_block(body),
            &text[range.end..]
        )
    } else {
        if login
            && active_lines(text).any(|line| {
                line.contains(".bashrc") && (line.contains("source ") || line.contains(". "))
            })
        {
            println!(
                "Existing shell integration preserved: {}",
                display(&path.to_string_lossy())
            );
            return Ok(None);
        }
        format!("{text}{}", managed_block(body))
    };
    if after.as_bytes() == original {
        return Ok(None);
    }
    Ok(Some(Edit {
        path: path.to_path_buf(),
        before,
        after: after.into_bytes(),
    }))
}
fn unique_file(path: &Path, label: &str) -> Result<(PathBuf, File)> {
    let name = path
        .file_name()
        .ok_or_else(|| file_error(path, "expected a file name"))?
        .to_string_lossy();
    for attempt in 0..16 {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let unique = path.with_file_name(format!(
            "{name}.j-jump-{label}-{stamp}-{}-{attempt}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&unique)
        {
            Ok(file) => return Ok((unique, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(file_error(&unique, e)),
        }
    }
    Err(file_error(path, "cannot create a unique backup or draft"))
}
fn atomic_write(path: &Path, bytes: &[u8], before: &Snapshot) -> Result<()> {
    let (temporary, mut file) = unique_file(path, "draft")?;
    let result = (|| {
        if let Some(permissions) = &before.permissions {
            file.set_permissions(permissions.clone())
                .map_err(|e| file_error(path, e))?;
        }
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| file_error(path, e))?;
        unchanged(path, before)?;
        fs::rename(&temporary, path).map_err(|e| file_error(path, e))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn apply(edits: &[Edit]) -> Result<()> {
    let mut locks = Vec::new();
    let mut parents: Vec<_> = edits.iter().filter_map(|edit| edit.path.parent()).collect();
    parents.sort();
    parents.dedup();
    for parent in parents {
        fs::create_dir_all(parent).map_err(|e| file_error(parent, e))?;
        let lock = File::open(parent).map_err(|e| file_error(parent, e))?;
        lock.lock_exclusive().map_err(|e| file_error(parent, e))?;
        locks.push(lock);
    }
    for edit in edits {
        unchanged(&edit.path, &edit.before)?;
    }
    let mut backups = Vec::new();
    // Finish every backup before the first startup file changes.
    for edit in edits {
        if let Some(bytes) = &edit.before.bytes {
            let (path, mut file) = unique_file(&edit.path, "backup")?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| file_error(&path, e))?;
            backups.push(path);
        }
    }
    for (index, edit) in edits.iter().enumerate() {
        let result = unchanged(&edit.path, &edit.before)
            .and_then(|_| atomic_write(&edit.path, &edit.after, &edit.before));
        if let Err(cause) = result {
            // Roll back only our own unchanged writes if a later file fails.
            for previous in edits[..index].iter().rev() {
                if fs::read(&previous.path).ok().as_deref() == Some(previous.after.as_slice()) {
                    if let Some(bytes) = &previous.before.bytes {
                        let current = snapshot(&previous.path)?;
                        atomic_write(&previous.path, bytes, &current)?;
                    } else {
                        fs::remove_file(&previous.path)
                            .map_err(|e| file_error(&previous.path, e))?;
                    }
                }
            }
            return Err(cause);
        }
    }
    for path in backups {
        println!("Backup: {}", display(&path.to_string_lossy()));
    }
    Ok(())
}

pub fn run(
    explicit_shell: Option<&str>,
    rc: Option<&Path>,
    name: &str,
    bin_dir: Option<&Path>,
    uninstall: bool,
) -> Result<()> {
    let shell = selected_shell(explicit_shell)?;
    shell::init(&shell, name)?;
    let home = directory("HOME", PathBuf::new())?;
    let bin = if uninstall {
        None
    } else {
        Some(binary_directory(bin_dir)?)
    };
    let mut edits = Vec::new();
    let mut seen = Vec::new();
    for (target, login) in targets(&shell, rc, &home)? {
        let path = resolved_target(&target)?;
        if seen.contains(&path) {
            return Err(file_error(
                &path,
                "multiple Bash startup files resolve to one file; choose one with --rc",
            ));
        }
        seen.push(path.clone());
        let body = if uninstall {
            String::new()
        } else if login {
            login_body(&home)?
        } else {
            startup_body(&shell, name, bin.as_ref().unwrap())?
        };
        if let Some(edit) = draft(&path, &body, &shell, name, login, uninstall)? {
            edits.push(edit);
        }
    }
    apply(&edits)?;
    for edit in &edits {
        println!(
            "{}: {}",
            if uninstall {
                "Shell integration removed"
            } else {
                "Shell connected"
            },
            display(&edit.path.to_string_lossy())
        );
    }
    if edits.is_empty() {
        println!("Shell configuration is unchanged.");
    }
    println!(
        "Open a new terminal {}.",
        if uninstall {
            "to finish removing the integration"
        } else {
            "to use J-Jump; first j/ji opens setup"
        }
    );
    Ok(())
}
