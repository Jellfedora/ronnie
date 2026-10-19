//! The process page (see `crate::procs`), opened from the resources' gauges in the sidebar: every
//! process of the machine, what it uses, the ports it listens on and the Ronnie tab it was started in;
//! filtered, sorted, grouped by name, and stopped (asked first, killed if it won't).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::procs::{Monitor, Port, Proc, Snapshot};

const ROW: f32 = 26.0;
/// A stop asked: after this long, the process still there can be killed.
const FORCE_AFTER: Duration = Duration::from_secs(3);
/// A first click on "Stop" waits this long for the second.
const CONFIRM_FOR: Duration = Duration::from_secs(4);

/// The tool's orange.
pub(super) const PROCS_ORANGE: Color32 = Color32::from_rgb(0xe0, 0x8a, 0x3c);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Column {
    Name,
    Pid,
    Ports,
    Cpu,
    Memory,
    Disk,
    Time,
}

/// A line of the list: a process, or those of one name when grouped.
#[derive(Clone)]
struct Row {
    pids: Vec<u32>,
    name: String,
    command: String,
    exe: Option<String>,
    cpu: f32,
    memory: u64,
    read: u64,
    written: u64,
    run_time: u64,
    mine: bool,
    ports: Vec<Port>,
    /// The tab whose pane runs it.
    tab: Option<usize>,
}

/// What the list is worked out from: the same, the same rows.
#[derive(Clone, PartialEq)]
struct Key {
    generation: u64,
    filter: String,
    mine: bool,
    ports_only: bool,
    ronnie_only: bool,
    grouped: bool,
    sort: Column,
    desc: bool,
}

pub(super) struct ProcsPage {
    monitor: Option<Monitor>,
    filter: String,
    mine: bool,
    ports_only: bool,
    ronnie_only: bool,
    grouped: bool,
    sort: Column,
    desc: bool,
    /// The row whose stop was clicked once (its first pid), and when.
    confirm: Option<(u32, Instant)>,
    /// The processes asked to stop, and when.
    stopping: HashMap<u32, Instant>,
    rows: Vec<Row>,
    key: Option<Key>,
    /// When the page was last drawn.
    drawn: Option<Instant>,
}

impl Default for ProcsPage {
    fn default() -> Self {
        Self { monitor: None, filter: String::new(), mine: true, ports_only: false, ronnie_only: false, grouped: false, sort: Column::Cpu, desc: true, confirm: None, stopping: HashMap::new(), rows: Vec::new(), key: None, drawn: None }
    }
}

impl ProcsPage {
    /// The page no longer drawn (left, or its window hidden): its reading thread stops, to start
    /// again with the page.
    pub fn poll(&mut self) {
        if self.monitor.is_some() && self.drawn.is_none_or(|at| at.elapsed() > Duration::from_secs(2)) {
            self.monitor = None;
            self.key = None;
            self.rows.clear();
        }
    }
}

impl App {
    /// The pid of each pane's shell, and its tab.
    fn shells(&self) -> HashMap<u32, usize> {
        self.tabs.iter().enumerate().flat_map(|(i, tab)| tab.panes.values().filter_map(move |term| Some((term.pid()?, i)))).collect()
    }

