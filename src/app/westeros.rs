//! Game of Thrones' rewards (unlocked with its theme, three Pause Café levels): the white direwolf cub
//! trotting along the foot of the screen, sitting down halfway to howl (desktop.rs walks it), and
//! Hodor in the chat, sent to a conversation as the dancing pig is: he holds the door a few seconds in
//! the middle of the screen of everyone in it, the sender's too, pale hands groping round its edge.

use super::*;

/// What the direwolf cub says when it sits down to howl.
pub(super) const WOLF_HOWL: &str = "Aouuuuh !";

const FUR: Color32 = Color32::from_rgb(0xf4, 0xf6, 0xf9);
const FUR_SHADE: Color32 = Color32::from_rgb(0xc5, 0xcd, 0xd8);
const FUR_LINE: Color32 = Color32::from_rgb(0x8a, 0x95, 0xa5);
const WOLF_EYE: Color32 = Color32::from_rgb(0xd2, 0x2b, 0x2b);
const NOSE: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x30);

/// The direwolf cub, `h` tall (standing, to its ears), its feet at `foot`, facing `dir` (1 right, -1
/// left): trotting at `step` (app time), or sitting, howling (`None`); `now` wags its tail.
pub(super) fn paint_wolf(painter: &egui::Painter, foot: Pos2, h: f32, dir: f32, step: Option<f32>, now: f64) {
    let k = h / 34.0;
    let line = Stroke::new(0.8 * k, FUR_LINE);
    match step {
        Some(step) => {
            let bob = (step * 10.0).sin().abs() * 1.0;
            let at = |x: f32, y: f32| foot + Vec2::new(x * dir, y - bob) * k;
            // The tail, bushy, wagging behind.
            let wag = ((now as f32) * 6.0).sin() * 2.0;
            for i in 0..5 {
                let f = i as f32 / 4.0;
                painter.circle_filled(at(-10.0 - f * 8.0, -17.0 - f * 4.0 + wag * f), (3.2 - f * 1.2) * k, if i == 4 { FUR } else { FUR_SHADE });
            }
            // The legs, in pairs, each pair its own way at each step.
            for (x, phase) in [(-8.0f32, 0.0f32), (-5.5, std::f32::consts::PI), (6.0, std::f32::consts::PI), (8.5, 0.0)] {
                let swing = (step * 10.0 + phase).sin() * 2.6;
                let top = at(x, -12.0);
                let paw = foot + Vec2::new((x + swing) * dir * k, 0.0);
                painter.line_segment([top, paw], Stroke::new(2.6 * k, FUR_SHADE));
                painter.line_segment([paw, paw + Vec2::new(1.6 * dir * k, 0.0)], Stroke::new(2.0 * k, FUR_LINE));
            }
            // The body, its belly lighter.
            painter.add(egui::Shape::ellipse_filled(at(0.0, -16.0), Vec2::new(12.0, 6.5) * k, FUR));
            painter.add(egui::Shape::ellipse_stroke(at(0.0, -16.0), Vec2::new(12.0, 6.5) * k, line));
            // The neck's ruff, the head.
            painter.circle_filled(at(9.5, -20.0), 5.0 * k, FUR);
            head(painter, at(13.0, -25.0), k, dir, false);
        }
        None => {
            let at = |x: f32, y: f32| foot + Vec2::new(x * dir, y) * k;
            // Sitting: the tail along the ground, the haunch, the chest up, the front legs straight.
            let wag = ((now as f32) * 3.0).sin() * 1.0;
            for i in 0..5 {
                let f = i as f32 / 4.0;
                painter.circle_filled(at(-8.0 - f * 9.0, -2.5 + wag * f), (3.0 - f * 1.0) * k, if i == 4 { FUR } else { FUR_SHADE });
            }
            painter.add(egui::Shape::ellipse_filled(at(-3.0, -7.5), Vec2::new(8.0, 7.5) * k, FUR_SHADE));
            painter.add(egui::Shape::ellipse_filled(at(2.0, -16.0), Vec2::new(6.5, 10.0) * k, FUR));
            painter.add(egui::Shape::ellipse_stroke(at(2.0, -16.0), Vec2::new(6.5, 10.0) * k, line));
            for x in [3.5f32, 6.0] {
                let paw = at(x, 0.0);
                painter.line_segment([at(x, -10.0), paw], Stroke::new(2.6 * k, FUR_SHADE));
                painter.line_segment([paw, paw + Vec2::new(1.6 * dir * k, 0.0)], Stroke::new(2.0 * k, FUR_LINE));
            }
            // The head thrown back, the muzzle to the sky.
            head(painter, at(5.0, -29.0), k, dir, true);
        }
    }
}

