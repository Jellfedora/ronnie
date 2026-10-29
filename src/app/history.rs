//! The Claude button of a local pane's strip: Claude Code started or, when it held conversations in
//! that folder already, the list of them, to take one up again or start a new one. In the pane when its
//! shell waits at its prompt or Claude Code runs there (the pane is started again), else (another
//! program runs there) in a new pane beside it.

use std::sync::{Arc, Mutex};

use super::*;
use crate::claude::Session;

/// The conversations of a pane's folder, read in the background as their list opens.
pub(super) struct History {
    pub(super) pane: PaneId,
    dir: PathBuf,
    sessions: Arc<Mutex<Option<Vec<Session>>>>,
    filter: String,
    focus: bool,
    /// The conversation being renamed: its id, the name typed, and whether its field is to be focused.
    renaming: Option<(String, String, bool)>,
    /// The conversation whose deletion is to be confirmed.
    deleting: Option<String>,
    /// Why renaming or deleting failed.
    error: Option<String>,
}

impl History {
    pub(super) fn open(pane: PaneId, dir: PathBuf, ctx: &egui::Context) -> Self {
        let sessions = Arc::new(Mutex::new(None));
        let (found, ctx, folder) = (sessions.clone(), ctx.clone(), dir.clone());
        std::thread::spawn(move || {
            let list = crate::claude::sessions(&folder);
            if let Ok(mut found) = found.lock() {
                *found = Some(list);
            }
            ctx.request_repaint();
        });
        History { pane, dir, sessions, filter: String::new(), focus: true, renaming: None, deleting: None, error: None }
    }

    /// Conversation `id` named `title` (nothing typed: as it was).
    fn rename(&mut self, id: &str, title: &str) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        match crate::claude::rename_session(&self.dir, id, title) {
            Ok(()) => self.change(|list| list.iter_mut().filter(|s| s.id == id).for_each(|s| s.title = title.to_owned())),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn delete(&mut self, id: &str) {
        match crate::claude::delete_session(&self.dir, id) {
            Ok(()) => self.change(|list| list.retain(|s| s.id != id)),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn change(&mut self, f: impl FnOnce(&mut Vec<Session>)) {
        self.error = None;
        if let Some(list) = self.sessions.lock().ok().as_mut().and_then(|s| s.as_mut()) {
            f(list);
        }
    }
}

/// Height of a conversation in the list.
const ROW_H: f32 = 28.0;

/// What to type at the pane's prompt: Claude Code, taking up conversation `id` if any.
pub(super) fn command(id: Option<&str>) -> String {
    match id {
        Some(id) => format!("claude --resume {id}\r"),
        None => "claude\r".to_owned(),
    }
}

/// Claude's mark in `rect` (faded: `tint` less than white).
pub(super) fn paint_mark(ui: &Ui, rect: Rect, tint: Color32) {
    let key = egui::Id::new("claude-logo");
    let texture = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(key)).unwrap_or_else(|| {
        let texture = load_png(ui.ctx(), "claude-logo", include_bytes!("../../assets/icon/claude-logo.png"));
        ui.ctx().data_mut(|d| d.insert_temp(key, texture.clone()));
        texture
    });
    ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
}

/// The list under the button: a new conversation, then those held, the latest first, filtered by what is
/// typed (Enter: the first one left); hovered, one can be renamed or deleted. What was picked: Some(None)
/// for a new one.
pub(super) fn menu(resp: &egui::Response, history: &mut History, theme: &Theme, t: &Strings) -> Option<Option<String>> {
    let mut picked = None;
    egui::Popup::menu(resp).width(380.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let new = egui::Button::new(egui::RichText::new(format!("＋  {}", t.claude_history_new)).size(13.0)).frame_when_inactive(false).min_size(Vec2::new(ui.available_width(), 26.0));
        if ui.add(new).clicked() {
            picked = Some(None);
            ui.close();
        }
        ui.separator();
        let edit = ui.add(egui::TextEdit::singleline(&mut history.filter).hint_text(t.claude_history_search).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 5.0)));
        if std::mem::take(&mut history.focus) {
            edit.request_focus();
        }
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(4.0);
        let sessions = history.sessions.lock().ok().and_then(|s| s.clone());
        let Some(sessions) = sessions else {
            loading::inline(ui, theme, t.claude_history_loading);
            return;
        };
        let needle = history.filter.trim().to_lowercase();
        let shown: Vec<&Session> = sessions.iter().filter(|s| s.title.to_lowercase().contains(&needle)).collect();
        if shown.is_empty() {
            ui.label(egui::RichText::new(t.claude_history_none).size(12.5).color(theme.text_muted));
            return;
        }
        if enter {
            picked = Some(Some(shown[0].id.clone()));
            ui.close();
            return;
        }
        // Sized for the rows (the popup's first frame, while they were read, was small).
        let height = (shown.len() as f32 * (ROW_H + ui.spacing().item_spacing.y)).min(420.0);
        egui::ScrollArea::vertical().min_scrolled_height(height).max_height(height).show(ui, |ui| {
            for session in shown {
                if let Some(pick) = row(ui, history, session, theme, t) {
                    picked = Some(Some(pick));
                    ui.close();
                }
            }
        });
        if let Some(e) = &history.error {
            ui.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
        }
    });
    picked
}

