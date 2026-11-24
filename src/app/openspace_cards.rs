//! Open Space, the cards and the missions, on screen (their rules in `crate::openspace_cards`): a card
//! drawn by hand (its rarity's frame, its picture, what it gives, its level), the « Cards » panel (the
//! raid deck's slots, the action cards to put in it, the troop cards), the mission under way (on the
//! left, an arrow on what to do), and the cards just found, turned over one after the other.

use super::openspace_scene as scene;
use super::*;
use crate::openspace::{self as os, tx, OpenSpace, Res};
use crate::openspace_battle::Troop;
use crate::openspace_cards::{card_bonus, card_level, is_action, Action, Card, Effect, Mission, Rarity, CARDS, COPIES, MISSIONS};

pub(super) fn rarity_color(r: Rarity) -> Color32 {
    match r {
        Rarity::Common => Color32::from_rgb(170, 175, 185),
        Rarity::Uncommon => Color32::from_rgb(80, 200, 120),
        Rarity::Rare => Color32::from_rgb(80, 150, 245),
        Rarity::Epic => Color32::from_rgb(180, 100, 240),
        Rarity::Legendary => Color32::from_rgb(255, 185, 45),
    }
}

pub(super) fn action_color(a: Action) -> Color32 {
    match a {
        Action::Reinforce => Color32::from_rgb(110, 200, 120),
        Action::Hotfix => Color32::from_rgb(255, 170, 40),
        Action::Standup => Color32::from_rgb(90, 220, 150),
        Action::Rush => Color32::from_rgb(220, 140, 80),
        Action::Freeze => Color32::from_rgb(130, 205, 255),
        Action::Friday => Color32::from_rgb(255, 80, 60),
    }
}

/// An action card's name, short (under its picture in a raid).
pub(super) fn action_short(a: Action, fr: bool) -> &'static str {
    tx(
        match a {
            Action::Reinforce => ("Renfort", "Backup"),
            Action::Hotfix => ("Hotfix", "Hotfix"),
            Action::Standup => ("Stand-up", "Stand-up"),
            Action::Rush => ("Café serré", "Espresso"),
            Action::Freeze => ("Gel", "Freeze"),
            Action::Friday => ("Prod ven.", "Fri. deploy"),
        },
        fr,
    )
}

pub(super) fn action_card(a: Action) -> &'static Card {
    CARDS.iter().find(|c| c.effect == Effect::Action(a)).unwrap_or(&CARDS[0])
}

