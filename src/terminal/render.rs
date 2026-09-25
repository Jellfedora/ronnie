use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use alacritty_terminal::Term;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, Vec2};

use super::boxdraw;
use crate::theme::Theme;

/// The four monospace faces used by the terminal, registered in `main.rs`.
pub struct FontSet {
    pub size: f32,
    /// Row height as a multiple of the font's natural line height.
    pub line_height: f32,
}

impl FontSet {
    fn font(&self, bold: bool, italic: bool) -> FontId {
        let name = match (bold, italic) {
            (false, false) => "mono",
            (true, false) => "mono-bold",
            (false, true) => "mono-italic",
            (true, true) => "mono-bold-italic",
        };
        FontId::new(self.size, FontFamily::Name(name.into()))
    }

    /// Cell size, snapped to physical pixels so rows never drift.
    pub fn cell_size(&self, ui: &Ui) -> Vec2 {
        let ppp = ui.ctx().pixels_per_point();
        let font = self.font(false, false);
        let (w, h) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        Vec2::new(w, ((h * self.line_height) * ppp).round() / ppp)
    }
}

/// Characters guaranteed to be in the main font with the cell's exact advance:
/// they can share a single text layout per row without misaligning.
fn is_grid_safe(c: char) -> bool {
    c.is_ascii() || ('\u{a0}'..='\u{24f}').contains(&c) || ('\u{2500}'..='\u{259f}').contains(&c)
}


#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: Color32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

/// Accumulates a run of grid-safe characters in one row into a single galley.
struct Run {
    col: usize,
    text: String,
    style: Option<Style>,
    job: LayoutJob,
}

impl Run {
    fn new(col: usize) -> Self {
        Self { col, text: String::new(), style: None, job: LayoutJob::default() }
    }

    fn push(&mut self, c: char, style: Style, fonts: &FontSet) {
        if self.style != Some(style) {
            self.commit(fonts);
            self.style = Some(style);
        }
        self.text.push(c);
    }

    fn commit(&mut self, fonts: &FontSet) {
        if let Some(s) = self.style {
            if !self.text.is_empty() {
                let line = |on: bool| if on { Stroke::new(1.0, s.fg) } else { Stroke::NONE };
                let format = TextFormat {
                    font_id: fonts.font(s.bold, s.italic),
                    color: s.fg,
                    underline: line(s.underline),
                    strikethrough: line(s.strike),
                    ..Default::default()
                };
                self.job.append(&self.text, 0.0, format);
                self.text.clear();
            }
        }
    }

    fn flush(&mut self, painter: &Painter, origin: Pos2, cell: Vec2, row: usize, fonts: &FontSet, next_col: usize) {
        self.commit(fonts);
        let job = std::mem::take(&mut self.job);
        // Rows made only of spaces don't need a galley at all.
        if !job.text.trim().is_empty() {
            // Center the glyphs vertically in the (taller) cell.
            let dy = (cell.y - cell.y / fonts.line_height) / 2.0;
            let pos = origin + Vec2::new(self.col as f32 * cell.x, row as f32 * cell.y + dy);
            let galley = painter.layout_job(job);
            painter.galley(pos, galley, Color32::WHITE);
        }
        *self = Run::new(next_col);
    }
}

