//! The Private life tab: the founder's home drawn (the house, the car, the family, a yacht, a jet),
//! the personal account and where its money comes from, the health and the stress, the family, the home
//! and the car, the placements, the hobbies and the luxuries.

use super::*;
use crate::tycoon::{CARS, HOBBIES, HOLIDAYS, HOMES, LUXURIES, MAX_HOBBIES, PLACEMENTS, RENTAL_PRICE};

fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Someone drawn: a head and a body, `h` tall.
fn person(painter: &egui::Painter, foot: Pos2, h: f32, body: Color32, skin: Color32) {
    let head = h * 0.22;
    let torso = Rect::from_min_max(Pos2::new(foot.x - h * 0.16, foot.y - h * 0.72), Pos2::new(foot.x + h * 0.16, foot.y - h * 0.28));
    painter.rect_filled(torso, h * 0.08, body);
    painter.line_segment([Pos2::new(foot.x - h * 0.07, foot.y - h * 0.3), Pos2::new(foot.x - h * 0.08, foot.y)], Stroke::new(h * 0.07, body.gamma_multiply(0.7)));
    painter.line_segment([Pos2::new(foot.x + h * 0.07, foot.y - h * 0.3), Pos2::new(foot.x + h * 0.08, foot.y)], Stroke::new(h * 0.07, body.gamma_multiply(0.7)));
    painter.circle_filled(Pos2::new(foot.x, foot.y - h * 0.72 - head * 0.8), head, skin);
}

