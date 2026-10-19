//! The tour of the main features: shown at the first launch (after the splash), once more when it is
//! redone (`VERSION`), and again from the settings. Each step lights up its part of a small drawing of
//! the window.

use super::*;
use crate::config::Category;

/// Raised when the tour is redone with new steps: those who saw the previous one see it once more.
pub(super) const VERSION: u32 = 2;

/// What a step shows on the drawing of the window.
#[derive(Clone, Copy, PartialEq)]
enum Spot {
    Welcome,
    Categories,
    Panel,
    Split,
    Header,
    Remote,
    Db,
    Claude,
    Leisure,
    Settings,
}

/// One a step, in the order of `Strings::tour_steps`.
const SPOTS: [Spot; 10] = [Spot::Welcome, Spot::Categories, Spot::Panel, Spot::Split, Spot::Header, Spot::Remote, Spot::Db, Spot::Claude, Spot::Leisure, Spot::Settings];

const DRAWING: Vec2 = Vec2::new(460.0, 214.0);

/// The tour under way: its step, and when that step began (for its animation).
pub(super) struct Tour {
    step: usize,
    since: f64,
}

impl Tour {
    pub(super) fn new() -> Self {
        Self { step: 0, since: f64::NAN }
    }
}

impl App {
    pub(super) fn tour_window(&mut self, ctx: &egui::Context) {
        let Some(tour) = &mut self.tour else { return };
        let t = self.config.settings.language.strings();
        let theme = self.theme.clone();
        let now = ctx.input(|i| i.time);
        if tour.since.is_nan() {
            tour.since = now;
        }
        let steps = t.tour_steps.len().min(SPOTS.len());
        let last = tour.step + 1 >= steps;
        let (left, right) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight) || i.consume_key(Modifiers::NONE, Key::Enter)));
        let (title, body) = t.tour_steps[tour.step];
        let spot = SPOTS[tour.step];
        let age = (now - tour.since) as f32;

        let (mut back, mut next, mut skip) = (left, right, false);
        let frame = Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).corner_radius(14.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let modal = egui::Modal::new(egui::Id::new("tour")).frame(frame).backdrop_color(Color32::from_black_alpha(170)).show(ctx, |ui| {
            ui.set_width(DRAWING.x);
            let (rect, _) = ui.allocate_exact_size(DRAWING, Sense::hover());
            paint_window(ui.painter(), rect, &theme, spot, age, t);
            ui.add_space(16.0);
            ui.label(egui::RichText::new(format!("{}  /  {}", tour.step + 1, steps)).size(11.5).color(theme.text_muted));
            ui.add_space(2.0);
            ui.label(egui::RichText::new(title).size(19.0).strong().color(theme.text));
            ui.add_space(6.0);
            // The same height for every step, so the buttons stay put.
            let text = ui.add(egui::Label::new(egui::RichText::new(body).size(13.5).color(theme.text_muted)).wrap());
            ui.add_space((54.0 - text.rect.height()).max(0.0) + 14.0);
            ui.horizontal(|ui| {
                // The steps, as dots.
                let (dots, _) = ui.allocate_exact_size(Vec2::new(steps as f32 * 14.0, 20.0), Sense::hover());
                for k in 0..steps {
                    let c = Pos2::new(dots.min.x + 5.0 + k as f32 * 14.0, dots.center().y);
                    let color = if k == tour.step { theme.accent } else { theme.tab_hover };
                    ui.painter().circle_filled(c, if k == tour.step { 4.0 } else { 3.0 }, color);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if last { t.tour_done } else { t.tour_next };
                    let primary = egui::Button::new(egui::RichText::new(label).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(96.0, 30.0));
                    if ui.add(primary).clicked() {
                        next = true;
                    }
                    if tour.step > 0 && ui.add(egui::Button::new(egui::RichText::new(t.tour_back).size(13.5)).corner_radius(6.0).min_size(Vec2::new(80.0, 30.0))).clicked() {
                        back = true;
                    }
                    if !last && ui.add(egui::Button::new(egui::RichText::new(t.tour_skip).size(13.5).color(theme.text_muted)).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0))).clicked() {
                        skip = true;
                    }
                });
            });
        });
        // The drawing moves on every step (categories in turn, a transfer under way…).
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        let mut done = skip || modal.should_close();
        if next {
            if last {
                done = true;
            } else {
                tour.step += 1;
                tour.since = now;
            }
        } else if back && tour.step > 0 {
            tour.step -= 1;
            tour.since = now;
        }
        if done {
            self.tour = None;
            self.focus_terminal = true;
            if self.config.settings.tour_version < VERSION {
                self.config.settings.tour_version = VERSION;
                self.save_config();
            }
        }
    }
}

