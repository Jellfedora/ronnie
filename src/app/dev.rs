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

/// A tool of the Dev category, with a page of its own (PHP, a mail catcher… to come).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum DevTool {
    #[default]
    Node,
}

impl DevTool {
    pub const ALL: [DevTool; 1] = [DevTool::Node];
}

/// What the Dev panel knows and does.
#[derive(Default)]
pub(super) struct Dev {
    /// The versions installed, the newest first.
    pub installed: Vec<String>,
    /// The versions published, once fetched (or why they couldn't be).
    releases: Option<Fetched>,
    fetching: Option<Arc<Mutex<Option<Fetched>>>>,
    installs: Vec<Install>,
    filter: String,
    /// Every version, not only the long-term support ones.
    all: bool,
    /// The tool whose page is shown.
    pub tool: DevTool,
    /// The version whose delete button was clicked once, on the page: a second click deletes it.
    confirm_delete: Option<String>,
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

    /// The "Dev" category: its tools, one a row (Node.js for now); a click shows the tool's page.
    pub(super) fn dev_section(&mut self, ui: &mut Ui, left: f32, row_w: f32, y: &mut f32) {
        let t = self.t();
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        paint_panel_title(&painter, Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, SECTION_HEADER_H)), t.cat_dev, &theme);
        *y += SECTION_HEADER_H;
        let mut open = None;
        for tool in DevTool::ALL {
            let slot = Rect::from_min_size(Pos2::new(left, *y), Vec2::new(row_w, ROW_H + 8.0));
            *y += slot.height() + ROW_GAP;
            let resp = ui.interact(slot, ui.id().with(("dev-tool", tool as u8)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let active = self.dev_page && self.dev.tool == tool;
            paint_row_bg(&painter, slot, active, resp.hovered(), &theme);
            let icon = Pos2::new(slot.min.x + 17.0, slot.center().y);
            let (name, note) = match tool {
                DevTool::Node => {
                    paint_node_icon(&painter, icon, NODE_GREEN);
                    let n = self.dev.installed.len();
                    ("Node.js", (n > 0).then(|| if n == 1 { t.node_version_one.to_owned() } else { t.node_versions.replace("{n}", &n.to_string()) }))
                }
            };
            painter.text(Pos2::new(slot.min.x + 36.0, slot.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), if active || resp.hovered() { theme.text } else { theme.text_muted });
            if let Some(note) = note {
                painter.text(Pos2::new(slot.max.x - 10.0, slot.center().y), Align2::RIGHT_CENTER, note, FontId::proportional(11.5), theme.text_muted.gamma_multiply(0.85));
            }
            if resp.clicked() {
                open = Some(tool);
            }
        }
        if let Some(tool) = open {
            self.dev.tool = tool;
            self.open_dev_page();
        }
    }
}

impl App {
    /// The Dev page, in this window: shown instead of the tabs. The list of versions is fetched if it
    /// isn't yet.
    pub(super) fn open_dev_page(&mut self) {
        self.dev_page = true;
        self.music_page = false;
        self.notes_page = false;
        self.home_game = false;
        self.dev.show_installed();
        if self.dev.releases.as_ref().is_none_or(Result::is_err) && self.dev.fetching.is_none() {
            let ctx = self.ctx.clone();
            self.dev.fetch(&ctx);
        }
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    /// The Dev page: Node.js, the versions installed and those to install side by side, and where a
    /// terminal's version is picked.
    pub(super) fn dev_page_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        // The whole page, margins aside: the card fills what the title leaves, its lists scroll.
        let margin = if rect.width() > 700.0 { 40.0 } else { 16.0 };
        let area = Rect::from_min_max(rect.min + Vec2::new(margin, 24.0), rect.max - Vec2::new(margin, 24.0));
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let width = area.width();
        self.dev_page_content(&mut inner, width, t, &theme);
    }

