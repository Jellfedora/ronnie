//! The card of a typing game round, to share: shown over the results, then copied to the clipboard as
//! an image (the window's screenshot, cut to the card) or as a line of text.

use super::*;

/// The card's size, in points (the image has the screen's pixels: twice as many on a Retina screen).
const CARD: Vec2 = Vec2::new(560.0, 300.0);

pub(super) struct ScoreCard {
    /// The player's name, typed under the card (may stay empty).
    pub(super) name: String,
    letters: u32,
    /// The round's length, combos' seconds included: "1 min 08 s".
    length: String,
    words: u32,
    wpm: u32,
    accuracy: u32,
    combo: u32,
    record: bool,
    date: String,
    /// Where the card was drawn last, to cut it out of the screenshot.
    rect: Rect,
    /// A screenshot asked, not arrived yet.
    capturing: bool,
    /// When it was copied (image: true), for "Copied!" a moment.
    copied: Option<(f64, bool)>,
    failed: bool,
}

impl ScoreCard {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(name: &str, letters: u32, length: String, words: u32, wpm: u32, accuracy: u32, combo: u32, record: bool) -> Self {
        let date = chrono::Local::now().format("%d/%m/%Y").to_string();
        Self { name: name.to_owned(), letters, length, words, wpm, accuracy, combo, record, date, rect: Rect::NOTHING, capturing: false, copied: None, failed: false }
    }

    /// The card and its buttons, over everything; true once closed. `clipboard` stays alive after (on
    /// Linux, what was copied goes with it).
    pub(super) fn ui(&mut self, ctx: &egui::Context, theme: &Theme, t: &Strings, now: f64, clipboard: &mut Option<arboard::Clipboard>) -> bool {
        // The screenshot asked: the card cut out of it, onto the clipboard.
        if self.capturing {
            let shot = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
            if let Some(image) = shot {
                self.capturing = false;
                let card = image.region(&self.rect, Some(ctx.pixels_per_point()));
                let bytes: Vec<u8> = card.pixels.iter().flat_map(|c| c.to_array()).collect();
                let data = arboard::ImageData { width: card.width(), height: card.height(), bytes: bytes.into() };
                if clipboard.is_none() {
                    *clipboard = arboard::Clipboard::new().ok();
                }
                self.failed = clipboard.as_mut().is_none_or(|c| c.set_image(data).is_err());
                if !self.failed {
                    self.copied = Some((now, true));
                }
            } else {
                ctx.request_repaint();
            }
        }

        let (mut close, mut image, mut text) = (false, false, false);
        let modal = egui::Modal::new(egui::Id::new("score-card")).frame(Frame::NONE).backdrop_color(Color32::from_black_alpha(200)).show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(CARD, Sense::hover());
            self.rect = rect;
            paint_card(ui.painter(), rect, theme, t, self);
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.share_name).size(13.5).color(theme.text_muted));
                ui.add(egui::TextEdit::singleline(&mut self.name).hint_text(t.share_name_hint).char_limit(24).desired_width(220.0));
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.set_width(CARD.x);
                let copied = self.copied.filter(|(at, _)| now - at < 2.5);
                let image_label = if copied.is_some_and(|(_, image)| image) { format!("✓  {}", t.share_copied) } else { t.share_copy_image.to_owned() };
                let primary = egui::Button::new(egui::RichText::new(image_label).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(150.0, 32.0));
                image = ui.add_enabled(!self.capturing, primary).clicked();
                let text_label = if copied.is_some_and(|(_, image)| !image) { format!("✓  {}", t.share_copied) } else { t.share_copy_text.to_owned() };
                text = ui.add(egui::Button::new(egui::RichText::new(text_label).size(13.5)).corner_radius(6.0).min_size(Vec2::new(130.0, 32.0))).clicked();
                if self.failed {
                    ui.label(egui::RichText::new(t.share_failed).size(12.0).color(theme.ansi[1]));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.add(egui::Button::new(egui::RichText::new(t.close).size(13.5)).corner_radius(6.0).min_size(Vec2::new(90.0, 32.0))).clicked();
                });
            });
            if copied_lately(self.copied, now) {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
        });
        if image {
            self.capturing = true;
            self.failed = false;
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
        }
        if text {
            ctx.copy_text(self.text(t));
            self.copied = Some((now, false));
        }
        close || (modal.should_close() && !self.capturing)
    }

    fn text(&self, t: &Strings) -> String {
        let text = t.game_share_text
            .replace("{letters}", &self.letters.to_string())
            .replace("{time}", &self.length)
            .replace("{wpm}", &self.wpm.to_string())
            .replace("{accuracy}", &self.accuracy.to_string())
            .replace("{combo}", &self.combo.to_string());
        if self.record { format!("{text}  ·  {}", t.game_share_record) } else { text }
    }
}

fn copied_lately(copied: Option<(f64, bool)>, now: f64) -> bool {
    copied.is_some_and(|(at, _)| now - at < 2.5)
}

