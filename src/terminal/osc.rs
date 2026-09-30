//! Shell integration sequences that the emulator ignores, picked out of the output as it streams in:
//! OSC 7 (the shell's working directory, sent by fish, many distributions' bash, Ronnie's zsh...) and
//! OSC 133 (prompt and command marks, FinalTerm / iTerm2 / kitty style).

use std::time::{Duration, Instant};

/// Longer sequences are not ours (or are broken): their content is dropped.
const MAX_LEN: usize = 4096;
/// Longest command line kept for a notification.
const MAX_COMMAND: usize = 200;

/// What the shell reported so far.
#[derive(Default)]
pub struct Shell {
    /// The shell marks its prompts and commands (OSC 133): command ends come from it instead of
    /// watching the foreground program.
    pub integrated: bool,
    /// Last directory reported with OSC 7 (a path on the machine running the shell).
    pub cwd: Option<String>,
    /// The command started and not finished yet: when, and its command line if sent.
    running: Option<(Instant, Option<String>)>,
    /// Commands finished since last taken.
    pub finished: Vec<Finished>,
    /// At the prompt: the line typed left of the cursor, and whether the cursor is at its end (Ronnie's
    /// zsh sends it as it changes).
    pub input: Option<(String, bool)>,
    /// The shell sends what is typed (Ronnie's zsh does, bash can't).
    pub reports_input: bool,
    /// Waiting at its prompt (marked), no command running.
    pub at_prompt: bool,
}

