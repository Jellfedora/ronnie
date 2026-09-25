//! Box drawing, block elements and Powerline separators, drawn as shapes so they
//! connect seamlessly across cells regardless of the font or line height.

use egui::{Color32, Pos2, Rect, Shape, Stroke};

/// Weight of one arm of a box-drawing character.
#[derive(Clone, Copy, PartialEq)]
enum W {
    None,
    Light,
    Heavy,
    Double,
}

/// Arms (up, right, down, left) of the supported box-drawing characters.
fn arms(c: char) -> Option<[W; 4]> {
    use W::{Double as D, Heavy as H, Light as L, None as N};
    Some(match c {
        '─' => [N, L, N, L],
        '━' => [N, H, N, H],
        '│' => [L, N, L, N],
        '┃' => [H, N, H, N],
        '┌' | '╭' => [N, L, L, N],
        '┐' | '╮' => [N, N, L, L],
        '└' | '╰' => [L, L, N, N],
        '┘' | '╯' => [L, N, N, L],
        '┏' => [N, H, H, N],
        '┓' => [N, N, H, H],
        '┗' => [H, H, N, N],
        '┛' => [H, N, N, H],
        '├' => [L, L, L, N],
        '┤' => [L, N, L, L],
        '┬' => [N, L, L, L],
        '┴' => [L, L, N, L],
        '┼' => [L, L, L, L],
        '┣' => [H, H, H, N],
        '┫' => [H, N, H, H],
        '┳' => [N, H, H, H],
        '┻' => [H, H, N, H],
        '╋' => [H, H, H, H],
        '═' => [N, D, N, D],
        '║' => [D, N, D, N],
        '╔' => [N, D, D, N],
        '╗' => [N, N, D, D],
        '╚' => [D, D, N, N],
        '╝' => [D, N, N, D],
        '╠' => [D, D, D, N],
        '╣' => [D, N, D, D],
        '╦' => [N, D, D, D],
        '╩' => [D, D, N, D],
        '╬' => [D, D, D, D],
        '╴' => [N, N, N, L],
        '╵' => [L, N, N, N],
        '╶' => [N, L, N, N],
        '╷' => [N, N, L, N],
        _ => return None,
    })
}

/// Shapes for `c` drawn in `rect`, or `None` if the font should render it.
pub fn shapes(c: char, rect: Rect, color: Color32, ppp: f32) -> Option<Vec<Shape>> {
    if let Some(arms) = arms(c) {
        return Some(lines(arms, rect, color, ppp));
    }
    blocks(c, rect, color).or_else(|| powerline(c, rect, color))
}

fn snap(v: f32, ppp: f32) -> f32 {
    (v * ppp).round() / ppp
}

fn lines(arms: [W; 4], rect: Rect, color: Color32, ppp: f32) -> Vec<Shape> {
    let light = snap((rect.width() / 8.0).max(1.0 / ppp), ppp).max(1.0 / ppp);
    let heavy = light * 2.0;
    let gap = light * 2.0; // distance between the strokes of a double line
    let cx = snap(rect.center().x, ppp);
    let cy = snap(rect.center().y, ppp);
    let mut out = Vec::new();

    let mut bar = |min: Pos2, max: Pos2| out.push(Shape::rect_filled(Rect::from_min_max(min, max), 0.0, color));

    for (i, w) in arms.iter().enumerate() {
        let offsets: &[f32] = match w {
            W::None => continue,
            W::Light | W::Heavy => &[0.0],
            W::Double => &[-gap / 2.0 - light / 2.0, gap / 2.0 + light / 2.0],
        };
        let t = if *w == W::Heavy { heavy } else { light };
        // Arms overlap the center by half a stroke so corners are closed.
        let half = if arms.contains(&W::Double) { gap / 2.0 + light } else { heavy / 2.0 };
        for &o in offsets {
            match i {
                0 => bar(Pos2::new(cx + o - t / 2.0, rect.min.y), Pos2::new(cx + o + t / 2.0, cy + half)),
                2 => bar(Pos2::new(cx + o - t / 2.0, cy - half), Pos2::new(cx + o + t / 2.0, rect.max.y)),
                1 => bar(Pos2::new(cx - half, cy + o - t / 2.0), Pos2::new(rect.max.x, cy + o + t / 2.0)),
                _ => bar(Pos2::new(rect.min.x, cy + o - t / 2.0), Pos2::new(cx + half, cy + o + t / 2.0)),
            }
        }
    }
    out
}

fn blocks(c: char, r: Rect, color: Color32) -> Option<Vec<Shape>> {
    let (w, h) = (r.width(), r.height());
    let fill = |min: Pos2, max: Pos2| Shape::rect_filled(Rect::from_min_max(min, max), 0.0, color);
    let shape = match c {
        '█' => fill(r.min, r.max),
        '▀' => fill(r.min, Pos2::new(r.max.x, r.min.y + h / 2.0)),
        '▄' => fill(Pos2::new(r.min.x, r.min.y + h / 2.0), r.max),
        '▌' => fill(r.min, Pos2::new(r.min.x + w / 2.0, r.max.y)),
        '▐' => fill(Pos2::new(r.min.x + w / 2.0, r.min.y), r.max),
        '▔' => fill(r.min, Pos2::new(r.max.x, r.min.y + h / 8.0)),
        '▕' => fill(Pos2::new(r.max.x - w / 8.0, r.min.y), r.max),
        // Lower eighths ▁▂▃▄▅▆▇.
        '\u{2581}'..='\u{2587}' => {
            let n = (c as u32 - 0x2580) as f32;
            fill(Pos2::new(r.min.x, r.max.y - h * n / 8.0), r.max)
        }
        // Left eighths ▉▊▋▌▍▎▏.
        '\u{2589}'..='\u{258f}' => {
            let n = (0x2590 - c as u32) as f32;
            fill(r.min, Pos2::new(r.min.x + w * n / 8.0, r.max.y))
        }
        '░' => Shape::rect_filled(r, 0.0, color.gamma_multiply(0.25)),
        '▒' => Shape::rect_filled(r, 0.0, color.gamma_multiply(0.5)),
        '▓' => Shape::rect_filled(r, 0.0, color.gamma_multiply(0.75)),
        _ => return None,
    };
    Some(vec![shape])
}

fn powerline(c: char, r: Rect, color: Color32) -> Option<Vec<Shape>> {
    let (tl, tr, bl, br, ml, mr) = (
        r.left_top(),
        r.right_top(),
        r.left_bottom(),
        r.right_bottom(),
        Pos2::new(r.min.x, r.center().y),
        Pos2::new(r.max.x, r.center().y),
    );
    let stroke = Stroke::new(1.0, color);
    let shape = match c {
        '\u{e0b0}' => Shape::convex_polygon(vec![tl, mr, bl], color, Stroke::NONE),
        '\u{e0b2}' => Shape::convex_polygon(vec![tr, br, ml], color, Stroke::NONE),
        '\u{e0b1}' => Shape::line(vec![tl, mr, bl], stroke),
        '\u{e0b3}' => Shape::line(vec![tr, ml, br], stroke),
        _ => return None,
    };
    Some(vec![shape])
}
