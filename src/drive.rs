//! Ronnie driven by Claude: commands Claude runs in a pane (`ronnie panes`, `ronnie run`, `ronnie
//! mails`…). Those reading what Ronnie keeps on disk (notes, mails caught) answer by themselves; the
//! others ask the Ronnie whose pane they run in: an `.ask` file left in its events folder (see
//! `claude::pane_env`), answered in `answers/<id>.json` beside, which the command waits for and prints.
//! What would change the user's view is only proposed (a notice to accept), never done.

use std::io::Read as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// What Claude asks the Ronnie it runs in. Panes are named by their number (`ronnie panes`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "do", rename_all = "kebab-case")]
pub enum Ask {
    /// The windows, tabs, panes and profiles.
    Panes,
    /// A file in the editor beside the terminals (of the asking pane's tab), proposed.
    Open { path: PathBuf },
    /// A pane split off the asking one, running `command`.
    Run { command: String, cwd: Option<PathBuf>, down: bool, name: Option<String> },
    /// A new tab, in the background.
    Tab { cwd: Option<PathBuf>, command: Option<String>, name: Option<String> },
    /// A line typed in a pane, then Return.
    Type { pane: u64, text: String },
    Cd { pane: u64, dir: PathBuf },
    /// The last lines a pane shows.
    Output { pane: u64, lines: usize },
    /// The Node.js version of a pane (None: the system's).
    Node { pane: u64, version: Option<String> },
    NodeInstall { version: String },
    Rename { pane: u64, name: Option<String> },
    Close { pane: u64 },
    /// A view of Ronnie proposed to the user.
    Show { what: View },
    Profile(NewProfile),
    /// A saved profile opened (in the background).
    OpenProfile { name: String },
}

/// What can be shown: a pane (its tab), the notes (one of them), the mails caught, the Dev page.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    Pane(u64),
    Notes,
    Note(String),
    Mails,
    Mail(String),
    Dev,
}

/// A profile to save: its panes side by side (or one under the other), each in its folder.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NewProfile {
    pub name: String,
    /// "#rrggbb".
    #[serde(default)]
    pub color: Option<String>,
    pub panes: Vec<NewPane>,
    /// One under the other rather than side by side.
    #[serde(default)]
    pub down: bool,
    /// Opened right away (in the background).
    #[serde(default)]
    pub open: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NewPane {
    pub cwd: PathBuf,
    #[serde(default)]
    pub name: Option<String>,
    /// Typed as the pane starts, each time the profile opens (one command a line).
    #[serde(default)]
    pub startup: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
}

/// An ask as left for Ronnie: its id (the answer's file), and the pane asking (its RONNIE_PANE).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Asked {
    pub id: String,
    pub pane: String,
    pub ask: Ask,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Answer {
    pub ok: bool,
    pub text: String,
}

/// Answers `asked` (in the events folder of this Ronnie).
pub fn answer(asked: &Asked, ok: bool, text: impl Into<String>) {
    let Some(dir) = crate::claude::events_dir() else { return };
    if !asked.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return;
    }
    if let Ok(json) = serde_json::to_string(&Answer { ok, text: text.into() }) {
        let _ = crate::claude::write_atomic(&dir.join("answers").join(format!("{}.json", asked.id)), &json);
    }
}

