//! The pull requests (GitHub) or merge requests (GitLab) of a repository, in its git view: the list (open,
//! merged, closed or all, searchable), one with its checks, description, reviews and comments, and the
//! files it changes, compared side by side like a commit's (its commits are fetched, nothing else
//! changes in the folder). Comment, approve, merge, close or reopen it, or check its branch out. Through
//! `gh` or `glab`, as the issues.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::git::{file_row, FileChange};
use super::issues::{explain, forge, gh, gl_author, gl_labels, glab, label_chip, login, post, timestamp, Author, Forge, GlLabel, GlUser, Label};
use super::*;

/// The list is read again this often while it is shown.
const REFRESH: Duration = Duration::from_secs(60);
const ROW_H: f32 = 46.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Open,
    Draft,
    Merged,
    Closed,
}

impl State {
    /// GitHub's "OPEN" / "MERGED" / "CLOSED" (and its draft flag), GitLab's "opened" / "merged" / "closed"
    /// / "locked".
    fn of(state: &str, draft: bool) -> Self {
        match state.to_ascii_lowercase().as_str() {
            "merged" => Self::Merged,
            "open" | "opened" if draft => Self::Draft,
            "open" | "opened" => Self::Open,
            _ => Self::Closed,
        }
    }

    fn color(self, theme: &Theme) -> Color32 {
        match self {
            Self::Open => theme.ansi[2],
            Self::Draft => theme.text_muted,
            Self::Merged => theme.ansi[5],
            Self::Closed => theme.ansi[1],
        }
    }

    fn label(self, t: &Strings) -> &'static str {
        match self {
            Self::Open => t.pulls_state_open,
            Self::Draft => t.pulls_state_draft,
            Self::Merged => t.pulls_state_merged,
            Self::Closed => t.pulls_state_closed,
        }
    }

    fn open(self) -> bool {
        matches!(self, Self::Open | Self::Draft)
    }
}

#[derive(Clone, PartialEq, Debug)]
struct Pull {
    number: u64,
    title: String,
    state: State,
    author: Option<Author>,
    labels: Vec<Label>,
    created_at: String,
    head: String,
    base: String,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Check {
    Pending,
    Passed,
    Failed,
    Skipped,
}

/// A check run on the last commit: its name, how it went, where to see it.
#[derive(Clone, PartialEq, Debug)]
struct CheckRun {
    name: String,
    state: Check,
    url: String,
}

/// The description, a comment or a review, in order.
#[derive(Clone, PartialEq, Debug)]
struct Entry {
    author: String,
    at: String,
    body: String,
    /// A review: what it said ("APPROVED", "CHANGES_REQUESTED", "COMMENTED").
    review: Option<String>,
}

#[derive(Clone, PartialEq, Debug)]
struct Detail {
    number: u64,
    title: String,
    state: State,
    author: Option<Author>,
    labels: Vec<Label>,
    created_at: String,
    body: String,
    url: String,
    head: String,
    base: String,
    /// The last commit of the branch, and the one of the base it is compared with (empty: unknown).
    head_oid: String,
    base_oid: String,
    /// Lines added and removed (GitHub only).
    added: u64,
    removed: u64,
    /// "APPROVED", "CHANGES_REQUESTED", "REVIEW_REQUIRED" (GitHub), or empty.
    decision: String,
    /// Can't be merged as it is.
    conflicts: bool,
    checks: Vec<CheckRun>,
    entries: Vec<Entry>,
}

// What gh gives.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPull {
    number: u64,
    title: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    labels: Vec<Label>,
    created_at: String,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    base_ref_name: String,
}

