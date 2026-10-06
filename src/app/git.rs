//! Git in a local pane's folder: the files changed and, for the one picked, its content in the last
//! commit and now, side by side; the history of the branch and what each commit changed; the branches,
//! to switch to another one or create one. Shown in place of the pane's terminal; it asks the system's
//! `git`. A folder holding repositories (not one itself) shows them all, one under the other.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;

/// Past this size a file isn't compared (it would be slow to lay out, and rarely read like that).
const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;
/// The list is read again this often while it is shown.
const REFRESH: Duration = Duration::from_secs(3);
/// Commits read at first, and at each "load more".
const LOG_PAGE: usize = 300;
/// Height of a commit in the history.
const COMMIT_ROW_H: f32 = 44.0;

/// The repository holding `dir`: its top folder (the one with `.git`). Remembered a few seconds, as it
/// is asked at every frame.
pub(super) fn repo_root(dir: &Path) -> Option<PathBuf> {
    type Roots = HashMap<PathBuf, (Instant, Option<PathBuf>)>;
    static CACHE: Mutex<Option<Roots>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some((at, root)) = cache.get(dir) {
        if at.elapsed() < Duration::from_secs(3) {
            return root.clone();
        }
    }
    // `.git` is a folder, or a file in a worktree or a submodule.
    let root = dir.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf);
    if cache.len() > 256 {
        cache.clear();
    }
    cache.insert(dir.to_path_buf(), (Instant::now(), root.clone()));
    root
}

/// The repositories right inside `dir`, when it isn't one itself: a folder of projects, each its own
/// repository (as VS Code finds them). Remembered a few seconds, as `repo_root`.
pub(super) fn child_repos(dir: &Path) -> Vec<PathBuf> {
    type Children = HashMap<PathBuf, (Instant, Vec<PathBuf>)>;
    static CACHE: Mutex<Option<Children>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some((at, repos)) = cache.get(dir)
        && at.elapsed() < Duration::from_secs(3)
    {
        return repos.clone();
    }
    let mut repos: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.') && e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| p.join(".git").exists())
        .collect();
    repos.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    if cache.len() > 256 {
        cache.clear();
    }
    cache.insert(dir.to_path_buf(), (Instant::now(), repos.clone()));
    repos
}

/// What the git view of `dir` shows: its repository, or `dir` itself when it holds repositories.
pub(super) fn git_root(dir: &Path) -> Option<PathBuf> {
    repo_root(dir).or_else(|| (!child_repos(dir).is_empty()).then(|| dir.to_path_buf()))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Status {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl Status {
    fn letter(self) -> &'static str {
        match self {
            Status::Modified => "M",
            Status::Added => "A",
            Status::Deleted => "D",
            Status::Renamed => "R",
            Status::Untracked => "U",
            Status::Conflict => "!",
        }
    }

    fn color(self, theme: &Theme) -> Color32 {
        match self {
            Status::Modified => theme.ansi[3],
            Status::Added | Status::Untracked => theme.ansi[2],
            Status::Deleted => theme.ansi[1],
            Status::Renamed => theme.ansi[4],
            Status::Conflict => theme.ansi[5],
        }
    }

    fn label(self, t: &Strings) -> &'static str {
        match self {
            Status::Modified => t.git_modified,
            Status::Added => t.git_added,
            Status::Deleted => t.git_deleted,
            Status::Renamed => t.git_renamed,
            Status::Untracked => t.git_untracked,
            Status::Conflict => t.git_conflict,
        }
    }
}

/// A file changed since the last commit, or by a commit (paths from the repository's top).
#[derive(Clone, PartialEq, Debug)]
struct FileChange {
    path: String,
    /// Its name before, when renamed.
    old_path: Option<String>,
    status: Status,
}

/// The branch checked out, and how far it is from the one it follows.
#[derive(Clone, Default, PartialEq, Debug)]
struct Head {
    /// Empty when detached.
    branch: String,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
}

/// The first line of `git status --branch`, without its "## ": "main...origin/main [ahead 1, behind 2]",
/// "No commits yet on main", "HEAD (no branch)".
fn parse_head(line: &str) -> Head {
    let line = line.strip_prefix("No commits yet on ").unwrap_or(line);
    if line.starts_with("HEAD (no branch)") {
        return Head::default();
    }
    let (names, counts) = line.split_once(" [").unwrap_or((line, ""));
    let (branch, upstream) = match names.split_once("...") {
        Some((b, u)) => (b, Some(u.to_owned())),
        None => (names, None),
    };
    let mut head = Head { branch: branch.to_owned(), upstream, ..Head::default() };
    for part in counts.trim_end_matches(']').split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            head.ahead = n.parse().unwrap_or(0);
        }
        if let Some(n) = part.strip_prefix("behind ") {
            head.behind = n.parse().unwrap_or(0);
        }
    }
    head
}

/// `git status --porcelain=v1 -z --branch`: the branch, then the files.
fn parse_status(out: &[u8]) -> (Head, Vec<FileChange>) {
    let text = String::from_utf8_lossy(out);
    let mut parts = text.split('\0').filter(|p| !p.is_empty());
    let mut head = Head::default();
    let mut files = Vec::new();
    while let Some(entry) = parts.next() {
        if let Some(line) = entry.strip_prefix("## ") {
            head = parse_head(line);
            continue;
        }
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = entry.split_at(3);
        let (x, y) = (xy.as_bytes()[0], xy.as_bytes()[1]);
        let status = match (x, y) {
            (b'?', b'?') => Status::Untracked,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => Status::Conflict,
            (b'R', _) | (_, b'R') | (b'C', _) => Status::Renamed,
            (b'A', _) => Status::Added,
            (b'D', _) | (_, b'D') => Status::Deleted,
            _ => Status::Modified,
        };
        // A rename is followed by the old name.
        let old_path = (status == Status::Renamed).then(|| parts.next().map(str::to_owned)).flatten();
        files.push(FileChange { path: path.to_owned(), old_path, status });
    }
    files.sort_by_key(|f| f.path.to_lowercase());
    (head, files)
}

/// `git diff-tree --name-status -z`: "M\0path\0", "R100\0old\0new\0".
fn parse_name_status(out: &[u8]) -> Vec<FileChange> {
    let text = String::from_utf8_lossy(out);
    let mut parts = text.split('\0').filter(|p| !p.is_empty());
    let mut files = Vec::new();
    while let Some(code) = parts.next() {
        let status = match code.as_bytes()[0] {
            b'A' => Status::Added,
            b'D' => Status::Deleted,
            b'R' | b'C' => Status::Renamed,
            b'U' => Status::Conflict,
            _ => Status::Modified,
        };
        let old_path = if status == Status::Renamed { parts.next().map(str::to_owned) } else { None };
        let Some(path) = parts.next() else { break };
        files.push(FileChange { path: path.to_owned(), old_path, status });
    }
    files.sort_by_key(|f| f.path.to_lowercase());
    files
}

/// A name shown next to a commit.
#[derive(Clone, PartialEq, Debug)]
enum Ref {
    /// The branch checked out ("HEAD" when detached).
    Head(String),
    Branch(String),
    Tag(String),
}

#[derive(Clone, PartialEq, Debug)]
struct Commit {
    hash: String,
    short: String,
    /// The first parent (None for the first commit).
    parent: Option<String>,
    merge: bool,
    author: String,
    email: String,
    /// Seconds since 1970.
    time: i64,
    subject: String,
    refs: Vec<Ref>,
}

/// Fields split by 0x1f, commits by 0x1e; the subject last, as it may hold anything.
const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s%x1e";

fn parse_log(out: &[u8]) -> Vec<Commit> {
    String::from_utf8_lossy(out)
        .split('\x1e')
        .filter_map(|record| {
            let f: Vec<&str> = record.trim_start_matches('\n').splitn(8, '\x1f').collect();
            if f.len() < 8 {
                return None;
            }
            let parents: Vec<&str> = f[2].split(' ').filter(|p| !p.is_empty()).collect();
            Some(Commit {
                hash: f[0].to_owned(),
                short: f[1].to_owned(),
                parent: parents.first().map(|p| p.to_string()),
                merge: parents.len() > 1,
                author: f[3].to_owned(),
                email: f[4].to_owned(),
                time: f[5].parse().unwrap_or(0),
                subject: f[7].trim_end().to_owned(),
                refs: parse_refs(f[6]),
            })
        })
        .collect()
}

/// `%D`: "HEAD -> main, origin/main, origin/HEAD, tag: v1.0".
fn parse_refs(decorations: &str) -> Vec<Ref> {
    decorations
        .split(", ")
        .filter(|r| !r.is_empty() && !r.ends_with("/HEAD"))
        .map(|r| {
            if let Some(b) = r.strip_prefix("HEAD -> ") {
                Ref::Head(b.to_owned())
            } else if let Some(tag) = r.strip_prefix("tag: ") {
                Ref::Tag(tag.to_owned())
            } else if r == "HEAD" {
                Ref::Head(r.to_owned())
            } else {
                Ref::Branch(r.to_owned())
            }
        })
        .collect()
}

/// The branches, the latest used first.
#[derive(Clone, Default, PartialEq, Debug)]
struct Branches {
    /// Name, and the branch it follows (empty when none).
    local: Vec<(String, String)>,
    /// "origin/main".
    remote: Vec<String>,
}

fn parse_branches(out: &[u8]) -> Branches {
    let mut branches = Branches::default();
    for line in String::from_utf8_lossy(out).lines() {
        let (name, upstream) = line.split_once('\x1f').unwrap_or((line, ""));
        if let Some(b) = name.strip_prefix("refs/heads/") {
            branches.local.push((b.to_owned(), upstream.to_owned()));
        } else if let Some(r) = name.strip_prefix("refs/remotes/") {
            if !r.ends_with("/HEAD") {
                branches.remote.push(r.to_owned());
            }
        }
    }
    branches
}

fn git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(["-c", "log.showSignature=false"]);
    // Reads never take the index lock (a commit typed meanwhile in the terminal would fail); nothing
    // asks for a password, as nobody could type it.
    cmd.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0").stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Its output, or what it said on failure.
fn run(mut cmd: Command) -> Result<Vec<u8>, String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

fn read_status(root: &Path) -> Result<(Head, Vec<FileChange>), String> {
    let mut cmd = git(root);
    cmd.args(["status", "--porcelain=v1", "-z", "--branch", "--untracked-files=all"]);
    run(cmd).map(|out| parse_status(&out))
}