/// An action's picture, `size` wide around `c`.
pub(super) fn paint_action_icon(p: &egui::Painter, a: Action, c: Pos2, size: f32, time: f64) {
    let s = size / 2.0;
    let col = action_color(a);
    match a {
        Action::Reinforce => {
            for (dx, k) in [(-0.45, 0.8), (0.45, 0.8), (0.0, 1.0)] {
                scene::paint_troop(p, Troop::Intern, c + Vec2::new(dx * s, s * 0.8), size / 30.0 * k, time as f32, false, false, (1.0, 0.3));
            }
        }
        Action::Hotfix => {
            let pts = vec![c + Vec2::new(0.25, -1.0) * s, c + Vec2::new(-0.35, 0.1) * s, c + Vec2::new(0.15, 0.1) * s, c + Vec2::new(-0.25, 1.0) * s];
            p.add(egui::Shape::line(pts.clone(), Stroke::new(s * 0.42, Color32::from_rgb(120, 60, 0))));
            p.add(egui::Shape::line(pts, Stroke::new(s * 0.26, col)));
        }
        Action::Standup => {
            p.circle_filled(c, s * 0.95, col.gamma_multiply(0.25));
            p.rect_filled(Rect::from_center_size(c, Vec2::new(s * 0.45, s * 1.4)), s * 0.08, col);
            p.rect_filled(Rect::from_center_size(c, Vec2::new(s * 1.4, s * 0.45)), s * 0.08, col);
        }
        Action::Rush => {
            scene::paint_res(p, Res::Coffee, c + Vec2::new(s * 0.15, 0.0), size * 0.75);
            for k in 0..3 {
                let y = c.y - s * 0.4 + k as f32 * s * 0.4;
                p.line_segment([Pos2::new(c.x - s * 1.0, y), Pos2::new(c.x - s * 0.55, y)], Stroke::new(s * 0.12, col));
            }
        }
        Action::Freeze => {
            for k in 0..3 {
                let d = Vec2::angled(k as f32 * std::f32::consts::PI / 3.0 + std::f32::consts::FRAC_PI_2) * s * 0.95;
                p.line_segment([c - d, c + d], Stroke::new(s * 0.14, col));
                for end in [c + d * 0.6, c - d * 0.6] {
                    let n = d.normalized().rot90() * s * 0.22;
                    let back = d.normalized() * s * 0.2 * if end.distance(c + d) < end.distance(c - d) { 1.0 } else { -1.0 };
                    p.line_segment([end, end + back + n], Stroke::new(s * 0.1, col));
                    p.line_segment([end, end + back - n], Stroke::new(s * 0.1, col));
                }
            }
        }
        Action::Friday => {
            let spin = time as f32 * 0.8;
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0 + spin;
                p.add(egui::Shape::convex_polygon(vec![c + Vec2::angled(a - 0.3) * s * 0.5, c + Vec2::angled(a) * s * 1.05, c + Vec2::angled(a + 0.3) * s * 0.5], col, Stroke::NONE));
            }
            p.circle_filled(c, s * 0.58, Color32::from_rgb(255, 200, 60));
            p.circle_filled(c, s * 0.3, Color32::from_rgb(255, 245, 200));
        }
    }
}

/// What card `n` gives at `level`, in a few words.
fn gives(n: usize, level: u8, fr: bool) -> String {
    let pct = card_bonus(n, level.max(1)) * 100.0;
    match CARDS[n].effect {
        Effect::Troop(t) => format!("{} +{pct:.0} %", tx(t.name(), fr)),
        Effect::ArmyHp => format!("{} +{pct:.0} %", tx(("PV armée", "Army HP"), fr)),
        Effect::ArmyDps => format!("{} +{pct:.0} %", tx(("Dégâts armée", "Army dmg"), fr)),
        Effect::Loot => format!("{} +{pct:.0} %", tx(("Butin", "Loot"), fr)),
        Effect::Action(a) => tx(("Action", "Action"), fr).to_owned() + " · " + action_short(a, fr),
    }
}

/// What card `n` does at `level`, in a sentence.
fn does(n: usize, level: u8, fr: bool) -> String {
    let pct = card_bonus(n, level.max(1)) * 100.0;
    match CARDS[n].effect {
        Effect::Troop(t) => if fr { format!("{} : +{pct:.0} % de PV et de dégâts.", tx(t.name(), fr)) } else { format!("{}: +{pct:.0} % hit points and damage.", tx(t.name(), fr)) },
        Effect::ArmyHp => if fr { format!("Toutes tes troupes : +{pct:.0} % de PV.") } else { format!("All your troops: +{pct:.0} % hit points.") },
        Effect::ArmyDps => if fr { format!("Toutes tes troupes : +{pct:.0} % de dégâts.") } else { format!("All your troops: +{pct:.0} % damage.") },
        Effect::Loot => if fr { format!("+{pct:.0} % de café et de budget ramenés des raids.") } else { format!("+{pct:.0} % coffee and budget brought back from raids.") },
        Effect::Action(a) => format!("{} {}", a.text(level.max(1), fr), tx(("Se joue une fois par raid.", "Played once per raid."), fr)),
    }
}

/// Text centered at `c`, wrapped at `width`.
fn text_at(painter: &egui::Painter, c: Pos2, text: &str, font: FontId, color: Color32, width: f32) {
    let g = painter.layout(text.to_owned(), font, color, width);
    painter.galley(c - g.size() / 2.0, g, color);
}