/// The direwolf's head around `c`: its ears, its muzzle forward (or up to the sky, howling), its red
/// eye.
fn head(painter: &egui::Painter, c: Pos2, k: f32, dir: f32, howling: bool) {
    let at = |x: f32, y: f32| c + Vec2::new(x * dir, y) * k;
    let line = Stroke::new(0.8 * k, FUR_LINE);
    // The ears, laid back when howling.
    let ears = if howling { [(-4.0, -2.0, -8.5, -6.0, -2.0, -5.0), (-1.5, -3.5, -5.0, -8.5, 0.5, -4.5)] } else { [(-3.5, -3.0, -3.0, -9.0, 0.0, -4.0), (-0.5, -4.0, 1.0, -9.5, 2.5, -3.5)] };
    for (ax, ay, bx, by, cx, cy) in ears {
        painter.add(egui::Shape::convex_polygon(vec![at(ax, ay), at(bx, by), at(cx, cy)], FUR_SHADE, line));
    }
    painter.circle_filled(c, 4.6 * k, FUR);
    painter.circle_stroke(c, 4.6 * k, line);
    // The muzzle, its nose at the tip; open when howling.
    let muzzle = if howling { vec![at(1.5, -3.5), at(6.5, -9.5), at(8.0, -8.0), at(4.5, -1.0)] } else { vec![at(2.5, -2.0), at(9.5, -0.5), at(9.0, 2.0), at(2.5, 3.0)] };
    let tip = if howling { at(7.6, -9.4) } else { at(9.6, 0.2) };
    painter.add(egui::Shape::convex_polygon(muzzle, FUR, line));
    painter.circle_filled(tip, 1.1 * k, NOSE);
    if howling {
        painter.line_segment([at(4.8, -4.5), at(7.2, -6.8)], Stroke::new(1.0 * k, NOSE));
        painter.circle_filled(at(0.8, -1.2), 0.9 * k, WOLF_EYE);
    } else {
        painter.circle_filled(at(1.2, -1.0), 0.9 * k, WOLF_EYE);
    }
}

/// The howl's bubble over the sitting cub (its feet at `foot`).
pub(super) fn paint_howl(painter: &egui::Painter, foot: Pos2) {
    let ink = Color32::from_rgb(0x1e, 0x26, 0x33);
    let paper = Color32::from_rgb(0xee, 0xf3, 0xf9);
    let galley = painter.layout_no_wrap(WOLF_HOWL.to_owned(), FontId::proportional(14.0), ink);
    let r = Rect::from_center_size(foot + Vec2::new(0.0, -62.0), galley.size() + Vec2::new(18.0, 10.0));
    painter.rect(r, 8.0, paper, Stroke::new(1.0, FUR_LINE), egui::StrokeKind::Inside);
    painter.add(egui::Shape::convex_polygon(vec![Pos2::new(r.center().x - 5.0, r.max.y), Pos2::new(r.center().x + 5.0, r.max.y), Pos2::new(r.center().x, r.max.y + 7.0)], paper, Stroke::NONE));
    painter.galley(r.center() - galley.size() / 2.0, galley, ink);
}

/// How long Hodor holds the door (seconds), fading in and out.
pub(super) const HOLD: f32 = 5.0;
const FADE_IN: f32 = 0.3;
const FADE_OUT: f32 = 0.6;