impl App {
    /// Whether to turn on the shared memory of projects, asked once: yes or no answers it for good;
    /// closed, it is asked again at the next launch.
    pub(super) fn worklog_ask_window(&mut self, ctx: &egui::Context) {
        if !self.worklog_ask {
            return;
        }
        let t = self.config.settings.language.strings();
        let theme = self.theme.clone();
        let (mut yes, mut no) = (false, false);
        let frame = Frame::popup(&ctx.global_style()).inner_margin(22.0).fill(theme.chrome_bg).corner_radius(14.0).stroke(Stroke::new(1.0, theme.tab_hover));
        let modal = egui::Modal::new(egui::Id::new("worklog-ask")).frame(frame).backdrop_color(Color32::from_black_alpha(170)).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(egui::RichText::new(t.worklog_ask_title).size(19.0).strong().color(theme.text));
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(t.worklog_ask_body).size(13.5).color(theme.text_muted)).wrap());
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(t.worklog_ask_private).size(12.5).color(theme.text_muted.gamma_multiply(0.85))).wrap());
            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let primary = egui::Button::new(egui::RichText::new(t.worklog_ask_yes).size(13.5).color(Color32::WHITE)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0));
                if ui.add(primary).clicked() {
                    yes = true;
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.worklog_ask_no).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 30.0))).clicked() {
                    no = true;
                }
            });
        });
        if yes || no {
            let s = &mut self.config.settings;
            if yes {
                match crate::claude::set_hooks(s.claude_notify, s.claude_context, true) {
                    Ok(()) => s.claude_worklog = true,
                    Err(e) => self.error = Some(e),
                }
            }
            self.config.settings.claude_worklog_asked = true;
            self.save_config();
        }
        if yes || no || modal.should_close() {
            self.worklog_ask = false;
            self.focus_terminal = true;
        }
    }
}

/// Where the parts of the drawing are.
struct Scene {
    strip: Rect,
    /// The strip's icons.
    icons: Vec<(Category, Rect)>,
    panel: Rect,
    settings: Rect,
    panes: [Rect; 2],
    headers: [Rect; 2],
    claude_badge: Rect,
}

/// The category shown by the sidebar of the drawing: the one the step speaks of, each in turn on the
/// steps about them.
fn shown_category(spot: Spot, age: f32) -> Category {
    let turn = (age / 0.9) as usize;
    match spot {
        Spot::Categories => Category::ALL[turn % Category::ALL.len()],
        Spot::Remote => Category::Ssh,
        Spot::Db => Category::Db,
        Spot::Leisure => [Category::Notes, Category::Music, Category::Games][turn % 3],
        _ => Category::Local,
    }
}

/// The drawing of the window: the strip of categories, the panel beside it and two panes side by side,
/// the step's part lit up (the rest dimmed, then that part drawn again over it).
fn paint_window(painter: &egui::Painter, rect: Rect, theme: &Theme, spot: Spot, age: f32, t: &Strings) {
    let appear = (age / 0.35).min(1.0);
    let pulse = 0.55 + 0.45 * (age * 3.0).sin().abs();
    let Scene { strip, icons, panel, settings, panes, headers, claude_badge } = paint_scene(painter, rect, theme, spot, age, t);
    let icon = |category: Category| icons.iter().find(|(c, _)| *c == category).map_or(Rect::NOTHING, |(_, r)| *r);
    // The step's part.
    let lit: Vec<Rect> = match spot {
        Spot::Welcome => Vec::new(),
        Spot::Categories => vec![strip.shrink(2.0)],
        Spot::Panel => vec![Rect::from_min_max(panel.min, Pos2::new(panel.max.x, settings.min.y - 4.0)).shrink(2.0)],
        Spot::Split => panes.iter().map(|p| p.shrink(2.0)).collect(),
        Spot::Header => headers.iter().map(|h| Rect::from_min_max(Pos2::new(h.max.x - 64.0, h.min.y + 1.0), Pos2::new(h.max.x - 2.0, h.max.y - 1.0))).collect(),
        Spot::Remote => vec![icon(Category::Ssh), panes[1].shrink(2.0)],
        Spot::Db => vec![icon(Category::Db), panes[1].shrink(2.0)],
        Spot::Claude => vec![claude_badge.expand(3.0)],
        Spot::Leisure => vec![icon(Category::Notes), icon(Category::Music), icon(Category::Games)],
        Spot::Settings => vec![settings.expand(1.0)],
    };
    if !lit.is_empty() {
        // Everything else dimmed, the lit parts framed.
        painter.rect_filled(rect, 10.0, Color32::from_black_alpha((150.0 * appear) as u8));
        for r in &lit {
            paint_scene(&painter.with_clip_rect(*r), rect, theme, spot, age, t);
            painter.rect_filled(*r, 4.0, theme.accent.gamma_multiply(0.08 * appear));
            for k in 0..3 {
                let alpha = [0.9, 0.35, 0.15][k] * pulse * appear;
                painter.rect_stroke(r.expand(k as f32 * 1.5), 4.0 + k as f32, Stroke::new(1.2, theme.accent.gamma_multiply(alpha)), egui::StrokeKind::Outside);
            }
        }
        if spot == Spot::Split {
            // The bar dragged elsewhere: an arrow from the left pane's bar to the right one.
            let (from, to) = (headers[0].center() + Vec2::new(0.0, 30.0), headers[1].center() + Vec2::new(0.0, 30.0));
            let k = ((age * 0.8) % 1.0).min(1.0);
            painter.arrow(from, (to - from) * k, Stroke::new(2.0, theme.cursor));
        }
    } else {
        // Welcome: the name over the window.
        let veil = Color32::from_black_alpha(140);
        painter.rect_filled(rect, 10.0, veil);
        let size = 34.0 + 6.0 * (1.0 - appear);
        painter.text(rect.center() - Vec2::new(0.0, 6.0), Align2::CENTER_CENTER, "Ronnie", FontId::new(size, egui::FontFamily::Name("metal".into())), theme.accent.gamma_multiply(appear));
        painter.text(rect.center() + Vec2::new(0.0, 28.0), Align2::CENTER_CENTER, t.ronnie_motto, FontId::proportional(12.0), theme.text_muted.gamma_multiply(appear * pulse));
    }
}