/// Card `n` drawn in `r` (its width sets the scale), with `copies` of it (0: not found yet).
pub(super) fn paint_card(painter: &egui::Painter, n: usize, r: Rect, copies: u32, fr: bool, time: f64) {
    let c = &CARDS[n];
    let k = r.width() / 100.0;
    let rare = rarity_color(c.rarity);
    let round = (9.0 * k) as u8;
    painter.rect_filled(r.translate(Vec2::new(0.0, 3.0 * k)), round, Color32::from_black_alpha(80));
    if copies == 0 {
        painter.rect_filled(r, round, Color32::from_rgb(34, 36, 44));
        painter.rect_stroke(r, round, Stroke::new(1.5 * k, rare.gamma_multiply(0.35)), egui::StrokeKind::Inside);
        painter.circle_filled(Pos2::new(r.center().x, r.min.y + 52.0 * k), 28.0 * k, Color32::from_rgb(44, 46, 56));
        painter.text(Pos2::new(r.center().x, r.min.y + 52.0 * k), Align2::CENTER_CENTER, "?", FontId::proportional(30.0 * k), rare.gamma_multiply(0.5));
        text_at(painter, Pos2::new(r.center().x, r.max.y - 22.0 * k), tx(c.rarity.name(), fr), FontId::proportional(10.0 * k), rare.gamma_multiply(0.6), r.width());
        return;
    }
    // The picture's colour: the action's, the troop's, else the rarity's.
    let tint = match c.effect {
        Effect::Action(a) => action_color(a),
        Effect::Troop(t) => scene::troop_colors(t).0,
        _ => rare,
    };
    painter.rect_filled(r, round, Color32::from_rgb(30, 32, 40));
    let art = Rect::from_min_max(r.min + Vec2::new(4.0 * k, 4.0 * k), Pos2::new(r.max.x - 4.0 * k, r.min.y + 92.0 * k));
    painter.rect_filled(art, (6.0 * k) as u8, tint.gamma_multiply(0.30));
    // A glow behind the picture, turning for the rarest.
    let center = Pos2::new(art.center().x, art.min.y + 44.0 * k);
    let clip = painter.with_clip_rect(art);
    if c.rarity >= Rarity::Rare {
        for i in 0..10 {
            let a = i as f32 * std::f32::consts::TAU / 10.0 + time as f32 * 0.3;
            clip.add(egui::Shape::convex_polygon(vec![center, center + Vec2::angled(a - 0.13) * 70.0 * k, center + Vec2::angled(a + 0.13) * 70.0 * k], tint.gamma_multiply(0.12), Stroke::NONE));
        }
    }
    clip.circle_filled(center, 30.0 * k, tint.gamma_multiply(0.35));
    match c.effect {
        Effect::Troop(t) => scene::paint_troop(&clip, t, center + Vec2::new(0.0, 30.0 * k), 2.3 * k, time as f32, false, false, (1.0, 0.3)),
        Effect::Action(a) => paint_action_icon(&clip, a, center, 44.0 * k, time),
        Effect::Loot => scene::paint_res(&clip, Res::Budget, center, 40.0 * k),
        Effect::ArmyHp => {
            for (dx, t) in [(-14.0, Troop::Intern), (14.0, Troop::Dev), (0.0, Troop::Sales)] {
                scene::paint_troop(&clip, t, center + Vec2::new(dx * k, 24.0 * k), 1.1 * k, time as f32, false, false, (1.0, 0.3));
            }
            clip.text(center + Vec2::new(18.0 * k, -18.0 * k), Align2::CENTER_CENTER, "♥", FontId::proportional(18.0 * k), Color32::from_rgb(255, 90, 110));
        }
        Effect::ArmyDps => {
            scene::paint_res(&clip, Res::Coffee, center, 40.0 * k);
            paint_action_icon(&clip, Action::Hotfix, center + Vec2::new(20.0 * k, -16.0 * k), 18.0 * k, time);
        }
    }
    // The name band, what it gives, the level's pips.
    let band = Rect::from_min_max(Pos2::new(r.min.x, art.max.y - 12.0 * k), Pos2::new(r.max.x, art.max.y + 10.0 * k));
    painter.rect_filled(band, 0.0, Color32::from_rgb(22, 24, 30));
    text_at(painter, band.center(), tx(c.name, fr), FontId::proportional(10.5 * k), Color32::WHITE, r.width() - 6.0 * k);
    let level = card_level(copies);
    text_at(painter, Pos2::new(r.center().x, band.max.y + 11.0 * k), &gives(n, level, fr), FontId::proportional(10.5 * k), tint.lerp_to_gamma(Color32::WHITE, 0.35), r.width() - 6.0 * k);
    for i in 0..5u8 {
        let p = Pos2::new(r.center().x + (i as f32 - 2.0) * 8.0 * k, r.max.y - 8.0 * k);
        if i < level {
            painter.circle_filled(p, 2.8 * k, rare);
        } else {
            painter.circle_stroke(p, 2.6 * k, Stroke::new(0.8 * k, Color32::from_white_alpha(60)));
        }
    }
    // The frame, in the rarity's colour (shining for the rarest).
    let shine = if c.rarity >= Rarity::Epic { ((time * 2.5).sin() as f32 * 0.5 + 0.5) * 0.4 + 0.6 } else { 1.0 };
    painter.rect_stroke(r, round, Stroke::new(2.5 * k, rare.gamma_multiply(shine)), egui::StrokeKind::Inside);
}