    /// The rows for what's asked, worked out again only when it changed.
    fn procs_rows(&mut self, snapshot: &Snapshot) {
        let p = &self.procs;
        let key = Key { generation: snapshot.generation, filter: p.filter.clone(), mine: p.mine, ports_only: p.ports_only, ronnie_only: p.ronnie_only, grouped: p.grouped, sort: p.sort, desc: p.desc };
        if p.key.as_ref() == Some(&key) {
            return;
        }
        // Each process's tab: the first shell among its ancestors.
        let shells = self.shells();
        let parents: HashMap<u32, Option<u32>> = snapshot.procs.iter().map(|p| (p.pid, p.parent)).collect();
        let tab_of = |pid: u32| {
            let mut at = Some(pid);
            for _ in 0..64 {
                let pid = at?;
                if let Some(tab) = shells.get(&pid) {
                    return Some(*tab);
                }
                at = parents.get(&pid).copied().flatten().filter(|p| *p != pid);
            }
            None
        };
        let words: Vec<String> = key.filter.split_whitespace().map(|w| w.trim_start_matches(':').to_lowercase()).collect();
        let matches = |p: &Proc| {
            words.iter().all(|w| {
                p.name.to_lowercase().contains(w) || p.pid.to_string() == *w || p.ports.iter().any(|port| port.port.to_string() == *w) || p.command.to_lowercase().contains(w)
            })
        };
        let row = |p: &Proc| Row { pids: vec![p.pid], name: p.name.clone(), command: p.command.clone(), exe: p.exe.clone(), cpu: p.cpu, memory: p.memory, read: p.read, written: p.written, run_time: p.run_time, mine: p.mine, ports: p.ports.clone(), tab: tab_of(p.pid) };
        let kept = snapshot.procs.iter().filter(|p| (!key.mine || p.mine) && (!key.ports_only || !p.ports.is_empty()) && matches(p)).map(row).filter(|r| !key.ronnie_only || r.tab.is_some());
        let mut rows: Vec<Row> = if key.grouped {
            let mut groups: Vec<Row> = Vec::new();
            let mut at: HashMap<String, usize> = HashMap::new();
            for r in kept {
                match at.get(&r.name) {
                    Some(&i) => {
                        let g = &mut groups[i];
                        g.pids.extend(r.pids);
                        g.cpu += r.cpu;
                        g.memory += r.memory;
                        g.read += r.read;
                        g.written += r.written;
                        g.run_time = g.run_time.max(r.run_time);
                        g.mine &= r.mine;
                        g.ports.extend(r.ports);
                        g.tab = g.tab.or(r.tab);
                    }
                    None => {
                        at.insert(r.name.clone(), groups.len());
                        groups.push(r);
                    }
                }
            }
            for g in &mut groups {
                g.ports.sort();
                g.ports.dedup_by_key(|p| p.port);
            }
            groups
        } else {
            kept.collect()
        };
        rows.sort_by(|a, b| {
            let order = match key.sort {
                Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Column::Pid => a.pids[0].cmp(&b.pids[0]),
                // Those without a port last, whichever way.
                Column::Ports => match (a.ports.first(), b.ports.first()) {
                    (Some(x), Some(y)) => x.port.cmp(&y.port),
                    (Some(_), None) => return std::cmp::Ordering::Less,
                    (None, Some(_)) => return std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                },
                Column::Cpu => a.cpu.total_cmp(&b.cpu),
                Column::Memory => a.memory.cmp(&b.memory),
                Column::Disk => (a.read + a.written).cmp(&(b.read + b.written)),
                Column::Time => a.run_time.cmp(&b.run_time),
            };
            let order = if key.desc { order.reverse() } else { order };
            order.then_with(|| a.pids[0].cmp(&b.pids[0]))
        });
        self.procs.rows = rows;
        self.procs.key = Some(key);
    }

    /// Asks the row's processes to stop (`force`: kills them). The list is read again at once.
    fn procs_stop(&mut self, row: &Row, force: bool) {
        let t = self.t();
        for &pid in &row.pids {
            match crate::procs::stop(pid, force) {
                Ok(()) => {
                    crate::log::info(&format!("Process {} ({pid}) {}", row.name, if force { "killed" } else { "asked to stop" }));
                    self.procs.stopping.insert(pid, Instant::now());
                }
                Err(e) => self.error = Some(t.procs_stop_failed.replace("{name}", &row.name).replace("{pid}", &pid.to_string()).replace("{e}", &e)),
            }
        }
        self.procs.confirm = None;
        if let Some(m) = &self.procs.monitor {
            m.refresh();
        }
    }

    /// The process page, in this window: shown instead of the tabs.
    pub(super) fn open_procs_page(&mut self) {
        self.procs_page = true;
        self.dev_page = false;
        self.modules_page = false;
        self.music_page = false;
        self.notes_page = false;
        self.home_game = false;
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    /// The page: the card fills it, margins aside.
    pub(super) fn procs_page_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let (t, theme) = (self.t(), self.theme.clone());
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        let margin = if rect.width() > 700.0 { 40.0 } else { 16.0 };
        let area = Rect::from_min_max(rect.min + Vec2::new(margin, 24.0), rect.max - Vec2::new(margin, 24.0));
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
        self.procs_card(&mut inner, t, &theme);
    }