/// Leaves `ask` for the Ronnie of this pane and waits for its answer.
fn ask(ask: Ask) -> Result<String, String> {
    let (Some(dir), Ok(pane)) = (std::env::var_os("RONNIE_CLAUDE_EVENTS"), std::env::var("RONNIE_PANE")) else {
        return Err("not in a pane of Ronnie".into());
    };
    let dir = PathBuf::from(dir);
    let id = uuid::Uuid::new_v4().to_string();
    let json = serde_json::to_string(&Asked { id: id.clone(), pane, ask }).map_err(|e| e.to_string())?;
    crate::claude::write_atomic(&dir.join(format!("{id}.ask")), &json).map_err(|e| e.to_string())?;
    let reply = dir.join("answers").join(format!("{id}.json"));
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if let Some(answer) = std::fs::read_to_string(&reply).ok().and_then(|t| serde_json::from_str::<Answer>(&t).ok()) {
            let _ = std::fs::remove_file(&reply);
            return if answer.ok { Ok(answer.text) } else { Err(answer.text) };
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(dir.join(format!("{id}.ask")));
    Err("Ronnie didn't answer (its window may be minimized or busy)".into())
}

pub const USAGE: &str = "\
Ronnie, driven from one of its panes (pane numbers: see `ronnie panes`):
  ronnie panes                          windows, tabs, panes (folder, program, Node.js), profiles
  ronnie run [--down] [--name N] [--cwd DIR] <command…>
                                        split this pane, the new one running <command>
  ronnie tab [--name N] [--cwd DIR] [<command…>]
                                        a new tab, in the background
  ronnie output <pane> [--lines N]      the last lines a pane shows (60)
  ronnie type <pane> <text…>            type a line in a pane, then Return
  ronnie cd <pane> <dir>                change a pane's folder
  ronnie node                           Node.js versions installed
  ronnie node <pane> <version|system>   a pane's Node.js version, from its next command
  ronnie node available                 the newest version of each line on nodejs.org (LTS marked)
  ronnie node install <latest|lts|22|vX.Y.Z>
                                        install a Node.js version (the newest matching)
  ronnie rename <pane> [name]           name a pane (no name: unnamed)
  ronnie close <pane>                   close a pane
  ronnie show <pane|notes|note ID|mails|mail ID|dev>
                                        propose a view to the user (a notice to accept)
  ronnie open <file>                    propose a file in the editor beside the terminals
  ronnie profile < profile.json         save a profile: {\"name\", \"color\"?: \"#rrggbb\", \"down\"?, \"open\"?,
                                        \"panes\": [{\"cwd\", \"name\"?, \"startup\"?, \"node\"?}]}
  ronnie profile open <name>            open a saved profile (in the background)
  ronnie note [--folder F] [file]       add a Markdown note (else from stdin)
  ronnie notes [words…]                 the notes (matching all the words): id, folder, date, title
  ronnie notes show <id>                a note's Markdown
  ronnie mails [words…] [--limit N]     the mails caught (Dev > test mailbox), newest first
  ronnie mails show <id|last> [--html | --source]
                                        a mail: headers, text (or its HTML, or as it came)";

/// The commands of this module: None when `ronnie` was started for something else.
pub fn run_cli() -> Option<Result<(), String>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, rest) = args.split_first()?;
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    let result = match command.as_str() {
        "panes" => ask(Ask::Panes),
        "run" => run(&rest),
        "tab" => tab(&rest),
        "output" => pane_arg(&rest).and_then(|(pane, rest)| {
            let lines = match rest {
                ["--lines", n] => n.parse().map_err(|_| "--lines: a number".to_owned())?,
                [] => 60,
                _ => return Err(USAGE.to_owned()),
            };
            ask(Ask::Output { pane, lines })
        }),
        "type" => pane_arg(&rest).and_then(|(pane, text)| if text.is_empty() { Err(USAGE.into()) } else { ask(Ask::Type { pane, text: text.join(" ") }) }),
        "cd" => match pane_arg(&rest) {
            Ok((pane, [dir])) => absolute(dir).and_then(|dir| ask(Ask::Cd { pane, dir })),
            Ok(_) => Err(USAGE.into()),
            Err(e) => Err(e),
        },
        "node" => match rest.as_slice() {
            [] => Ok(installed_node()),
            ["install", spec] => resolve_node(spec).and_then(|version| ask(Ask::NodeInstall { version })),
            ["available"] => available_node(),
            _ => match pane_arg(&rest) {
                Ok((pane, [version])) => ask(Ask::Node { pane, version: (*version != "system").then(|| (*version).to_owned()) }),
                Ok(_) => Err(USAGE.into()),
                Err(e) => Err(e),
            },
        },
        "rename" => pane_arg(&rest).and_then(|(pane, name)| ask(Ask::Rename { pane, name: (!name.is_empty()).then(|| name.join(" ")) })),
        "close" => match pane_arg(&rest) {
            Ok((pane, [])) => ask(Ask::Close { pane }),
            Ok(_) => Err(USAGE.into()),
            Err(e) => Err(e),
        },
        "show" => view(&rest).and_then(|what| ask(Ask::Show { what })),
        "open" => match rest.as_slice() {
            [path] => absolute(path).and_then(|path| ask(Ask::Open { path })),
            _ => Err(USAGE.into()),
        },
        "profile" => match rest.as_slice() {
            ["open", name @ ..] if !name.is_empty() => ask(Ask::OpenProfile { name: name.join(" ") }),
            [] => profile(),
            _ => Err(USAGE.into()),
        },
        "notes" => notes(&rest),
        "mails" => mails(&rest),
        _ => return None,
    };
    Some(result.map(|text| {
        if !text.is_empty() {
            println!("{}", text.trim_end());
        }
    }))
}

