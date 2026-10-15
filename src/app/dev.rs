//! The "Dev" category of the sidebar: the Node.js versions installed in Ronnie's folder (see
//! `crate::node`), those published to install, and the version each local pane uses.

use std::sync::{Arc, Mutex};

use super::sidebar::{paint_panel_title, paint_row_bg};
use super::*;
use crate::node::{self, Progress, Release};

/// How many published versions the list shows at most (the filter finds the others).
const LIST_MAX: usize = 80;

/// The published versions, once fetched (or why they couldn't be).
type Fetched = Result<Vec<Release>, String>;

/// What the Dev panel knows and does.
#[derive(Default)]
pub(super) struct Dev {
    /// The versions installed, the newest first.
    pub installed: Vec<String>,
    /// The versions published, once fetched (or why they couldn't be).
    releases: Option<Fetched>,
    fetching: Option<Arc<Mutex<Option<Fetched>>>>,
    installs: Vec<Install>,
    /// The published versions listed, to install one.
    browse: bool,
    filter: String,
    /// Every version, not only the long-term support ones.
    all: bool,
}

struct Install {
    version: String,
    progress: Arc<Mutex<Progress>>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Dev {
    pub fn new() -> Self {
        Self { installed: node::installed(), ..Default::default() }
    }

    /// The installed versions read again (from the pane's menu: one may have been added by hand).
    pub fn show_installed(&mut self) {
        self.installed = node::installed();
    }

    fn installing(&self, version: &str) -> Option<Progress> {
        self.installs.iter().find(|i| i.version == version).map(|i| *i.progress.lock().unwrap())
    }

    fn fetch(&mut self, ctx: &egui::Context) {
        let slot = Arc::new(Mutex::new(None));
        self.fetching = Some(slot.clone());
        self.releases = None;
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("node-releases".into()).spawn(move || {
            *slot.lock().unwrap() = Some(node::releases().map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
    }

    fn install(&mut self, ctx: &egui::Context, version: String) {
        if self.installing(&version).is_some() {
            return;
        }
        let progress = Arc::new(Mutex::new(Progress::default()));
        let result = Arc::new(Mutex::new(None));
        let (p, r, v, ctx) = (progress.clone(), result.clone(), version.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("node-install".into()).spawn(move || {
            let done = node::install(&v, &p, &ctx).map_err(|e| format!("{e:#}"));
            crate::log::info(&match &done {
                Ok(()) => format!("Node.js {v} installed"),
                Err(e) => format!("Node.js {v} not installed: {e}"),
            });
            *r.lock().unwrap() = Some(done);
            ctx.request_repaint();
        });
        self.installs.push(Install { version, progress, result });
    }
}

impl App {
    /// Collects what finished in the background: the list of versions, the installs (an error is
    /// shown).
    pub(super) fn dev_poll(&mut self) {
        if let Some(fetched) = self.dev.fetching.as_ref().and_then(|f| f.lock().unwrap().take()) {
            self.dev.releases = Some(fetched);
            self.dev.fetching = None;
        }
        let mut changed = false;
        let t = self.t();
        for install in std::mem::take(&mut self.dev.installs) {
            let done = install.result.lock().unwrap().take();
            match done {
                None => self.dev.installs.push(install),
                Some(Ok(())) => changed = true,
                Some(Err(e)) => self.error = Some(format!("{} : {e}", t.node_install_failed.replace("{v}", &install.version))),
            }
        }
        if changed {
            self.dev.installed = node::installed();
        }
    }

    /// The tab and pane a version picked in the panel goes to: the focused pane of the local tab shown.
    fn dev_target(&self) -> Option<(usize, PaneId)> {
        let index = self.shown_tab()?;
        let tab = self.tabs.get(index).filter(|t| t.ssh.is_none() && t.db.is_none())?;
        Some((index, tab.focused))
    }

    /// Makes pane `pane` of tab `index` use Node.js `version` (None: the system's), from its next
    /// command.
    pub(super) fn set_pane_node(&mut self, index: usize, pane: PaneId, version: Option<String>) {
        let Some(tab) = self.tabs.get_mut(index) else { return };
        let history = *tab.histories.entry(pane).or_insert_with(Uuid::new_v4);
        node::set_pane(history, version.as_deref());
        match version {
            Some(v) => tab.nodes.insert(pane, v),
            None => tab.nodes.remove(&pane),
        };
    }

    /// Deletes an installed version: the open panes using it go back to the system's, and so do new
    /// terminals if it was their default.
    fn uninstall_node(&mut self, version: &str) {
        if let Err(e) = node::uninstall(version) {
            self.error = Some(format!("{e:#}"));
        }
        let using: Vec<(usize, PaneId)> = self.tabs.iter().enumerate().flat_map(|(i, t)| t.nodes.iter().filter(|(_, v)| *v == version).map(move |(p, _)| (i, *p))).collect();
        for (index, pane) in using {
            self.set_pane_node(index, pane, None);
        }
        if self.config.settings.node_default.as_deref() == Some(version) {
            self.config.settings.node_default = None;
        }
        self.dev.installed = node::installed();
    }

    /// The "Dev" category: the Node.js versions installed, and those to install.
    pub(super) fn dev_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        paint_panel_title(&painter, Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H)), t.cat_dev, &theme);
        *y += SECTION_HEADER_H;

        // Node.js, with its hexagon.
        let head = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, 24.0));
        paint_node_icon(&painter, Pos2::new(head.min.x + 14.0, head.center().y), NODE_GREEN);
        painter.text(Pos2::new(head.min.x + 28.0, head.center().y), Align2::LEFT_CENTER, "Node.js", FontId::proportional(13.5), theme.text);
        *y += head.height() + 4.0;

        let target = self.dev_target();
        let target_version = target.and_then(|(i, p)| self.tabs[i].nodes.get(&p).cloned());
        let mut use_in = None;
        let mut set_default = None;
        let mut uninstall = None;
        if self.dev.installed.is_empty() {
            let hint = painter.layout(t.node_none_installed.to_owned(), FontId::proportional(12.0), theme.text_muted, row_w - 12.0);
            let h = hint.size().y;
            painter.galley(Pos2::new(left + 6.0, *y + 2.0), hint, theme.text_muted);
            *y += h + 10.0;
        }
        for version in self.dev.installed.clone() {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H));
            *y += ROW_H + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("node-version", &version)), Sense::click());
            let here = target_version.as_deref() == Some(version.as_str());
            paint_row_bg(&painter, slot, here, resp.hovered(), &theme);
            paint_node_icon(&painter, Pos2::new(slot.min.x + 17.0, slot.center().y), if here { NODE_GREEN } else { NODE_GREEN.gamma_multiply(0.6) });
            let color = if here || resp.hovered() { theme.text } else { theme.text_muted };
            painter.text(Pos2::new(slot.min.x + 34.0, slot.center().y), Align2::LEFT_CENTER, &version, FontId::monospace(12.5), color);
            // On the right: the default star, how many open terminals use it.
            let mut right = slot.max.x - 10.0;
            let panes = self.tabs.iter().flat_map(|t| t.nodes.values()).filter(|v| **v == version).count();
            if self.config.settings.node_default.as_deref() == Some(version.as_str()) {
                let g = painter.text(Pos2::new(right, slot.center().y), Align2::RIGHT_CENTER, "★", FontId::proportional(12.0), theme.accent);
                right = g.min.x - 6.0;
            }
            if panes > 0 {
                let text = if panes == 1 { t.node_pane_one.to_owned() } else { t.node_panes.replace("{n}", &panes.to_string()) };
                painter.text(Pos2::new(right, slot.center().y), Align2::RIGHT_CENTER, text, FontId::proportional(11.0), theme.text_muted.gamma_multiply(0.85));
            }
            let default = self.config.settings.node_default.as_deref() == Some(version.as_str());
            let tip = match target {
                Some(_) => format!("{}\n{}", t.node_use_here, if default { t.node_default_tip } else { "" }).trim_end().to_owned(),
                None => t.node_no_target.to_owned(),
            };
            let resp = resp.on_hover_text(tip);
            if resp.clicked() && target.is_some() {
                use_in = Some(Some(version.clone()));
            }
            resp.context_menu(|ui| {
                ui.set_min_width(230.0);
                if ui.add_enabled(target.is_some() && !here, egui::Button::new(t.node_use_here)).clicked() {
                    use_in = Some(Some(version.clone()));
                    ui.close();
                }
                let label = if default { t.node_unset_default } else { t.node_set_default };
                if ui.button(label).clicked() {
                    set_default = Some((!default).then(|| version.clone()));
                    ui.close();
                }
                if ui.button(t.open_location).clicked() {
                    if let Some(dir) = node::bin_dir(&version) {
                        config::open_folder(if cfg!(windows) { &dir } else { dir.parent().unwrap_or(&dir) });
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button(egui::RichText::new(t.delete).color(theme.ansi[1])).clicked() {
                    uninstall = Some(version.clone());
                    ui.close();
                }
            });
        }
        // The pane shown on the system's: a row for it, to go back to it.
        if let Some((_, _)) = target.filter(|_| !self.dev.installed.is_empty()) {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H));
            *y += ROW_H + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with("node-system"), Sense::click());
            let here = target_version.is_none();
            paint_row_bg(&painter, slot, here, resp.hovered(), &theme);
            paint_node_icon(&painter, Pos2::new(slot.min.x + 17.0, slot.center().y), theme.text_muted.gamma_multiply(0.7));
            painter.text(Pos2::new(slot.min.x + 34.0, slot.center().y), Align2::LEFT_CENTER, t.node_system, FontId::proportional(12.5), if here || resp.hovered() { theme.text } else { theme.text_muted });
            if resp.on_hover_text(t.node_system_tip).clicked() {
                use_in = Some(None);
            }
        }
        *y += 6.0;

        // To install one: the published versions, filtered.
        let label = if self.dev.browse { t.node_hide_list } else { t.node_install };
        let button = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, 26.0));
        let resp = ui.put(button, egui::Button::new(egui::RichText::new(format!("{}  {label}", if self.dev.browse { "▾" } else { "＋" })).size(12.5)).corner_radius(6.0));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.dev.browse = !self.dev.browse;
            if self.dev.browse && self.dev.releases.as_ref().is_none_or(Result::is_err) && self.dev.fetching.is_none() {
                self.dev.fetch(ui.ctx());
            }
        }
        *y += button.height() + 8.0;
        if self.dev.browse {
            self.dev_browse(ui, left, row_w, y);
        }

        if let (Some(version), Some((index, pane))) = (use_in, target) {
            self.set_pane_node(index, pane, version);
            self.focus_terminal = true;
        }
        if let Some(default) = set_default {
            self.config.settings.node_default = default;
        }
        if let Some(version) = uninstall {
            self.uninstall_node(&version);
        }
    }

    /// The versions published on nodejs.org: a filter, LTS only or all, and the list (a click installs).
    fn dev_browse(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        let field = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, 24.0));
        ui.put(field, egui::TextEdit::singleline(&mut self.dev.filter).hint_text(t.node_filter_hint).font(FontId::proportional(12.5)));
        *y += field.height() + 4.0;
        let check = Rect::from_min_size(Pos2::new(left + 2.0, *y), Vec2::new(row_w - 2.0, 20.0));
        let mut lts = !self.dev.all;
        ui.put(check, egui::Checkbox::new(&mut lts, egui::RichText::new(t.node_lts_only).size(12.0)));
        self.dev.all = !lts;
        *y += check.height() + 6.0;

        let message = |painter: &egui::Painter, y: &mut f32, text: &str, color: Color32| {
            let galley = painter.layout(text.to_owned(), FontId::proportional(12.0), color, row_w - 12.0);
            let h = galley.size().y;
            painter.galley(Pos2::new(left + 6.0, *y), galley, color);
            *y += h + 8.0;
        };
        let releases = match &self.dev.releases {
            None => {
                message(&painter, y, t.node_loading, theme.text_muted);
                return;
            }
            Some(Err(e)) => {
                message(&painter, y, e, theme.ansi[1]);
                let retry = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(110.0, 24.0));
                if ui.put(retry, egui::Button::new(t.node_retry).corner_radius(5.0)).clicked() {
                    self.dev.fetch(ui.ctx());
                }
                *y += retry.height() + 8.0;
                return;
            }
            Some(Ok(r)) => r,
        };
        // "20" finds the 20.x, "20.1" the 20.1x and 20.1.x...
        let filter = self.dev.filter.trim().trim_start_matches('v').to_owned();
        let shown: Vec<Release> = releases.iter().filter(|r| (self.dev.all || r.lts.is_some()) && (filter.is_empty() || r.version[1..].starts_with(&filter))).take(LIST_MAX).cloned().collect();
        if shown.is_empty() {
            message(&painter, y, t.node_no_match, theme.text_muted);
            return;
        }
        let mut install = None;
        for release in shown {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H - 4.0));
            *y += slot.height() + 2.0;
            let installed = self.dev.installed.contains(&release.version);
            let progress = self.dev.installing(&release.version);
            let resp = ui.interact(slot, ui.id().with(("node-release", &release.version)), Sense::click());
            let can = !installed && progress.is_none();
            paint_row_bg(&painter, slot, false, can && resp.hovered(), &theme);
            let color = if installed { theme.text_muted.gamma_multiply(0.6) } else if resp.hovered() { theme.text } else { theme.text_muted };
            painter.text(Pos2::new(slot.min.x + 8.0, slot.center().y), Align2::LEFT_CENTER, &release.version, FontId::monospace(12.0), color);
            if let Some(lts) = &release.lts {
                painter.text(Pos2::new(slot.min.x + 84.0, slot.center().y), Align2::LEFT_CENTER, lts, FontId::proportional(11.0), NODE_GREEN.gamma_multiply(if installed { 0.5 } else { 0.9 }));
            }
            let right = Pos2::new(slot.max.x - 8.0, slot.center().y);
            match progress {
                Some(p) => {
                    // A bar filling up, then "Unpacking…".
                    let bar = Rect::from_min_max(Pos2::new(slot.max.x - 70.0, slot.center().y - 2.0), Pos2::new(slot.max.x - 8.0, slot.center().y + 2.0));
                    painter.rect_filled(bar, 2.0, theme.tab_hover);
                    let fraction = if p.unpacking { 1.0 } else if p.total > 0 { p.received as f32 / p.total as f32 } else { 0.0 };
                    painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * fraction.clamp(0.0, 1.0), bar.height())), 2.0, NODE_GREEN);
                    if p.unpacking {
                        painter.text(Pos2::new(bar.min.x - 6.0, slot.center().y), Align2::RIGHT_CENTER, t.node_unpacking, FontId::proportional(10.5), theme.text_muted);
                    }
                }
                None if installed => {
                    painter.text(right, Align2::RIGHT_CENTER, "✓", FontId::proportional(12.0), NODE_GREEN);
                }
                None => {
                    let text = if resp.hovered() { format!("⬇  {}", t.node_do_install) } else { release.date.clone() };
                    painter.text(right, Align2::RIGHT_CENTER, text, FontId::proportional(11.0), if resp.hovered() { theme.accent } else { theme.text_muted.gamma_multiply(0.7) });
                }
            }
            if can && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                install = Some(release.version.clone());
            }
        }
        if let Some(version) = install {
            self.dev.install(ui.ctx(), version);
        }
    }
}