/// A card's tooltip: big, with what it does and how far its next level is.
fn card_tip(ui: &mut Ui, n: usize, copies: u32, in_deck: Option<bool>, fr: bool) {
    let c = &CARDS[n];
    let time = ui.input(|i| i.time);
    ui.set_width(330.0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(130.0, 182.0), Sense::hover());
        paint_card(ui.painter(), n, r, copies, fr, time);
        ui.vertical(|ui| {
            ui.set_width(186.0);
            if copies == 0 {
                ui.label(egui::RichText::new(tx(("Pas encore trouvée", "Not found yet"), fr)).size(14.0).strong());
                ui.label(egui::RichText::new(tx(c.rarity.name(), fr)).size(12.0).color(rarity_color(c.rarity)));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tx(("Gagne des raids, termine des missions ou ouvre le courrier interne pour la trouver.", "Win raids, finish missions or open the internal mail to find it."), fr)).size(12.0));
                return;
            }
            let level = card_level(copies);
            ui.label(egui::RichText::new(tx(c.name, fr)).size(15.0).strong());
            ui.label(egui::RichText::new(format!("{} · {} {level}", tx(c.rarity.name(), fr), tx(("niveau", "level"), fr))).size(12.0).color(rarity_color(c.rarity)));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(does(n, level, fr)).size(12.5));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("« {} »", tx(c.quote, fr))).size(11.5).italics().weak());
            ui.add_space(6.0);
            match COPIES.get(level as usize) {
                Some(&need) => {
                    ui.label(egui::RichText::new(format!("{} {copies} / {need}", tx(("Exemplaires pour le niveau suivant :", "Copies for the next level:"), fr))).size(11.5));
                }
                None => {
                    ui.label(egui::RichText::new(tx(("Niveau maximum !", "Top level!"), fr)).size(11.5).strong());
                }
            }
            if let Some(on) = in_deck {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(if on { tx(("Dans ton deck. Clique pour la retirer.", "In your deck. Click to take it out."), fr) } else { tx(("Clique pour la mettre dans ton deck.", "Click to put it in your deck."), fr) }).size(11.5).strong());
            }
        });
    });
}

const CARD: Vec2 = Vec2::new(84.0, 118.0);

