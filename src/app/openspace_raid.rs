//! Open Space, the army and the raids, on screen: the « Army » panel (recruiting, the troops being
//! trained, the meeting room's upgrades), the campaign's map (« Attack »), and a raid: the rival open
//! space, the troops to drop at the bottom, the time and the stars at the top, what it brought at the
//! end. The raid runs its fixed steps as the time goes (see `crate::openspace_battle`).

use super::openspace_scene::{self as scene, Iso, View};
use super::*;
use crate::openspace::{self as os, tx, OpenSpace, Why};
use crate::openspace_battle::{self as war, Battle, Hero, Outcome, Troop, CHAPTERS, LEVELS, STEP, TIME_LIMIT, TROOPS};

/// What a raid is on.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Campaign,
    /// In the world: a player, a bot or the boss (floor's id for it), its name.
    World { id: String, name: String },
    /// A day's quest on the world's map.
    Quest { id: String, name: String },
}

/// What floor said a world raid brought (kept after the cards' loot bonus and the room left).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Paid {
    pub coffee: f64,
    pub budget: f64,
    pub tickets: u32,
    /// The trophies won (or lost) and the new total, for a raid on a player or a bot.
    pub trophies: Option<(i64, i64)>,
    pub troops: Vec<(Troop, u32)>,
}

/// A raid under way.
pub(super) struct Fight {
    pub b: Battle,
    pub level: u32,
    pub target: Target,
    /// A world raid's answer from floor: none while waiting, an error when it couldn't be counted.
    pub paid: Option<Result<Paid, String>>,
    /// Sent to floor once over.
    pub sent: bool,
    pick: Option<Troop>,
    /// The action card picked (its place in the battle's cards), to play where clicked.
    card: Option<usize>,
    acc: f32,
    last_drop: f64,
    pub result: Option<Outcome>,
    speed: f32,
    view: View,
}

impl Fight {
    /// A raid on the campaign's `level`, with the whole army, and the hero when ready.
    pub fn new(level: u32, s: &OpenSpace, now: i64) -> Self {
        Self::on(&war::rival_base(level), level, Target::Campaign, s, now)
    }

    /// A raid on `base` (a player's open space, or one drawn like the campaign's `level`).
    pub fn on(base: &[os::Building], level: u32, target: Target, s: &OpenSpace, now: i64) -> Self {
        let mut army: Vec<(Troop, u8, u32)> = s.army.iter().map(|(t, n)| (*t, s.troop_level(*t), *n)).collect();
        if s.hero(now) == Hero::Ready {
            army.push((Troop::Lead, s.hero_level(), 1));
        }
        let mut b = Battle::new(base, army);
        b.boosts = TROOPS.iter().map(|&t| { let (hp, dps) = s.troop_boost(t); (t, hp, dps) }).collect();
        b.cards = s.raid_cards().into_iter().map(|(a, l)| (a, l, false)).collect();
        let pick = b.left.first().map(|(t, _, _)| *t);
        Fight { b, level, target, paid: None, sent: false, pick, card: None, acc: 0.0, last_drop: 0.0, result: None, speed: 1.0, view: View { zoom: 0.95, pan: Vec2::ZERO } }
    }
}


/// A troop's little portrait, in `r`.
pub(super) fn portrait(painter: &egui::Painter, t: Troop, r: Rect, time: f64) {
    painter.circle_filled(r.center(), r.width().min(r.height()) / 2.0, scene::troop_colors(t).0.gamma_multiply(0.25));
    let feet = r.center_bottom() - Vec2::new(0.0, r.height() * 0.12);
    scene::paint_troop(painter, t, feet + if t.stats().air { Vec2::new(0.0, 10.0) } else { Vec2::ZERO }, r.height() / 34.0, time as f32, false, false, (1.0, 0.3));
}

