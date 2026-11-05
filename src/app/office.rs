//! What the fishing (`super::fishing`) unlocks, apart from its avatar frame (`super::frames`):
//! - the Open space theme, for the whole binder: a cork board of post-its, a coffee machine steaming, a
//!   plant, a water cooler at the foot of the sidebar; a strip of post-its over the settings and over
//!   the active tab; « T'as 5 minutes ? » over a pane when its bell rings;
//! - the five-legged sheep, for the legendary card: now and then it walks along the foot of the
//!   sidebar, stops to graze, and bleats when hovered (switched off in the settings);
//! - the « Chasseur de têtes » badge, for 100 candidates hired (kept on floor): a small hook beside the
//!   avatar, wherever the others see it.

use super::*;

/// The post-its' colors.
const POSTITS: [Color32; 4] = [Color32::from_rgb(0xff, 0xe4, 0x5a), Color32::from_rgb(0xff, 0x9e, 0xc8), Color32::from_rgb(0x8a, 0xe0, 0xff), Color32::from_rgb(0xa8, 0xf0, 0x8a)];

/// The badge floor gives for 100 candidates hired.
pub(super) const HUNTER_BADGE: &str = "chasseur";

/// Whether the sheep is unlocked: the legendary card caught (all of it in development).
pub(super) fn sheep_unlocked(s: &crate::config::Settings) -> bool {
    let id = crate::fishing::FISH[crate::fishing::FISH.len() - 1].id;
    !crate::config::OFFICIAL || s.fish_cards.get(id).is_some_and(|c| c.count > 0)
}

fn postit(painter: &egui::Painter, c: Pos2, s: f32, tilt: f32, color: Color32) {
    let rot = |dx: f32, dy: f32| c + Vec2::new(dx * tilt.cos() - dy * tilt.sin(), dx * tilt.sin() + dy * tilt.cos()) * s;
    painter.add(egui::Shape::convex_polygon(vec![rot(-1.0, -1.0) + Vec2::new(1.0, 1.5), rot(1.0, -1.0) + Vec2::new(1.0, 1.5), rot(1.0, 1.0) + Vec2::new(1.0, 1.5), rot(-1.0, 1.0) + Vec2::new(1.0, 1.5)], Color32::from_black_alpha(30), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![rot(-1.0, -1.0), rot(1.0, -1.0), rot(1.0, 0.6), rot(0.6, 1.0), rot(-1.0, 1.0)], color, Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![rot(1.0, 0.6), rot(0.6, 0.6), rot(0.6, 1.0)], color.gamma_multiply(0.75), Stroke::NONE));
    // Two scribbled lines.
    for (k, w) in [(0.0, 1.2), (0.45, 0.8)] {
        painter.line_segment([rot(-0.7, -0.45 + k), rot(-0.7 + w, -0.45 + k)], Stroke::new(0.7, Color32::from_black_alpha(90)));
    }
}

