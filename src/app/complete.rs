//! Path suggestions in local terminals: the word being typed at the zsh prompt (sent by Ronnie's shell
//! integration) is completed from the files and folders it names, shown under the cursor. On Windows,
//! the line typed at PowerShell's prompt (read on the screen), with its paths and escapes.

use std::path::{Path, PathBuf};

/// What can complete the word being typed.
#[derive(Debug, PartialEq)]
pub(super) struct Suggestions {
    /// The folder listed.
    pub dir: PathBuf,
    /// The part of the name already typed (unescaped).
    pub prefix: String,
}

/// The last word of `line` (what is left of the cursor), unescaped, if it looks like a path to
/// complete: an argument (not the command name, unless it has a "/"), without quotes, variables or
/// patterns. Nothing typed yet after `cd` or `pushd`: the folder's names, all of them.
fn last_word(line: &str) -> Option<String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                word.push(chars.next()?);
                in_word = true;
            }
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            // A new command: what follows is a command name again.
            ';' | '|' | '&' | '(' | ')' => {
                if in_word {
                    word.clear();
                    in_word = false;
                }
                words.clear();
            }
            '\'' | '"' | '$' | '`' | '*' | '?' | '[' | '{' => return None,
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    // Nothing typed yet in this word.
    if !in_word || word.is_empty() {
        return (words.len() == 1 && matches!(words[0].as_str(), "cd" | "pushd")).then(String::new);
    }
    // A command name: only a path to a program (./script, bin/tool).
    if words.is_empty() && !word.contains('/') {
        return None;
    }
    // Options.
    if word.starts_with('-') {
        return None;
    }
    Some(word)
}

/// Where to look, and the name's start, for `line` typed in `cwd`.
pub(super) fn parse(line: &str, cwd: &Path, home: &Path) -> Option<Suggestions> {
    let word = last_word(line)?;
    let (head, prefix) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word.as_str()),
    };
    let dir = if let Some(rest) = head.strip_prefix("~/") {
        home.join(rest)
    } else if head.starts_with('/') {
        PathBuf::from(head)
    } else if head.is_empty() && prefix == "~" {
        return None;
    } else {
        cwd.join(head)
    };
    Some(Suggestions { dir, prefix: prefix.to_owned() })
}

/// The last word of a PowerShell `line`, unescaped (` escapes), if it looks like a path to complete
/// (as `last_word`).
fn last_word_windows(line: &str) -> Option<String> {
    let mut words = 0;
    let mut first = String::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '`' => {
                word.push(chars.next()?);
                in_word = true;
            }
            ' ' | '\t' => {
                if in_word {
                    words += 1;
                    if words == 1 {
                        first = word.to_lowercase();
                    }
                    word.clear();
                    in_word = false;
                }
            }
            ';' | '|' | '&' | '(' | ')' | '{' | '}' => {
                word.clear();
                in_word = false;
                words = 0;
            }
            '\'' | '"' | '$' | '*' | '?' | '[' => return None,
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if !in_word || word.is_empty() {
        return (words == 1 && matches!(first.as_str(), "cd" | "pushd" | "sl" | "set-location")).then(String::new);
    }
    if word.starts_with('-') {
        return None;
    }
    // A command name: only a path to a program (.\script.ps1).
    if words == 0 && !word.contains(['\\', '/']) {
        return None;
    }
    Some(word)
}

/// Like `parse`, for PowerShell on Windows (folders separated by "\" or "/").
pub(super) fn parse_windows(line: &str, cwd: &Path, home: &Path) -> Option<Suggestions> {
    let word = last_word_windows(line)?;
    let (head, prefix) = match word.rfind(['\\', '/']) {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word.as_str()),
    };
    let dir = if let Some(rest) = head.strip_prefix("~\\").or_else(|| head.strip_prefix("~/")) {
        home.join(rest)
    } else if head.is_empty() && prefix == "~" {
        return None;
    } else {
        // An absolute path ("C:\", "\") replaces the folder it is joined to.
        cwd.join(head)
    };
    Some(Suggestions { dir, prefix: prefix.to_owned() })
}

