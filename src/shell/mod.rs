//! Shell integration: each local terminal gets its own command history (↑ and the history search),
//! kept across restarts in `<config dir>/history/<id>`.
//!
//! zsh: ZDOTDIR points to Ronnie's startup files, which load the user's usual ones (~/.zshenv,
//! ~/.zprofile, ~/.zshrc, ~/.zlogin) and then switch HISTFILE to the terminal's file.
//! bash: HISTFILE is set in the environment (kept unless ~/.bashrc overrides it).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;
use uuid::Uuid;

const ZSH_FILES: [(&str, &str); 4] = [
    (".zshenv", include_str!("zshenv")),
    (".zprofile", include_str!("zprofile")),
    (".zshrc", include_str!("zshrc")),
    (".zlogin", include_str!("zlogin")),
];

fn history_dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("history"))
}

/// File holding the command history of the terminal `id`.
pub fn history_path(id: Uuid) -> Option<PathBuf> {
    history_dir().map(|d| d.join(id.to_string()))
}

/// The user's login shell, as the pseudo-terminal starts it.
fn user_shell() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    Path::new(&shell).file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Makes the shell started by `cmd` keep its history in `history`.
pub fn use_history(cmd: &mut CommandBuilder, history: &Path) {
    match user_shell().as_deref() {
        Some("zsh") => {
            let Some(dir) = crate::config::config_dir().map(|d| d.join("shell").join("zsh")) else { return };
            if write_files(&dir).is_err() {
                return;
            }
            let user_dir = std::env::var_os("ZDOTDIR").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
            cmd.env("RONNIE_USER_ZDOTDIR", user_dir);
            cmd.env("ZDOTDIR", &dir);
            cmd.env("RONNIE_HISTFILE", history);
        }
        Some("bash") => cmd.env("HISTFILE", history),
        _ => {}
    }
}

/// Writes the zsh startup files, only when they changed.
fn write_files(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, content) in ZSH_FILES {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(content) {
            std::fs::write(&path, content)?;
        }
    }
    Ok(())
}

/// Commands of a history file, most recent first, without duplicates. Understands zsh's extended
/// format (`: <time>:<duration>;command`), its multi-line entries and its "metafied" bytes.
pub fn read_history(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    // zsh escapes some bytes as 0x83 followed by the byte xor 32.
    let mut raw = Vec::with_capacity(bytes.len());
    let mut it = bytes.into_iter();
    while let Some(b) = it.next() {
        raw.push(if b == 0x83 { it.next().map_or(b, |n| n ^ 32) } else { b });
    }
    let text = String::from_utf8_lossy(&raw);

    let mut entries: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if current.is_empty() {
            let command = match line.strip_prefix(": ").and_then(|l| l.split_once(';')) {
                Some((_, command)) => command,
                None => line,
            };
            current.push_str(command);
        } else {
            current.push('\n');
            current.push_str(line);
        }
        // A trailing backslash continues the entry on the next line.
        if current.ends_with('\\') {
            current.pop();
            continue;
        }
        let entry = std::mem::take(&mut current);
        if !entry.trim().is_empty() {
            entries.push(entry);
        }
    }
    let mut seen = HashSet::new();
    entries.into_iter().rev().filter(|e| seen.insert(e.clone())).collect()
}

/// Deletes the history of terminals that no longer exist (not in `keep`).
pub fn forget_others(keep: &HashSet<Uuid>) {
    let Some(entries) = history_dir().and_then(|d| std::fs::read_dir(d).ok()) else { return };
    for entry in entries.flatten() {
        let id = entry.file_name().to_str().and_then(|n| Uuid::parse_str(n).ok());
        if id.is_some_and(|id| !keep.contains(&id)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_zsh_history() {
        let path = std::env::temp_dir().join(format!("ronnie-hist-{}", std::process::id()));
        std::fs::write(&path, b": 1700000000:0;git pull\n: 1700000001:0;npm run dev\nls\n: 1700000002:0;echo a\\\nb\n: 1700000003:0;git pull\n").unwrap();
        assert_eq!(read_history(&path), ["git pull", "echo a\nb", "ls", "npm run dev"]);
        std::fs::remove_file(&path).unwrap();
    }
}
