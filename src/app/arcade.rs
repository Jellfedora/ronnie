//! The games' page: the games by kind (multiplayer, arcade, brain teasers, build and collect), each in a
//! card with a banner in its colors (its icon on a medallion, lively when hovered), what it is, its best
//! score or who is online, its rewards. Reached from the sidebar's Games icon. A click opens it, a right
//! click in a new window.

use super::four::HomeGame;
use super::*;

/// The kinds of games, in the page's order.
const KINDS: usize = 4;

/// A game's kind (its section, in `KINDS`).
fn kind(game: HomeGame) -> usize {
    match game {
        HomeGame::Four | HomeGame::Larsen | HomeGame::Blob | HomeGame::Monday => 0,
        HomeGame::SpeedMetal | HomeGame::Runner | HomeGame::Riff | HomeGame::Backstage | HomeGame::BugHunt => 1,
        HomeGame::Mines | HomeGame::Coffee => 2,
        HomeGame::Tycoon | HomeGame::Fishing => 3,
    }
}

/// A game's banner, from its top color to its bottom one.
fn banner_colors(game: HomeGame) -> (Color32, Color32) {
    let c = Color32::from_rgb;
    match game {
        HomeGame::SpeedMetal => (c(96, 100, 114), c(30, 32, 40)),
        HomeGame::Four => (c(64, 124, 232), c(28, 56, 140)),
        HomeGame::Blob => (c(58, 204, 156), c(18, 104, 88)),
        HomeGame::Mines => (c(128, 138, 160), c(58, 64, 84)),
        HomeGame::Runner => (c(255, 150, 64), c(188, 62, 42)),
        HomeGame::Riff => (c(214, 72, 206), c(88, 30, 144)),
        HomeGame::Backstage => (c(88, 74, 138), c(30, 24, 54)),
        HomeGame::Coffee => (c(184, 122, 78), c(88, 52, 32)),
        HomeGame::Larsen => (c(236, 82, 74), c(118, 24, 42)),
        HomeGame::BugHunt => (c(132, 206, 70), c(38, 98, 42)),
        HomeGame::Fishing => (c(66, 178, 228), c(18, 78, 140)),
        HomeGame::Monday => (c(124, 160, 98), c(48, 66, 44)),
        HomeGame::Tycoon => (c(244, 196, 78), c(172, 108, 28)),
    }
}

/// A rectangle filled from `top` to `bottom`, its corners rounded by `r` (top left, top right,
/// bottom right, bottom left), its edge smoothed: a ring fading out over a pixel, as egui does.
fn paint_gradient(painter: &egui::Painter, rect: Rect, r: [f32; 4], top: Color32, bottom: Color32) {
    let color = |y: f32| top.lerp_to_gamma(bottom, ((y - rect.min.y) / rect.height()).clamp(0.0, 1.0));
    let half = 0.5 / painter.pixels_per_point();
    // Around, clockwise from the left: each corner's arc (one point when not rounded), and the way out there.
    let corners = [(Pos2::new(rect.min.x + r[0], rect.min.y + r[0]), r[0], 180.0f32), (Pos2::new(rect.max.x - r[1], rect.min.y + r[1]), r[1], 270.0), (Pos2::new(rect.max.x - r[2], rect.max.y - r[2]), r[2], 0.0), (Pos2::new(rect.min.x + r[3], rect.max.y - r[3]), r[3], 90.0)];
    let mut edge = Vec::new();
    for (center, radius, start) in corners {
        for k in 0..=12 {
            let a = (start + 90.0 * k as f32 / 12.0).to_radians();
            let out = Vec2::new(a.cos(), a.sin());
            edge.push((center + radius * out, out));
        }
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), color(rect.center().y));
    for &(p, out) in &edge {
        let inner = p - out * half;
        mesh.colored_vertex(inner, color(inner.y));
        mesh.colored_vertex(p + out * half, Color32::TRANSPARENT);
    }
    let m = edge.len() as u32;
    for i in 0..m {
        let j = (i + 1) % m;
        let (a, b, a_out, b_out) = (1 + 2 * i, 1 + 2 * j, 2 + 2 * i, 2 + 2 * j);
        mesh.add_triangle(0, a, b);
        mesh.add_triangle(a, a_out, b_out);
        mesh.add_triangle(a, b_out, b);
    }
    painter.add(mesh);
}

