//! Waiting screens, all alike: an amp's level meter bouncing on a soft glow, what is awaited and a
//! hint below. Redrawn only while shown.

use super::*;

/// How often a waiting screen is redrawn (smooth, and only while it is there).
const FRAME: Duration = Duration::from_millis(16);

/// Bars of a level meter at `time`: each between 0.15 and 1, bouncing at its own pace.
fn levels(time: f64, n: usize) -> Vec<f32> {
    (0..n)
        .map(|k| {
            let k = k as f64;
            let wave = (time * (3.1 + 0.7 * k) + k * 1.3).sin() * 0.5 + 0.5;
            let beat = (time * 2.2 + k * 0.4).sin().max(0.0).powi(3);
            (0.15 + 0.6 * wave + 0.25 * beat).min(1.0) as f32
        })
        .collect()
}

/// A level meter of `n` bars in `rect`, from the bottom.
fn paint_meter(painter: &egui::Painter, rect: Rect, n: usize, time: f64, theme: &Theme) {
    let gap = rect.width() / (n as f32 * 3.0 - 1.0);
    let bar_w = gap * 2.0;
    for (k, level) in levels(time, n).into_iter().enumerate() {
        let x = rect.min.x + k as f32 * (bar_w + gap);
        let h = (rect.height() * level).max(bar_w);
        let bar = Rect::from_min_max(Pos2::new(x, rect.max.y - h), Pos2::new(x + bar_w, rect.max.y));
        // Hotter toward the top, like a meter in the red.
        let color = theme.accent.lerp_to_gamma(theme.ansi[3], (level - 0.5).max(0.0));
        painter.rect_filled(bar, bar_w / 2.0, color);
    }
}

/// A whole area waiting: the meter, `title` (with animated dots), `detail` under it, `hint` lower.
pub(super) fn screen(ui: &Ui, rect: Rect, theme: &Theme, title: &str, detail: Option<&str>, hint: Option<&str>) {
    let painter = ui.painter_at(rect);
    let time = ui.input(|i| i.time);
    let center = rect.center() - Vec2::new(0.0, 30.0);
    // A glow breathing behind the meter.
    let breath = ((time * 1.6).sin() * 0.5 + 0.5) as f32;
    for k in 0..8 {
        painter.circle_filled(center, 26.0 + k as f32 * 9.0 * (0.9 + 0.1 * breath), theme.accent.gamma_multiply(0.014 + 0.006 * breath));
    }
    paint_meter(&painter, Rect::from_center_size(center, Vec2::new(46.0, 38.0)), 5, time, theme);
    let dots = ".".repeat(1 + (time * 2.5) as usize % 3);
    let title_at = center + Vec2::new(0.0, 56.0);
    // The dots don't move the title: drawn after it.
    let galley = painter.layout_no_wrap(title.to_owned(), FontId::proportional(16.0), theme.text);
    let left = title_at.x - galley.size().x / 2.0;
    painter.galley(Pos2::new(left, title_at.y - galley.size().y / 2.0), galley.clone(), theme.text);
    painter.text(Pos2::new(left + galley.size().x, title_at.y), Align2::LEFT_CENTER, dots, FontId::proportional(16.0), theme.text);
    let mut y = title_at.y + 24.0;
    if let Some(detail) = detail {
        painter.text(Pos2::new(title_at.x, y), Align2::CENTER_CENTER, detail, FontId::monospace(12.5), theme.text_muted);
        y += 34.0;
    }
    if let Some(hint) = hint {
        let galley = painter.layout(hint.to_owned(), FontId::proportional(12.0), theme.text_muted.gamma_multiply(0.8), (rect.width() - 40.0).max(100.0));
        painter.galley(Pos2::new(title_at.x - galley.size().x / 2.0, y - 6.0), galley, theme.text_muted);
    }
    ui.ctx().request_repaint_after(FRAME);
}

/// A line waiting, in a list: a small meter and `text`.
pub(super) fn inline(ui: &mut Ui, theme: &Theme, text: &str) {
    let time = ui.input(|i| i.time);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 13.0), Sense::hover());
        paint_meter(ui.painter(), rect, 3, time, theme);
        ui.label(egui::RichText::new(text).size(12.5).color(theme.text_muted));
    });
    ui.ctx().request_repaint_after(FRAME);
}