fn read_log(root: &Path, limit: usize) -> Result<Vec<Commit>, String> {
    let mut cmd = git(root);
    cmd.args(["log", &format!("-n{limit}"), LOG_FORMAT, "HEAD", "--"]);
    match run(cmd) {
        Ok(out) => Ok(parse_log(&out)),
        // No commit yet.
        Err(e) if e.contains("unknown revision") || e.contains("does not have any commits") => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

fn read_branches(root: &Path) -> Result<Branches, String> {
    let mut cmd = git(root);
    cmd.args(["for-each-ref", "--sort=-committerdate", "--format=%(refname)%1f%(upstream:short)", "refs/heads", "refs/remotes"]);
    run(cmd).map(|out| parse_branches(&out))
}

/// A commit's whole message, and the files it changed (compared with its first parent).
fn read_commit(root: &Path, hash: &str, parent: Option<&str>) -> Result<(String, Vec<FileChange>), String> {
    let mut cmd = git(root);
    cmd.args(["show", "-s", "--format=%B", hash]);
    let message = String::from_utf8_lossy(&run(cmd)?).trim().to_owned();
    let mut cmd = git(root);
    cmd.args(["diff-tree", "-r", "-z", "-M", "--name-status", "--no-commit-id"]);
    match parent {
        Some(p) => cmd.args([p, hash]),
        None => cmd.args(["--root", hash]),
    };
    Ok((message, parse_name_status(&run(cmd)?)))
}

/// What changes the repository, asked from the branch menu or the top bar.
#[derive(Clone, PartialEq, Debug)]
enum Op {
    Switch(String),
    /// Check out a remote branch as a new local one following it.
    Track(String),
    Create(String),
    Fetch,
}

fn run_op(root: &Path, op: &Op) -> Result<(), String> {
    let mut cmd = git(root);
    match op {
        Op::Switch(b) => cmd.args(["switch", b]),
        Op::Track(r) => cmd.args(["switch", "--track", r]),
        Op::Create(b) => cmd.args(["switch", "-c", b]),
        Op::Fetch => cmd.args(["fetch", "--prune"]),
    };
    run(cmd).map(|_| ())
}

/// "3 min ago", then the date past a month.
pub(super) fn ago(time: i64, t: &Strings) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let s = (now - time).max(0);
    let n = |d: i64| (s / d).to_string();
    if s < 60 {
        t.git_now.to_owned()
    } else if s < 3600 {
        t.git_minutes.replace("{n}", &n(60))
    } else if s < 86_400 {
        t.git_hours.replace("{n}", &n(3600))
    } else if s < 30 * 86_400 {
        t.git_days.replace("{n}", &n(86_400))
    } else {
        date(time, "%Y-%m-%d")
    }
}

fn date(time: i64, format: &str) -> String {
    use chrono::TimeZone as _;
    chrono::Local.timestamp_opt(time, 0).single().map(|d| d.format(format).to_string()).unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum RowKind {
    Same,
    Removed,
    Added,
    Changed,
}

/// A line of the comparison: the old line on the left, the new one on the right (with their numbers).
#[derive(Clone, PartialEq, Debug)]
struct Row {
    left: Option<(usize, String)>,
    right: Option<(usize, String)>,
    kind: RowKind,
}

/// The two versions of a file, compared.
#[derive(Clone, PartialEq)]
enum Diff {
    /// `widest`: the longest line, in characters.
    Rows { rows: Vec<Row>, hunks: Vec<usize>, added: usize, removed: usize, widest: usize },
    Binary,
    TooBig,
}

/// Lines side by side: unchanged lines face each other, removed ones on the left, added ones on the
/// right, changed ones facing their replacement.
fn compare(before: &str, after: &str) -> Diff {
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let deadline = Instant::now() + Duration::from_secs(2);
    let ops = similar::capture_diff_slices_deadline(similar::Algorithm::Myers, &old, &new, Some(deadline));
    let line = |s: &str| s.trim_end_matches(['\n', '\r']).replace('\t', "    ");
    let (mut rows, mut hunks, mut added, mut removed) = (Vec::new(), Vec::new(), 0, 0);
    for op in ops {
        let (tag, o, n) = op.as_tag_tuple();
        if tag != similar::DiffTag::Equal {
            hunks.push(rows.len());
        }
        match tag {
            similar::DiffTag::Equal => {
                for (i, j) in o.zip(n) {
                    rows.push(Row { left: Some((i + 1, line(old[i]))), right: Some((j + 1, line(new[j]))), kind: RowKind::Same });
                }
            }
            similar::DiffTag::Delete => {
                removed += o.len();
                rows.extend(o.map(|i| Row { left: Some((i + 1, line(old[i]))), right: None, kind: RowKind::Removed }));
            }
            similar::DiffTag::Insert => {
                added += n.len();
                rows.extend(n.map(|j| Row { left: None, right: Some((j + 1, line(new[j]))), kind: RowKind::Added }));
            }
            similar::DiffTag::Replace => {
                removed += o.len();
                added += n.len();
                let (o, n): (Vec<usize>, Vec<usize>) = (o.collect(), n.collect());
                for k in 0..o.len().max(n.len()) {
                    let left = o.get(k).map(|&i| (i + 1, line(old[i])));
                    let right = n.get(k).map(|&j| (j + 1, line(new[j])));
                    let kind = match (&left, &right) {
                        (Some(_), Some(_)) => RowKind::Changed,
                        (Some(_), None) => RowKind::Removed,
                        _ => RowKind::Added,
                    };
                    rows.push(Row { left, right, kind });
                }
            }
        }
    }
    let widest = rows.iter().flat_map(|r| [&r.left, &r.right]).flatten().map(|(_, l)| l.chars().count()).max().unwrap_or(0);
    Diff::Rows { rows, hunks, added, removed, widest }
}

/// A file at a commit (empty when it wasn't there).
fn blob(root: &Path, rev: &str, path: &str) -> Vec<u8> {
    let mut cmd = git(root);
    cmd.arg("show").arg(format!("{rev}:{path}"));
    run(cmd).unwrap_or_default()
}

/// A file before and after, compared: in the last commit and now (`commit` None), or in a commit's
/// first parent and in the commit.
fn read_diff(root: &Path, change: &FileChange, commit: Option<(&str, Option<&str>)>) -> Result<Diff, String> {
    let old_name = change.old_path.as_deref().unwrap_or(&change.path);
    let (before_rev, after_rev) = match commit {
        None => (Some("HEAD"), None),
        Some((hash, parent)) => (parent, Some(hash)),
    };
    let before = match (before_rev, change.status) {
        (None, _) | (_, Status::Untracked | Status::Added) => Vec::new(),
        (Some(rev), _) => blob(root, rev, old_name),
    };
    let after = match (after_rev, change.status) {
        (_, Status::Deleted) => Vec::new(),
        (Some(rev), _) => blob(root, rev, &change.path),
        (None, _) => std::fs::read(root.join(&change.path)).unwrap_or_default(),
    };
    if before.len().max(after.len()) > MAX_DIFF_BYTES {
        return Ok(Diff::TooBig);
    }
    let binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
    if binary(&before) || binary(&after) {
        return Ok(Diff::Binary);
    }
    Ok(compare(&String::from_utf8_lossy(&before), &String::from_utf8_lossy(&after)))
}

/// Which file a comparison is of: the commit (None: the working copy), and the path.
type DiffKey = (Option<String>, String);

enum Msg {
    Status(Result<(Head, Vec<FileChange>), String>),
    Diff(DiffKey, Result<Diff, String>),
    Log(Result<Vec<Commit>, String>),
    Branches(Result<Branches, String>),
    Commit(String, Result<(String, Vec<FileChange>), String>),
    Op(Op, Result<(), String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Changes,
    History,
    /// The repository's GitHub issues.
    Issues,
}

/// Why the view asks to change where it is shown.
pub(super) enum Exit {
    /// Back to the terminal.
    Back,
    /// Into a window of its own.
    Window,
}

/// A commit picked in the history.
struct OpenCommit {
    commit: Commit,
    /// Its message and files, once read.
    detail: Option<Result<(String, Vec<FileChange>), String>>,
    file: Option<String>,
}

pub(super) struct GitView {
    root: PathBuf,
    mode: Mode,
    head: Head,
    files: Vec<FileChange>,
    /// The list was read once (before that, a spinner).
    loaded: bool,
    error: Option<String>,
    selected: Option<String>,
    diff: Option<(DiffKey, Result<Diff, String>)>,
    status_running: bool,
    diff_running: bool,
    last_status: Option<Instant>,
    /// The history: the latest `limit` commits of the branch, filtered by `search`.
    commits: Vec<Commit>,
    limit: usize,
    log_loaded: bool,
    log_running: bool,
    log_error: Option<String>,
    search: String,
    commit: Option<OpenCommit>,
    /// Read when its tab is first shown.
    issues: Option<Box<super::issues::Issues>>,
    /// The branch menu: the branches (read when it opens), what is typed in it.
    branches: Option<Result<Branches, String>>,
    branch_filter: String,
    focus_filter: bool,
    op_running: bool,
    /// What the last switch, creation or fetch gave (true: failed), and when.
    notice: Option<(String, bool, Instant)>,
    /// Scroll the comparison to this offset (a change picked with ↑ ↓, a click in the map), and the
    /// change shown last.
    scroll_to: Option<f32>,
    hunk: usize,
    /// Widths set by dragging: the list of files, and where the two versions meet (a fraction).
    list_w: Option<f32>,
    split: f32,
    /// How far both versions are scrolled sideways, and the part of the comparison in sight.
    hscroll: f32,
    v_offset: f32,
    v_visible: f32,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    /// A folder holding repositories (not one itself): a view of each, listed together.
    repos: Vec<GitView>,
    /// The repository shown on its own (its history, its branches), from that list.
    open_repo: Option<usize>,
    /// The repository of the file compared, in that list.
    picked_repo: Option<usize>,
    /// Repositories folded in that list.
    folded: std::collections::HashSet<PathBuf>,
    /// One of those repositories: its bar goes back to the list.
    nested: bool,
}

impl GitView {
    pub fn new(root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            root,
            mode: Mode::Changes,
            head: Head::default(),
            files: Vec::new(),
            loaded: false,
            error: None,
            selected: None,
            diff: None,
            status_running: false,
            diff_running: false,
            last_status: None,
            commits: Vec::new(),
            limit: LOG_PAGE,
            log_loaded: false,
            log_running: false,
            log_error: None,
            search: String::new(),
            commit: None,
            issues: None,
            branches: None,
            branch_filter: String::new(),
            focus_filter: false,
            op_running: false,
            notice: None,
            scroll_to: None,
            hunk: 0,
            list_w: None,
            split: 0.5,
            hscroll: 0.0,
            v_offset: 0.0,
            v_visible: 0.0,
            tx,
            rx,
            repos: Vec::new(),
            open_repo: None,
            picked_repo: None,
            folded: Default::default(),
            nested: false,
        }
    }

    /// The view of `root`: a repository, or a folder of repositories.
    pub fn open(root: PathBuf) -> Self {
        let repos = if root.join(".git").exists() { Vec::new() } else { child_repos(&root) };
        let mut view = Self::new(root);
        view.repos = repos
            .into_iter()
            .map(|r| {
                let mut repo = Self::new(r);
                repo.nested = true;
                repo
            })
            .collect();
        view
    }

    /// Runs `job` on a thread; what it gives comes back through `poll`.
    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&Path) -> Msg + Send + 'static) {
        let (root, tx, ctx) = (self.root.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(&root));
            ctx.request_repaint();
        });
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.last_status = Some(Instant::now());
        if !self.status_running {
            self.status_running = true;
            self.spawn(ctx, |root| Msg::Status(read_status(root)));
        }
        match self.mode {
            // The file shown may have changed too.
            Mode::Changes => {
                if let Some(change) = self.selected.as_ref().and_then(|p| self.files.iter().find(|f| &f.path == p)).cloned() {
                    self.load_diff(ctx, change, None);
                }
            }
            // A commit typed in the terminal, another branch.
            Mode::History => self.load_log(ctx),
            Mode::Issues => {}
        }
    }

    fn load_log(&mut self, ctx: &egui::Context) {
        if self.log_running {
            return;
        }
        self.log_running = true;
        let limit = self.limit;
        self.spawn(ctx, move |root| Msg::Log(read_log(root, limit)));
    }

    /// `commit`: the commit and its first parent, for a file it changed.
    fn load_diff(&mut self, ctx: &egui::Context, change: FileChange, commit: Option<(String, Option<String>)>) {
        if self.diff_running {
            return;
        }
        self.diff_running = true;
        self.spawn(ctx, move |root| {
            let diff = read_diff(root, &change, commit.as_ref().map(|(h, p)| (h.as_str(), p.as_deref())));
            Msg::Diff((commit.map(|(h, _)| h), change.path), diff)
        });
    }

    fn open_commit(&mut self, ctx: &egui::Context, commit: Commit) {
        let (hash, parent) = (commit.hash.clone(), commit.parent.clone());
        self.commit = Some(OpenCommit { commit, detail: None, file: None });
        self.spawn(ctx, move |root| {
            let detail = read_commit(root, &hash, parent.as_deref());
            Msg::Commit(hash, detail)
        });
    }

    /// Shows a file of the commit open.
    fn pick_commit_file(&mut self, ctx: &egui::Context, path: String) {
        let Some(open) = &mut self.commit else { return };
        let Some(Ok((_, files))) = &open.detail else { return };
        let Some(change) = files.iter().find(|f| f.path == path).cloned() else { return };
        open.file = Some(path);
        let source = (open.commit.hash.clone(), open.commit.parent.clone());
        self.reset_diff();
        self.load_diff(ctx, change, Some(source));
    }

    fn reset_diff(&mut self) {
        self.diff = None;
        self.hunk = 0;
        self.hscroll = 0.0;
        self.diff_running = false;
    }

    fn start(&mut self, ctx: &egui::Context, op: Op) {
        if self.op_running {
            return;
        }
        self.op_running = true;
        self.notice = None;
        self.spawn(ctx, move |root| {
            let result = run_op(root, &op);
            Msg::Op(op, result)
        });
    }

    fn poll(&mut self, ctx: &egui::Context, t: &Strings) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Status(Ok((head, files))) => {
                    self.head = head;
                    self.files = files;
                    self.error = None;
                    self.loaded = true;
                    self.status_running = false;
                    // A file committed or reverted meanwhile: nothing to show for it any more.
                    if self.selected.as_ref().is_some_and(|p| !self.files.iter().any(|f| &f.path == p)) {
                        self.selected = None;
                        if self.mode == Mode::Changes {
                            self.diff = None;
                        }
                    }
                }
                Msg::Status(Err(e)) => {
                    self.error = Some(e);
                    self.loaded = true;
                    self.status_running = false;
                }
                Msg::Diff(key, diff) => {
                    self.diff_running = false;
                    if self.current_key().as_ref() == Some(&key) {
                        // Read again unchanged: the scroll position stays.
                        if self.diff.as_ref().is_none_or(|(k, d)| k != &key || d != &diff) {
                            self.diff = Some((key, diff));
                        }
                    }
                }
                Msg::Log(result) => {
                    self.log_running = false;
                    self.log_loaded = true;
                    match result {
                        Ok(commits) => {
                            if commits != self.commits {
                                self.commits = commits;
                            }
                            self.log_error = None;
                        }
                        Err(e) => self.log_error = Some(e),
                    }
                }
                Msg::Branches(result) => self.branches = Some(result),
                Msg::Commit(hash, detail) => {
                    let Some(open) = self.commit.as_mut().filter(|o| o.commit.hash == hash) else { continue };
                    let first = detail.as_ref().ok().and_then(|(_, files)| files.first()).map(|f| f.path.clone());
                    open.detail = Some(detail);
                    // Its first file shown right away.
                    if let Some(path) = first {
                        self.pick_commit_file(ctx, path);
                    }
                }
                Msg::Op(op, result) => {
                    self.op_running = false;
                    let text = match (&op, result) {
                        (_, Err(e)) => Err(e),
                        (Op::Switch(b), Ok(())) => Ok(t.git_switched.replace("{name}", b)),
                        // "origin/feature" is checked out as "feature".
                        (Op::Track(r), Ok(())) => Ok(t.git_switched.replace("{name}", r.split_once('/').map_or(r.as_str(), |(_, b)| b))),
                        (Op::Create(b), Ok(())) => Ok(t.git_created.replace("{name}", b)),
                        (Op::Fetch, Ok(())) => Ok(t.git_fetched.to_owned()),
                    };
                    self.notice = Some(match text {
                        Ok(text) => (text, false, Instant::now()),
                        Err(e) => (e, true, Instant::now()),
                    });
                    self.branches = None;
                    self.last_status = None;
                    if self.log_loaded {
                        self.load_log(ctx);
                    }
                }
            }
        }
    }

    /// The comparison that should be shown.
    fn current_key(&self) -> Option<DiffKey> {
        match self.mode {
            Mode::Changes => self.selected.clone().map(|p| (None, p)),
            Mode::History => self.commit.as_ref().and_then(|o| o.file.clone().map(|p| (Some(o.commit.hash.clone()), p))),
            Mode::Issues => None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Draws the view in `rect` (`window`: a window of its own, else in place of a pane's terminal);
    /// what was asked from its top bar.
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &'static Strings, window: bool) -> Option<Exit> {
        if !self.repos.is_empty() {
            return self.repos_ui(ui, rect, theme, t, window);
        }
        let ctx = ui.ctx().clone();
        self.poll(&ctx, t);
        if self.last_status.is_none_or(|at| at.elapsed() >= REFRESH) {
            self.refresh(&ctx);
        }
        ctx.request_repaint_after(REFRESH);
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let mut exit = None;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        ui.set_clip_rect(rect);
        let ui = &mut ui;

        // Top: the repository, its branch (a menu to switch), how far from the server, how many files
        // changed; fetch, refresh and back.
        let (bar, _) = ui.allocate_exact_size(Vec2::new(rect.width(), 34.0), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.painter().hline(bar.x_range(), bar.max.y, Stroke::new(1.0, theme.tab_hover));
        let repo = self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut x = bar.min.x + 36.0;
        if self.nested {
            // One of a folder's repositories: the folder first, a click goes back to all of them.
            let folder = self.root.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let label = format!("‹  {folder}");
            let w = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(13.5), theme.text_muted).size().x + 16.0;
            let at = Rect::from_min_size(Pos2::new(bar.min.x + 6.0, bar.min.y + 4.0), Vec2::new(w, 26.0));
            let crumb = egui::Button::new(egui::RichText::new(label).size(13.5).color(theme.text_muted)).frame_when_inactive(false).corner_radius(5.0);
            if ui.put(at, crumb).on_hover_text(t.git_all_repos).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                exit = Some(Exit::Back);
            }
            x = at.max.x + 6.0;
            let slash = ui.painter().layout_no_wrap("/".into(), FontId::proportional(13.5), theme.text_muted);
            ui.painter().galley(Pos2::new(x, bar.center().y - slash.size().y / 2.0), slash.clone(), theme.text_muted);
            x += slash.size().x + 8.0;
        } else {
            paint_branch_icon(ui.painter(), Rect::from_center_size(Pos2::new(bar.min.x + 20.0, bar.center().y), Vec2::splat(16.0)), theme.accent);
        }
        let galley = ui.painter().layout_no_wrap(repo, FontId::proportional(13.5), theme.text);
        ui.painter().galley(Pos2::new(x, bar.center().y - galley.size().y / 2.0), galley.clone(), theme.text);
        x += galley.size().x + 8.0;
        if self.loaded && self.error.is_none() {
            x = self.branch_button(ui, Pos2::new(x, bar.center().y), theme, t) + 8.0;
            let mut job = egui::text::LayoutJob::default();
            if self.head.ahead > 0 {
                job.append(&format!("↑{} ", self.head.ahead), 0.0, egui::TextFormat::simple(FontId::monospace(12.0), theme.ansi[2]));
            }
            if self.head.behind > 0 {
                job.append(&format!("↓{} ", self.head.behind), 0.0, egui::TextFormat::simple(FontId::monospace(12.0), theme.ansi[3]));
            }
            if !job.is_empty() {
                let galley = ui.painter().layout_job(job);
                let at = Rect::from_min_size(Pos2::new(x, bar.center().y - galley.size().y / 2.0), galley.size());
                ui.painter().galley(at.min, galley, theme.text);
                let upstream = self.head.upstream.clone().unwrap_or_default();
                let mut tip = Vec::new();
                if self.head.ahead > 0 {
                    tip.push(t.git_ahead.replace("{n}", &self.head.ahead.to_string()).replace("{upstream}", &upstream));
                }
                if self.head.behind > 0 {
                    tip.push(t.git_behind.replace("{n}", &self.head.behind.to_string()).replace("{upstream}", &upstream));
                }
                ui.interact(at, egui::Id::new(("git-ahead", &self.root)), Sense::hover()).on_hover_text(tip.join("\n"));
                x = at.max.x + 4.0;
            }
            let count = if self.files.is_empty() { t.git_no_changes.to_owned() } else { t.git_changes.replace("{n}", &self.files.len().to_string()) };
            let mut job = egui::text::LayoutJob::simple_singleline(format!("·   {count}"), FontId::proportional(12.5), theme.text_muted);
            job.wrap = egui::text::TextWrapping::truncate_at_width((bar.max.x - 160.0 - x).max(0.0));
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(Pos2::new(x, bar.center().y - galley.size().y / 2.0), galley, theme.text);
        }
        let button = |ui: &mut Ui, right: f32, text: &str, tip: &str| {
            let at = Rect::from_min_size(Pos2::new(right - 28.0, bar.min.y + 4.0), Vec2::new(28.0, 26.0));
            let resp = ui.put(at, egui::Button::new(egui::RichText::new(text).size(15.0)).frame_when_inactive(false).corner_radius(5.0)).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand);
            (resp.clicked(), at)
        };
        // In a window: closes it. In a pane: its terminal again, or the view in a window.
        let mut right = bar.max.x - 6.0;
        if self.nested {
            let (back, at) = button(ui, right, "", t.git_all_repos);
            paint_return_icon(ui.painter(), at.center(), theme.fg);
            if back {
                exit = Some(Exit::Back);
            }
        } else if window {
            let (close, at) = button(ui, right, "", t.close);
            paint_close_icon(ui.painter(), at.center(), theme.fg);
            if close {
                exit = Some(Exit::Back);
            }
        } else {
            let (back, at) = button(ui, right, "", t.git_back);
            paint_return_icon(ui.painter(), at.center(), theme.fg);
            if back {
                exit = Some(Exit::Back);
            }
            right -= 32.0;
            let (pop_out, at) = button(ui, right, "", t.new_window_open);
            paint_window_icon(ui.painter(), at.center(), theme.fg);
            if pop_out {
                exit = Some(Exit::Window);
            }
        }
        right -= 32.0;
        if button(ui, right, "↻", t.git_refresh).0 {
            self.last_status = None;
        }
        right -= 32.0;
        let (fetch, at) = button(ui, right, "", t.git_fetch);
        paint_fetch_icon(ui.painter(), at.center(), if self.op_running { theme.text_muted } else { theme.fg });
        if fetch {
            self.start(&ctx, Op::Fetch);
        }
        if (self.status_running && !self.loaded) || self.op_running {
            ui.put(Rect::from_center_size(Pos2::new(right - 42.0, bar.center().y), Vec2::splat(14.0)), egui::Spinner::new().size(14.0));
        }

        // What the last switch or fetch gave.
        let mut top = bar.max.y + 1.0;
        if self.notice.as_ref().is_some_and(|(_, failed, at)| !failed && at.elapsed() > Duration::from_secs(5)) {
            self.notice = None;
        }
        if let Some((text, failed, _)) = self.notice.clone() {
            let color = if failed { theme.ansi[1] } else { theme.ansi[2] };
            let galley = ui.painter().layout(text, FontId::proportional(12.5), color, rect.width() - 60.0);
            let strip = Rect::from_min_size(Pos2::new(rect.min.x, top), Vec2::new(rect.width(), galley.size().y + 14.0));
            ui.painter().rect_filled(strip, 0.0, color.gamma_multiply(0.12));
            ui.painter().galley(Pos2::new(strip.min.x + 12.0, strip.min.y + 7.0), galley, color);
            let close = Rect::from_min_size(Pos2::new(strip.max.x - 30.0, strip.min.y + 4.0), Vec2::new(24.0, 22.0));
            if ui.put(close, egui::Button::new("×").frame_when_inactive(false)).clicked() {
                self.notice = None;
            }
            if !failed {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
            top = strip.max.y;
        }

        let body = Rect::from_min_max(Pos2::new(rect.min.x, top), rect.max);
        if let Some(e) = &self.error {
            let text = if e.contains("No such file") || e.contains("not found") || e.contains("introuvable") { t.git_missing.to_owned() } else { e.clone() };
            ui.put(body.shrink(20.0), egui::Label::new(egui::RichText::new(text).size(13.0).color(theme.ansi[1])).wrap());
            return exit;
        }
        if !self.loaded {
            super::loading::screen(ui, body, theme, t.git_loading, Some(&self.root.display().to_string()), None);
            return exit;
        }

        // Left: changes or history. Right: the file picked, compared.
        let list_w = self.list_w.unwrap_or((body.width() * 0.32).clamp(260.0, 380.0)).clamp(200.0, (body.width() - 300.0).max(200.0));
        let list = Rect::from_min_max(body.min, Pos2::new(body.min.x + list_w, body.max.y));
        let view = Rect::from_min_max(Pos2::new(list.max.x + 1.0, body.min.y), body.max);
        ui.painter().rect_filled(list, 0.0, theme.chrome_bg);
        ui.painter().vline(list.max.x, list.y_range(), Stroke::new(1.0, theme.tab_hover));
        let tabs = Rect::from_min_size(list.min + Vec2::new(8.0, 8.0), Vec2::new(list.width() - 16.0, 26.0));
        if let Some(mode) = mode_tabs(ui, tabs, self.mode, &self.root, theme, t) {
            if mode != self.mode {
                self.mode = mode;
                self.reset_diff();
                if mode == Mode::History && !self.log_loaded {
                    self.load_log(&ctx);
                }
                // What was shown in this mode, shown again.
                match (mode, self.current_key()) {
                    (Mode::Changes, Some((_, path))) => {
                        if let Some(change) = self.files.iter().find(|f| f.path == path).cloned() {
                            self.load_diff(&ctx, change, None);
                        }
                    }
                    (Mode::History, Some((_, path))) => self.pick_commit_file(&ctx, path),
                    _ => {}
                }
            }
        }
        let content = Rect::from_min_max(Pos2::new(list.min.x, tabs.max.y + 6.0), list.max);
        match self.mode {
            Mode::Changes => self.changes_ui(ui, content, view, theme, t),
            Mode::History => self.history_ui(ui, content, view, theme, t),
            Mode::Issues => {
                let root = self.root.clone();
                self.issues.get_or_insert_with(|| Box::new(super::issues::Issues::new(root))).ui(ui, content, view, theme, t);
            }
        }
        if let Some(dx) = splitter(ui, egui::Id::new(("git-list-split", &self.root)), list.max.x, list.y_range(), theme) {
            self.list_w = Some(list_w + dx);
        }
        exit
    }

    /// A folder of repositories: each one's branch and changes, one under the other (folded on a click);
    /// a file picked, compared on the right. A repository opened shows on its own, as usual.
    fn repos_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &'static Strings, window: bool) -> Option<Exit> {
        let ctx = ui.ctx().clone();
        // Repositories cloned or removed meanwhile.
        let found = child_repos(&self.root);
        if !found.is_empty() && found.iter().ne(self.repos.iter().map(|r| &r.root)) {
            let picked = self.picked_repo.and_then(|i| self.repos.get(i)).map(|r| r.root.clone());
            let mut old: HashMap<PathBuf, GitView> = self.repos.drain(..).map(|r| (r.root.clone(), r)).collect();
            self.repos = found
                .into_iter()
                .map(|root| {
                    old.remove(&root).unwrap_or_else(|| {
                        let mut repo = Self::new(root);
                        repo.nested = true;
                        repo
                    })
                })
                .collect();
            self.picked_repo = picked.and_then(|p| self.repos.iter().position(|r| r.root == p));
            self.open_repo = None;
        }
        for repo in &mut self.repos {
            repo.poll(&ctx, t);
            if repo.last_status.is_none_or(|at| at.elapsed() >= REFRESH) {
                repo.refresh(&ctx);
            }
        }
        ctx.request_repaint_after(REFRESH);
        if let Some(i) = self.open_repo {
            if let Some(repo) = self.repos.get_mut(i) {
                if let Some(Exit::Back) = repo.ui(ui, rect, theme, t, window) {
                    self.open_repo = None;
                }
                return None;
            }
            self.open_repo = None;
        }

        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let mut exit = None;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        ui.set_clip_rect(rect);
        let ui = &mut ui;

        // Top: the folder, how many repositories; refresh and back.
        let (bar, _) = ui.allocate_exact_size(Vec2::new(rect.width(), 34.0), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.painter().hline(bar.x_range(), bar.max.y, Stroke::new(1.0, theme.tab_hover));
        paint_branch_icon(ui.painter(), Rect::from_center_size(Pos2::new(bar.min.x + 20.0, bar.center().y), Vec2::splat(16.0)), theme.accent);
        let name = self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut job = egui::text::LayoutJob::default();
        job.append(&name, 0.0, egui::TextFormat::simple(FontId::proportional(13.5), theme.text));
        let changed: usize = self.repos.iter().map(|r| r.files.len()).sum();
        let count = t.git_repos.replace("{n}", &self.repos.len().to_string());
        let changes = if changed == 0 { t.git_no_changes.to_owned() } else { t.git_changes.replace("{n}", &changed.to_string()) };
        job.append(&format!("   ·   {count}   ·   {changes}"), 0.0, egui::TextFormat::simple(FontId::proportional(12.5), theme.text_muted));
        job.wrap = egui::text::TextWrapping::truncate_at_width((bar.width() - 140.0).max(0.0));
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(Pos2::new(bar.min.x + 36.0, bar.center().y - galley.size().y / 2.0), galley, theme.text);
        let button = |ui: &mut Ui, right: f32, text: &str, tip: &str| {
            let at = Rect::from_min_size(Pos2::new(right - 28.0, bar.min.y + 4.0), Vec2::new(28.0, 26.0));
            let resp = ui.put(at, egui::Button::new(egui::RichText::new(text).size(15.0)).frame_when_inactive(false).corner_radius(5.0)).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand);
            (resp.clicked(), at)
        };
        let mut right = bar.max.x - 6.0;
        if window {
            let (close, at) = button(ui, right, "", t.close);
            paint_close_icon(ui.painter(), at.center(), theme.fg);
            if close {
                exit = Some(Exit::Back);
            }
        } else {
            let (back, at) = button(ui, right, "", t.git_back);
            paint_return_icon(ui.painter(), at.center(), theme.fg);
            if back {
                exit = Some(Exit::Back);
            }
            right -= 32.0;
            let (pop_out, at) = button(ui, right, "", t.new_window_open);
            paint_window_icon(ui.painter(), at.center(), theme.fg);
            if pop_out {
                exit = Some(Exit::Window);
            }
        }
        right -= 32.0;
        if button(ui, right, "↻", t.git_refresh).0 {
            for repo in &mut self.repos {
                repo.last_status = None;
            }
        }

        let body = Rect::from_min_max(Pos2::new(rect.min.x, bar.max.y + 1.0), rect.max);
        let list_w = self.list_w.unwrap_or((body.width() * 0.32).clamp(280.0, 420.0)).clamp(220.0, (body.width() - 300.0).max(220.0));
        let list = Rect::from_min_max(body.min, Pos2::new(body.min.x + list_w, body.max.y));
        let view = Rect::from_min_max(Pos2::new(list.max.x + 1.0, body.min.y), body.max);
        ui.painter().rect_filled(list, 0.0, theme.chrome_bg);
        ui.painter().vline(list.max.x, list.y_range(), Stroke::new(1.0, theme.tab_hover));

        // Each repository: a header (fold, name, branch, how far from the server, changes, open), then
        // its files.
        let (mut fold, mut open, mut picked) = (None, None, None);
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list.shrink2(Vec2::new(4.0, 6.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        egui::ScrollArea::vertical().id_salt(("git-repos", &self.root)).auto_shrink(false).show(&mut list_ui, |ui| {
            for (i, repo) in self.repos.iter().enumerate() {
                let folded = self.folded.contains(&repo.root);
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(row, 5.0, theme.tab_hover);
                }
                let c = Pos2::new(row.min.x + 10.0, row.center().y);
                let arrow = if folded { vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(2.5, 0.0), c + Vec2::new(-2.0, 4.0)] } else { vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(0.0, 2.5), c + Vec2::new(4.0, -2.0)] };
                ui.painter().add(egui::Shape::line(arrow, Stroke::new(1.4, theme.text_muted)));
                // Open on its own: its history and branches.
                let open_at = Rect::from_min_size(Pos2::new(row.max.x - 26.0, row.min.y + 3.0), Vec2::new(24.0, 22.0));
                let open_resp = ui.put(open_at, egui::Button::new(egui::RichText::new("›").size(15.0)).frame_when_inactive(false).corner_radius(5.0)).on_hover_text(t.git_open_repo).on_hover_cursor(egui::CursorIcon::PointingHand);
                if open_resp.clicked() {
                    open = Some(i);
                }
                let mut job = egui::text::LayoutJob::default();
                let name = repo.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                job.append(&name, 0.0, egui::TextFormat::simple(FontId::proportional(13.0), theme.text));
                if repo.loaded && repo.error.is_none() {
                    let detached = repo.head.branch.is_empty();
                    let branch = if detached { t.git_detached.to_owned() } else { repo.head.branch.clone() };
                    job.append(&format!("  {branch}"), 0.0, egui::TextFormat::simple(FontId::monospace(11.5), if detached { theme.ansi[3] } else { theme.accent }));
                    if repo.head.ahead > 0 {
                        job.append(&format!(" ↑{}", repo.head.ahead), 0.0, egui::TextFormat::simple(FontId::monospace(11.5), theme.ansi[2]));
                    }
                    if repo.head.behind > 0 {
                        job.append(&format!(" ↓{}", repo.head.behind), 0.0, egui::TextFormat::simple(FontId::monospace(11.5), theme.ansi[3]));
                    }
                }
                // The number of files changed, in a badge.
                let mut name_end = open_at.min.x - 4.0;
                if !repo.loaded {
                    ui.put(Rect::from_center_size(Pos2::new(name_end - 10.0, row.center().y), Vec2::splat(12.0)), egui::Spinner::new().size(12.0));
                    name_end -= 24.0;
                } else if !repo.files.is_empty() {
                    let n = ui.painter().layout_no_wrap(repo.files.len().to_string(), FontId::proportional(11.0), theme.bg);
                    let badge = Rect::from_center_size(Pos2::new(name_end - 4.0 - (n.size().x + 10.0) / 2.0, row.center().y), Vec2::new((n.size().x + 10.0).max(18.0), 17.0));
                    ui.painter().rect_filled(badge, 8.5, theme.accent);
                    ui.painter().galley(badge.center() - n.size() / 2.0, n, theme.bg);
                    name_end = badge.min.x - 6.0;
                }
                job.wrap = egui::text::TextWrapping::truncate_at_width((name_end - row.min.x - 22.0).max(0.0));
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(Pos2::new(row.min.x + 22.0, row.center().y - galley.size().y / 2.0), galley, theme.text);
                if resp.on_hover_text(repo.root.display().to_string()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    fold = Some(repo.root.clone());
                }
                if folded {
                    continue;
                }
                if let Some(e) = &repo.error {
                    ui.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                }
                ui.indent(("git-repo", i), |ui| {
                    for change in &repo.files {
                        let selected = self.picked_repo == Some(i) && repo.selected.as_ref() == Some(&change.path);
                        if file_row(ui, change, selected, theme, t) {
                            picked = Some((i, change.clone()));
                        }
                    }
                });
                ui.add_space(4.0);
            }
        });
        if let Some(root) = fold
            && !self.folded.remove(&root)
        {
            self.folded.insert(root);
        }
        if let Some(i) = open {
            self.open_repo = Some(i);
        }
        if let Some((i, change)) = picked {
            if let Some(old) = self.picked_repo.filter(|&j| j != i).and_then(|j| self.repos.get_mut(j)) {
                old.selected = None;
                old.reset_diff();
            }
            self.picked_repo = Some(i);
            let repo = &mut self.repos[i];
            if repo.mode != Mode::Changes || repo.selected.as_ref() != Some(&change.path) {
                repo.mode = Mode::Changes;
                repo.selected = Some(change.path.clone());
                repo.reset_diff();
                repo.load_diff(&ctx, change, None);
            }
        }
        if let Some(dx) = splitter(ui, egui::Id::new(("git-repos-split", &self.root)), list.max.x, list.y_range(), theme) {
            self.list_w = Some(list_w + dx);
        }

        // Right: the file picked, in its repository.
        let shown = self.picked_repo.and_then(|i| self.repos.get_mut(i)).and_then(|repo| {
            let path = repo.selected.clone().filter(|_| repo.mode == Mode::Changes)?;
            let status = repo.files.iter().find(|f| f.path == path).map(|f| f.status);
            Some((repo, path, status))
        });
        match shown {
            Some((repo, path, status)) => repo.diff_ui(ui, view, (None, path), status, (t.git_before.to_owned(), t.git_after.to_owned()), theme, t),
            None if changed == 0 => centered_text(ui, view, t.git_all_committed, theme),
            None => centered_text(ui, view, t.git_pick, theme),
        }
        exit
    }

    /// The branch checked out, as a button opening the branch menu; where it ends.
    fn branch_button(&mut self, ui: &mut Ui, left_center: Pos2, theme: &Theme, t: &Strings) -> f32 {
        let detached = self.head.branch.is_empty();
        let name = if detached { t.git_detached.to_owned() } else { self.head.branch.clone() };
        let galley = ui.painter().layout_no_wrap(name, FontId::monospace(12.5), if detached { theme.ansi[3] } else { theme.accent });
        let at = Rect::from_min_size(Pos2::new(left_center.x, left_center.y - 12.0), Vec2::new(galley.size().x + 30.0, 24.0));
        let resp = ui.interact(at, egui::Id::new(("git-branch", &self.root)), Sense::click()).on_hover_text(t.git_branch_tip).on_hover_cursor(egui::CursorIcon::PointingHand);
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
        if resp.hovered() || open {
            ui.painter().rect_filled(at, 5.0, theme.tab_hover);
        }
        ui.painter().rect_stroke(at, 5.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        ui.painter().galley(Pos2::new(at.min.x + 9.0, at.center().y - galley.size().y / 2.0), galley, theme.text);
        // ▾
        let c = Pos2::new(at.max.x - 11.0, at.center().y);
        ui.painter().add(egui::Shape::line(vec![c + Vec2::new(-3.5, -1.5), c + Vec2::new(0.0, 2.0), c + Vec2::new(3.5, -1.5)], Stroke::new(1.4, theme.text_muted)));
        if resp.clicked() {
            self.branches = None;
            self.branch_filter.clear();
            self.focus_filter = true;
            self.spawn(ui.ctx(), |root| Msg::Branches(read_branches(root)));
        }
        if let Some(op) = self.branch_menu(&resp, theme, t) {
            self.start(ui.ctx(), op);
        }
        at.max.x
    }

    /// The branches, local then remote, filtered by what is typed; typing a new name offers to create
    /// it. Enter: the only branch left, or the new one.
    fn branch_menu(&mut self, resp: &egui::Response, theme: &Theme, t: &Strings) -> Option<Op> {
        let mut op = None;
        egui::Popup::menu(resp).width(320.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut self.branch_filter).hint_text(t.git_branch_filter).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 5.0)));
            if std::mem::take(&mut self.focus_filter) {
                edit.request_focus();
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let name = self.branch_filter.trim().to_owned();
            let needle = name.to_lowercase();
            let matches = |b: &str| b.to_lowercase().contains(&needle);
            ui.add_space(4.0);
            let branches = match &self.branches {
                None => {
                    super::loading::inline(ui, theme, t.git_loading_branches);
                    return;
                }
                Some(Err(e)) => {
                    ui.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                    return;
                }
                Some(Ok(b)) => b,
            };
            let local: Vec<&(String, String)> = branches.local.iter().filter(|(b, _)| matches(b)).collect();
            // A remote branch already followed by a local one: the local one is enough.
            let followed = |r: &String| branches.local.iter().any(|(_, up)| up == r);
            let remote: Vec<&String> = branches.remote.iter().filter(|r| !followed(r) && matches(r)).collect();
            let exists = branches.local.iter().any(|(b, _)| *b == name);
            if !name.is_empty() && !exists {
                if ui.button(t.git_create_branch.replace("{name}", &name)).clicked() || (enter && local.is_empty() && remote.is_empty()) {
                    // "-x" would be read as an option.
                    op = Some(if name.starts_with('-') || name.contains(char::is_whitespace) { Err(()) } else { Ok(Op::Create(name.clone())) });
                    ui.close();
                }
                ui.separator();
            }
            let pick_remote = |r: &str| {
                // "origin/feature": "feature" if it exists already, else a new one following it.
                let short = r.split_once('/').map_or(r, |(_, b)| b);
                if branches.local.iter().any(|(b, _)| b == short) { Op::Switch(short.to_owned()) } else { Op::Track(r.to_owned()) }
            };
            if enter && op.is_none() {
                match (local.as_slice(), remote.as_slice()) {
                    ([(b, _)], []) if *b != self.head.branch => op = Some(Ok(Op::Switch(b.clone()))),
                    ([], [r]) => op = Some(Ok(pick_remote(r))),
                    _ => {}
                }
                if op.is_some() {
                    ui.close();
                }
            }
            let heading = |ui: &mut Ui, text: &str| {
                ui.add_space(2.0);
                ui.label(egui::RichText::new(text).size(11.0).color(theme.text_muted));
            };
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                if !local.is_empty() {
                    heading(ui, t.git_local_branches);
                }
                for (b, upstream) in &local {
                    let current = *b == self.head.branch;
                    let text = egui::RichText::new(b).monospace().size(12.5).color(if current { theme.accent } else { theme.text });
                    let button = egui::Button::new(if current { text.strong() } else { text }).shortcut_text(egui::RichText::new(upstream).size(11.0)).truncate();
                    if ui.add(button).clicked() {
                        if !current {
                            op = Some(Ok(Op::Switch(b.clone())));
                        }
                        ui.close();
                    }
                }
                if !remote.is_empty() {
                    ui.add_space(4.0);
                    heading(ui, t.git_remote_branches);
                }
                for r in &remote {
                    let text = egui::RichText::new(r.as_str()).monospace().size(12.5).color(theme.fg);
                    if ui.add(egui::Button::new(text).truncate()).clicked() {
                        op = Some(Ok(pick_remote(r)));
                        ui.close();
                    }
                }
            });
        });
        match op {
            Some(Ok(op)) => Some(op),
            Some(Err(())) => {
                self.notice = Some((t.git_bad_name.to_owned(), true, Instant::now()));
                None
            }
            None => None,
        }
    }

    /// The files changed now; the one picked, compared with the last commit.
    fn changes_ui(&mut self, ui: &mut Ui, content: Rect, view: Rect, theme: &Theme, t: &Strings) {
        let mut picked = None;
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(content.shrink2(Vec2::new(4.0, 0.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        if self.files.is_empty() {
            list_ui.add_space(8.0);
            list_ui.label(egui::RichText::new(t.git_no_changes).size(12.5).color(theme.text_muted));
        }
        egui::ScrollArea::vertical().id_salt(("git-files", &self.root)).auto_shrink(false).show(&mut list_ui, |ui| {
            for change in &self.files {
                if file_row(ui, change, self.selected.as_ref() == Some(&change.path), theme, t) {
                    picked = Some(change.clone());
                }
            }
        });
        if let Some(change) = picked {
            if self.selected.as_ref() != Some(&change.path) {
                self.selected = Some(change.path.clone());
                self.reset_diff();
                self.load_diff(ui.ctx(), change, None);
            }
        }
        if self.files.is_empty() {
            return centered_text(ui, view, t.git_all_committed, theme);
        }
        let Some(path) = self.selected.clone() else {
            return centered_text(ui, view, t.git_pick, theme);
        };
        let status = self.files.iter().find(|f| f.path == path).map(|f| f.status);
        self.diff_ui(ui, view, (None, path), status, (t.git_before.to_owned(), t.git_after.to_owned()), theme, t);
    }

    /// The commits of the branch, searchable; one picked: its message and files, and the file picked
    /// compared with the commit before.
    fn history_ui(&mut self, ui: &mut Ui, content: Rect, view: Rect, theme: &Theme, t: &Strings) {
        if self.commit.is_some() {
            return self.commit_ui(ui, content, view, theme, t);
        }
        let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(content.shrink2(Vec2::new(6.0, 0.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        panel.add(egui::TextEdit::singleline(&mut self.search).hint_text(t.git_search_commits).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 4.0)));
        panel.add_space(4.0);
        if !self.log_loaded {
            super::loading::inline(&mut panel, theme, t.git_loading_history);
        } else if let Some(e) = &self.log_error {
            panel.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
        } else if self.commits.is_empty() {
            panel.label(egui::RichText::new(t.git_no_commits).size(12.5).color(theme.text_muted));
        } else {
            let needle = self.search.trim().to_lowercase();
            let shown: Vec<usize> = (0..self.commits.len())
                .filter(|&i| {
                    let c = &self.commits[i];
                    needle.is_empty() || c.subject.to_lowercase().contains(&needle) || c.author.to_lowercase().contains(&needle) || c.hash.starts_with(&needle)
                })
                .collect();
            // All read so far: maybe more before.
            let more = self.commits.len() >= self.limit;
            let (mut picked, mut load_more) = (None, false);
            panel.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt(("git-log", &self.root)).auto_shrink(false).show_rows(&mut panel, COMMIT_ROW_H, shown.len() + more as usize, |ui, range| {
                for i in range {
                    let Some(&k) = shown.get(i) else {
                        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), COMMIT_ROW_H), Sense::hover());
                        if ui.put(Rect::from_center_size(row.center(), Vec2::new(140.0, 26.0)), egui::Button::new(t.git_load_more)).clicked() {
                            load_more = true;
                        }
                        continue;
                    };
                    if commit_row(ui, &self.commits[k], theme, t) {
                        picked = Some(self.commits[k].clone());
                    }
                }
            });
            if load_more {
                self.limit += LOG_PAGE;
                self.load_log(ui.ctx());
            }
            if let Some(commit) = picked {
                self.reset_diff();
                self.open_commit(ui.ctx(), commit);
            }
        }
        centered_text(ui, view, t.git_pick_commit, theme);
    }

    /// The commit open: back to the history, its message, author and files.
    fn commit_ui(&mut self, ui: &mut Ui, content: Rect, view: Rect, theme: &Theme, t: &Strings) {
        let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(content.shrink2(Vec2::new(6.0, 0.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        let Some(open) = &self.commit else { return };
        let c = &open.commit;
        let (mut back, mut picked) = (false, None);
        egui::ScrollArea::vertical().id_salt(("git-commit", &self.root, &c.hash)).auto_shrink(false).show(&mut panel, |ui| {
            if ui.add(egui::Button::new(egui::RichText::new(format!("←  {}", t.git_tab_history)).size(12.5)).frame_when_inactive(false)).clicked() {
                back = true;
            }
            ui.add_space(6.0);
            ui.add(egui::Label::new(egui::RichText::new(&c.subject).size(13.5).strong().color(theme.text)).wrap());
            // The message below its first line.
            if let Some(Ok((message, _))) = &open.detail {
                let rest = message.split_once('\n').map(|(_, r)| r.trim()).unwrap_or("");
                if !rest.is_empty() {
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(egui::RichText::new(rest).size(12.0).color(theme.fg)).wrap());
                }
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new(&c.author).size(12.0).color(theme.text)).on_hover_text(&c.email);
            ui.label(egui::RichText::new(format!("{}  ·  {}", date(c.time, "%Y-%m-%d %H:%M"), ago(c.time, t))).size(11.5).color(theme.text_muted));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&c.short).monospace().size(11.5).color(theme.accent)).on_hover_text(&c.hash);
                if ui.small_button(t.git_copy_hash).clicked() {
                    ui.ctx().copy_text(c.hash.clone());
                }
            });
            if c.merge {
                ui.label(egui::RichText::new(t.git_merge_note).size(11.5).color(theme.text_muted));
            }
            ui.add_space(6.0);
            ui.separator();
            match &open.detail {
                None => super::loading::inline(ui, theme, t.git_loading_commit),
                Some(Err(e)) => {
                    ui.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                }
                Some(Ok((_, files))) => {
                    ui.label(egui::RichText::new(t.git_commit_files.replace("{n}", &files.len().to_string())).size(11.5).color(theme.text_muted));
                    ui.add_space(2.0);
                    for change in files {
                        if file_row(ui, change, open.file.as_ref() == Some(&change.path), theme, t) {
                            picked = Some(change.path.clone());
                        }
                    }
                }
            }
        });
        if back {
            self.commit = None;
            self.reset_diff();
            return;
        }
        if let Some(path) = picked {
            if open.file.as_ref() != Some(&path) {
                self.pick_commit_file(ui.ctx(), path);
            }
        }
        let Some(open) = &self.commit else { return };
        let Some(path) = open.file.clone() else {
            return match &open.detail {
                None => super::loading::screen(ui, view, theme, t.git_loading_commit, Some(&open.commit.short), None),
                Some(_) => centered_text(ui, view, t.git_pick, theme),
            };
        };
        let status = match &open.detail {
            Some(Ok((_, files))) => files.iter().find(|f| f.path == path).map(|f| f.status),
            _ => None,
        };
        let short = |h: &str| h.chars().take(open.commit.short.len().max(7)).collect::<String>();
        let before = match &open.commit.parent {
            Some(p) => t.git_before_commit.replace("{hash}", &short(p)),
            None => t.git_before_root.to_owned(),
        };
        let after = t.git_after_commit.replace("{hash}", &open.commit.short);
        let key = (Some(open.commit.hash.clone()), path);
        self.diff_ui(ui, view, key, status, (before, after), theme, t);
    }

    /// The file picked: its two versions side by side, the changes tinted.
    /// `labels`: above the version before, and the one after.
    #[allow(clippy::too_many_arguments)]
    fn diff_ui(&mut self, ui: &mut Ui, view: Rect, key: DiffKey, status: Option<Status>, labels: (String, String), theme: &Theme, t: &Strings) {
        let path = key.1.clone();
        let Some((_, diff)) = self.diff.as_ref().filter(|(k, _)| *k == key) else {
            super::loading::screen(ui, view, theme, t.git_loading_diff, Some(&path), None);
            return;
        };
        let (rows, hunks, added, removed, widest) = match diff {
            Ok(Diff::Rows { rows, hunks, added, removed, widest }) => (rows, hunks, *added, *removed, *widest),
            Ok(Diff::Binary) => return centered(ui, view, t.git_binary, theme.text_muted),
            Ok(Diff::TooBig) => return centered(ui, view, t.git_too_big, theme.text_muted),
            Err(e) => return centered(ui, view, e, theme.ansi[1]),
        };
        let font = FontId::monospace(12.5);
        let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 3.0;
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));

        // Its path, what was added and removed, and ↑ ↓ from one change to the next.
        let head = Rect::from_min_size(view.min, Vec2::new(view.width(), 30.0));
        ui.painter().rect_filled(head, 0.0, theme.chrome_bg);
        ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, theme.tab_hover));
        let mut job = egui::text::LayoutJob::default();
        job.append(&path, 0.0, egui::TextFormat::simple(FontId::monospace(12.5), theme.text));
        if let Some(s) = status {
            job.append(&format!("   {}", s.label(t)), 0.0, egui::TextFormat::simple(FontId::proportional(12.0), s.color(theme)));
        }
        job.append(&format!("   +{added}"), 0.0, egui::TextFormat::simple(FontId::monospace(12.0), theme.ansi[2]));
        job.append(&format!(" −{removed}"), 0.0, egui::TextFormat::simple(FontId::monospace(12.0), theme.ansi[1]));
        job.wrap = egui::text::TextWrapping::truncate_at_width(head.width() - 90.0);
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(Pos2::new(head.min.x + 10.0, head.center().y - galley.size().y / 2.0), galley, theme.text);
        let nav = |ui: &mut Ui, right: f32, text: &str, tip: &str, enabled: bool| {
            let at = Rect::from_min_size(Pos2::new(right - 26.0, head.min.y + 3.0), Vec2::new(26.0, 24.0));
            ui.put(at, egui::Button::new(egui::RichText::new(text).size(14.0)).frame_when_inactive(false).corner_radius(5.0)).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && enabled
        };
        let many = !hunks.is_empty();
        if nav(ui, head.max.x - 6.0, "↓", t.git_next_change, many) {
            self.hunk = (self.hunk + 1) % hunks.len();
            self.scroll_to = Some((hunks[self.hunk] as f32 - 3.0).max(0.0) * row_h);
        }
        if nav(ui, head.max.x - 34.0, "↑", t.git_prev_change, many) {
            self.hunk = (self.hunk + hunks.len() - 1) % hunks.len();
            self.scroll_to = Some((hunks[self.hunk] as f32 - 3.0).max(0.0) * row_h);
        }

        // Below: the two versions, a sideways scroll bar under each, and the map of the file on the right.
        const MAP_W: f32 = 46.0;
        const BAR_H: f32 = 11.0;
        let titles = Rect::from_min_max(Pos2::new(view.min.x, head.max.y + 1.0), Pos2::new(view.max.x - MAP_W, head.max.y + 23.0));
        let map = Rect::from_min_max(Pos2::new(view.max.x - MAP_W, titles.min.y), view.max);
        let code = Rect::from_min_max(Pos2::new(view.min.x, titles.max.y), Pos2::new(map.min.x - 1.0, view.max.y - BAR_H));
        let min_side = 120.0_f32.min(code.width() / 2.0);
        let mid = code.min.x + (code.width() * self.split).clamp(min_side, code.width() - min_side);
        ui.painter().text(Pos2::new(titles.min.x + 10.0, titles.center().y), Align2::LEFT_CENTER, &labels.0, FontId::proportional(11.5), theme.text_muted);
        ui.painter().text(Pos2::new(mid + 11.0, titles.center().y), Align2::LEFT_CENTER, &labels.1, FontId::proportional(11.5), theme.text_muted);
        ui.painter().vline(map.min.x - 0.5, map.y_range(), Stroke::new(1.0, theme.tab_hover));

        let syntax = super::editor::Syntax::detect(path.rsplit('/').next().unwrap_or(&path));
        let gutter = 44.0;
        let (red, green) = (theme.ansi[1], theme.ansi[2]);
        // Sideways: both versions together, with the trackpad, or with the bars below.
        let text_w = widest as f32 * char_w + 24.0;
        let side_w = (mid - code.min.x).min(code.max.x - mid - 1.0) - gutter;
        let max_h = (text_w - side_w).max(0.0);
        if ui.rect_contains_pointer(code) {
            let dx = ui.input(|i| i.smooth_scroll_delta.x);
            if dx != 0.0 {
                self.hscroll -= dx;
            }
        }
        self.hscroll = self.hscroll.clamp(0.0, max_h);
        let hscroll = self.hscroll;

        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(code).layout(egui::Layout::top_down(egui::Align::Min)));
        // Rows exactly `row_h` apart: the map, its frame and ↑ ↓ count on it.
        area.spacing_mut().item_spacing.y = 0.0;
        let mut scroll = egui::ScrollArea::vertical().id_salt(("git-diff", &self.root, &key)).auto_shrink(false);
        if let Some(offset) = self.scroll_to.take() {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        let out = scroll.show_rows(&mut area, row_h, rows.len(), |ui, range| {
            for row in &rows[range] {
                let (r, _) = ui.allocate_exact_size(Vec2::new(code.width(), row_h), Sense::hover());
                let left = Rect::from_min_max(r.min, Pos2::new(mid, r.max.y));
                let right = Rect::from_min_max(Pos2::new(mid + 1.0, r.min.y), r.max);
                let (left_tint, right_tint) = match row.kind {
                    RowKind::Same => (None, None),
                    RowKind::Removed => (Some(red), None),
                    RowKind::Added => (None, Some(green)),
                    RowKind::Changed => (Some(red), Some(green)),
                };
                for (side, line, tint) in [(left, &row.left, left_tint), (right, &row.right, right_tint)] {
                    let painter = ui.painter_at(side);
                    match line {
                        Some((number, text)) => {
                            if let Some(c) = tint {
                                painter.rect_filled(side, 0.0, c.gamma_multiply(0.14));
                                painter.rect_filled(Rect::from_min_size(side.min, Vec2::new(3.0, side.height())), 0.0, c.gamma_multiply(0.8));
                            }
                            painter.text(Pos2::new(side.min.x + gutter - 8.0, side.center().y), Align2::RIGHT_CENTER, number.to_string(), font.clone(), theme.text_muted.gamma_multiply(0.8));
                            // The text scrolls sideways under the numbers, which stay.
                            let text_clip = Rect::from_min_max(Pos2::new(side.min.x + gutter - 4.0, side.min.y), side.max);
                            let mut job = super::editor::highlight(text, syntax, theme, &font);
                            job.wrap.max_width = f32::INFINITY;
                            let galley = painter.layout_job(job);
                            ui.painter_at(text_clip).galley(Pos2::new(side.min.x + gutter - hscroll, side.center().y - galley.size().y / 2.0), galley, theme.text);
                        }
                        // The other side has lines here: this one none.
                        None => {
                            painter.rect_filled(side, 0.0, theme.chrome_bg.gamma_multiply(0.6));
                        }
                    }
                }
            }
        });
        self.v_offset = out.state.offset.y;
        self.v_visible = out.inner_rect.height();

        // Where the versions meet: dragged to give one more room.
        let full = Rect::from_min_max(Pos2::new(code.min.x, titles.min.y), Pos2::new(code.max.x, view.max.y));
        ui.painter().vline(mid, full.y_range(), Stroke::new(1.0, theme.tab_hover));
        if let Some(dx) = splitter(ui, egui::Id::new(("git-split", &self.root)), mid, full.y_range(), theme) {
            self.split = ((mid + dx - code.min.x) / code.width()).clamp(0.1, 0.9);
        }

        // A sideways bar under each version, moving both.
        let bars = Rect::from_min_max(Pos2::new(code.min.x, code.max.y), Pos2::new(code.max.x, view.max.y));
        ui.painter().rect_filled(bars, 0.0, theme.bg);
        if max_h > 0.0 {
            for (i, track) in [
                Rect::from_min_max(Pos2::new(bars.min.x + gutter, bars.min.y + 2.0), Pos2::new(mid - 4.0, bars.max.y - 2.0)),
                Rect::from_min_max(Pos2::new(mid + 1.0 + gutter, bars.min.y + 2.0), Pos2::new(bars.max.x - 4.0, bars.max.y - 2.0)),
            ]
            .into_iter()
            .enumerate()
            {
                if track.width() < 30.0 {
                    continue;
                }
                let thumb_w = (track.width() * side_w / text_w).clamp(24.0, track.width());
                let travel = track.width() - thumb_w;
                let thumb = Rect::from_min_size(Pos2::new(track.min.x + travel * hscroll / max_h, track.min.y), Vec2::new(thumb_w, track.height()));
                let resp = ui.interact(track, egui::Id::new(("git-hbar", &self.root, i)), Sense::click_and_drag());
                if resp.dragged() && travel > 0.0 {
                    self.hscroll = (self.hscroll + resp.drag_delta().x * max_h / travel).clamp(0.0, max_h);
                } else if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        self.hscroll = (((p.x - track.min.x - thumb_w / 2.0) / travel.max(1.0)) * max_h).clamp(0.0, max_h);
                    }
                }
                let hot = resp.hovered() || resp.dragged();
                ui.painter().rect_filled(thumb, 4.0, theme.text_muted.gamma_multiply(if hot { 0.55 } else { 0.3 }));
            }
        }

        if let Some(offset) = map_ui(ui, map, rows, row_h, theme, egui::Id::new(("git-map", &self.root)), (self.v_offset, self.v_visible)) {
            self.scroll_to = Some(offset);
        }
    }
}