/// The « Cards » panel: the cards the mail just gave, if opened.
pub(super) fn cards_ui(ui: &mut Ui, s: &mut OpenSpace, fr: bool, theme: &Theme, unix: i64) -> Vec<usize> {
    let time = ui.input(|i| i.time);
    let mut got = Vec::new();
    let found = (0..CARDS.len()).filter(|&n| s.copies(n) > 0).count();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tx(("Cartes de combat", "Battle cards"), fr)).size(17.0).strong().color(theme.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("{found} / {}", CARDS.len())).size(13.0).color(theme.text_muted));
        });
    });
    // The internal mail.
    let at = s.mail_at();
    if unix >= at {
        let b = egui::Button::new(egui::RichText::new(format!("📬  {}", tx(("Courrier interne : une carte gratuite !", "Internal mail: a free card!"), fr))).size(13.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
        if ui.add_sized(Vec2::new(ui.available_width(), 30.0), b).clicked()
            && let Some(n) = s.open_mail(unix)
        {
            got.push(n);
        }
    } else {
        ui.label(egui::RichText::new(format!("📭  {} {}", tx(("Prochain courrier interne dans", "Next internal mail in"), fr), os::duration_text(at - unix, fr))).size(12.0).color(theme.text_muted));
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // The deck: its slots, filled or not.
        ui.add_space(6.0);
        let size = s.deck_size();
        ui.label(egui::RichText::new(format!("{}  {} / {size}", tx(("Ton deck de raid", "Your raid deck"), fr), s.deck.len())).size(13.5).strong().color(theme.text));
        ui.label(egui::RichText::new(tx(("Les cartes action du deck partent en raid avec toi : chacune se joue une fois, où tu veux.", "The deck's action cards come on raids with you: each is played once, wherever you want."), fr)).size(11.5).color(theme.text_muted));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for slot in 0..4 {
                let (r, resp) = ui.allocate_exact_size(CARD, Sense::click());
                if let Some(&n) = s.deck.get(slot) {
                    let r = if resp.hovered() { r.translate(Vec2::new(0.0, -3.0)) } else { r };
                    paint_card(ui.painter(), n, r, s.copies(n), fr, time);
                    let copies = s.copies(n);
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_ui(|ui| card_tip(ui, n, copies, Some(true), fr)).clicked() {
                        s.toggle_deck(n);
                    }
                } else if slot < size {
                    ui.painter().rect_stroke(r, 9.0, Stroke::new(1.5, theme.text_muted.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                    ui.painter().text(r.center() - Vec2::new(0.0, 8.0), Align2::CENTER_CENTER, "+", FontId::proportional(26.0), theme.text_muted.gamma_multiply(0.6));
                    text_at(ui.painter(), r.center() + Vec2::new(0.0, 22.0), tx(("Place libre", "Free slot"), fr), FontId::proportional(10.5), theme.text_muted, r.width() - 8.0);
                } else {
                    ui.painter().rect_filled(r, 9.0, theme.tab_hover.gamma_multiply(0.4));
                    ui.painter().text(r.center() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, "🔒", FontId::proportional(18.0), theme.text_muted.gamma_multiply(0.6));
                    let why = if slot == 2 { tx(("Salle de formation", "Training room"), fr) } else { tx(("Formation niv. 3", "Training lv. 3"), fr) };
                    text_at(ui.painter(), r.center() + Vec2::new(0.0, 18.0), why, FontId::proportional(10.0), theme.text_muted, r.width() - 8.0);
                }
            }
        });
        // The cards, actions first.
        for (title, sub, actions) in [
            (tx(("Cartes action", "Action cards"), fr), tx(("Clique pour la mettre dans ton deck ou l'en retirer.", "Click to put it in your deck or take it out."), fr), true),
            (tx(("Cartes de troupe", "Troop cards"), fr), tx(("Toujours actives : elles renforcent tes troupes en raid.", "Always on: they make your troops stronger on raids."), fr), false),
        ] {
            ui.add_space(10.0);
            ui.label(egui::RichText::new(title).size(13.5).strong().color(theme.text));
            ui.label(egui::RichText::new(sub).size(11.5).color(theme.text_muted));
            ui.add_space(4.0);
            let mut list: Vec<usize> = (0..CARDS.len()).filter(|&n| is_action(n) == actions).collect();
            list.sort_by_key(|&n| (s.copies(n) == 0, CARDS[n].rarity));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                for n in list {
                    let copies = s.copies(n);
                    let (r, resp) = ui.allocate_exact_size(CARD, Sense::click());
                    let in_deck = s.deck.contains(&n);
                    let r = if resp.hovered() && copies > 0 { r.translate(Vec2::new(0.0, -3.0)) } else { r };
                    paint_card(ui.painter(), n, r, copies, fr, time);
                    if copies > 1 {
                        let badge = Pos2::new(r.max.x - 10.0, r.min.y + 10.0);
                        ui.painter().circle_filled(badge, 9.0, Color32::from_black_alpha(200));
                        ui.painter().text(badge, Align2::CENTER_CENTER, format!("×{copies}"), FontId::proportional(9.0), Color32::WHITE);
                    }
                    if in_deck {
                        let tag = Rect::from_min_size(r.min + Vec2::new(5.0, 5.0), Vec2::new(36.0, 15.0));
                        ui.painter().rect_filled(tag, 7.0, theme.accent);
                        ui.painter().text(tag.center(), Align2::CENTER_CENTER, "DECK", FontId::proportional(9.5), theme.bg);
                    }
                    let resp = resp.on_hover_ui(|ui| card_tip(ui, n, copies, (actions && copies > 0).then_some(in_deck), fr));
                    if actions && copies > 0 && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !s.toggle_deck(n) && !in_deck {
                        // The deck full: say so where the cards are.
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("os-deck-full"), time));
                    }
                }
            });
        }
        if let Some(at) = ui.ctx().data(|d| d.get_temp::<f64>(egui::Id::new("os-deck-full")))
            && time - at < 2.5
        {
            ui.label(egui::RichText::new(tx(("Ton deck est plein : retire d'abord une carte.", "Your deck is full: take a card out first."), fr)).size(12.0).color(theme.ansi[3]));
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tx(("Une carte par raid gagné. Un doublon fait monter la carte de niveau (5 niveaux).", "A card for each raid won. A copy levels the card up (5 levels)."), fr)).size(11.0).color(theme.text_muted));
    });
    got
}

