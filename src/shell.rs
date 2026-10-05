use crate::{Error, Result};
pub fn init(shell: &str, name: &str) -> Result<String> {
    if [
        "jjump", "j-jump", "cd", "if", "then", "else", "elif", "fi", "for", "while", "case",
        "esac", "do", "done", "select", "until", "function", "time", "coproc", "in", "begin",
        "end", "switch", "and", "or", "not", "return",
    ]
    .contains(&name)
        || name.starts_with("__jj_")
        || name.is_empty()
        || !name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
    {
        return Err(Error(
            2,
            "command name must be an ASCII identifier, not a shell keyword, jjump, cd or __jj_ internal name".into(),
        ));
    }
    let template = match shell {
        "bash" => include_str!("../shell/bash.sh"),
        "zsh" => include_str!("../shell/zsh.zsh"),
        "fish" => include_str!("../shell/fish.fish"),
        _ => return Err(Error(2, "supported shells: bash, zsh, fish".into())),
    };
    Ok(template.replace("__JJ_CMD__", name))
}
