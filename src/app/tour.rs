//! The tour of the main features: shown once, at the first launch (after the splash), and again from
//! the settings. Each step lights up its part of a small drawing of the window.

use super::*;

/// What a step shows on the drawing of the window.
#[derive(Clone, Copy, PartialEq)]
enum Spot {
    Welcome,
    Sidebar,
    Split,
    Header,
    Remote,
    Claude,
    Settings,
}

/// One a step, in the order of `Strings::tour_steps`.
const SPOTS: [Spot; 7] = [Spot::Welcome, Spot::Sidebar, Spot::Split, Spot::Header, Spot::Remote, Spot::Claude, Spot::Settings];

const DRAWING: Vec2 = Vec2::new(440.0, 190.0);

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
        let shortcuts = &self.config.settings.shortcuts;
        let now = ctx.input(|i| i.time);
        if tour.since.is_nan() {
            tour.since = now;
        }
        let steps = t.tour_steps.len().min(SPOTS.len());
        let last = tour.step + 1 >= steps;
        let (left, right) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight) || i.consume_key(Modifiers::NONE, Key::Enter)));
        let keys = [
            ("{split_right}", &shortcuts.split_right),
            ("{split_down}", &shortcuts.split_down),
            ("{find}", &shortcuts.find_text),
            ("{sidebar}", &shortcuts.toggle_sidebar),
            ("{settings}", &shortcuts.open_settings),
        ];
        let (title, body) = t.tour_steps[tour.step];
        let body = keys.iter().fold(body.to_owned(), |text, (key, shortcut)| text.replace(key, &shortcut.label()));
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
            ui.add(egui::Label::new(egui::RichText::new(body).size(13.5).color(theme.text_muted)).wrap());
            ui.add_space(18.0);
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
        // Animated for a moment after each step's start.
        if age < 2.0 || spot == Spot::Welcome {
            ctx.request_repaint();
        }

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
            if !self.config.settings.tour_seen {
                self.config.settings.tour_seen = true;
                self.save_config();
            }
        }
    }
}