const WOOD: Color32 = Color32::from_rgb(0x6e, 0x4b, 0x2c);
const WOOD_DARK: Color32 = Color32::from_rgb(0x48, 0x30, 0x1c);
const IRON: Color32 = Color32::from_rgb(0x3a, 0x3d, 0x42);
const SKIN: Color32 = Color32::from_rgb(0xe0, 0xb0, 0x88);
const SKIN_DARK: Color32 = Color32::from_rgb(0xb8, 0x86, 0x62);
const HAIR: Color32 = Color32::from_rgb(0x5c, 0x40, 0x28);
const TUNIC: Color32 = Color32::from_rgb(0x7a, 0x5f, 0x45);
const TUNIC_DARK: Color32 = Color32::from_rgb(0x55, 0x40, 0x2d);
const LEATHER: Color32 = Color32::from_rgb(0x3b, 0x2a, 0x1e);
const WIGHT: Color32 = Color32::from_rgb(0xb6, 0xc6, 0xd8);
const FROST: Color32 = Color32::from_rgb(0x8f, 0xc8, 0xff);

/// How visible he is `t` seconds in (0 to 1).
fn opacity(t: f32) -> f32 {
    if t < FADE_IN {
        t / FADE_IN
    } else if t > HOLD - FADE_OUT {
        ((HOLD - t) / FADE_OUT).max(0.0)
    } else {
        1.0
    }
}