/// The founder's home, drawn: bigger with the home, the car beside it, the family in front.
fn paint_life(painter: &egui::Painter, rect: Rect, c: &Corp, now: f64, fr: bool, theme: &Theme) {
    let l = &c.life;
    painter.rect_filled(rect, 10.0, rgb(0x9fd3f0));
    // The sun, or the moon on holiday nights… just the sun.
    painter.circle_filled(Pos2::new(rect.max.x - 50.0, rect.min.y + 34.0), 16.0, rgb(0xffe082));
    let ground = rect.max.y - 34.0;
    let water = l.has("yacht") || l.home == 7;
    painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x, ground), rect.max), egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, rgb(0x7cb342));
    if water {
        painter.rect_filled(Rect::from_min_max(Pos2::new(rect.max.x - 170.0, ground), rect.max), egui::CornerRadius { nw: 0, ne: 0, sw: 0, se: 10 }, rgb(0x4fa3d9));
    }
    // The home: wider and taller with its tier.
    let tier = l.home as f32;
    let w = 60.0 + tier * 22.0;
    let h = 44.0 + tier * 9.0;
    let left = rect.min.x + 30.0;
    let house = Rect::from_min_max(Pos2::new(left, ground - h), Pos2::new(left + w, ground));
    let (wall, roof) = match l.home {
        0 | 1 => (rgb(0xe0d6c8), rgb(0x8d6e63)),
        2 => (rgb(0xf5e6c8), rgb(0xa1887f)),
        3 => (rgb(0xfff3e0), rgb(0xc62828)),
        4 => (rgb(0xffffff), rgb(0xff8a65)),
        5 => (rgb(0xb0bec5), rgb(0x455a64)),
        6 => (rgb(0xd7ccc8), rgb(0x5d4037)),
        _ => (rgb(0xfff8e1), rgb(0x2e7d32)),
    };
    painter.rect_filled(house, 2.0, wall);
    if l.home == 6 {
        // Towers.
        for x in [house.min.x - 10.0, house.max.x - 4.0] {
            painter.rect_filled(Rect::from_min_max(Pos2::new(x, house.min.y - 20.0), Pos2::new(x + 14.0, ground)), 1.0, wall.gamma_multiply(0.9));
            painter.add(egui::Shape::convex_polygon(vec![Pos2::new(x - 3.0, house.min.y - 20.0), Pos2::new(x + 7.0, house.min.y - 38.0), Pos2::new(x + 17.0, house.min.y - 20.0)], roof, Stroke::NONE));
        }
    }
    if l.home == 5 {
        painter.rect_filled(Rect::from_min_max(Pos2::new(house.min.x, house.min.y - 6.0), Pos2::new(house.max.x, house.min.y)), 0.0, roof);
    } else {
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(house.min.x - 8.0, house.min.y), Pos2::new(house.center().x, house.min.y - h * 0.45), Pos2::new(house.max.x + 8.0, house.min.y)], roof, Stroke::NONE));
    }
    // Windows, lit, and the door.
    let cols = ((w - 20.0) / 22.0).floor().max(1.0) as usize;
    let rows = if h > 70.0 { 2 } else { 1 };
    for row in 0..rows {
        for col in 0..cols {
            let x = house.min.x + 12.0 + col as f32 * 22.0;
            let y = house.min.y + 10.0 + row as f32 * 22.0;
            if col == cols / 2 && row == rows - 1 {
                continue;
            }
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(12.0, 11.0)), 1.0, rgb(0xfff59d));
        }
    }
    let door_x = house.min.x + 12.0 + (cols / 2) as f32 * 22.0;
    painter.rect_filled(Rect::from_min_max(Pos2::new(door_x, ground - 20.0), Pos2::new(door_x + 12.0, ground)), 1.0, rgb(0x6d4c41));
    if l.home == 4 {
        painter.rect_filled(Rect::from_min_max(Pos2::new(house.max.x + 6.0, ground + 6.0), Pos2::new(house.max.x + 60.0, ground + 18.0)), 4.0, rgb(0x4dd0e1));
    }
    if l.home == 0 {
        painter.text(Pos2::new(house.center().x, house.min.y - h * 0.45 - 8.0), Align2::CENTER_BOTTOM, tx(("chez les parents", "at your parents'"), fr), FontId::proportional(10.0), rgb(0x37474f));
    }

    // The car.
    let car_x = house.max.x + 30.0;
    if l.car > 0 {
        let color = [rgb(0x000000), rgb(0x90a4ae), rgb(0x42a5f5), rgb(0x6d4c41), rgb(0x263238), rgb(0xeceff1), rgb(0xd32f2f), rgb(0x37474f)][(l.car as usize).min(7)];
        if l.car == 7 {
            // The helicopter, on its pad.
            let body = Rect::from_min_size(Pos2::new(car_x, ground - 22.0), Vec2::new(40.0, 18.0));
            painter.rect_filled(body, 9.0, color);
            painter.line_segment([Pos2::new(body.max.x, body.center().y), Pos2::new(body.max.x + 22.0, body.center().y - 4.0)], Stroke::new(3.0, color));
            let blade = 26.0 * (now as f32 * 20.0).sin();
            painter.line_segment([Pos2::new(body.center().x - blade, body.min.y - 4.0), Pos2::new(body.center().x + blade, body.min.y - 4.0)], Stroke::new(2.0, rgb(0x212121)));
        } else {
            let len = 38.0 + l.car as f32 * 2.0;
            let low = l.car == 6;
            let body = Rect::from_min_size(Pos2::new(car_x, ground - if low { 12.0 } else { 16.0 }), Vec2::new(len, if low { 9.0 } else { 11.0 }));
            painter.rect_filled(body, 4.0, color);
            let cab = Rect::from_min_max(Pos2::new(body.min.x + len * 0.25, body.min.y - if low { 6.0 } else { 9.0 }), Pos2::new(body.max.x - len * 0.25, body.min.y + 1.0));
            painter.rect_filled(cab, 3.0, color);
            painter.rect_filled(cab.shrink(2.0), 2.0, rgb(0xb3e5fc));
            for wx in [body.min.x + 8.0, body.max.x - 8.0] {
                painter.circle_filled(Pos2::new(wx, ground - 2.0), 4.5, rgb(0x212121));
            }
        }
    } else {
        // The bus stop.
        painter.line_segment([Pos2::new(car_x + 6.0, ground), Pos2::new(car_x + 6.0, ground - 34.0)], Stroke::new(2.0, rgb(0x546e7a)));
        painter.rect_filled(Rect::from_min_size(Pos2::new(car_x, ground - 40.0), Vec2::new(14.0, 10.0)), 2.0, rgb(0x1e88e5));
    }

    // The family, in front of the home.
    let mut x = house.center().x - 18.0;
    let skin = rgb(0xf1c27d);
    person(painter, Pos2::new(x, ground + 26.0), 30.0, theme.accent, skin);
    painter.text(Pos2::new(x, ground + 26.0 - 36.0), Align2::CENTER_BOTTOM, "👑", FontId::proportional(9.0), theme.text);
    if l.partner.is_some() {
        x += 16.0;
        person(painter, Pos2::new(x, ground + 26.0), 29.0, rgb(0x8e24aa), rgb(0xe0ac69));
    }
    for k in l.kids.iter().filter(|k| !k.grown) {
        x += 14.0;
        let h = (10.0 + k.years() as f32 * 1.0).min(27.0);
        person(painter, Pos2::new(x, ground + 26.0), h, rgb(0xff7043), skin);
    }
    // The luxuries.
    if l.has("yacht") {
        let bob = (now as f32 * 1.5).sin() * 1.5;
        let y = ground + 12.0 + bob;
        let bx = rect.max.x - 140.0;
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(bx, y), Pos2::new(bx + 90.0, y), Pos2::new(bx + 78.0, y + 10.0), Pos2::new(bx + 10.0, y + 10.0)], rgb(0xffffff), Stroke::NONE));
        painter.rect_filled(Rect::from_min_size(Pos2::new(bx + 22.0, y - 10.0), Vec2::new(44.0, 10.0)), 3.0, rgb(0xeceff1));
    }
    if l.has("jet") {
        let t = (now as f32 * 0.08).fract();
        let jx = rect.min.x + t * (rect.width() + 80.0) - 40.0;
        let jy = rect.min.y + 26.0;
        painter.add(egui::Shape::convex_polygon(vec![Pos2::new(jx, jy), Pos2::new(jx + 34.0, jy + 3.0), Pos2::new(jx, jy + 6.0)], rgb(0xeceff1), Stroke::NONE));
        painter.line_segment([Pos2::new(jx + 12.0, jy + 3.0), Pos2::new(jx + 4.0, jy - 7.0)], Stroke::new(2.5, rgb(0xcfd8dc)));
    }
    // What's going on.
    let status = if l.burnout > 0.0 {
        Some((format!("{} · {}", tx(("Burn-out", "Burn-out"), fr), corp::duration(l.burnout, fr)), theme.ansi[1]))
    } else if l.holiday > 0.0 {
        Some((format!("{} · {}", tx(("En vacances 🌴", "On holiday 🌴"), fr), corp::duration(l.holiday, fr)), theme.ansi[2]))
    } else {
        None
    };
    if let Some((text, color)) = status {
        let pos = Pos2::new(rect.center().x, rect.min.y + 12.0);
        let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), color);
        let bg = Rect::from_center_size(pos + Vec2::new(0.0, galley.size().y / 2.0), galley.size() + Vec2::new(16.0, 8.0));
        painter.rect_filled(bg, 8.0, theme.chrome_bg);
        painter.galley(Pos2::new(pos.x - galley.size().x / 2.0, pos.y), galley, color);
    }
}