fn paint_scene(painter: &egui::Painter, rect: Rect, theme: &Theme, spot: Spot, age: f32, t: &Strings) -> Scene {
    painter.rect_filled(rect, 10.0, theme.bg);
    painter.rect_stroke(rect, 10.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
    let muted = theme.text_muted.gamma_multiply(0.5);
    let bar = |r: Rect, color: Color32| painter.rect_filled(r, 2.0, color);
    let shown = shown_category(spot, age);

    // The sidebar: the strip of categories, a shade darker, and the panel of the one shown.
    let side = Rect::from_min_max(rect.min, Pos2::new(rect.min.x + 134.0, rect.max.y));
    painter.rect_filled(side, egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, theme.chrome_bg);
    let strip = Rect::from_min_max(side.min, Pos2::new(side.min.x + 30.0, side.max.y));
    painter.rect_filled(strip, egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, theme.bg.gamma_multiply(0.3));
    painter.vline(strip.max.x - 0.5, strip.y_range(), Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7)));
    let mut icons = Vec::new();
    for (k, &category) in Category::ALL.iter().enumerate() {
        let slot = Rect::from_center_size(Pos2::new(strip.center().x, strip.min.y + 16.0 + k as f32 * 21.0), Vec2::splat(20.0));
        let active = category == shown;
        if active {
            painter.rect_filled(slot, 5.0, theme.tab_active);
            painter.rect_filled(Rect::from_min_size(Pos2::new(strip.min.x + 1.5, slot.min.y + 5.0), Vec2::new(2.0, slot.height() - 10.0)), 1.0, theme.accent);
        }
        super::sidebar::paint_category_icon(painter, slot.center(), category, if active { theme.accent } else { theme.text_muted });
        icons.push((category, slot));
    }
    // The machine's resources at its bottom: the processor's curve, the memory and the disk.
    let gauges = Rect::from_min_max(Pos2::new(strip.min.x + 6.0, strip.max.y - 38.0), Pos2::new(strip.max.x - 6.0, strip.max.y - 8.0));
    painter.hline(gauges.x_range(), gauges.min.y - 4.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7)));
    let curve: Vec<Pos2> = (0..10).map(|i| Pos2::new(gauges.min.x + i as f32 * gauges.width() / 9.0, gauges.min.y + 10.0 - 8.0 * (0.5 + 0.5 * (i as f32 * 1.3 + age * 2.0).sin()))).collect();
    painter.add(egui::Shape::line(curve, Stroke::new(1.2, theme.accent)));
    for (k, (fill, color)) in [(0.55, theme.ansi[2]), (0.8, theme.ansi[3])].into_iter().enumerate() {
        let track = Rect::from_min_size(Pos2::new(gauges.min.x, gauges.min.y + 16.0 + k as f32 * 8.0), Vec2::new(gauges.width(), 3.0));
        painter.rect_filled(track, 1.5, theme.tab_hover);
        painter.rect_filled(Rect::from_min_size(track.min, Vec2::new(track.width() * fill, 3.0)), 1.5, color);
    }

    // The panel: the logo, the category's name, its rows and its groups, the settings at the bottom.
    let panel = Rect::from_min_max(Pos2::new(strip.max.x, side.min.y), side.max);
    painter.text(Pos2::new(panel.center().x, panel.min.y + 16.0), Align2::CENTER_CENTER, "Ronnie", FontId::new(15.0, egui::FontFamily::Name("metal".into())), theme.accent);
    let name = super::sidebar::category_name(t, shown).to_uppercase();
    painter.text(Pos2::new(panel.min.x + 10.0, panel.min.y + 34.0), Align2::LEFT_TOP, name, FontId::proportional(8.5), theme.text_muted);
    let mut y = panel.min.y + 50.0;
    for (k, w) in [0.6, 0.45, 0.5, 0.0, 0.55, 0.4].into_iter().enumerate() {
        if w == 0.0 {
            // A group: its name, then its rows.
            bar(Rect::from_min_size(Pos2::new(panel.min.x + 10.0, y + 4.0), Vec2::new(30.0, 3.0)), theme.text_muted.gamma_multiply(0.7));
            y += 14.0;
            continue;
        }
        let row = Rect::from_min_size(Pos2::new(panel.min.x + 8.0, y), Vec2::new(panel.width() - 16.0, 12.0));
        if k == 0 {
            painter.rect_filled(row, 3.0, theme.tab_active);
            bar(Rect::from_min_size(row.min, Vec2::new(2.5, row.height())), theme.accent);
        }
        bar(Rect::from_min_size(row.min + Vec2::new(8.0, 4.0), Vec2::new(row.width() * w, 4.0)), muted);
        y += 16.0;
    }
    let settings = Rect::from_min_size(Pos2::new(panel.min.x + 8.0, panel.max.y - 26.0), Vec2::new(panel.width() - 16.0, 18.0));
    painter.rect_filled(settings, 4.0, theme.tab_hover.gamma_multiply(0.7));
    paint_gear(painter, Pos2::new(settings.min.x + 11.0, settings.center().y), theme.accent);
    bar(Rect::from_min_size(Pos2::new(settings.min.x + 24.0, settings.center().y - 2.0), Vec2::new(40.0, 4.0)), theme.text_muted);

    // Two panes, each with its bar and a few lines of output: a local one where Claude runs, a server
    // with its health (its files, or a table of its database, when the step speaks of them).
    let main = Rect::from_min_max(Pos2::new(side.max.x + 1.0, rect.min.y), rect.max);
    let half = main.width() / 2.0;
    let panes = [Rect::from_min_size(main.min, Vec2::new(half - 1.0, main.height())), Rect::from_min_max(Pos2::new(main.min.x + half + 1.0, main.min.y), main.max)];
    let mut headers = [Rect::NOTHING; 2];
    let mut claude_badge = Rect::NOTHING;
    for (k, pane) in panes.iter().enumerate() {
        let head = Rect::from_min_size(pane.min, Vec2::new(pane.width(), 18.0));
        painter.rect_filled(head, if k == 1 { egui::CornerRadius { ne: 10, ..Default::default() } } else { egui::CornerRadius::ZERO }, theme.chrome_bg);
        bar(Rect::from_min_size(head.min + Vec2::new(8.0, 7.0), Vec2::new(30.0, 4.0)), if k == 0 { theme.accent } else { theme.text_muted });
        // On the right: git's branch, then the bar's buttons.
        super::git::paint_branch_icon(painter, Rect::from_center_size(Pos2::new(head.max.x - 52.0, head.center().y), Vec2::splat(11.0)), theme.ansi[2]);
        for i in 0..3 {
            painter.circle_filled(Pos2::new(head.max.x - 10.0 - i as f32 * 11.0, head.center().y), 2.4, theme.text_muted);
        }
        if k == 0 {
            claude_badge = Rect::from_min_size(Pos2::new(head.min.x + 44.0, head.min.y + 4.0), Vec2::new(40.0, 10.0));
            painter.rect_stroke(claude_badge, 2.0, Stroke::new(1.0, theme.accent), egui::StrokeKind::Inside);
            bar(claude_badge.shrink2(Vec2::new(5.0, 3.5)), theme.accent.gamma_multiply(0.7));
        } else {
            // The server's health: processor, memory, disk.
            for (i, (fill, color)) in [(0.35 + 0.25 * (age * 1.7).sin().abs(), theme.ansi[2]), (0.6, theme.ansi[2]), (0.84, theme.ansi[3])].into_iter().enumerate() {
                let track = Rect::from_min_size(Pos2::new(head.min.x + 44.0 + i as f32 * 14.0, head.center().y - 1.5), Vec2::new(11.0, 3.0));
                painter.rect_filled(track, 1.5, theme.tab_hover);
                painter.rect_filled(Rect::from_min_size(track.min, Vec2::new(track.width() * fill, 3.0)), 1.5, color);
            }
        }
        headers[k] = head;
        let body = Rect::from_min_max(Pos2::new(pane.min.x + 10.0, head.max.y + 10.0), Pos2::new(pane.max.x - 10.0, pane.max.y - 10.0));
        match (k, spot) {
            (1, Spot::Remote) => paint_files(painter, body, theme, age),
            (1, Spot::Db) => paint_table(painter, body, theme),
            _ => {
                let colors = [theme.ansi[4], theme.fg, theme.ansi[2], theme.ansi[6], theme.fg, theme.ansi[3], theme.ansi[1], theme.fg, theme.ansi[2], theme.fg, theme.ansi[5], theme.fg, theme.ansi[4]];
                for line in 0..13 {
                    let w = [0.7, 0.45, 0.6, 0.35, 0.55, 0.4, 0.5][(line + k * 3) % 7] * body.width();
                    bar(Rect::from_min_size(Pos2::new(body.min.x, body.min.y + line as f32 * 13.0), Vec2::new(w, 4.0)), colors[(line + k) % colors.len()].gamma_multiply(0.75));
                }
            }
        }
    }
    painter.vline(main.min.x + half, main.y_range(), Stroke::new(2.0, theme.tab_hover));
    Scene { strip, icons, panel, settings, panes, headers, claude_badge }
}