/// Node.js's green.
pub(super) const NODE_GREEN: Color32 = Color32::from_rgb(0x5f, 0xa0, 0x4e);

/// A hexagon, standing on a point: Node.js.
pub(super) fn paint_node_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let points: Vec<Pos2> = (0..6).map(|k| c + Vec2::angled(std::f32::consts::FRAC_PI_6 * (2 * k + 1) as f32) * 6.5).collect();
    painter.add(egui::Shape::closed_line(points, Stroke::new(1.4, color)));
}

/// "</>": the Dev category.
pub(super) fn paint_dev_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    painter.line_segment([c + Vec2::new(-4.0, -4.0), c + Vec2::new(-8.0, 0.0)], stroke);
    painter.line_segment([c + Vec2::new(-8.0, 0.0), c + Vec2::new(-4.0, 4.0)], stroke);
    painter.line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(8.0, 0.0)], stroke);
    painter.line_segment([c + Vec2::new(8.0, 0.0), c + Vec2::new(4.0, 4.0)], stroke);
    painter.line_segment([c + Vec2::new(1.5, -5.5), c + Vec2::new(-1.5, 5.5)], stroke);
}

/// The Node.js submenu of a local pane: the system's or an installed version (the current one
/// checked), and the way to the Dev panel. `picked`: Some(version) chosen; `manage`: the panel asked.
pub(super) fn node_menu(ui: &mut Ui, t: &Strings, installed: &[String], current: Option<&str>, picked: &mut Option<Option<String>>, manage: &mut bool) {
    ui.set_min_width(190.0);
    let mark = |on: bool| if on { "✓  " } else { "     " };
    if ui.button(format!("{}{}", mark(current.is_none()), t.node_system)).clicked() {
        *picked = Some(None);
        ui.close();
    }
    for version in installed {
        if ui.button(egui::RichText::new(format!("{}{version}", mark(current == Some(version.as_str())))).monospace()).clicked() {
            *picked = Some(Some(version.clone()));
            ui.close();
        }
    }
    // Picked but deleted since: said so.
    if let Some(gone) = current.filter(|c| !installed.iter().any(|v| v == c)) {
        ui.label(egui::RichText::new(t.node_missing.replace("{v}", gone)).small().color(ui.visuals().warn_fg_color));
    }
    ui.separator();
    if ui.button(t.node_manage).clicked() {
        *manage = true;
        ui.close();
    }
}