/// The office at the foot of the sidebar, in `field`: a cork board of post-its (one flapping in the
/// air), a plant swaying, the coffee machine steaming, the water cooler bubbling. Seconds before
/// it is drawn again.
pub(super) fn scene(painter: &egui::Painter, field: Rect, theme: &Theme, now: f64) -> f32 {
    let tm = now as f32;
    let floor_y = field.max.y - 6.0;
    // The cork board.
    let board = Rect::from_min_size(Pos2::new(field.min.x + 14.0, field.min.y + 12.0), Vec2::new((field.width() * 0.42).min(110.0), 56.0));
    painter.rect_filled(board, 3.0, Color32::from_rgb(0xc8, 0x9a, 0x62));
    painter.rect_stroke(board, 3.0, Stroke::new(2.0, Color32::from_rgb(0x8a, 0x62, 0x3a)), egui::StrokeKind::Inside);
    for k in 0..5 {
        let x = board.min.x + 12.0 + (k as f32 * 21.0) % (board.width() - 18.0);
        let y = board.min.y + 14.0 + (k % 2) as f32 * 24.0;
        let flap = if k == 3 { (tm * 3.0).sin() * 0.25 } else { 0.0 };
        postit(painter, Pos2::new(x, y), 7.5, (k as f32 * 0.7).sin() * 0.15 + flap, POSTITS[k % 4]);
        painter.circle_filled(Pos2::new(x, y - 6.5), 1.3, theme.ansi[1]);
    }
    // The plant, in its pot.
    let pot = Pos2::new(field.min.x + 26.0, floor_y);
    painter.add(egui::Shape::convex_polygon(vec![pot + Vec2::new(-9.0, -14.0), pot + Vec2::new(9.0, -14.0), pot + Vec2::new(6.0, 0.0), pot + Vec2::new(-6.0, 0.0)], Color32::from_rgb(0xc0, 0x6a, 0x3a), Stroke::NONE));
    for k in 0..6 {
        let a = -std::f32::consts::FRAC_PI_2 + (k as f32 - 2.5) * 0.35 + (tm * 1.1 + k as f32).sin() * 0.06;
        let tip = pot + Vec2::new(0.0, -14.0) + Vec2::angled(a) * (16.0 + (k % 3) as f32 * 5.0);
        let base = pot + Vec2::new(0.0, -14.0);
        let side = Vec2::angled(a + std::f32::consts::FRAC_PI_2) * 2.6;
        let mid = base.lerp(tip, 0.5);
        painter.add(egui::Shape::convex_polygon(vec![base, mid + side, tip, mid - side], Color32::from_rgb(0x3f, 0x8f, 0x4a), Stroke::NONE));
    }
    // The coffee machine, steaming.
    let m = coffee_rect(field);
    painter.rect_filled(m, 3.0, Color32::from_rgb(0x3a, 0x3d, 0x46));
    painter.rect_filled(Rect::from_min_size(m.min + Vec2::new(4.0, 4.0), Vec2::new(m.width() - 8.0, 8.0)), 1.5, Color32::from_rgb(0x6f, 0xd0, 0xff).gamma_multiply(0.8));
    let cup = Rect::from_center_size(Pos2::new(m.center().x, m.max.y - 6.0), Vec2::new(8.0, 7.0));
    painter.rect_filled(cup, 1.5, Color32::WHITE);
    painter.circle_stroke(Pos2::new(cup.max.x + 1.5, cup.center().y), 2.0, Stroke::new(1.0, Color32::WHITE));
    for k in 0..3 {
        let phase = (tm * 0.6 + k as f32 / 3.0) % 1.0;
        let p = Pos2::new(cup.center().x + (tm * 2.0 + k as f32 * 2.0).sin() * 2.5, cup.min.y - 2.0 - phase * 18.0);
        painter.circle_filled(p, 2.0 + phase * 2.5, theme.text_muted.gamma_multiply(0.35 * (1.0 - phase)));
    }
    // The water cooler, bubbling.
    let wc = Pos2::new(field.max.x - 22.0, floor_y);
    painter.rect_filled(Rect::from_min_max(wc + Vec2::new(-9.0, -26.0), wc + Vec2::new(9.0, 0.0)), 2.0, Color32::from_rgb(0xd8, 0xdc, 0xe2));
    let bottle = Rect::from_min_max(wc + Vec2::new(-8.0, -48.0), wc + Vec2::new(8.0, -26.0));
    painter.rect_filled(bottle, 5.0, Color32::from_rgb(0x8a, 0xc8, 0xf0).gamma_multiply(0.75));
    for k in 0..3 {
        let phase = (tm * 0.45 + k as f32 / 3.0) % 1.0;
        painter.circle_stroke(Pos2::new(bottle.center().x + (k as f32 - 1.0) * 3.5, bottle.max.y - 3.0 - phase * (bottle.height() - 6.0)), 1.4, Stroke::new(0.8, Color32::WHITE.gamma_multiply(0.8)));
    }
    0.05
}