/// Like `parse`, for a server (paths with "/", whatever this computer uses): the folder to list,
/// and the name's start. `cwd` and `home` are the server's.
pub(super) fn parse_remote(line: &str, cwd: &str, home: &str) -> Option<(String, String)> {
    let word = last_word(line)?;
    let (head, prefix) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word.as_str()),
    };
    let join = |a: &str, b: &str| if a.ends_with('/') { format!("{a}{b}") } else { format!("{a}/{b}") };
    let dir = if head == "~/" || head.starts_with("~/") {
        join(home, &head[2..])
    } else if head.starts_with('/') {
        head.to_owned()
    } else if head.is_empty() && prefix == "~" {
        return None;
    } else {
        join(cwd, head)
    };
    Some((dir, prefix.to_owned()))
}

/// Names of `entries` (name, is a folder) completing `prefix`: hidden ones only when asked for with a
/// ".", folders first, at most `max`. A file already typed in full is not suggested.
/// `ignore_case`: on Windows, whose names don't tell cases apart.
pub(super) fn matching<'a>(entries: &'a [(String, bool)], prefix: &str, max: usize, ignore_case: bool) -> Vec<&'a (String, bool)> {
    let lower = prefix.to_lowercase();
    let starts = |name: &str| if ignore_case { name.to_lowercase().starts_with(&lower) } else { name.starts_with(prefix) };
    let same = |name: &str| if ignore_case { name.to_lowercase() == lower } else { name == prefix };
    let mut out: Vec<&(String, bool)> = entries
        .iter()
        .filter(|(name, is_dir)| starts(name) && (prefix.starts_with('.') || !name.starts_with('.')) && (*is_dir || !same(name)) && !name.chars().any(char::is_control))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    out.truncate(max);
    out
}

/// What to type to complete `name` after `prefix`: the rest of the name, escaped for the shell, and a
/// "/" for folders.
pub(super) fn completion(name: &str, prefix: &str, is_dir: bool) -> String {
    let rest = &name[prefix.len()..];
    let mut out = String::new();
    for c in rest.chars() {
        if !(c.is_alphanumeric() || "._+-@%,:=/".contains(c)) {
            out.push('\\');
        }
        out.push(c);
    }
    if is_dir {
        out.push('/');
    }
    out
}

/// Like `completion`, for PowerShell: special characters escaped with "`", and a "\\" for folders.
pub(super) fn completion_windows(name: &str, prefix: &str, is_dir: bool) -> String {
    let mut out = String::new();
    for c in name.chars().skip(prefix.chars().count()) {
        if c.is_whitespace() || "`'\"$(){}[];,&|@#".contains(c) {
            out.push('`');
        }
        out.push(c);
    }
    if is_dir {
        out.push('\\');
    }
    out
}

