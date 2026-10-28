//! Preview of the images of a folder, local or on the server: read in memory (nothing written to disk),
//! shown fitted to the pane, zoomed with a pinch (or ⌘ / Ctrl + wheel) and moved by dragging; ← → go through the
//! folder's other images.

use std::sync::mpsc::Receiver;

use super::*;

/// Extensions the preview decodes (others open with a program, as before).
const VIEWABLE: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "jfif"];
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 32.0;

/// Whether the preview shows this file.
pub(super) fn viewable(name: &str) -> bool {
    let ext = name.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_lowercase());
    VIEWABLE.contains(&ext.as_str())
}

/// A decoded image: what to make the texture of, and its real size (the texture can be smaller).
struct Decoded {
    image: egui::ColorImage,
    size: [u32; 2],
}

enum State {
    /// Being read (on the server: done / total bytes when known).
    Reading(Option<(u64, u64)>),
    Decoding(Receiver<Result<Decoded, String>>),
    Shown { texture: egui::TextureHandle, size: [u32; 2] },
    Failed(String),
}

pub(super) enum ImageAction {
    None,
    Close,
}

pub(super) struct ImageView {
    pub remote: bool,
    dir: String,
    /// The folder's images, in the panel's order.
    names: Vec<String>,
    index: usize,
    state: State,
    /// Size of the file shown, in bytes.
    bytes: u64,
    /// A server read going on (one at a time): its path.
    in_flight: Option<String>,
    /// The image to read next, once nothing is in flight.
    want: bool,
    /// Points per image pixel; None: fitted to the pane.
    zoom: Option<f32>,
    /// From the pane's center to the image's.
    offset: Vec2,
    close_requested: bool,
}

impl ImageView {
    pub fn new(remote: bool, dir: String, names: Vec<String>, index: usize) -> Self {
        Self { remote, dir, names, index, state: State::Reading(None), bytes: 0, in_flight: None, want: true, zoom: None, offset: Vec2::ZERO, close_requested: false }
    }

    pub fn name(&self) -> &str {
        &self.names[self.index]
    }

    pub fn path(&self) -> String {
        if self.remote { crate::sftp::join(&self.dir, self.name()) } else { Path::new(&self.dir).join(self.name()).display().to_string() }
    }

    pub fn request_close(&mut self) {
        self.close_requested = true;
    }

    /// The file to read next, if it is time: the server's is asked for by the file manager, a local one
    /// is read here (with the decoding, on another thread).
    pub fn take_request(&mut self, ctx: &egui::Context) -> Option<String> {
        if !self.want || self.in_flight.is_some() {
            return None;
        }
        self.want = false;
        let path = self.path();
        if self.remote {
            self.in_flight = Some(path.clone());
            return Some(path);
        }
        self.bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
        self.state = State::Decoding(decode_in_thread(ctx, move || std::fs::read(&path).map_err(|e| e.to_string())));
        None
    }

    /// Bytes of a server file arrived (or reading it failed).
    pub fn arrived(&mut self, ctx: &egui::Context, path: &str, result: Result<Vec<u8>, String>) {
        if self.in_flight.as_deref() != Some(path) {
            return;
        }
        self.in_flight = None;
        // Moved on to another image meanwhile: that one is read now instead.
        if self.want || path != self.path() {
            return;
        }
        match result {
            Ok(data) => {
                self.bytes = data.len() as u64;
                self.state = State::Decoding(decode_in_thread(ctx, move || Ok(data)));
            }
            Err(e) => self.state = State::Failed(e),
        }
    }

    pub fn progress(&mut self, path: &str, done: u64, total: u64) {
        if self.in_flight.as_deref() == Some(path) && !self.want {
            if let State::Reading(p) = &mut self.state {
                *p = Some((done, total));
            }
        }
    }