/// The server's files: folders, then files, and a transfer under way at the bottom.
fn paint_files(painter: &egui::Painter, body: Rect, theme: &Theme, age: f32) {
    for (k, w) in [0.4, 0.55, 0.3, 0.5, 0.45, 0.35, 0.6].into_iter().enumerate() {
        let y = body.min.y + 4.0 + k as f32 * 18.0;
        let c = Pos2::new(body.min.x + 6.0, y + 2.0);
        if k < 3 {
            super::notes::paint_folder_icon(painter, c, theme.accent);
        } else {
            painter.rect_stroke(Rect::from_center_size(c, Vec2::new(8.0, 10.0)), 1.5, Stroke::new(1.1, theme.text_muted), egui::StrokeKind::Middle);
        }
        painter.rect_filled(Rect::from_min_size(Pos2::new(body.min.x + 18.0, y), Vec2::new((body.width() - 18.0) * w, 4.0)), 2.0, theme.fg.gamma_multiply(0.6));
    }
    let track = Rect::from_min_size(Pos2::new(body.min.x, body.max.y - 6.0), Vec2::new(body.width(), 4.0));
    painter.rect_filled(track, 2.0, theme.tab_hover);
    painter.rect_filled(Rect::from_min_size(track.min, Vec2::new(track.width() * ((age * 0.35) % 1.0), 4.0)), 2.0, theme.accent);
}