pub fn paint<L: EventListener>(
    painter: &Painter,
    origin: Pos2,
    cell: Vec2,
    term: &Term<L>,
    theme: &Theme,
    fonts: &FontSet,
    focused: bool,
) {
    let content = term.renderable_content();
    let offset = content.display_offset as i32;
    let colors = content.colors;
    let cursor = content.cursor;
    let default_bg = theme.resolve(Color::Named(NamedColor::Background), colors);
    let show_cursor = content.mode.contains(TermMode::SHOW_CURSOR) && cursor.shape != CursorShape::Hidden;
    let block_cursor = show_cursor && focused && cursor.shape == CursorShape::Block;
    let cursor_color = theme.resolve(Color::Named(NamedColor::Cursor), colors);

    let cell_rect = |row: usize, col: usize, width: usize| {
        Rect::from_min_size(
            origin + Vec2::new(col as f32 * cell.x, row as f32 * cell.y),
            Vec2::new(cell.x * width as f32, cell.y),
        )
    };

    let mut backgrounds: Vec<Shape> = Vec::new();
    let mut bg_run: Option<(usize, usize, usize, Color32)> = None; // row, start, end (exclusive), color
    let mut standalone: Vec<(Pos2, char, Style, usize)> = Vec::new();
    let mut drawn_glyphs: Vec<Shape> = Vec::new();
    let ppp = painter.pixels_per_point();

    // Galleys are painted after backgrounds, so collect text first into a separate layer.
    let text_layer = painter.clone();
    let bg_layer_idx = painter.add(Shape::Noop);

    let mut row_cur = usize::MAX;
    let mut run = Run::new(0);

    for indexed in content.display_iter {
        let row = (indexed.point.line.0 + offset) as usize;
        let col = indexed.point.column.0;
        let flags = indexed.cell.flags;

        if row != row_cur {
            if row_cur != usize::MAX {
                run.flush(&text_layer, origin, cell, row_cur, fonts, 0);
            }
            if let Some((r, s, e, c)) = bg_run.take() {
                backgrounds.push(Shape::rect_filled(cell_rect(r, s, e - s), 0.0, c));
            }
            row_cur = row;
            run = Run::new(0);
        }

        // Colors.
        let mut fg_color = indexed.cell.fg;
        if flags.contains(Flags::BOLD) {
            if let Color::Named(n) = fg_color {
                if (n as usize) < 8 {
                    fg_color = Color::Indexed(n as u8 + 8);
                }
            }
        }
        let mut fg = theme.resolve(fg_color, colors);
        let mut bg = theme.resolve(indexed.cell.bg, colors);
        if flags.contains(Flags::DIM) {
            fg = fg.gamma_multiply(0.66);
        }
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if content.selection.is_some_and(|s| s.contains(indexed.point)) {
            bg = theme.selection;
        }
        let is_cursor = indexed.point == cursor.point;
        if block_cursor && is_cursor {
            bg = cursor_color;
            fg = theme.bg;
        }

        // Background runs.
        let width = if flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
        if flags.contains(Flags::WIDE_CHAR_SPACER) {
            // Covered by the wide char before it.
        } else if bg != default_bg {
            match &mut bg_run {
                Some((_, _, end, c)) if *end == col && *c == bg => *end = col + width,
                _ => {
                    if let Some((r, s, e, c)) = bg_run.take() {
                        backgrounds.push(Shape::rect_filled(cell_rect(r, s, e - s), 0.0, c));
                    }
                    bg_run = Some((row, col, col + width, bg));
                }
            }
        } else if let Some((r, s, e, c)) = bg_run.take() {
            backgrounds.push(Shape::rect_filled(cell_rect(r, s, e - s), 0.0, c));
        }

        // Text.
        if flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let style = Style {
            fg,
            bold: flags.contains(Flags::BOLD),
            italic: flags.contains(Flags::ITALIC),
            underline: flags.intersects(Flags::ALL_UNDERLINES),
            strike: flags.contains(Flags::STRIKEOUT),
        };
        let c = if flags.contains(Flags::HIDDEN) { ' ' } else { indexed.cell.c };
        if let Some(mut shapes) = boxdraw::shapes(c, cell_rect(row, col, width), fg, ppp) {
            drawn_glyphs.append(&mut shapes);
            // Keep the run going so the following characters stay aligned.
            run.push(' ', style, fonts);
        } else if width == 1 && is_grid_safe(c) && indexed.cell.zerowidth().is_none() {
            run.push(c, style, fonts);
        } else {
            run.flush(&text_layer, origin, cell, row, fonts, col + width);
            standalone.push((cell_rect(row, col, width).center(), c, style, width));
        }
    }
    if row_cur != usize::MAX {
        run.flush(&text_layer, origin, cell, row_cur, fonts, 0);
    }
    if let Some((r, s, e, c)) = bg_run.take() {
        backgrounds.push(Shape::rect_filled(cell_rect(r, s, e - s), 0.0, c));
    }

    for (center, c, style, _) in standalone {
        text_layer.text(center, egui::Align2::CENTER_CENTER, c, fonts.font(style.bold, style.italic), style.fg);
    }

    painter.extend(drawn_glyphs);

    // Non-block cursors are drawn on top of the text.
    if show_cursor && !block_cursor {
        let row = cursor.point.line.0 + offset;
        if row >= 0 {
            let r = cell_rect(row as usize, cursor.point.column.0, 1);
            let shape = match (focused, cursor.shape) {
                (_, CursorShape::Beam) => {
                    Shape::rect_filled(Rect::from_min_size(r.min, Vec2::new(2.0, r.height())), 0.0, cursor_color)
                }
                (_, CursorShape::Underline) => Shape::rect_filled(
                    Rect::from_min_max(Pos2::new(r.min.x, r.max.y - 2.0), r.max),
                    0.0,
                    cursor_color,
                ),
                _ => Shape::rect_stroke(r.shrink(0.5), 0.0, Stroke::new(1.0, cursor_color), StrokeKind::Inside),
            };
            painter.add(shape);
        }
    }

    painter.set(bg_layer_idx, Shape::Vec(backgrounds));
}