/// The hero's card: their portrait, level, stats, where they are (ready, training, resting), the
/// training to the next level and the rest cut short for tickets.
pub(super) fn hero_ui(ui: &mut Ui, s: &mut OpenSpace, fr: bool, theme: &Theme, unix: i64, said: &mut Option<Why>) {
    let time = ui.input(|i| i.time);
    let state = s.hero(unix);
    if state == Hero::None {
        return;
    }
    let level = s.hero_level();
    let gold = Color32::from_rgb(255, 196, 64);
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 62.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r.shrink2(Vec2::new(0.0, 2.0)), 10.0, gold.gamma_multiply(0.12));
    p.rect_stroke(r.shrink2(Vec2::new(0.0, 2.0)), 10.0, Stroke::new(1.5, gold.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    portrait(p, Troop::Lead, Rect::from_center_size(Pos2::new(r.min.x + 30.0, r.center().y), Vec2::splat(48.0)), time);
    let left = r.min.x + 62.0;
    p.text(Pos2::new(left, r.min.y + 15.0), Align2::LEFT_CENTER, format!("♛ {}  {} {level} / {}", tx(Troop::Lead.name(), fr), tx(("niv.", "lv."), fr), s.hero_cap()), FontId::proportional(14.0), theme.text);
    let st = Troop::Lead.stats();
    p.text(
        Pos2::new(left, r.min.y + 32.0),
        Align2::LEFT_CENTER,
        format!("{:.0} {}  ·  {:.0} {}  ·  {} {}", Troop::Lead.hp(level), tx(("PV", "HP"), fr), Troop::Lead.dps(level), tx(("dégâts/s", "dmg/s"), fr), st.housing, tx(("place", "room"), fr)),
        FontId::proportional(11.5),
        theme.text_muted,
    );
    let (status, color) = match state {
        Hero::Ready => (tx(("Prêt pour le prochain raid", "Ready for the next raid"), fr).to_owned(), theme.ansi[2]),
        Hero::Training(until) => (format!("{} {} ({})", tx(("À l'entraînement vers le niveau", "Training to level"), fr), level + 1, os::duration_text(until - unix, fr)), theme.accent),
        Hero::Resting(until) => (format!("{} ({})", tx(("Se repose après sa chute", "Resting after falling"), fr), os::duration_text(until - unix, fr)), theme.ansi[3]),
        Hero::None => (String::new(), theme.text),
    };
    p.text(Pos2::new(left, r.min.y + 48.0), Align2::LEFT_CENTER, status, FontId::proportional(11.5), color);
    ui.horizontal_wrapped(|ui| {
        match state {
            Hero::Resting(_) => {
                let price = s.wake_hero_cost(unix);
                if ui.add(egui::Button::new(egui::RichText::new(format!("☕ {} {price} 🎫", tx(("Le réveiller :", "Wake them:"), fr))).size(11.5)).corner_radius(6.0)).clicked()
                    && let Err(why) = s.wake_hero(unix)
                {
                    *said = Some(why);
                }
            }
            Hero::Training(_) => {}
            _ => match s.can_train_hero() {
                Err(Why::MaxLevel) => {
                    let text = if level >= war::HERO_MAX { tx(("Niveau maximum", "Highest level"), fr) } else { tx(("Améliore son bureau pour aller plus haut", "Upgrade their office to go higher"), fr) };
                    ui.label(egui::RichText::new(text).size(11.5).color(theme.text_muted));
                }
                can => {
                    let (price, secs) = war::hero_training(level + 1);
                    let text = format!("⬆ {} {}  ·  {}  ·  {}", tx(("Niveau", "Level"), fr), level + 1, os::amount_text(price as f64, os::Res::Coffee, fr), os::duration_text(secs as i64, fr));
                    let resp = ui.add_enabled(can.is_ok(), egui::Button::new(egui::RichText::new(text).size(11.5)).corner_radius(6.0));
                    if resp.on_hover_text(tx(("Pas de raid pour lui pendant l'entraînement.", "No raids for them while training."), fr)).clicked()
                        && let Err(why) = s.train_hero(unix)
                    {
                        *said = Some(why);
                    }
                    if let Err(why) = can {
                        ui.label(egui::RichText::new(why.text(fr)).size(11.0).color(theme.ansi[3]));
                    }
                }
            },
        }
    });
}

/// The « Army » panel: true when something was done (the tutorial checks).
pub(super) fn army_ui(ui: &mut Ui, s: &mut OpenSpace, fr: bool, theme: &Theme, unix: i64, said: &mut Option<Why>) {
    let time = ui.input(|i| i.time);
    let (used, room) = (s.housing_used(), s.housing_room());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tx(("Armée", "Army"), fr)).size(17.0).strong().color(theme.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("{used} / {room}")).size(13.0).color(if used >= room && room > 0 { theme.ansi[3] } else { theme.text_muted }))
                .on_hover_text(tx(("Place prise (armée et troupes en formation) / place des salles de pause", "Room taken (army and troops in training) / the break rooms' room"), fr));
        });
    });
    hero_ui(ui, s, fr, theme, unix, said);
    if room == 0 {
        ui.label(egui::RichText::new(tx(("Construis une salle de pause pour loger tes troupes, et une salle de recrutement pour les former.", "Build a break room to house your troops, and a recruiting room to train them."), fr)).size(12.5).color(theme.ansi[3]));
    }
    // The army ready, and those being trained.
    if !s.army.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (t, n) in s.army.clone() {
                let (r, _) = ui.allocate_exact_size(Vec2::new(40.0, 46.0), Sense::hover());
                portrait(ui.painter(), t, Rect::from_min_size(r.min, Vec2::splat(36.0)), time);
                ui.painter().text(r.right_bottom(), Align2::RIGHT_BOTTOM, format!("×{n}"), FontId::proportional(12.0), theme.text);
            }
        });
    }
    if let Some(&head) = s.queue.first() {
        ui.add_space(2.0);
        let left = (s.queue_until - unix).max(0);
        ui.label(egui::RichText::new(format!("{} {} · {} {}  ({})", tx(("En formation :", "Training:"), fr), s.queue.len(), tx(head.name(), fr), tx(("dans", "in"), fr), os::duration_text(left, fr))).size(12.0).color(theme.text_muted));
        ui.horizontal_wrapped(|ui| {
            let mut seen: Vec<(Troop, u32)> = Vec::new();
            for t in &s.queue {
                match seen.iter_mut().find(|(k, _)| k == t) {
                    Some(e) => e.1 += 1,
                    None => seen.push((*t, 1)),
                }
            }
            for (t, n) in seen {
                if ui.add(egui::Button::new(egui::RichText::new(format!("{} ×{n}  ✕", tx(t.name(), fr))).size(11.5)).corner_radius(10.0)).on_hover_text(tx(("Retirer un (café rendu)", "Take one off (coffee back)"), fr)).clicked() {
                    s.untrain(t, unix);
                }
            }
        });
    }
    ui.add_space(6.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(egui::RichText::new(tx(("Recruter", "Recruit"), fr)).size(12.0).strong().color(theme.text_muted));
        for t in TROOPS {
            let st = t.stats();
            let can = s.can_train(t);
            let level = s.troop_level(t);
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::click());
            let ok = can.is_ok();
            let p = ui.painter();
            p.rect_filled(r.shrink2(Vec2::new(0.0, 2.0)), 8.0, if resp.hovered() && ok { theme.tab_active } else { theme.tab_hover.gamma_multiply(0.6) });
            portrait(p, t, Rect::from_center_size(Pos2::new(r.min.x + 28.0, r.center().y), Vec2::splat(44.0)), time);
            let left = r.min.x + 58.0;
            p.text(Pos2::new(left, r.min.y + 13.0), Align2::LEFT_CENTER, format!("{}  {} {level}", tx(t.name(), fr), tx(("niv.", "lv."), fr)), FontId::proportional(13.5), if ok { theme.text } else { theme.text_muted });
            p.text(Pos2::new(r.max.x - 8.0, r.min.y + 13.0), Align2::RIGHT_CENTER, format!("×{}", s.army_count(t)), FontId::proportional(12.0), theme.text_muted);
            let line = format!("{}  ·  {}  ·  {} {}", os::amount_text(t.cost(level) as f64, os::Res::Coffee, fr), os::duration_text(s.train_secs(t), fr), st.housing, tx(("place", "room"), fr));
            p.text(Pos2::new(left, r.min.y + 31.0), Align2::LEFT_CENTER, line, FontId::proportional(11.5), if matches!(can, Err(Why::Short(..))) { theme.ansi[1] } else { theme.text_muted });
            if let Err(why) = can {
                p.text(Pos2::new(left, r.min.y + 47.0), Align2::LEFT_CENTER, why.text(fr), FontId::proportional(11.0), theme.ansi[3].gamma_multiply(0.9));
            }
            let resp = resp.on_hover_text(format!(
                "{}\n\n{} {:.0}  ·  {} {:.0}{}  ·  {} {:.1}",
                tx(t.about(), fr),
                tx(("PV", "HP"), fr),
                t.hp(level),
                if st.heals { tx(("soins/s", "heal/s"), fr) } else { tx(("dégâts/s", "dmg/s"), fr) },
                t.dps(level),
                if st.splash > 0.0 { tx((" (zone)", " (area)"), fr) } else { "" },
                tx(("portée", "range"), fr),
                st.range
            ));
            if ok && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                && let Err(why) = s.train(t, unix)
            {
                *said = Some(why);
            }
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tx(("Salle de réunion : améliorer", "Meeting room: upgrades"), fr)).size(12.0).strong().color(theme.text_muted));
        if let Some((t, until, secs)) = s.research {
            let frac = 1.0 - (until - unix).max(0) as f32 / secs.max(1) as f32;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(120.0, 8.0), Sense::hover());
                ui.painter().rect_filled(r, 4.0, theme.tab_hover);
                ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * frac, 8.0)), 4.0, theme.ansi[2]);
                ui.label(egui::RichText::new(format!("{} → {}  ({})", tx(t.name(), fr), s.troop_level(t) + 1, os::duration_text(until - unix, fr))).size(12.0).color(theme.text));
            });
        }
        for t in TROOPS {
            let level = s.troop_level(t);
            match s.can_research(t) {
                Err(Why::NeedRecruit(_)) => continue,
                Err(Why::NeedHq(l)) => {
                    ui.label(egui::RichText::new(format!("{} {l}", tx(("Construis une salle de réunion (QG", "Build a meeting room (HQ"), fr)) + ")").size(12.0).color(theme.text_muted));
                    break;
                }
                can => {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{}  {} {level}", tx(t.name(), fr), tx(("niv.", "lv."), fr))).size(12.5).color(theme.text));
                        match can {
                            Err(Why::MaxLevel) => {
                                ui.label(egui::RichText::new(if level >= t.stats().max_level { tx(("max", "max"), fr) } else { tx(("améliore la salle de réunion", "upgrade the meeting room"), fr) }).size(11.5).color(theme.text_muted));
                            }
                            _ => {
                                let (price, secs) = t.research(level + 1);
                                let enabled = can.is_ok();
                                let text = format!("⬆ {}  ·  {}", os::amount_text(price as f64, os::Res::Coffee, fr), os::duration_text(secs as i64, fr));
                                if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new(text).size(11.5)).corner_radius(6.0)).clicked()
                                    && let Err(why) = s.start_research(t, unix)
                                {
                                    *said = Some(why);
                                }
                            }
                        }
                    });
                }
            }
        }
    });
}