/// A table of the database: its columns' names, then the rows.
fn paint_table(painter: &egui::Painter, body: Rect, theme: &Theme) {
    let row_h = 17.0;
    let cols = 3;
    let col_w = body.width() / cols as f32;
    for row in 0..9 {
        let y = body.min.y + row as f32 * row_h;
        if row == 0 {
            painter.rect_filled(Rect::from_min_size(Pos2::new(body.min.x, y), Vec2::new(body.width(), row_h)), 2.0, theme.chrome_bg);
        }
        for col in 0..cols {
            let w = [0.6, 0.8, 0.45, 0.7, 0.5][(row + col * 2) % 5] * (col_w - 10.0);
            let color = if row == 0 { theme.accent } else if col == 0 { theme.ansi[3] } else { theme.fg.gamma_multiply(0.6) };
            painter.rect_filled(Rect::from_min_size(Pos2::new(body.min.x + 5.0 + col as f32 * col_w, y + row_h / 2.0 - 2.0), Vec2::new(w, 4.0)), 2.0, color);
        }
        painter.hline(body.x_range(), y + row_h, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.6)));
    }
    for col in 1..cols {
        painter.vline(body.min.x + col as f32 * col_w, body.min.y..=body.min.y + 9.0 * row_h, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.6)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spot_for_each_step() {
        for lang in Lang::ALL {
            assert_eq!(lang.strings().tour_steps.len(), SPOTS.len());
        }
    }
}