/// Hodor holding the door, `t` seconds in: the door's foot and his at `feet`'s height, the whole
/// centred on `feet.x`, `s` its scale (1: some 270 px high), every color faded by `alpha`.
pub(super) fn paint_hodor(painter: &egui::Painter, feet: Pos2, s: f32, t: f32, alpha: f32) {
    let c = |color: Color32| color.gamma_multiply(alpha);
    // The door shakes under the blows from behind, harder now and then.
    let blows = 0.6 + 0.4 * (t * 2.3).sin().max(0.0);
    let shake = (t * 31.0).sin() * 2.5 * s * blows;
    let door = Rect::from_min_max(Pos2::new(feet.x - 150.0 * s + shake, feet.y - 250.0 * s), Pos2::new(feet.x - 20.0 * s + shake, feet.y));
    // The cold behind: a blue glow round the door's left edge.
    for i in 0..4 {
        let w = (8.0 + i as f32 * 9.0) * s;
        painter.rect_filled(Rect::from_min_max(Pos2::new(door.min.x - w, door.min.y + 8.0 * s), Pos2::new(door.min.x + 2.0 * s, door.max.y)), 0.0, c(FROST.gamma_multiply(0.10)));
    }
    // The pale hands groping out of the gap, each its own way.
    for (i, y) in [0.22f32, 0.45, 0.63, 0.82].iter().enumerate() {
        let reach = (12.0 + 10.0 * ((t * 4.0 + i as f32 * 1.7).sin() * 0.5 + 0.5)) * s;
        let wrist = Pos2::new(door.min.x + 4.0 * s, door.min.y + door.height() * y);
        let palm = wrist + Vec2::new(-reach, ((t * 3.0 + i as f32).sin()) * 4.0 * s);
        painter.line_segment([wrist, palm], Stroke::new(7.0 * s, c(WIGHT)));
        for f in 0..4 {
            let a = (150.0 + f as f32 * 22.0 + (t * 6.0 + f as f32).sin() * 8.0).to_radians();
            painter.line_segment([palm, palm + Vec2::new(a.cos(), a.sin() - 0.6) * 9.0 * s], Stroke::new(2.4 * s, c(WIGHT)));
        }
    }
    // The door: its planks, two iron bands, its ring.
    painter.rect_filled(door, 3.0 * s, c(WOOD));
    for i in 1..5 {
        let x = door.min.x + door.width() * i as f32 / 5.0;
        painter.line_segment([Pos2::new(x, door.min.y + 2.0 * s), Pos2::new(x, door.max.y - 2.0 * s)], Stroke::new(1.5 * s, c(WOOD_DARK)));
    }
    for y in [0.2f32, 0.75] {
        let band = Rect::from_min_size(Pos2::new(door.min.x, door.min.y + door.height() * y), Vec2::new(door.width(), 12.0 * s));
        painter.rect_filled(band, 0.0, c(IRON));
        for i in 0..5 {
            painter.circle_filled(Pos2::new(band.min.x + (12.0 + i as f32 * 26.0) * s, band.center().y), 2.0 * s, c(Color32::from_rgb(0x6a, 0x6e, 0x75)));
        }
    }
    painter.rect_stroke(door, 3.0 * s, Stroke::new(3.0 * s, c(WOOD_DARK)), egui::StrokeKind::Inside);
    painter.circle_stroke(Pos2::new(door.max.x - 22.0 * s, door.center().y + 10.0 * s), 7.0 * s, Stroke::new(2.5 * s, c(IRON)));

    // Hodor, braced against it, leaning in with each blow.
    let lean = shake * 0.6;
    let hx = feet.x + 70.0 * s;
    // The feet stay put; all above them leans.
    let at = |x: f32, y: f32| Pos2::new(hx + x * s, feet.y + y * s);
    let top = |x: f32, y: f32| Pos2::new(hx + x * s + lean, feet.y + y * s);
    // His shadow.
    painter.add(egui::Shape::ellipse_filled(at(0.0, 4.0), Vec2::new(70.0 * s, 9.0 * s), c(Color32::from_black_alpha(60))));
    // The legs, wide apart, pushing: the back one stretched out.
    painter.line_segment([top(-16.0, -95.0), at(-38.0, -4.0)], Stroke::new(20.0 * s, c(TUNIC_DARK)));
    painter.line_segment([top(16.0, -95.0), at(40.0, -4.0)], Stroke::new(20.0 * s, c(TUNIC_DARK)));
    for x in [-38.0f32, 40.0] {
        painter.add(egui::Shape::ellipse_filled(at(x - 6.0, -4.0), Vec2::new(16.0 * s, 7.0 * s), c(LEATHER)));
    }
    // The tunic, the belt.
    painter.add(egui::Shape::convex_polygon(vec![top(-46.0, -205.0), top(46.0, -205.0), top(40.0, -88.0), top(-40.0, -88.0)], c(TUNIC), Stroke::new(2.0 * s, c(TUNIC_DARK))));
    painter.rect_filled(Rect::from_min_max(top(-42.0, -112.0), top(42.0, -100.0)), 2.0 * s, c(LEATHER));
    painter.rect_filled(Rect::from_center_size(top(0.0, -106.0), Vec2::new(12.0 * s, 10.0 * s)), 1.0 * s, c(Color32::from_rgb(0x9a, 0x86, 0x5a)));
    // The arms: both out to the door, palms flat on it.
    let hand_x = door.max.x - 6.0 * s;
    for (shoulder, y) in [(top(-40.0, -196.0), -175.0f32), (top(-24.0, -186.0), -128.0)] {
        let hand = Pos2::new(hand_x, feet.y + y * s);
        painter.line_segment([shoulder, hand], Stroke::new(17.0 * s, c(TUNIC_DARK)));
        painter.line_segment([shoulder + (hand - shoulder) * 0.6, hand], Stroke::new(14.0 * s, c(SKIN)));
        painter.add(egui::Shape::ellipse_filled(hand, Vec2::new(7.0 * s, 11.0 * s), c(SKIN)));
        painter.add(egui::Shape::ellipse_stroke(hand, Vec2::new(7.0 * s, 11.0 * s), Stroke::new(1.5 * s, c(SKIN_DARK))));
    }
    // The head: the hair, the beard, eyes screwed up, mouth wide open on his one word.
    let shout = 0.6 + 0.4 * (t * 5.0).sin().abs();
    let face = top(-6.0, -232.0);
    painter.circle_filled(face + Vec2::new(0.0, -6.0 * s), 27.0 * s, c(HAIR));
    painter.circle_filled(face, 23.0 * s, c(SKIN));
    painter.add(egui::Shape::convex_polygon(vec![face + Vec2::new(-22.0, 2.0) * s, face + Vec2::new(22.0, 2.0) * s, face + Vec2::new(14.0, 30.0) * s, face + Vec2::new(-14.0, 30.0) * s], c(HAIR), Stroke::NONE));
    painter.add(egui::Shape::ellipse_filled(face + Vec2::new(0.0, 13.0 * s), Vec2::new(8.0 * s, 9.0 * s * shout), c(Color32::from_rgb(0x3a, 0x16, 0x14))));
    for side in [-1.0f32, 1.0] {
        let eye = face + Vec2::new(side * 9.0, -5.0) * s;
        painter.line_segment([eye + Vec2::new(-4.0, -1.5 * side) * s, eye + Vec2::new(4.0, 1.5 * side) * s], Stroke::new(2.5 * s, c(HAIR)));
        painter.line_segment([eye + Vec2::new(-4.0, 1.0) * s, eye + Vec2::new(4.0, 1.0) * s], Stroke::new(1.8 * s, c(Color32::from_rgb(0x2b, 0x1d, 0x16))));
    }
    // HODOR!, swelling with each shout.
    let font = FontId::proportional(54.0 * s * (0.9 + 0.2 * shout));
    let galley = painter.layout_no_wrap("HODOR !".to_owned(), font, c(Color32::WHITE));
    let at = Pos2::new(feet.x - galley.size().x / 2.0 + (t * 23.0).sin() * 2.0 * s, feet.y - 330.0 * s);
    for (dx, dy) in [(-2.0f32, 0.0f32), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0), (0.0, 4.0)] {
        painter.galley_with_override_text_color(at + Vec2::new(dx, dy) * s, galley.clone(), c(Color32::from_rgb(0x1a, 0x12, 0x0c)));
    }
    painter.galley(at, galley, c(Color32::WHITE));
}