/// The card: the score in big gold metal letters, the round's figures on the right, a stage of red
/// slashes and sparks behind.
fn paint_card(painter: &egui::Painter, rect: Rect, theme: &Theme, t: &Strings, card: &ScoreCard) {
    let painter = painter.with_clip_rect(rect);
    let gold = theme.ansi[3];
    // The background: the theme's, lit with its accent at the top left, darker at the bottom.
    let mut mesh = egui::Mesh::default();
    let top = theme.bg.lerp_to_gamma(theme.accent, 0.22);
    let bottom = theme.chrome_bg.lerp_to_gamma(Color32::BLACK, 0.35);
    for (pos, color) in [(rect.left_top(), top), (rect.right_top(), theme.bg), (rect.right_bottom(), bottom), (rect.left_bottom(), theme.chrome_bg)] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
    // Slashes across the right side.
    for (k, (x, w, alpha)) in [(330.0, 46.0, 0.10), (392.0, 18.0, 0.16), (424.0, 70.0, 0.07), (510.0, 10.0, 0.2)].into_iter().enumerate() {
        let x = rect.min.x + x;
        let lean = 120.0 + k as f32 * 6.0;
        let band = vec![Pos2::new(x, rect.max.y), Pos2::new(x + lean, rect.min.y), Pos2::new(x + lean + w, rect.min.y), Pos2::new(x + w, rect.max.y)];
        painter.add(egui::Shape::convex_polygon(band, theme.accent.gamma_multiply(alpha), Stroke::NONE));
    }
    // Sparks, always at the same places.
    let mut seed = 0x9e37_79b9_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.0
    };
    for _ in 0..34 {
        let (x, y, r, a) = (next(), next(), next(), next());
        let color = if a > 0.5 { gold } else { theme.accent };
        painter.circle_filled(Pos2::new(rect.min.x + x * rect.width(), rect.min.y + y * rect.height()), 0.6 + r * 1.8, color.gamma_multiply(0.15 + a * 0.45));
    }
    painter.rect_stroke(rect, 0.0, Stroke::new(2.0, theme.accent.gamma_multiply(0.7)), egui::StrokeKind::Inside);

    // The top: Ronnie, the game's name, the date.
    let metal = |size: f32| FontId::new(size, egui::FontFamily::Name("metal".into()));
    let left = rect.min.x + 28.0;
    painter.text(Pos2::new(left, rect.min.y + 22.0), Align2::LEFT_TOP, "Ronnie", metal(30.0), theme.accent);
    // The player's name, when given, above the game's name and the date.
    let name = card.name.trim();
    if name.is_empty() {
        painter.text(Pos2::new(rect.max.x - 28.0, rect.min.y + 26.0), Align2::RIGHT_TOP, "SPEED METAL", FontId::proportional(13.0), theme.text);
        painter.text(Pos2::new(rect.max.x - 28.0, rect.min.y + 44.0), Align2::RIGHT_TOP, &card.date, FontId::proportional(11.5), theme.text_muted);
    } else {
        painter.text(Pos2::new(rect.max.x - 28.0, rect.min.y + 18.0), Align2::RIGHT_TOP, name, metal(26.0), theme.text);
        painter.text(Pos2::new(rect.max.x - 28.0, rect.min.y + 54.0), Align2::RIGHT_TOP, format!("SPEED METAL  ·  {}", card.date), FontId::proportional(11.5), theme.text_muted);
    }

    // The score, with a shadow and a red edge.
    let score = card.letters.to_string();
    // Anchored at its bottom: the metal font's glyphs stop well above it, hence the label raised after.
    let at = Pos2::new(left - 4.0, rect.min.y + 198.0);
    painter.text(at + Vec2::new(4.0, 5.0), Align2::LEFT_BOTTOM, &score, metal(118.0), Color32::from_black_alpha(160));
    painter.text(at + Vec2::new(2.0, 2.0), Align2::LEFT_BOTTOM, &score, metal(118.0), theme.accent);
    painter.text(at, Align2::LEFT_BOTTOM, &score, metal(118.0), gold);
    painter.text(Pos2::new(left, at.y - 12.0), Align2::LEFT_TOP, t.share_letters.replace("{time}", &card.length), FontId::proportional(15.0), theme.text);

    // A record: a stamp, a little askew.
    if card.record {
        let center = Pos2::new(left + 112.0, rect.max.y - 50.0);
        let angle = -0.08_f32;
        let galley = painter.layout_no_wrap(t.game_record.to_owned(), metal(20.0), gold);
        let size = galley.size() + Vec2::new(22.0, 10.0);
        let rot = egui::emath::Rot2::from_angle(angle);
        let corners: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(x, y)| center + rot * Vec2::new(x * size.x / 2.0, y * size.y / 2.0)).collect();
        painter.add(egui::Shape::convex_polygon(corners.clone(), theme.accent.gamma_multiply(0.18), Stroke::NONE));
        painter.add(egui::Shape::closed_line(corners, Stroke::new(2.0, gold)));
        let text_at = center + rot * (-galley.size() / 2.0);
        painter.add(egui::epaint::TextShape::new(text_at, galley, gold).with_angle(angle));
    }

    // The figures, in a column on the right.
    let x = rect.max.x - 28.0;
    let figures = [(card.wpm.to_string(), t.game_wpm), (format!("{} %", card.accuracy), t.game_accuracy), (format!("×{}", card.combo), t.game_combo), (card.words.to_string(), t.game_words)];
    for (k, (value, label)) in figures.iter().enumerate() {
        let y = rect.min.y + 86.0 + k as f32 * 46.0;
        painter.text(Pos2::new(x, y), Align2::RIGHT_TOP, value, FontId::monospace(22.0), theme.text);
        painter.text(Pos2::new(x, y + 25.0), Align2::RIGHT_TOP, *label, FontId::proportional(11.5), theme.text_muted);
    }
    painter.text(Pos2::new(x, rect.max.y - 18.0), Align2::RIGHT_BOTTOM, "github.com/Jellfedora/ronnie", FontId::proportional(11.0), theme.text_muted.gamma_multiply(0.8));
}