/// "Changes | History | Issues" above the list, each as wide as its label needs; the one clicked.
fn mode_tabs(ui: &mut Ui, rect: Rect, mode: Mode, root: &Path, theme: &Theme, t: &Strings) -> Option<Mode> {
    ui.painter().rect_filled(rect, 6.0, theme.bg);
    let tabs = [(Mode::Changes, t.git_tab_changes), (Mode::History, t.git_tab_history), (Mode::Issues, t.git_tab_issues)];
    let font = FontId::proportional(12.5);
    let widths: Vec<f32> = tabs.iter().map(|(_, label)| ui.painter().layout_no_wrap(label.to_string(), font.clone(), theme.text).size().x + 14.0).collect();
    // The room left shared out evenly (or taken back, when too narrow).
    let extra = (rect.width() - widths.iter().sum::<f32>()) / tabs.len() as f32;
    let mut clicked = None;
    let mut x = rect.min.x;
    for (k, ((m, label), w)) in tabs.into_iter().zip(widths).enumerate() {
        let w = (w + extra).max(20.0);
        let cell = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w, rect.height())).shrink(2.0);
        x += w;
        let resp = ui.interact(cell, egui::Id::new(("git-mode", root, k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let active = m == mode;
        if active || resp.hovered() {
            ui.painter().rect_filled(cell, 5.0, if active { theme.tab_active } else { theme.tab_hover });
        }
        let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), font.clone(), if active { theme.text } else { theme.text_muted });
        job.wrap = egui::text::TextWrapping::truncate_at_width(cell.width() - 4.0);
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(cell.center() - galley.size() / 2.0, galley, theme.text);
        if resp.clicked() {
            clicked = Some(m);
        }
    }
    clicked
}

