//! The GitHub issues of a repository, in its git view: the list (open, closed or all, searchable),
//! one issue with its comments, and forms to create one, comment, close or reopen. It asks GitHub's
//! `gh`, which knows the account and the repository from the folder.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::*;

/// The list is read again this often while it is shown (others may open issues meanwhile).
const REFRESH: Duration = Duration::from_secs(60);
const ROW_H: f32 = 46.0;

#[derive(Clone, Default, Deserialize, PartialEq, Debug)]
struct Author {
    login: String,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
struct Label {
    name: String,
    /// "d73a4a".
    #[serde(default)]
    color: String,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct Issue {
    number: u64,
    title: String,
    /// "OPEN" or "CLOSED".
    state: String,
    /// None for a deleted account.
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    labels: Vec<Label>,
    created_at: String,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct Comment {
    #[serde(default)]
    author: Option<Author>,
    body: String,
    created_at: String,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct Detail {
    number: u64,
    title: String,
    state: String,
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    labels: Vec<Label>,
    created_at: String,
    #[serde(default)]
    body: String,
    url: String,
    #[serde(default)]
    comments: Vec<Comment>,
}

fn is_open(state: &str) -> bool {
    state.eq_ignore_ascii_case("open")
}

fn login(author: &Option<Author>) -> &str {
    author.as_ref().map_or("ghost", |a| a.login.as_str())
}

/// "2026-09-29T08:47:48Z": seconds since 1970.
fn timestamp(date: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(date).map(|d| d.timestamp()).unwrap_or(0)
}

/// `gh`: in the PATH, or where installers put it (an app opened from the Finder doesn't get the
/// shell's PATH).
fn gh_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin", "/snap/bin", "/usr/bin"].map(PathBuf::from));
    if let Some(home) = directories::BaseDirs::new() {
        dirs.push(home.home_dir().join(".local/bin"));
    }
    if let Some(programs) = std::env::var_os("ProgramFiles") {
        dirs.push(PathBuf::from(programs).join("GitHub CLI"));
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// Runs `gh` in the repository with `args` (and `input` on its standard input): its output, or what
/// it said on failure. None when `gh` isn't installed.
fn gh(root: &Path, args: &[&str], input: Option<&str>) -> Option<Result<Vec<u8>, String>> {
    let path = gh_path()?;
    let mut cmd = Command::new(&path);
    cmd.current_dir(root).args(args);
    // Never a question nobody could answer; plain output.
    cmd.env("GH_PROMPT_DISABLED", "1").env("GH_NO_UPDATE_NOTIFIER", "1").env("NO_COLOR", "1").env("GH_SPINNER_DISABLED", "1");
    // It runs git too: found next to it (Homebrew) or in the usual places.
    let mut paths: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    if let Some(dir) = path.parent() {
        paths.push(dir.to_path_buf());
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        cmd.env("PATH", joined);
    }
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut run = || -> Result<Vec<u8>, String> {
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
        }
    };
    Some(run())
}

/// Why `gh` failed, as shown: not installed, not logged in, not on GitHub, or what it said.
fn explain(result: Option<Result<Vec<u8>, String>>, t: &Strings) -> Result<Vec<u8>, String> {
    match result {
        None => Err(t.issues_no_gh.to_owned()),
        Some(Err(e)) if e.contains("gh auth login") => Err(t.issues_no_auth.to_owned()),
        Some(Err(e)) if e.contains("none of the git remotes") || e.contains("not a git repository") => Err(t.issues_not_github.to_owned()),
        Some(other) => other,
    }
}

fn read_list(root: &Path, t: &Strings) -> Result<Vec<Issue>, String> {
    let out = explain(gh(root, &["issue", "list", "--state", "all", "--limit", "300", "--json", "number,title,state,author,labels,createdAt"], None), t)?;
    serde_json::from_slice(&out).map_err(|e| e.to_string())
}

fn read_detail(root: &Path, number: u64, t: &Strings) -> Result<Detail, String> {
    let n = number.to_string();
    let out = explain(gh(root, &["issue", "view", &n, "--json", "number,title,state,author,labels,createdAt,body,url,comments"], None), t)?;
    serde_json::from_slice(&out).map_err(|e| e.to_string())
}

/// What changes an issue on GitHub.
#[derive(Clone, PartialEq, Debug)]
enum Op {
    Create { title: String, body: String },
    Comment(u64, String),
    Close(u64),
    Reopen(u64),
}

/// The issue concerned (the new one's number, read from the address `gh` prints).
fn run_op(root: &Path, op: &Op, t: &Strings) -> Result<u64, String> {
    match op {
        Op::Create { title, body } => {
            let out = explain(gh(root, &["issue", "create", "--title", title, "--body-file", "-"], Some(body)), t)?;
            let url = String::from_utf8_lossy(&out);
            url.trim().rsplit('/').next().and_then(|n| n.parse().ok()).ok_or_else(|| url.trim().to_owned())
        }
        Op::Comment(n, body) => explain(gh(root, &["issue", "comment", &n.to_string(), "--body-file", "-"], Some(body)), t).map(|_| *n),
        Op::Close(n) => explain(gh(root, &["issue", "close", &n.to_string()], None), t).map(|_| *n),
        Op::Reopen(n) => explain(gh(root, &["issue", "reopen", &n.to_string()], None), t).map(|_| *n),
    }
}

enum Msg {
    List(Result<Vec<Issue>, String>),
    Detail(u64, Result<Detail, String>),
    Op(Op, Result<u64, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    Open,
    Closed,
    All,
}

/// A new issue being written.
#[derive(Default)]
struct Draft {
    title: String,
    body: String,
    focus: bool,
}

pub(super) struct Issues {
    root: PathBuf,
    list: Option<Result<Vec<Issue>, String>>,
    list_running: bool,
    loaded_at: Option<Instant>,
    filter: Filter,
    search: String,
    selected: Option<u64>,
    detail: Option<(u64, Result<Detail, String>)>,
    draft: Option<Draft>,
    comment: String,
    op_running: bool,
    /// What the last change gave (true: failed).
    notice: Option<(String, bool, Instant)>,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
}

impl Issues {
    pub fn new(root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        Self { root, list: None, list_running: false, loaded_at: None, filter: Filter::Open, search: String::new(), selected: None, detail: None, draft: None, comment: String::new(), op_running: false, notice: None, tx, rx }
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

    fn load_detail(&mut self, ctx: &egui::Context, t: &'static Strings, number: u64) {
        self.spawn(ctx, t, move |root, t| Msg::Detail(number, read_detail(root, number, t)));
    }

    fn select(&mut self, ctx: &egui::Context, t: &'static Strings, number: u64) {
        self.draft = None;
        if self.selected != Some(number) {
            self.selected = Some(number);
            self.detail = None;
            self.comment.clear();
        }
        self.load_detail(ctx, t, number);
    }

    fn start(&mut self, ctx: &egui::Context, t: &'static Strings, op: Op) {
        if self.op_running {
            return;
        }
        self.op_running = true;
        self.notice = None;
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
                    if self.selected == Some(n) {
                        self.detail = Some((n, result));
                    }
                }
                Msg::Op(op, result) => {
                    self.op_running = false;
                    match result {
                        Ok(n) => {
                            let text = match &op {
                                Op::Create { .. } => t.issues_created,
                                Op::Comment(..) => t.issues_commented,
                                Op::Close(_) => t.issues_closed_done,
                                Op::Reopen(_) => t.issues_reopened,
                            };
                            self.notice = Some((text.replace("{n}", &n.to_string()), false, Instant::now()));
                            if matches!(op, Op::Comment(..)) {
                                self.comment.clear();
                            }
                            // The new one shown, in the list it belongs to.
                            if matches!(op, Op::Create { .. } | Op::Reopen(_)) && self.filter == Filter::Closed {
                                self.filter = Filter::Open;
                            }
                            self.select(ctx, t, n);
                            self.list_running = false;
                            self.load_list(ctx, t);
                        }
                        // What was typed stays, to try again.
                        Err(e) => self.notice = Some((e, true, Instant::now())),
                    }
                }
            }
        }
    }

    /// The list in `list` (left), the issue picked or the new one in `view` (right).
    pub fn ui(&mut self, ui: &mut Ui, list: Rect, view: Rect, theme: &Theme, t: &'static Strings) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx, t);
        if self.loaded_at.is_none_or(|at| at.elapsed() >= REFRESH) {
            self.load_list(&ctx, t);
        }
        ctx.request_repaint_after(REFRESH);
        self.list_ui(ui, list, theme, t);
        self.view_ui(ui, view, theme, t);
    }

    fn list_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &'static Strings) {
        let ctx = ui.ctx().clone();
        let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 0.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        // Open / closed / all, then refresh and new.
        panel.horizontal(|ui| {
            for (filter, label) in [(Filter::Open, t.issues_open), (Filter::Closed, t.issues_closed), (Filter::All, t.issues_all)] {
                if ui.selectable_label(self.filter == filter, egui::RichText::new(label).size(12.0)).clicked() {
                    self.filter = filter;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("+").size(15.0)).frame_when_inactive(false)).on_hover_text(t.issues_new).clicked() {
                    self.selected = None;
                    self.draft = Some(Draft { focus: true, ..Draft::default() });
                }
                if ui.add(egui::Button::new(egui::RichText::new("↻").size(14.0)).frame_when_inactive(false)).on_hover_text(t.git_refresh).clicked() {
                    self.load_list(&ctx, t);
                }
                if self.list_running {
                    ui.add(egui::Spinner::new().size(12.0));
                }
            });
        });
        panel.add_space(2.0);
        panel.add(egui::TextEdit::singleline(&mut self.search).hint_text(t.issues_search).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 4.0)));
        panel.add_space(4.0);
        let issues = match &self.list {
            None => {
                super::loading::inline(&mut panel, theme, t.issues_loading);
                return;
            }
            Some(Err(e)) => {
                panel.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                return;
            }
            Some(Ok(issues)) => issues,
        };
        let needle = self.search.trim().to_lowercase();
        let number = needle.trim_start_matches('#').parse::<u64>().ok();
        let shown: Vec<&Issue> = issues
            .iter()
            .filter(|i| match self.filter {
                Filter::Open => is_open(&i.state),
                Filter::Closed => !is_open(&i.state),
                Filter::All => true,
            })
            .filter(|i| needle.is_empty() || Some(i.number) == number || i.title.to_lowercase().contains(&needle) || i.labels.iter().any(|l| l.name.to_lowercase().contains(&needle)) || login(&i.author).to_lowercase().contains(&needle))
            .collect();
        if shown.is_empty() {
            panel.label(egui::RichText::new(t.issues_empty).size(12.5).color(theme.text_muted));
            return;
        }
        let mut picked = None;
        panel.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical().id_salt(("issues", &self.root)).auto_shrink(false).show_rows(&mut panel, ROW_H, shown.len(), |ui, range| {
            for issue in &shown[range] {
                if issue_row(ui, issue, self.selected == Some(issue.number), theme, t) {
                    picked = Some(issue.number);
                }
            }
        });
        if let Some(n) = picked {
            self.select(&ctx, t, n);
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
        if self.draft.is_some() {
            return self.draft_ui(&mut page, theme, t);
        }
        let Some(number) = self.selected else {
            ui.painter().text(view.center(), Align2::CENTER_CENTER, t.issues_pick, FontId::proportional(14.0), theme.text_muted);
            return;
        };
        let detail = match self.detail.as_ref().filter(|(n, _)| *n == number) {
            None => {
                super::loading::screen(ui, view, theme, &t.issues_loading_one.replace("{n}", &number.to_string()), None, None);
                return;
            }
            Some((_, Err(e))) => {
                page.add(egui::Label::new(egui::RichText::new(e).size(13.0).color(theme.ansi[1])).wrap());
                return;
            }
            Some((_, Ok(d))) => d.clone(),
        };

        // Title, state, who opened it and when, labels; open on GitHub, close or reopen.
        page.add(egui::Label::new(egui::RichText::new(&detail.title).size(19.0).strong().color(theme.text)).wrap());
        page.add_space(6.0);
        let open = is_open(&detail.state);
        let mut op = None;
        page.horizontal_wrapped(|ui| {
            let (label, color) = if open { (t.issues_state_open, theme.ansi[2]) } else { (t.issues_state_closed, theme.ansi[5]) };
            Frame::NONE.fill(color.gamma_multiply(0.18)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(9, 2)).show(ui, |ui| {
                ui.label(egui::RichText::new(label).size(12.0).color(color).strong());
            });
            ui.label(egui::RichText::new(format!("#{}  ·  {}  ·  {}", detail.number, login(&detail.author), super::git::ago(timestamp(&detail.created_at), t))).size(12.0).color(theme.text_muted));
            for label in &detail.labels {
                label_chip(ui, label, theme);
            }
        });
        page.add_space(8.0);
        page.horizontal(|ui| {
            if ui.button(t.issues_open_web).clicked() && detail.url.starts_with("https://") {
                crate::terminal::open_url(&detail.url);
            }
            let (text, action) = if open { (t.issues_close, Op::Close(number)) } else { (t.issues_reopen, Op::Reopen(number)) };
            if ui.add_enabled(!self.op_running, egui::Button::new(text)).clicked() {
                op = Some(action);
            }
            if self.op_running {
                ui.add(egui::Spinner::new().size(14.0));
            }
        });
        page.add_space(8.0);
        page.separator();

        // The description and the comments, then a comment to add at the bottom.
        let composer_h = 118.0;
        let height = (page.available_height() - composer_h).max(80.0);
        egui::ScrollArea::vertical().id_salt(("issue", &self.root, number)).max_height(height).auto_shrink([false, true]).show(&mut page, |ui| {
            ui.set_width(ui.available_width());
            let body = detail.body.trim();
            post(ui, login(&detail.author), &detail.created_at, if body.is_empty() { None } else { Some(body) }, theme, t);
            if !detail.comments.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.issues_comments.replace("{n}", &detail.comments.len().to_string())).size(12.0).color(theme.text_muted));
                ui.add_space(4.0);
            }
            for c in &detail.comments {
                post(ui, login(&c.author), &c.created_at, Some(c.body.trim()), theme, t);
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

    /// The new issue: its title and description.
    fn draft_ui(&mut self, page: &mut Ui, theme: &Theme, t: &'static Strings) {
        let ctx = page.ctx().clone();
        let Some(draft) = &mut self.draft else { return };
        page.label(egui::RichText::new(t.issues_new).size(19.0).strong().color(theme.text));
        page.add_space(12.0);
        let title = page.add(egui::TextEdit::singleline(&mut draft.title).hint_text(t.issues_title_hint).font(FontId::proportional(14.0)).desired_width(f32::INFINITY).margin(Vec2::new(8.0, 6.0)));
        if std::mem::take(&mut draft.focus) {
            title.request_focus();
        }
        page.add_space(8.0);
        let rows = ((page.available_height() - 60.0) / 18.0).max(4.0) as usize;
        page.add(egui::TextEdit::multiline(&mut draft.body).hint_text(t.issues_body_hint).desired_rows(rows).desired_width(f32::INFINITY).margin(Vec2::new(8.0, 6.0)));
        page.add_space(10.0);
        let mut create = None;
        let mut cancel = false;
        page.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let ready = !draft.title.trim().is_empty() && !self.op_running;
            if ui.add_enabled(ready, egui::Button::new(t.issues_create)).clicked() {
                create = Some(Op::Create { title: draft.title.trim().to_owned(), body: draft.body.trim().to_owned() });
            }
            if ui.button(t.cancel).clicked() {
                cancel = true;
            }
            if self.op_running {
                ui.add(egui::Spinner::new().size(14.0));
            }
        });
        if cancel {
            self.draft = None;
        }
        if let Some(op) = create {
            self.start(&ctx, t, op);
        }
    }
}

