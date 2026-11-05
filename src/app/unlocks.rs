//! What a game unlocks, on its page: a small pill in the bottom left corner, out of a game, that shows
//! on hover each thing to unlock, how, and whether it is (or how far the player is).

use super::four::HomeGame;
use super::*;

/// One thing a game unlocks: what, how, and done or how far (`progress` of `need`).
struct Unlock {
    what: String,
    how: String,
    done: bool,
    progress: Option<(u32, u32)>,
    /// Said for information (unlocked elsewhere on the page): not counted.
    info: bool,
}

impl App {
    fn unlocks(&self, game: HomeGame) -> Vec<Unlock> {
        let t = self.t();
        let s = &self.config.settings;
        match game {
            HomeGame::SpeedMetal => {
                let best = s.typing_scores.first().map_or(0, |s| s.letters);
                vec![Unlock { what: t.unlock_metal.into(), how: t.unlock_metal_how.replace("{n}", &crate::theme::METAL_UNLOCK.to_string()), done: best >= crate::theme::METAL_UNLOCK, progress: Some((best, crate::theme::METAL_UNLOCK)), info: false }]
            }
            HomeGame::Blob => vec![
                Unlock { what: t.unlock_jurassic.into(), how: t.unlock_jurassic_how.replace("{n}", &crate::theme::JURASSIC_UNLOCK.to_string()), done: s.jurassic_unlocked, progress: Some((s.blob_best, crate::theme::JURASSIC_UNLOCK)), info: false },
                Unlock { what: t.unlock_skins.into(), how: t.unlock_skins_how.to_owned(), done: false, progress: None, info: true },
            ],
            HomeGame::Mines => vec![Unlock { what: t.unlock_wizz.into(), how: t.unlock_wizz_how.to_owned(), done: s.mines_best > 0, progress: None, info: false }],
            HomeGame::Four => vec![Unlock { what: t.unlock_claude.into(), how: t.unlock_claude_how.to_owned(), done: s.four_online_played, progress: None, info: false }],
            HomeGame::Larsen => vec![Unlock { what: t.unlock_star_wars.into(), how: t.unlock_star_wars_how.to_owned(), done: s.hyperspace_unlocked, progress: None, info: false }],
            HomeGame::BugHunt => super::frames::FRAMES
                .iter()
                .map(|f| {
                    let what = t.unlock_frame.replace("{n}", super::frames::frame_name(t, f.id));
                    Unlock { what, how: t.frame_how.replace("{n}", &f.wave.to_string()), done: super::frames::unlocked(f, s.bughunt_best_wave), progress: Some((s.bughunt_best_wave, f.wave)), info: false }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The pill of what this game unlocks, in the bottom left corner of `rect` (nothing for a game
    /// that unlocks nothing); hovered, the list.
    pub(super) fn unlocks_pill(&mut self, ui: &mut Ui, rect: Rect, game: HomeGame) {
        let items = self.unlocks(game);
        if items.is_empty() {
            return;
        }
        let t = self.t();
        let theme = &self.theme;
        let left = items.iter().filter(|u| !u.done && !u.info).count();
        let label = if left == 0 { t.unlock_all_done.to_owned() } else { t.unlock_left.replace("{n}", &left.to_string()) };
        let font = FontId::proportional(12.0);
        let w = ui.painter().layout_no_wrap(label.clone(), font.clone(), theme.text).size().x + 40.0;
        let pill = Rect::from_min_size(Pos2::new(rect.min.x + 16.0, rect.max.y - 16.0 - 28.0), Vec2::new(w, 28.0));
        let resp = ui.interact(pill, ui.id().with(("unlocks", game as u8)), Sense::hover());
        let painter = ui.painter();
        painter.rect(pill, 14.0, if resp.hovered() { theme.tab_hover } else { theme.chrome_bg }, Stroke::new(1.0, if left == 0 { theme.ansi[2].gamma_multiply(0.6) } else { theme.accent.gamma_multiply(0.6) }), egui::StrokeKind::Inside);
        paint_padlock(painter, Pos2::new(pill.min.x + 15.0, pill.center().y), left == 0, if left == 0 { theme.ansi[2] } else { theme.accent });
        painter.text(Pos2::new(pill.min.x + 28.0, pill.center().y), Align2::LEFT_CENTER, label, font, theme.text);
        if !resp.hovered() {
            return;
        }
        // The list, over the pill.
        egui::Area::new(ui.id().with(("unlocks-list", game as u8))).order(egui::Order::Tooltip).fixed_pos(Pos2::new(pill.min.x, pill.min.y - 8.0)).pivot(Align2::LEFT_BOTTOM).show(ui.ctx(), |ui| {
            Frame::popup(ui.style()).fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_max_width(340.0);
                ui.label(egui::RichText::new(t.unlock_title).size(12.0).color(theme.text_muted));
                for u in &items {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (mark, color) = if u.done { ("✓", theme.ansi[2]) } else if u.info { ("•", theme.text_muted) } else { ("•", theme.accent) };
                        ui.label(egui::RichText::new(mark).size(13.5).strong().color(color));
                        ui.label(egui::RichText::new(&u.what).size(13.5).strong().color(theme.text));
                    });
                    let mut how = u.how.clone();
                    if let (Some((have, need)), false) = (u.progress, u.done) {
                        how = format!("{how} ({})", t.unlock_best.replace("{n}", &have.min(need).to_string()));
                    }
                    ui.add(egui::Label::new(egui::RichText::new(if u.done && u.progress.is_some() { t.unlock_done.to_owned() } else { how }).size(12.0).color(theme.text_muted)).wrap());
                }
            });
        });
    }
}

/// A padlock, closed or open.
pub(super) fn paint_padlock(painter: &egui::Painter, c: Pos2, open: bool, color: Color32) {
    let body = Rect::from_center_size(c + Vec2::new(0.0, 2.5), Vec2::new(10.0, 8.0));
    painter.rect_filled(body, 1.5, color);
    let lift = if open { -2.5 } else { 0.0 };
    let shackle: Vec<Pos2> = (0..=10).map(|k| {
        let a = std::f32::consts::PI * (1.0 + k as f32 / 10.0);
        c + Vec2::new(0.0, -1.5 + lift) + Vec2::angled(a) * 3.2
    }).collect();
    let (first, last) = (shackle[0], shackle[10]);
    painter.add(egui::Shape::line(shackle, Stroke::new(1.6, color)));
    painter.line_segment([first, first + Vec2::new(0.0, 2.0 - lift)], Stroke::new(1.6, color));
    if !open {
        painter.line_segment([last, last + Vec2::new(0.0, 2.0)], Stroke::new(1.6, color));
    }
}