/// Where the coffee machine stands in the scene (it says something when hovered).
pub(super) fn coffee_rect(field: Rect) -> Rect {
    Rect::from_min_size(Pos2::new(field.center().x + 4.0, field.max.y - 6.0 - 34.0), Vec2::new(22.0, 34.0))
}

/// A strip of post-its along `r` (over the settings, over the active tab).
pub(super) fn strip(painter: &egui::Painter, r: Rect) {
    let n = ((r.width() / 14.0) as usize).max(1);
    for k in 0..n {
        let x = r.min.x + (k as f32 + 0.5) * r.width() / n as f32;
        let tilt = ((k * 37 % 11) as f32 - 5.0) * 0.03;
        let rot = |dx: f32, dy: f32| Pos2::new(x, r.center().y) + Vec2::new(dx * tilt.cos() - dy * tilt.sin(), dx * tilt.sin() + dy * tilt.cos());
        let half = Vec2::new(r.width() / n as f32 * 0.42, r.height() / 2.0);
        painter.add(egui::Shape::convex_polygon(vec![rot(-half.x, -half.y), rot(half.x, -half.y), rot(half.x, half.y), rot(-half.x, half.y)], POSTITS[k % 4], Stroke::NONE));
    }
}

/// « T'as 5 minutes ? » over the top of a pane whose bell rang, `k` from 1 (just rung) to 0.
pub(super) fn bubble(painter: &egui::Painter, pane: Rect, k: f32, text: &str, theme: &Theme) {
    let rise = (1.0 - k) * 6.0;
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(13.0), Color32::from_rgb(0x2e, 0x34, 0x40));
    let r = Rect::from_center_size(Pos2::new(pane.max.x - galley.size().x / 2.0 - 34.0, pane.min.y + 24.0 - rise), galley.size() + Vec2::new(22.0, 12.0));
    let alpha = (k * 3.0).min(1.0);
    let fill = POSTITS[0].gamma_multiply(alpha);
    painter.rect(r, 8.0, fill, Stroke::new(1.0, theme.text_muted.gamma_multiply(0.5 * alpha)), egui::StrokeKind::Inside);
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(r.max.x - 26.0, r.max.y), Pos2::new(r.max.x - 14.0, r.max.y), Pos2::new(r.max.x - 10.0, r.max.y + 8.0)], fill, Stroke::NONE));
    painter.galley(r.center() - galley.size() / 2.0, galley, Color32::from_rgb(0x2e, 0x34, 0x40).gamma_multiply(alpha));
}

/// The sheep's walk: every so often, along `strip` (the foot of the sidebar), from one side to the
/// other, a stop to graze halfway. Its rect when it shows (to hover), and when to draw again.
pub(super) fn sheep(painter: &egui::Painter, strip: Rect, now: f64) -> (Option<Rect>, f32) {
    // A walk of 16 s every 50 s.
    const EVERY: f64 = 50.0;
    const WALK: f64 = 16.0;
    let t = (now + 20.0) % EVERY;
    if t > WALK {
        return (None, (EVERY - t) as f32);
    }
    let lap = ((now + 20.0) / EVERY).floor() as i64;
    let rightward = lap % 2 == 0;
    // Walking 0-6 s, grazing 6-10 s, walking 10-16 s.
    let (along, grazing) = if t < 6.0 {
        (t / 6.0 * 0.5, false)
    } else if t < 10.0 {
        (0.5, true)
    } else {
        (0.5 + (t - 10.0) / 6.0 * 0.5, false)
    };
    let span = strip.width() + 60.0;
    let x = strip.min.x - 30.0 + span * along as f32;
    let x = if rightward { x } else { strip.max.x + strip.min.x - x };
    let c = Pos2::new(x, strip.max.y - 14.0);
    paint_sheep(painter, c, if rightward { 1.0 } else { -1.0 }, if grazing { None } else { Some(now as f32) });
    (Some(Rect::from_center_size(c, Vec2::new(34.0, 26.0))), 0.04)
}