/// The campaign: the levels by chapter, their stars; the one picked to raid.
pub(super) fn campaign_ui(ui: &mut Ui, s: &OpenSpace, fr: bool, theme: &Theme) -> Option<u32> {
    let mut picked = None;
    let total: u32 = s.campaign.iter().map(|&n| n as u32).sum();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tx(("Campagne", "Campaign"), fr)).size(17.0).strong().color(theme.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("★ {total} / {}", LEVELS * 3)).size(13.0).color(Color32::from_rgb(255, 196, 64)));
        });
    });
    if s.army.is_empty() {
        ui.label(egui::RichText::new(tx(("Ton armée est vide : recrute des troupes avant d'attaquer.", "Your army is empty: recruit troops before attacking."), fr)).size(12.5).color(theme.ansi[3]));
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (c, name) in CHAPTERS.iter().enumerate() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("{} {} — {}", tx(("Chapitre", "Chapter"), fr), c + 1, tx(*name, fr))).size(12.0).strong().color(theme.text_muted));
            for level in (c as u32 * 10 + 1)..=(c as u32 * 10 + 10) {
                let open = s.level_open(level);
                let stars = s.stars(level);
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                let hot = resp.hovered() && open;
                let p = ui.painter();
                p.rect_filled(r.shrink2(Vec2::new(0.0, 1.5)), 6.0, if hot { theme.tab_active } else if open { theme.tab_hover.gamma_multiply(0.6) } else { theme.tab_hover.gamma_multiply(0.25) });
                let color = if open { theme.text } else { theme.text_muted.gamma_multiply(0.6) };
                p.text(Pos2::new(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, format!("{level}. {}", war::rival_name(level)), FontId::proportional(13.0), color);
                let st: String = (0..3).map(|k| if k < stars { '★' } else { '☆' }).collect();
                p.text(Pos2::new(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, if open { st } else { "🔒".into() }, FontId::proportional(14.0), if stars > 0 { Color32::from_rgb(255, 196, 64) } else { theme.text_muted });
                let (coffee, budget) = war::reward(level);
                p.text(Pos2::new(r.max.x - 70.0, r.center().y), Align2::RIGHT_CENTER, format!("{} · {}", os::amount_text(coffee as f64, os::Res::Coffee, fr), os::amount_text(budget as f64, os::Res::Budget, fr)), FontId::proportional(11.0), theme.text_muted);
                if open && !s.army.is_empty() && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(level);
                }
            }
        }
    });
    picked
}

