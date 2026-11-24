//! Open Space, in the games (its rules in `crate::openspace`, drawn by `openspace_scene`): the open
//! space filling the page, the camera moved by a drag and zoomed by the wheel; the resources, the
//! interns and the seniority at the top; the shop (« Construire ») on the right; the building
//! clicked in a card at the bottom (upgrade, collect, cancel, finish with tickets), moved by a drag.
//!
//! The open space runs whatever page is shown (`openspace_tick`, each frame): its works end and its
//! machines make in real time, even with Ronnie closed (caught up at the next opening). It is put in
//! the settings (sealed with the rewards, synced with floor) at each of the player's actions and
//! every minute.

use super::four::HomeGame;
use super::openspace_scene::{self as scene, Iso, Overlay, View};
use super::*;
use crate::openspace::{self as os, tx, Group, Kind, OpenSpace, Res, Summary, Why, GROUPS, KINDS};

/// The open space put in the settings at least this often (seconds).
const WRITE_EVERY: f64 = 60.0;

/// What a drag on the open space does.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Pan,
    /// Moving building `id`, grabbed `(dx, dy)` tiles from its corner.
    Move { id: u32, dx: f32, dy: f32 },
}

pub(super) struct Space {
    /// The open space running.
    live: Option<OpenSpace>,
    /// What was last put in the settings (another value there: synced, or edited elsewhere).
    written: Option<OpenSpace>,
    wrote_at: f64,
    /// Caught up once since Ronnie opened.
    started: bool,
    /// What happened while away, to show.
    away: Option<Summary>,
    view: View,
    selected: Option<u32>,
    /// A building being placed, from the shop.
    placing: Option<Kind>,
    shop: bool,
    drag: Drag,
    /// Where the building being moved would go, and whether it can.
    moving: Option<(u32, i32, i32, bool)>,
    /// A refusal to show (why), and since when.
    said: Option<(String, f64)>,
    scene: scene::Scene,
    /// A kind just built for the first time: its card, to show.
    discovered: Option<Kind>,
    /// Where the shop's rows and the « Build » button were drawn (the tutorial points at them).
    shop_rects: Vec<(Kind, Rect)>,
    build_rect: Rect,
    army_rect: Rect,
    attack_rect: Rect,
    /// The side panel open: 0 none, 1 the army, 2 the campaign, 3 the cards, 4 the achievements.
    panel: u8,
    /// A raid under way.
    fight: Option<super::openspace_raid::Fight>,
    /// Cards just found, being turned over (since when).
    reveal: Option<(Vec<usize>, f64)>,
    cards_rect: Rect,
    /// The mission's card folded (just its title).
    mission_folded: bool,
}

impl Default for Space {
    fn default() -> Self {
        Self { live: None, written: None, wrote_at: 0.0, started: false, away: None, view: View { zoom: 1.15, pan: Vec2::ZERO }, selected: None, placing: None, shop: false, drag: Drag::None, moving: None, said: None, scene: Default::default(), discovered: None, shop_rects: Vec::new(), build_rect: Rect::NOTHING, army_rect: Rect::NOTHING, attack_rect: Rect::NOTHING, panel: 0, fight: None, reveal: None, cards_rect: Rect::NOTHING, mission_folded: false }
    }
}

/// A pulsing ring around `r`, and an arrow bouncing above it: « here ».
fn point_at(painter: &egui::Painter, r: Rect, time: f64, color: Color32) {
    let pulse = ((time * 4.0).sin() as f32 * 0.5 + 0.5) * 4.0;
    painter.rect_stroke(r.expand(3.0 + pulse), 10.0, Stroke::new(3.0, color), egui::StrokeKind::Outside);
    arrow_at(painter, r.center_top() - Vec2::new(0.0, 6.0 + pulse), time, color);
}

/// An arrow pointing down at `tip`, bouncing.
fn arrow_at(painter: &egui::Painter, tip: Pos2, time: f64, color: Color32) {
    let bob = ((time * 5.0).sin() as f32 * 0.5 + 0.5) * 8.0;
    let tip = tip - Vec2::new(0.0, bob);
    painter.add(egui::Shape::convex_polygon(vec![tip, tip + Vec2::new(-13.0, -16.0), tip + Vec2::new(13.0, -16.0)], color, Stroke::new(1.5, Color32::from_black_alpha(120))));
    painter.rect_filled(Rect::from_min_max(tip + Vec2::new(-5.0, -32.0), tip + Vec2::new(5.0, -15.0)), 2.0, color);
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Its icon: a small building in volume, seen from above at an angle.
pub(super) fn paint_openspace_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = scale;
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let top = vec![at(0.0, -11.0), at(9.0, -6.5), at(0.0, -2.0), at(-9.0, -6.5)];
    let left = vec![at(-9.0, -6.5), at(0.0, -2.0), at(0.0, 10.0), at(-9.0, 5.5)];
    let right = vec![at(0.0, -2.0), at(9.0, -6.5), at(9.0, 5.5), at(0.0, 10.0)];
    painter.add(egui::Shape::convex_polygon(left, theme.accent.gamma_multiply(0.8), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(right, theme.accent.gamma_multiply(0.55), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(top, theme.accent, Stroke::NONE));
    for k in 0..2 {
        for row in 0..2 {
            let (x, y) = (-6.5 + k as f32 * 3.5, 1.5 + row as f32 * 4.0 + k as f32 * 1.75);
            painter.add(egui::Shape::convex_polygon(vec![at(x, y), at(x + 2.2, y + 1.1), at(x + 2.2, y + 3.3), at(x, y + 2.2)], theme.bg, Stroke::NONE));
        }
    }
}

fn button(ui: &mut Ui, enabled: bool, text: impl Into<String>, theme: &Theme) -> bool {
    let b = egui::Button::new(egui::RichText::new(text.into()).size(13.0).color(if enabled { theme.bg } else { theme.text_muted })).fill(if enabled { theme.accent } else { theme.tab_hover }).corner_radius(6.0);
    ui.add_enabled(enabled, b).clicked()
}

fn small(ui: &mut Ui, text: impl Into<String>) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(text.into()).size(12.5)).corner_radius(6.0)).clicked()
}

