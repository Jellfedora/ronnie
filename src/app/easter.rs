//! "ronnie" typed in a terminal: the command never reaches the shell, the pane puts on a show
//! (lightning, flames, the name headbanging, the sign of the horns) and the video opens in the browser.

use super::*;

pub(super) const VIDEO: &str = "https://www.youtube.com/watch?v=7Wq9U3ypcDY";
/// Length of the show, in seconds.
const LENGTH: f32 = 8.0;

/// Whether `line` (what is typed at the prompt) calls the show.
pub(super) fn called(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("ronnie")
}

/// A number from 0 to 1, always the same for `seed`.
fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x % 10_000) as f32 / 10_000.0
}

/// A lightning bolt from the top of `rect`, zigzagging down; `seed` picks its path.
fn paint_bolt(painter: &egui::Painter, rect: Rect, seed: u32, alpha: f32, theme: &Theme) {
    let mut x = rect.min.x + rect.width() * (0.1 + 0.8 * noise(seed));
    let mut y = rect.min.y;
    let mut points = vec![Pos2::new(x, y)];
    let steps = 7 + (noise(seed + 1) * 5.0) as u32;
    for k in 0..steps {
        x += (noise(seed + 10 + k) - 0.5) * 70.0;
        y += rect.height() * 0.55 / steps as f32;
        points.push(Pos2::new(x, y));
    }
    painter.add(egui::Shape::line(points.clone(), Stroke::new(9.0, theme.ansi[4].gamma_multiply(0.25 * alpha))));
    painter.add(egui::Shape::line(points, Stroke::new(2.5, Color32::WHITE.gamma_multiply(alpha))));
}

/// Flames along the bottom of `rect`, licking up.
fn paint_flames(painter: &egui::Painter, rect: Rect, time: f32, alpha: f32, theme: &Theme) {
    let n = ((rect.width() / 14.0) as u32).max(8);
    for k in 0..n {
        let x = rect.min.x + (k as f32 + 0.5) * rect.width() / n as f32;
        let flicker = (time * (7.0 + noise(k) * 5.0) + noise(k + 99) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let h = rect.height() * (0.10 + 0.14 * noise(k + 7) + 0.08 * flicker);
        let w = rect.width() / n as f32 * 1.6;
        // Three layers, from red at the back to yellow at the heart.
        for (layer, color, scale) in [(0, theme.ansi[1], 1.0), (1, Color32::from_rgb(0xff, 0x8c, 0x1a), 0.7), (2, theme.ansi[3], 0.42)] {
            let top = Pos2::new(x + (flicker - 0.5) * 8.0 * (layer + 1) as f32, rect.max.y - h * scale);
            let points = vec![Pos2::new(x - w * scale / 2.0, rect.max.y), top, Pos2::new(x + w * scale / 2.0, rect.max.y)];
            painter.add(egui::Shape::convex_polygon(points, color.gamma_multiply(0.75 * alpha), Stroke::NONE));
        }
    }
}

/// The show over `rect`, `time` seconds in; true once it is over (or dismissed with a click or Escape).
pub(super) fn show(ui: &Ui, rect: Rect, theme: &Theme, t: &Strings, time: f32, hand: &egui::TextureHandle) -> bool {
    let dismissed = time > 0.4 && ui.input(|i| i.key_pressed(Key::Escape) || (i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| rect.contains(p))));
    if time >= LENGTH || dismissed {
        return true;
    }
    ui.ctx().request_repaint();
    let painter = ui.painter_at(rect);
    // In, and out at the end.
    let alpha = (time / 0.3).min(1.0) * ((LENGTH - time) / 0.8).min(1.0);
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha((225.0 * alpha) as u8));
    // The opening strike: a white flash.
    if time < 0.35 {
        painter.rect_filled(rect, 0.0, Color32::WHITE.gamma_multiply(0.7 * (1.0 - time / 0.35)));
    }
    // Lightning now and then (a new one every sixth of a second, not always).
    let tick = (time * 6.0) as u32;
    if noise(tick * 3) > 0.55 {
        paint_bolt(&painter, rect, tick * 17, alpha * (1.0 - (time * 6.0).fract()), theme);
        painter.rect_filled(rect, 0.0, theme.ansi[4].gamma_multiply(0.05 * alpha));
    }
    paint_flames(&painter, rect, time, alpha, theme);

    // The name, headbanging to the beat (about 2 beats a second).
    let size = (rect.width() / 6.0).clamp(40.0, 120.0);
    let beat = (time * std::f32::consts::TAU * 2.0).sin();
    let nod = beat.max(0.0).powi(3);
    let center = rect.center() - Vec2::new(0.0, size * 0.35) + Vec2::new(0.0, nod * size * 0.12);
    let drop = |i: usize| ((time - 0.25 - i as f32 * 0.09) / 0.35).clamp(0.0, 1.0);
    let font = FontId::new(size, egui::FontFamily::Name("metal".into()));
    let widths: Vec<f32> = "Ronnie".chars().map(|c| painter.layout_no_wrap(c.to_string(), font.clone(), Color32::WHITE).size().x).collect();
    let mut x = center.x - widths.iter().sum::<f32>() / 2.0;
    for (i, c) in "Ronnie".chars().enumerate() {
        let p = drop(i);
        if p > 0.0 {
            let y = center.y - (1.0 - p).powi(2) * size * 1.5;
            paint_metal(&painter, Pos2::new(x, y), Align2::LEFT_CENTER, &c.to_string(), size, theme.accent, p * alpha);
        }
        x += widths[i];
    }
    // The horns on both sides, pumping.
    let horns = ((time - 1.0) / 0.4).clamp(0.0, 1.0) * alpha;
    if horns > 0.0 {
        let side = size * (0.9 + 0.12 * nod);
        let span = widths.iter().sum::<f32>() / 2.0 + side * 0.7;
        for dx in [-span, span] {
            let at = Rect::from_center_size(Pos2::new(center.x + dx, center.y - nod * 10.0), Vec2::splat(side));
            egui::Image::new(hand).tint(Color32::WHITE.gamma_multiply(horns)).paint_at(ui, at);
        }
    }
    // The motto, then how to leave.
    let motto = ((time - 1.4) / 0.5).clamp(0.0, 1.0) * alpha;
    painter.text(center + Vec2::new(0.0, size * 0.85), Align2::CENTER_CENTER, t.ronnie_motto, FontId::proportional((size * 0.22).max(15.0)), theme.ansi[3].gamma_multiply(motto));
    painter.text(Pos2::new(rect.center().x, rect.min.y + 22.0), Align2::CENTER_CENTER, t.ronnie_hint, FontId::proportional(12.0), Color32::WHITE.gamma_multiply(0.6 * motto));
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ronnie_calls_the_show() {
        assert!(called("ronnie"));
        assert!(called("  Ronnie "));
        assert!(!called("ronnie --version"));
        assert!(!called("ls ronnie"));
        assert!(!called(""));
    }
}
