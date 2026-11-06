//! The dancing pig of the chat (unlocked at Backstage, 1000 points and more): sent to a conversation,
//! it dances a few seconds in the middle of the screen of everyone in it, the sender's too. Drawn
//! here, standing on its hind legs: it sways, bobs, waves its front legs to the beat and taps its
//! feet, music notes rising around it.

use std::f32::consts::{PI, TAU};

use super::*;

/// How long it dances (seconds), fading in and out.
pub(super) const DANCE: f32 = 5.0;
const FADE_IN: f32 = 0.3;
const FADE_OUT: f32 = 0.6;
/// Beats a second.
const BEATS: f32 = 2.0;

const PINK: Color32 = Color32::from_rgb(0xf4, 0xa6, 0xb8);
const PINK_LIGHT: Color32 = Color32::from_rgb(0xfa, 0xc8, 0xd3);
const PINK_DARK: Color32 = Color32::from_rgb(0xd9, 0x74, 0x8e);
const HOOF: Color32 = Color32::from_rgb(0x7a, 0x4a, 0x55);
const EYE: Color32 = Color32::from_rgb(0x2b, 0x1d, 0x22);
const NOTES: [Color32; 3] = [Color32::from_rgb(0x6c, 0x9c, 0xff), Color32::from_rgb(0xff, 0xb3, 0x47), Color32::from_rgb(0x5f, 0xd3, 0x8d)];

/// How visible it is `t` seconds in (0 to 1).
fn opacity(t: f32) -> f32 {
    if t < FADE_IN {
        t / FADE_IN
    } else if t > DANCE - FADE_OUT {
        ((DANCE - t) / FADE_OUT).max(0.0)
    } else {
        1.0
    }
}