/// A file in a list: its icon, name, folder and status letter; true when clicked.
fn file_row(ui: &mut Ui, change: &FileChange, selected: bool, theme: &Theme, t: &Strings) -> bool {
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(row, 5.0, if selected { theme.tab_active } else { theme.tab_hover });
    }
    let (dir, name) = match change.path.rsplit_once('/') {
        Some((d, n)) => (d, n),
        None => ("", change.path.as_str()),
    };
    super::files::paint_file_icon(ui.painter(), Rect::from_center_size(Pos2::new(row.min.x + 14.0, row.center().y), Vec2::splat(15.0)), name, false, false, None, theme);
    ui.painter().text(Pos2::new(row.max.x - 12.0, row.center().y), Align2::CENTER_CENTER, change.status.letter(), FontId::monospace(12.5), change.status.color(theme));
    let mut job = egui::text::LayoutJob::default();
    let mut name_format = egui::TextFormat::simple(FontId::proportional(13.0), if selected { theme.text } else { theme.fg });
    if change.status == Status::Deleted {
        name_format.strikethrough = Stroke::new(1.0, theme.text_muted);
    }
    job.append(name, 0.0, name_format);
    if !dir.is_empty() {
        job.append(&format!("  {dir}"), 0.0, egui::TextFormat::simple(FontId::proportional(11.5), theme.text_muted));
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(row.width() - 52.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(Pos2::new(row.min.x + 28.0, row.center().y - galley.size().y / 2.0), galley, theme.text);
    let mut tip = format!("{}\n{}", change.path, change.status.label(t));
    if let Some(old) = &change.old_path {
        tip.push_str(&format!(" ({old} → {})", change.path));
    }
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A commit in the history: a dot on the line of commits, its branches and tags, its subject; below,
/// its hash, author and age. True when clicked.
fn commit_row(ui: &mut Ui, c: &Commit, theme: &Theme, t: &Strings) -> bool {
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), COMMIT_ROW_H), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(row.shrink2(Vec2::new(0.0, 1.0)), 5.0, theme.tab_hover);
    }
    let painter = ui.painter_at(row);
    let dot = Pos2::new(row.min.x + 11.0, row.min.y + 15.0);
    painter.vline(dot.x, row.y_range(), Stroke::new(1.5, theme.text_muted.gamma_multiply(0.3)));
    let head = c.refs.iter().any(|r| matches!(r, Ref::Head(_)));
    if head {
        painter.circle_filled(dot, 4.5, theme.accent);
    } else if c.merge {
        painter.circle_filled(dot, 4.0, theme.chrome_bg);
        painter.circle_stroke(dot, 3.5, Stroke::new(1.5, theme.text_muted));
    } else {
        painter.circle_filled(dot, 3.5, theme.text_muted);
    }
    let left = row.min.x + 24.0;
    let right = row.max.x - 6.0;
    let mut x = left;
    for r in &c.refs {
        let (text, color) = match r {
            Ref::Head(b) => (b, theme.accent),
            Ref::Branch(b) => (b, theme.ansi[4]),
            Ref::Tag(tag) => (tag, theme.ansi[3]),
        };
        let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(11.0), color);
        let w = galley.size().x + 10.0;
        // Room left for the subject.
        if x + w > right - 60.0 {
            break;
        }
        let chip = Rect::from_min_size(Pos2::new(x, dot.y - 8.5), Vec2::new(w, 17.0));
        painter.rect_filled(chip, 4.0, color.gamma_multiply(0.16));
        if matches!(r, Ref::Head(_)) {
            painter.rect_stroke(chip, 4.0, Stroke::new(1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        }
        painter.galley(chip.center() - galley.size() / 2.0, galley, color);
        x += w + 5.0;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(c.subject.clone(), FontId::proportional(13.0), theme.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - x).max(10.0));
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(x, dot.y - galley.size().y / 2.0), galley, theme.text);
    let mut job = egui::text::LayoutJob::default();
    job.append(&c.short, 0.0, egui::TextFormat::simple(FontId::monospace(11.5), theme.accent.gamma_multiply(0.8)));
    job.append(&format!("   {}  ·  {}", c.author, ago(c.time, t)), 0.0, egui::TextFormat::simple(FontId::proportional(11.5), theme.text_muted));
    job.wrap = egui::text::TextWrapping::truncate_at_width(right - left);
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(left, row.min.y + 32.0 - galley.size().y / 2.0), galley, theme.text);
    let tip = format!("{}\n\n{}\n{} <{}>\n{}", c.subject, c.hash, c.author, c.email, date(c.time, "%Y-%m-%d %H:%M"));
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Fetch: an arrow down onto a line.
fn paint_fetch_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    painter.line_segment([c + Vec2::new(0.0, -6.0), c + Vec2::new(0.0, 2.5)], stroke);
    painter.add(egui::Shape::line(vec![c + Vec2::new(-3.5, -1.0), c + Vec2::new(0.0, 2.5), c + Vec2::new(3.5, -1.0)], stroke));
    painter.line_segment([c + Vec2::new(-5.5, 6.0), c + Vec2::new(5.5, 6.0)], stroke);
}