    fn go(&mut self, step: isize) {
        if self.names.len() < 2 {
            return;
        }
        self.index = (self.index as isize + step).rem_euclid(self.names.len() as isize) as usize;
        self.state = State::Reading(None);
        self.want = true;
        self.zoom = None;
        self.offset = Vec2::ZERO;
    }

    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) -> ImageAction {
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut ui;
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        if let State::Decoding(rx) = &self.state {
            match rx.try_recv() {
                Ok(Ok(decoded)) => {
                    let texture = ui.ctx().load_texture(self.name().to_owned(), decoded.image, egui::TextureOptions::LINEAR);
                    self.state = State::Shown { texture, size: decoded.size };
                }
                Ok(Err(e)) => self.state = State::Failed(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.state = State::Failed(String::new()),
            }
        }
        let typing = ui.ctx().egui_wants_keyboard_input() || ui.ctx().memory(|m| m.top_modal_layer().is_some());
        let (prev, next, close) = if typing {
            (false, false, false)
        } else {
            ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowLeft) || i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowRight) || i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::Escape) || i.consume_key(Modifiers::NONE, Key::Space),
                )
            })
        };
        if prev {
            self.go(-1);
        }
        if next {
            self.go(1);
        }
        if close {
            self.close_requested = true;
        }

        // Header.
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 36.0));
        ui.painter().rect_filled(head, 0.0, theme.chrome_bg);
        ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, theme.tab_hover));
        let shown = matches!(self.state, State::Shown { .. });
        ui.scope_builder(egui::UiBuilder::new().max_rect(head.shrink2(Vec2::new(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.label(egui::RichText::new("👁").size(14.0).color(theme.accent));
            ui.label(egui::RichText::new(self.name()).size(14.0).strong());
            if self.names.len() > 1 {
                ui.label(egui::RichText::new(format!("{} / {}", self.index + 1, self.names.len())).size(12.0).color(theme.text_muted));
            }
            ui.add(egui::Label::new(egui::RichText::new(&self.dir).size(12.0).color(theme.text_muted)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new(format!("✕  {}", t.close)).size(13.0)).clicked() {
                    self.close_requested = true;
                }
                if self.names.len() > 1 {
                    ui.add_space(6.0);
                    if ui.button(egui::RichText::new("›").size(15.0)).on_hover_text(t.image_next).clicked() {
                        self.go(1);
                    }
                    if ui.button(egui::RichText::new("‹").size(15.0)).on_hover_text(t.image_prev).clicked() {
                        self.go(-1);
                    }
                }
                ui.add_space(6.0);
                if ui.add_enabled(shown, egui::Button::new(egui::RichText::new("1:1").size(12.5)).selected(self.zoom == Some(1.0))).on_hover_text(t.image_actual).clicked() {
                    self.zoom = Some(1.0);
                    self.offset = Vec2::ZERO;
                }
                if ui.add_enabled(shown, egui::Button::new(egui::RichText::new(t.image_fit).size(12.5)).selected(self.zoom.is_none())).clicked() {
                    self.zoom = None;
                    self.offset = Vec2::ZERO;
                }
            });
        });

        let status_h = 24.0;
        let body = Rect::from_min_max(Pos2::new(rect.min.x, head.max.y + 1.0), Pos2::new(rect.max.x, rect.max.y - status_h));
        let ppp = ui.ctx().pixels_per_point();
        let mut status = super::files::format_size(self.bytes, t);
        match &self.state {
            State::Reading(progress) => {
                let text = match progress {
                    Some((done, total)) if *total > 0 => format!("{}  {} %", t.files_loading, done * 100 / total),
                    _ => t.files_loading.to_owned(),
                };
                ui.painter().text(body.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0), theme.text_muted);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
            State::Decoding(_) => {
                ui.painter().text(body.center(), Align2::CENTER_CENTER, t.files_loading, FontId::proportional(13.0), theme.text_muted);
            }
            State::Failed(e) => {
                let text = if e.is_empty() { t.image_unreadable.to_owned() } else { format!("{}\n{e}", t.image_unreadable) };
                ui.painter().text(body.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0), theme.ansi[1]);
            }
            State::Shown { texture, size } => {
                // In points at 1:1 (one image pixel per screen pixel).
                let natural = Vec2::new(size[0] as f32, size[1] as f32) / ppp;
                let room = body.shrink(12.0).size();
                let fit = (room.x / natural.x).min(room.y / natural.y).min(1.0);
                let zoom = self.zoom.unwrap_or(fit);
                let resp = ui.interact(body, ui.id().with("image"), egui::Sense::click_and_drag());
                let (pinch, scroll) = if resp.hovered() { ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta)) } else { (1.0, Vec2::ZERO) };
                let mut zoom_to = None;
                if pinch != 1.0 {
                    zoom_to = Some((zoom * pinch, resp.hover_pos().unwrap_or(body.center())));
                }
                if resp.double_clicked() {
                    let target = if self.zoom.is_none() || (zoom - fit).abs() < 0.001 { 1.0 } else { fit };
                    zoom_to = Some((target, resp.interact_pointer_pos().unwrap_or(body.center())));
                }
                let mut zoom = zoom;
                if let Some((to, at)) = zoom_to {
                    let to = to.clamp(MIN_ZOOM.min(fit), MAX_ZOOM);
                    // The point under the pointer stays there.
                    let from_center = at - body.center();
                    self.offset = from_center - (from_center - self.offset) * (to / zoom);
                    zoom = to;
                    self.zoom = if (to - fit).abs() < 0.001 { None } else { Some(to) };
                }
                if resp.dragged() {
                    self.offset += resp.drag_delta();
                }
                if pinch == 1.0 {
                    self.offset += scroll;
                }
                let shown = natural * zoom;
                // Not moved away from the pane: centered when it fits, its edges kept at the pane's otherwise.
                let slack = ((shown - body.size()) / 2.0).max(Vec2::ZERO);
                self.offset = self.offset.clamp(-slack, slack);
                let image_rect = Rect::from_center_size(body.center() + self.offset, shown);
                let painter = ui.painter().with_clip_rect(body);
                painter.image(texture.id(), image_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if shown.x > body.width() || shown.y > body.height() {
                    resp.on_hover_cursor(if ui.input(|i| i.pointer.primary_down()) { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                }
                status = format!("{} × {}    {} %    {status}", size[0], size[1], (zoom * ppp * 100.0).round());
            }
        }

        let bar = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - status_h), rect.max);
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.painter().hline(bar.x_range(), bar.min.y, Stroke::new(1.0, theme.tab_hover));
        ui.painter().text(bar.right_center() - Vec2::new(12.0, 0.0), Align2::RIGHT_CENTER, status, FontId::proportional(11.5), theme.text_muted);
        ui.painter().text(bar.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, t.image_hint, FontId::proportional(11.5), theme.text_muted);
        if self.close_requested {
            self.close_requested = false;
            return ImageAction::Close;
        }
        ImageAction::None
    }
}

