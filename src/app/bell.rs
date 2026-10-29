//! The notices not seen yet: each notice shown (see `Toast`) is kept until the user sees what it is
//! about (its tab, the mail, the notes…), follows it or dismisses it. A bell at the bottom of the
//! strip of categories counts them and lists them; with none, it isn't there.

use super::*;
use super::drive::Lead;

/// The notices kept at most: the oldest go beyond.
const KEPT: usize = 50;
const LIST_W: f32 = 340.0;

/// A notice not seen yet.
pub(super) struct Notice {
    id: u64,
    ok: bool,
    title: String,
    body: String,
    at: chrono::DateTime<chrono::Local>,
    /// What it is about: a mail, where a notice from Claude leads, or (else) a pane's tab.
    mail: Option<String>,
    lead: Option<Lead>,
    pane: Option<PaneId>,
}

impl Notice {
    /// Seen by its own: a failure about nothing in particular (a disk almost full) stays until
    /// dismissed.
    fn stays(&self) -> bool {
        !self.ok && self.mail.is_none() && self.lead.is_none()
    }
}

impl App {
    /// Shows a notice, and keeps it until seen.
    pub(super) fn push_toast(&mut self, mut toast: Toast) {
        self.keep_notice(&mut toast);
        self.toasts.push(toast);
    }

    /// Keeps a notice until seen without showing it (told by the system while Ronnie isn't in front).
    pub(super) fn keep_notice(&mut self, toast: &mut Toast) {
        self.next_notice += 1;
        toast.notice = self.next_notice;
        let pane = (toast.mail.is_none() && toast.lead.is_none()).then(|| self.tabs.get(toast.tab).map(|t| t.focused)).flatten();
        self.notices.push(Notice { id: toast.notice, ok: toast.ok, title: toast.title.clone(), body: toast.body.clone(), at: chrono::Local::now(), mail: toast.mail.clone(), lead: toast.lead.clone(), pane });
        if self.notices.len() > KEPT {
            self.notices.remove(0);
        }
    }

    pub(super) fn forget_notice(&mut self, id: u64) {
        self.notices.retain(|n| n.id != id);
    }

