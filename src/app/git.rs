//! Git in a pane, read-only: the files changed in the repository of a local pane's folder and, for the
//! one picked, its content in the last commit and now, side by side. Shown in place of the pane's
//! terminal; it asks the system's `git`.

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

/// A file changed since the last commit (paths from the repository's top).
#[derive(Clone, PartialEq, Debug)]
struct FileChange {
    path: String,
    /// Its name in the last commit, when renamed.
    old_path: Option<String>,
    status: Status,
}

/// `git status --porcelain=v1 -z --branch`: the branch, then the files.
fn parse_status(out: &[u8]) -> (String, Vec<FileChange>) {
    let text = String::from_utf8_lossy(out);
    let mut parts = text.split('\0').filter(|p| !p.is_empty());
    let mut branch = String::new();
    let mut files = Vec::new();
    while let Some(entry) = parts.next() {
        if let Some(head) = entry.strip_prefix("## ") {
            // "main...origin/main [ahead 1]", "No commits yet on main", "HEAD (no branch)".
            let head = head.strip_prefix("No commits yet on ").unwrap_or(head);
            branch = head.split("...").next().unwrap_or(head).split(" [").next().unwrap_or(head).to_owned();
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
    (branch, files)
}

fn git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root);
    // Only reads: never takes the index lock (a commit typed meanwhile in the terminal would fail).
    cmd.env("GIT_OPTIONAL_LOCKS", "0").stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn read_status(root: &Path) -> Result<(String, Vec<FileChange>), String> {
    let out = git(root).args(["status", "--porcelain=v1", "-z", "--branch", "--untracked-files=all"]).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(parse_status(&out.stdout))
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

/// The file in the last commit (empty when it wasn't there) and now (empty when deleted), compared.
fn read_diff(root: &Path, change: &FileChange) -> Result<Diff, String> {
    let before = match change.status {
        Status::Untracked | Status::Added => Vec::new(),
        _ => {
            let name = change.old_path.as_deref().unwrap_or(&change.path);
            let out = git(root).arg("show").arg(format!("HEAD:{name}")).output().map_err(|e| e.to_string())?;
            if out.status.success() { out.stdout } else { Vec::new() }
        }
    };
    let after = match change.status {
        Status::Deleted => Vec::new(),
        _ => std::fs::read(root.join(&change.path)).unwrap_or_default(),
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

enum Msg {
    Status(Result<(String, Vec<FileChange>), String>),
    Diff(String, Result<Diff, String>),
}

pub(super) struct GitView {
    root: PathBuf,
    branch: String,
    files: Vec<FileChange>,
    /// The list was read once (before that, a spinner).
    loaded: bool,
    error: Option<String>,
    selected: Option<String>,
    diff: Option<(String, Result<Diff, String>)>,
    status_running: bool,
    diff_running: bool,
    last_status: Option<Instant>,
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
}

impl GitView {
    pub fn new(root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        Self { root, branch: String::new(), files: Vec::new(), loaded: false, error: None, selected: None, diff: None, status_running: false, diff_running: false, last_status: None, scroll_to: None, hunk: 0, list_w: None, split: 0.5, hscroll: 0.0, v_offset: 0.0, v_visible: 0.0, tx, rx }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.status_running {
            return;
        }
        self.status_running = true;
        self.last_status = Some(Instant::now());
        let (root, tx, wake) = (self.root.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Status(read_status(&root)));
            wake.request_repaint();
        });
        // The file shown may have changed too.
        if let Some(change) = self.selected.as_ref().and_then(|p| self.files.iter().find(|f| &f.path == p)).cloned() {
            self.load_diff(ctx, change);
        }
    }

    fn load_diff(&mut self, ctx: &egui::Context, change: FileChange) {
        if self.diff_running {
            return;
        }
        self.diff_running = true;
        let (root, tx, ctx) = (self.root.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let diff = read_diff(&root, &change);
            let _ = tx.send(Msg::Diff(change.path, diff));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Status(Ok((branch, files))) => {
                    self.branch = branch;
                    self.files = files;
                    self.error = None;
                    self.loaded = true;
                    self.status_running = false;
                    // A file committed or reverted meanwhile: nothing to show for it any more.
                    if self.selected.as_ref().is_some_and(|p| !self.files.iter().any(|f| &f.path == p)) {
                        self.selected = None;
                        self.diff = None;
                    }
                }
                Msg::Status(Err(e)) => {
                    self.error = Some(e);
                    self.loaded = true;
                    self.status_running = false;
                }
                Msg::Diff(path, diff) => {
                    self.diff_running = false;
                    if self.selected.as_ref() == Some(&path) {
                        // Read again unchanged: the scroll position stays.
                        if self.diff.as_ref().is_none_or(|(p, d)| p != &path || d != &diff) {
                            self.diff = Some((path, diff));
                        }
                    }
                }
            }
        }
    }

    /// Draws the view in `rect`; true when "back to the terminal" was clicked.
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) -> bool {
        self.poll();
        if self.last_status.is_none_or(|at| at.elapsed() >= REFRESH) {
            self.refresh(ui.ctx());
        }
        ui.ctx().request_repaint_after(REFRESH);
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let mut back = false;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        ui.set_clip_rect(rect);
        let ui = &mut ui;

        // Top: the repository and its branch, how many files changed; refresh and back.
        let (bar, _) = ui.allocate_exact_size(Vec2::new(rect.width(), 34.0), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.painter().hline(bar.x_range(), bar.max.y, Stroke::new(1.0, theme.tab_hover));
        let repo = self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        paint_branch_icon(ui.painter(), Rect::from_center_size(Pos2::new(bar.min.x + 20.0, bar.center().y), Vec2::splat(16.0)), theme.accent);
        let mut job = egui::text::LayoutJob::default();
        job.append(&repo, 0.0, egui::TextFormat::simple(FontId::proportional(13.5), theme.text));
        if !self.branch.is_empty() {
            job.append(&format!("   {}", self.branch), 0.0, egui::TextFormat::simple(FontId::monospace(12.5), theme.accent));
        }
        if self.loaded && self.error.is_none() {
            let count = if self.files.is_empty() { t.git_no_changes.to_owned() } else { t.git_changes.replace("{n}", &self.files.len().to_string()) };
            job.append(&format!("   ·   {count}"), 0.0, egui::TextFormat::simple(FontId::proportional(12.5), theme.text_muted));
        }
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(Pos2::new(bar.min.x + 36.0, bar.center().y - galley.size().y / 2.0), galley, theme.text);
        let button = |ui: &mut Ui, right: f32, text: &str, tip: &str| {
            let at = Rect::from_min_size(Pos2::new(right - 28.0, bar.min.y + 4.0), Vec2::new(28.0, 26.0));
            ui.put(at, egui::Button::new(egui::RichText::new(text).size(15.0)).frame_when_inactive(false).corner_radius(5.0)).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        };
        if button(ui, bar.max.x - 6.0, "⤺", t.git_back) {
            back = true;
        }
        if button(ui, bar.max.x - 38.0, "↻", t.git_refresh) {
            self.last_status = None;
        }
        if self.status_running && !self.loaded {
            ui.put(Rect::from_center_size(Pos2::new(bar.max.x - 84.0, bar.center().y), Vec2::splat(14.0)), egui::Spinner::new().size(14.0));
        }

        let body = Rect::from_min_max(Pos2::new(rect.min.x, bar.max.y + 1.0), rect.max);
        if let Some(e) = &self.error {
            let text = if e.contains("No such file") || e.contains("not found") || e.contains("introuvable") { t.git_missing.to_owned() } else { e.clone() };
            ui.put(body.shrink(20.0), egui::Label::new(egui::RichText::new(text).size(13.0).color(theme.ansi[1])).wrap());
            return back;
        }
        if !self.loaded {
            return back;
        }
        if self.files.is_empty() {
            ui.painter().text(body.center(), Align2::CENTER_CENTER, t.git_all_committed, FontId::proportional(14.0), theme.text_muted);
            return back;
        }

        // Left: the files changed. Right: the one picked, compared.
        let list_w = self.list_w.unwrap_or((body.width() * 0.32).clamp(180.0, 340.0)).clamp(140.0, (body.width() - 300.0).max(140.0));
        let list = Rect::from_min_max(body.min, Pos2::new(body.min.x + list_w, body.max.y));
        let view = Rect::from_min_max(Pos2::new(list.max.x + 1.0, body.min.y), body.max);
        ui.painter().rect_filled(list, 0.0, theme.chrome_bg);
        ui.painter().vline(list.max.x, list.y_range(), Stroke::new(1.0, theme.tab_hover));
        let mut picked = None;
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list.shrink2(Vec2::new(4.0, 6.0))).layout(egui::Layout::top_down(egui::Align::Min)));
        egui::ScrollArea::vertical().id_salt(("git-files", &self.root)).auto_shrink(false).show(&mut list_ui, |ui| {
            for change in &self.files {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                let selected = self.selected.as_ref() == Some(&change.path);
                if selected || resp.hovered() {
                    ui.painter().rect_filled(row, 5.0, if selected { theme.tab_active } else { theme.tab_hover });
                }
                let (dir, name) = match change.path.rsplit_once('/') {
                    Some((d, n)) => (d, n),
                    None => ("", change.path.as_str()),
                };
                super::files::paint_file_icon(ui.painter(), Rect::from_center_size(Pos2::new(row.min.x + 14.0, row.center().y), Vec2::splat(15.0)), name, false, false, None, theme);
                let letter_x = row.max.x - 12.0;
                ui.painter().text(Pos2::new(letter_x, row.center().y), Align2::CENTER_CENTER, change.status.letter(), FontId::monospace(12.5), change.status.color(theme));
                let mut job = egui::text::LayoutJob::default();
                let strike = change.status == Status::Deleted;
                let mut name_format = egui::TextFormat::simple(FontId::proportional(13.0), if selected { theme.text } else { theme.fg });
                if strike {
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
                if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(change.clone());
                }
            }
        });
        if let Some(dx) = splitter(ui, egui::Id::new(("git-list-split", &self.root)), list.max.x, list.y_range(), theme) {
            self.list_w = Some(list_w + dx);
        }
        if let Some(change) = picked {
            if self.selected.as_ref() != Some(&change.path) {
                self.selected = Some(change.path.clone());
                self.diff = None;
                self.hunk = 0;
                self.hscroll = 0.0;
                self.diff_running = false;
                self.load_diff(ui.ctx(), change);
            }
        }
        self.diff_ui(ui, view, theme, t);
        back
    }

    /// The file picked: its two versions side by side, the changes tinted.
    fn diff_ui(&mut self, ui: &mut Ui, view: Rect, theme: &Theme, t: &Strings) {
        let Some(path) = self.selected.clone() else {
            ui.painter().text(view.center(), Align2::CENTER_CENTER, t.git_pick, FontId::proportional(14.0), theme.text_muted);
            return;
        };
        let Some((_, diff)) = self.diff.as_ref().filter(|(p, _)| *p == path) else {
            ui.put(Rect::from_center_size(view.center(), Vec2::splat(20.0)), egui::Spinner::new().size(20.0));
            return;
        };
        let (rows, hunks, added, removed, widest) = match diff {
            Ok(Diff::Rows { rows, hunks, added, removed, widest }) => (rows, hunks, *added, *removed, *widest),
            Ok(Diff::Binary) => return centered(ui, view, t.git_binary, theme.text_muted),
            Ok(Diff::TooBig) => return centered(ui, view, t.git_too_big, theme.text_muted),
            Err(e) => return centered(ui, view, e, theme.ansi[1]),
        };
        let status = self.files.iter().find(|f| f.path == path).map(|f| f.status);
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
        ui.painter().text(Pos2::new(titles.min.x + 10.0, titles.center().y), Align2::LEFT_CENTER, t.git_before, FontId::proportional(11.5), theme.text_muted);
        ui.painter().text(Pos2::new(mid + 11.0, titles.center().y), Align2::LEFT_CENTER, t.git_after, FontId::proportional(11.5), theme.text_muted);
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
        let mut scroll = egui::ScrollArea::vertical().id_salt(("git-diff", &self.root, &path)).auto_shrink(false);
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
fn splitter(ui: &mut Ui, id: egui::Id, x: f32, y: egui::Rangef, theme: &Theme) -> Option<f32> {
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
        let (branch, files) = parse_status(out);
        assert_eq!(branch, "main");
        let find = |p: &str| files.iter().find(|f| f.path == p).unwrap();
        assert_eq!(find("src/a.rs").status, Status::Modified);
        assert_eq!(find("new file.txt").status, Status::Untracked);
        assert_eq!(find("b2.rs").old_path.as_deref(), Some("b.rs"));
        assert_eq!(find("gone.rs").status, Status::Deleted);
        assert_eq!(find("both.rs").status, Status::Conflict);
        assert_eq!(files.len(), 5);
        assert_eq!(parse_status(b"## No commits yet on dev\0").0, "dev");
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
    fn finds_this_repository() {
        let here = std::env::current_dir().unwrap();
        let root = repo_root(&here.join("src/app")).expect("inside a repository");
        assert!(root.join(".git").exists());
        let (branch, files) = read_status(&root).expect("git status");
        assert!(!branch.is_empty());
        // Whatever changed here compares without error.
        if let Some(change) = files.iter().find(|f| f.status == Status::Modified) {
            assert!(matches!(read_diff(&root, change), Ok(Diff::Rows { .. } | Diff::Binary | Diff::TooBig)));
        }
    }
}