    fn dev_page_content(&mut self, ui: &mut Ui, width: f32, t: &'static Strings, theme: &Theme) {
        // The title, in the logo's font.
        let (title, _) = ui.allocate_exact_size(Vec2::new(width, 56.0), Sense::hover());
        paint_metal(ui.painter(), Pos2::new(title.min.x, title.center().y), Align2::LEFT_CENTER, t.cat_dev, 44.0, theme.accent, 1.0);
        ui.label(egui::RichText::new(t.dev_subtitle).size(13.5).color(theme.text_muted));
        // Where a terminal's version is picked.
        ui.label(egui::RichText::new(t.node_where).size(12.5).color(theme.text_muted.gamma_multiply(0.85)));
        ui.add_space(18.0);

        let card = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
            let height = ui.available_height() - 40.0;
            Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(14.0).inner_margin(20.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(height.max(200.0));
                add(ui);
            });
        };

        // Node.js: installed | to install.
        card(ui, &mut |ui| {
            let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
            paint_node_icon(ui.painter(), Pos2::new(head.min.x + 9.0, head.center().y), NODE_GREEN);
            ui.painter().text(Pos2::new(head.min.x + 26.0, head.center().y), Align2::LEFT_CENTER, "Node.js", FontId::proportional(19.0), theme.text);
            ui.add_space(10.0);
            let wide = ui.available_width() > 640.0;
            if wide {
                ui.columns(2, |cols| {
                    let height = cols[0].available_height();
                    egui::ScrollArea::vertical().id_salt("dev-installed").max_height(height).auto_shrink([false, true]).show(&mut cols[0], |ui| self.dev_installed_list(ui, t, theme));
                    self.dev_releases_list(&mut cols[1], t, theme);
                });
            } else {
                // One under the other: the installed ones take what they need, up to a third.
                let third = ui.available_height() / 3.0;
                egui::ScrollArea::vertical().id_salt("dev-installed-narrow").max_height(third).auto_shrink([false, true]).show(ui, |ui| self.dev_installed_list(ui, t, theme));
                ui.add_space(16.0);
                self.dev_releases_list(ui, t, theme);
            }
        });
    }

    /// The versions installed: default star, terminals using it, delete (a second click confirms).
    fn dev_installed_list(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        ui.label(egui::RichText::new(t.node_installed_title.to_uppercase()).size(11.0).color(theme.text_muted).extra_letter_spacing(1.2));
        ui.add_space(6.0);
        if self.dev.installed.is_empty() {
            ui.label(egui::RichText::new(t.node_none_installed).size(12.5).color(theme.text_muted));
            return;
        }
        let lts_of = |v: &str| self.dev.releases.as_ref().and_then(|r| r.as_ref().ok()).and_then(|r| r.iter().find(|x| x.version == v)).and_then(|x| x.lts.clone());
        let (mut set_default, mut delete, mut open) = (None, None, None);
        for version in self.dev.installed.clone() {
            let default = self.config.settings.node_default.as_deref() == Some(version.as_str());
            let panes = self.tabs.iter().flat_map(|t| t.nodes.values()).filter(|v| **v == version).count();
            Frame::NONE.fill(theme.bg.gamma_multiply(0.6)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&version).monospace().size(13.5).color(theme.text));
                    if let Some(lts) = lts_of(&version) {
                        ui.label(egui::RichText::new(format!("LTS {lts}")).size(11.0).color(NODE_GREEN));
                    }
                    if panes > 0 {
                        let text = if panes == 1 { t.node_pane_one.to_owned() } else { t.node_panes.replace("{n}", &panes.to_string()) };
                        ui.label(egui::RichText::new(text).size(11.0).color(theme.text_muted));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let confirming = self.dev.confirm_delete.as_deref() == Some(version.as_str());
                        let label = if confirming { egui::RichText::new(t.node_confirm_delete).color(theme.bg) } else { egui::RichText::new(t.delete).color(theme.ansi[1]) };
                        let button = egui::Button::new(label.size(12.0)).corner_radius(5.0);
                        let button = if confirming { button.fill(theme.ansi[1]) } else { button.frame_when_inactive(false) };
                        if ui.add(button).clicked() {
                            if confirming { delete = Some(version.clone()) } else { self.dev.confirm_delete = Some(version.clone()) }
                        }
                        // The folder icon drawn (the emoji renders badly).
                        let folder = ui.add(egui::Button::new("    ").frame_when_inactive(false)).on_hover_text(t.open_location);
                        super::notes::paint_folder_icon(ui.painter(), folder.rect.center(), if folder.hovered() { theme.text } else { theme.text_muted });
                        if folder.clicked() {
                            open = Some(version.clone());
                        }
                        let star = egui::RichText::new(format!("★ {}", t.node_default_short)).size(12.0).color(if default { theme.accent } else { theme.text_muted.gamma_multiply(0.7) });
                        if ui.add(egui::Button::new(star).frame_when_inactive(default).corner_radius(5.0)).on_hover_text(if default { t.node_unset_default } else { t.node_set_default }).clicked() {
                            set_default = Some((!default).then(|| version.clone()));
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
        if let Some(default) = set_default {
            self.config.settings.node_default = default;
        }
        if let Some(version) = open.and_then(|v| node::bin_dir(&v)) {
            config::open_folder(if cfg!(windows) { &version } else { version.parent().unwrap_or(&version) });
        }
        if let Some(version) = delete {
            self.dev.confirm_delete = None;
            self.uninstall_node(&version);
        }
    }

    /// The versions on nodejs.org: filter, LTS only, the newest LTS recommended, install buttons.
    fn dev_releases_list(&mut self, ui: &mut Ui, t: &'static Strings, theme: &Theme) {
        ui.label(egui::RichText::new(t.node_available_title.to_uppercase()).size(11.0).color(theme.text_muted).extra_letter_spacing(1.2));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.dev.filter).hint_text(t.node_filter_hint).desired_width(150.0));
            let mut lts = !self.dev.all;
            ui.checkbox(&mut lts, t.node_lts_only);
            self.dev.all = !lts;
        });
        ui.add_space(6.0);
        let releases = match &self.dev.releases {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(t.node_loading).color(theme.text_muted));
                });
                return;
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new(e).color(theme.ansi[1]));
                if ui.button(t.node_retry).clicked() {
                    self.dev.fetch(ui.ctx());
                }
                return;
            }
            Some(Ok(r)) => r.clone(),
        };
        let newest_lts = releases.iter().find(|r| r.lts.is_some()).map(|r| r.version.clone());
        let filter = self.dev.filter.trim().trim_start_matches('v').to_owned();
        let shown: Vec<&Release> = releases.iter().filter(|r| (self.dev.all || r.lts.is_some()) && (filter.is_empty() || r.version[1..].starts_with(&filter))).take(LIST_MAX).collect();
        if shown.is_empty() {
            ui.label(egui::RichText::new(t.node_no_match).color(theme.text_muted));
            return;
        }
        let mut install = None;
        egui::ScrollArea::vertical().id_salt("dev-releases").max_height(ui.available_height()).auto_shrink([false, true]).show(ui, |ui| {
            for release in shown {
                let installed = self.dev.installed.contains(&release.version);
                let progress = self.dev.installing(&release.version);
                ui.horizontal(|ui| {
                    ui.set_min_height(26.0);
                    ui.label(egui::RichText::new(&release.version).monospace().size(13.0).color(if installed { theme.text_muted } else { theme.text }));
                    if let Some(lts) = &release.lts {
                        ui.label(egui::RichText::new(lts).size(11.0).color(NODE_GREEN));
                    }
                    if newest_lts.as_deref() == Some(release.version.as_str()) {
                        ui.label(egui::RichText::new(t.node_recommended).size(10.5).color(theme.bg).background_color(NODE_GREEN));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match progress {
                            Some(p) if p.unpacking => {
                                ui.label(egui::RichText::new(t.node_unpacking).size(11.5).color(theme.text_muted));
                            }
                            Some(p) => {
                                let fraction = if p.total > 0 { p.received as f32 / p.total as f32 } else { 0.0 };
                                ui.add(egui::ProgressBar::new(fraction).desired_width(110.0).fill(NODE_GREEN).show_percentage());
                            }
                            None if installed => {
                                ui.label(egui::RichText::new(format!("✓ {}", t.node_installed_badge)).size(11.5).color(NODE_GREEN));
                            }
                            None => {
                                if ui.add(egui::Button::new(egui::RichText::new(format!("↓  {}", t.node_do_install)).size(12.0)).corner_radius(5.0)).clicked() {
                                    install = Some(release.version.clone());
                                }
                            }
                        }
                        ui.label(egui::RichText::new(&release.date).size(11.0).color(theme.text_muted.gamma_multiply(0.8)));
                    });
                });
            }
        });
        if let Some(version) = install {
            self.dev.install(ui.ctx(), version);
        }
    }
}

/// Node.js's green.
pub(super) const NODE_GREEN: Color32 = Color32::from_rgb(0x5f, 0xa0, 0x4e);

/// A hexagon, standing on a point: Node.js.
pub(super) fn paint_node_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    paint_node_icon_sized(painter, c, color, 6.5);
}

/// The hexagon, of `radius`.
pub(super) fn paint_node_icon_sized(painter: &egui::Painter, c: Pos2, color: Color32, radius: f32) {
    let points: Vec<Pos2> = (0..6).map(|k| c + Vec2::angled(std::f32::consts::FRAC_PI_6 * (2 * k + 1) as f32) * radius).collect();
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
