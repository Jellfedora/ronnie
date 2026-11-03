//! The chat's emojis on screen (see `crate::emoji`): a text with its emojis in colour (big when alone),
//! the picker (search, categories, recent ones, skin tones), the field's emojis drawn in colour over
//! egui's black and white ones, and the `:name` completion.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::emoji::{self, Piece};
use crate::i18n::Strings;
use crate::theme::Theme;

/// The reactions offered at once, on a message's bar.
pub(super) const QUICK: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🎉"];
/// The recent emojis kept.
pub(super) const RECENT_MAX: usize = 32;

/// A text with its emojis in colour: as one label when it has none, big when it is only 1 to 3 emojis.
pub(super) fn emoji_label(ui: &mut Ui, sheet: Option<&egui::TextureHandle>, text: &str, size: f32, color: Color32) -> egui::Response {
    let Some(sheet) = sheet.filter(|_| emoji::has_emoji(text)) else {
        return ui.add(egui::Label::new(egui::RichText::new(text).size(size).color(color)).wrap().selectable(true));
    };
    let pieces = emoji::pieces(text);
    if let Some(n) = emoji::only_emojis(text) {
        let side = if n == 1 { 46.0 } else { 38.0 };
        return ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for piece in &pieces {
                    if let Piece::Emoji(chars, cell) = piece {
                        let name = emoji::find(chars).map(|e| e.name.clone()).unwrap_or_default();
                        ui.add(egui::Image::new((sheet.id(), Vec2::splat(side))).uv(emoji::uv(*cell))).on_hover_text(name);
                    }
                }
            })
            .response;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
        for piece in &pieces {
            match piece {
                Piece::Text(t) => {
                    for (k, line) in t.split('\n').enumerate() {
                        if k > 0 {
                            ui.end_row();
                        }
                        if !line.is_empty() {
                            ui.add(egui::Label::new(egui::RichText::new(line).size(size).color(color)).wrap().selectable(true));
                        }
                    }
                }
                Piece::Emoji(chars, cell) => {
                    let name = emoji::find(chars).map(|e| e.name.clone()).unwrap_or_default();
                    ui.add_space(1.0);
                    ui.add(egui::Image::new((sheet.id(), Vec2::splat((size * 1.3).round()))).uv(emoji::uv(*cell))).on_hover_text(name);
                    ui.add_space(1.0);
                }
            }
        }
    })
    .response
}

/// The picker's state: the search typed, the category shown (0: the recent ones).
#[derive(Default)]
pub(super) struct Picker {
    pub search: String,
    pub category: usize,
    pub focus: bool,
}