/// The mission under way, on the left (`folded`: just its title). What it gave once claimed (a card).
pub(super) fn mission_ui(ui: &mut Ui, at: Pos2, s: &mut OpenSpace, folded: &mut bool, fr: bool, theme: &Theme) -> Option<Option<usize>> {
    let m = s.mission()?;
    let time = ui.input(|i| i.time);
    let gold = Color32::from_rgb(255, 196, 64);
    let done = s.mission_done();
    let (have, need) = m.progress(s);
    let mut claimed = None;
    egui::Area::new(egui::Id::new("os-mission")).fixed_pos(at).order(egui::Order::Middle).show(ui.ctx(), |ui| {
        let stroke = if done { Stroke::new(2.0 + ((time * 4.0).sin() as f32 * 0.5 + 0.5) * 1.5, gold) } else { Stroke::new(1.0, theme.tab_hover) };
        Frame::new().fill(theme.chrome_bg).stroke(stroke).corner_radius(12.0).inner_margin(12.0).shadow(egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(80) }).show(ui, |ui| {
            ui.set_width(if *folded { 230.0 } else { 290.0 });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("🎯 {} {}/{}", tx(("Mission", "Mission"), fr), s.missions + 1, MISSIONS.len())).size(11.5).strong().color(gold));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(if *folded { "▾" } else { "▴" }).size(12.0)).frame(false)).on_hover_text(if *folded { tx(("Déplier", "Unfold"), fr) } else { tx(("Replier", "Fold"), fr) }).clicked() {
                        *folded = !*folded;
                    }
                });
            });
            ui.label(egui::RichText::new(tx(m.title, fr)).size(14.5).strong().color(theme.text));
            if !*folded {
                ui.add(egui::Label::new(egui::RichText::new(tx(m.text, fr)).size(12.5).color(theme.text_muted)).wrap());
            }
            ui.add_space(4.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
            let bar = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(r.width() - 70.0, 8.0));
            ui.painter().rect_filled(bar, 4.0, theme.tab_hover);
            ui.painter().rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * (have as f32 / need.max(1) as f32), 8.0)), 4.0, if done { gold } else { theme.accent });
            ui.painter().text(Pos2::new(r.max.x, r.center().y), Align2::RIGHT_CENTER, format!("{} / {}", os::short(have as f64, fr), os::short(need as f64, fr)), FontId::proportional(11.5), theme.text);
            if !*folded || done {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(("Récompense :", "Reward:"), fr)).size(11.5).color(theme.text_muted));
                    let rw = m.reward;
                    let mut parts = Vec::new();
                    if rw.tickets > 0 {
                        parts.push(format!("{} 🎫", rw.tickets));
                    }
                    if rw.coffee > 0 {
                        parts.push(os::amount_text(rw.coffee as f64, Res::Coffee, fr));
                    }
                    if rw.budget > 0 {
                        parts.push(os::amount_text(rw.budget as f64, Res::Budget, fr));
                    }
                    if let Some(r) = rw.card {
                        parts.push(format!("♠ {} {}", tx(("carte", "card"), fr), tx(r.name(), fr).to_lowercase()));
                    }
                    ui.label(egui::RichText::new(parts.join(" · ")).size(11.5).strong().color(theme.text));
                });
            }
            if done {
                ui.add_space(6.0);
                let b = egui::Button::new(egui::RichText::new(tx(("Récupérer la récompense", "Claim the reward"), fr)).size(13.0).strong().color(theme.bg)).fill(gold).corner_radius(8.0);
                if ui.add_sized(Vec2::new(ui.available_width(), 30.0), b).clicked() {
                    claimed = s.claim_mission();
                }
            }
        });
    });
    claimed
}