fn pane_arg<'a>(rest: &'a [&'a str]) -> Result<(u64, &'a [&'a str]), String> {
    let (pane, rest) = rest.split_first().ok_or(USAGE)?;
    let pane = pane.trim_start_matches('#').parse().map_err(|_| format!("{pane}: a pane's number (see `ronnie panes`)"))?;
    Ok((pane, rest))
}

/// A path as given, made absolute from this folder.
fn absolute(path: &str) -> Result<PathBuf, String> {
    let path = crate::ssh::expand_home(std::path::Path::new(path));
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(std::env::current_dir().map_err(|e| e.to_string())?.join(path))
}

/// `--down`, `--name N`, `--cwd DIR` then the command's words.
fn options<'a>(rest: &'a [&'a str]) -> Result<(bool, Option<String>, Option<PathBuf>, &'a [&'a str]), String> {
    let (mut down, mut name, mut cwd, mut k) = (false, None, None, 0);
    while k < rest.len() {
        match rest[k] {
            "--down" => down = true,
            "--name" => {
                k += 1;
                name = Some(rest.get(k).ok_or("--name: which name?")?.to_string());
            }
            "--cwd" => {
                k += 1;
                cwd = Some(absolute(rest.get(k).ok_or("--cwd: which folder?")?)?);
            }
            "--" => {
                k += 1;
                break;
            }
            _ => break,
        }
        k += 1;
    }
    Ok((down, name, cwd, &rest[k..]))
}

fn run(rest: &[&str]) -> Result<String, String> {
    let (down, name, cwd, command) = options(rest)?;
    if command.is_empty() {
        return Err(USAGE.into());
    }
    ask(Ask::Run { command: command.join(" "), cwd, down, name })
}

fn tab(rest: &[&str]) -> Result<String, String> {
    let (_, name, cwd, command) = options(rest)?;
    let cwd = match cwd {
        Some(cwd) => Some(cwd),
        None => std::env::current_dir().ok(),
    };
    ask(Ask::Tab { cwd, command: (!command.is_empty()).then(|| command.join(" ")), name })
}

fn view(rest: &[&str]) -> Result<View, String> {
    Ok(match rest {
        ["notes"] => View::Notes,
        ["note", id] => View::Note((*id).to_owned()),
        ["mails"] => View::Mails,
        ["mail", id] => View::Mail(mail_id(id)?),
        ["dev"] => View::Dev,
        [_] => View::Pane(pane_arg(rest)?.0),
        _ => return Err(USAGE.into()),
    })
}

fn profile() -> Result<String, String> {
    let mut text = String::new();
    std::io::stdin().take(1 << 20).read_to_string(&mut text).map_err(|e| e.to_string())?;
    let mut profile: NewProfile = serde_json::from_str(&text).map_err(|e| format!("the profile's JSON: {e}"))?;
    if profile.name.trim().is_empty() || profile.panes.is_empty() {
        return Err("a profile needs a name and a pane at least".into());
    }
    for pane in &mut profile.panes {
        pane.cwd = absolute(&pane.cwd.to_string_lossy())?;
    }
    ask(Ask::Profile(profile))
}

fn installed_node() -> String {
    let installed = crate::node::installed();
    if installed.is_empty() {
        "No Node.js version installed in Ronnie (`ronnie node install vX.Y.Z`).".into()
    } else {
        format!("Node.js versions installed in Ronnie: {}", installed.join(", "))
    }
}

/// An exact version for `spec`: as given (vX.Y.Z), else the newest on nodejs.org that is "latest",
/// "lts", or of that line ("22", "v22", "22.11").
fn resolve_node(spec: &str) -> Result<String, String> {
    let spec = spec.trim();
    let v = if spec.starts_with('v') { spec.to_owned() } else { format!("v{spec}") };
    if v.split('.').count() == 3 {
        return Ok(v);
    }
    let releases = crate::node::releases().map_err(|e| format!("{e:#}"))?;
    let found = match spec.to_lowercase().as_str() {
        "latest" | "current" => releases.first(),
        "lts" => releases.iter().find(|r| r.lts.is_some()),
        _ => releases.iter().find(|r| r.version.starts_with(&format!("{v}."))),
    };
    found.map(|r| r.version.clone()).ok_or_else(|| format!("no Node.js {spec} on nodejs.org"))
}

/// The newest version of each of the last lines, from nodejs.org.
fn available_node() -> Result<String, String> {
    let releases = crate::node::releases().map_err(|e| format!("{e:#}"))?;
    let installed = crate::node::installed();
    let mut lines = Vec::new();
    let mut majors = Vec::new();
    for r in &releases {
        let major = crate::node::parse(&r.version).0;
        if majors.contains(&major) {
            continue;
        }
        majors.push(major);
        let mut line = format!("{}  {}", r.version, r.date);
        if let Some(lts) = &r.lts {
            line.push_str(&format!("  LTS {lts}"));
        }
        if installed.contains(&r.version) {
            line.push_str("  (installed)");
        }
        lines.push(line);
        if lines.len() == 8 {
            break;
        }
    }
    Ok(format!("{}
Installed in Ronnie: {}", lines.join("\n"), if installed.is_empty() { "none".into() } else { installed.join(", ") }))
}

/// The notes as saved (a change made in the last seconds may not be yet).
#[derive(Deserialize, Default)]
#[serde(default)]
struct NotesFile {
    folders: Vec<NoteFolder>,
    notes: Vec<NoteEntry>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NoteFolder {
    id: String,
    name: String,
    parent: Option<String>,
    deleted: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NoteEntry {
    id: String,
    folder: Option<String>,
    text: String,
    updated: i64,
    trashed: Option<i64>,
}

fn notes(rest: &[&str]) -> Result<String, String> {
    let path = crate::config::config_dir().ok_or("no config folder")?.join("notes.json");
    let file: NotesFile = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(_) => NotesFile::default(),
    };
    let live: Vec<&NoteEntry> = file.notes.iter().filter(|n| n.trashed.is_none()).collect();
    if let ["show", id] = rest {
        let found: Vec<&&NoteEntry> = live.iter().filter(|n| n.id.starts_with(id)).collect();
        return match found.as_slice() {
            [note] => Ok(note.text.clone()),
            [] => Err(format!("no note {id}")),
            _ => Err(format!("{id}: several notes, give more of the id")),
        };
    }
    let folder_path = |id: &Option<String>| -> String {
        let mut names = Vec::new();
        let mut at = id.clone();
        while let Some(f) = at.as_ref().and_then(|id| file.folders.iter().find(|f| &f.id == id && !f.deleted)) {
            names.insert(0, f.name.clone());
            at = f.parent.clone();
            if names.len() > 20 {
                break;
            }
        }
        if names.is_empty() { "Notes".into() } else { names.join(" / ") }
    };
    let words: Vec<String> = rest.iter().map(|w| w.to_lowercase()).collect();
    let mut found: Vec<&&NoteEntry> = live.iter().filter(|n| {
        let hay = format!("{}\n{}", folder_path(&n.folder), n.text).to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    }).collect();
    found.sort_by_key(|n| std::cmp::Reverse(n.updated));
    if found.is_empty() {
        return Ok("No note.".into());
    }
    let lines: Vec<String> = found.iter().map(|n| {
        let title = n.text.lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("");
        format!("{}  {}  [{}]  {}", &n.id[..8.min(n.id.len())], date(n.updated / 1000), folder_path(&n.folder), title)
    }).collect();
    Ok(format!("{}\n(`ronnie notes show <id>` for one)", lines.join("\n")))
}

fn date(secs: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local.timestamp_opt(secs, 0).single().map_or_else(String::new, |d| d.format("%Y-%m-%d %H:%M").to_string())
}

fn mail_id(id: &str) -> Result<String, String> {
    if id != "last" {
        return Ok(id.to_owned());
    }
    let store = crate::mailbox::Store::load();
    let store = store.lock().unwrap();
    store.mails.first().map(|m| m.id.clone()).ok_or_else(|| "no mail caught".to_owned())
}

fn mails(rest: &[&str]) -> Result<String, String> {
    if let ["show", id, how @ ..] = rest {
        let id = mail_id(id)?;
        let raw = crate::mailbox::raw(&id).ok_or_else(|| format!("no mail {id}"))?;
        if how == ["--source"] {
            return Ok(String::from_utf8_lossy(&raw).into_owned());
        }
        let message = mail_parser::MessageParser::default().parse(&raw).ok_or("unreadable mail")?;
        let addresses = |a: Option<&mail_parser::Address>| crate::mailbox::format_address(a).join(", ");
        if how == ["--html"] {
            return message.body_html(0).map(|h| h.into_owned()).ok_or_else(|| "no HTML in this mail".into());
        }
        if !how.is_empty() {
            return Err(USAGE.into());
        }
        let mut out = format!(
            "From: {}\nTo: {}\nCc: {}\nSubject: {}\nDate: {}\n",
            addresses(message.from()),
            addresses(message.to()),
            addresses(message.cc()),
            message.subject().unwrap_or_default(),
            message.date().map(|d| d.to_rfc3339()).unwrap_or_default(),
        );
        let attachments: Vec<String> = message.attachments().map(|a| {
            use mail_parser::MimeHeaders as _;
            format!("{} ({} bytes)", a.attachment_name().unwrap_or("?"), a.contents().len())
        }).collect();
        if !attachments.is_empty() {
            out.push_str(&format!("Attachments: {}\n", attachments.join(", ")));
        }
        out.push_str(&format!("HTML: {}\n\n", if message.body_html(0).is_some() { "yes (`--html`)" } else { "no" }));
        out.push_str(&message.body_text(0).unwrap_or_default());
        return Ok(out);
    }
    let (mut words, mut limit, mut k) = (Vec::new(), 20, 0);
    while k < rest.len() {
        if rest[k] == "--limit" {
            k += 1;
            limit = rest.get(k).and_then(|n| n.parse().ok()).ok_or("--limit: a number")?;
        } else {
            words.push(rest[k].to_lowercase());
        }
        k += 1;
    }
    let store = crate::mailbox::Store::load();
    let store = store.lock().unwrap();
    let found: Vec<String> = store.mails.iter().filter(|m| m.matches(&words)).take(limit).map(|m| {
        let clip = if m.attachments > 0 { format!("  📎{}", m.attachments) } else { String::new() };
        format!("{}  {}  {}  →  {}  {}{clip}", m.id, date(m.received), m.from, m.to.join(", "), m.subject)
    }).collect();
    if found.is_empty() {
        return Ok("No mail caught.".into());
    }
    Ok(format!("{}\n(`ronnie mails show <id>` for one)", found.join("\n")))
}

/// Text without its colors (the terminal's dump).
pub fn plain(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_round_trip() {
        let ask = Ask::Run { command: "npm run dev".into(), cwd: None, down: true, name: Some("dev".into()) };
        let json = serde_json::to_string(&ask).unwrap();
        assert!(json.contains("\"do\":\"run\""));
        assert_eq!(serde_json::from_str::<Ask>(&json).unwrap(), ask);
        let show = serde_json::to_string(&Ask::Show { what: View::Pane(3) }).unwrap();
        assert_eq!(serde_json::from_str::<Ask>(&show).unwrap(), Ask::Show { what: View::Pane(3) });
    }

    #[test]
    fn strips_colors() {
        assert_eq!(plain(b"\x1b[0;1;32mok\x1b[0m done\r\n"), "ok done\n");
    }
}