/// One conversation of the list: its title and when, or its name being typed, or its deletion to
/// confirm. Its id, when clicked.
fn row(ui: &mut Ui, history: &mut History, session: &Session, theme: &Theme, t: &Strings) -> Option<String> {
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    let text_at = |galley: &std::sync::Arc<egui::Galley>| Pos2::new(row.min.x + 8.0, row.center().y - galley.size().y / 2.0);
    // Renamed: Enter (or a click elsewhere) keeps the name, Esc leaves it as it was.
    if let Some((_, name, fresh)) = history.renaming.as_mut().filter(|(id, ..)| *id == session.id) {
        let edit = ui.put(row.shrink2(Vec2::new(2.0, 2.0)), egui::TextEdit::singleline(name).font(FontId::proportional(13.0)).margin(Vec2::new(6.0, 4.0)));
        if std::mem::take(fresh) {
            edit.request_focus();
            select_all(ui, edit.id, name);
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            history.renaming = None;
        } else if edit.lost_focus() {
            let (id, name, _) = history.renaming.take().unwrap_or_default();
            history.rename(&id, &name);
        }
        return None;
    }
    // Deleted: asked first, in the row.
    if history.deleting.as_deref() == Some(session.id.as_str()) {
        ui.painter().rect_filled(row, 4.0, theme.ansi[1].gamma_multiply(0.15));
        let ask = ui.painter().layout_no_wrap(t.claude_history_delete_ask.to_owned(), FontId::proportional(13.0), theme.text);
        ui.painter().galley(text_at(&ask), ask, theme.text);
        let button = |text: &str| egui::Button::new(egui::RichText::new(text).size(12.0)).corner_radius(5.0);
        let delete = Rect::from_min_max(Pos2::new(row.max.x - 92.0, row.min.y + 3.0), Pos2::new(row.max.x - 4.0, row.max.y - 3.0));
        let cancel = Rect::from_min_max(Pos2::new(delete.min.x - 76.0, delete.min.y), Pos2::new(delete.min.x - 6.0, delete.max.y));
        if ui.put(delete, button(t.claude_history_delete).fill(theme.ansi[1])).clicked() {
            history.deleting = None;
            history.delete(&session.id);
        }
        if ui.put(cancel, button(t.cancel)).clicked() {
            history.deleting = None;
        }
        return None;
    }
    // Hovered: rename and delete in place of when.
    let hovered = ui.rect_contains_pointer(row);
    if hovered {
        ui.painter().rect_filled(row, 4.0, theme.tab_hover);
    }
    let mut room = row.width() - 16.0;
    if hovered {
        let icon = |k: f32, salt: &str, tip: &str, paint: fn(&egui::Painter, Pos2, Color32)| {
            let at = Rect::from_center_size(Pos2::new(row.max.x - 16.0 - k * 28.0, row.center().y), Vec2::new(24.0, 22.0));
            let resp = ui.interact(at, egui::Id::new(("claude-session", salt, &session.id)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(at, 5.0, theme.tab_active);
            }
            paint(ui.painter(), at.center(), if resp.hovered() { theme.text } else { theme.text_muted });
            resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        };
        if icon(0.0, "delete", t.claude_history_delete, notes::paint_trash_icon) {
            history.deleting = Some(session.id.clone());
        }
        if icon(1.0, "rename", t.claude_history_rename, paint_pencil) {
            history.renaming = Some((session.id.clone(), session.title.clone(), true));
        }
        room -= 64.0;
    } else {
        let when = ui.painter().layout_no_wrap(git::ago(session.modified, t), FontId::proportional(11.5), theme.text_muted);
        room -= when.size().x + 8.0;
        ui.painter().galley(Pos2::new(row.max.x - 8.0 - when.size().x, row.center().y - when.size().y / 2.0), when, theme.text_muted);
    }
    let mut job = egui::text::LayoutJob::simple_singleline(session.title.clone(), FontId::proportional(13.0), theme.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(0.0));
    let title = ui.painter().layout_job(job);
    ui.painter().galley(text_at(&title), title, theme.text);
    resp.on_hover_text(&session.title).on_hover_cursor(egui::CursorIcon::PointingHand).clicked().then(|| session.id.clone())
}