impl From<GhPull> for Pull {
    fn from(p: GhPull) -> Self {
        Pull { number: p.number, title: p.title, state: State::of(&p.state, p.is_draft), author: p.author, labels: p.labels, created_at: p.created_at, head: p.head_ref_name, base: p.base_ref_name }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhComment {
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    body: String,
    created_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhReview {
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    submitted_at: Option<String>,
    #[serde(default)]
    state: String,
}

/// A check run (GitHub Actions...) or an older commit status: one or the other's fields.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhCheck {
    #[serde(default)]
    name: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    details_url: String,
    #[serde(default)]
    target_url: String,
}

impl From<GhCheck> for CheckRun {
    fn from(c: GhCheck) -> Self {
        let state = match (c.status.as_str(), c.conclusion.as_str(), c.state.as_str()) {
            (_, "SUCCESS", _) | (_, _, "SUCCESS") => Check::Passed,
            (_, "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE", _) | (_, _, "FAILURE" | "ERROR") => Check::Failed,
            (_, "SKIPPED" | "NEUTRAL" | "STALE", _) => Check::Skipped,
            _ => Check::Pending,
        };
        let name = if c.name.is_empty() { c.context } else { c.name };
        CheckRun { name, state, url: if c.details_url.is_empty() { c.target_url } else { c.details_url } }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhDetail {
    number: u64,
    title: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    labels: Vec<Label>,
    created_at: String,
    #[serde(default)]
    body: String,
    url: String,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    head_ref_oid: String,
    #[serde(default)]
    base_ref_oid: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    mergeable: String,
    #[serde(default)]
    status_check_rollup: Vec<GhCheck>,
    #[serde(default)]
    comments: Vec<GhComment>,
    #[serde(default)]
    reviews: Vec<GhReview>,
}

impl From<GhDetail> for Detail {
    fn from(d: GhDetail) -> Self {
        let mut entries: Vec<Entry> = d.comments.into_iter().map(|c| Entry { author: login(&c.author).to_owned(), at: c.created_at, body: c.body, review: None }).collect();
        // A review with nothing to say but comments on lines: those are on GitHub.
        entries.extend(d.reviews.into_iter().filter(|r| r.state != "COMMENTED" || !r.body.trim().is_empty()).filter(|r| r.state != "PENDING").map(|r| Entry {
            author: login(&r.author).to_owned(),
            at: r.submitted_at.unwrap_or_default(),
            body: r.body,
            review: Some(r.state),
        }));
        entries.sort_by_key(|e| timestamp(&e.at));
        Detail {
            number: d.number,
            title: d.title,
            state: State::of(&d.state, d.is_draft),
            author: d.author,
            labels: d.labels,
            created_at: d.created_at,
            body: d.body,
            url: d.url,
            head: d.head_ref_name,
            base: d.base_ref_name,
            head_oid: d.head_ref_oid,
            base_oid: d.base_ref_oid,
            added: d.additions,
            removed: d.deletions,
            decision: d.review_decision.unwrap_or_default(),
            conflicts: d.mergeable == "CONFLICTING",
            checks: d.status_check_rollup.into_iter().map(CheckRun::from).collect(),
            entries,
        }
    }
}

// What GitLab's API gives.

#[derive(Deserialize)]
struct GlMr {
    iid: u64,
    title: String,
    state: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    author: Option<GlUser>,
    #[serde(default)]
    labels: Vec<GlLabel>,
    created_at: String,
    #[serde(default)]
    source_branch: String,
    #[serde(default)]
    target_branch: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    web_url: String,
    #[serde(default)]
    diff_refs: Option<GlRefs>,
    #[serde(default)]
    head_pipeline: Option<GlPipeline>,
    #[serde(default)]
    has_conflicts: bool,
}

#[derive(Deserialize)]
struct GlRefs {
    #[serde(default)]
    base_sha: Option<String>,
    #[serde(default)]
    head_sha: Option<String>,
}

#[derive(Deserialize)]
struct GlPipeline {
    status: String,
    #[serde(default)]
    web_url: String,
}

#[derive(Deserialize)]
struct GlNote {
    #[serde(default)]
    author: Option<GlUser>,
    body: String,
    created_at: String,
    #[serde(default)]
    system: bool,
}

impl From<GlMr> for Pull {
    fn from(m: GlMr) -> Self {
        Pull { number: m.iid, title: m.title, state: State::of(&m.state, m.draft), author: gl_author(m.author), labels: gl_labels(m.labels), created_at: m.created_at, head: m.source_branch, base: m.target_branch }
    }
}

fn gl_check(p: GlPipeline) -> CheckRun {
    let state = match p.status.as_str() {
        "success" => Check::Passed,
        "failed" | "canceled" => Check::Failed,
        "skipped" | "manual" => Check::Skipped,
        _ => Check::Pending,
    };
    CheckRun { name: "Pipeline".into(), state, url: p.web_url }
}

fn read_list(root: &Path, t: &Strings) -> Result<Vec<Pull>, String> {
    match forge(root) {
        None => Err(t.issues_not_github.to_owned()),
        Some(Forge::GitHub) => {
            let out = explain(gh(root, &["pr", "list", "--state", "all", "--limit", "300", "--json", "number,title,state,isDraft,author,labels,createdAt,headRefName,baseRefName"], None), t)?;
            let pulls: Vec<GhPull> = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
            Ok(pulls.into_iter().map(Pull::from).collect())
        }
        Some(Forge::GitLab(host)) => {
            let mut pulls = Vec::new();
            for page in 1..=3 {
                let endpoint = format!("projects/:id/merge_requests?state=all&per_page=100&page={page}&with_labels_details=true");
                let out = glab(root, &host, "GET", &endpoint, &[], None, t)?;
                let batch: Vec<GlMr> = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
                let full = batch.len() == 100;
                pulls.extend(batch.into_iter().map(Pull::from));
                if !full {
                    break;
                }
            }
            Ok(pulls)
        }
    }
}

fn read_detail(root: &Path, number: u64, t: &Strings) -> Result<Detail, String> {
    let n = number.to_string();
    match forge(root) {
        None => Err(t.issues_not_github.to_owned()),
        Some(Forge::GitHub) => {
            let fields = "number,title,state,isDraft,author,labels,createdAt,body,url,headRefName,baseRefName,headRefOid,baseRefOid,additions,deletions,reviewDecision,mergeable,statusCheckRollup,comments,reviews";
            let out = explain(gh(root, &["pr", "view", &n, "--json", fields], None), t)?;
            serde_json::from_slice::<GhDetail>(&out).map(Detail::from).map_err(|e| e.to_string())
        }
        Some(Forge::GitLab(host)) => {
            let out = glab(root, &host, "GET", &format!("projects/:id/merge_requests/{n}?with_labels_details=true"), &[], None, t)?;
            let m: GlMr = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
            let out = glab(root, &host, "GET", &format!("projects/:id/merge_requests/{n}/notes?sort=asc&order_by=created_at&per_page=100"), &[], None, t)?;
            let notes: Vec<GlNote> = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
            let entries = notes.into_iter().filter(|n| !n.system).map(|n| Entry { author: gl_author(n.author).map(|a| a.login).unwrap_or_else(|| "ghost".into()), at: n.created_at, body: n.body, review: None }).collect();
            let (base_oid, head_oid) = m.diff_refs.as_ref().map(|r| (r.base_sha.clone().unwrap_or_default(), r.head_sha.clone().unwrap_or_default())).unwrap_or_default();
            Ok(Detail {
                number: m.iid,
                title: m.title,
                state: State::of(&m.state, m.draft),
                author: gl_author(m.author),
                labels: gl_labels(m.labels),
                created_at: m.created_at,
                body: m.description.unwrap_or_default(),
                url: m.web_url,
                head: m.source_branch,
                base: m.target_branch,
                head_oid,
                base_oid,
                added: 0,
                removed: 0,
                decision: String::new(),
                conflicts: m.has_conflicts,
                checks: m.head_pipeline.map(gl_check).into_iter().collect(),
                entries,
            })
        }
    }
}

/// The files a pull request changes, and the two commits they are compared between.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct PrFiles {
    pub head: String,
    pub base: String,
    pub files: Vec<FileChange>,
}

/// The remote the pull request lives on: the one whose address holds its project's path ("owner/repo"
/// in its address on the forge), else "origin".
fn remote_for(root: &Path, url: &str) -> String {
    let project = url.split_once("://").map_or(url, |(_, r)| r).split_once('/').map_or("", |(_, r)| r);
    let project = project.split("/pull/").next().unwrap_or("").split("/-/").next().unwrap_or("").to_lowercase();
    let mut cmd = super::git::git(root);
    cmd.args(["remote", "-v"]);
    let out = super::git::run(cmd).unwrap_or_default();
    let text = String::from_utf8_lossy(&out);
    let found = text.lines().find_map(|l| {
        let mut words = l.split_whitespace();
        let (name, address) = (words.next()?, words.next()?.to_lowercase());
        (!project.is_empty() && address.trim_end_matches(".git").ends_with(&project)).then(|| name.to_owned())
    });
    found.unwrap_or_else(|| "origin".into())
}

fn has_commit(root: &Path, oid: &str) -> bool {
    let mut cmd = super::git::git(root);
    cmd.args(["cat-file", "-e", &format!("{oid}^{{commit}}")]);
    !oid.is_empty() && super::git::run(cmd).is_ok()
}

/// Fetches `what` from `remote` (no branch of the folder moves): the commit it points to.
fn fetch(root: &Path, remote: &str, what: &str) -> Result<String, String> {
    let mut cmd = super::git::git(root);
    cmd.args(["fetch", "--no-tags", remote, what]);
    super::git::run(cmd)?;
    let mut cmd = super::git::git(root);
    cmd.args(["rev-parse", "FETCH_HEAD"]);
    Ok(String::from_utf8_lossy(&super::git::run(cmd)?).trim().to_owned())
}

/// The pull request's commits, fetched when not here yet, and what changed from where it left its base.
fn read_files(root: &Path, d: &Detail) -> Result<PrFiles, String> {
    let gitlab = d.url.contains("/-/merge_requests/");
    let remote = remote_for(root, &d.url);
    let head = if has_commit(root, &d.head_oid) {
        d.head_oid.clone()
    } else {
        let refspec = if gitlab { format!("refs/merge-requests/{}/head", d.number) } else { format!("refs/pull/{}/head", d.number) };
        fetch(root, &remote, &refspec)?
    };
    let tip = if has_commit(root, &d.base_oid) { d.base_oid.clone() } else { fetch(root, &remote, &d.base)? };
    let mut cmd = super::git::git(root);
    cmd.args(["merge-base", &tip, &head]);
    let base = super::git::run(cmd).map(|out| String::from_utf8_lossy(&out).trim().to_owned()).unwrap_or(tip);
    let mut cmd = super::git::git(root);
    cmd.args(["diff", "--name-status", "-z", "-M", &base, &head]);
    Ok(PrFiles { files: super::git::parse_name_status(&super::git::run(cmd)?), head, base })
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Method {
    Squash,
    Merge,
    Rebase,
}

/// What changes a pull request.
#[derive(Clone, PartialEq, Debug)]
enum Op {
    Comment(u64, String),
    Approve(u64),
    Merge(u64, Method),
    Close(u64),
    Reopen(u64),
    /// Its branch checked out in the folder.
    Checkout(u64),
}

impl Op {
    fn number(&self) -> u64 {
        match self {
            Op::Comment(n, _) | Op::Approve(n) | Op::Merge(n, _) | Op::Close(n) | Op::Reopen(n) | Op::Checkout(n) => *n,
        }
    }
}

fn run_op(root: &Path, op: &Op, t: &Strings) -> Result<(), String> {
    let n = op.number().to_string();
    match forge(root) {
        None => Err(t.issues_not_github.to_owned()),
        Some(Forge::GitHub) => {
            let result = match op {
                Op::Comment(_, body) => gh(root, &["pr", "comment", &n, "--body-file", "-"], Some(body)),
                Op::Approve(_) => gh(root, &["pr", "review", &n, "--approve"], None),
                Op::Merge(_, method) => {
                    let flag = match method {
                        Method::Squash => "--squash",
                        Method::Merge => "--merge",
                        Method::Rebase => "--rebase",
                    };
                    gh(root, &["pr", "merge", &n, flag], None)
                }
                Op::Close(_) => gh(root, &["pr", "close", &n], None),
                Op::Reopen(_) => gh(root, &["pr", "reopen", &n], None),
                Op::Checkout(_) => gh(root, &["pr", "checkout", &n], None),
            };
            explain(result, t).map(|_| ())
        }
        Some(Forge::GitLab(host)) => {
            let mr = format!("projects/:id/merge_requests/{n}");
            let result = match op {
                Op::Comment(_, body) => glab(root, &host, "POST", &format!("{mr}/notes"), &[("body", "@-")], Some(body), t),
                Op::Approve(_) => glab(root, &host, "POST", &format!("{mr}/approve"), &[], None, t),
                Op::Merge(_, method) => glab(root, &host, "PUT", &format!("{mr}/merge"), &[("squash", if *method == Method::Squash { "true" } else { "false" })], None, t),
                Op::Close(_) => glab(root, &host, "PUT", &mr, &[("state_event", "close")], None, t),
                Op::Reopen(_) => glab(root, &host, "PUT", &mr, &[("state_event", "reopen")], None, t),
                Op::Checkout(_) => match super::issues::cli("glab", root, &["mr", "checkout", &n], None) {
                    None => Err(t.issues_no_glab.replace("{host}", &host)),
                    Some(result) => result,
                },
            };
            result.map(|_| ())
        }
    }
}

enum Msg {
    List(Result<Vec<Pull>, String>),
    Detail(u64, Result<Box<Detail>, String>),
    Files(u64, Result<PrFiles, String>),
    Op(Op, Result<(), String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    Open,
    Merged,
    Closed,
    All,
}

pub(super) struct Pulls {
    root: PathBuf,
    list: Option<Result<Vec<Pull>, String>>,
    list_running: bool,
    loaded_at: Option<Instant>,
    filter: Filter,
    search: String,
    selected: Option<u64>,
    detail: Option<(u64, Result<Detail, String>)>,
    files: Option<(u64, Result<PrFiles, String>)>,
    files_running: bool,
    /// The file compared (None: the conversation).
    file: Option<String>,
    /// A file just picked: the git view reads its comparison.
    file_picked: bool,
    comment: String,
    /// A merge asked for, waiting to be confirmed.
    confirm: Option<Method>,
    op_running: bool,
    /// The folder's branch changed (a branch checked out): the git view reads it again.
    moved: bool,
    notice: Option<(String, bool, Instant)>,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
}

impl Pulls {
    pub fn new(root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            root,
            list: None,
            list_running: false,
            loaded_at: None,
            filter: Filter::Open,
            search: String::new(),
            selected: None,
            detail: None,
            files: None,
            files_running: false,
            file: None,
            file_picked: false,
            comment: String::new(),
            confirm: None,
            op_running: false,
            moved: false,
            notice: None,
            tx,
            rx,
        }
    }

    fn spawn(&self, ctx: &egui::Context, t: &'static Strings, job: impl FnOnce(&Path, &'static Strings) -> Msg + Send + 'static) {
        let (root, tx, ctx) = (self.root.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(&root, t));
            ctx.request_repaint();
        });
    }

    fn load_list(&mut self, ctx: &egui::Context, t: &'static Strings) {
        if self.list_running {
            return;
        }
        self.list_running = true;
        self.loaded_at = Some(Instant::now());
        self.spawn(ctx, t, |root, t| Msg::List(read_list(root, t)));
    }

    fn select(&mut self, ctx: &egui::Context, t: &'static Strings, number: u64) {
        if self.selected != Some(number) {
            self.selected = Some(number);
            self.detail = None;
            self.files = None;
            self.file = None;
            self.comment.clear();
            self.confirm = None;
        }
        self.spawn(ctx, t, move |root, t| Msg::Detail(number, read_detail(root, number, t).map(Box::new)));
    }

    fn load_files(&mut self, ctx: &egui::Context, t: &'static Strings, detail: Detail) {
        if self.files_running {
            return;
        }
        self.files_running = true;
        let n = detail.number;
        self.spawn(ctx, t, move |root, _| Msg::Files(n, read_files(root, &detail)));
    }

    fn start(&mut self, ctx: &egui::Context, t: &'static Strings, op: Op) {
        if self.op_running {
            return;
        }
        self.op_running = true;
        self.notice = None;
        self.confirm = None;
        self.spawn(ctx, t, move |root, t| {
            let result = run_op(root, &op, t);
            Msg::Op(op, result)
        });
    }

    fn poll(&mut self, ctx: &egui::Context, t: &'static Strings) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::List(result) => {
                    self.list_running = false;
                    self.list = Some(result);
                }
                Msg::Detail(n, result) => {
                    let result = result.map(|d| *d);
                    if self.selected != Some(n) {
                        continue;
                    }
                    // Its files, read again when its branch moved.
                    if let Ok(d) = &result {
                        let stale = self.files.as_ref().is_none_or(|(m, f)| *m != n || f.as_ref().is_ok_and(|f| !d.head_oid.is_empty() && f.head != d.head_oid));
                        if stale {
                            self.load_files(ctx, t, d.clone());
                        }
                    }
                    self.detail = Some((n, result));
                }
                Msg::Files(n, result) => {
                    self.files_running = false;
                    if self.selected == Some(n) {
                        // A file gone from it: the conversation again.
                        if result.as_ref().is_ok_and(|f| self.file.as_ref().is_some_and(|p| !f.files.iter().any(|c| &c.path == p))) {
                            self.file = None;
                        }
                        self.files = Some((n, result));
                    }
                }
                Msg::Op(op, result) => {
                    self.op_running = false;
                    let n = op.number();
                    match result {
                        Ok(()) => {
                            let text = match &op {
                                Op::Comment(..) => t.pulls_commented,
                                Op::Approve(_) => t.pulls_approved_done,
                                Op::Merge(..) => t.pulls_merged_done,
                                Op::Close(_) => t.pulls_closed_done,
                                Op::Reopen(_) => t.pulls_reopened,
                                Op::Checkout(_) => t.pulls_checked_out,
                            };
                            self.notice = Some((text.replace("{n}", &n.to_string()), false, Instant::now()));
                            if matches!(op, Op::Comment(..)) {
                                self.comment.clear();
                            }
                            if matches!(op, Op::Checkout(_) | Op::Merge(..)) {
                                self.moved = true;
                            }
                            if self.selected == Some(n) {
                                self.select(ctx, t, n);
                            }
                            self.list_running = false;
                            self.load_list(ctx, t);
                        }
                        Err(e) => self.notice = Some((e, true, Instant::now())),
                    }
                }
            }
        }
    }

    /// The list, or the pull request open with its files, in `list`.
    pub fn side(&mut self, ui: &mut Ui, list: Rect, theme: &Theme, t: &'static Strings) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx, t);
        if self.loaded_at.is_none_or(|at| at.elapsed() >= REFRESH) {
            self.load_list(&ctx, t);
        }
        ctx.request_repaint_after(REFRESH);
        if self.selected.is_some() {
            self.files_ui(ui, list, theme, t);
        } else {
            self.list_ui(ui, list, theme, t);
        }
    }

    /// The conversation of the pull request open, in `view` (a file of it shows through the git view).
    pub fn main(&mut self, ui: &mut Ui, view: Rect, theme: &Theme, t: &'static Strings) {
        self.poll(&ui.ctx().clone(), t);
        self.view_ui(ui, view, theme, t);
    }

    /// The file compared, if one is: the commits it is compared between, and the file.
    pub fn shown_file(&self) -> Option<(String, String, FileChange)> {
        let path = self.file.as_ref()?;
        let (_, Ok(f)) = self.files.as_ref().filter(|(n, _)| Some(*n) == self.selected)? else { return None };
        let change = f.files.iter().find(|c| &c.path == path)?;
        Some((f.head.clone(), f.base.clone(), change.clone()))
    }

    /// A file was just picked (its comparison is to be read).
    pub fn take_file_picked(&mut self) -> bool {
        std::mem::take(&mut self.file_picked)
    }

    /// The folder's branch changed.
    pub fn take_moved(&mut self) -> bool {
        std::mem::take(&mut self.moved)
    }

    /// The pull request shown ("#12"), or its file's name.
    pub fn picked(&self) -> Option<String> {
        let n = self.selected?;
        Some(match &self.file {
            Some(p) => p.rsplit('/').next().unwrap_or(p).to_owned(),
            None => format!("#{n}"),
        })
    }

    pub fn unpick(&mut self) {
        self.selected = None;
        self.file = None;
    }

    pub fn number(&self) -> Option<u64> {
        self.selected
    }

    fn list_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &'static Strings) {
        let ctx = ui.ctx().clone();
        let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 0.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        let filters = [(Filter::Open, t.issues_open), (Filter::Merged, t.pulls_merged), (Filter::Closed, t.issues_closed), (Filter::All, t.issues_all)];
        let mut refresh = false;
        panel.horizontal_wrapped(|ui| {
            for (filter, label) in filters {
                if ui.selectable_label(self.filter == filter, egui::RichText::new(label).size(12.0)).clicked() {
                    self.filter = filter;
                }
            }
        });
        panel.add_space(2.0);
        panel.horizontal(|ui| {
            let tools_w = 30.0 + if self.list_running { 18.0 } else { 0.0 };
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(t.pulls_search).desired_width((ui.available_width() - tools_w).max(60.0)).margin(Vec2::new(6.0, 4.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                refresh |= ui.add(egui::Button::new(egui::RichText::new("↻").size(14.0)).frame_when_inactive(false)).on_hover_text(t.git_refresh).clicked();
                if self.list_running {
                    ui.add(egui::Spinner::new().size(12.0));
                }
            });
        });
        if refresh {
            self.load_list(&ctx, t);
        }
        panel.add_space(4.0);
        let pulls = match &self.list {
            None => {
                super::loading::inline(&mut panel, theme, t.pulls_loading);
                return;
            }
            Some(Err(e)) => {
                panel.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                return;
            }
            Some(Ok(pulls)) => pulls,
        };
        let needle = self.search.trim().to_lowercase();
        let number = needle.trim_start_matches(['#', '!']).parse::<u64>().ok();
        let shown: Vec<&Pull> = pulls
            .iter()
            .filter(|p| match self.filter {
                Filter::Open => p.state.open(),
                Filter::Merged => p.state == State::Merged,
                Filter::Closed => p.state == State::Closed,
                Filter::All => true,
            })
            .filter(|p| {
                needle.is_empty()
                    || Some(p.number) == number
                    || p.title.to_lowercase().contains(&needle)
                    || p.head.to_lowercase().contains(&needle)
                    || p.labels.iter().any(|l| l.name.to_lowercase().contains(&needle))
                    || login(&p.author).to_lowercase().contains(&needle)
            })
            .collect();
        if shown.is_empty() {
            panel.label(egui::RichText::new(t.pulls_empty).size(12.5).color(theme.text_muted));
            return;
        }
        let mut picked = None;
        panel.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical().id_salt(("pulls", &self.root)).auto_shrink(false).show_rows(&mut panel, ROW_H, shown.len(), |ui, range| {
            for pull in &shown[range] {
                if pull_row(ui, pull, theme, t) {
                    picked = Some(pull.number);
                }
            }
        });
        if let Some(n) = picked {
            self.select(&ctx, t, n);
        }
    }

    /// The pull request open: back to the list, its title and branches, its conversation and its files.
    fn files_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &'static Strings) {
        let Some(number) = self.selected else { return };
        let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 0.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        let listed = self.list.as_ref().and_then(|l| l.as_ref().ok()).and_then(|l| l.iter().find(|p| p.number == number)).cloned();
        let detail = self.detail.as_ref().and_then(|(_, d)| d.as_ref().ok());
        let title = detail.map(|d| d.title.clone()).or_else(|| listed.as_ref().map(|p| p.title.clone())).unwrap_or_default();
        let state = detail.map(|d| d.state).or(listed.as_ref().map(|p| p.state));
        let branches = detail.map(|d| (d.head.clone(), d.base.clone())).or_else(|| listed.as_ref().map(|p| (p.head.clone(), p.base.clone())));
        let (mut back, mut conversation, mut picked) = (false, false, None);
        egui::ScrollArea::vertical().id_salt(("pull-files", &self.root, number)).auto_shrink(false).show(&mut panel, |ui| {
            if ui.add(egui::Button::new(egui::RichText::new(format!("←  {}", t.pulls_back)).size(12.5)).frame_when_inactive(false)).clicked() {
                back = true;
            }
            ui.add_space(6.0);
            ui.add(egui::Label::new(egui::RichText::new(&title).size(13.5).strong().color(theme.text)).wrap());
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if let Some(s) = state {
                    ui.label(egui::RichText::new(s.label(t)).size(11.5).color(s.color(theme)).strong());
                }
                ui.label(egui::RichText::new(format!("#{number}")).size(11.5).color(theme.text_muted));
            });
            if let Some((head, base)) = &branches {
                ui.add(egui::Label::new(egui::RichText::new(format!("{head}  →  {base}")).monospace().size(11.5).color(theme.accent)).truncate()).on_hover_text(format!("{head} → {base}"));
            }
            ui.add_space(6.0);
            ui.separator();
            // The conversation, as a row above the files.
            let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
            let on = self.file.is_none();
            if on || resp.hovered() {
                ui.painter().rect_filled(row, 5.0, if on { theme.tab_active } else { theme.tab_hover });
            }
            ui.painter().text(Pos2::new(row.min.x + 14.0, row.center().y), Align2::CENTER_CENTER, "💬", FontId::proportional(12.0), theme.accent);
            ui.painter().text(Pos2::new(row.min.x + 28.0, row.center().y), Align2::LEFT_CENTER, t.pulls_conversation, FontId::proportional(13.0), if on { theme.text } else { theme.fg });
            conversation = resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            ui.add_space(4.0);
            match self.files.as_ref().filter(|(n, _)| *n == number) {
                None => super::loading::inline(ui, theme, t.pulls_files_loading),
                Some((_, Err(e))) => {
                    ui.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                }
                Some((_, Ok(f))) => {
                    ui.label(egui::RichText::new(t.git_commit_files.replace("{n}", &f.files.len().to_string())).size(11.5).color(theme.text_muted));
                    ui.add_space(2.0);
                    for change in &f.files {
                        if file_row(ui, change, self.file.as_ref() == Some(&change.path), theme, t) {
                            picked = Some(change.path.clone());
                        }
                    }
                }
            }
        });
        if back {
            self.unpick();
        } else if conversation {
            self.file = None;
        } else if let Some(path) = picked.filter(|p| self.file.as_ref() != Some(p)) {
            self.file = Some(path);
            self.file_picked = true;
        }
    }

    fn view_ui(&mut self, ui: &mut Ui, view: Rect, theme: &Theme, t: &'static Strings) {
        let ctx = ui.ctx().clone();
        if self.notice.as_ref().is_some_and(|(_, failed, at)| !failed && at.elapsed() > Duration::from_secs(5)) {
            self.notice = None;
        }
        let mut page = ui.new_child(egui::UiBuilder::new().max_rect(view.shrink2(Vec2::new(22.0, 16.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        if let Some((text, failed, _)) = self.notice.clone() {
            let color = if failed { theme.ansi[1] } else { theme.ansi[2] };
            Frame::NONE.fill(color.gamma_multiply(0.12)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 6)).show(&mut page, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(color)).wrap());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("×").frame_when_inactive(false)).clicked() {
                            self.notice = None;
                        }
                    });
                });
            });
            page.add_space(10.0);
            if !failed {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        }
        let Some(number) = self.selected else {
            ui.painter().text(view.center(), Align2::CENTER_CENTER, t.pulls_pick, FontId::proportional(14.0), theme.text_muted);
            return;
        };
        let d = match self.detail.as_ref().filter(|(n, _)| *n == number) {
            None => {
                super::loading::screen(ui, view, theme, &t.pulls_loading_one.replace("{n}", &number.to_string()), None, None);
                return;
            }
            Some((_, Err(e))) => {
                page.add(egui::Label::new(egui::RichText::new(e).size(13.0).color(theme.ansi[1])).wrap());
                return;
            }
            Some((_, Ok(d))) => d.clone(),
        };
        let gitlab = d.url.contains("/-/merge_requests/");
        let mut op = None;

        // Title, state, who opened it and when, its branches and size, labels.
        page.add(egui::Label::new(egui::RichText::new(&d.title).size(19.0).strong().color(theme.text)).wrap());
        page.add_space(6.0);
        page.horizontal_wrapped(|ui| {
            let color = d.state.color(theme);
            Frame::NONE.fill(color.gamma_multiply(0.18)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(9, 2)).show(ui, |ui| {
                ui.label(egui::RichText::new(d.state.label(t)).size(12.0).color(color).strong());
            });
            ui.label(egui::RichText::new(format!("#{}  ·  {}  ·  {}", d.number, login(&d.author), super::git::ago(timestamp(&d.created_at), t))).size(12.0).color(theme.text_muted));
            let decision = match d.decision.as_str() {
                "APPROVED" => Some((t.pulls_approved, theme.ansi[2])),
                "CHANGES_REQUESTED" => Some((t.pulls_changes_requested, theme.ansi[1])),
                "REVIEW_REQUIRED" if d.state.open() => Some((t.pulls_review_required, theme.ansi[3])),
                _ => None,
            };
            if let Some((text, color)) = decision {
                ui.label(egui::RichText::new(format!("·  {text}")).size(12.0).color(color));
            }
            for label in &d.labels {
                label_chip(ui, label, theme);
            }
        });
        page.add_space(4.0);
        page.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("{}  →  {}", d.head, d.base)).monospace().size(12.0).color(theme.accent));
            if d.added + d.removed > 0 {
                ui.label(egui::RichText::new(format!("+{}", d.added)).monospace().size(12.0).color(theme.ansi[2]));
                ui.label(egui::RichText::new(format!("−{}", d.removed)).monospace().size(12.0).color(theme.ansi[1]));
            }
            if d.conflicts && d.state.open() {
                ui.label(egui::RichText::new(format!("⚠  {}", t.pulls_conflicts.replace("{base}", &d.base))).size(12.0).color(theme.ansi[3]));
            }
        });
        page.add_space(8.0);

        // What can be done with it.
        page.horizontal_wrapped(|ui| {
            let web = if gitlab { t.issues_open_gitlab } else { t.issues_open_web };
            if ui.button(web).clicked() && d.url.starts_with("https://") {
                crate::terminal::open_url(&d.url);
            }
            let idle = !self.op_running;
            if ui.add_enabled(idle, egui::Button::new(t.pulls_checkout)).on_hover_text(t.pulls_checkout_hint.replace("{branch}", &d.head)).clicked() {
                op = Some(Op::Checkout(number));
            }
            if d.state.open() {
                if ui.add_enabled(idle, egui::Button::new(format!("✓  {}", t.pulls_approve))).clicked() {
                    op = Some(Op::Approve(number));
                }
                ui.add_enabled_ui(idle && d.state == State::Open, |ui| {
                    ui.menu_button(format!("⤵  {}", t.pulls_merge), |ui| {
                        ui.set_min_width(220.0);
                        let methods: &[(Method, &str)] = if gitlab {
                            &[(Method::Squash, t.pulls_merge_squash), (Method::Merge, t.pulls_merge_commit)]
                        } else {
                            &[(Method::Squash, t.pulls_merge_squash), (Method::Merge, t.pulls_merge_commit), (Method::Rebase, t.pulls_merge_rebase)]
                        };
                        for (method, label) in methods {
                            if ui.button(*label).clicked() {
                                self.confirm = Some(*method);
                                ui.close();
                            }
                        }
                    });
                });
            }
            let (text, action) = if d.state.open() { (t.pulls_close, Op::Close(number)) } else { (t.pulls_reopen, Op::Reopen(number)) };
            if d.state != State::Merged && ui.add_enabled(idle, egui::Button::new(text)).clicked() {
                op = Some(action);
            }
            if self.op_running {
                ui.add(egui::Spinner::new().size(14.0));
            }
        });
        if let Some(method) = self.confirm {
            page.add_space(8.0);
            let color = theme.ansi[5];
            Frame::NONE.fill(color.gamma_multiply(0.12)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 6)).show(&mut page, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let how = match method {
                        Method::Squash => t.pulls_merge_squash,
                        Method::Merge => t.pulls_merge_commit,
                        Method::Rebase => t.pulls_merge_rebase,
                    };
                    ui.label(egui::RichText::new(t.pulls_merge_confirm.replace("{n}", &number.to_string()).replace("{base}", &d.base).replace("{how}", how)).size(12.5).color(theme.text));
                    if ui.button(egui::RichText::new(t.pulls_merge).strong()).clicked() {
                        op = Some(Op::Merge(number, method));
                    }
                    if ui.button(t.cancel).clicked() {
                        self.confirm = None;
                    }
                });
            });
        }
        page.add_space(8.0);

        // The checks of its last commit.
        if !d.checks.is_empty() {
            let failed = d.checks.iter().filter(|c| c.state == Check::Failed).count();
            let pending = d.checks.iter().filter(|c| c.state == Check::Pending).count();
            let summary = if failed > 0 {
                (t.pulls_checks_failed.replace("{n}", &failed.to_string()), theme.ansi[1])
            } else if pending > 0 {
                (t.pulls_checks_pending.replace("{n}", &pending.to_string()), theme.ansi[3])
            } else {
                (t.pulls_checks_passed.to_owned(), theme.ansi[2])
            };
            egui::CollapsingHeader::new(egui::RichText::new(summary.0).size(12.5).color(summary.1)).id_salt(("pull-checks", number)).default_open(failed > 0).show(&mut page, |ui| {
                for c in &d.checks {
                    ui.horizontal(|ui| {
                        let (mark, color) = match c.state {
                            Check::Passed => ("✓", theme.ansi[2]),
                            Check::Failed => ("✕", theme.ansi[1]),
                            Check::Pending => ("●", theme.ansi[3]),
                            Check::Skipped => ("–", theme.text_muted),
                        };
                        ui.label(egui::RichText::new(mark).size(12.0).color(color));
                        let name = egui::RichText::new(&c.name).size(12.5).color(theme.fg);
                        if c.url.starts_with("https://") {
                            if ui.add(egui::Label::new(name).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&c.url).clicked() {
                                crate::terminal::open_url(&c.url);
                            }
                        } else {
                            ui.label(name);
                        }
                    });
                }
            });
            page.add_space(4.0);
        }
        page.separator();

        // The description, the comments and reviews, then a comment to add at the bottom.
        let composer_h = 118.0;
        let height = (page.available_height() - composer_h).max(80.0);
        egui::ScrollArea::vertical().id_salt(("pull", &self.root, number)).max_height(height).auto_shrink([false, true]).show(&mut page, |ui| {
            ui.set_width(ui.available_width());
            let body = d.body.trim();
            post(ui, login(&d.author), &d.created_at, if body.is_empty() { None } else { Some(body) }, theme, t);
            for e in &d.entries {
                let who = match e.review.as_deref() {
                    Some("APPROVED") => format!("{}  ·  ✓ {}", e.author, t.pulls_review_approved),
                    Some("CHANGES_REQUESTED") => format!("{}  ·  ✕ {}", e.author, t.pulls_review_changes),
                    Some("DISMISSED") => format!("{}  ·  {}", e.author, t.pulls_review_dismissed),
                    _ => e.author.clone(),
                };
                let body = e.body.trim();
                if body.is_empty() {
                    // An approval without a word: one line.
                    ui.label(egui::RichText::new(format!("{who}  ·  {}", super::git::ago(timestamp(&e.at), t))).size(12.0).color(theme.text_muted));
                    ui.add_space(8.0);
                } else {
                    post(ui, &who, &e.at, Some(body), theme, t);
                }
            }
        });
        page.add_space(8.0);
        page.add(egui::TextEdit::multiline(&mut self.comment).hint_text(t.issues_comment_hint).desired_rows(3).desired_width(f32::INFINITY).margin(Vec2::new(8.0, 6.0)));
        page.add_space(6.0);
        page.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let ready = !self.comment.trim().is_empty() && !self.op_running;
            if ui.add_enabled(ready, egui::Button::new(t.issues_comment)).clicked() {
                op = Some(Op::Comment(number, self.comment.trim().to_owned()));
            }
        });
        if let Some(op) = op {
            self.start(&ctx, t, op);
        }
    }
}