fn clock(secs: f32) -> String {
    let s = secs.max(0.0).ceil() as i32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A raid's page: true once left.
pub(super) fn fight_page(ui: &mut Ui, rect: Rect, f: &mut Fight, s: &mut OpenSpace, fr: bool, theme: &Theme) -> bool {
    let time = ui.input(|i| i.time);
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    ui.ctx().request_repaint();
    if f.result.is_none() {
        if f.b.started || f.b.time > 0.0 {
            f.acc += dt * f.speed;
        }
        while f.acc >= STEP {
            f.b.step();
            f.acc -= STEP;
        }
        if f.b.over() {
            f.result = Some(match f.target {
                Target::Campaign => s.finish_raid(f.level, f.b.stars(), f.b.destroyed(), &f.b.survivors()),
                _ => s.finish_world(f.b.stars(), &f.b.survivors()),
            });
            if f.b.hero_fell() {
                s.hero_fell(super::openspace::unix_now());
            }
        }
    }

    let map = ui.interact(rect, ui.id().with("os-fight"), Sense::click_and_drag());
    if let Some(p) = map.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = pinch * (scroll * 0.0018).exp();
        if (factor - 1.0).abs() > 1e-4 {
            let old = f.view.zoom;
            let new = (old * factor).clamp(0.3, 2.4);
            let d = p - (rect.center() + f.view.pan);
            f.view.pan += d - d * (new / old);
            f.view.zoom = new;
        }
    }
    let carding = f.card.is_some() && f.result.is_none();
    let picking = f.pick.is_some() && f.result.is_none() && !carding;
    // Dropping with the left button (held: one after the other); the camera with the right one, or the
    // left one when nothing is picked.
    if map.dragged_by(egui::PointerButton::Secondary) || map.dragged_by(egui::PointerButton::Middle) || (!picking && !carding && map.dragged_by(egui::PointerButton::Primary)) {
        f.view.pan += map.drag_delta();
    }
    let iso = Iso::new(rect, &f.view);
    let hover = map.hover_pos().map(|p| iso.back(p));
    // An action card played where clicked; the troops picked again after.
    if carding
        && let Some(i) = f.card
        && map.clicked()
        && let Some(p) = map.interact_pointer_pos()
    {
        let (x, y) = iso.back(p);
        if f.b.play(i, x, y) {
            f.card = None;
            f.last_drop = time;
        }
    }
    if carding && (map.secondary_clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        f.card = None;
    }
    if picking
        && let Some(t) = f.pick
        && let Some(p) = map.interact_pointer_pos()
        && ui.input(|i| i.pointer.primary_down())
        && time - f.last_drop > 0.12
    {
        let (x, y) = iso.back(p);
        // The hero isn't in the army: dropped from the raid's own count.
        if (t == Troop::Lead || s.army_count(t) > 0) && f.b.drop(t, x, y) {
            if t != Troop::Lead {
                s.take_troop(t);
            }
            f.last_drop = time;
        }
        if !f.b.left.iter().any(|(k, _, n)| *k == t && *n > 0) {
            f.pick = f.b.left.iter().find(|(_, _, n)| *n > 0).map(|(k, _, _)| *k);
        }
    }
    let painter = ui.painter_at(rect);
    scene::paint_battle(&painter, rect, &f.view, &f.b, theme, time, picking, if picking { hover } else { None });
    // The card's reach under the pointer; the frozen defenses.
    for d in f.b.defs.iter().filter(|d| d.frozen > 0.0 && d.alive()) {
        let (cx, cy) = d.center();
        let half = d.kind.size() as f32 / 2.0;
        let pts: Vec<Pos2> = [(-half, -half), (half, -half), (half, half), (-half, half)].iter().map(|(dx, dy)| iso.at(cx + dx, cy + dy)).collect();
        painter.add(egui::Shape::convex_polygon(pts, Color32::from_rgba_unmultiplied(140, 210, 255, 70), Stroke::new(1.5, Color32::from_rgb(170, 225, 255))));
        painter.text(iso.up(iso.at(cx, cy), 40.0), Align2::CENTER_CENTER, "❄", FontId::proportional(16.0), Color32::from_rgb(190, 235, 255));
    }
    if carding
        && let (Some(i), Some((hx, hy))) = (f.card, hover)
        && let Some(&(action, _, _)) = f.b.cards.get(i)
    {
        let r = action.radius();
        let pts: Vec<Pos2> = (0..40).map(|k| {
            let a = k as f32 * std::f32::consts::TAU / 40.0;
            iso.at(hx + a.cos() * r, hy + a.sin() * r)
        }).collect();
        let col = super::openspace_cards::action_color(action);
        let pulse = (time * 5.0).sin() as f32 * 0.5 + 0.5;
        painter.add(egui::Shape::convex_polygon(pts, col.gamma_multiply(0.18 + 0.08 * pulse), Stroke::new(2.0, col)));
        super::openspace_cards::paint_action_icon(&painter, action, iso.at(hx, hy), 22.0, time);
    }

    // The top: the rival, the time, the share destroyed, the stars.
    let gold = Color32::from_rgb(255, 196, 64);
    let bar = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 36.0), Vec2::new(420.0, 52.0));
    painter.rect_filled(bar, 12.0, Color32::from_black_alpha(150));
    painter.text(bar.left_center() + Vec2::new(14.0, -10.0), Align2::LEFT_CENTER, match &f.target {
        Target::Campaign => format!("{}. {}", f.level, war::rival_name(f.level)),
        Target::World { name, .. } | Target::Quest { name, .. } => name.clone(),
    }, FontId::proportional(14.0), Color32::WHITE);
    painter.text(bar.left_center() + Vec2::new(14.0, 11.0), Align2::LEFT_CENTER, format!("{} {}", tx(("Temps", "Time"), fr), clock(TIME_LIMIT - f.b.time)), FontId::proportional(12.0), Color32::from_white_alpha(200));
    painter.text(bar.center() + Vec2::new(40.0, 0.0), Align2::CENTER_CENTER, format!("{:.0} %", f.b.destroyed() * 100.0), FontId::proportional(22.0), Color32::WHITE);
    let stars = f.b.stars();
    for k in 0..3u8 {
        painter.text(bar.right_center() + Vec2::new(-74.0 + k as f32 * 24.0, 0.0), Align2::CENTER_CENTER, if k < stars { "★" } else { "☆" }, FontId::proportional(24.0), if k < stars { gold } else { Color32::from_white_alpha(120) });
    }

    // The bottom: the troops to drop.
    if f.result.is_none() {
        let n = f.b.left.len().max(1) as f32 + f.b.cards.len() as f32 * 0.85 + if f.b.cards.is_empty() { 0.0 } else { 0.2 };
        let w = (n * 74.0 + 12.0).min(rect.width() - 260.0);
        let tray = Rect::from_center_size(Pos2::new(rect.center().x, rect.max.y - 50.0), Vec2::new(w.max(200.0), 84.0));
        ui.interact(tray, ui.id().with("os-tray"), Sense::click());
        painter.rect_filled(tray, 14.0, Color32::from_black_alpha(160));
        if f.b.left.is_empty() {
            painter.text(tray.center(), Align2::CENTER_CENTER, tx(("Plus de troupes à poser", "No troops left to drop"), fr), FontId::proportional(13.0), Color32::from_white_alpha(200));
        }
        for (k, (t, level, count)) in f.b.left.clone().into_iter().enumerate() {
            let r = Rect::from_min_size(tray.min + Vec2::new(8.0 + k as f32 * 74.0, 8.0), Vec2::new(68.0, 68.0));
            let resp = ui.interact(r, ui.id().with(("os-troop", k)), Sense::click());
            let on = f.pick == Some(t) && f.card.is_none();
            painter.rect_filled(r, 10.0, if on { theme.accent.gamma_multiply(0.5) } else { Color32::from_white_alpha(if resp.hovered() { 30 } else { 14 }) });
            if on {
                painter.rect_stroke(r, 10.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
            }
            portrait(&painter, t, r.shrink(10.0), time);
            painter.text(r.right_top() + Vec2::new(-6.0, 6.0), Align2::RIGHT_TOP, format!("×{count}"), FontId::proportional(13.0), if count > 0 { Color32::WHITE } else { Color32::from_white_alpha(90) });
            painter.text(r.left_bottom() + Vec2::new(6.0, -5.0), Align2::LEFT_BOTTOM, format!("{level}"), FontId::proportional(11.0), gold);
            if resp.clicked() && count > 0 {
                f.pick = Some(t);
                f.card = None;
            }
            resp.on_hover_text(tx(t.about(), fr));
        }
        // The action cards, after the troops: each played once.
        let x0 = tray.min.x + 8.0 + f.b.left.len() as f32 * 74.0 + 14.0;
        for (k, (action, level, used)) in f.b.cards.clone().into_iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(x0 + k as f32 * 63.0, tray.min.y + 6.0), Vec2::new(56.0, 72.0));
            let resp = ui.interact(r, ui.id().with(("os-card", k)), Sense::click());
            let on = f.card == Some(k);
            let col = super::openspace_cards::action_color(action);
            let lift = if on { -6.0 } else if resp.hovered() && !used { -3.0 } else { 0.0 };
            let r = r.translate(Vec2::new(0.0, lift));
            painter.rect_filled(r, 8.0, if used { Color32::from_rgb(45, 45, 50) } else { Color32::from_rgb(40, 42, 54) });
            painter.rect_stroke(r, 8.0, Stroke::new(if on { 3.0 } else { 2.0 }, if used { Color32::from_white_alpha(40) } else { col }), egui::StrokeKind::Inside);
            if used {
                super::openspace_cards::paint_action_icon(&painter.with_clip_rect(r), action, r.center() - Vec2::new(0.0, 6.0), 22.0, 0.0);
                painter.rect_filled(r, 8.0, Color32::from_black_alpha(140));
                painter.text(r.center(), Align2::CENTER_CENTER, "✔", FontId::proportional(20.0), Color32::from_white_alpha(140));
            } else {
                super::openspace_cards::paint_action_icon(&painter, action, r.center() - Vec2::new(0.0, 8.0), 24.0, time);
                painter.text(r.center_bottom() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, super::openspace_cards::action_short(action, fr), FontId::proportional(9.5), Color32::WHITE);
                painter.text(r.left_top() + Vec2::new(5.0, 4.0), Align2::LEFT_TOP, format!("{level}"), FontId::proportional(10.0), gold);
            }
            if resp.clicked() && !used {
                f.card = if on { None } else { Some(k) };
            }
            resp.on_hover_text(format!("{}\n{}", tx(super::openspace_cards::action_card(action).name, fr), action.text(level, fr)));
        }
        if carding {
            let hint = tx(("Clique où jouer la carte · clic droit ou Échap pour annuler", "Click where to play the card · right click or Esc to cancel"), fr);
            let g = painter.layout_no_wrap(hint.to_owned(), FontId::proportional(13.0), Color32::WHITE);
            let r = Rect::from_center_size(Pos2::new(rect.center().x, tray.min.y - 22.0), g.size() + Vec2::new(24.0, 12.0));
            painter.rect_filled(r, 10.0, Color32::from_black_alpha(170));
            painter.galley(r.min + Vec2::new(12.0, 6.0), g, Color32::WHITE);
        }
        // End now, the speed.
        let end = Rect::from_min_size(Pos2::new(rect.max.x - 170.0, rect.max.y - 58.0), Vec2::new(156.0, 40.0));
        let label = if f.b.started { tx(("Terminer le raid", "End the raid"), fr) } else { tx(("Rentrer", "Go back"), fr) };
        if ui.put(end, egui::Button::new(egui::RichText::new(label).size(14.0).color(Color32::WHITE)).fill(Color32::from_rgb(170, 50, 50)).corner_radius(10.0)).clicked() {
            if !f.b.started {
                return true;
            }
            f.b.time = TIME_LIMIT;
        }
        let sp = Rect::from_min_size(Pos2::new(rect.min.x + 16.0, rect.max.y - 58.0), Vec2::new(70.0, 40.0));
        if ui.put(sp, egui::Button::new(egui::RichText::new(format!("▶ ×{}", f.speed as u32)).size(14.0)).corner_radius(10.0)).on_hover_text(tx(("Vitesse du raid", "Raid speed"), fr)).clicked() {
            f.speed = match f.speed as u32 {
                1 => 2.0,
                2 => 4.0,
                _ => 1.0,
            };
        }
        // The hero's « Crunch mode », once they're on the field.
        if !f.b.crunched
            && let Some(h) = f.b.hero()
        {
            let (heal, secs) = war::crunch_power(h.level);
            let at = Rect::from_min_size(Pos2::new(rect.min.x + 16.0, rect.max.y - 108.0), Vec2::new(170.0, 42.0));
            let pulse = (time * 4.0).sin() as f32 * 0.5 + 0.5;
            let gold = Color32::from_rgb(255, 196, 64);
            let button = egui::Button::new(egui::RichText::new(format!("⚡ {}", tx(("Mode crunch", "Crunch mode"), fr))).size(14.5).strong().color(Color32::from_rgb(40, 20, 0)))
                .fill(gold.gamma_multiply(0.8 + 0.2 * pulse))
                .stroke(Stroke::new(2.0, Color32::WHITE.gamma_multiply(0.4 + 0.6 * pulse)))
                .corner_radius(12.0);
            let tip = format!(
                "{} {:.0} % {}, {} {:.0} s {} ({} {:.1} {}).",
                tx(("Ton Lead Dev récupère", "Your lead dev gets back"), fr),
                heal * 100.0,
                tx(("de ses PV", "of their HP"), fr),
                tx(("et fonce avec les troupes autour pendant", "and rushes with the troops around for"), fr),
                secs,
                tx(("(plus vite, plus fort)", "(faster, harder)"), fr),
                tx(("rayon", "radius"), fr),
                war::CRUNCH_RADIUS,
                tx(("cases", "tiles"), fr)
            );
            if ui.put(at, button).on_hover_text(tip).clicked() {
                f.b.crunch();
            }
        }
        if !f.b.started {
            let hint = tx(("Choisis une troupe en bas, puis clique (ou reste appuyé) autour de leur open space, hors de la zone rouge.", "Pick a troop at the bottom, then click (or hold) around their open space, outside the red area."), fr);
            let g = painter.layout_no_wrap(hint.to_owned(), FontId::proportional(13.0), Color32::WHITE);
            let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 88.0), g.size() + Vec2::new(24.0, 14.0));
            painter.rect_filled(r, 10.0, Color32::from_black_alpha(170));
            painter.galley(r.min + Vec2::new(12.0, 7.0), g, Color32::WHITE);
        }
        return false;
    }

    // The end: the stars, what it brought.
    let mut leave = false;
    if let Some(o) = &f.result {
        egui::Window::new("os-result")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(2.0, gold)).corner_radius(16.0).inner_margin(24.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(340.0);
                ui.vertical_centered(|ui| {
                    let title = match o.stars {
                        0 => tx(("Raid raté…", "Raid failed…"), fr),
                        3 => tx(("Victoire totale !", "Total victory!"), fr),
                        _ => tx(("Victoire !", "Victory!"), fr),
                    };
                    ui.label(egui::RichText::new(title).size(22.0).strong().color(if o.stars > 0 { gold } else { theme.text }));
                    let st: String = (0..3).map(|k| if k < o.stars { '★' } else { '☆' }).collect();
                    ui.label(egui::RichText::new(st).size(40.0).color(gold));
                    ui.label(egui::RichText::new(format!("{:.0} % {}", f.b.destroyed() * 100.0, tx(("détruit", "destroyed"), fr))).size(14.0).color(theme.text_muted));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("+{}   +{}", os::amount_text(o.coffee, os::Res::Coffee, fr), os::amount_text(o.budget, os::Res::Budget, fr))).size(15.0).color(theme.text));
                    if o.tickets > 0 {
                        ui.label(egui::RichText::new(format!("+{} 🎫 {}", o.tickets, tx(("pour tes 3 étoiles !", "for your 3 stars!"), fr))).size(14.0).color(Color32::from_rgb(0xe2, 0x6b, 0xd8)));
                    }
                    if !o.cards.is_empty() {
                        let text = if o.cards.len() == 1 { tx(("+1 carte à découvrir !", "+1 card to turn over!"), fr).to_owned() } else { format!("+{} {}", o.cards.len(), tx(("cartes à découvrir !", "cards to turn over!"), fr)) };
                        ui.label(egui::RichText::new(format!("♠ {text}")).size(14.0).color(Color32::from_rgb(255, 196, 64)));
                    }
                    if o.back > 0 {
                        let text = if o.back == 1 { tx(("1 troupe rentre au bureau", "1 troop comes back to the office"), fr).to_owned() } else { format!("{} {}", o.back, tx(("troupes rentrent au bureau", "troops come back to the office"), fr)) };
                        ui.label(egui::RichText::new(text).size(12.5).color(theme.text_muted));
                    }
                    if f.b.hero_fell() {
                        let back = os::duration_text(war::hero_rest_secs(s.hero_level()), fr);
                        ui.label(egui::RichText::new(format!("{} {back}", tx(("Ton Lead Dev est tombé : il se repose, de retour dans", "Your lead dev fell: resting, back in"), fr))).size(12.5).color(theme.ansi[3]));
                    }
                    if f.target != Target::Campaign {
                        world_paid(ui, f, fr, theme);
                    }
                    if o.stars > 0 && f.target == Target::Campaign && f.level < LEVELS {
                        ui.label(egui::RichText::new(format!("{} {}", tx(("Niveau suivant débloqué :", "Next level unlocked:"), fr), war::rival_name(f.level + 1))).size(12.5).color(theme.ansi[2]));
                    }
                    ui.add_space(12.0);
                    if ui.add(egui::Button::new(egui::RichText::new(tx(("Retour à mon open space", "Back to my open space"), fr)).size(14.0).color(theme.bg)).fill(theme.accent).corner_radius(8.0)).clicked() {
                        leave = true;
                    }
                });
            });
    }
    leave
}