    /// The machine's load, the filters, the list.
    fn procs_card(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        self.procs.drawn = Some(Instant::now());
        let snapshot = self.procs.monitor.get_or_insert_with(|| Monitor::start(ui.ctx())).snapshot();
        // Those gone are no longer stopping.
        if !self.procs.stopping.is_empty() {
            let alive: std::collections::HashSet<u32> = snapshot.procs.iter().map(|p| p.pid).collect();
            self.procs.stopping.retain(|pid, _| alive.contains(pid));
        }
        if self.procs.confirm.is_some_and(|(_, at)| at.elapsed() > CONFIRM_FOR) {
            self.procs.confirm = None;
        }
        let height = ui.available_height() - 40.0;
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(14.0).inner_margin(20.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(height.max(300.0));
            self.procs_header(ui, t, theme, &snapshot);
            ui.add_space(12.0);
            self.procs_toolbar(ui, t);
            ui.add_space(8.0);
            if snapshot.generation == 0 {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(t.procs_loading).color(theme.text_muted));
                });
                return;
            }
            self.procs_rows(&snapshot);
            self.procs_table(ui, t, theme, &snapshot);
        });
        // A stop that may need forcing: its button shows when the time comes.
        if let Some(at) = self.procs.stopping.values().min() {
            ui.ctx().request_repaint_after(FORCE_AFTER.saturating_sub(at.elapsed()) + Duration::from_millis(50));
        }
    }

    /// The title, how many processes and ports, the machine's CPU and memory.
    fn procs_header(&self, ui: &mut Ui, t: &'static Strings, theme: &Theme, s: &Arc<Snapshot>) {
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(Vec2::new(22.0, 28.0), Sense::hover());
            paint_procs_icon(ui.painter(), icon.center(), PROCS_ORANGE);
            ui.label(egui::RichText::new(t.procs_tool).size(19.0).color(theme.text));
            ui.add_space(10.0);
            if s.generation > 0 {
                let ports: usize = s.procs.iter().map(|p| p.ports.len()).sum();
                let text = format!("{}  ·  {}", t.procs_count.replace("{n}", &s.procs.len().to_string()), t.procs_listening.replace("{n}", &ports.to_string()));
                ui.label(egui::RichText::new(text).size(12.5).color(theme.text_muted));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if s.generation == 0 {
                    return;
                }
                let ram = if s.ram_total > 0 { s.ram_used as f32 / s.ram_total as f32 } else { 0.0 };
                let ram_text = format!("{} / {}", crate::health::bytes(s.ram_used, t.health_units), crate::health::bytes(s.ram_total, t.health_units));
                meter(ui, "RAM", &ram_text, ram, theme.ansi[2], theme);
                ui.add_space(14.0);
                meter(ui, "CPU", &format!("{:.0} %", s.cpu), s.cpu / 100.0, theme.accent, theme);
            });
        });
    }

    fn procs_toolbar(&mut self, ui: &mut Ui, t: &'static Strings) {
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.procs.filter).hint_text(t.procs_filter_hint).desired_width(260.0));
            if !self.procs.filter.is_empty() && ui.add(egui::Button::new("✕").frame_when_inactive(false)).clicked() {
                self.procs.filter.clear();
            }
            ui.add_space(8.0);
            ui.checkbox(&mut self.procs.mine, t.procs_mine).on_hover_text(t.procs_mine_hint);
            ui.checkbox(&mut self.procs.ports_only, t.procs_ports_only);
            ui.checkbox(&mut self.procs.ronnie_only, t.procs_ronnie_only);
            ui.checkbox(&mut self.procs.grouped, t.procs_grouped);
        });
    }

    /// The header (a click sorts), then the rows that show.
    fn procs_table(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme, snapshot: &Snapshot) {
        let width = ui.available_width();
        let wide = width > 900.0;
        // (column, title, width); the name takes what's left.
        let mut columns = vec![(Column::Pid, "PID", 70.0), (Column::Ports, t.procs_col_ports, 150.0), (Column::Cpu, "CPU", 84.0), (Column::Memory, t.procs_col_memory, 96.0)];
        if wide {
            columns.push((Column::Disk, t.procs_col_disk, 96.0));
            columns.push((Column::Time, t.procs_col_time, 72.0));
        }
        let action_w = 96.0;
        let name_w = (width - columns.iter().map(|c| c.2).sum::<f32>() - action_w).max(120.0);
        let mut layout = vec![(Column::Name, t.procs_col_name, name_w)];
        layout.extend(columns);

        let (head, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
        let mut x = head.min.x;
        for &(column, title, w) in &layout {
            let cell = Rect::from_min_size(Pos2::new(x, head.min.y), Vec2::new(w, head.height()));
            x += w;
            let resp = ui.interact(cell, ui.id().with(("procs-sort", column as u8)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let resp = match column {
                Column::Cpu => resp.on_hover_text(t.procs_cpu_hint),
                _ => resp,
            };
            let sorted = self.procs.sort == column;
            let text = if sorted { format!("{}  {}", title.to_uppercase(), if self.procs.desc { "▼" } else { "▲" }) } else { title.to_uppercase() };
            let (align, at) = if numeric(column) { (Align2::RIGHT_CENTER, Pos2::new(cell.max.x - 10.0, cell.center().y)) } else { (Align2::LEFT_CENTER, Pos2::new(cell.min.x + 8.0, cell.center().y)) };
            ui.painter().text(at, align, text, FontId::proportional(11.0), if sorted || resp.hovered() { theme.text } else { theme.text_muted });
            if resp.clicked() {
                if sorted {
                    self.procs.desc = !self.procs.desc;
                } else {
                    self.procs.sort = column;
                    // Numbers: the biggest first; words: from A.
                    self.procs.desc = numeric(column) && column != Column::Pid;
                }
            }
        }
        ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, theme.tab_hover));
        ui.add_space(4.0);

        if self.procs.rows.is_empty() {
            ui.add_space(12.0);
            ui.label(egui::RichText::new(t.procs_none).color(theme.text_muted));
            return;
        }
        let me = std::process::id();
        let rows = std::mem::take(&mut self.procs.rows);
        let (mut stop, mut force, mut go_tab, mut confirm, mut group) = (None, None, None, None, None);
        let tab_names: Vec<String> = self.tabs.iter().map(|t| t.title().to_owned()).collect();
        egui::ScrollArea::vertical().id_salt("procs-list").auto_shrink([false, false]).show_rows(ui, ROW, rows.len(), |ui, range| {
            for (k, row) in rows[range.clone()].iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW), Sense::hover());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 5.0, theme.tab_hover.gamma_multiply(0.6));
                } else if (range.start + k) % 2 == 1 {
                    ui.painter().rect_filled(rect, 5.0, theme.bg.gamma_multiply(0.35));
                }
                let painter = ui.painter().clone();
                let y = rect.center().y;
                let mut x = rect.min.x;
                for &(column, _, w) in &layout {
                    let cell = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w, ROW));
                    x += w;
                    let right = Pos2::new(cell.max.x - 10.0, y);
                    let value = |text: String, color: Color32| {
                        painter.text(right, Align2::RIGHT_CENTER, text, FontId::monospace(12.0), color);
                    };
                    match column {
                        Column::Name => {
                            let color = if row.mine { theme.text } else { theme.text_muted };
                            let mut name = row.name.clone();
                            if row.pids.len() > 1 {
                                name = format!("{name}  ×{}", row.pids.len());
                            }
                            let galley = painter.layout_no_wrap(name, FontId::proportional(13.0), color);
                            let room = cell.width() - 16.0;
                            let name_rect = Rect::from_min_size(Pos2::new(cell.min.x + 8.0, y - galley.size().y / 2.0), galley.size().min(Vec2::new(room, ROW)));
                            painter.with_clip_rect(cell.shrink2(Vec2::new(8.0, 0.0))).galley(name_rect.min, galley.clone(), color);
                            let hover = ui.interact(name_rect, ui.id().with(("procs-name", row.pids[0])), if row.pids.len() > 1 { Sense::click() } else { Sense::hover() });
                            let detail = if row.pids.len() > 1 {
                                t.procs_group_hint.replace("{n}", &row.pids.len().to_string())
                            } else {
                                let mut lines = vec![];
                                if !row.command.is_empty() {
                                    lines.push(clip(&row.command, 600));
                                }
                                if let Some(exe) = row.exe.as_ref().filter(|e| !row.command.starts_with(e.as_str())) {
                                    lines.push(exe.clone());
                                }
                                lines.join("\n")
                            };
                            let hover = if detail.is_empty() { hover } else { hover.on_hover_text(detail) };
                            if hover.clicked() {
                                group = Some(row.name.clone());
                            }
                            // The tab it runs in, after the name.
                            if let Some(tab) = row.tab
                                && let Some(title) = tab_names.get(tab)
                                && galley.size().x + 40.0 < room
                            {
                                let at = Pos2::new(name_rect.max.x + 10.0, y);
                                let badge = painter.layout_no_wrap(format!("› {title}"), FontId::proportional(11.5), theme.accent);
                                let badge_rect = Rect::from_min_size(Pos2::new(at.x, y - badge.size().y / 2.0), badge.size().min(Vec2::new(cell.max.x - 8.0 - at.x, ROW)));
                                let r = ui.interact(badge_rect, ui.id().with(("procs-tab", row.pids[0])), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.procs_in_tab.replace("{t}", title));
                                painter.with_clip_rect(badge_rect).galley(badge_rect.min, badge, if r.hovered() { theme.text } else { theme.accent });
                                if r.clicked() {
                                    go_tab = Some(tab);
                                }
                            }
                        }
                        Column::Pid => value(if row.pids.len() > 1 { "—".into() } else { row.pids[0].to_string() }, theme.text_muted),
                        Column::Ports => {
                            let mut px = cell.min.x + 8.0;
                            for port in &row.ports {
                                let text = painter.layout_no_wrap(port.port.to_string(), FontId::monospace(11.5), theme.text);
                                let chip = Rect::from_min_size(Pos2::new(px, y - 9.0), Vec2::new(text.size().x + 12.0, 18.0));
                                if chip.max.x > cell.max.x - 6.0 {
                                    painter.text(Pos2::new(px, y), Align2::LEFT_CENTER, "…", FontId::proportional(12.0), theme.text_muted);
                                    break;
                                }
                                px = chip.max.x + 4.0;
                                let r = ui.interact(chip, ui.id().with(("procs-port", row.pids[0], port.port)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                                let mut tip = t.procs_open_port.replace("{p}", &port.port.to_string());
                                if !port.local {
                                    tip = format!("{tip}\n{}", t.procs_port_network);
                                }
                                let r = r.on_hover_text(tip);
                                let color = if port.local { theme.accent } else { theme.ansi[3] };
                                painter.rect(chip, 4.0, if r.hovered() { color.gamma_multiply(0.25) } else { Color32::TRANSPARENT }, Stroke::new(1.0, color.gamma_multiply(0.8)), egui::StrokeKind::Inside);
                                painter.galley(chip.center() - text.size() / 2.0, text, theme.text);
                                if r.clicked() {
                                    crate::terminal::open_url(&format!("http://localhost:{}", port.port));
                                }
                            }
                        }
                        Column::Cpu => {
                            bar(&painter, cell, row.cpu / 100.0, theme.accent);
                            value(format!("{:.1} %", row.cpu), if row.cpu >= 0.1 { theme.text } else { theme.text_muted });
                        }
                        Column::Memory => {
                            bar(&painter, cell, if snapshot.ram_total > 0 { row.memory as f32 / snapshot.ram_total as f32 } else { 0.0 }, theme.ansi[2]);
                            value(crate::health::bytes(row.memory, t.health_units), theme.text);
                        }
                        Column::Disk => {
                            let total = row.read + row.written;
                            value(if total == 0 { "—".into() } else { format!("{}/s", crate::health::bytes(total, t.health_units)) }, if total == 0 { theme.text_muted } else { theme.text });
                            if total > 0 {
                                let rates = t.procs_disk_hint.replace("{r}", &crate::health::bytes(row.read, t.health_units)).replace("{w}", &crate::health::bytes(row.written, t.health_units));
                                ui.interact(cell, ui.id().with(("procs-disk", row.pids[0])), Sense::hover()).on_hover_text(rates);
                            }
                        }
                        Column::Time => value(duration(row.run_time, t), theme.text_muted),
                    }
                }
                // Stop: asked twice; still there a moment after, it can be killed. Not Ronnie itself.
                let action = Rect::from_min_max(Pos2::new(rect.max.x - action_w, rect.min.y + 2.0), Pos2::new(rect.max.x - 4.0, rect.max.y - 2.0));
                let asked = row.pids.iter().filter_map(|p| self.procs.stopping.get(p)).min();
                if row.pids.contains(&me) {
                    painter.text(Pos2::new(action.max.x - 6.0, y), Align2::RIGHT_CENTER, t.procs_is_ronnie, FontId::proportional(11.5), theme.text_muted);
                } else if let Some(at) = asked {
                    if at.elapsed() < FORCE_AFTER {
                        painter.text(Pos2::new(action.max.x - 6.0, y), Align2::RIGHT_CENTER, t.procs_stopping, FontId::proportional(11.5), theme.text_muted);
                    } else {
                        let button = egui::Button::new(egui::RichText::new(t.procs_force).size(12.0).color(theme.bg)).fill(theme.ansi[1]).corner_radius(5.0);
                        if ui.put(action, button).on_hover_text(t.procs_force_hint).clicked() {
                            force = Some(row.clone());
                        }
                    }
                } else if resp.hovered() || self.procs.confirm.is_some_and(|(p, _)| p == row.pids[0]) {
                    let confirming = self.procs.confirm.is_some_and(|(p, _)| p == row.pids[0]);
                    let label = if confirming { egui::RichText::new(t.procs_confirm_stop).color(theme.bg) } else { egui::RichText::new(t.procs_stop).color(theme.ansi[1]) };
                    let button = egui::Button::new(label.size(12.0)).corner_radius(5.0);
                    let button = if confirming { button.fill(theme.ansi[1]) } else { button.frame_when_inactive(false) };
                    if ui.put(action, button).clicked() {
                        if confirming { stop = Some(row.clone()) } else { confirm = Some(row.pids[0]) }
                    }
                }
            }
        });
        self.procs.rows = rows;
        if let Some(pid) = confirm {
            self.procs.confirm = Some((pid, Instant::now()));
        }
        if let Some(row) = stop {
            self.procs_stop(&row, false);
        }
        if let Some(row) = force {
            self.procs_stop(&row, true);
        }
        if let Some(name) = group {
            self.procs.grouped = false;
            self.procs.filter = name;
        }
        if let Some(tab) = go_tab {
            self.select(tab);
        }
    }
}