/// Reads (`read`) and decodes an image on another thread: turned as its EXIF says, and made small enough
/// for a texture.
fn decode_in_thread(ctx: &egui::Context, read: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static) -> Receiver<Result<Decoded, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (ctx, max_side) = (ctx.clone(), ctx.input(|i| i.max_texture_side) as u32);
    std::thread::spawn(move || {
        let _ = tx.send(read().and_then(|data| decode(&data, max_side)));
        ctx.request_repaint();
    });
    rx
}

fn decode(data: &[u8], max_side: u32) -> Result<Decoded, String> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().map_err(|e| e.to_string())?.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    let size = [image.width(), image.height()];
    if size[0].max(size[1]) > max_side {
        image = image.thumbnail(max_side, max_side);
    }
    let rgba = image.to_rgba8();
    Ok(Decoded { image: egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()), size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewable_by_extension() {
        assert!(viewable("photo.JPG"));
        assert!(viewable("a.b.webp"));
        assert!(!viewable("film.mp4"));
        assert!(!viewable("png"));
    }

    #[test]
    fn decodes_and_shrinks() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(300, 100, image::Rgba([255, 0, 0, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let d = decode(png.get_ref(), 150).unwrap();
        assert_eq!(d.size, [300, 100]);
        assert_eq!(d.image.size, [150, 50]);
        assert!(decode(b"not an image", 150).is_err());
    }
}