/// A command that ran to its end.
#[derive(Clone, Debug)]
pub struct Finished {
    /// Its command line (or program name), when known.
    pub command: Option<String>,
    /// Its exit status, when the shell sent it.
    pub code: Option<i32>,
    pub duration: Duration,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum State {
    #[default]
    Ground,
    Esc,
    Osc,
    /// ESC inside an OSC: "\" ends it, anything else starts another escape sequence.
    OscEsc,
}

/// Finds OSC sequences in a byte stream, even when split between two reads.
#[derive(Default)]
pub struct Scanner {
    state: State,
    buf: Vec<u8>,
    overflow: bool,
}

impl Scanner {
    /// Scans a chunk of output, updating `shell` with the sequences found.
    pub fn scan(&mut self, bytes: &[u8], shell: &mut Shell) {
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match self.state {
                State::Ground => match memchr(0x1b, &bytes[i..]) {
                    Some(at) => {
                        i += at;
                        self.state = State::Esc;
                    }
                    None => return,
                },
                State::Esc => {
                    self.state = match b {
                        b']' => {
                            self.buf.clear();
                            self.overflow = false;
                            State::Osc
                        }
                        0x1b => State::Esc,
                        _ => State::Ground,
                    }
                }
                State::Osc => match b {
                    0x07 => {
                        self.finish(shell);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::OscEsc,
                    // CAN, SUB: sequence cancelled.
                    0x18 | 0x1a => self.state = State::Ground,
                    _ if self.buf.len() < MAX_LEN => self.buf.push(b),
                    _ => self.overflow = true,
                },
                State::OscEsc => {
                    if b == b'\\' {
                        self.finish(shell);
                        self.state = State::Ground;
                    } else {
                        // Another escape sequence begins with that ESC: read this byte again after it.
                        self.state = State::Esc;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    fn finish(&mut self, shell: &mut Shell) {
        if self.overflow {
            return;
        }
        let text = String::from_utf8_lossy(&self.buf);
        let mut parts = text.split(';');
        match parts.next() {
            Some("7") => {
                if let Some(path) = parts.next().and_then(file_url_path) {
                    shell.cwd = Some(path);
                }
            }
            Some("1337") => {
                if let Some(url) = parts.next().and_then(|p| p.strip_prefix("RonnieInput=")) {
                    let end = parts.any(|p| p == "end=1");
                    shell.input = Some((percent_decode(url), end));
                    shell.reports_input = true;
                }
            }
            Some("133") => {
                shell.integrated = true;
                match parts.next() {
                    // A new prompt: nothing typed yet.
                    Some("A") => {
                        shell.input = None;
                        shell.at_prompt = true;
                    }
                    // Command started (the user pressed Enter).
                    Some("C") => {
                        shell.input = None;
                        shell.at_prompt = false;
                        let command = parts.find_map(|p| {
                            p.strip_prefix("cmdline_url=").map(|u| percent_decode(u)).or_else(|| p.strip_prefix("cmdline=").map(str::to_owned))
                        });
                        let command = command.map(|c| c.trim().chars().take(MAX_COMMAND).collect::<String>()).filter(|c| !c.is_empty());
                        shell.running = Some((Instant::now(), command));
                    }
                    // Command finished, with its exit status. Without a start mark (Enter on an empty
                    // line) there is nothing to report.
                    Some("D") => {
                        if let Some((start, command)) = shell.running.take() {
                            let code = parts.next().and_then(|c| c.trim().parse().ok());
                            shell.finished.push(Finished { command, code, duration: start.elapsed() });
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// Path of a `file://host/path` URL (or kitty's `kitty-shell-cwd://`), decoded.
fn file_url_path(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    // After the host (possibly empty).
    let path = &rest[rest.find('/')?..];
    Some(percent_decode(path))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(hi << 4 | lo);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A Windows path from an OSC 7 URL's path ("/C:/Users/me" → "C:\\Users\\me").
pub fn windows_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    let path = if drive { &path[1..] } else { path };
    let mut out = path.replace('/', "\\");
    // "C:" alone is the drive's current directory, not its root.
    if out.len() == 2 && drive {
        out.push('\\');
    }
    out
}

/// Working directory shown in a window title of the form `user@host: path` (the default prompt of
/// Debian and Ubuntu's bash sets it so), for shells that don't send OSC 7.
pub fn title_path(title: &str) -> Option<&str> {
    let (who, path) = title.split_once(':')?;
    let (user, host) = who.split_once('@')?;
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    let path = path.trim_start();
    (word(user) && word(host) && (path.starts_with('/') || path == "~" || path.starts_with("~/"))).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Shell {
        let (mut scanner, mut shell) = (Scanner::default(), Shell::default());
        for chunk in chunks {
            scanner.scan(chunk, &mut shell);
        }
        shell
    }

    #[test]
    fn reads_windows_paths() {
        assert_eq!(windows_path("/C:/Users/Jo%20B"), "C:\\Users\\Jo%20B");
        assert_eq!(windows_path("/D:"), "D:\\");
        assert_eq!(scan(&[b"\x1b]7;file://PC/C:/Users/Jo%20B\x07"]).cwd.as_deref().map(windows_path).as_deref(), Some("C:\\Users\\Jo B"));
    }

    #[test]
    fn reads_cwd_across_chunks() {
        let shell = scan(&[b"hello \x1b]7;file://mac.local/Users/bob/My%20", b"Docs\x1b", b"\\ $ "]);
        assert_eq!(shell.cwd.as_deref(), Some("/Users/bob/My Docs"));
        assert!(!shell.integrated);
    }

    #[test]
    fn reads_command_marks() {
        let shell = scan(&[b"\x1b]133;A\x07$ ", b"\x1b]133;C;cmdline_url=npm%20run%20build\x07out\r\n", b"\x1b]133;D;2\x07\x1b]133;A\x07"]);
        assert!(shell.integrated);
        assert_eq!(shell.finished.len(), 1);
        assert_eq!(shell.finished[0].command.as_deref(), Some("npm run build"));
        assert_eq!(shell.finished[0].code, Some(2));
    }

    #[test]
    fn knows_when_at_the_prompt() {
        assert!(!scan(&[b"loading..."]).at_prompt);
        assert!(scan(&[b"\x1b]133;A\x07$ "]).at_prompt);
        assert!(!scan(&[b"\x1b]133;A\x07$ ", b"\x1b]133;C;cmdline_url=npm\x07"]).at_prompt, "a command runs");
    }

    #[test]
    fn ignores_end_without_start() {
        let shell = scan(&[b"\x1b]133;D;0\x07\x1b]133;D\x07"]);
        assert!(shell.finished.is_empty());
    }

    #[test]
    fn other_escapes_do_not_confuse_it() {
        // An OSC cut by another escape sequence, colors, a title.
        let shell = scan(&[b"\x1b]7;file://h/a\x1b[31mred\x1b]0;title\x07\x1b]7;file://h/b\x07"]);
        assert_eq!(shell.cwd.as_deref(), Some("/b"));
    }

    #[test]
    fn reads_title_paths() {
        assert_eq!(title_path("bob@web-1: /var/www"), Some("/var/www"));
        assert_eq!(title_path("root@srv:~/app"), Some("~/app"));
        assert_eq!(title_path("ubuntu@ip-10-0-0-1: ~"), Some("~"));
        assert_eq!(title_path("vim README.md"), None);
        assert_eq!(title_path("Note: bob@x"), None);
        assert_eq!(title_path("bob@srv: htop"), None);
    }
}