/// A world raid's end: what floor counted (trophies, loot), or that it's coming.
fn world_paid(ui: &mut Ui, f: &Fight, fr: bool, theme: &Theme) {
    match &f.paid {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(tx(("Le monde compte ton raid…", "The world is counting your raid…"), fr)).size(13.0).color(theme.text_muted));
            });
        }
        Some(Err(why)) => {
            ui.label(egui::RichText::new(why).size(12.5).color(theme.ansi[1]));
        }
        Some(Ok(p)) => {
            if let Some((won, total)) = p.trophies {
                let color = if won >= 0 { Color32::from_rgb(255, 196, 64) } else { theme.ansi[1] };
                ui.label(egui::RichText::new(format!("{}{won} 🏆   ({} {total})", if won >= 0 { "+" } else { "" }, tx(("total", "total"), fr))).size(16.0).strong().color(color));
            }
            if p.coffee >= 1.0 || p.budget >= 1.0 {
                ui.label(egui::RichText::new(format!("+{}   +{}", os::amount_text(p.coffee, os::Res::Coffee, fr), os::amount_text(p.budget, os::Res::Budget, fr))).size(15.0).color(theme.text));
            }
            if p.tickets > 0 {
                ui.label(egui::RichText::new(format!("+{} 🎫", p.tickets)).size(14.0).color(Color32::from_rgb(0xe2, 0x6b, 0xd8)));
            }
            if !p.troops.is_empty() {
                let list: Vec<String> = p.troops.iter().map(|(t, n)| format!("{n} {}", tx(t.name(), fr))).collect();
                ui.label(egui::RichText::new(format!("+ {}", list.join(", "))).size(13.0).color(theme.text));
            }
        }
    }
}