/// A card over the open space: it takes the clicks under it (not the open space).
fn panel<R>(ui: &mut Ui, rect: Rect, id: &str, theme: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.interact(rect, ui.id().with(("os-panel", id)), Sense::click_and_drag());
    ui.painter().rect_filled(rect.translate(Vec2::new(0.0, 3.0)), 12.0, Color32::from_black_alpha(60));
    ui.painter().rect_filled(rect, 12.0, theme.chrome_bg.gamma_multiply(0.97));
    ui.painter().rect_stroke(rect, 12.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(12.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    add(&mut child)
}

impl App {
    pub(super) fn open_openspace(&mut self) {
        self.open_game();
        self.home_which = HomeGame::OpenSpace;
    }

    fn os_fr(&self) -> bool {
        self.config.settings.language == Lang::Fr
    }

    /// Beside its name in the games' list: the boss's office level; a hammer when an intern is free.
    pub(super) fn openspace_note(&self) -> Option<String> {
        let s = self.openspace.live.as_ref()?;
        let free = s.busy() < s.interns();
        Some(format!("{} {}{}", tx(("QG", "HQ"), self.os_fr()), s.hq_level(), if free { " · 🔨" } else { "" }))
    }

    /// Each frame, whatever is shown: the open space run to now, and put in the settings when due.
    pub(super) fn openspace_tick(&mut self, ctx: &egui::Context) {
        let saved = &self.config.settings.openspace;
        if saved.is_none() && self.openspace.live.is_none() {
            return;
        }
        let now = unix_now();
        let time = ctx.input(|i| i.time);
        // Brought by a sync (or the config edited): taken, and run from where it was.
        if self.openspace.written.as_ref() != saved.as_ref() {
            self.openspace.live = saved.clone();
            self.openspace.written = saved.clone();
            self.openspace.wrote_at = time;
        }
        let first = !self.openspace.started;
        self.openspace.started = true;
        let Some(s) = self.openspace.live.as_mut() else { return };
        if now > s.at {
            let sum = s.run(now);
            if first && sum.secs >= 600.0 && (sum.coffee >= 1.0 || sum.budget >= 1.0 || !sum.finished.is_empty()) {
                self.openspace.away = Some(sum);
            }
        }
        let acted = self.openspace.written.as_ref().map(|w| w.actions) != Some(s.actions);
        if acted || time - self.openspace.wrote_at >= WRITE_EVERY {
            self.openspace_write(time);
        }
        // Kept running now and then (a work ending), even with nothing else to draw.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }

    fn openspace_write(&mut self, time: f64) {
        let Some(s) = self.openspace.live.clone() else { return };
        // A reward just won (the drone, the frame): said, as the other games do.
        if let Some(w) = &self.openspace.written {
            use super::openspace_drone::{DRONE_STARS, FRAME_HQ};
            let t = self.t();
            let stars = |o: &OpenSpace| o.campaign.iter().map(|&n| n as u64).sum::<u64>();
            let mut won: Vec<String> = Vec::new();
            if stars(w) < DRONE_STARS && stars(&s) >= DRONE_STARS {
                won.push(t.unlock_drone.to_owned());
            }
            if w.hq_level() < FRAME_HQ && s.hq_level() >= FRAME_HQ {
                won.push(t.unlock_frame.replace("{n}", super::frames::frame_name(t, "firewall")));
            }
            if !won.is_empty() {
                self.push_toast(super::Toast { ok: true, title: t.openspace_reward_title.to_owned(), body: format!("{}\n{}", won.join(" · "), t.corp_reward_where), tab: self.active, at: std::time::Instant::now(), mail: None, notice: 0, lead: None });
            }
        }
        self.config.settings.openspace = Some(s.clone());
        self.openspace.written = Some(s);
        self.openspace.wrote_at = time;
        self.save_config();
    }

    pub(super) fn openspace_page(&mut self, ui: &mut Ui, rect: Rect) {
        self.fold_for_game(false);
        let t = self.t();
        let fr = self.os_fr();
        let theme = self.theme.clone();
        let time = ui.input(|i| i.time);
        let unix = unix_now();
        ui.ctx().request_repaint();

        let Some(mut s) = self.openspace.live.clone() else {
            ui.painter_at(rect).rect_filled(rect, 0.0, theme.bg);
            if self.openspace_welcome(ui, rect, fr, &theme) {
                let seed = (unix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (time.to_bits());
                self.openspace.live = Some(OpenSpace::new(unix, seed));
                self.openspace_write(time);
            }
            self.openspace_back(ui, rect, t);
            return;
        };

        // A raid under way: its page only.
        if let Some(fight) = self.openspace.fight.as_mut() {
            if super::openspace_raid::fight_page(ui, rect, fight, &mut s, fr, &theme) {
                let found = fight.result.as_ref().map(|o| o.cards.clone()).unwrap_or_default();
                if !found.is_empty() {
                    self.openspace.reveal = Some((found, time));
                }
                self.openspace.fight = None;
            }
            if self.openspace.live.as_ref() != Some(&s) {
                self.openspace.live = Some(s);
            }
            return;
        }

        let painter = ui.painter_at(rect);
        let st = &mut self.openspace;
        let map = ui.interact(rect, ui.id().with("os-map"), Sense::click_and_drag());
        let pointer = map.hover_pos().or(ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)));

        // The wheel (or a pinch) zooms where the pointer is.
        if let Some(p) = map.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.0018).exp();
            if (factor - 1.0).abs() > 1e-4 {
                let old = st.view.zoom;
                let new = (old * factor).clamp(0.3, 2.4);
                let d = p - (rect.center() + st.view.pan);
                st.view.pan += d - d * (new / old);
                st.view.zoom = new;
            }
        }
        let iso = Iso::new(rect, &st.view);

        // What's under the pointer, and under the press: the building in front first, a bubble over all.
        let press = map.interact_pointer_pos();
        let (hover, picked, bubbled) = {
            let mut front: Vec<&os::Building> = s.buildings.iter().collect();
            front.sort_by_key(|b| std::cmp::Reverse((b.x + b.y + b.kind.size(), b.x)));
            let pick = |p: Pos2| front.iter().find(|b| scene::inside(&scene::outline(&iso, b), p)).map(|b| b.id);
            let bubble = |p: Pos2| s.buildings.iter().filter(|b| scene::ripe(b)).find(|b| scene::bubble_at(&iso, b, time).distance(p) < 16.0 * iso.k().clamp(0.7, 1.3)).map(|b| b.id);
            (pointer.and_then(|p| bubble(p).or_else(|| pick(p))), press.and_then(pick), press.and_then(bubble))
        };

        // The building being placed follows the pointer.
        let ghost = st.placing.zip(pointer).map(|(kind, p)| {
            let (gx, gy) = iso.back(p);
            let half = kind.size() as f32 / 2.0;
            let (x, y) = ((gx - half).round() as i32, (gy - half).round() as i32);
            (kind, x, y, s.fits(kind, x, y, None) && s.can_build(kind).is_ok())
        });

        // Dragging: the selected building moves, anything else moves the camera.
        if map.drag_started()
            && let Some(p) = map.interact_pointer_pos()
        {
            st.drag = match (st.placing, st.selected, picked) {
                (None, Some(sel), Some(id)) if sel == id && !s.get(id).is_some_and(|b| b.kind.is_obstacle()) => {
                    let b = s.get(id).cloned();
                    let (gx, gy) = iso.back(p);
                    b.map_or(Drag::Pan, |b| Drag::Move { id, dx: gx - b.x as f32, dy: gy - b.y as f32 })
                }
                _ => Drag::Pan,
            };
        }
        if map.dragged() {
            match st.drag {
                Drag::Pan => st.view.pan += map.drag_delta(),
                Drag::Move { id, dx, dy } => {
                    if let (Some(p), Some(b)) = (map.interact_pointer_pos(), s.get(id)) {
                        let (gx, gy) = iso.back(p);
                        let (x, y) = ((gx - dx).round() as i32, (gy - dy).round() as i32);
                        st.moving = Some((id, x, y, s.fits(b.kind, x, y, Some(id))));
                    }
                }
                Drag::None => {}
            }
        }
        if map.drag_stopped() {
            if let Some((id, x, y, true)) = st.moving {
                s.move_to(id, x, y);
            }
            st.moving = None;
            st.drag = Drag::None;
        }

        if map.clicked() {
            if let Some((kind, x, y, _)) = ghost {
                match s.build(kind, x, y, unix) {
                    Ok(id) => {
                        if s.discover(kind) {
                            st.discovered = Some(kind);
                        }
                        // Firewalls one after the other; anything else: placed, selected.
                        if kind != Kind::Wall || s.can_build(Kind::Wall).is_err() {
                            st.placing = None;
                            st.selected = Some(id);
                        }
                    }
                    Err(why) => st.said = Some((why.text(fr), time)),
                }
            } else if let Some(id) = bubbled {
                let took = s.collect(id);
                if let Some(b) = s.get(id) {
                    let half = b.kind.size() as f32 / 2.0;
                    let r = b.kind.makes().unwrap_or(Res::Coffee);
                    if took >= 1.0 {
                        st.scene.float(format!("+{}", os::amount_text(took, r, fr)), scene::res_color(r), (b.x as f32 + half, b.y as f32 + half), time);
                    } else {
                        st.said = Some((tx(("Plus de place : améliore tes frigos et tes coffres-forts", "No room left: upgrade your fridges and safes"), fr).into(), time));
                    }
                }
            } else {
                st.selected = picked;
            }
        }
        if map.secondary_clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if st.placing.is_some() {
                st.placing = None;
            } else {
                st.selected = None;
                st.shop = false;
            }
        }
        if hover.is_some() && st.placing.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if st.drag == Drag::Pan && map.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }

        let overlay = Overlay { selected: st.selected, ghost, moving: st.moving, hover };
        scene::paint(&painter, rect, &st.view, &s, &overlay, &mut st.scene, &theme, time, unix);

        // The top: back, the name, the resources.
        self.openspace_back(ui, rect, t);
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 30.0), Align2::CENTER_CENTER, t.openspace_name, 28.0, theme.accent, 1.0);
        self.openspace_hud(ui, rect, &mut s, fr, &theme, time);
        // What the game unlocks, as the other games: the pill in the bottom left corner.
        self.unlocks_pill(ui, rect, HomeGame::OpenSpace);

        // The shop, the building clicked, the time away.
        let st = &mut self.openspace;
        if st.shop {
            let shop = Rect::from_min_max(Pos2::new(rect.max.x - 330.0, rect.min.y + 96.0), Pos2::new(rect.max.x - 14.0, rect.max.y - 70.0));
            let rects = &mut st.shop_rects;
            rects.clear();
            if let Some(kind) = panel(ui, shop, "shop", &theme, |ui| shop_ui(ui, &s, fr, &theme, time, rects)) {
                st.placing = Some(kind);
                st.shop = false;
                st.selected = None;
            }
        }
        let build = Rect::from_min_size(Pos2::new(rect.max.x - 190.0, rect.max.y - 58.0), Vec2::new(176.0, 42.0));
        st.build_rect = build;
        let label = if st.placing.is_some() { format!("✕  {}", tx(("Arrêter de poser", "Stop placing"), fr)) } else { format!("🔨  {}", tx(("Construire", "Build"), fr)) };
        if ui.put(build, egui::Button::new(egui::RichText::new(label).size(15.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(10.0)).clicked() {
            if st.placing.is_some() {
                st.placing = None;
            } else {
                st.shop = !st.shop;
                st.panel = 0;
            }
        }
        // The army, the campaign: in the same place as the shop.
        let attack = Rect::from_min_size(Pos2::new(build.min.x - 150.0, build.min.y), Vec2::new(140.0, 42.0));
        let army = Rect::from_min_size(Pos2::new(attack.min.x - 130.0, build.min.y), Vec2::new(120.0, 42.0));
        st.attack_rect = attack;
        st.army_rect = army;
        if ui.put(attack, egui::Button::new(egui::RichText::new(format!("⚔  {}", tx(("Attaquer", "Attack"), fr))).size(15.0).strong().color(Color32::WHITE)).fill(Color32::from_rgb(190, 60, 50)).corner_radius(10.0)).clicked() {
            st.panel = if st.panel == 2 { 0 } else { 2 };
            st.shop = false;
            st.placing = None;
        }
        let army_label = format!("👥  {} {}", tx(("Armée", "Army"), fr), s.army_size());
        if ui.put(army, egui::Button::new(egui::RichText::new(army_label).size(14.0).strong()).corner_radius(10.0)).clicked() {
            st.panel = if st.panel == 1 { 0 } else { 1 };
            st.shop = false;
            st.placing = None;
        }
        // The cards; a dot when the mail came.
        let cards = Rect::from_min_size(Pos2::new(army.min.x - 120.0, build.min.y), Vec2::new(110.0, 42.0));
        st.cards_rect = cards;
        if ui.put(cards, egui::Button::new(egui::RichText::new(format!("♠  {}", tx(("Cartes", "Cards"), fr))).size(14.0).strong()).corner_radius(10.0)).clicked() {
            st.panel = if st.panel == 3 { 0 } else { 3 };
            st.shop = false;
            st.placing = None;
        }
        if unix >= s.mail_at() {
            let c = cards.right_top() + Vec2::new(-4.0, 4.0);
            let pulse = (time * 4.0).sin() as f32 * 0.5 + 0.5;
            ui.painter().circle_filled(c, 6.0 + pulse * 1.5, Color32::from_rgb(220, 60, 60));
        }
        let wide = if st.panel == 3 { 420.0 } else { 360.0 };
        let side = Rect::from_min_max(Pos2::new(rect.max.x - wide, rect.min.y + 96.0), Pos2::new(rect.max.x - 14.0, rect.max.y - 70.0));
        if st.panel == 1 {
            let mut said = None;
            panel(ui, side, "army", &theme, |ui| super::openspace_raid::army_ui(ui, &mut s, fr, &theme, unix, &mut said));
            if let Some(why) = said {
                st.said = Some((why.text(fr), time));
            }
        } else if st.panel == 2
            && let Some(level) = panel(ui, side, "campaign", &theme, |ui| super::openspace_raid::campaign_ui(ui, &s, fr, &theme))
        {
            st.fight = Some(super::openspace_raid::Fight::new(level, &s));
            st.panel = 0;
            st.selected = None;
        } else if st.panel == 3 {
            let found = panel(ui, side, "cards", &theme, |ui| super::openspace_cards::cards_ui(ui, &mut s, fr, &theme, unix));
            if !found.is_empty() {
                st.reveal = Some((found, time));
            }
        }
        if let Some(kind) = st.placing {
            let text = format!("{} — {}", tx(kind.name(), fr), tx(("clique pour poser · Échap ou clic droit pour arrêter", "click to place · Esc or right click to stop"), fr));
            let g = ui.painter().layout_no_wrap(text, FontId::proportional(13.0), Color32::WHITE);
            let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.max.y - 36.0), g.size() + Vec2::new(24.0, 14.0));
            ui.painter().rect_filled(r, 10.0, Color32::from_black_alpha(170));
            ui.painter().galley(r.min + Vec2::new(12.0, 7.0), g, Color32::WHITE);
        }
        if st.placing.is_none()
            && let Some(id) = st.selected
            && s.get(id).is_some()
        {
            let card = Rect::from_center_size(Pos2::new(rect.center().x, rect.max.y - 96.0), Vec2::new(500.0_f32.min(rect.width() - 230.0).max(320.0), 160.0));
            let card = card.translate(Vec2::new((rect.min.x + 14.0 - card.min.x).max(0.0), 0.0));
            let mut said = None;
            let close = panel(ui, card, "sel", &theme, |ui| selected_ui(ui, &mut s, id, fr, &theme, unix, &mut said));
            if let Some(why) = said {
                st.said = Some((why.text(fr), time));
            }
            if close {
                st.selected = None;
            }
        }
        if let Some((text, at)) = &st.said {
            let age = time - at;
            if age > 3.0 {
                st.said = None;
            } else {
                let a = (1.0 - ((age - 2.4) / 0.6).max(0.0)) as f32;
                let g = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(14.0), Color32::WHITE.gamma_multiply(a));
                let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 110.0), g.size() + Vec2::new(28.0, 16.0));
                ui.painter().rect_filled(r, 10.0, Color32::from_rgba_unmultiplied(170, 40, 40, (220.0 * a) as u8));
                ui.painter().galley(r.min + Vec2::new(14.0, 8.0), g, Color32::WHITE);
            }
        }
        // « ? »: the tutorial again.
        let help = Rect::from_min_size(rect.min + Vec2::new(134.0, 14.0), Vec2::new(28.0, 28.0));
        if ui.put(help, egui::Button::new(egui::RichText::new("?").size(14.0).strong()).corner_radius(14.0)).on_hover_text(tx(("Revoir le tutoriel", "See the tutorial again"), fr)).clicked() {
            s.tutorial_restart();
            st.discovered = None;
        }

        // A building built for the first time: what it is, how it works.
        if let Some(kind) = st.discovered
            && discovery_card(ui, kind, fr, &theme, time)
        {
            st.discovered = None;
        }

        // The tutorial, then the missions: a card on the left, an arrow on what to do.
        s.tutorial_advance();
        let gold = Color32::from_rgb(255, 196, 64);
        let mut waiting = false;
        let spot = if st.discovered.is_some() {
            None
        } else if let Some(step) = s.step() {
            Some(step.spot)
        } else if st.mission_folded {
            None
        } else {
            super::openspace_cards::mission_spot(&s)
        };
        if let Some(spot) = spot {
            match spot {
                os::Spot::Shop(kind) if st.placing != Some(kind) => {
                    if !st.shop {
                        point_at(ui.painter(), st.build_rect, time, gold);
                    } else if let Some((_, r)) = st.shop_rects.iter().find(|(k, _)| *k == kind) {
                        point_at(ui.painter(), *r, time, gold);
                    }
                }
                os::Spot::Building(kind) => {
                    if let Some(b) = s.buildings.iter().find(|b| b.kind == kind) {
                        let half = b.kind.size() as f32 / 2.0;
                        let tip = iso.up(iso.at(b.x as f32 + half, b.y as f32 + half), scene::height(b.kind, b.level.max(1)) + if b.job.is_some() { 60.0 } else { 34.0 });
                        arrow_at(ui.painter(), tip, time, gold);
                    }
                }
                os::Spot::Army if st.panel != 1 => point_at(ui.painter(), st.army_rect, time, gold),
                os::Spot::Attack if st.panel != 2 => point_at(ui.painter(), st.attack_rect, time, gold),
                os::Spot::Cards if st.panel != 3 => point_at(ui.painter(), st.cards_rect, time, gold),
                os::Spot::Bubble => match s.buildings.iter().find(|b| scene::ripe(b)) {
                    Some(b) => arrow_at(ui.painter(), scene::bubble_at(&iso, b, time) - Vec2::new(0.0, 18.0), time, gold),
                    None => waiting = true,
                },
                _ => {}
            }
        }
        if st.discovered.is_none()
            && s.step().is_none()
            && let Some(got) = super::openspace_cards::mission_ui(ui, rect.min + Vec2::new(16.0, 104.0), &mut s, &mut st.mission_folded, fr, &theme)
        {
            st.scene.float(tx(("Mission accomplie !", "Mission done!"), fr).to_owned(), gold, (os::GRID as f32 / 2.0, os::GRID as f32 / 2.0), time);
            if let Some(card) = got {
                st.reveal = Some((vec![card], time));
            }
        }
        if st.discovered.is_none()
            && let Some(step) = s.step()
        {
            let n = s.tuto as usize;
            let mut next = false;
            let mut skip = false;
            egui::Area::new(egui::Id::new("os-tuto"))
                .fixed_pos(rect.min + Vec2::new(16.0, 104.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(2.0, gold)).corner_radius(12.0).inner_margin(14.0).shadow(egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(90) }).show(ui, |ui| {
                        ui.set_width(310.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(tx(step.title, fr)).size(15.0).strong().color(gold));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(format!("{}/{}", n + 1, os::TUTORIAL.len())).size(11.5).color(theme.text_muted));
                            });
                        });
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(egui::RichText::new(tx(step.text, fr)).size(13.0).color(theme.text)).wrap());
                        if waiting {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(tx(("Patiente quelques secondes : la bulle apparaît dès que la machine a produit un peu.", "Wait a few seconds: the bubble shows as soon as the machine has made a little."), fr)).size(12.0).italics().color(theme.text_muted));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if step.done.is_none() {
                                next = button(ui, true, if n + 1 == os::TUTORIAL.len() { tx(("C'est parti !", "Let's go!"), fr) } else { tx(("Suivant", "Next"), fr) }, &theme);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                skip = ui.add(egui::Button::new(egui::RichText::new(tx(("Passer le tutoriel", "Skip the tutorial"), fr)).size(11.5).color(theme.text_muted)).frame(false)).clicked();
                            });
                        });
                    });
                });
            if next {
                s.tutorial_next();
            }
            if skip {
                s.tutorial_skip();
            }
        }

        if let Some(sum) = st.away.clone()
            && openspace_away(ui, &sum, fr, &theme)
        {
            st.away = None;
        }

        if let Some((found, since)) = st.reveal.clone()
            && super::openspace_cards::reveal(ui, rect, &found, since, &s, fr, &theme)
        {
            st.reveal = None;
        }

        if self.openspace.live.as_ref() != Some(&s) {
            self.openspace.live = Some(s);
        }
    }

    fn openspace_back(&mut self, ui: &mut Ui, rect: Rect, t: &Strings) {
        let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
        if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).corner_radius(6.0)).clicked() {
            self.home_game = false;
        }
    }

    /// The top right: coffee, budget, tickets, the interns, the seniority.
    fn openspace_hud(&mut self, ui: &mut Ui, rect: Rect, s: &mut OpenSpace, fr: bool, theme: &Theme, time: f64) {
        let painter = ui.painter();
        let mut x = rect.max.x - 14.0;
        let y = rect.min.y + 14.0;
        let chip = |painter: &egui::Painter, right: f32, w: f32| {
            let r = Rect::from_min_size(Pos2::new(right - w, y), Vec2::new(w, 34.0));
            painter.rect_filled(r.translate(Vec2::new(0.0, 2.0)), 17.0, Color32::from_black_alpha(50));
            painter.rect_filled(r, 17.0, theme.chrome_bg);
            painter.rect_stroke(r, 17.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
            r
        };
        // The resources: a bar of what's stored; a click collects every machine.
        for r in [Res::Budget, Res::Coffee] {
            let rc = chip(painter, x, 168.0);
            x = rc.min.x - 8.0;
            scene::paint_res(painter, r, Pos2::new(rc.min.x + 20.0, rc.center().y), 15.0);
            let (have, cap) = (s.amount(r), s.capacity(r));
            let bar = Rect::from_min_size(Pos2::new(rc.min.x + 38.0, rc.max.y - 11.0), Vec2::new(rc.width() - 52.0, 5.0));
            painter.rect_filled(bar, 2.5, theme.tab_hover);
            let frac = (have / cap).clamp(0.0, 1.0) as f32;
            painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * frac, 5.0)), 2.5, if frac >= 1.0 { theme.ansi[3] } else { scene::res_color(r) });
            painter.text(Pos2::new(rc.min.x + 38.0, rc.min.y + 12.0), Align2::LEFT_CENTER, os::short(have, fr), FontId::proportional(14.0), theme.text);
            painter.text(Pos2::new(rc.max.x - 14.0, rc.min.y + 12.0), Align2::RIGHT_CENTER, format!("/ {}", os::short(cap, fr)), FontId::proportional(11.0), theme.text_muted);
            let waiting: f64 = s.buildings.iter().filter(|b| b.kind.makes() == Some(r)).map(|b| b.stored).sum();
            let resp = ui.interact(rc, ui.id().with(("os-res", r as u8)), Sense::click()).on_hover_text(format!(
                "{}\n{} {}",
                tx(match r { Res::Coffee => ("Café : recrute et améliore les troupes. Stocké dans les frigos.", "Coffee: recruits and upgrades the troops. Stored in the fridges."), Res::Budget => ("Budget : construit et améliore les bâtiments. Stocké dans les coffres-forts.", "Budget: builds and upgrades the buildings. Stored in the safes.") }, fr),
                os::amount_text(waiting, r, fr),
                tx(("dans les machines : clique pour tout récolter", "in the machines: click to collect it all"), fr)
            ));
            if resp.clicked() {
                let took = s.collect_all(r);
                if took >= 1.0 {
                    self.openspace.said = None;
                }
            }
        }
        let rc = chip(painter, x, 82.0);
        x = rc.min.x - 8.0;
        scene::paint_ticket(painter, Pos2::new(rc.min.x + 20.0, rc.center().y), 14.0, rgb_ticket());
        painter.text(Pos2::new(rc.min.x + 36.0, rc.center().y), Align2::LEFT_CENTER, s.tickets.to_string(), FontId::proportional(14.0), theme.text);
        ui.interact(rc, ui.id().with("os-tickets"), Sense::hover()).on_hover_text(tx(("Tickets RH : rares. Ils finissent un chantier tout de suite. On en gagne avec les succès, les 3 étoiles de la campagne, et parfois sous les obstacles.", "HR tickets: rare. They finish a work at once. Earned with achievements, the campaign's 3 stars, and now and then under obstacles."), fr));
        let rc = chip(painter, x, 112.0);
        x = rc.min.x - 8.0;
        let (busy, all) = (s.busy(), s.interns());
        painter.circle_filled(Pos2::new(rc.min.x + 20.0, rc.center().y), 8.0, theme.ansi[3]);
        painter.text(Pos2::new(rc.min.x + 36.0, rc.center().y), Align2::LEFT_CENTER, format!("{}/{}", all - busy.min(all), all), FontId::proportional(14.0), if busy < all { theme.text } else { theme.text_muted });
        ui.interact(rc, ui.id().with("os-interns"), Sense::hover()).on_hover_text(tx(("Alternants libres / en tout : un chantier chacun. Un bureau des alternants de plus en donne un autre.", "Free interns / in all: one work each. Another interns' desk gives one more."), fr));
        // The seniority, on the left under the back button.
        let (level, have, need) = os::seniority(s.xp);
        let r = Rect::from_min_size(Pos2::new(rect.min.x + 16.0, rect.min.y + 52.0), Vec2::new(170.0, 40.0));
        painter.rect_filled(r, 10.0, theme.chrome_bg);
        painter.rect_stroke(r, 10.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let badge = Pos2::new(r.min.x + 20.0, r.center().y);
        painter.circle_filled(badge, 14.0, theme.accent);
        painter.text(badge, Align2::CENTER_CENTER, level.to_string(), FontId::proportional(13.0), theme.bg);
        painter.text(Pos2::new(r.min.x + 42.0, r.min.y + 13.0), Align2::LEFT_CENTER, tx(os::seniority_title(level), fr), FontId::proportional(13.0), theme.text);
        let bar = Rect::from_min_size(Pos2::new(r.min.x + 42.0, r.max.y - 12.0), Vec2::new(r.width() - 54.0, 5.0));
        painter.rect_filled(bar, 2.5, theme.tab_hover);
        painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * (have as f32 / need as f32), 5.0)), 2.5, theme.accent);
        ui.interact(r, ui.id().with("os-xp"), Sense::hover()).on_hover_text(format!("{} {have} / {need} XP\n{}", tx(("Séniorité :", "Seniority:"), fr), tx(("Chaque chantier terminé en donne (plus il est long, plus il en donne).", "Each work finished gives some (the longer, the more)."), fr)));
        let _ = (x, time);
    }

    /// The first time: what the game is, and a button to start. True: started.
    fn openspace_welcome(&mut self, ui: &mut Ui, rect: Rect, fr: bool, theme: &Theme) -> bool {
        let card = Rect::from_center_size(rect.center(), Vec2::new(520.0_f32.min(rect.width() - 40.0), 330.0));
        let time = ui.input(|i| i.time);
        panel(ui, card, "welcome", theme, |ui| {
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(120.0, 90.0), Sense::hover());
                scene::paint_mini(ui.painter(), Kind::Hq, r.center(), 110.0, time);
                ui.label(egui::RichText::new(tx(("Ton open space", "Your open space"), fr)).size(20.0).strong().color(theme.text));
                ui.add_space(6.0);
                ui.add(egui::Label::new(egui::RichText::new(tx(("Tu prends la tête d'un open space. Construis, fais monter de niveau, récolte le café et le budget, protège-toi derrière tes pare-feux… Les chantiers durent de quelques secondes à plusieurs jours, et tout continue même Ronnie fermé : reviens aux pauses ! Forme ton armée, attaque les boîtes concurrentes, collectionne les cartes… et bientôt, les raids entre collègues.", "You take over an open space. Build, level up, collect coffee and budget, shelter behind your firewalls… Works take from a few seconds to several days, and everything goes on even with Ronnie closed: come back at your breaks! Train your army, attack rival companies, collect the cards… and soon, raids between colleagues."), fr)).size(13.5).color(theme.text_muted)).wrap());
                ui.add_space(14.0);
                button(ui, true, tx(("Ouvrir mon open space", "Open my open space"), fr), theme)
            })
            .inner
        })
    }
}

