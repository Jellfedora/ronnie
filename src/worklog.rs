//! What the user works on with Claude, across the panes: each Claude Code session in a Ronnie pane
//! notes its project (the git repository, or the folder) and the first thing asked in it; a session
//! starting elsewhere is told of the other projects, roughly (names, when, a few subjects), not their
//! details.
//!
//! Kept in the user's own Ronnie folder, in a file of its own: never sent anywhere, nor part of the
//! configuration (exported, or someday synced). Each user knows only their own projects.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Subjects kept a project.
const TOPICS: usize = 3;
/// Projects told of at most, and how old the latest may be.
const TOLD: usize = 8;
const TOLD_DAYS: i64 = 30;
/// Projects kept at most (the oldest go).
const KEPT: usize = 40;
/// A subject's length at most, in characters.
const TOPIC_LEN: usize = 110;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Worklog {
    #[serde(default)]
    pub projects: Vec<Project>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    /// The repository's (or the folder's) path.
    pub path: String,
    /// Unix seconds.
    pub last: i64,
    /// The newest first.
    #[serde(default)]
    pub topics: Vec<Topic>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Topic {
    pub session: String,
    pub text: String,
}

fn path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("claude-worklog.json"))
}

fn load() -> Worklog {
    path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(log: &Worklog) {
    if let (Some(path), Ok(text)) = (path(), serde_json::to_string_pretty(log)) {
        let _ = crate::claude::write_atomic(&path, &text);
    }
}

/// The project of folder `cwd`: the git repository it is in, or the folder itself.
pub fn project_of(cwd: &Path) -> PathBuf {
    cwd.ancestors().find(|d| d.join(".git").exists()).unwrap_or(cwd).to_path_buf()
}

impl Worklog {
    fn project(&mut self, root: &Path, now: i64) -> &mut Project {
        let key = root.display().to_string();
        let at = match self.projects.iter().position(|p| p.path == key) {
            Some(at) => at,
            None => {
                self.projects.push(Project { path: key, last: now, topics: Vec::new() });
                self.projects.len() - 1
            }
        };
        let project = &mut self.projects[at];
        project.last = now;
        project
    }

    /// A session asked something in `root`: its first meaningful request is the session's subject.
    pub fn prompted(&mut self, root: &Path, session: &str, prompt: &str, now: i64) {
        let project = self.project(root, now);
        if let Some(text) = subject(prompt)
            && !session.is_empty()
            && !project.topics.iter().any(|t| t.session == session)
        {
            project.topics.insert(0, Topic { session: session.to_owned(), text });
            project.topics.truncate(TOPICS);
        }
        self.forget_old();
    }

    /// A session started in `root`.
    pub fn started(&mut self, root: &Path, now: i64) {
        self.project(root, now);
        self.forget_old();
    }

    fn forget_old(&mut self) {
        self.projects.sort_by_key(|p| std::cmp::Reverse(p.last));
        self.projects.truncate(KEPT);
    }

    /// What a session in `root` is told: the other projects lately, the latest first. None when
    /// there are none.
    pub fn others(&self, root: &Path, home: Option<&Path>, now: i64) -> Option<String> {
        let key = root.display().to_string();
        let lines: Vec<String> = self
            .projects
            .iter()
            .filter(|p| p.path != key && now - p.last <= TOLD_DAYS * 86_400)
            .take(TOLD)
            .map(|p| {
                let path = Path::new(&p.path);
                let name = path.file_name().map_or_else(|| p.path.clone(), |n| n.to_string_lossy().into_owned());
                let shown = match home.and_then(|h| path.strip_prefix(h).ok()) {
                    Some(rest) => format!("~/{}", rest.display()),
                    None => p.path.clone(),
                };
                let mut line = format!("- {name} ({shown}), {}", ago(now - p.last));
                if !p.topics.is_empty() {
                    let topics: Vec<String> = p.topics.iter().map(|t| format!("\"{}\"", t.text)).collect();
                    line.push_str(&format!(": {}", topics.join("; ")));
                }
                line
            })
            .collect();
        (!lines.is_empty()).then(|| {
            format!(
                "Other projects the user worked on lately with Claude, in other Ronnie panes (the latest first; quoted, the first \
request of a few sessions, in the user's words). For context only: the user works on several things; mention one only when \
it bears on the task (shared code, a project it depends on…), and don't look into them unasked.\n{}",
                lines.join("\n")
            )
        })
    }
}

/// A request as a subject: on one line, short; none for a command (`/clear`) or a word or two
/// ("ok", "go on").
fn subject(prompt: &str) -> Option<String> {
    let text = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.starts_with('/') || text.split(' ').count() < 4 {
        return None;
    }
    let mut short: String = text.chars().take(TOPIC_LEN).collect();
    if short.len() < text.len() {
        short.push('…');
    }
    Some(short.replace('"', "'"))
}

fn ago(seconds: i64) -> String {
    match seconds {
        s if s < 3_600 => "within the hour".to_owned(),
        s if s < 86_400 => format!("{} h ago", s / 3_600),
        s if s < 2 * 86_400 => "yesterday".to_owned(),
        s => format!("{} days ago", s / 86_400),
    }
}

/// A session started in folder `cwd`: noted, and what it is told of the others.
pub fn session_started(cwd: &Path) -> Option<String> {
    let root = project_of(cwd);
    let now = crate::claude::now();
    let mut log = load();
    let told = log.others(&root, directories::BaseDirs::new().as_ref().map(|d| d.home_dir()), now);
    log.started(&root, now);
    save(&log);
    told
}

/// A request in a session in folder `cwd`.
pub fn prompted(cwd: &Path, session: &str, prompt: &str) {
    let mut log = load();
    log.prompted(&project_of(cwd), session, prompt, crate::claude::now());
    save(&log);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_other_projects() {
        let mut log = Worklog::default();
        let (shop, blog) = (Path::new("/home/me/dev/shop"), Path::new("/home/me/dev/blog"));
        log.started(blog, 1000);
        log.prompted(blog, "a", "ok", 1000);
        log.prompted(blog, "a", "ajoute un flux RSS aux articles", 1010);
        // Only the session's first request.
        log.prompted(blog, "a", "et maintenant pagine la liste des articles", 1020);
        log.prompted(blog, "b", "/clear", 2000);
        log.started(shop, 5000);
        let told = log.others(shop, Some(Path::new("/home/me")), 2000 + 7200).unwrap();
        assert!(told.contains("- blog (~/dev/blog), 2 h ago: \"ajoute un flux RSS aux articles\""), "{told}");
        assert!(!told.contains("shop (") && !told.contains("pagine"));
        // Its own project alone: nothing to tell.
        assert!(log.others(blog, None, 5000).is_some() && Worklog::default().others(shop, None, 0).is_none());
        // Too old: not told.
        assert!(log.others(shop, None, 2000 + 31 * 86_400).is_none());
    }

    #[test]
    fn makes_short_subjects() {
        assert_eq!(subject("  fais  un\nbouton \"stop\" ici "), Some("fais un bouton 'stop' ici".into()));
        assert_eq!(subject("ok super commit"), None);
        assert!(subject(&"mot ".repeat(100)).unwrap().ends_with('…'));
    }
}