/// A window with an arrow out of its corner: open in a new window.
/// A curved arrow going back, to the left: back to the terminal (or to all the repositories).
fn paint_return_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    // Half a circle open to the left, from its top to its bottom, then the stem back to the left.
    let (center, r) = (c + Vec2::new(1.5, 0.5), 4.5);
    let mut arc: Vec<Pos2> = (0..=12).map(|k| center + Vec2::angled(-std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * k as f32 / 12.0) * r).collect();
    arc.push(c + Vec2::new(-4.5, 0.5 + r));
    arc.reverse();
    painter.add(egui::Shape::line(arc, stroke));
    // On the top end, a short stem to the left and its head.
    let tip = center + Vec2::new(-6.5, -r);
    painter.line_segment([tip, center + Vec2::new(0.0, -r)], stroke);
    painter.add(egui::Shape::line(vec![tip + Vec2::new(3.0, -3.0), tip, tip + Vec2::new(3.0, 3.0)], stroke));
}

fn paint_window_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let frame = Rect::from_center_size(c + Vec2::new(-1.0, 1.0), Vec2::splat(10.0));
    // The frame, open at its top right corner where the arrow leaves.
    painter.add(egui::Shape::line(vec![Pos2::new(frame.center().x, frame.min.y), frame.left_top(), frame.left_bottom(), frame.right_bottom(), Pos2::new(frame.max.x, frame.center().y)], stroke));
    let tip = c + Vec2::new(6.0, -6.0);
    painter.line_segment([c + Vec2::new(0.5, -0.5), tip], stroke);
    painter.line_segment([tip, tip + Vec2::new(-4.0, 0.0)], stroke);
    painter.line_segment([tip, tip + Vec2::new(0.0, 4.0)], stroke);
}