fn rgb_ticket() -> Color32 {
    Color32::from_rgb(0xe2, 0x6b, 0xd8)
}

/// The shop: every kind by section, what it costs, how many; the one picked to place.
fn shop_ui(ui: &mut Ui, s: &OpenSpace, fr: bool, theme: &Theme, time: f64, rects: &mut Vec<(Kind, Rect)>) -> Option<Kind> {
    let mut picked = None;
    ui.label(egui::RichText::new(tx(("Construire", "Build"), fr)).size(17.0).strong().color(theme.text));
    ui.add_space(4.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (group, name) in GROUPS {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tx(name, fr)).size(12.0).strong().color(theme.text_muted));
            for kind in KINDS.iter().copied().filter(|k| k.group() == group && *k != Kind::Hq) {
                if let Some(k) = shop_row(ui, s, kind, fr, theme, time, rects) {
                    picked = Some(k);
                }
            }
        }
        let _ = Group::Economy;
    });
    picked
}

fn shop_row(ui: &mut Ui, s: &OpenSpace, kind: Kind, fr: bool, theme: &Theme, time: f64, rects: &mut Vec<(Kind, Rect)>) -> Option<Kind> {
    let width = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(Vec2::new(width, 62.0), Sense::click());
    // Seen only when in the shop's visible part.
    if ui.clip_rect().contains_rect(r) {
        rects.push((kind, r));
    }
    let can = s.can_build(kind);
    let ok = can.is_ok();
    let hot = resp.hovered() && ok;
    let p = ui.painter();
    p.rect_filled(r.shrink2(Vec2::new(0.0, 2.0)), 8.0, if hot { theme.tab_active } else { theme.tab_hover.gamma_multiply(0.6) });
    scene::paint_mini(p, kind, Pos2::new(r.min.x + 32.0, r.center().y), 56.0, time);
    let (count, _) = kind.allowed(s.hq_level());
    let left = r.min.x + 66.0;
    let name_c = if ok { theme.text } else { theme.text_muted };
    p.text(Pos2::new(left, r.min.y + 14.0), Align2::LEFT_CENTER, tx(kind.name(), fr), FontId::proportional(13.5), name_c);
    p.text(Pos2::new(r.max.x - 8.0, r.min.y + 14.0), Align2::RIGHT_CENTER, format!("{}/{}", s.count(kind), count), FontId::proportional(12.0), theme.text_muted);
    let (res, price) = os::cost(kind, 1, s.count(kind));
    let secs = os::build_time(kind, 1);
    let line2 = format!("{}  ·  {}", if price == 0 { tx(("Gratuit", "Free"), fr).to_owned() } else { os::amount_text(price as f64, res, fr) }, if secs == 0 { tx(("immédiat", "at once"), fr).to_owned() } else { os::duration_text(secs as i64, fr) });
    let price_c = if matches!(can, Err(Why::Short(..))) { theme.ansi[1] } else { theme.text_muted };
    p.text(Pos2::new(left, r.min.y + 33.0), Align2::LEFT_CENTER, line2, FontId::proportional(12.0), price_c);
    if let Err(why) = can {
        p.text(Pos2::new(left, r.min.y + 50.0), Align2::LEFT_CENTER, why.text(fr), FontId::proportional(11.0), theme.ansi[3].gamma_multiply(0.9));
    }
    let resp = resp.on_hover_text(tx(kind.about(), fr));
    if ok {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked().then_some(kind)
    } else {
        None
    }
}