/// An issue in the list: open or closed, its number and title; below, its labels, author and age.
fn issue_row(ui: &mut Ui, issue: &Issue, selected: bool, theme: &Theme, t: &Strings) -> bool {
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(row.shrink2(Vec2::new(0.0, 1.0)), 5.0, if selected { theme.tab_active } else { theme.tab_hover });
    }
    let painter = ui.painter_at(row);
    let color = if is_open(&issue.state) { theme.ansi[2] } else { theme.ansi[5] };
    let dot = Pos2::new(row.min.x + 11.0, row.min.y + 15.0);
    painter.circle_stroke(dot, 4.5, Stroke::new(1.5, color));
    painter.circle_filled(dot, 1.6, color);
    let left = row.min.x + 24.0;
    let right = row.max.x - 6.0;
    let mut job = egui::text::LayoutJob::default();
    job.append(&format!("#{}  ", issue.number), 0.0, egui::TextFormat::simple(FontId::monospace(11.5), theme.text_muted));
    job.append(&issue.title, 0.0, egui::TextFormat::simple(FontId::proportional(13.0), if selected { theme.text } else { theme.fg }));
    job.wrap = egui::text::TextWrapping::truncate_at_width(right - left);
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(left, dot.y - galley.size().y / 2.0), galley, theme.text);
    let mut x = left;
    let y = row.min.y + 32.0;
    for label in &issue.labels {
        let c = label_color(label, theme);
        let galley = painter.layout_no_wrap(label.name.clone(), FontId::proportional(10.5), c);
        let w = galley.size().x + 10.0;
        if x + w > right - 60.0 {
            break;
        }
        let chip = Rect::from_min_size(Pos2::new(x, y - 7.5), Vec2::new(w, 15.0));
        painter.rect_filled(chip, 7.5, c.gamma_multiply(0.18));
        painter.galley(chip.center() - galley.size() / 2.0, galley, c);
        x += w + 4.0;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(format!("{}  ·  {}", login(&issue.author), super::git::ago(timestamp(&issue.created_at), t)), FontId::proportional(11.5), theme.text_muted);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - x).max(10.0));
    let galley = painter.layout_job(job);
    painter.galley(Pos2::new(x, y - galley.size().y / 2.0), galley, theme.text_muted);
    resp.on_hover_text(&issue.title).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A label's color on GitHub, kept readable on this theme.