/// A section's cards in a line of the page: its kind, its first card (in the kind's list), how many,
/// where it starts (from the top left of all the sections), its title above or not (a section too
/// long for a line goes on over the next ones, untitled).
struct Block {
    kind: usize,
    from: usize,
    count: usize,
    at: Vec2,
    titled: bool,
}

/// The sections (`sizes`: their number of cards) in lines `avail` wide, each one on its own (side by side,
/// they read badly), one longer than a line going on over the next ones. The blocks, and the size of the whole.
fn pack(sizes: &[usize], avail: f32, card: Vec2, gap: f32, head: f32) -> (Vec<Block>, Vec2) {
    let line_gap = 22.0;
    let max_cols = (((avail + gap) / (card.x + gap)).floor() as usize).max(1);
    let width = |n: usize| n as f32 * card.x + n.saturating_sub(1) as f32 * gap;
    let mut blocks = Vec::new();
    let (mut x, mut y, mut line_h, mut widest, mut open) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, false);
    for (kind, &n) in sizes.iter().enumerate() {
        let mut from = 0;
        while from < n {
            let count = (n - from).min(max_cols);
            if open {
                (x, y, line_h) = (0.0, y + line_h + line_gap, 0.0);
            }
            let titled = from == 0;
            blocks.push(Block { kind, from, count, at: Vec2::new(x, y), titled });
            x += width(count);
            widest = widest.max(x);
            line_h = line_h.max(card.y + if titled { head } else { 0.0 });
            open = true;
            from += count;
        }
    }
    (blocks, Vec2::new(widest, y + line_h))
}