/// The chat's icon of Hodor: a door, its planks, its ring (about 18 px).
pub(super) fn paint_hodor_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let door = Rect::from_center_size(c, Vec2::new(12.0, 17.0));
    painter.rect_stroke(door, 1.5, stroke, egui::StrokeKind::Middle);
    painter.line_segment([Pos2::new(c.x - 2.0, door.min.y + 2.0), Pos2::new(c.x - 2.0, door.max.y - 2.0)], Stroke::new(1.0, color));
    painter.circle_filled(c + Vec2::new(3.0, 1.0), 1.3, color);
    // The hand on its edge.
    painter.line_segment([Pos2::new(door.min.x - 3.5, c.y - 3.0), Pos2::new(door.min.x - 0.5, c.y - 2.0)], stroke);
    painter.line_segment([Pos2::new(door.min.x - 3.5, c.y + 1.0), Pos2::new(door.min.x - 0.5, c.y + 1.0)], stroke);
}

impl App {
    /// Hodor holding the door over everything, in the middle of the window, who sent him under it.
    pub(super) fn talk_hodor(&mut self, ctx: &egui::Context) {
        let Some((since, who)) = &self.talk_ui.hodor else { return };
        let Some(t) = Instant::now().checked_duration_since(*since).map(|d| d.as_secs_f32()) else {
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        };
        if t >= HOLD {
            self.talk_ui.hodor = None;
            return;
        }
        ctx.request_repaint();
        let screen = ctx.content_rect();
        let s = (screen.height() / 640.0).clamp(0.5, 1.25);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("talk-hodor")));
        let alpha = opacity(t);
        let feet = Pos2::new(screen.center().x, screen.center().y + 150.0 * s);
        paint_hodor(&painter, feet, s, t, alpha);
        let font = FontId::proportional(18.0 * s.max(0.8));
        let galley = painter.layout_no_wrap(who.clone(), font, self.theme.text.gamma_multiply(alpha));
        let pill = Rect::from_center_size(feet + Vec2::new(0.0, 34.0 * s), galley.size() + Vec2::new(24.0, 12.0));
        painter.rect_filled(pill, pill.height() / 2.0, self.theme.chrome_bg.gamma_multiply(0.92 * alpha));
        painter.galley(pill.center() - galley.size() / 2.0, galley, self.theme.text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hodor_fades_in_holds_and_fades_out() {
        assert_eq!(opacity(0.0), 0.0);
        assert_eq!(opacity(FADE_IN), 1.0);
        assert_eq!(opacity(HOLD / 2.0), 1.0);
        assert_eq!(opacity(HOLD), 0.0);
        assert!((0..50).all(|k| (0.0..=1.0).contains(&opacity(k as f32 * HOLD / 50.0))));
    }
}