/// The pig `t` seconds into its dance, its feet's middle at `feet`, `s` its scale (1: some 260 px
/// high), every color faded by `alpha`.
pub(super) fn paint_dancing_pig(painter: &egui::Painter, feet: Pos2, s: f32, t: f32, alpha: f32) {
    let c = |color: Color32| color.gamma_multiply(alpha);
    let beat = t * BEATS * TAU;
    // A sway a bar (two beats), a bob each beat.
    let sway = (beat / 2.0).sin() * 16.0 * s;
    let bob = -(beat / 2.0).sin().abs() * 12.0 * s;
    let stroke = Stroke::new(2.5 * s, c(PINK_DARK));

    // Its shadow, smaller as it rises.
    let lift = -bob / (12.0 * s).max(0.01);
    painter.add(egui::Shape::ellipse_filled(feet + Vec2::new(sway * 0.4, 4.0 * s), Vec2::new((58.0 - 10.0 * lift) * s, 9.0 * s), c(Color32::from_black_alpha(60))));

    // The hind legs, tapping in turn.
    let hip_y = feet.y - 46.0 * s + bob;
    for (side, phase) in [(-1.0f32, 0.0f32), (1.0, PI)] {
        let up = (beat + phase).sin().max(0.0) * 14.0 * s;
        let hip = Pos2::new(feet.x + sway * 0.6 + side * 22.0 * s, hip_y);
        let foot = Pos2::new(feet.x + sway * 0.3 + side * 26.0 * s, feet.y - up);
        painter.line_segment([hip, foot], Stroke::new(17.0 * s, c(PINK)));
        painter.add(egui::Shape::ellipse_filled(foot + Vec2::new(0.0, 2.0 * s), Vec2::new(11.0 * s, 6.0 * s), c(HOOF)));
    }

    // The curly tail, behind on its right.
    let body = Pos2::new(feet.x + sway, feet.y - 96.0 * s + bob);
    let tail_root = body + Vec2::new(46.0 * s, 24.0 * s);
    let curl: Vec<Pos2> = (0..=24)
        .map(|k| {
            let a = k as f32 / 24.0 * 2.2 * TAU + beat;
            let r = (3.0 + k as f32 * 0.45) * s;
            tail_root + Vec2::new(k as f32 * 0.9 * s + a.cos() * r, a.sin() * r)
        })
        .collect();
    painter.add(egui::Shape::line(curl, Stroke::new(3.0 * s, c(PINK_DARK))));

    // The body and its belly.
    painter.add(egui::Shape::ellipse_filled(body, Vec2::new(50.0 * s, 64.0 * s), c(PINK)));
    painter.add(egui::Shape::ellipse_stroke(body, Vec2::new(50.0 * s, 64.0 * s), stroke));
    painter.add(egui::Shape::ellipse_filled(body + Vec2::new(0.0, 10.0 * s), Vec2::new(30.0 * s, 42.0 * s), c(PINK_LIGHT)));

    // The front legs, waved to the beat: up and down, one then the other.
    for (side, phase) in [(-1.0f32, 0.0f32), (1.0, PI)] {
        let shoulder = body + Vec2::new(side * 40.0 * s, -30.0 * s);
        let wave = (beat + phase).sin();
        let angle = -PI / 2.0 + side * (0.5 + 0.75 * wave);
        let elbow = shoulder + Vec2::new(side * 22.0 * s, -6.0 * s * wave);
        let hand = elbow + Vec2::new(angle.cos(), angle.sin()) * 30.0 * s;
        painter.add(egui::Shape::line(vec![shoulder, elbow, hand], Stroke::new(14.0 * s, c(PINK))));
        painter.circle_filled(hand, 7.5 * s, c(HOOF));
    }

    // The head, swaying a little more than the body.
    let head = Pos2::new(feet.x + sway * 1.35, body.y - 78.0 * s + bob * 0.4);
    // Its ears, flapping.
    for side in [-1.0f32, 1.0] {
        let flap = (beat + side).sin() * 4.0 * s;
        let ear = vec![head + Vec2::new(side * 16.0 * s, -30.0 * s), head + Vec2::new(side * 40.0 * s, -50.0 * s + flap), head + Vec2::new(side * 36.0 * s, -16.0 * s)];
        painter.add(egui::Shape::convex_polygon(ear, c(PINK), stroke));
    }
    painter.circle_filled(head, 40.0 * s, c(PINK));
    painter.circle_stroke(head, 40.0 * s, stroke);
    // Its cheeks.
    for side in [-1.0f32, 1.0] {
        painter.add(egui::Shape::ellipse_filled(head + Vec2::new(side * 24.0 * s, 12.0 * s), Vec2::new(8.0 * s, 5.0 * s), c(Color32::from_rgb(0xf0, 0x80, 0x9c).gamma_multiply(0.7))));
    }
    // Its eyes, blinking now and then.
    let blink = (t * 0.6).fract() > 0.94;
    for side in [-1.0f32, 1.0] {
        let eye = head + Vec2::new(side * 14.0 * s, -8.0 * s);
        if blink {
            painter.line_segment([eye - Vec2::new(5.0 * s, 0.0), eye + Vec2::new(5.0 * s, 0.0)], Stroke::new(2.5 * s, c(EYE)));
        } else {
            painter.circle_filled(eye, 5.5 * s, c(EYE));
            painter.circle_filled(eye + Vec2::new(-1.8 * s, -1.8 * s), 1.8 * s, c(Color32::WHITE));
        }
    }
    // Its snout, its nostrils, a smile under it.
    let snout = head + Vec2::new(0.0, 10.0 * s);
    painter.add(egui::Shape::ellipse_filled(snout, Vec2::new(17.0 * s, 12.0 * s), c(PINK_LIGHT)));
    painter.add(egui::Shape::ellipse_stroke(snout, Vec2::new(17.0 * s, 12.0 * s), stroke));
    for side in [-1.0f32, 1.0] {
        painter.add(egui::Shape::ellipse_filled(snout + Vec2::new(side * 6.0 * s, 0.0), Vec2::new(2.8 * s, 4.0 * s), c(PINK_DARK)));
    }
    let smile: Vec<Pos2> = (0..=10).map(|k| {
        let a = PI * (0.2 + 0.6 * k as f32 / 10.0);
        head + Vec2::new(a.cos() * 13.0 * s, 24.0 * s + a.sin() * 6.0 * s)
    }).collect();
    painter.add(egui::Shape::line(smile, Stroke::new(2.2 * s, c(EYE))));

    // Music notes rising around it, one after the other.
    for k in 0..5 {
        let life = (t * 0.7 + k as f32 / 5.0).fract();
        let side = if k % 2 == 0 { -1.0 } else { 1.0 };
        let at = Pos2::new(feet.x + side * (95.0 + 20.0 * (k as f32 * 1.7).sin()) * s + (life * TAU).sin() * 10.0 * s, feet.y - 60.0 * s - life * 220.0 * s);
        let fade = (1.0 - life) * (life * 6.0).min(1.0);
        paint_note(painter, at, s, c(NOTES[k % NOTES.len()]).gamma_multiply(fade), k % 2 == 0);
    }
}

