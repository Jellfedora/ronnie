//! A segmented control: the choices side by side in one framed bar, thin lines between them, the
//! chosen one filled, instead of a row of separate pills or selectable labels.

use egui::{Rect, Sense, Stroke, Ui, Vec2};

use crate::theme::Theme;

/// The usual height of a segmented control, beside buttons and labels.
pub const HEIGHT: f32 = 24.0;

/// Draws the choices (`chosen`: the one filled, if any) and gives the one clicked. The padding shrinks
/// when the bar would be wider than the room left; a disabled `ui` dims it and takes no click.
pub fn segmented<S: AsRef<str>>(ui: &mut Ui, theme: &Theme, height: f32, labels: &[S], chosen: Option<usize>) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }
    let enabled = ui.is_enabled();
    let font = egui::FontId::proportional(12.0);
    let galleys: Vec<_> = labels.iter().map(|l| ui.painter().layout_no_wrap(l.as_ref().to_owned(), font.clone(), theme.text)).collect();
    let text: f32 = galleys.iter().map(|g| g.size().x).sum();
    let padding = ((ui.available_width() - text) / labels.len() as f32).clamp(10.0, 20.0);
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + padding).collect();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(widths.iter().sum(), height), Sense::hover());
    let radius = 7.0;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, radius, theme.bg.gamma_multiply(0.5));
    let mut clicked = None;
    let mut x = rect.left();
    for (i, (galley, w)) in galleys.into_iter().zip(&widths).enumerate() {
        let seg = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(*w, height));
        let mut resp = ui.interact(seg, ui.id().with(("segmented", i)), Sense::click());
        if enabled {
            resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        let on = chosen == Some(i);
        if on {
            painter.rect_filled(seg.shrink(2.0), radius - 2.0, theme.tab_active);
        } else if resp.hovered() && enabled {
            painter.rect_filled(seg.shrink(2.0), radius - 2.0, theme.tab_hover);
        }
        if i > 0 && !on && chosen != Some(i - 1) {
            painter.vline(x, seg.y_range().shrink(6.0), Stroke::new(1.0, theme.tab_hover));
        }
        let mut color = if on { theme.text } else { theme.text_muted };
        if !enabled {
            color = color.gamma_multiply(0.5);
        }
        painter.galley_with_override_text_color(seg.center() - galley.size() / 2.0, galley, color);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += w;
    }
    painter.rect_stroke(rect, radius, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
    clicked
}