/// The tabs: the recent ones, then the categories (an emoji standing for each).
const TABS: [&str; 10] = ["🕘", "😀", "👋", "🐶", "🍔", "⚽", "🚗", "💡", "❤️", "🏁"];
/// The skin tones' colours (none: the yellow of the emojis).
const SKINS: [u32; 6] = [0xffcc4d, 0xf7dece, 0xf3d2a2, 0xd5ab88, 0xaf7e57, 0x7c533e];

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// The picker: the emoji clicked, if one (with the skin tone chosen, when it has tones).
pub(super) fn picker(ui: &mut Ui, state: &mut Picker, sheet: &egui::TextureHandle, recent: &[String], skin: &mut u8, t: &Strings, theme: &Theme) -> Option<String> {
    const COLS: usize = 9;
    const CELL: f32 = 36.0;
    let width = CELL * COLS as f32 + 8.0;
    ui.set_width(width);
    let mut chosen = None;
    // The search, and the skin tones.
    ui.horizontal(|ui| {
        let search = egui::TextEdit::singleline(&mut state.search).hint_text(t.emoji_search).desired_width(width - 6.0 * 20.0 - 12.0).margin(Vec2::new(8.0, 5.0));
        let resp = ui.add(search);
        if std::mem::take(&mut state.focus) {
            resp.request_focus();
        }
        for (tone, color) in SKINS.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
            let on = usize::from(*skin) == tone;
            ui.painter().circle_filled(rect.center(), if on { 7.5 } else { 6.0 }, rgb(*color));
            if on {
                ui.painter().circle_stroke(rect.center(), 9.0, Stroke::new(1.5, theme.accent));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.emoji_skin).clicked() {
                *skin = tone as u8;
            }
        }
    });
    ui.add_space(6.0);
    // The tabs (hidden while searching).
    let searching = !state.search.trim().is_empty();
    if !searching {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (k, tab) in TABS.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new((width - 18.0) / TABS.len() as f32, 28.0), Sense::click());
                let on = state.category == k;
                if on || resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, if on { theme.tab_hover } else { theme.tab_hover.gamma_multiply(0.5) });
                }
                if let Some(cell) = emoji::cell(tab) {
                    let side = 18.0;
                    let r = Rect::from_center_size(rect.center(), Vec2::splat(side));
                    ui.painter().image(sheet.id(), r, emoji::uv(cell), if on { Color32::WHITE } else { Color32::from_white_alpha(170) });
                }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.emoji_categories[k]).clicked() {
                    state.category = k;
                }
            }
        });
        ui.add_space(4.0);
    }
    // The emojis shown: those found, the recent ones, or a category's.
    let shown: Vec<(String, (u16, u16), String)> = if searching {
        emoji::search(&state.search, 300).into_iter().map(|e| toned(e, *skin)).collect()
    } else if state.category == 0 {
        let list: Vec<String> = if recent.is_empty() { emoji::POPULAR.iter().map(|s| (*s).to_owned()).collect() } else { recent.to_vec() };
        list.iter().filter_map(|c| Some((c.clone(), emoji::cell(c)?, emoji::find(c).map(|e| e.name.clone()).unwrap_or_default()))).collect()
    } else {
        emoji::all().iter().filter(|e| usize::from(e.category) == state.category - 1).map(|e| toned(e, *skin)).collect()
    };
    egui::ScrollArea::vertical().id_salt(("emoji-grid", state.category, searching)).max_height(250.0).min_scrolled_height(250.0).auto_shrink([false, false]).show(ui, |ui| {
        if shown.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(t.emoji_none).size(12.5).color(theme.text_muted)));
            return;
        }
        for row in shown.chunks(COLS) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (chars, cell, name) in row {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CELL), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect.shrink(1.0), 7.0, theme.tab_hover);
                    }
                    ui.painter().image(sheet.id(), Rect::from_center_size(rect.center(), Vec2::splat(26.0)), emoji::uv(*cell), Color32::WHITE);
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(name).clicked() {
                        chosen = Some(chars.clone());
                    }
                }
            });
        }
    });
    chosen
}

/// An emoji with this skin tone (when it has tones), its cell and its name.
fn toned(e: &emoji::Emoji, skin: u8) -> (String, (u16, u16), String) {
    let (chars, cell) = e.toned(skin);
    (chars.to_owned(), cell, e.name.clone())
}

/// The recent emojis with this one first.
pub(super) fn remember(recent: &mut Vec<String>, chars: &str) {
    recent.retain(|c| c != chars);
    recent.insert(0, chars.to_owned());
    recent.truncate(RECENT_MAX);
}

/// The field's emojis drawn in colour over egui's black and white glyphs (`bg`: the field's colour).
pub(super) fn paint_field_emojis(painter: &egui::Painter, output: &egui::text_edit::TextEditOutput, text: &str, sheet: &egui::TextureHandle, bg: Color32) {
    if !emoji::has_emoji(text) {
        return;
    }
    let galley = &output.galley;
    let origin = output.galley_pos;
    let mut chars_before = 0;
    for piece in emoji::pieces(text) {
        match piece {
            Piece::Text(t) => chars_before += t.chars().count(),
            Piece::Emoji(chars, cell) => {
                let n = chars.chars().count();
                let a = galley.pos_from_cursor(egui::text::CCursor::new(chars_before));
                let b = galley.pos_from_cursor(egui::text::CCursor::new(chars_before + n));
                chars_before += n;
                // On one row only (a glyph is never split).
                if (a.min.y - b.min.y).abs() > 1.0 || b.min.x <= a.min.x {
                    continue;
                }
                let glyph = Rect::from_min_max(origin + a.min.to_vec2(), Pos2::new(origin.x + b.min.x, origin.y + a.max.y));
                painter.rect_filled(glyph, 0.0, bg);
                let side = glyph.height().min(20.0);
                painter.image(sheet.id(), Rect::from_center_size(glyph.center(), Vec2::splat(side)), emoji::uv(cell), Color32::WHITE);
            }
        }
    }
}

