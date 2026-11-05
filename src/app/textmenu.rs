//! The right-click menu of a text field: cut, copy, paste, select all, as the keyboard does.

use egui::text::{CCursor, CCursorRange};
use egui::{Id, Response};

use crate::i18n::Strings;

/// What the menu did.
#[derive(Default, PartialEq)]
pub(super) enum TextMenu {
    #[default]
    None,
    /// The text changed (cut, pasted).
    Changed,
    /// "Paste" with no text on the clipboard: for the field to take what is there (an image...).
    PasteOther,
}

/// The menu on `resp`, the TextEdit `id` editing `text` (`editable`: cut and paste offered).
pub(super) fn text_menu(resp: &Response, id: Id, text: &mut String, editable: bool, t: &Strings) -> TextMenu {
    let mut out = TextMenu::None;
    // A right click puts the text field's cursor where it is (egui does so for any button), which
    // would lose the selection to copy: the one before it is kept, and put back.
    let ctx = resp.ctx.clone();
    let (kept_id, open_id) = (id.with("menu-selection"), id.with("menu-open"));
    let was_open = ctx.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let right_click = resp.hovered() && ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
    let kept = ctx.data(|d| d.get_temp::<CCursorRange>(kept_id));
    if right_click {
        if let Some(range) = kept.filter(|r| r.primary.index != r.secondary.index)
            && let Some(mut state) = egui::TextEdit::load_state(&ctx, id)
        {
            state.cursor.set_char_range(Some(range));
            state.store(&ctx, id);
        }
    } else if !was_open && let Some(range) = egui::TextEdit::load_state(&ctx, id).and_then(|s| s.cursor.char_range()) {
        ctx.data_mut(|d| d.insert_temp(kept_id, range));
    }
    let mut open = false;
    resp.context_menu(|ui| {
        open = true;
        ui.set_min_width(180.0);
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
        let (a, b) = state.cursor.char_range().map_or((0, 0), |r| {
            let (x, y) = (r.primary.index.0, r.secondary.index.0);
            (x.min(y), x.max(y))
        });
        let count = text.chars().count();
        let (a, b) = (a.min(count), b.min(count));
        let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(i, _)| i);
        let (start, end) = (byte(a), byte(b));
        let selected = start < end;
        let key = |k: &str| format!("⌘ {k}");
        let item = |ui: &mut egui::Ui, label: &str, shortcut: &str, enabled: bool| ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(if cfg!(target_os = "macos") { key(shortcut) } else { format!("Ctrl {shortcut}") })).clicked();
        let mut cursor = None;
        if item(ui, t.cut, "X", editable && selected) {
            ctx.copy_text(text[start..end].to_owned());
            text.replace_range(start..end, "");
            cursor = Some(a);
            out = TextMenu::Changed;
            ui.close();
        }
        if item(ui, t.copy, "C", selected) {
            ctx.copy_text(text[start..end].to_owned());
            ui.close();
        }
        if item(ui, t.paste, "V", editable) {
            match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                Ok(pasted) if !pasted.is_empty() => {
                    let pasted = pasted.replace("\r\n", "\n");
                    text.replace_range(start..end, &pasted);
                    cursor = Some(a + pasted.chars().count());
                    out = TextMenu::Changed;
                }
                _ => {
                    cursor = Some(b);
                    out = TextMenu::PasteOther;
                }
            }
            ui.close();
        }
        ui.separator();
        let mut range = cursor.map(|c| CCursorRange::one(CCursor::new(c)));
        if item(ui, t.select_all, "A", count > 0) {
            range = Some(CCursorRange::two(CCursor::new(0), CCursor::new(count)));
            ui.close();
        }
        if let Some(range) = range {
            state.cursor.set_char_range(Some(range));
            state.store(&ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
        }
    });
    ctx.data_mut(|d| d.insert_temp(open_id, open));
    out
}