impl App {
    pub(super) fn arcade_page(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        ui.painter_at(rect).rect_filled(rect, 0.0, theme.bg);

        let games = self.games_list();
        // What is still to unlock, game by game, and in all; the games played enough this half-day.
        let rewards: Vec<Option<(usize, usize)>> = games.iter().map(|(g, _, _)| self.unlocks_count(*g)).collect();
        let (left_all, total_all) = rewards.iter().flatten().fold((0, 0), |(l, t), (a, b)| (l + a, t + b));
        let locked: Vec<bool> = games.iter().map(|(g, _, _)| self.game_locked(*g).is_some()).collect();
        // The games of each kind, in the sidebar's order.
        let mut by_kind: [Vec<usize>; KINDS] = Default::default();
        for (k, (game, _, _)) in games.iter().enumerate() {
            by_kind[kind(*game)].push(k);
        }
        let sizes: Vec<usize> = by_kind.iter().map(Vec::len).collect();
        let kind_names = [t.arcade_kind_online, t.arcade_kind_reflex, t.arcade_kind_puzzle, t.arcade_kind_long];

        // Tall cards (the banner on top), the page scrolling when they don't fit; flat ones (the banner
        // on the left) only on a page too low for a line of tall ones.
        let avail = (rect.width() - 96.0).max(200.0);
        let layout = |tall: bool| {
            let (card, gap, head, title_h) = if tall { (Vec2::new(228.0, 238.0), 16.0, 34.0, 96.0) } else { (Vec2::new(300.0, 96.0), 12.0, 30.0, 78.0) };
            let card = Vec2::new(card.x.min(avail), card.y);
            let (blocks, size) = pack(&sizes, avail, card, gap, head);
            (card, gap, head, title_h, blocks, size)
        };
        let wide = rect.height() >= 560.0;
        let (card_size, gap, head, title_h, blocks, size) = layout(wide);
        let content_h = (40.0 + title_h + size.y + 24.0).max(rect.height());
        let time = ui.input(|i| i.time) as f32;

        let (mut open, mut new_window, mut back, mut any_hot) = (None, None, false, false);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        egui::ScrollArea::vertical().id_salt("arcade").auto_shrink([false; 2]).show(&mut child, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(rect.width(), content_h), Sense::hover());
            let painter = ui.painter().clone();

            // Back to the home page.
            let back_rect = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            back = ui.put(back_rect, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked();

            let top = (rect.center().y - (size.y + title_h) / 2.0).max(rect.min.y + 40.0);
            let (title_size, sub_y) = if wide { (46.0, 78.0) } else { (36.0, 58.0) };
            paint_metal(&painter, Pos2::new(rect.center().x, top + title_size * 0.7), Align2::CENTER_CENTER, t.cat_games, title_size, theme.accent, 1.0);
            // The subtitle, and the rewards unlocked in all.
            let mut job = egui::text::LayoutJob::default();
            job.append(t.arcade_subtitle, 0.0, egui::TextFormat::simple(FontId::proportional(13.0), theme.text_muted));
            if total_all > 0 {
                let color = if left_all == 0 { theme.ansi[2] } else { theme.accent };
                job.append(&t.arcade_rewards.replace("{n}", &(total_all - left_all).to_string()).replace("{max}", &total_all.to_string()), 14.0, egui::TextFormat::simple(FontId::proportional(13.0), color));
            }
            let galley = painter.layout_job(job);
            painter.galley(Pos2::new(rect.center().x - galley.size().x / 2.0, top + sub_y - galley.size().y / 2.0), galley, theme.text_muted);

            let origin = Pos2::new(rect.center().x - size.x / 2.0, top + title_h);
            for block in &blocks {
                let mut at = origin + block.at;
                // The section's title: a bar of the accent, its name, how many games.
                if block.titled {
                    painter.rect_filled(Rect::from_min_size(at + Vec2::new(0.0, 4.0), Vec2::new(3.0, 15.0)), 1.5, theme.accent);
                    let named = painter.text(at + Vec2::new(11.0, 11.5), Align2::LEFT_CENTER, kind_names[block.kind], FontId::proportional(15.0), theme.text);
                    painter.text(Pos2::new(named.max.x + 8.0, at.y + 12.0), Align2::LEFT_CENTER, sizes[block.kind].to_string(), FontId::proportional(12.0), theme.text_muted);
                    at.y += head;
                }
                for (col, &k) in by_kind[block.kind][block.from..block.from + block.count].iter().enumerate() {
                    let (game, name, note) = &games[k];
                    let (game, reward, locked) = (*game, rewards[k], locked[k]);
                    let base = Rect::from_min_size(at + Vec2::new(col as f32 * (card_size.x + gap), 0.0), card_size);
                    let resp = ui.interact(base, ui.id().with(("arcade", game as u8)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    let hot = resp.hovered();
                    any_hot |= hot;
                    // Hovered, it rises a little, its shadow grows, its icon floats.
                    let lift = ui.ctx().animate_bool_with_time(ui.id().with(("arcade-lift", game as u8)), hot, 0.14);
                    let card = base.translate(Vec2::new(0.0, -3.0 * lift));
                    let shadow = egui::epaint::Shadow { offset: [0, (3.0 + 4.0 * lift) as i8], blur: (10.0 + 10.0 * lift) as u8, spread: 0, color: Color32::from_black_alpha((if theme.dark { 70.0 } else { 26.0 } + 40.0 * lift) as u8) };
                    painter.add(shadow.as_shape(card, 14.0));
                    painter.rect_filled(card, 14.0, if hot { theme.tab_active } else { theme.chrome_bg });

                    // The banner, its glows drifting when hovered, the icon on its medallion.
                    let banner = if wide { Rect::from_min_max(card.min, Pos2::new(card.max.x, card.min.y + 108.0)) } else { Rect::from_min_max(card.min, Pos2::new(card.min.x + card.height(), card.max.y)) };
                    let corners = if wide { [14.0, 14.0, 0.0, 0.0] } else { [14.0, 0.0, 0.0, 14.0] };
                    let (c_top, c_bottom) = banner_colors(game);
                    paint_gradient(&painter, banner, corners, c_top, c_bottom);
                    let seed = |i: u32| ((game as u32 * 7919 + i * 104_729 + 31) % 997) as f32 / 997.0;
                    // Each glow wholly inside the banner, away from its rounded corners, drifting a little.
                    for i in 0..5 {
                        let radius = 6.0 + seed(i + 23) * 12.0;
                        let room = banner.shrink(radius + 8.0);
                        let drift = Vec2::new((time * 0.9 + i as f32 * 1.7).sin(), (time * 0.7 + i as f32 * 2.3).cos()) * 5.0 * lift;
                        let p = room.min + Vec2::new(seed(i) * room.width(), seed(i + 11) * room.height()) + drift;
                        painter.circle_filled(p, radius, Color32::from_white_alpha(14 + (seed(i + 37) * 14.0) as u8));
                    }
                    let (radius, scale) = if wide { (36.0, 2.3) } else { (28.0, 1.7) };
                    let bob = (time * 3.2).sin() * 3.0 * lift;
                    let icon = banner.center() + Vec2::new(0.0, bob - 2.0 * lift);
                    painter.circle_filled(icon + Vec2::new(0.0, 3.0), radius + 1.0, Color32::from_black_alpha(50));
                    painter.circle(icon, radius * (1.0 + 0.04 * lift), theme.bg, Stroke::new(2.0, Color32::from_white_alpha(70 + (60.0 * lift) as u8)));
                    match game {
                        HomeGame::SpeedMetal => paint_metal(&painter, icon, Align2::CENTER_CENTER, "R", 20.0 * scale, theme.accent, 1.0),
                        HomeGame::Four => super::four::paint_four_icon(&painter, icon, &theme, scale),
                        HomeGame::Blob => super::blob::paint_blob_icon(&painter, icon, &theme, scale),
                        HomeGame::Mines => super::mines::paint_mines_icon(&painter, icon, &theme, scale),
                        HomeGame::Runner => super::runner::paint_runner_icon(&painter, icon, &theme, scale),
                        HomeGame::Riff => super::riff::paint_riff_icon(&painter, icon, &theme, scale),
                        HomeGame::Backstage => super::backstage::paint_backstage_icon(&painter, icon, &theme, scale),
                        HomeGame::Coffee => super::coffee::paint_coffee_icon(&painter, icon, &theme, scale),
                        HomeGame::Larsen => super::larsen::paint_larsen_icon(&painter, icon, &theme, scale),
                        HomeGame::BugHunt => super::bughunt::paint_bughunt_icon(&painter, icon, &theme, scale),
                        HomeGame::Fishing => super::fishing::paint_fishing_icon(&painter, icon, &theme, scale),
                        HomeGame::Monday => super::zombie::paint_monday_icon(&painter, icon, &theme, scale),
                        HomeGame::Tycoon => super::tycoon::paint_tycoon_icon(&painter, icon, &theme, scale),
                    }
                    // Played enough this half-day: the banner dimmed (the note says when it's back).
                    if locked {
                        let r = |v: f32| v as u8;
                        painter.rect_filled(banner, egui::CornerRadius { nw: r(corners[0]), ne: r(corners[1]), se: r(corners[2]), sw: r(corners[3]) }, Color32::from_black_alpha(130));
                    }
                    painter.rect_stroke(card, 14.0, Stroke::new(if hot { 1.5 } else { 1.0 }, if hot { theme.accent.gamma_multiply(0.7) } else { theme.tab_hover }), egui::StrokeKind::Inside);

                    let desc = match game {
                        HomeGame::SpeedMetal => t.arcade_metal,
                        HomeGame::Four => t.arcade_four,
                        HomeGame::Blob => t.blob_tagline,
                        HomeGame::Mines => t.arcade_mines,
                        HomeGame::Runner => t.arcade_runner,
                        HomeGame::Riff => t.arcade_riff,
                        HomeGame::Backstage => t.arcade_backstage,
                        HomeGame::Coffee => t.arcade_coffee,
                        HomeGame::Larsen => t.arcade_larsen,
                        HomeGame::BugHunt => t.arcade_bughunt,
                        HomeGame::Fishing => t.fishing_arcade,
                        HomeGame::Monday => t.arcade_monday,
                        HomeGame::Tycoon => t.tycoon_arcade,
                    };
                    let name_color = if hot { theme.text } else { theme.text.gamma_multiply(0.92) };
                    let note_color = if locked { theme.text_muted } else { theme.accent };
                    // Its rewards: how many still to unlock (a padlock), or all of them.
                    let reward_text = reward.map(|(left, _)| if left == 0 { (t.unlock_all_done.to_owned(), theme.ansi[2], true) } else { (t.unlock_left.replace("{n}", &left.to_string()), theme.accent, false) });
                    let elided;
                    if wide {
                        // On the banner, top right: the rewards, on a dark pill.
                        if let Some((text, color, done)) = &reward_text {
                            let color = color.lerp_to_gamma(Color32::WHITE, 0.35);
                            let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(11.5), color);
                            let pill = Rect::from_min_size(Pos2::new(banner.max.x - 10.0 - galley.size().x - 26.0, banner.min.y + 10.0), Vec2::new(galley.size().x + 26.0, 20.0));
                            painter.rect_filled(pill, 10.0, Color32::from_black_alpha(120));
                            super::unlocks::paint_padlock(&painter, Pos2::new(pill.min.x + 11.0, pill.center().y), *done, color);
                            painter.galley(Pos2::new(pill.min.x + 20.0, pill.center().y - galley.size().y / 2.0), galley, color);
                        }
                        let x = card.min.x + 16.0;
                        painter.text(Pos2::new(x, banner.max.y + 20.0), Align2::LEFT_CENTER, name, FontId::proportional(17.0), name_color);
                        let mut job = egui::text::LayoutJob::simple(desc.to_owned(), FontId::proportional(12.5), theme.text_muted, card.width() - 32.0);
                        job.wrap.max_rows = if note.is_some() { 4 } else { 5 };
                        let galley = painter.layout_job(job);
                        elided = galley.elided;
                        painter.galley(Pos2::new(x, banner.max.y + 36.0), galley, theme.text_muted);
                        if let Some(note) = note {
                            painter.text(Pos2::new(x, card.max.y - 16.0), Align2::LEFT_CENTER, note, FontId::proportional(12.5), note_color);
                        }
                    } else {
                        let x = banner.max.x + 14.0;
                        painter.text(Pos2::new(x, card.min.y + 20.0), Align2::LEFT_CENTER, name, FontId::proportional(16.0), name_color);
                        // At the bottom: the rewards on the left, the note on the right.
                        if let Some(note) = note {
                            painter.text(Pos2::new(card.max.x - 12.0, card.max.y - 15.0), Align2::RIGHT_CENTER, note, FontId::proportional(12.0), note_color);
                        }
                        // As many lines of the description as fit above them.
                        let bottom = if reward_text.is_some() || note.is_some() { card.max.y - 28.0 } else { card.max.y - 8.0 };
                        let lines = (((bottom - card.min.y - 34.0) / 16.0).floor() as usize).max(1);
                        let mut job = egui::text::LayoutJob::simple(desc.to_owned(), FontId::proportional(12.0), theme.text_muted, card.max.x - x - 12.0);
                        job.wrap.max_rows = lines;
                        let galley = painter.layout_job(job);
                        elided = galley.elided;
                        painter.galley(Pos2::new(x, card.min.y + 34.0), galley, theme.text_muted);
                        if let Some((text, color, done)) = &reward_text {
                            let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(11.5), *color);
                            super::unlocks::paint_padlock(&painter, Pos2::new(x + 5.0, card.max.y - 15.0), *done, *color);
                            painter.galley(Pos2::new(x + 16.0, card.max.y - 15.0 - galley.size().y / 2.0), galley, *color);
                        }
                    }
                    // Hovered: the whole description if cut, and what it unlocks.
                    let resp = if elided || reward.is_some() {
                        resp.on_hover_ui(|ui| {
                            ui.set_max_width(340.0);
                            if elided {
                                ui.add(egui::Label::new(egui::RichText::new(desc).size(12.5).color(theme.text)).wrap());
                            }
                            if reward.is_some() {
                                if elided {
                                    ui.add_space(8.0);
                                }
                                self.unlocks_ui(ui, game);
                            }
                        })
                    } else {
                        resp
                    };
                    if resp.clicked() {
                        open = Some(game);
                    }
                    resp.context_menu(|ui| menu_item(ui, &format!("⧉  {}", t.new_window_open), game, &mut new_window));
                }
            }
        });
        // The hovered card's icon floats.
        if any_hot {
            ui.ctx().request_repaint();
        }
        if back {
            self.home_game = false;
            self.games_hub = false;
        }
        if let Some(game) = new_window {
            self.game_to_new_window(game);
        } else if let Some(game) = open.filter(|&g| !self.show_game_elsewhere(g)) {
            self.open_home_game(game);
        }
    }

    /// A game's board from floor, beside its page: the player's best (`best`, worded), then floor's
    /// best rounds (`score` words each one); without a pseudo, why and a button to pick one (true: clicked).
    pub(super) fn game_board(&self, ui: &mut Ui, area: Rect, theme: &Theme, board: Option<&[crate::floor::GameEntry]>, best: Option<String>, score: fn(u32) -> String) -> bool {
        self.game_board_noted(ui, area, theme, board, best, score, &|_| None)
    }

    /// The same, each line with a note before its score (`note` words it from the line: Bug Hunt's wave).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn game_board_noted(&self, ui: &mut Ui, area: Rect, theme: &Theme, board: Option<&[crate::floor::GameEntry]>, best: Option<String>, score: fn(u32) -> String, note: &dyn Fn(&crate::floor::GameEntry) -> Option<String>) -> bool {
        let t = self.t();
        let painter = ui.painter_at(area);
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        let mut y = area.min.y + 4.0;
        painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, t.game_board_world, FontId::proportional(15.0), theme.text);
        y += 28.0;
        if let Some(best) = best {
            painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, best, FontId::proportional(13.0), theme.accent);
            y += 24.0;
        }
        if pseudo.is_none() {
            let galley = painter.layout(t.game_need_pseudo.to_owned(), FontId::proportional(12.5), theme.text_muted, area.width());
            let h = galley.size().y;
            painter.galley(Pos2::new(area.min.x, y), galley, theme.text_muted);
            y += h + 10.0;
            return ui.put(Rect::from_min_size(Pos2::new(area.min.x, y), Vec2::new(150.0, 28.0)), egui::Button::new(egui::RichText::new(t.pseudo_pick).size(13.0)).corner_radius(6.0)).clicked();
        }
        let Some(board) = board else {
            painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, "…", FontId::proportional(13.0), theme.text_muted);
            return false;
        };
        if board.is_empty() {
            let galley = painter.layout(t.game_board_empty.to_owned(), FontId::proportional(12.5), theme.text_muted, area.width());
            painter.galley(Pos2::new(area.min.x, y), galley, theme.text_muted);
            return false;
        }
        for entry in board {
            if y + 18.0 > area.max.y {
                break;
            }
            let color = if entry.me { theme.accent } else { theme.text };
            let name = if entry.me { pseudo.as_deref() } else { entry.name.as_deref() }.unwrap_or(t.game_anonymous);
            painter.text(Pos2::new(area.min.x, y), Align2::LEFT_TOP, format!("{}.", entry.rank), FontId::monospace(13.0), theme.text_muted);
            // Their avatar, with their frame.
            let account = self.config.settings.floor.as_ref();
            let face = entry.id.as_deref().and_then(|id| self.avatars.get(ui.ctx(), account, id, entry.avatar.as_deref()));
            super::talk::paint_avatar(&painter, Pos2::new(area.min.x + 40.0, y + 8.0), 8.0, name, false, face.as_ref(), entry.frame.as_deref(), theme);
            super::office::paint_badge(&painter, Pos2::new(area.min.x + 40.0, y + 8.0), 8.0, &entry.badges);
            let named = painter.text(Pos2::new(area.min.x + 56.0, y), Align2::LEFT_TOP, name, FontId::proportional(13.0), color);
            // Their Lundi matin title, in gold, when there's room.
            if let Some(title) = entry.title.as_deref().and_then(|id| super::talk::title_name(t, id)) {
                let galley = painter.layout_no_wrap(format!("« {title} »"), FontId::proportional(10.5), Color32::from_rgb(222, 180, 80));
                if named.max.x + 6.0 + galley.size().x < area.max.x - 120.0 {
                    painter.galley(Pos2::new(named.max.x + 6.0, y + 2.0), galley, Color32::from_rgb(222, 180, 80));
                }
            }
            painter.text(Pos2::new(area.max.x, y), Align2::RIGHT_TOP, score(entry.score), FontId::monospace(13.0), color);
            if let Some(note) = note(entry) {
                painter.text(Pos2::new(area.max.x - 76.0, y + 1.0), Align2::RIGHT_TOP, note, FontId::proportional(12.0), theme.text_muted);
            }
            y += 22.0;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each section on its own line, one longer than a line going on, untitled, over the next.
    #[test]
    fn sections_pack_in_lines() {
        let card = Vec2::new(100.0, 50.0);
        // Room for 6 cards: a line each, all at the left.
        let (blocks, size) = pack(&[4, 5, 2, 2], 6.0 * 100.0 + 5.0 * 10.0, card, 10.0, 20.0);
        let lines: Vec<f32> = blocks.iter().map(|b| b.at.y).collect();
        assert_eq!(lines, [0.0, 92.0, 184.0, 276.0]);
        assert!(blocks.iter().all(|b| b.at.x == 0.0 && b.titled));
        assert_eq!(size, Vec2::new(5.0 * 100.0 + 4.0 * 10.0, 276.0 + 70.0));
        // Room for 3: the 5 on two lines, the second untitled.
        let (blocks, _) = pack(&[4, 5, 2, 2], 320.0, card, 10.0, 20.0);
        let five: Vec<(usize, usize, bool)> = blocks.iter().filter(|b| b.kind == 1).map(|b| (b.from, b.count, b.titled)).collect();
        assert_eq!(five, [(0, 3, true), (3, 2, false)]);
    }
}