fn numeric(column: Column) -> bool {
    !matches!(column, Column::Name | Column::Ports)
}

/// A gauge in the header: its name, a bar, its value.
fn meter(ui: &mut Ui, name: &str, text: &str, fraction: f32, color: Color32, theme: &Theme) {
    ui.label(egui::RichText::new(text).monospace().size(12.0).color(theme.text));
    let (rect, _) = ui.allocate_exact_size(Vec2::new(70.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, theme.tab_hover);
    ui.painter().rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height())), 3.0, color);
    ui.label(egui::RichText::new(name).size(11.0).color(theme.text_muted));
}

/// A share drawn behind a cell's value: thin, at its bottom.
fn bar(painter: &egui::Painter, cell: Rect, fraction: f32, color: Color32) {
    if fraction <= 0.002 {
        return;
    }
    let track = Rect::from_min_max(Pos2::new(cell.min.x + 10.0, cell.max.y - 5.0), Pos2::new(cell.max.x - 10.0, cell.max.y - 3.0));
    painter.rect_filled(Rect::from_min_size(track.min, Vec2::new((track.width() * fraction.clamp(0.0, 1.0)).max(1.5), track.height())), 1.0, color.gamma_multiply(0.75));
}

/// "3 j", "5 h 12", "12 min", "40 s".
fn duration(seconds: u64, t: &Strings) -> String {
    match seconds {
        s if s >= 86_400 => t.procs_days.replace("{n}", &(s / 86_400).to_string()),
        s if s >= 3_600 => format!("{} h {:02}", s / 3_600, s % 3_600 / 60),
        s if s >= 60 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// A pulse: the process list.
pub(super) fn paint_procs_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let points = [(-8.0, 1.0), (-4.0, 1.0), (-2.0, -5.0), (1.0, 5.0), (3.5, -2.0), (5.0, 1.0), (8.0, 1.0)].map(|(x, y)| c + Vec2::new(x, y));
    painter.add(egui::Shape::line(points.to_vec(), Stroke::new(1.4, color)));
}
