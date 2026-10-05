//! Optional, fixed-protocol fzf adapter. Display text is never path authority.
use crate::{Error, Result, engine::Candidate};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Restore {
    fd: libc::c_int,
    mode: libc::termios,
}
impl Drop for Restore {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.mode) };
    }
}
fn records(candidates: &[Candidate], suggested: Option<&str>) -> Vec<Vec<u8>> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{i}\t{}{}",
                crate::display(&c.path.to_string_lossy()),
                if Some(c.id.as_str()) == suggested {
                    " [Jev suggestion]"
                } else {
                    ""
                }
            )
            .into_bytes()
        })
        .collect()
}
fn selected(output: &[u8], rows: &[Vec<u8>]) -> Result<usize> {
    let row = output.strip_suffix(&[0]).ok_or(Error(
        130,
        "invalid picker output; directory unchanged".into(),
    ))?;
    rows.iter().position(|r| r == row).ok_or(Error(
        130,
        "invalid picker output; directory unchanged".into(),
    ))
}
/// Explicit fzf selection. Every helper failure is an error, never a mode switch.
pub fn fuzzy(
    tty: &File,
    candidates: &[Candidate],
    suggested: Option<&str>,
    chinese: bool,
) -> Result<PathBuf> {
    let unavailable = || {
        Error(
            5,
            "fzf unavailable or failed; choose numbered mode with J_JUMP_PICKER=numbered ji".into(),
        )
    };
    if matches!(std::env::var("TERM").as_deref(), Ok("dumb") | Ok("")) {
        return Err(unavailable());
    }
    let mut mode = std::mem::MaybeUninit::uninit();
    if unsafe { libc::tcgetattr(tty.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
        return Err(unavailable());
    }
    let mode: libc::termios = unsafe { mode.assume_init() };
    let _restore = Restore {
        fd: tty.as_raw_fd(),
        mode,
    };
    // Raw completion widgets can otherwise deliver Ctrl-C to the parent editor too.
    if mode.c_lflag & libc::ICANON == 0 {
        let mut raw = mode;
        raw.c_lflag &= !libc::ISIG;
        if unsafe { libc::tcsetattr(tty.as_raw_fd(), libc::TCSANOW, &raw) } != 0 {
            return Err(unavailable());
        }
    }
    let rows = records(candidates, suggested);
    let executable = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path).find_map(|dir| {
            use std::os::unix::fs::PermissionsExt;
            let file = dir.join("fzf");
            let metadata = file.metadata().ok()?;
            (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
                .then(|| file.canonicalize().ok())
                .flatten()
        })
    });
    let Some(executable) = executable else {
        return Err(unavailable());
    };
    let mut cmd = Command::new(executable);
    // No implicit option files, preview/execute bindings, default commands,
    // credentials, proxy or provider environment enter the child process.
    cmd.env_clear();
    for key in ["TERM", "COLORTERM", "LANG", "LC_ALL", "LC_CTYPE"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.args([
        "--read0",
        "--print0",
        "--no-multi",
        "--no-select-1",
        "--no-exit-0",
        "--no-height",
        "--layout=reverse",
        "--border=none",
        "--no-mouse",
        "--keep-right",
        "--scheme=path",
        "--tiebreak=index",
        "--delimiter=\t",
        "--with-nth=2..",
        if chinese {
            "--prompt=目录> "
        } else {
            "--prompt=Directory> "
        },
        if chinese {
            "--header=输入筛选 | 上下/翻页 | Enter选择 | Esc取消"
        } else {
            "--header=Type to filter | Up/Down/PgUp/PgDn | Enter select | Esc cancel"
        },
        "--bind=ctrl-c:abort,ctrl-d:abort,esc:abort,tab:down,shift-tab:up",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::from(
        tty.try_clone()
            .map_err(|_| Error(130, "terminal closed".into()))?,
    ));
    if let Some(id) = suggested
        && let Some(candidate) = candidates.iter().find(|c| c.id == id)
    {
        let name = candidate
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        cmd.arg(format!(
            "--header=Jev suggestion: {} | Enter select | Esc cancel",
            crate::display(&name)
        ));
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => return Err(unavailable()),
    };
    let mut input = child.stdin.take().expect("piped picker input");
    let mut output = child.stdout.take().expect("piped picker output");
    let max = rows.iter().map(Vec::len).max().unwrap_or(0) + 1;
    let (bytes, status) = std::thread::scope(|scope| {
        let input_rows = &rows;
        let writer = scope.spawn(move || {
            for row in input_rows {
                input.write_all(row)?;
                input.write_all(&[0])?;
            }
            Ok::<_, std::io::Error>(())
        });
        let mut bytes = Vec::new();
        let read = output
            .by_ref()
            .take((max + 1) as u64)
            .read_to_end(&mut bytes);
        if read.is_err() || bytes.len() > max {
            let _ = child.kill();
        }
        let status = child.wait();
        let _ = writer.join();
        (bytes, status)
    });
    match status {
        Ok(s) if s.code() == Some(0) => Ok(candidates[selected(&bytes, &rows)?].path.clone()),
        Ok(s)
            if s.code() == Some(130)
                || (s.code().is_none() && s.signal() == Some(libc::SIGINT)) =>
        {
            Err(Error(
                130,
                "selection cancelled; directory unchanged".into(),
            ))
        }
        _ => Err(unavailable()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_is_not_authority() {
        let candidates = vec![Candidate {
            id: "x".into(),
            path: "/tmp/evil\x1b]52;clipboard\x07\t\u{202e}".into(),
            count: 0,
            last_seen: 0,
            weight: 0.0,
        }];
        let rows = records(&candidates, Some("x"));
        assert!(!rows[0].contains(&27));
        assert_eq!(rows[0].iter().filter(|b| **b == b'\t').count(), 1);
        let mut good = rows[0].clone();
        good.push(0);
        assert_eq!(selected(&good, &rows).unwrap(), 0);
        assert!(selected(b"0\tforged\0", &rows).is_err());
        good.extend_from_slice(&rows[0]);
        good.push(0);
        assert!(selected(&good, &rows).is_err());
    }
}
