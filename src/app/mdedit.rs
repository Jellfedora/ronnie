//! Markdown written and shown outside the notes (the project tracking's descriptions), with the
//! notes' own rendering (`notes::live`): headings, **bold**, *italics*, ~~struck~~, `code`, links,
//! lists, boxes to tick, quotes, rules, code blocks and tables.
//!
//! `md_view` shows a text rendered (its boxes ticked by a click, its links opened); `md_editor` edits
//! one with a toolbar (and ⌘ B, ⌘ I): rendered while not being edited, its Markdown shown (marks
//! dimmed) while it is, Tab going from cell to cell in a table, a list going on at Return.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::ops::Range;

use super::notes::{self, Format, Prefix};
use super::*;
use egui::Id;

/// The notes' layout of a text, kept between frames while neither it nor what it's laid out for change.
#[derive(Clone)]
struct Laid {
    key: u64,
    live: std::sync::Arc<notes::Live>,
}

fn key_of(text: &str, raw: bool, width: f32, theme: &Theme) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    (text, raw, width.to_bits(), notes::color_key(theme)).hash(&mut h);
    h.finish()
}

/// The notes' layout of `text`, kept in memory under `id`.
fn laid_out(ctx: &egui::Context, id: Id, text: &str, raw: bool, width: f32, theme: &Theme) -> std::sync::Arc<notes::Live> {
    let key = key_of(text, raw, width, theme);
    let store = id.with("md-laid");
    if let Some(l) = ctx.data(|d| d.get_temp::<Laid>(store)).filter(|l| l.key == key) {
        return l.live;
    }
    let live = std::sync::Arc::new(notes::live_with(text, None, raw, theme, width, &|_| None, false));
    ctx.data_mut(|d| d.insert_temp(store, Laid { key, live: live.clone() }));
    live
}

/// What a click in a rendered text asks.
#[derive(Default)]
pub(super) struct Clicked {
    /// A box ticked or unticked: the text with it changed.
    pub(super) toggled: Option<String>,
}

/// `text` rendered, `width` wide. A box ticked gives the text changed (the caller saves it, or not);
/// a link opens.
pub(super) fn md_view(ui: &mut Ui, id: Id, text: &str, width: f32, theme: &Theme, t: &Strings) -> Clicked {
    let live = laid_out(ui.ctx(), id, text, false, width, theme);
    let back = ui.painter().add(egui::Shape::Noop);
    let mut job = live.job.clone();
    job.wrap.max_width = width;
    let galley = ui.painter().layout_job(job);
    // Drawn as it is (no text field: nothing to scroll to, nothing to focus).
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, galley.size().y), Sense::click());
    ui.painter().galley(rect.min, galley.clone(), theme.text);
    let hits = notes::paint_live(ui, &live, &galley, rect.min, width, back, &response, &HashMap::new(), theme, t, true, None);
    let mut clicked = Clicked::default();
    if let Some(byte) = hits.toggle {
        let mut changed = text.to_owned();
        let mark = if changed.as_bytes().get(byte) == Some(&b' ') { "x" } else { " " };
        if byte < changed.len() {
            changed.replace_range(byte..byte + 1, mark);
            clicked.toggled = Some(changed);
        }
    }
    if let Some(url) = hits.open.filter(|u| u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")) {
        crate::terminal::open_url(&url);
    }
    clicked
}

/// The formats of the toolbar (the notes' own buttons, but images and files).
fn buttons(t: &Strings) -> Vec<Option<(Format, String, notes::Paint)>> {
    notes::format_buttons(t).into_iter().filter(|b| !matches!(b, Some((Format::Image | Format::Attach, _, _)))).collect()
}

/// `format` done on `text`, its selection `sel` (characters): the new selection.
fn apply(text: &mut String, sel: Range<usize>, format: Format, t: &Strings) -> Range<usize> {
    let len = text.chars().count();
    let sel = sel.start.min(len)..sel.end.min(len);
    match format {
        Format::Heading => {
            let c = notes::toggle_prefix(text, sel.start, Prefix::Heading);
            c..c
        }
        Format::Bullet => notes::prefix_lines(text, sel, Prefix::Bullet),
        Format::Numbered => notes::prefix_lines(text, sel, Prefix::Numbered),
        Format::Check => notes::prefix_lines(text, sel, Prefix::Check),
        Format::Quote => notes::prefix_lines(text, sel, Prefix::Quote),
        Format::Bold => notes::wrap(text, sel, "**"),
        Format::Italic => notes::wrap(text, sel, "*"),
        Format::Strike => notes::wrap(text, sel, "~~"),
        Format::Code => notes::wrap(text, sel, "`"),
        Format::Link => notes::link(text, sel, t.notes_link_text),
        Format::Rule => {
            let (_, after) = notes::insert_block(text, sel.end, "---");
            after..after
        }
        Format::Table => notes::table(text, sel.end, t.notes_column),
        Format::Image | Format::Attach => sel,
    }
}