/// A pull request in the list: its state, number and title; below, its branch, author and age.
fn pull_row(ui: &mut Ui, pull: &Pull, theme: &Theme, t: &Strings) -> bool {
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(row.shrink2(Vec2::new(0.0, 1.0)), 5.0, theme.tab_hover);
    }
    let painter = ui.painter_at(row);
    let color = pull.state.color(theme);
    // Two commits joined: the branch, filled once merged.
    let (top, bottom) = (Pos2::new(row.min.x + 11.0, row.min.y + 10.0), Pos2::new(row.min.x + 11.0, row.min.y + 21.0));
    painter.line_segment([top, bottom], Stroke::new(1.4, color));
    painter.circle_filled(top, 2.6, color);
    if pull.state == State::Merged {
        painter.circle_filled(bottom, 2.6, color);
    } else {
        painter.circle_stroke(bottom, 2.4, Stroke::new(1.3, color));
    }
    let left = row.min.x + 24.0;
    let right = row.max.x - 6.0;
    let y = row.min.y + 15.0;
    let mut job = egui::text::LayoutJob::default();
    job.append(&format!("#{}  ", pull.number), 0.0, egui::TextFormat::simple(FontId::monospace(11.5), theme.text_muted));
    if pull.state == State::Draft {
        job.append(&format!("{}  ", t.pulls_state_draft), 0.0, egui::TextFormat::simple(FontId::proportional(11.5), theme.text_muted));
    }
    job.append(&pull.title, 0.0, egui::TextFormat::simple(FontId::proportional(13.0), theme.fg));
    job.wrap = egui::text::TextWrapping::truncate_at_width(right - left);
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(left, y - galley.size().y / 2.0), galley, theme.text);
    let mut job = egui::text::LayoutJob::default();
    job.append(&pull.head, 0.0, egui::TextFormat::simple(FontId::monospace(11.0), theme.accent.gamma_multiply(0.85)));
    job.append(&format!("  ·  {}  ·  {}", login(&pull.author), super::git::ago(timestamp(&pull.created_at), t)), 0.0, egui::TextFormat::simple(FontId::proportional(11.5), theme.text_muted));
    job.wrap = egui::text::TextWrapping::truncate_at_width(right - left);
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(left, row.min.y + 32.0 - galley.size().y / 2.0), galley, theme.text_muted);
    resp.on_hover_text(format!("{}\n{} → {}", pull.title, pull.head, pull.base)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pulls() {
        let out = br#"[{"author":{"login":"jo"},"baseRefName":"main","createdAt":"2026-10-29T08:04:32Z","headRefName":"games","isDraft":false,"labels":[],"number":13,"reviewDecision":"REVIEW_REQUIRED","state":"OPEN","title":"Games"},
            {"author":{"login":"jo"},"baseRefName":"main","createdAt":"2026-10-27T15:30:26Z","headRefName":"runner","isDraft":false,"labels":[],"number":11,"state":"MERGED","title":"Run"},
            {"author":null,"baseRefName":"main","createdAt":"2026-10-20T00:00:00Z","headRefName":"wip","isDraft":true,"labels":[],"number":9,"state":"OPEN","title":"Wip"}]"#;
        let pulls: Vec<Pull> = serde_json::from_slice::<Vec<GhPull>>(out).unwrap().into_iter().map(Pull::from).collect();
        assert_eq!(pulls.iter().map(|p| p.state).collect::<Vec<_>>(), [State::Open, State::Merged, State::Draft]);
        assert_eq!(pulls[0].head, "games");
        assert!(pulls[2].state.open());
    }

    #[test]
    fn reads_a_pull() {
        let out = br#"{"additions":10,"deletions":2,"author":{"login":"jo"},"baseRefName":"main","baseRefOid":"aaa","body":"Text","comments":[{"author":{"login":"bob"},"body":"Later","createdAt":"2026-10-28T10:00:00Z"}],
            "createdAt":"2026-10-28T08:00:00Z","headRefName":"runner","headRefOid":"bbb","isDraft":false,"labels":[],"mergeable":"CONFLICTING","number":11,"reviewDecision":"APPROVED",
            "reviews":[{"author":{"login":"jl"},"body":"","submittedAt":"2026-10-28T09:00:00Z","state":"APPROVED"},{"author":{"login":"jl"},"body":"","submittedAt":"2026-10-28T09:30:00Z","state":"COMMENTED"}],
            "state":"OPEN","statusCheckRollup":[{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://x"},{"__typename":"StatusContext","context":"ci","state":"PENDING","targetUrl":"https://y"}],
            "title":"Run","url":"https://github.com/a/b/pull/11"}"#;
        let d = Detail::from(serde_json::from_slice::<GhDetail>(out).unwrap());
        assert!(d.conflicts);
        assert_eq!((d.added, d.removed), (10, 2));
        assert_eq!(d.checks.iter().map(|c| c.state).collect::<Vec<_>>(), [Check::Failed, Check::Pending]);
        assert_eq!(d.checks[1].name, "ci");
        // The approval, then the comment; the empty review left out.
        assert_eq!(d.entries.len(), 2);
        assert_eq!(d.entries[0].review.as_deref(), Some("APPROVED"));
        assert_eq!(d.entries[1].author, "bob");
    }

    #[test]
    fn reads_gitlab_merge_requests() {
        let json = r#"[{"iid":4,"title":"Fix","state":"merged","draft":false,"author":{"username":"jl"},"labels":[],"created_at":"2026-10-01T08:00:00Z","source_branch":"fix","target_branch":"main"},
            {"iid":5,"title":"Wip","state":"opened","draft":true,"author":null,"labels":["doc"],"created_at":"2026-10-02T08:00:00Z","source_branch":"wip","target_branch":"main"}]"#;
        let pulls: Vec<Pull> = serde_json::from_str::<Vec<GlMr>>(json).unwrap().into_iter().map(Pull::from).collect();
        assert_eq!(pulls[0].state, State::Merged);
        assert_eq!(pulls[1].state, State::Draft);
        assert_eq!(pulls[1].base, "main");
    }

    /// Needs gh logged in, and the network; fetches the first pull request's commits.
    #[test]
    #[ignore]
    fn reads_this_repository() {
        let root = std::env::current_dir().unwrap();
        let t = crate::i18n::Lang::Fr.strings();
        let pulls = read_list(&root, t).expect("gh pr list");
        if let Some(first) = pulls.first() {
            let d = read_detail(&root, first.number, t).expect("gh pr view");
            let files = read_files(&root, &d).expect("files");
            assert!(!files.files.is_empty());
        }
    }
}