/// The building clicked: what it is, its level, what it makes or holds, its works; true: closed.
fn selected_ui(ui: &mut Ui, s: &mut OpenSpace, id: u32, fr: bool, theme: &Theme, unix: i64, said: &mut Option<Why>) -> bool {
    let Some(b) = s.get(id).cloned() else { return true };
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tx(b.kind.name(), fr)).size(16.0).strong().color(theme.text));
        let level = if b.level == 0 { tx(("en construction", "being built"), fr).to_owned() } else { format!("{} {}", tx(("niveau", "level"), fr), b.level) };
        ui.label(egui::RichText::new(level).size(13.0).color(theme.accent));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(egui::RichText::new("✕").size(13.0)).frame(false)).clicked() {
                close = true;
            }
        });
    });
    ui.add(egui::Label::new(egui::RichText::new(tx(b.kind.about(), fr)).size(12.0).color(theme.text_muted)).wrap());
    ui.add_space(4.0);
    // What it does at its level.
    let mut facts: Vec<String> = Vec::new();
    if let Some(r) = b.kind.makes()
        && b.level >= 1
    {
        let (per_hour, keeps) = os::production(b.level);
        facts.push(format!("{}/h", os::amount_text(per_hour, r, fr)));
        facts.push(format!("{} {} / {}", tx(("dedans :", "inside:"), fr), os::short(b.stored, fr), os::short(keeps, fr)));
    }
    if let Some(r) = b.kind.stores()
        && b.level >= 1
    {
        facts.push(format!("{} {}", tx(("garde", "holds"), fr), os::amount_text(os::storage(b.level), r, fr)));
    }
    if b.kind == Kind::Hq {
        facts.push(format!("{} {} ☕ / 💶", tx(("garde", "holds"), fr), os::short(os::hq_storage(b.level), fr)));
    }
    // An obstacle: only cleared.
    if b.kind.is_obstacle() {
        let price = OpenSpace::clear_cost(b.kind);
        let enough = s.budget >= price as f64;
        if button(ui, enough, format!("🧹 {}  ·  {}", tx(("Débarrasser", "Clear"), fr), os::amount_text(price as f64, Res::Budget, fr)), theme) {
            match s.clear(id) {
                Ok(_) => close = true,
                Err(why) => *said = Some(why),
            }
        }
        return close;
    }
    facts.push(format!("{} {}", os::short(b.kind.hp(b.level) as f64, fr), tx(("PV", "HP"), fr)));
    ui.label(egui::RichText::new(facts.join("   ·   ")).size(12.5).color(theme.text));
    ui.add_space(6.0);

    if let Some(job) = &b.job {
        let left = job.until - unix;
        ui.horizontal(|ui| {
            let frac = b.progress(unix).unwrap_or(0.0) as f32;
            let (r, _) = ui.allocate_exact_size(Vec2::new(150.0, 8.0), Sense::hover());
            ui.painter().rect_filled(r, 4.0, theme.tab_hover);
            ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * frac, 8.0)), 4.0, theme.ansi[2]);
            ui.label(egui::RichText::new(format!("{} {}", tx(("encore", "left:"), fr), os::duration_text(left, fr))).size(12.5).color(theme.text));
            let price = os::finish_cost(left);
            if small(ui, format!("{} {price} 🎫", tx(("Finir :", "Finish:"), fr)))
                && let Err(why) = s.finish_now(id, unix)
            {
                *said = Some(why);
            }
            if b.kind != Kind::Hq || b.level > 0 {
                if small(ui, tx(("Annuler (½ remboursé)", "Cancel (½ back)"), fr)) {
                    s.cancel(id);
                }
            }
        });
        return close;
    }
    ui.horizontal(|ui| {
        match s.can_upgrade(id) {
            Err(Why::MaxLevel) => {
                ui.label(egui::RichText::new(tx(("Niveau maximum", "Highest level"), fr)).size(12.5).color(theme.ansi[2]));
            }
            can => {
                let (r, price) = os::cost(b.kind, b.level + 1, 0);
                let secs = os::build_time(b.kind, b.level + 1);
                let text = format!("⬆ {} {}  ·  {}  ·  {}", tx(("Niveau", "Level"), fr), b.level + 1, os::amount_text(price as f64, r, fr), if secs == 0 { tx(("immédiat", "at once"), fr).to_owned() } else { os::duration_text(secs as i64, fr) });
                if button(ui, can.is_ok(), text, theme) {
                    if let Err(why) = s.upgrade(id, unix) {
                        *said = Some(why);
                    }
                }
                if let Err(why) = can {
                    ui.label(egui::RichText::new(why.text(fr)).size(12.0).color(theme.ansi[3]));
                }
            }
        }
        if b.kind.makes().is_some() && b.stored >= 1.0 && small(ui, tx(("Récolter", "Collect"), fr)) && s.collect(id) < 1.0 {
            *said = Some(Why::Short(b.kind.makes().unwrap_or(Res::Coffee), 0));
        }
    });
    ui.label(egui::RichText::new(tx(("Glisse le bâtiment pour le déplacer.", "Drag the building to move it."), fr)).size(11.0).color(theme.text_muted));
    close
}

