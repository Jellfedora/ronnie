//! The Claude button of a local pane's strip: Claude Code started in the pane or, when it held
//! conversations in that folder already, the list of them, to take one up again or start a new one.

use std::sync::{Arc, Mutex};

use super::*;
use crate::claude::Session;

/// The orange of Claude's mark.
pub(super) const CLAUDE_ORANGE: Color32 = Color32::from_rgb(0xD9, 0x77, 0x57);

/// The conversations of a pane's folder, read in the background as their list opens.
pub(super) struct History {
    pub(super) pane: PaneId,
    sessions: Arc<Mutex<Option<Vec<Session>>>>,
    filter: String,
    focus: bool,
}

impl History {
    pub(super) fn open(pane: PaneId, dir: PathBuf, ctx: &egui::Context) -> Self {
        let sessions = Arc::new(Mutex::new(None));
        let (found, ctx) = (sessions.clone(), ctx.clone());
        std::thread::spawn(move || {
            let list = crate::claude::sessions(&dir);
            if let Ok(mut found) = found.lock() {
                *found = Some(list);
            }
            ctx.request_repaint();
        });
        History { pane, sessions, filter: String::new(), focus: true }
    }
}

/// What to type at the pane's prompt: Claude Code, taking up conversation `id` if any.
pub(super) fn command(id: Option<&str>) -> String {
    match id {
        Some(id) => format!("claude --resume {id}\r"),
        None => "claude\r".to_owned(),
    }
}

/// Claude's mark: rays of two lengths around a center.
pub(super) fn paint_mark(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    for k in 0..12 {
        let dir = Vec2::angled(k as f32 * std::f32::consts::TAU / 12.0 - std::f32::consts::FRAC_PI_2);
        let len = if k % 2 == 0 { r } else { r * 0.72 };
        painter.line_segment([c + dir * 1.2, c + dir * len], Stroke::new(1.7, color));
    }
}

/// The list under the button: a new conversation, then those held, the latest first, filtered by what is
/// typed (Enter: the first one left). What was picked: Some(None) for a new one.
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
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for session in shown {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(row, 4.0, theme.tab_hover);
                }
                let when = ui.painter().layout_no_wrap(git::ago(session.modified, t), FontId::proportional(11.5), theme.text_muted);
                let room = row.width() - when.size().x - 24.0;
                let mut job = egui::text::LayoutJob::simple_singleline(session.title.clone(), FontId::proportional(13.0), theme.text);
                job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(0.0));
                let title = ui.painter().layout_job(job);
                ui.painter().galley(Pos2::new(row.min.x + 8.0, row.center().y - title.size().y / 2.0), title, theme.text);
                ui.painter().galley(Pos2::new(row.max.x - 8.0 - when.size().x, row.center().y - when.size().y / 2.0), when, theme.text_muted);
                if resp.on_hover_text(&session.title).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(Some(session.id.clone()));
                    ui.close();
                }
            }
        });
    });
    picked
}
