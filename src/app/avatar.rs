//! The account's avatar: its picture (round), else a round with the pseudo's first letter.

use super::*;

/// A colour of the theme, always the same for a pseudo.
fn tint(theme: &Theme, name: &str) -> Color32 {
    let h = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    [theme.ansi[1], theme.ansi[2], theme.ansi[3], theme.ansi[4], theme.ansi[5], theme.ansi[6]][h as usize % 6]
}

pub(super) fn paint_avatar(painter: &egui::Painter, c: Pos2, r: f32, name: &str, face: Option<&egui::TextureHandle>, theme: &Theme) {
    if let Some(face) = face {
        let rect = Rect::from_center_size(c, Vec2::splat(r * 2.0));
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.add(egui::epaint::RectShape::filled(rect, r, Color32::WHITE).with_texture(face.id(), uv));
        return;
    }
    let color = tint(theme, name);
    painter.circle_filled(c, r, color.gamma_multiply(0.85));
    let letter: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    painter.text(c, Align2::CENTER_CENTER, letter, FontId::proportional(r * 1.05), theme.bg);
}