/// The card of a building built for the first time: true once closed.
fn discovery_card(ui: &mut Ui, kind: Kind, fr: bool, theme: &Theme, time: f64) -> bool {
    let mut close = false;
    egui::Window::new(tx(("Nouveau bâtiment !", "New building!"), fr))
        .id(egui::Id::new("os-discovery"))
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, -20.0))
        .frame(Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.accent)).corner_radius(14.0).inner_margin(20.0))
        .show(ui.ctx(), |ui| {
            ui.set_width(400.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(tx(("NOUVEAU BÂTIMENT", "NEW BUILDING"), fr)).size(11.0).strong().color(theme.text_muted));
                let (r, _) = ui.allocate_exact_size(Vec2::new(160.0, 120.0), Sense::hover());
                // A glow behind it.
                ui.painter().circle_filled(r.center(), 56.0, theme.accent.gamma_multiply(0.12));
                scene::paint_mini(ui.painter(), kind, r.center(), 130.0, time);
                ui.label(egui::RichText::new(tx(kind.name(), fr)).size(20.0).strong().color(theme.text));
            });
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(tx(kind.guide(), fr)).size(13.5).color(theme.text)).wrap());
            ui.add_space(8.0);
            let (_, level) = kind.allowed(os::HQ_MAX);
            ui.label(egui::RichText::new(format!("{} {}  ·  {} {}", tx(("Niveau max :", "Top level:"), fr), level, tx(("débloqué au QG", "unlocked at HQ"), fr), kind.unlocked_at())).size(12.0).color(theme.text_muted));
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                if button(ui, true, tx(("Compris !", "Got it!"), fr), theme) {
                    close = true;
                }
            });
        });
    close
}