fn label_color(label: &Label, theme: &Theme) -> Color32 {
    let rgb = u32::from_str_radix(label.color.trim_start_matches('#'), 16).ok().filter(|_| label.color.len() >= 6);
    let Some(rgb) = rgb else { return theme.accent };
    let c = Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    // Too dark on a dark theme (or too light on a light one): toward the text color.
    let luma = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if (theme.dark && luma < 90.0) || (!theme.dark && luma > 190.0) {
        c.lerp_to_gamma(theme.text, 0.5)
    } else {
        c
    }
}

fn label_chip(ui: &mut Ui, label: &Label, theme: &Theme) {
    let c = label_color(label, theme);
    Frame::NONE.fill(c.gamma_multiply(0.18)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(egui::RichText::new(&label.name).size(11.5).color(c));
    });
}

/// The description or a comment: who and when, then the text (Markdown, shown as written).
fn post(ui: &mut Ui, author: &str, created_at: &str, body: Option<&str>, theme: &Theme, t: &Strings) {
    Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(format!("{author}  ·  {}", super::git::ago(timestamp(created_at), t))).size(11.5).color(theme.text_muted));
        ui.add_space(4.0);
        match body {
            Some(text) => ui.add(egui::Label::new(egui::RichText::new(text).size(13.0).color(theme.fg)).wrap().selectable(true)),
            None => ui.label(egui::RichText::new(t.issues_no_body).size(12.5).italics().color(theme.text_muted)),
        };
    });
    ui.add_space(8.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_issues() {
        let out = br#"[{"author":{"id":"U_1","is_bot":false,"login":"ann","name":""},"createdAt":"2026-09-29T08:47:48Z","labels":[{"id":"L","name":"bug","description":"","color":"d73a4a"}],"number":6,"state":"OPEN","title":"Bug"},{"author":null,"createdAt":"2026-09-01T00:00:00Z","labels":[],"number":2,"state":"CLOSED","title":"Old"}]"#;
        let issues: Vec<Issue> = serde_json::from_slice(out).unwrap();
        assert_eq!(issues.len(), 2);
        assert!(is_open(&issues[0].state) && !is_open(&issues[1].state));
        assert_eq!(issues[0].labels[0].name, "bug");
        assert_eq!(login(&issues[1].author), "ghost");
        assert_eq!(timestamp(&issues[0].created_at), 1_790_671_668);
    }

    #[test]
    fn reads_an_issue() {
        let out = br#"{"author":{"login":"ann"},"body":"Text","comments":[{"author":{"login":"bob"},"authorAssociation":"OWNER","body":"Seen","createdAt":"2026-09-29T09:00:00Z","url":"u"}],"createdAt":"2026-09-29T08:47:48Z","labels":[],"number":6,"state":"OPEN","title":"Bug","url":"https://github.com/a/b/issues/6"}"#;
        let detail: Detail = serde_json::from_slice(out).unwrap();
        assert_eq!(detail.comments.len(), 1);
        assert_eq!(login(&detail.comments[0].author), "bob");
    }

    /// Needs gh logged in, and the network.
    #[test]
    #[ignore]
    fn reads_this_repository() {
        let root = std::env::current_dir().unwrap();
        let t = crate::i18n::Lang::Fr.strings();
        let issues = read_list(&root, t).expect("gh issue list");
        if let Some(first) = issues.first() {
            let detail = read_detail(&root, first.number, t).expect("gh issue view");
            assert_eq!(detail.title, first.title);
        }
    }
}