impl App {
    pub(super) fn tycoon_life(&mut self, ui: &mut Ui, c: &mut Corp, fr: bool, theme: &Theme) {
        let now = ui.input(|i| i.time);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 170.0), Sense::hover());
        paint_life(ui.painter(), rect, c, now, fr, theme);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
        ui.add_space(10.0);
        let l = c.life.clone();

        // The money.
        let pay = if c.cash >= 0.0 { c.founder_pay() } else { 0.0 };
        let dividends = c.dividends() * c.founder_share();
        let income = pay + dividends + l.rent_income();
        let spend = l.costs();
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                heading(ui, tx(("Ton compte perso", "Your personal account"), fr), theme);
                ui.label(egui::RichText::new(money(l.account, fr)).size(18.0).color(if l.account < 0.0 { theme.ansi[1] } else { theme.ansi[2] }).strong());
                let net = income - spend;
                muted(ui, format!("{}{} · {} {}", if net >= 0.0 { "+" } else { "" }, per_min(net, fr), tx(("patrimoine", "net worth"), fr), money(l.net_worth(), fr)), theme);
            });
            muted(ui, format!("{} {} ({} {} · {} {} · {} {}) · {} {}", tx(("Entrées", "In"), fr), per_min(income, fr), tx(("salaire", "pay"), fr), per_min(pay, fr), tx(("dividendes", "dividends"), fr), per_min(dividends, fr), tx(("loyers", "rents"), fr), per_min(l.rent_income(), fr), tx(("dépenses", "out"), fr), per_min(spend, fr)), theme);
            if l.account < 0.0 {
                ui.label(egui::RichText::new(tx(("À découvert : ton stress grimpe et ton couple en souffre.", "Overdrawn: your stress climbs and your couple suffers."), fr)).size(12.5).color(theme.ansi[1]));
            }
            ui.add_space(6.0);
            let mut p = c.policy.clone();
            ui.label(egui::RichText::new(tx(("Ton salaire de fondateur·rice", "Your founder's pay"), fr)).size(13.0).color(theme.text).strong());
            muted(ui, tx(("Payé par la boîte (il compte dans ses dépenses), et seulement si sa trésorerie est positive. Il grandit avec l'étape de la boîte.", "Paid by the company (it counts in its spending), and only while its cash is positive. It grows with the company's stage."), fr), theme);
            let labels: Vec<String> = (0..4u8).map(|k| {
                let amount = [0.0, 0.4, 1.5, 5.0][k as usize] * (1.0 + c.stage as f64).powi(2);
                let name = tx([("Aucun", "None"), ("Modeste", "Modest"), ("Confortable", "Comfortable"), ("Ministre", "Minister")][k as usize], fr);
                if k == 0 { name.to_owned() } else { format!("{name} ({})", per_min(amount, fr)) }
            }).collect();
            if let Some(v) = segmented(ui, p.founder_pay, &labels, theme) {
                p.founder_pay = v;
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tx(("Dividendes", "Dividends"), fr)).size(13.0).color(theme.text).strong());
            muted(ui, if c.board.is_some() { tx(("Une part du bénéfice, quand la trésorerie dépasse la réserve. Coté·e en bourse, tu n'en touches qu'un quart (le reste va aux actionnaires) ; le conseil les apprécie : +3 ou +8 de confiance par trimestre, −6 sans.", "A share of the profit, while the cash is above the reserve. Listed, you only get a quarter (the rest goes to the shareholders); the board likes them: +3 or +8 trust a quarter, −6 without."), fr) } else { tx(("Une part du bénéfice, quand la trésorerie dépasse la réserve : tout pour toi tant que tu n'es pas en bourse.", "A share of the profit, while the cash is above the reserve: all yours until you're listed."), fr) }, theme);
            let labels: Vec<String> = [("Aucun", "None"), ("10 % du bénéfice", "10 % of the profit"), ("30 % du bénéfice", "30 % of the profit")].iter().map(|x| tx(*x, fr).to_owned()).collect();
            if let Some(v) = segmented(ui, p.dividends, &labels, theme) {
                p.dividends = v;
            }
            c.set_policy(p);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let put = (l.account * 0.5).floor();
                if button(ui, put >= 1.0, format!("{} ({})", tx(("Renflouer la boîte", "Bail out the company"), fr), money(put, fr)), theme) {
                    c.invest_in_company(put);
                }
                let free = (c.cash - c.reserve()).max(0.0);
                let take = (free * 0.25).floor();
                if ui.add_enabled(take >= 1.0, egui::Button::new(egui::RichText::new(format!("{} ({} → {})", tx(("Se verser un dividende exceptionnel", "Pay yourself a one-off dividend"), fr), money(take, fr), money(take * 0.7 * c.founder_share().max(0.25), fr))).size(12.0)).corner_radius(6.0)).on_hover_text(tx(("30 % d'impôts, et jamais sous la réserve de la boîte.", "30 % tax, and never below the company's reserve."), fr)).clicked() {
                    c.take_from_company(take);
                }
            });
        });
        ui.add_space(8.0);

        // The health.
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, tx(("Ta forme", "How you're doing"), fr), theme);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tx(("Santé", "Health"), fr)).size(12.5).color(theme.text_muted));
                bar(ui, l.health / 100.0, level_color(l.health / 100.0, theme), 120.0, 8.0, theme).on_hover_text(format!("{:.0}/100", l.health));
                ui.add_space(10.0);
                ui.label(egui::RichText::new(tx(("Stress", "Stress"), fr)).size(12.5).color(theme.text_muted));
                bar(ui, l.stress / 100.0, level_color(1.0 - l.stress / 100.0, theme), 120.0, 8.0, theme).on_hover_text(format!("{:.0}/100", l.stress));
                ui.add_space(10.0);
                muted(ui, format!("{} {:.0} %", tx(("Ton efficacité au travail :", "Your efficiency at work:"), fr), l.work_factor() * 100.0), theme);
            });
            muted(ui, tx(("Le crunch, une boîte dans le rouge, un compte à découvert, des mois sans vacances : le stress monte. Au-delà de 70, la santé baisse ; sous 15, c'est le burn-out (20 min sans travailler). Le stress pèse aussi sur ton moral.", "Crunch, a company in the red, an overdrawn account, months without holidays: stress rises. Past 70, health drops; under 15, it's burn-out (20 min without work). Stress also weighs on your morale."), fr), theme);
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tx(("Temps pour toi et ta famille", "Time for you and your family"), fr)).size(13.0).color(theme.text).strong());
            let labels: Vec<String> = [("Peu (+10 % au travail, stress +10)", "Little (+10 % at work, stress +10)"), ("Normal", "Normal"), ("Beaucoup (−15 % au travail, stress −10)", "Lots (−15 % at work, stress −10)")].iter().map(|x| tx(*x, fr).to_owned()).collect();
            if let Some(v) = segmented(ui, l.family_time, &labels, theme) {
                c.set_family_time(v);
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tx(("Partir en vacances", "Go on holiday"), fr)).size(13.0).color(theme.text).strong());
            muted(ui, tx(("Tu ne travailles plus le temps des vacances (la boîte tourne sans toi), ton stress fond, ta famille est ravie.", "You don't work while on holiday (the company runs without you), your stress melts, your family is delighted."), fr), theme);
            let mut go = None;
            ui.horizontal_wrapped(|ui| {
                for (k, (name, _, secs, _)) in HOLIDAYS.iter().enumerate() {
                    let secs = if l.has("jet") { secs * 0.5 } else { *secs };
                    if button(ui, c.can_holiday(k), format!("{} · {} ({})", tx(*name, fr), corp::duration(secs, fr), money(c.holiday_cost(k), fr)), theme) {
                        go = Some(k);
                    }
                }
            });
            if let Some(k) = go {
                c.go_on_holiday(k);
            }
        });
        ui.add_space(8.0);

        // The family.
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, tx(("Ta famille", "Your family"), fr), theme);
            match &l.partner {
                None => muted(ui, tx(("Célibataire. Une rencontre finira bien par arriver (un imprévu), si tu n'es pas trop stressé·e.", "Single. You'll meet someone eventually (a surprise), if you're not too stressed."), fr), theme),
                Some(p) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(&p.name).size(14.0).color(theme.text).strong());
                        ui.label(egui::RichText::new(if p.married { tx(("marié·e·s", "married"), fr) } else { tx(("en couple", "together"), fr) }).size(12.5).color(theme.accent));
                        muted(ui, format!("{} {}", tx(("depuis", "for"), fr), corp::duration(p.since, fr)), theme);
                        ui.label(egui::RichText::new(tx(("Bonheur", "Happiness"), fr)).size(12.5).color(theme.text_muted));
                        bar(ui, p.happiness / 100.0, level_color(p.happiness / 100.0, theme), 110.0, 8.0, theme).on_hover_text(format!("{:.0}/100", p.happiness));
                    });
                    muted(ui, tx(("Son bonheur monte avec le temps passé ensemble, le confort de la maison, les vacances ; il baisse avec ton stress, le crunch, un compte à découvert. À zéro, c'est la rupture (marié·e·s : la moitié de tout).", "Their happiness rises with time together, a comfortable home, holidays; it drops with your stress, crunch, an overdrawn account. At zero, it's over (married: half of everything)."), fr), theme);
                    ui.horizontal_wrapped(|ui| {
                        if !p.married {
                            let can = c.can_marry();
                            if button(ui, can && l.account >= c.wedding_cost(false), format!("{} ({})", tx(("Mariage intime", "Small wedding"), fr), money(c.wedding_cost(false), fr)), theme) {
                                c.marry(false);
                            }
                            if button(ui, can && l.account >= c.wedding_cost(true), format!("{} ({})", tx(("Grand mariage", "Big wedding"), fr), money(c.wedding_cost(true), fr)), theme) {
                                c.marry(true);
                            }
                            if !can {
                                muted(ui, tx(("(ensemble depuis 20 min et un bonheur d'au moins 60)", "(together 20 min and happiness of 60 at least)"), fr), theme);
                            }
                        }
                        if button(ui, c.can_have_kid(), tx(("Avoir un enfant", "Have a child"), fr), theme) {
                            c.have_kid();
                        }
                        if !c.can_have_kid() {
                            let why = if p.happiness < 50.0 { tx(("(un bonheur d'au moins 50)", "(happiness of 50 at least)"), fr) } else { tx(("(une chambre libre : déménage)", "(a free bedroom: move)"), fr) };
                            muted(ui, why, theme);
                        }
                    });
                }
            }
            let mut school = None;
            for (k, kid) in l.kids.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(&kid.name).size(13.5).color(theme.text).strong());
                    muted(ui, format!("{:.0} {} · {}", kid.years().floor(), tx(("ans", "years old"), fr), tx(kid.stage(), fr)), theme);
                    if !kid.grown {
                        muted(ui, per_min(kid.cost(), fr), theme);
                        if (3.0..18.0).contains(&kid.years()) || kid.private_school {
                            let mut private = kid.private_school;
                            if ui.checkbox(&mut private, egui::RichText::new(tx(("École privée (plus cher, meilleur niveau plus tard)", "Private school (dearer, a better level later)"), fr)).size(12.0)).changed() {
                                school = Some((k, private));
                            }
                        }
                    }
                });
            }
            if let Some((k, private)) = school {
                c.set_school(k, private);
            }
            if l.kids.iter().any(|k| !k.grown) {
                muted(ui, tx(("Un an passe toutes les 15 minutes. À 23 ans, ils quittent la maison… et frappent à la porte de ta boîte.", "A year goes by every 15 minutes. At 23, they leave home… and knock at your company's door."), fr), theme);
            }
        });
        ui.add_space(8.0);

        // The home and the car.
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, tx(("Logement", "Home"), fr), theme);
            muted(ui, format!("{} · {} {} · {} {}", tx(l.home().name, fr), l.home().rooms, tx(("chambre(s)", "bedroom(s)"), fr), tx(("confort", "comfort"), fr), l.home().comfort), theme);
            if l.loan > 0.0 {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("{} {} ({} {})", tx(("Crédit immobilier :", "Mortgage:"), fr), money(l.loan, fr), per_min(l.loan_pay, fr), tx(("de mensualités", "of repayments"), fr))).size(12.5).color(theme.ansi[3]));
                    if small(ui, format!("{} ({})", tx(("Tout rembourser", "Pay it all off"), fr), money(l.loan, fr))) {
                        c.repay_loan();
                    }
                });
            }
            let mut buy = None;
            for (k, h) in HOMES.iter().enumerate().skip(1) {
                if k == l.home as usize {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(h.name, fr)).size(13.0).color(theme.text));
                    muted(ui, format!("{} · {} {} · {}", money(h.price, fr), h.rooms, tx(("ch.", "bd."), fr), per_min(h.upkeep, fr)), theme);
                    if small_enabled(ui, c.can_buy_home(k, false), format!("{} ({})", tx(("Acheter", "Buy"), fr), money(c.home_cost(k, false), fr))).clicked() {
                        buy = Some((k, false));
                    }
                    if h.price >= 100_000.0 && small_enabled(ui, c.can_buy_home(k, true), format!("{} ({})", tx(("À crédit", "With a mortgage"), fr), money(c.home_cost(k, true), fr))).on_hover_text(tx(("Un cinquième tout de suite, le reste (10 % de plus) remboursé en 4 ans de jeu (une heure).", "A fifth now, the rest (10 % more) paid back over 4 game years (an hour)."), fr)).clicked() {
                        buy = Some((k, true));
                    }
                });
            }
            if let Some((k, loan)) = buy {
                c.buy_home(k, loan);
            }
            ui.add_space(8.0);
            heading(ui, tx(("Voiture", "Car"), fr), theme);
            muted(ui, tx(l.car().name, fr), theme);
            let mut car = None;
            ui.horizontal_wrapped(|ui| {
                for (k, x) in CARS.iter().enumerate().skip(1) {
                    if k == l.car as usize {
                        continue;
                    }
                    if small_enabled(ui, l.account >= c.car_cost(k), format!("{} ({})", tx(x.name, fr), money(c.car_cost(k), fr))).on_hover_text(format!("{} {} · {} {}", tx(("prix", "price"), fr), money(x.price, fr), tx(("entretien", "upkeep"), fr), per_min(x.upkeep, fr))).clicked() {
                        car = Some(k);
                    }
                }
            });
            if let Some(k) = car {
                c.buy_car(k);
            }
        });
        ui.add_space(8.0);

        // The placements.
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, tx(("Placements", "Investments"), fr), theme);
            muted(ui, tx(("Un an (15 min) rapporte en moyenne ce qui est indiqué ; les actions et surtout la crypto montent et descendent.", "A year (15 min) brings on average what's shown; shares and above all crypto go up and down."), fr), theme);
            let held = [l.savings, l.stocks, l.crypto];
            let mut act = None;
            for (k, (name, mu, sigma)) in PLACEMENTS.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(*name, fr)).size(13.0).color(theme.text));
                    ui.label(egui::RichText::new(money(held[k], fr)).size(13.0).color(theme.accent).strong());
                    muted(ui, format!("+{:.0} %/{} · {} {:.0} %", mu * 100.0, tx(("an", "year"), fr), tx(("volatilité", "volatility"), fr), sigma * 100.0), theme);
                    let tenth = (l.account * 0.1).floor();
                    if small_enabled(ui, tenth >= 1.0, format!("+ {}", money(tenth, fr))).clicked() {
                        act = Some((k, tenth));
                    }
                    if small_enabled(ui, held[k] >= 1.0, tx(("Tout retirer", "Take it all out"), fr)).clicked() {
                        act = Some((k, -held[k]));
                    }
                });
            }
            if let Some((k, amount)) = act {
                c.place(k, amount);
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{} : {}", tx(("Appartements en location", "Flats to let"), fr), l.rentals)).size(13.0).color(theme.text));
                muted(ui, format!("+{}", per_min(l.rent_income(), fr)), theme);
                if small_enabled(ui, l.account >= RENTAL_PRICE, format!("{} ({})", tx(("En acheter un", "Buy one"), fr), money(RENTAL_PRICE, fr))).clicked() {
                    c.buy_rental();
                }
                if small_enabled(ui, l.rentals > 0, format!("{} (+{})", tx(("En revendre un", "Sell one"), fr), money(RENTAL_PRICE * 0.95, fr))).clicked() {
                    c.sell_rental();
                }
            });
        });
        ui.add_space(8.0);

        // The hobbies and the luxuries.
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, &format!("{} ({}/{MAX_HOBBIES})", tx(("Loisirs", "Hobbies"), fr), l.hobbies.len()), theme);
            let mut toggle = None;
            ui.horizontal_wrapped(|ui| {
                for h in HOBBIES.iter() {
                    let on = l.does(h.id);
                    let b = egui::Button::new(egui::RichText::new(format!("{} · {}", tx(h.name, fr), per_min(h.upkeep, fr))).size(12.5).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(12.0);
                    if ui.add_enabled(on || l.hobbies.len() < MAX_HOBBIES, b).on_hover_text(tx(h.about, fr)).clicked() {
                        toggle = Some(h.id);
                    }
                }
            });
            if let Some(id) = toggle {
                c.toggle_hobby(id);
            }
            ui.add_space(8.0);
            heading(ui, tx(("Le luxe", "Luxury"), fr), theme);
            let mut buy = None;
            let mut sell = None;
            for x in LUXURIES.iter() {
                let have = l.has(x.id);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(x.name, fr)).size(13.0).color(if have { theme.ansi[2] } else { theme.text }));
                    muted(ui, tx(x.about, fr), theme);
                    if x.upkeep > 0.0 {
                        muted(ui, format!("{} {}", tx(("entretien", "upkeep"), fr), per_min(x.upkeep, fr)), theme);
                    }
                    if have {
                        let back = x.price * if x.id == "art" { 1.0 } else { 0.6 };
                        if small(ui, format!("{} (+{})", tx(("Revendre", "Sell"), fr), money(back, fr))) {
                            sell = Some(x.id);
                        }
                    } else if small_enabled(ui, l.account >= x.price, format!("{} ({})", tx(("S'offrir", "Treat yourself"), fr), money(x.price, fr))).clicked() {
                        buy = Some(x.id);
                    }
                });
            }
            if let Some(id) = buy {
                c.buy_luxury(id);
            }
            if let Some(id) = sell {
                c.sell_luxury(id);
            }
        });
        let _ = self;
    }
}

/// A small button, greyed when not possible.
fn small_enabled(ui: &mut Ui, enabled: bool, text: impl Into<String>) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(egui::RichText::new(text.into()).size(12.0)).corner_radius(6.0))
}