    /// The notices about what this window shows (while it is in front) are seen.
    pub(super) fn sweep_notices(&mut self, ctx: &egui::Context) {
        if self.notices.is_empty() || !ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
            return;
        }
        let page = self.home.is_some() || self.notes_page || self.music_page || self.dev_page || self.modules_page || self.procs_page;
        let tab = if page { None } else { self.tabs.get(self.active) };
        let pane_shown = |id: PaneId| tab.is_some_and(|t| t.layout.leaves().contains(&id));
        let mails = self.dev_page && self.dev.tool == dev::DevTool::Mail;
        let (notes, dev) = (self.notes_page, self.dev_page);
        self.notices.retain(|n| {
            let seen = match (&n.mail, &n.lead, n.pane) {
                (Some(_), _, _) => mails,
                (None, Some(Lead::Note(_) | Lead::Notes), _) => notes,
                (None, Some(Lead::Dev), _) => dev,
                (None, Some(Lead::Pane(id) | Lead::Doc(id, _)), _) => pane_shown(*id),
                (None, Some(Lead::Profile(id)), _) => tab.is_some_and(|t| t.profile == Some(*id)),
                (None, None, Some(id)) => !n.stays() && pane_shown(id),
                (None, None, None) => false,
            };
            !seen
        });
    }

    /// The bell, centered on `cx` and ending at `bottom` (not drawn if it doesn't fit below `above`),
    /// with the number of notices not seen; a click lists them beside.
    pub(super) fn bell_ui(&mut self, ui: &Ui, cx: f32, above: f32, bottom: f32) {
        let rect = Rect::from_center_size(Pos2::new(cx, bottom - 16.0), Vec2::splat(30.0));
        let count = self.notices.len();
        // Nothing to see: no bell (and its list closes, the last one dismissed).
        if count == 0 {
            if self.bell_open == Some(self.viewport) {
                self.bell_open = None;
            }
            return;
        }
        if rect.min.y < above {
            return;
        }
        let t = self.t();
        let resp = ui.interact(rect, ui.id().with("bell"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.notices_tip);
        let open = self.bell_open == Some(self.viewport);
        if resp.hovered() || open {
            ui.painter().rect_filled(rect, 8.0, self.theme.tab_active);
        }
        paint_bell(ui.painter(), rect.center(), if count > 0 { self.theme.text } else { self.theme.text_muted });
        if count > 0 {
            let text = if count > 9 { "9+".to_owned() } else { count.to_string() };
            let at = rect.center() + Vec2::new(7.0, -7.0);
            ui.painter().circle_filled(at, 7.0, self.theme.ansi[1]);
            ui.painter().text(at, Align2::CENTER_CENTER, text, FontId::proportional(8.5), Color32::WHITE);
        }
        if resp.clicked() {
            self.bell_open = if open { None } else { Some(self.viewport) };
        }
        if self.bell_open == Some(self.viewport) {
            self.notices_list(ui.ctx(), rect);
        }
    }

    /// The notices, newest first, beside the bell: one leads where it is about, ✕ forgets it.
    fn notices_list(&mut self, ctx: &egui::Context, bell: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let mut go = None;
        let mut forget = None;
        let mut clear = false;
        let area = egui::Area::new(egui::Id::new("notices")).order(egui::Order::Foreground).pivot(Align2::LEFT_BOTTOM).fixed_pos(Pos2::new(bell.max.x + 14.0, bell.max.y + 4.0)).show(ctx, |ui| {
            Frame::NONE
                .fill(theme.chrome_bg)
                .stroke(Stroke::new(1.0, theme.tab_hover))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow { offset: [4, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(LIST_W - 24.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(t.notices_title).size(14.0).strong().color(theme.text));
                        if !self.notices.is_empty() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Button::new(egui::RichText::new(t.notices_clear).size(12.0).color(theme.text_muted)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    clear = true;
                                }
                            });
                        }
                    });
                    ui.add_space(6.0);
                    if self.notices.is_empty() {
                        ui.label(egui::RichText::new(t.notices_none).size(12.5).color(theme.text_muted));
                        return;
                    }
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for n in self.notices.iter().rev() {
                            let row = Frame::NONE.inner_margin(egui::Margin::symmetric(8, 7)).corner_radius(8.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let mark = if n.ok { theme.accent } else { theme.ansi[1] };
                                    let (dot, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
                                    ui.painter().circle_filled(dot.center(), 3.5, mark);
                                    ui.label(egui::RichText::new(&n.title).size(13.0).strong().color(theme.text));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add(egui::Button::new(egui::RichText::new("✕").size(11.0).color(theme.text_muted)).frame(false)).on_hover_text(t.close).clicked() {
                                            forget = Some(n.id);
                                        }
                                        ui.label(egui::RichText::new(n.at.format("%H:%M").to_string()).size(11.0).color(theme.text_muted));
                                    });
                                });
                                if !n.body.is_empty() {
                                    ui.add(egui::Label::new(egui::RichText::new(&n.body).size(12.0).color(theme.text_muted)).wrap());
                                }
                            });
                            let resp = ui.interact(row.response.rect, ui.id().with(("notice", n.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.hovered() {
                                ui.painter().rect_filled(row.response.rect, 8.0, theme.tab_hover.gamma_multiply(0.5));
                            }
                            if resp.clicked() && forget.is_none() {
                                go = Some(n.id);
                            }
                        }
                    });
                });
        });
        // A click elsewhere closes the list.
        let outside = ctx.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !bell.contains(p)));
        if outside {
            self.bell_open = None;
        }
        if clear {
            self.notices.clear();
            self.bell_open = None;
        }
        if let Some(id) = forget {
            self.forget_notice(id);
        }
        if let Some(id) = go
            && let Some(k) = self.notices.iter().position(|n| n.id == id)
        {
            let n = self.notices.remove(k);
            self.bell_open = None;
            self.toasts.retain(|t| t.notice != id);
            match (n.mail, n.lead, n.pane) {
                (Some(mail), _, _) => self.open_mail(&mail),
                (None, Some(lead), _) => self.follow(lead),
                (None, None, Some(pane)) => self.show_pane(pane),
                (None, None, None) => {}
            }
        }
    }
}

/// A bell: its dome, its rim, its clapper.
fn paint_bell(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let points: Vec<Pos2> = [(-6.0, 4.0), (-4.6, 2.6), (-4.4, -1.2), (-3.4, -3.9), (-1.6, -5.3), (0.0, -5.6), (1.6, -5.3), (3.4, -3.9), (4.4, -1.2), (4.6, 2.6), (6.0, 4.0)].iter().map(|&(x, y)| c + Vec2::new(x, y)).collect();
    painter.add(egui::Shape::closed_line(points, stroke));
    painter.circle_filled(c + Vec2::new(0.0, 6.2), 1.6, color);
    painter.circle_filled(c + Vec2::new(0.0, -6.6), 1.0, color);
}