/// ✕, drawn (the font's is smaller than the other icons).
fn paint_close_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    painter.line_segment([c + Vec2::new(-5.0, -5.0), c + Vec2::new(5.0, 5.0)], stroke);
    painter.line_segment([c + Vec2::new(-5.0, 5.0), c + Vec2::new(5.0, -5.0)], stroke);
}

fn centered_text(ui: &mut Ui, rect: Rect, text: &str, theme: &Theme) {
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), theme.text_muted);
}

/// The whole file in miniature, VS Code style: its lines as strokes, the changes in red and green,
/// and the part in sight (`sight`: scroll offset and height); a click or a drag there gives the offset
/// to scroll to.
fn map_ui(ui: &mut Ui, map: Rect, rows: &[Row], row_h: f32, theme: &Theme, id: egui::Id, sight: (f32, f32)) -> Option<f32> {
    let painter = ui.painter_at(map);
    painter.rect_filled(map, 0.0, theme.chrome_bg);
    let n = rows.len();
    if n == 0 {
        return None;
    }
    let inner = map.shrink2(Vec2::new(4.0, 4.0));
    // Up to 3 px a line; a long file: several lines a pixel.
    let used_h = (inner.height()).min(n as f32 * 3.0);
    let buckets = n.min(inner.height().max(1.0) as usize).max(1);
    let bucket_h = used_h / buckets as f32;
    let half = (inner.width() - 2.0) / 2.0;
    let (red, green) = (theme.ansi[1], theme.ansi[2]);
    let stroke_color = theme.text_muted.gamma_multiply(0.45);
    for b in 0..buckets {
        let (from, to) = (b * n / buckets, ((b + 1) * n / buckets).max(b * n / buckets + 1));
        let y = inner.min.y + b as f32 * bucket_h;
        for (k, tint) in [(0usize, red), (1, green)] {
            let x0 = inner.min.x + k as f32 * (half + 2.0);
            let mut changed = false;
            let (mut indent, mut len) = (usize::MAX, 0usize);
            for row in &rows[from..to.min(n)] {
                let line = if k == 0 { &row.left } else { &row.right };
                changed |= match row.kind {
                    RowKind::Same => false,
                    RowKind::Removed => k == 0,
                    RowKind::Added => k == 1,
                    RowKind::Changed => true,
                } && line.is_some();
                if let Some((_, text)) = line {
                    let trimmed = text.trim_start();
                    if !trimmed.is_empty() {
                        indent = indent.min(text.len() - trimmed.len());
                        len = len.max(text.chars().count());
                    }
                }
            }
            let band = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(half, bucket_h.max(1.0)));
            if changed {
                painter.rect_filled(band, 0.0, tint.gamma_multiply(0.55));
            }
            if len > 0 {
                let scale = half / 100.0;
                let start = (indent.min(100) as f32 * scale).min(half);
                let end = (len.min(100) as f32 * scale).max(start + 1.0);
                let h = (bucket_h * 0.6).max(1.0);
                painter.rect_filled(Rect::from_min_size(Pos2::new(x0 + start, y + (bucket_h - h) / 2.0), Vec2::new(end - start, h)), 0.0, if changed { tint } else { stroke_color });
            }
        }
    }
    // The part in sight.
    let total = n as f32 * row_h;
    let visible = sight.1.min(total);
    let frame = Rect::from_min_size(Pos2::new(map.min.x + 1.0, inner.min.y + sight.0 / total * used_h), Vec2::new(map.width() - 2.0, (visible / total * used_h).max(6.0)));
    painter.rect_filled(frame, 2.0, theme.text.gamma_multiply(0.08));
    painter.rect_stroke(frame, 2.0, Stroke::new(1.0, theme.text.gamma_multiply(0.18)), egui::StrokeKind::Inside);
    let resp = ui.interact(map, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if !(resp.is_pointer_button_down_on() || resp.dragged() || resp.clicked()) {
        return None;
    }
    let p = resp.interact_pointer_pos()?;
    ui.ctx().request_repaint();
    let fraction = ((p.y - inner.min.y) / used_h).clamp(0.0, 1.0);
    Some((fraction * total - visible / 2.0).clamp(0.0, (total - visible).max(0.0)))
}