/// The five-legged sheep, its middle at `c`, facing `dir` (1 right, -1 left); walking at `step`
/// (app time) or grazing.
pub(super) fn paint_sheep(painter: &egui::Painter, c: Pos2, dir: f32, step: Option<f32>) {
    let at = |x: f32, y: f32| c + Vec2::new(x * dir, y);
    let wool = Color32::from_rgb(0xfa, 0xf7, 0xee);
    let dark = Color32::from_rgb(0x35, 0x30, 0x30);
    let gold = Color32::from_rgb(0xf2, 0xc2, 0x3a);
    // Five legs, each its own beat.
    for k in 0..5 {
        let x = -9.0 + k as f32 * 4.5;
        let kick = step.map_or(0.0, |s| (s * 9.0 + k as f32 * 1.3).sin() * 2.2);
        painter.line_segment([at(x, 4.0), at(x + kick, 12.0)], Stroke::new(1.8, dark));
    }
    // The fleece, in puffs, a gold glint (it is legendary).
    for (x, y, r) in [(-8.0, -1.0, 6.0), (-2.0, -4.0, 6.5), (4.0, -1.5, 6.0), (-4.0, 3.0, 5.5), (3.0, 3.0, 5.5)] {
        painter.circle_filled(at(x, y), r, wool);
    }
    painter.circle_stroke(at(-2.0, -1.0), 10.5, Stroke::new(0.8, gold.gamma_multiply(0.6)));
    // The head: down to the grass when grazing.
    let head = if step.is_some() { at(12.0, -3.0) } else { at(12.0, 6.0) };
    painter.circle_filled(head, 4.2, dark);
    painter.circle_filled(head + Vec2::new(-2.5 * dir, -3.0), 1.8, dark);
    painter.circle_filled(head + Vec2::new(1.4 * dir, -0.8), 0.9, Color32::WHITE);
    if step.is_none() {
        painter.line_segment([head + Vec2::new(2.0 * dir, 4.0), head + Vec2::new(3.5 * dir, 1.0)], Stroke::new(1.2, Color32::from_rgb(0x4c, 0x9a, 0x48)));
    }
}

/// The « Chasseur de têtes » badge beside an avatar of radius `r` centered at `c`: a small round
/// medal with a hook, at its bottom left (the bottom right is the online dot's).
pub(super) fn paint_badge(painter: &egui::Painter, c: Pos2, r: f32, badges: &[String]) {
    if !badges.iter().any(|b| b == HUNTER_BADGE) {
        return;
    }
    let s = (r * 0.42).clamp(4.5, 12.0);
    let m = c + Vec2::new(-r * 0.78, r * 0.78);
    painter.circle_filled(m, s, Color32::from_rgb(0x2f, 0x6f, 0xde));
    painter.circle_stroke(m, s, Stroke::new((s * 0.16).max(0.8), Color32::from_rgb(0xf2, 0xc2, 0x3a)));
    // The hook.
    let w = (s * 0.17).max(0.9);
    let stroke = Stroke::new(w, Color32::WHITE);
    painter.line_segment([m + Vec2::new(0.15, -0.6) * s, m + Vec2::new(0.15, 0.25) * s], stroke);
    let curve: Vec<Pos2> = (0..=8).map(|k| m + Vec2::new(-0.15, 0.25) * s + Vec2::angled(k as f32 / 8.0 * std::f32::consts::PI) * 0.3 * s).collect();
    painter.add(egui::Shape::line(curve, stroke));
    painter.circle_filled(m + Vec2::new(0.15, -0.65) * s, w * 0.9, Color32::WHITE);
}

impl App {
    /// The player's own badges, as floor gives them for what was played here: the « chasseur » once
    /// 100 candidates hired (floor's own tally may lag a sync behind).
    pub(super) fn own_badges(&self) -> Vec<String> {
        let hires: u32 = self.config.settings.fish_cards.values().map(|c| c.count).sum();
        if hires >= crate::fishing::HUNTER { vec![HUNTER_BADGE.to_owned()] } else { Vec::new() }
    }
}
