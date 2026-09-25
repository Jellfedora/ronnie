mod boxdraw;
mod input;
mod links;
mod pty;
mod render;

use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event as TermEvent, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use alacritty_terminal::Term;
use anyhow::Result;
use egui::{Event, EventFilter, Id, MouseWheelUnit, PointerButton, Pos2, Rect, Response, Sense, Ui, Vec2};

pub use links::{open as open_url, LocalUrl};
pub use render::FontSet;

use crate::theme::Theme;

/// Inner padding between the pane border and the character grid.
const PADDING: f32 = 8.0;

/// How long a looked-up working directory is reused before asking the OS again.
const CWD_TTL: Duration = Duration::from_millis(500);
/// How often the foreground program and the local server URLs are looked up again.
const ACTIVITY_TTL: Duration = Duration::from_secs(1);
/// Most occurrences kept by the text search.
const FIND_LIMIT: usize = 5000;
/// While output streams in, the text search is redone at most this often.
const FIND_REFRESH: Duration = Duration::from_millis(250);
/// Rows of output (scrollback included) searched for local server URLs. Found URLs are kept while the
/// program runs, so only the recent output needs searching.
const URL_SCAN_ROWS: usize = 500;

/// Where the bytes of a terminal come from and go to (local shell, SSH channel...).
pub trait Backend: Send {
    fn write(&mut self, data: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16, cell_w: u16, cell_h: u16);
    /// Current working directory of the session, when it can be known.
    fn cwd(&self) -> Option<PathBuf> {
        None
    }
    /// Process id of the program started (the shell, or ssh).
    fn pid(&self) -> Option<u32> {
        None
    }
    /// Name of the program in the foreground when it isn't the one started (the shell): something
    /// that closing the terminal would interrupt.
    fn foreground(&self) -> Option<String> {
        None
    }
}

#[derive(Clone)]
struct Listener {
    tx: Sender<TermEvent>,
    ctx: egui::Context,
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

#[derive(Clone, Copy)]
struct GridSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Occurrences of a searched text, and the one shown.
struct Find {
    query: String,
    matches: Vec<Vec<Point>>,
    /// Index of the current occurrence in `matches` (top to bottom).
    current: usize,
    /// `output_seq` when searched: new output moves lines, the search is then redone.
    seq: u64,
    /// When it was last searched: streaming output redoes it at most every FIND_REFRESH.
    at: Instant,
}

/// What a terminal is busy with, refreshed at most every `ACTIVITY_TTL`.
#[derive(Default)]
struct Activity {
    checked: Option<Instant>,
    /// `output_seq` at the last check.
    checked_seq: u64,
    /// `output_seq` when the URLs were last searched.
    scanned: u64,
    program: Option<String>,
    urls: Vec<links::LocalUrl>,
}

/// One terminal session: the emulator state plus the process/connection feeding it.
pub struct Terminal {
    id: Id,
    term: Arc<FairMutex<Term<Listener>>>,
    backend: Box<dyn Backend>,
    events: Receiver<TermEvent>,
    exited: Arc<AtomicBool>,
    /// The program has written something (an ssh session still connecting hasn't).
    received: Arc<AtomicBool>,
    /// Bumped by the reader thread on each chunk of output, to rescan only when something changed.
    output_seq: Arc<AtomicU64>,
    /// Shown on screen (in the active tab): its output redraws the window right away.
    visible: Arc<AtomicBool>,
    activity: Activity,
    title: Option<String>,
    size: GridSize,
    cell: Vec2,
    scroll_acc: f32,
    mouse_down: bool,
    /// Link under the pointer, underlined and opened on click.
    hover_link: Option<links::Link>,
    /// A clicked link that needs the user's confirmation before opening (see `links::is_safe`).
    link_request: Option<String>,
    /// Text search in the output (Cmd+F), highlighted on screen.
    find: Option<Find>,
    /// Where the grid was drawn last frame, to place things next to the cursor.
    grid_origin: Pos2,
    /// Last looked-up working directory and when, so painting every frame stays cheap.
    cwd_cache: Option<(Instant, Option<PathBuf>, u64)>,
}

impl Terminal {
    /// Starts `launch` (the user's shell if none) in a local pseudo-terminal.
    /// A shell keeps its command history in `history` (ignored for other programs).
    pub fn local(ctx: &egui::Context, cwd: Option<&Path>, launch: Option<&crate::ssh::Launch>, history: Option<&Path>) -> Result<Self> {
        let size = GridSize { cols: 80, rows: 24 };
        let (backend, reader) = pty::LocalPty::spawn(size.cols as u16, size.rows as u16, cwd, launch, history)?;
        Ok(Self::start(ctx, Box::new(backend), reader, size))
    }