/// Where the parts of the drawing are.
struct Scene {
    side: Rect,
    /// The sidebar's groups: local tabs, SSH hosts, databases.
    groups: Vec<(&'static str, Rect)>,
    gear: Rect,
    panes: [Rect; 2],
    headers: Vec<Rect>,
    claude_badge: Rect,
}

/// The drawing of the window: the sidebar and two panes side by side, the step's part lit up (the rest
/// dimmed, then that part drawn again over it).
fn paint_window(painter: &egui::Painter, rect: Rect, theme: &Theme, spot: Spot, age: f32, t: &Strings) {
    let appear = (age / 0.35).min(1.0);
    let pulse = 0.55 + 0.45 * (age * 3.0).sin().abs();
    let Scene { side, groups, gear, panes, headers, claude_badge } = paint_scene(painter, rect, theme);
    // The step's part.
    let lit: Vec<Rect> = match spot {
        Spot::Welcome => Vec::new(),
        Spot::Sidebar => vec![side.shrink(2.0)],
        Spot::Split => panes.iter().map(|p| p.shrink(2.0)).collect(),
        Spot::Header => headers.iter().map(|h| Rect::from_min_max(Pos2::new(h.max.x - 56.0, h.min.y + 1.0), Pos2::new(h.max.x - 2.0, h.max.y - 1.0))).collect(),
        Spot::Remote => groups.iter().filter(|(l, _)| *l != "LOCAL").map(|(_, r)| *r).collect(),
        Spot::Claude => vec![claude_badge.expand(3.0)],
        Spot::Settings => vec![gear.expand(2.0)],
    };
    if !lit.is_empty() {
        // Everything else dimmed, the lit parts framed.
        painter.rect_filled(rect, 10.0, Color32::from_black_alpha((150.0 * appear) as u8));
        for r in &lit {
            paint_scene(&painter.with_clip_rect(*r), rect, theme);
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

fn paint_scene(painter: &egui::Painter, rect: Rect, theme: &Theme) -> Scene {
    painter.rect_filled(rect, 10.0, theme.bg);
    painter.rect_stroke(rect, 10.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
    let muted = theme.text_muted.gamma_multiply(0.5);
    let bar = |r: Rect, color: Color32| painter.rect_filled(r, 2.0, color);

    // The sidebar: local tabs, SSH hosts, databases, the settings at the bottom.
    let side = Rect::from_min_max(rect.min, Pos2::new(rect.min.x + 104.0, rect.max.y));
    painter.rect_filled(side, egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, theme.chrome_bg);
    let mut y = side.min.y + 14.0;
    let mut groups = Vec::new();
    for (label, rows) in [("LOCAL", 2), ("SSH", 2), ("SQL", 1)] {
        let top = y - 4.0;
        painter.text(Pos2::new(side.min.x + 12.0, y), Align2::LEFT_TOP, label, FontId::proportional(9.0), theme.text_muted);
        y += 16.0;
        for k in 0..rows {
            let row = Rect::from_min_size(Pos2::new(side.min.x + 10.0, y), Vec2::new(side.width() - 20.0, 12.0));
            if label == "LOCAL" && k == 0 {
                painter.rect_filled(row, 3.0, theme.tab_active);
                bar(Rect::from_min_size(row.min, Vec2::new(2.5, row.height())), theme.accent);
            }
            bar(Rect::from_min_size(row.min + Vec2::new(8.0, 4.0), Vec2::new(row.width() * if k == 0 { 0.6 } else { 0.45 }, 4.0)), muted);
            y += 16.0;
        }
        groups.push((label, Rect::from_min_max(Pos2::new(side.min.x + 4.0, top), Pos2::new(side.max.x - 4.0, y - 2.0))));
        y += 6.0;
    }
    let gear = Rect::from_center_size(Pos2::new(side.min.x + 20.0, side.max.y - 18.0), Vec2::splat(20.0));
    paint_gear(painter, gear.center(), theme.accent);

    // Two panes, each with its bar (icons on the right) and a few lines of output.
    let main = Rect::from_min_max(Pos2::new(side.max.x + 1.0, rect.min.y), rect.max);
    let half = main.width() / 2.0;
    let panes = [Rect::from_min_size(main.min, Vec2::new(half - 1.0, main.height())), Rect::from_min_max(Pos2::new(main.min.x + half + 1.0, main.min.y), main.max)];
    let mut headers = Vec::new();
    let mut claude_badge = Rect::NOTHING;
    for (k, pane) in panes.iter().enumerate() {
        let head = Rect::from_min_size(pane.min, Vec2::new(pane.width(), 18.0));
        painter.rect_filled(head, if k == 1 { egui::CornerRadius { ne: 10, ..Default::default() } } else { egui::CornerRadius::ZERO }, theme.chrome_bg);
        bar(Rect::from_min_size(head.min + Vec2::new(8.0, 7.0), Vec2::new(38.0, 4.0)), if k == 0 { theme.accent } else { theme.text_muted });
        for i in 0..4 {
            painter.circle_filled(Pos2::new(head.max.x - 10.0 - i as f32 * 12.0, head.center().y), 2.6, theme.text_muted);
        }
        if k == 1 {
            claude_badge = Rect::from_min_size(Pos2::new(head.max.x - 104.0, head.min.y + 4.0), Vec2::new(46.0, 10.0));
            painter.rect_stroke(claude_badge, 2.0, Stroke::new(1.0, theme.accent), egui::StrokeKind::Inside);
            bar(claude_badge.shrink2(Vec2::new(5.0, 3.5)), theme.accent.gamma_multiply(0.7));
        }
        headers.push(head);
        let colors = [theme.ansi[4], theme.fg, theme.ansi[2], theme.ansi[6], theme.fg, theme.ansi[3], theme.ansi[1]];
        for line in 0..7 {
            let w = [0.7, 0.45, 0.6, 0.35, 0.55, 0.4, 0.5][(line + k * 3) % 7] * (pane.width() - 20.0);
            bar(Rect::from_min_size(Pos2::new(pane.min.x + 10.0, head.max.y + 12.0 + line as f32 * 13.0), Vec2::new(w, 4.0)), colors[(line + k) % 7].gamma_multiply(0.75));
        }
    }
    painter.vline(main.min.x + half, main.y_range(), Stroke::new(2.0, theme.tab_hover));
    Scene { side, groups, gear, panes, headers, claude_badge }
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
