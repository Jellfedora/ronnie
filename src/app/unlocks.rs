//! What a game unlocks, on its page: a small pill in the bottom left corner, out of a game, that shows
//! on hover each thing to unlock, how, and whether it is (or how far the player is).

use super::four::HomeGame;
use super::*;

/// One thing a game unlocks: what, how, and done or how far (`progress` of `need`).
pub(super) struct Unlock {
    pub(super) what: String,
    pub(super) how: String,
    pub(super) done: bool,
    pub(super) progress: Option<(u32, u32)>,
    /// Said for information (unlocked elsewhere on the page): not counted.
    pub(super) info: bool,
    /// What switches it on, once unlocked (None: on by itself).
    pub(super) reward: Option<Reward>,
}

/// How an unlocked thing is put to use.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Reward {
    Theme(&'static str),
    Trail,
    Sheep,
    /// An avatar frame (its id), worn from the account.
    Frame(&'static str),
    /// A badge beside the avatar (its id), worn by itself.
    Badge(&'static str),
    /// Lundi matin's horde at 10, over the whole screen.
    Horde,
    /// Gérard strolling along the foot of the screen.
    Gerard,
}

impl App {
    pub(super) fn unlocks(&self, game: HomeGame) -> Vec<Unlock> {
        let mut list = self.unlocks_won(game);
        // In development, everything is won (to try it all).
        if !crate::config::OFFICIAL {
            for u in list.iter_mut().filter(|u| !u.info) {
                u.done = true;
            }
        }
        list
    }

    fn unlocks_won(&self, game: HomeGame) -> Vec<Unlock> {
        let t = self.t();
        let s = &self.config.settings;
        match game {
            HomeGame::SpeedMetal => {
                let best = s.typing_scores.first().map_or(0, |s| s.letters);
                vec![Unlock { what: t.unlock_metal.into(), how: t.unlock_metal_how.replace("{n}", &crate::theme::METAL_UNLOCK.to_string()), done: best >= crate::theme::METAL_UNLOCK, progress: Some((best, crate::theme::METAL_UNLOCK)), info: false, reward: Some(Reward::Theme(crate::theme::METAL_THEME)) }]
            }
            HomeGame::Blob => vec![
                Unlock { what: t.unlock_jurassic.into(), how: t.unlock_jurassic_how.replace("{n}", &crate::theme::JURASSIC_UNLOCK.to_string()), done: s.jurassic_unlocked, progress: Some((s.blob_best, crate::theme::JURASSIC_UNLOCK)), info: false, reward: Some(Reward::Theme(crate::theme::JURASSIC_THEME)) },
                Unlock { what: t.unlock_skins.into(), how: t.unlock_skins_how.to_owned(), done: false, progress: None, info: true, reward: None },
            ],
            HomeGame::Mines => vec![Unlock { what: t.unlock_wizz.into(), how: t.unlock_wizz_how.to_owned(), done: s.mines_best > 0, progress: None, info: false, reward: None }],
            HomeGame::Four => vec![Unlock { what: t.unlock_claude.into(), how: t.unlock_claude_how.to_owned(), done: s.four_online_played, progress: None, info: false, reward: None }],
            HomeGame::Backstage => {
                let need = super::backstage::PIG_UNLOCK;
                vec![Unlock { what: t.unlock_pig.into(), how: t.unlock_pig_how.replace("{n}", &need.to_string()), done: s.backstage_best >= need, progress: Some((s.backstage_best, need)), info: false, reward: None }]
            }
            HomeGame::Coffee => {
                let need = crate::coffee::THRONES_UNLOCK as u32;
                let done = s.coffee_bests.iter().filter(|&&b| b > 0).count() as u32;
                vec![Unlock { what: t.unlock_thrones.into(), how: t.unlock_thrones_how.replace("{n}", &need.to_string()), done: !crate::config::OFFICIAL || done >= need, progress: Some((done.min(need), need)), info: false, reward: Some(Reward::Theme(crate::theme::THRONES_THEME)) }]
            }
            HomeGame::Larsen => vec![Unlock { what: t.unlock_star_wars.into(), how: t.unlock_star_wars_how.to_owned(), done: s.hyperspace_unlocked, progress: None, info: false, reward: Some(Reward::Theme(crate::theme::HYPERSPACE_THEME)) }],
            HomeGame::Riff => {
                let best = s.riff_bests.iter().copied().max().unwrap_or(0);
                let need = crate::theme::OFFICE_UNLOCK;
                vec![Unlock { what: t.unlock_office.into(), how: t.unlock_office_how.replace("{n}", &need.to_string()), done: !crate::config::OFFICIAL || best >= need, progress: Some((best, need)), info: false, reward: Some(Reward::Theme(crate::theme::OFFICE_THEME)) }]
            }
            HomeGame::Runner => {
                let need = super::trail::TRAIL_METERS;
                vec![Unlock { what: t.unlock_trail.into(), how: t.unlock_trail_how.replace("{n}", &need.to_string()), done: super::trail::unlocked(s.runner_best), progress: Some((s.runner_best, need)), info: false, reward: Some(Reward::Trail) }]
            }
            HomeGame::BugHunt | HomeGame::Fishing => {
                let frames = super::frames::FRAMES.iter().filter(|f| if game == HomeGame::BugHunt { matches!(f.need, super::frames::Need::Wave(_)) } else { matches!(f.need, super::frames::Need::Cards(_)) }).map(|f| {
                    let what = t.unlock_frame.replace("{n}", super::frames::frame_name(t, f.id));
                    Unlock { what, how: super::frames::how(f, t), done: super::frames::unlocked(f, s), progress: Some(super::frames::progress(f, s)), info: false, reward: Some(Reward::Frame(f.id)) }
                });
                if game == HomeGame::BugHunt {
                    return frames.collect();
                }
                // The fishing: the sheep, the badge, then the frame.
                let hires: u32 = s.fish_cards.values().map(|c| c.count).sum();
                let sheep = super::office::sheep_unlocked(s);
                let mut list = vec![
                    Unlock { what: t.unlock_sheep.into(), how: t.unlock_sheep_how.to_owned(), done: sheep, progress: None, info: false, reward: Some(Reward::Sheep) },
                    Unlock { what: t.unlock_hunter.into(), how: t.unlock_hunter_how.replace("{n}", &crate::fishing::HUNTER.to_string()), done: hires >= crate::fishing::HUNTER, progress: Some((hires, crate::fishing::HUNTER)), info: false, reward: Some(Reward::Badge(super::office::HUNTER_BADGE)) },
                ];
                list.extend(frames);
                list
            }
            // Lundi matin: the bite in the chat, the letters, Gérard on the screen, Agnès' card at the
            // fishing, and a mystery (what it gives shows once won).
            HomeGame::Monday => {
                use crate::zombie::{ESCORTS_CARD, LETTERS, REVIVES_BITE};
                let horde = if s.monday_horde { t.unlock_horde } else { t.unlock_mystery };
                vec![
                    Unlock { what: t.unlock_bite.into(), how: t.unlock_bite_how.replace("{n}", &REVIVES_BITE.to_string()), done: s.monday_revives >= REVIVES_BITE, progress: Some((s.monday_revives.min(REVIVES_BITE), REVIVES_BITE)), info: false, reward: None },
                    Unlock { what: t.unlock_letters.into(), how: t.unlock_letters_how.replace("{n}", &LETTERS.to_string()), done: s.monday_letters >= LETTERS, progress: Some((s.monday_letters.min(LETTERS), LETTERS)), info: false, reward: None },
                    Unlock { what: t.unlock_gerard.into(), how: t.unlock_gerard_how.to_owned(), done: s.monday_gerard, progress: None, info: false, reward: Some(Reward::Gerard) },
                    Unlock { what: t.unlock_agnes_card.into(), how: t.unlock_agnes_card_how.replace("{n}", &ESCORTS_CARD.to_string()), done: s.monday_escorts >= ESCORTS_CARD, progress: Some((s.monday_escorts.min(ESCORTS_CARD), ESCORTS_CARD)), info: false, reward: None },
                    Unlock { what: horde.into(), how: t.unlock_horde_how.to_owned(), done: s.monday_horde, progress: None, info: false, reward: s.monday_horde.then_some(Reward::Horde) },
                    Unlock { what: t.unlock_monday_theme.into(), how: t.unlock_monday_theme_how.replace("{n}", &crate::zombie::LAST_WAVE.to_string()), done: s.monday_wins > 0, progress: None, info: false, reward: Some(Reward::Theme(crate::theme::MONDAY_THEME)) },
                    Unlock { what: t.unlock_monday_frame.into(), how: t.unlock_monday_frame_how.replace("{n}", &crate::zombie::DAILIES_FRAME.to_string()), done: s.monday_dailies >= crate::zombie::DAILIES_FRAME, progress: Some((s.monday_dailies.min(crate::zombie::DAILIES_FRAME), crate::zombie::DAILIES_FRAME)), info: false, reward: Some(Reward::Frame("survivor")) },
                ]
            }
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
                self.unlocks_list_ui(ui, &items);
            });
        });
    }

    /// What a game unlocks still and in all (None: nothing).
    pub(super) fn unlocks_count(&self, game: HomeGame) -> Option<(usize, usize)> {
        let items: Vec<Unlock> = self.unlocks(game).into_iter().filter(|u| !u.info).collect();
        (!items.is_empty()).then(|| (items.iter().filter(|u| !u.done).count(), items.len()))
    }

    /// The list of what a game unlocks, for a tooltip (nothing for a game that unlocks nothing).
    pub(super) fn unlocks_ui(&self, ui: &mut Ui, game: HomeGame) {
        let items = self.unlocks(game);
        if !items.is_empty() {
            self.unlocks_list_ui(ui, &items);
        }
    }

    fn unlocks_list_ui(&self, ui: &mut Ui, items: &[Unlock]) {
        let t = self.t();
        let theme = &self.theme;
                ui.label(egui::RichText::new(t.unlock_title).size(12.0).color(theme.text_muted));
                for u in items {
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