    fn start(ctx: &egui::Context, backend: Box<dyn Backend>, mut reader: Box<dyn Read + Send>, size: GridSize) -> Self {
        let (tx, events) = mpsc::channel();
        let listener = Listener { tx, ctx: ctx.clone() };
        let term = Arc::new(FairMutex::new(Term::new(TermConfig::default(), &size, listener)));
        let exited = Arc::new(AtomicBool::new(false));
        let received = Arc::new(AtomicBool::new(false));
        let output_seq = Arc::new(AtomicU64::new(0));
        let visible = Arc::new(AtomicBool::new(true));

        {
            let term = term.clone();
            let exited = exited.clone();
            let received = received.clone();
            let output_seq = output_seq.clone();
            let visible = visible.clone();
            let ctx = ctx.clone();
            thread::Builder::new()
                .name("pty-reader".into())
                .spawn(move || {
                    let mut parser: Processor = Processor::new();
                    let mut buf = vec![0u8; 1 << 16];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                received.store(true, Ordering::Relaxed);
                                output_seq.fetch_add(1, Ordering::Relaxed);
                                parser.advance(&mut *term.lock(), &buf[..n]);
                                // A hidden pane (other tab) needn't redraw the window at the pace of its
                                // output: its badge and title catch up twice a second.
                                if visible.load(Ordering::Relaxed) {
                                    ctx.request_repaint();
                                } else {
                                    ctx.request_repaint_after(Duration::from_millis(500));
                                }
                            }
                            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                            Err(_) => break,
                        }
                    }
                    exited.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                })
                .expect("spawn reader thread");
        }

        Self {
            id: Id::new(("terminal", Arc::as_ptr(&term) as usize)),
            term,
            backend,
            events,
            exited,
            received,
            output_seq,
            visible,
            activity: Activity::default(),
            title: None,
            size,
            cell: Vec2::new(8.0, 16.0),
            scroll_acc: 0.0,
            mouse_down: false,
            hover_link: None,
            find: None,
            link_request: None,
            grid_origin: Pos2::ZERO,
            cwd_cache: None,
        }
    }

    /// Title set by the running program (OSC 0/2), if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn cwd(&self) -> Option<PathBuf> {
        self.backend.cwd()
    }

    /// Refreshes the foreground program and the local servers it announced (dropped once it stops).
    fn refresh_activity(&mut self, ctx: &egui::Context) {
        // Only after new output (a program starting echoes its command line, one ending prints the
        // prompt), at most every ACTIVITY_TTL: idle terminals cost nothing and wake nothing up.
        let (now, seq) = (Instant::now(), self.output_seq.load(Ordering::Relaxed));
        if self.activity.checked.is_some() && self.activity.checked_seq == seq {
            return;
        }
        if let Some(at) = self.activity.checked.filter(|at| now.duration_since(*at) < ACTIVITY_TTL) {
            ctx.request_repaint_after(ACTIVITY_TTL - now.duration_since(at));
            return;
        }
        self.activity.checked = Some(now);
        self.activity.checked_seq = seq;
        self.activity.program = self.foreground();
        if self.activity.program.is_none() {
            self.activity.urls.clear();
            return;
        }
        let seq = self.output_seq.load(Ordering::Relaxed);
        if seq != self.activity.scanned {
            self.activity.scanned = seq;
            for url in links::local_urls(&self.term.lock(), URL_SCAN_ROWS) {
                if !self.activity.urls.iter().any(|u| u.port == url.port) {
                    self.activity.urls.push(url);
                }
            }
        }
    }

    /// Foreground program (cached), for the live badge.
    pub fn live_program(&mut self, ctx: &egui::Context) -> Option<&str> {
        self.refresh_activity(ctx);
        self.activity.program.as_deref()
    }

    /// Local servers announced by the running program, oldest first (cached).
    pub fn local_urls(&mut self, ctx: &egui::Context) -> &[links::LocalUrl] {
        self.refresh_activity(ctx);
        &self.activity.urls
    }

    /// Process id of the program started in the pane (the shell, or ssh).
    pub fn pid(&self) -> Option<u32> {
        self.backend.pid()
    }

    /// Program running in the foreground instead of the shell (`npm`, `vim`...), if any.
    pub fn foreground(&self) -> Option<String> {
        if self.has_exited() { None } else { self.backend.foreground() }
    }

    /// Working directory for display. Looked up again only after new output (a `cd` prints a new
    /// prompt), at most every `CWD_TTL`: an idle terminal costs nothing and wakes nothing up.
    pub fn cached_cwd(&mut self, ctx: &egui::Context) -> Option<&Path> {
        let (now, seq) = (Instant::now(), self.output_seq.load(Ordering::Relaxed));
        match &self.cwd_cache {
            Some((_, _, seen)) if *seen == seq => {}
            // Changed very recently: look once the throttle allows.
            Some((at, _, _)) if now.duration_since(*at) < CWD_TTL => ctx.request_repaint_after(CWD_TTL - now.duration_since(*at)),
            _ => self.cwd_cache = Some((now, self.backend.cwd(), seq)),
        }
        self.cwd_cache.as_ref().and_then(|(_, cwd, _)| cwd.as_deref())
    }

    /// Whether the program has written anything yet.
    pub fn has_output(&self) -> bool {
        self.received.load(Ordering::Relaxed)
    }

    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::Relaxed)
    }

    /// Types text as if the user did (snaps the view to the bottom).
    pub fn type_text(&mut self, text: &str) {
        self.write_user(text.as_bytes());
    }

    /// Whether the program is waiting for a password: the text before the cursor looks like
    /// "[sudo] password for bob:", "Password:", "Mot de passe :"...
    pub fn awaits_password(&self) -> bool {
        let term = self.term.lock();
        if term.grid().display_offset() != 0 {
            return false;
        }
        let cursor = term.grid().cursor.point;
        let row = &term.grid()[cursor.line];
        let line: String = (0..cursor.column.0.min(self.size.cols)).map(|c| row[Column(c)].c).collect();
        looks_like_password_prompt(&line)
    }

    /// Screen position just right of the cursor, as drawn last frame.
    pub fn cursor_pos(&self) -> Pos2 {
        let cursor = self.term.lock().grid().cursor.point;
        self.grid_origin + Vec2::new(cursor.column.0 as f32 * self.cell.x, cursor.line.0 as f32 * self.cell.y)
    }

    /// Text currently selected with the mouse, if any.
    /// Whether text is selected (cheap, unlike building the selected text).
    pub fn has_selection(&self) -> bool {
        self.term.lock().selection.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::Relaxed);
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_to_string().filter(|s| !s.is_empty())
    }

    /// Pastes as if typed, honoring bracketed paste mode.
    pub fn paste_text(&mut self, text: &str) {
        let mode = *self.term.lock().mode();
        self.paste(text, mode);
    }

    /// A clicked link waiting for confirmation, taken once.
    pub fn take_link_request(&mut self) -> Option<String> {
        self.link_request.take()
    }

    /// Searches `query` in the output and shows the most recent occurrence. Returns (current, total).
    pub fn find(&mut self, query: &str) -> (usize, usize) {
        let matches = links::find_text(&self.term.lock(), query, FIND_LIMIT);
        let current = matches.len().saturating_sub(1);
        self.find = Some(Find { query: query.to_owned(), matches, current, seq: self.output_seq.load(Ordering::Relaxed), at: Instant::now() });
        self.reveal_find();
        self.find_status()
    }

    /// Moves to the previous (`older`, upward) or next occurrence, wrapping around.
    pub fn find_step(&mut self, older: bool) {
        let Some(find) = &mut self.find else { return };
        let n = find.matches.len();
        if n == 0 {
            return;
        }
        find.current = if older { (find.current + n - 1) % n } else { (find.current + 1) % n };
        self.reveal_find();
    }

    /// (1-based current occurrence, total); (0, 0) without any.
    pub fn find_status(&self) -> (usize, usize) {
        match &self.find {
            Some(f) if !f.matches.is_empty() => (f.current + 1, f.matches.len()),
            _ => (0, 0),
        }
    }

    pub fn clear_find(&mut self) {
        self.find = None;
    }

    /// Scrolls so that the current occurrence is on screen.
    fn reveal_find(&mut self) {
        let Some(line) = self.find.as_ref().and_then(|f| f.matches.get(f.current)).and_then(|m| m.first()).map(|p| p.line.0) else { return };
        let mut term = self.term.lock();
        let offset = term.grid().display_offset() as i32;
        let rows = self.size.rows as i32;
        if line >= -offset && line < rows - offset {
            return;
        }
        let history = term.grid().history_size() as i32;
        let target = (rows / 2 - line).clamp(0, history);
        term.scroll_display(Scroll::Delta(target - offset));
    }

    /// Redoes the search after new output (lines moved), staying on the same occurrence counted from
    /// the most recent one.
    fn refresh_find(&mut self, ctx: &egui::Context) {
        let seq = self.output_seq.load(Ordering::Relaxed);
        let Some(find) = &self.find else { return };
        if find.seq == seq {
            return;
        }
        // A log streaming in would otherwise rescan the whole scrollback on every frame.
        let age = find.at.elapsed();
        if age < FIND_REFRESH {
            ctx.request_repaint_after(FIND_REFRESH - age);
            return;
        }
        let from_end = find.matches.len().saturating_sub(find.current + 1);
        let query = find.query.clone();
        let matches = links::find_text(&self.term.lock(), &query, FIND_LIMIT);
        let current = matches.len().saturating_sub(1 + from_end);
        self.find = Some(Find { query, matches, current, seq, at: Instant::now() });
    }

    /// Clears the screen and the scrollback (Cmd+K). An idle shell is asked to redraw its prompt;
    /// a running program is left alone.
    pub fn clear(&mut self) {
        use alacritty_terminal::vte::ansi::{ClearMode, Handler};
        let idle = self.foreground().is_none();
        {
            let mut term = self.term.lock();
            term.grid_mut().clear_history();
            if !idle {
                term.clear_screen(ClearMode::All);
            }
        }
        if idle {
            // Ctrl+L: the shell clears the screen and prints its prompt again.
            self.write(b"\x0c");
        }
    }

    pub fn request_focus(&self, ui: &Ui) {
        ui.memory_mut(|m| m.request_focus(self.id));
    }

    fn write(&mut self, data: &[u8]) {
        self.backend.write(data);
    }

    /// Writes user input: also snaps the view back to the bottom and drops the selection.
    fn write_user(&mut self, data: &[u8]) {
        {
            let mut term = self.term.lock();
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        }
        self.write(data);
    }

    pub fn process_events(&mut self, ctx: &egui::Context, theme: &Theme) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                TermEvent::Title(t) => self.title = Some(t),
                TermEvent::ResetTitle => self.title = None,
                TermEvent::PtyWrite(s) => self.write(s.as_bytes()),
                TermEvent::ClipboardStore(_, text) => ctx.copy_text(text),
                TermEvent::ColorRequest(index, fmt) => {
                    let color = match index {
                        256 => Color::Named(NamedColor::Foreground),
                        257 => Color::Named(NamedColor::Background),
                        258 => Color::Named(NamedColor::Cursor),
                        i => Color::Indexed(i.min(255) as u8),
                    };
                    let c = theme.resolve(color, self.term.lock().colors());
                    let reply = fmt(alacritty_terminal::vte::ansi::Rgb { r: c.r(), g: c.g(), b: c.b() });
                    self.write(reply.as_bytes());
                }
                TermEvent::TextAreaSizeRequest(fmt) => {
                    let reply = fmt(self.window_size());
                    self.write(reply.as_bytes());
                }
                TermEvent::Exit | TermEvent::ChildExit(_) => self.exited.store(true, Ordering::Relaxed),
                _ => {}
            }
        }
    }

    fn window_size(&self) -> WindowSize {
        WindowSize {
            num_lines: self.size.rows as u16,
            num_cols: self.size.cols as u16,
            cell_width: self.cell.x as u16,
            cell_height: self.cell.y as u16,
        }
    }

    fn resize_to(&mut self, area: Rect, cell: Vec2) {
        let cols = ((area.width() / cell.x).floor() as usize).max(2);
        let rows = ((area.height() / cell.y).floor() as usize).max(1);
        if cols == self.size.cols && rows == self.size.rows && cell == self.cell {
            return;
        }
        self.size = GridSize { cols, rows };
        self.cell = cell;
        self.term.lock().resize(self.size);
        self.backend.resize(cols as u16, rows as u16, cell.x as u16, cell.y as u16);
    }

    /// Grid point under a screen position, plus which half of the cell it falls in.
    fn point_at(&self, grid_origin: Pos2, pos: Pos2, display_offset: usize) -> (Point, Side) {
        let rel = pos - grid_origin;
        let col_f = (rel.x / self.cell.x).max(0.0);
        let col = (col_f as usize).min(self.size.cols - 1);
        let row = ((rel.y / self.cell.y).max(0.0) as usize).min(self.size.rows - 1);
        let side = if col_f.fract() > 0.5 { Side::Right } else { Side::Left };
        (Point::new(Line(row as i32 - display_offset as i32), Column(col)), side)
    }

    /// Draws the terminal in `rect` and handles its keyboard and mouse input.
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, fonts: &FontSet) -> Response {
        self.process_events(ui.ctx(), theme);
        self.refresh_find(ui.ctx());
        let cell = fonts.cell_size(ui);
        let grid_rect = rect.shrink(PADDING);
        self.resize_to(grid_rect, cell);

        let response = ui.interact(rect, self.id, Sense::click_and_drag());
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
            });
            self.handle_keyboard(ui);
        }
        self.hover_link = self.link_under_pointer(ui, &response, grid_rect.min);
        self.handle_mouse(ui, &response, grid_rect.min);
        // An OSC 8 link's visible text may hide its target: show the real one.
        let response = match self.hover_link.as_ref().filter(|l| l.osc8) {
            Some(link) => response.on_hover_text(&link.url),
            None => response,
        };

        if response.hovered() {
            let icon = if self.hover_link.is_some() { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Text };
            ui.ctx().set_cursor_icon(icon);
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.bg);
        let term = self.term.lock();
        render::paint(&painter, grid_rect.min, cell, &term, theme, fonts, focused);
        self.grid_origin = grid_rect.min;
        if let Some(link) = &self.hover_link {
            let offset = term.grid().display_offset() as i32;
            for p in &link.cells {
                let row = p.line.0 + offset;
                if row < 0 || row >= self.size.rows as i32 {
                    continue;
                }
                let x = grid_rect.min.x + p.column.0 as f32 * cell.x;
                let y = grid_rect.min.y + (row + 1) as f32 * cell.y - 2.0;
                painter.hline(x..=x + cell.x, y, egui::Stroke::new(1.0, theme.accent));
            }
        }
        // Text search: every occurrence on screen, the current one stronger.
        if let Some(find) = &self.find {
            let offset = term.grid().display_offset() as i32;
            for (i, cells) in find.matches.iter().enumerate() {
                let color = if i == find.current { theme.accent.gamma_multiply(0.55) } else { theme.ansi[3].gamma_multiply(0.28) };
                for p in cells {
                    let row = p.line.0 + offset;
                    if row < 0 || row >= self.size.rows as i32 {
                        continue;
                    }
                    let min = grid_rect.min + Vec2::new(p.column.0 as f32 * cell.x, row as f32 * cell.y);
                    painter.rect_filled(Rect::from_min_size(min, cell), 0.0, color);
                }
            }
        }
        drop(term);
        response
    }

    fn handle_keyboard(&mut self, ui: &Ui) {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        let mode = *self.term.lock().mode();
        let mac = cfg!(target_os = "macos");

        for event in events {
            match event {
                Event::Text(text) => {
                    // On Linux/Windows, Alt+key is sent as ESC-prefixed through the key event.
                    if !(mods.alt && !mac) {
                        self.write_user(text.as_bytes());
                    }
                }
                Event::Ime(egui::ImeEvent::Commit(text)) => self.write_user(text.as_bytes()),
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(bytes) = input::key_to_bytes(key, modifiers, mode) {
                        self.write_user(&bytes);
                    }
                }
                // Outside macOS, Ctrl+C/X/V arrive as clipboard events: only Ctrl+Shift means clipboard.
                Event::Copy => {
                    if mac || mods.shift {
                        if let Some(text) = self.term.lock().selection_to_string() {
                            ui.ctx().copy_text(text);
                        }
                    } else {
                        self.write_user(b"\x03");
                    }
                }
                Event::Cut if !mac => self.write_user(b"\x18"),
                Event::Paste(text) => {
                    if mac || mods.shift {
                        self.paste(&text, mode);
                    } else {
                        self.write_user(b"\x16");
                    }
                }
                _ => {}
            }
        }
    }

    fn paste(&mut self, text: &str, mode: TermMode) {
        if mode.contains(TermMode::BRACKETED_PASTE) {
            let clean = text.replace('\x1b', "");
            self.write_user(format!("\x1b[200~{clean}\x1b[201~").as_bytes());
        } else {
            self.write_user(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
        }
    }

    /// Links are clickable unless the program handles the mouse itself; Cmd (Ctrl) makes them clickable anyway.
    fn link_under_pointer(&self, ui: &Ui, response: &Response, grid_origin: Pos2) -> Option<links::Link> {
        if !response.hovered() {
            return None;
        }
        let (pos, command) = ui.input(|i| (i.pointer.hover_pos(), i.modifiers.command));
        let term = self.term.lock();
        if term.mode().intersects(TermMode::MOUSE_MODE) && !command {
            return None;
        }
        let (point, _) = self.point_at(grid_origin, pos?, term.grid().display_offset());
        links::link_at(&term, point)
    }

    fn handle_mouse(&mut self, ui: &Ui, response: &Response, grid_origin: Pos2) {
        if response.clicked_by(PointerButton::Primary) {
            if let Some(link) = &self.hover_link {
                // Web links open right away; anything else waits for the user's confirmation.
                if links::is_safe(&link.url) {
                    links::open(&link.url);
                } else {
                    self.link_request = Some(link.url.clone());
                }
                return;
            }
        }
        let (mode, offset) = {
            let term = self.term.lock();
            (*term.mode(), term.grid().display_offset())
        };
        let shift = ui.input(|i| i.modifiers.shift);
        let report = mode.intersects(TermMode::MOUSE_MODE) && !shift;
        let pos = ui.input(|i| i.pointer.interact_pos());

        // Wheel.
        if response.hovered() {
            let events = ui.input(|i| i.events.clone());
            for event in events {
                if let Event::MouseWheel { unit, delta, .. } = event {
                    self.scroll_acc += match unit {
                        MouseWheelUnit::Point => delta.y / self.cell.y,
                        MouseWheelUnit::Line => delta.y,
                        MouseWheelUnit::Page => delta.y * self.size.rows as f32,
                    };
                }
            }
            let lines = self.scroll_acc.trunc() as i32;
            if lines != 0 {
                self.scroll_acc -= lines as f32;
                if report {
                    if let Some(p) = pos {
                        let (point, _) = self.point_at(grid_origin, p, 0);
                        let button = if lines > 0 { 64 } else { 65 };
                        for _ in 0..lines.abs() {
                            self.report_mouse(mode, button, point, true);
                        }
                    }
                } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
                    let seq = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                        (true, true) => "\x1bOA",
                        (true, false) => "\x1b[A",
                        (false, true) => "\x1bOB",
                        (false, false) => "\x1b[B",
                    };
                    self.write(seq.repeat(lines.unsigned_abs() as usize).as_bytes());
                } else {
                    self.term.lock().scroll_display(Scroll::Delta(lines));
                }
            }
        }

        let Some(pos) = pos else { return };

        if report {
            let (point, _) = self.point_at(grid_origin, pos, 0);
            let pressed = ui.input(|i| i.pointer.button_pressed(PointerButton::Primary));
            let released = ui.input(|i| i.pointer.button_released(PointerButton::Primary));
            if response.hovered() && pressed {
                self.mouse_down = true;
                self.report_mouse(mode, 0, point, true);
            } else if self.mouse_down && released {
                self.mouse_down = false;
                self.report_mouse(mode, 0, point, false);
            } else if self.mouse_down && response.dragged() && mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
                self.report_mouse(mode, 32, point, true);
            }
            return;
        }

        // Selection.
        let (point, side) = self.point_at(grid_origin, pos, offset);
        if response.double_clicked() || response.triple_clicked() {
            let ty = if response.triple_clicked() { SelectionType::Lines } else { SelectionType::Semantic };
            self.term.lock().selection = Some(Selection::new(ty, point, side));
        } else if response.drag_started_by(PointerButton::Primary) {
            self.term.lock().selection = Some(Selection::new(SelectionType::Simple, point, side));
        } else if response.dragged_by(PointerButton::Primary) {
            if let Some(sel) = self.term.lock().selection.as_mut() {
                sel.update(point, side);
            }
        } else if response.clicked_by(PointerButton::Primary) {
            self.term.lock().selection = None;
        } else if response.clicked_by(PointerButton::Middle) {
            // Middle click pastes the clipboard, or the current selection if the clipboard is empty.
            let clipboard = arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok().filter(|t| !t.is_empty());
            if let Some(text) = clipboard.or_else(|| self.term.lock().selection_to_string()) {
                self.paste(&text, mode);
            }
        }
    }

    fn report_mouse(&mut self, mode: TermMode, button: u8, point: Point, pressed: bool) {
        let (col, row) = (point.column.0 + 1, point.line.0.max(0) as usize + 1);
        if mode.contains(TermMode::SGR_MOUSE) {
            let suffix = if pressed { 'M' } else { 'm' };
            self.write(format!("\x1b[<{button};{col};{row}{suffix}").as_bytes());
        } else {
            let b = if pressed { button } else { 3 };
            let enc = |v: usize| (32 + v.min(223)) as u8;
            self.write(&[0x1b, b'[', b'M', 32 + b, enc(col), enc(row)]);
        }
    }
}

fn looks_like_password_prompt(line: &str) -> bool {
    let line = line.trim_end().to_lowercase();
    let asks = ["password", "mot de passe", "passwort", "contraseña", "passphrase"].iter().any(|w| line.contains(w));
    asks && line.ends_with(':')
}

#[cfg(test)]
mod tests {
    use super::looks_like_password_prompt;

    #[test]
    fn detects_password_prompts() {
        assert!(looks_like_password_prompt("[sudo] password for fserver: "));
        assert!(looks_like_password_prompt("Password:"));
        assert!(looks_like_password_prompt("Mot de passe : "));
        assert!(looks_like_password_prompt("bob@host's password: "));
        assert!(!looks_like_password_prompt("fserver@fserver:~$ echo password"));
        assert!(!looks_like_password_prompt("Enter the new password in the file"));
    }
}