/// A vertical line that can be dragged sideways: how far it moved this frame.
pub(super) fn splitter(ui: &mut Ui, id: egui::Id, x: f32, y: egui::Rangef, theme: &Theme) -> Option<f32> {
    let rect = Rect::from_x_y_ranges(x - 3.0..=x + 3.0, y);
    let resp = ui.interact(rect, id, Sense::drag());
    if resp.hovered() || resp.dragged() {
        ui.painter().vline(x, y, Stroke::new(2.0, theme.accent.gamma_multiply(0.8)));
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    (resp.dragged() && resp.drag_delta().x != 0.0).then(|| resp.drag_delta().x)
}

fn centered(ui: &mut Ui, rect: Rect, text: &str, color: Color32) {
    ui.put(rect.shrink(24.0), egui::Label::new(egui::RichText::new(text).size(13.5).color(color)).wrap());
}

/// A branch: two commits on a line, a third branching off.
pub(super) fn paint_branch_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = rect.width() / 16.0;
    let at = |x: f32, y: f32| rect.min + Vec2::new(x * s, y * s);
    let stroke = Stroke::new(1.5 * s, color);
    painter.line_segment([at(5.0, 4.5), at(5.0, 11.5)], stroke);
    painter.add(egui::Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
        [at(11.0, 6.5), at(11.0, 9.5), at(5.0, 8.5), at(5.0, 11.0)],
        false,
        Color32::TRANSPARENT,
        stroke,
    )));
    for (x, y) in [(5.0, 3.0), (5.0, 13.0), (11.0, 4.5)] {
        painter.circle_stroke(at(x, y), 1.8 * s, stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_status() {
        let out = b"## main...origin/main [ahead 1]\0 M src/a.rs\0?? new file.txt\0R  b2.rs\0b.rs\0D  gone.rs\0UU both.rs\0";
        let (head, files) = parse_status(out);
        assert_eq!(head, Head { branch: "main".into(), upstream: Some("origin/main".into()), ahead: 1, behind: 0 });
        let find = |p: &str| files.iter().find(|f| f.path == p).unwrap();
        assert_eq!(find("src/a.rs").status, Status::Modified);
        assert_eq!(find("new file.txt").status, Status::Untracked);
        assert_eq!(find("b2.rs").old_path.as_deref(), Some("b.rs"));
        assert_eq!(find("gone.rs").status, Status::Deleted);
        assert_eq!(find("both.rs").status, Status::Conflict);
        assert_eq!(files.len(), 5);
        assert_eq!(parse_status(b"## No commits yet on dev\0").0.branch, "dev");
    }

    #[test]
    fn reads_head() {
        assert_eq!(parse_head("dev...origin/dev [ahead 2, behind 3]"), Head { branch: "dev".into(), upstream: Some("origin/dev".into()), ahead: 2, behind: 3 });
        assert_eq!(parse_head("dev...origin/dev [gone]").ahead, 0);
        assert_eq!(parse_head("local"), Head { branch: "local".into(), ..Head::default() });
        assert_eq!(parse_head("HEAD (no branch)"), Head::default());
    }

    #[test]
    fn reads_log() {
        let out = "aaaa\x1faa\x1fpppp qqqq\x1fAnn\x1fann@x.org\x1f1700000000\x1fHEAD -> main, origin/main, origin/HEAD, tag: v1\x1fMerge: a \x1f b\x1e\nbbbb\x1fbb\x1f\x1fBob\x1fbob@x.org\x1f1600000000\x1f\x1fFirst\x1e\n";
        let commits = parse_log(out.as_bytes());
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].parent.as_deref(), Some("pppp"));
        assert!(commits[0].merge);
        assert_eq!(commits[0].subject, "Merge: a \x1f b");
        assert_eq!(commits[0].refs, vec![Ref::Head("main".into()), Ref::Branch("origin/main".into()), Ref::Tag("v1".into())]);
        assert_eq!(commits[1].parent, None);
        assert!(commits[1].refs.is_empty());
    }

    #[test]
    fn reads_commit_files() {
        let files = parse_name_status(b"M\0src/a.rs\0R087\0old.rs\0new.rs\0A\0added.txt\0D\0gone.rs\0");
        assert_eq!(files.len(), 4);
        let renamed = files.iter().find(|f| f.path == "new.rs").unwrap();
        assert_eq!((renamed.status, renamed.old_path.as_deref()), (Status::Renamed, Some("old.rs")));
        assert_eq!(files.iter().find(|f| f.path == "added.txt").unwrap().status, Status::Added);
    }

    #[test]
    fn reads_branches() {
        let b = parse_branches(b"refs/heads/main\x1forigin/main\nrefs/heads/wip\x1f\nrefs/remotes/origin/HEAD\x1f\nrefs/remotes/origin/main\x1f\n");
        assert_eq!(b.local, vec![("main".into(), "origin/main".into()), ("wip".into(), String::new())]);
        assert_eq!(b.remote, vec!["origin/main".to_owned()]);
    }

    #[test]
    fn compares_side_by_side() {
        let Diff::Rows { rows, hunks, added, removed, .. } = compare("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n") else { panic!() };
        assert_eq!((added, removed), (2, 1));
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[1].kind, RowKind::Changed);
        assert_eq!(rows[1].left, Some((2, "b".into())));
        assert_eq!(rows[1].right, Some((2, "B".into())));
        assert_eq!(rows[4], Row { left: None, right: Some((5, "e".into())), kind: RowKind::Added });
        assert_eq!(hunks, vec![1, 4]);
    }

    #[test]
    fn finds_repositories_inside() {
        let dir = std::env::temp_dir().join(format!("ronnie-repos-{}", std::process::id()));
        for sub in ["b-api/.git", "A-front/.git", "notes", ".hidden/.git"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let names: Vec<String> = child_repos(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["A-front", "b-api"]);
        // Not in a repository itself: the folder is what the view shows.
        if repo_root(&dir).is_none() {
            assert_eq!(git_root(&dir), Some(dir.clone()));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_this_repository() {
        let here = std::env::current_dir().unwrap();
        let root = repo_root(&here.join("src/app")).expect("inside a repository");
        assert!(root.join(".git").exists());
        let (head, files) = read_status(&root).expect("git status");
        // Checked out on a branch, or detached (CI).
        let _ = head;
        // Whatever changed here compares without error.
        if let Some(change) = files.iter().find(|f| f.status == Status::Modified) {
            assert!(matches!(read_diff(&root, change, None), Ok(Diff::Rows { .. } | Diff::Binary | Diff::TooBig)));
        }
        // The history, and what its last commit changed.
        let commits = read_log(&root, 5).expect("git log");
        assert!(!commits.is_empty());
        let c = &commits[0];
        let (message, files) = read_commit(&root, &c.hash, c.parent.as_deref()).expect("git diff-tree");
        assert!(message.starts_with(&c.subject));
        if let Some(change) = files.first() {
            assert!(read_diff(&root, change, Some((&c.hash, c.parent.as_deref()))).is_ok());
        }
        assert!(read_branches(&root).is_ok());
    }
}