/// What happened while away; true: closed.
fn openspace_away(ui: &mut Ui, sum: &Summary, fr: bool, theme: &Theme) -> bool {
    let mut close = false;
    egui::Window::new(tx(("Pendant ton absence", "While you were away"), fr))
        .id(egui::Id::new("os-away"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.accent)).corner_radius(12.0).inner_margin(18.0))
        .show(ui.ctx(), |ui| {
            ui.set_max_width(360.0);
            ui.label(egui::RichText::new(format!("{} {}", tx(("Absent·e", "Away for"), fr), os::duration_text(sum.secs as i64, fr))).size(13.0).color(theme.text_muted));
            ui.add_space(6.0);
            for (r, n) in [(Res::Coffee, sum.coffee), (Res::Budget, sum.budget)] {
                if n >= 1.0 {
                    ui.label(egui::RichText::new(format!("+{} {}", os::amount_text(n, r, fr), tx(("dans tes machines", "in your machines"), fr))).size(14.0).color(theme.text));
                }
            }
            for (kind, level) in &sum.finished {
                ui.label(egui::RichText::new(format!("✔ {} → {} {level}", tx(kind.name(), fr), tx(("niveau", "level"), fr))).size(13.5).color(theme.ansi[2]));
            }
            ui.add_space(10.0);
            if button(ui, true, tx(("Au boulot !", "Back to work!"), fr), theme) {
                close = true;
            }
        });
    close
}