/// Entries of a local folder: (name, is a folder, links to folders included).
pub(super) fn list(dir: &Path) -> Vec<(String, bool)> {
    std::fs::read_dir(dir)
        .map(|d| d.flatten().take(5000).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir())).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_word_to_complete() {
        let (cwd, home) = (Path::new("/work"), Path::new("/home/me"));
        let s = |line: &str| parse(line, cwd, home);
        assert_eq!(s("cd Doc"), Some(Suggestions { dir: "/work/".into(), prefix: "Doc".into() }));
        assert_eq!(s("ls ~/Proj"), Some(Suggestions { dir: "/home/me/".into(), prefix: "Proj".into() }));
        assert_eq!(s("vim src/app/ed"), Some(Suggestions { dir: "/work/src/app/".into(), prefix: "ed".into() }));
        assert_eq!(s("cat /etc/ho"), Some(Suggestions { dir: "/etc/".into(), prefix: "ho".into() }));
        assert_eq!(s("cd My\\ Doc"), Some(Suggestions { dir: "/work/".into(), prefix: "My Doc".into() }));
        assert_eq!(s("cd src/"), Some(Suggestions { dir: "/work/src/".into(), prefix: String::new() }));
        assert_eq!(s("./scr"), Some(Suggestions { dir: "/work/./".into(), prefix: "scr".into() }), "a program in a folder");
        assert_eq!(s("./scripts/re"), Some(Suggestions { dir: "/work/./scripts/".into(), prefix: "re".into() }));
        assert_eq!(s("git"), None);
        assert_eq!(s("ls "), None);
        assert_eq!(s("cd "), Some(Suggestions { dir: "/work".into(), prefix: String::new() }), "after cd, without a \"/\"");
        assert_eq!(s("cd src "), None);
        assert_eq!(s("ls -la"), None);
        assert_eq!(s("echo \"a b"), None);
        assert_eq!(s("ls $HOME/x"), None);
        assert_eq!(s("make && cd bu"), Some(Suggestions { dir: "/work/".into(), prefix: "bu".into() }));
        assert_eq!(s("cat a | gre"), None);
    }

    #[test]
    fn finds_the_server_folder() {
        assert_eq!(parse_remote("cd /et", "/home/p", "/root"), Some(("/".into(), "et".into())));
        assert_eq!(parse_remote("vim conf/ng", "/home/p", "/root"), Some(("/home/p/conf/".into(), "ng".into())));
        assert_eq!(parse_remote("ls ~/.ss", "/tmp", "/root"), Some(("/root/".into(), ".ss".into())));
        assert_eq!(parse_remote("cat va", "/", "/root"), Some(("/".into(), "va".into())));
    }

    #[test]
    fn suggests_and_escapes() {
        let entries = vec![("Documents".to_owned(), true), ("Doc 2.txt".to_owned(), false), (".config".to_owned(), true), ("Do".to_owned(), false)];
        let names: Vec<&str> = matching(&entries, "Do", 6, false).iter().map(|e| e.0.as_str()).collect();
        assert_eq!(names, ["Documents", "Doc 2.txt"]);
        assert_eq!(matching(&entries, ".c", 6, false).len(), 1);
        assert!(matching(&entries, "do", 6, false).is_empty());
        assert_eq!(matching(&entries, "do", 6, true).len(), 2, "Windows: any case");
        assert_eq!(completion("Doc 2.txt", "Do", false), "c\\ 2.txt");
        assert_eq!(completion("Documents", "Do", true), "cuments/");
        assert_eq!(completion("l'été (1)", "", false), "l\\'été\\ \\(1\\)");
        assert_eq!(completion_windows("My Documents", "my", true), "` Documents\\");
        assert_eq!(completion_windows("l'été (1).txt", "l", false), "`'été` `(1`).txt");
    }

    #[test]
    fn finds_the_powershell_word() {
        let (cwd, home) = (Path::new("/work"), Path::new("/home/me"));
        let s = |line: &str| parse_windows(line, cwd, home);
        assert_eq!(s("cd Doc"), Some(Suggestions { dir: "/work/".into(), prefix: "Doc".into() }));
        assert_eq!(s("cd src\\ap"), Some(Suggestions { dir: "/work/src\\".into(), prefix: "ap".into() }));
        assert_eq!(s("ls ~\\Proj"), Some(Suggestions { dir: "/home/me/".into(), prefix: "Proj".into() }));
        assert_eq!(s("cd My` Doc"), Some(Suggestions { dir: "/work/".into(), prefix: "My Doc".into() }));
        assert_eq!(s(".\\scr"), Some(Suggestions { dir: "/work/.\\".into(), prefix: "scr".into() }));
        assert_eq!(s("git"), None);
        assert_eq!(s("Set-Location "), Some(Suggestions { dir: "/work".into(), prefix: String::new() }));
        assert_eq!(s("ls "), None);
        assert_eq!(s("ls -Force"), None);
        assert_eq!(s("cd $HOME\\x"), None);
        assert_eq!(s("cd 'a b"), None);
    }
}