/// The mission under way's place to point at (None: nothing, or done).
pub(super) fn mission_spot(s: &OpenSpace) -> Option<os::Spot> {
    let m: &Mission = s.mission()?;
    (!s.mission_done()).then_some(m.spot)
}

/// The cards just found, turned over one after the other since `since`: true once closed.
pub(super) fn reveal(ui: &mut Ui, rect: Rect, found: &[usize], since: f64, s: &OpenSpace, fr: bool, theme: &Theme) -> bool {
    let time = ui.input(|i| i.time);
    let age = (time - since) as f32;
    ui.ctx().request_repaint();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("os-reveal")));
    ui.interact(rect, ui.id().with("os-reveal-block"), Sense::click_and_drag());
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha((age * 400.0).min(180.0) as u8));
    let n = found.len().max(1) as f32;
    let w = 180.0_f32.min((rect.width() - 60.0) / n - 24.0);
    let h = w * CARD.y / CARD.x;
    let title = if found.len() > 1 { tx(("Nouvelles cartes !", "New cards!"), fr) } else { tx(("Nouvelle carte !", "New card!"), fr) };
    painter.text(Pos2::new(rect.center().x, rect.center().y - h / 2.0 - 46.0), Align2::CENTER_CENTER, title, FontId::proportional(26.0), Color32::WHITE);
    for (i, &card) in found.iter().enumerate() {
        // Several copies of one card in the same draw: counted one after the other.
        let later = found[i + 1..].iter().filter(|&&m| m == card).count() as u32;
        let copies = s.copies(card).saturating_sub(later);
        let t = age - 0.25 - i as f32 * 0.5;
        if t <= 0.0 {
            continue;
        }
        let c = Pos2::new(rect.center().x + (i as f32 - (n - 1.0) / 2.0) * (w + 28.0), rect.center().y);
        let color = rarity_color(CARDS[card].rarity);
        // It comes up face down, then turns over (its width through zero), with a burst behind it.
        let grow = (t / 0.25).min(1.0);
        let turn = ((t - 0.25) / 0.35).clamp(0.0, 1.0);
        let sx = ((turn * std::f32::consts::PI).cos()).abs().max(0.04);
        let full = Vec2::new(w, h) * (0.6 + 0.4 * grow);
        let r = Rect::from_center_size(c, Vec2::new(full.x * sx, full.y));
        if turn >= 0.5 {
            let glow = (t - 0.6).clamp(0.0, 1.0);
            for k in 0..12 {
                let a = k as f32 * std::f32::consts::TAU / 12.0 + time as f32 * 0.5;
                let len = h * (0.78 + 0.1 * ((time * 3.0) as f32 + k as f32).sin());
                painter.add(egui::Shape::convex_polygon(vec![c, c + Vec2::angled(a - 0.09) * len, c + Vec2::angled(a + 0.09) * len], color.gamma_multiply((0.10 + 0.07 * CARDS[card].rarity as u8 as f32) * glow), Stroke::NONE));
            }
            let flash = ((t - 0.6) / 0.4).clamp(0.0, 1.0);
            if flash < 1.0 {
                painter.circle_filled(c, h * 0.8 * flash + 1.0, Color32::from_white_alpha(((1.0 - flash) * 90.0) as u8));
            }
            if sx > 0.97 {
                paint_card(&painter, card, Rect::from_center_size(c, full), copies, fr, time);
            } else {
                painter.rect_filled(r, 9.0, Color32::from_rgb(30, 32, 40));
                painter.rect_stroke(r, 9.0, Stroke::new(3.0, color), egui::StrokeKind::Inside);
            }
            // New, a level up, or a copy toward the next.
            let level = card_level(copies);
            let (note, col) = if copies == 1 {
                (tx(("NOUVELLE !", "NEW!"), fr).to_owned(), Color32::from_rgb(255, 196, 64))
            } else if COPIES.contains(&copies) {
                (format!("{} {level} !", tx(("NIVEAU", "LEVEL"), fr)), Color32::from_rgb(110, 220, 130))
            } else {
                let need = COPIES.get(level as usize).copied().unwrap_or(copies);
                (format!("{} {copies} / {need}", tx(("Exemplaires", "Copies"), fr)), Color32::from_white_alpha(210))
            };
            painter.text(Pos2::new(c.x, c.y + h / 2.0 + 20.0), Align2::CENTER_CENTER, note, FontId::proportional(15.0), col);
            text_at(&painter, Pos2::new(c.x, c.y + h / 2.0 + 44.0), &does(card, level, fr), FontId::proportional(12.0), Color32::from_white_alpha(220), w + 4.0);
        } else {
            // Its back.
            painter.rect_filled(r, 9.0, Color32::from_rgb(40, 44, 72));
            painter.rect_stroke(r, 9.0, Stroke::new(3.0, Color32::from_rgb(120, 130, 200)), egui::StrokeKind::Inside);
            painter.text(r.center(), Align2::CENTER_CENTER, "⚔", FontId::proportional(34.0 * sx.max(0.2)), Color32::from_rgb(140, 150, 220));
        }
    }
    let mut close = false;
    if age > 0.25 + n * 0.5 + 0.4 {
        let b = Rect::from_center_size(Pos2::new(rect.center().x, rect.center().y + h / 2.0 + 96.0), Vec2::new(150.0, 38.0));
        egui::Area::new(egui::Id::new("os-reveal-ok")).fixed_pos(b.min).order(egui::Order::Tooltip).show(ui.ctx(), |ui| {
            close = ui.add_sized(b.size(), egui::Button::new(egui::RichText::new(tx(("Super !", "Great!"), fr)).size(15.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(10.0)).clicked();
        });
        if ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    }
    close
}