/// A music note: a quaver, or two joined (`double`).
fn paint_note(painter: &egui::Painter, at: Pos2, s: f32, color: Color32, double: bool) {
    let stem = Stroke::new(2.5 * s, color);
    let head = |p: Pos2| painter.add(egui::Shape::ellipse_filled(p, Vec2::new(7.0 * s, 5.0 * s), color));
    head(at);
    let top = at + Vec2::new(6.0 * s, -28.0 * s);
    painter.line_segment([at + Vec2::new(6.0 * s, 0.0), top], stem);
    if double {
        let other = at + Vec2::new(22.0 * s, -4.0 * s);
        head(other);
        let other_top = other + Vec2::new(6.0 * s, -28.0 * s);
        painter.line_segment([other + Vec2::new(6.0 * s, 0.0), other_top], stem);
        painter.line_segment([top, other_top], Stroke::new(5.0 * s, color));
    } else {
        painter.add(egui::Shape::line(vec![top, top + Vec2::new(9.0 * s, 9.0 * s), top + Vec2::new(7.0 * s, 18.0 * s)], stem));
    }
}

/// The chat's icon of the pig: its face, its ears, its snout (about 18 px).
pub(super) fn paint_pig_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    for side in [-1.0f32, 1.0] {
        painter.add(egui::Shape::closed_line(vec![c + Vec2::new(side * 3.5, -5.5), c + Vec2::new(side * 8.0, -9.0), c + Vec2::new(side * 7.5, -3.0)], stroke));
    }
    painter.circle_stroke(c + Vec2::new(0.0, 1.0), 7.0, stroke);
    painter.add(egui::Shape::ellipse_stroke(c + Vec2::new(0.0, 3.0), Vec2::new(3.5, 2.5), stroke));
    painter.circle_filled(c + Vec2::new(-1.3, 3.0), 0.8, color);
    painter.circle_filled(c + Vec2::new(1.3, 3.0), 0.8, color);
}

impl App {
    /// The pig dancing over everything, in the middle of the window, who sent it under it.
    pub(super) fn talk_pig(&mut self, ctx: &egui::Context) {
        let Some((since, who)) = &self.talk_ui.pig else { return };
        let Some(t) = Instant::now().checked_duration_since(*since).map(|d| d.as_secs_f32()) else {
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        };
        if t >= DANCE {
            self.talk_ui.pig = None;
            return;
        }
        ctx.request_repaint();
        let screen = ctx.content_rect();
        let s = (screen.height() / 620.0).clamp(0.55, 1.3);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("talk-pig")));
        let alpha = opacity(t);
        let feet = Pos2::new(screen.center().x, screen.center().y + 140.0 * s);
        paint_dancing_pig(&painter, feet, s, t, alpha);
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
    fn it_fades_in_dances_and_fades_out() {
        assert_eq!(opacity(0.0), 0.0);
        assert_eq!(opacity(FADE_IN), 1.0);
        assert_eq!(opacity(DANCE / 2.0), 1.0);
        assert!(opacity(DANCE - FADE_OUT / 2.0) < 1.0);
        assert_eq!(opacity(DANCE + 1.0), 0.0);
    }
}