/// A Markdown editor `width` wide, at least `rows` lines high, its toolbar on top: true when `text`
/// changed. Rendered when not focused (a click edits it), its Markdown while being edited.
#[allow(clippy::too_many_arguments)]
pub(super) fn md_editor(ui: &mut Ui, id: Id, text: &mut String, width: f32, rows: usize, hint: &str, theme: &Theme, t: &Strings) -> bool {
    let ctx = ui.ctx().clone();
    let editor = id.with("md-editor");
    let focused = ctx.memory(|m| m.has_focus(editor));
    let mut changed = false;
    let selection = |ctx: &egui::Context| {
        egui::TextEdit::load_state(ctx, editor).and_then(|s| s.cursor.char_range()).map(|r| {
            let (a, b) = (r.primary.index.0, r.secondary.index.0);
            a.min(b)..a.max(b)
        })
    };

    // The toolbar; ⌘ B and ⌘ I while typing.
    let mut asked = None;
    if focused {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
            asked = Some(Format::Bold);
        } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I)) {
            asked = Some(Format::Italic);
        }
    }
    Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, if focused { theme.accent.gamma_multiply(0.7) } else { theme.tab_hover })).corner_radius(8.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(width - 12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for b in buttons(t) {
                match b {
                    None => {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(9.0, 24.0), Sense::hover());
                        ui.painter().vline(r.center().x, r.min.y + 5.0..=r.max.y - 5.0, Stroke::new(1.0, theme.tab_hover));
                    }
                    Some((format, tip, paint)) => {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(26.0, 24.0), Sense::click());
                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 5.0, theme.tab_hover);
                        }
                        paint(ui.painter(), r.center(), if resp.hovered() { theme.text } else { theme.text_muted });
                        if resp.clicked() {
                            asked = Some(format);
                        }
                    }
                }
            }
        });
        ui.add_space(4.0);
        ui.painter().hline(ui.min_rect().x_range(), ui.cursor().min.y - 2.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7)));
        ui.add_space(2.0);

        if let Some(format) = asked {
            let len = text.chars().count();
            let sel = selection(&ctx).unwrap_or(len..len);
            let new = apply(text, sel, format, t);
            notes::select(&ctx, editor, new);
            ctx.memory_mut(|m| m.request_focus(editor));
            changed = true;
        }
        // Tab in a table: to the next cell (Shift: the one before).
        if focused
            && let Some(c) = selection(&ctx)
        {
            let back = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
            let next = back || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
            if next && let Some(cell) = notes::next_cell(text, c.start, back) {
                notes::select(&ctx, editor, cell);
                changed = true;
            }
        }

        // Rendered when not edited; its Markdown, dimmed marks, while being edited.
        let raw = focused || ctx.memory(|m| m.has_focus(editor));
        let inner = width - 24.0;
        let live = laid_out(&ctx, id, text, raw, inner, theme);
        let back = ui.painter().add(egui::Shape::Noop);
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.is_none());
        let theme_c = theme.clone();
        let mut layouter = move |ui: &Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
            let text = buf.as_str();
            let live = laid_out(ui.ctx(), id, text, raw, inner, &theme_c);
            let mut job = live.job.clone();
            job.wrap.max_width = wrap;
            ui.painter().layout_job(job)
        };
        let out = egui::TextEdit::multiline(text)
            .id(editor)
            .frame(egui::Frame::NONE)
            .margin(Vec2::new(4.0, 4.0))
            .desired_width(inner)
            .desired_rows(rows)
            .lock_focus(true)
            .hint_text(hint)
            .layouter(&mut layouter)
            .show(ui);
        if out.response.changed() {
            changed = true;
            if enter
                && let Some(c) = out.cursor_range.map(|r| r.primary.index.0)
                && let Some(c) = notes::continue_list(text, c)
            {
                notes::set_cursor(&ctx, editor, c);
            }
        }
        // The layout the editor used this frame (the text may have changed since: laid out again).
        let live = if out.response.changed() { laid_out(&ctx, id, text, raw, inner, theme) } else { live };
        let hits = notes::paint_live(ui, &live, &out.galley, out.galley_pos, inner, back, &out.response, &HashMap::new(), theme, t, !raw, None);
        if let Some(byte) = hits.toggle
            && byte < text.len()
        {
            let mark = if text.as_bytes().get(byte) == Some(&b' ') { "x" } else { " " };
            text.replace_range(byte..byte + 1, mark);
            changed = true;
        }
        if let Some(c) = hits.cursor {
            notes::set_cursor(&ctx, editor, c);
            ctx.memory_mut(|m| m.request_focus(editor));
        }
    });
    changed
}

/// The first line of a Markdown text, its marks gone (a title for a list).
pub(super) fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.trim_start_matches('#').trim_start_matches(['-', '*', '+', '>']).trim();
    let line = line.strip_prefix("[ ]").or_else(|| line.strip_prefix("[x]")).unwrap_or(line).trim();
    line.replace("**", "").replace('`', "").replace("~~", "")
}
