//! The games' chat, when two players or more are there: the last messages above a one-line field
//! (Enter sends). Puissance 4 keeps it beside the grid; Ronnie.io shows the messages a moment, the
//! field when Enter opens it.

use super::*;

/// The longest message floor keeps.
const MAX: usize = 200;
/// Ronnie.io: a message shown this long (seconds), fading at the end.
const SHOWN: f32 = 12.0;

/// A message: who sent it (the player's own: `mine`), how long ago (None: always shown).
pub(super) struct Line<'a> {
    pub from: &'a str,
    pub text: &'a str,
    pub mine: bool,
    pub age: Option<f32>,
}

#[derive(Default)]
pub(super) struct Chat {
    draft: String,
    /// The field had the keyboard at the last frame: the game's keys are for it.
    pub typing: bool,
    /// The field takes the keyboard at the next frame.
    focus: bool,
}

impl Chat {
    pub fn open(&mut self) {
        self.focus = true;
    }

    /// The messages in `area`, the newest at the bottom, above the field (`field`: shown; `keep`: it
    /// keeps the keyboard once a message is sent). What the player sent.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(&mut self, ui: &mut Ui, area: Rect, lines: &[Line], field: bool, keep: bool, salt: &str, theme: &Theme, t: &Strings) -> Option<String> {
        let painter = ui.painter_at(area);
        let field = field || self.typing || self.focus;
        // A panel behind it while it's used (always, when the field stays).
        if field {
            painter.rect_filled(area, 10.0, theme.chrome_bg.gamma_multiply(0.82));
        }
        let pad = 10.0;
        let input = Rect::from_min_max(Pos2::new(area.min.x + pad, area.max.y - pad - 28.0), Pos2::new(area.max.x - pad, area.max.y - pad));
        let mut y = if field { input.min.y - 6.0 } else { area.max.y };
        let width = area.width() - pad * 2.0;
        for line in lines.iter().rev() {
            // Not used: only the recent ones, fading.
            let alpha = match line.age {
                Some(age) if !field => ((SHOWN - age) / 1.5).clamp(0.0, 1.0),
                _ => 1.0,
            };
            if alpha <= 0.0 {
                continue;
            }
            let font = FontId::proportional(12.5);
            let mut job = egui::text::LayoutJob::default();
            let name = if line.mine { t.game_you } else { line.from };
            let name_color = if line.mine { theme.accent } else { theme.text };
            job.append(&format!("{name}  "), 0.0, egui::TextFormat { font_id: font.clone(), color: name_color.gamma_multiply(alpha), ..Default::default() });
            job.append(line.text, 0.0, egui::TextFormat { font_id: font, color: theme.text_muted.gamma_multiply(alpha), ..Default::default() });
            job.wrap.max_width = width - if field { 0.0 } else { 16.0 };
            let galley = painter.layout_job(job);
            let h = galley.size().y;
            if y - h < area.min.y + pad {
                break;
            }
            y -= h;
            // Without the panel: each line on its own small one, readable over the game.
            if !field {
                let back = Rect::from_min_size(Pos2::new(area.min.x, y - 3.0), Vec2::new(galley.size().x + 16.0, h + 6.0));
                painter.rect_filled(back, 6.0, theme.chrome_bg.gamma_multiply(0.75 * alpha));
                painter.galley(Pos2::new(area.min.x + 8.0, y), galley, theme.text);
                y -= 10.0;
            } else {
                painter.galley(Pos2::new(area.min.x + pad, y), galley, theme.text);
                y -= 4.0;
            }
        }
        if !field {
            return None;
        }
        let edit = egui::TextEdit::singleline(&mut self.draft).hint_text(t.chat_hint).char_limit(MAX).id_salt(("game-chat", salt)).margin(Vec2::new(8.0, 5.0));
        let resp = ui.put(input, edit);
        if std::mem::take(&mut self.focus) {
            resp.request_focus();
        }
        let sent = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        self.typing = resp.has_focus();
        let text = self.draft.trim().to_owned();
        if !sent || text.is_empty() {
            return None;
        }
        self.draft.clear();
        if keep {
            self.focus = true;
            self.typing = true;
        }
        Some(text)
    }
}