/// The `:name` being typed before the cursor (`cursor`: a char index): where its colon is (a char
/// index) and what follows it; None when there's none (at least 2 letters, after a space or at the start).
pub(super) fn completion(text: &str, cursor: usize) -> Option<(usize, String)> {
    let before: Vec<char> = text.chars().take(cursor).collect();
    let mut k = before.len();
    while k > 0 && (before[k - 1].is_alphanumeric() || matches!(before[k - 1], '_' | '+' | '-')) {
        k -= 1;
    }
    if k == 0 || before[k - 1] != ':' || before.len() - k < 2 {
        return None;
    }
    let colon = k - 1;
    if colon > 0 && !before[colon - 1].is_whitespace() {
        return None;
    }
    Some((colon, before[k..].iter().collect()))
}

/// `text` with the chars from `start` to `end` (char indexes) replaced: the new text, and the char
/// index right after what was put.
pub(super) fn replaced(text: &str, start: usize, end: usize, with: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut out: String = chars[..start.min(chars.len())].iter().collect();
    out.push_str(with);
    let at = out.chars().count();
    out.extend(&chars[end.min(chars.len())..]);
    (out, at)
}

/// The completion's list, above the field: each emoji, its `:name:`, its French name; the one chosen
/// lit. Returns the one clicked.
pub(super) fn completion_list(ui: &Ui, at: Pos2, found: &[&emoji::Emoji], lit: usize, skin: u8, sheet: &egui::TextureHandle, theme: &Theme) -> Option<usize> {
    let mut clicked = None;
    egui::Area::new(egui::Id::new("emoji-completion")).order(egui::Order::Foreground).fixed_pos(at).pivot(Align2::LEFT_BOTTOM).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).inner_margin(4.0).show(ui, |ui| {
            ui.set_width(320.0);
            for (k, e) in found.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(312.0, 30.0), Sense::click());
                if k == lit || resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, theme.tab_hover);
                }
                let (_, cell) = e.toned(skin);
                ui.painter().image(sheet.id(), Rect::from_center_size(Pos2::new(rect.min.x + 18.0, rect.center().y), Vec2::splat(22.0)), emoji::uv(cell), Color32::WHITE);
                let short = e.names.first().map(|n| format!(":{n}:")).unwrap_or_default();
                ui.painter().text(Pos2::new(rect.min.x + 38.0, rect.center().y), Align2::LEFT_CENTER, &short, FontId::monospace(12.5), theme.text);
                ui.painter().text(Pos2::new(rect.max.x - 8.0, rect.center().y), Align2::RIGHT_CENTER, &e.name, FontId::proportional(12.0), theme.text_muted);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    clicked = Some(k);
                }
            }
        });
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_name_being_typed() {
        assert_eq!(completion("salut :pou", 10), Some((6, "pou".into())));
        assert_eq!(completion(":tada", 5), Some((0, "tada".into())));
        assert_eq!(completion("salut :p", 8), None, "one letter is not enough");
        assert_eq!(completion("http://x", 8), None, "not after a word");
        assert_eq!(completion("12:30", 5), None);
        assert_eq!(completion("un :café", 8), Some((3, "café".into())));
        assert_eq!(completion("un :pouce et", 12), None, "the cursor is after a space");
    }

    #[test]
    fn replaces_the_name_with_the_emoji() {
        assert_eq!(replaced("salut :pou", 6, 10, "👍"), ("salut 👍".into(), 7));
        assert_eq!(replaced("a :tada b", 2, 7, "🎉"), ("a 🎉 b".into(), 3));
        let mut recent = vec!["😂".to_owned(), "👍".to_owned()];
        remember(&mut recent, "👍");
        assert_eq!(recent, ["👍", "😂"]);
    }
}
