//! The home page: shown at launch and after the tab shown closed. What can be opened (profiles, SSH
//! hosts, databases), the tabs open to go back to, and quick ways to start something new. The typing
//! game hides behind it: three clicks on the logo.

use super::*;
use super::sidebar::paint_badge;

/// Width a card aims at (the columns fill the width), its height.
const CARD_W: f32 = 230.0;
const CARD_H: f32 = 54.0;
const GAP: f32 = 10.0;

/// A card of the home page.
struct Card {
    name: String,
    hint: String,
    color: Option<Color32>,
    ssh: bool,
    open: bool,
    action: TabAction,
}

impl App {
    pub(super) fn home_dashboard(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let mut action = None;
        // The tips scroll by at the bottom, as with the game.
        let tips_h = if self.config.settings.home_tips { 44.0 } else { 0.0 };
        let ticker = Rect::from_min_size(Pos2::new(rect.min.x, rect.max.y - tips_h), Vec2::new(rect.width(), tips_h));
        if self.config.settings.home_tips {
            super::sidebar::tips_ticker(ui, ticker, &theme, t, &self.config.settings.shortcuts);
        }
        let page = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, ticker.min.y));

        let open_tabs: Vec<Card> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let hint = if let Some(h) = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)) {
                    h.address()
                } else if let Some(c) = tab.db.and_then(|id| self.config.databases.iter().find(|c| c.id == id)) {
                    c.address()
                } else {
                    format!("{} {}", tab.layout.leaves().len(), t.layout_panes)
                };
                Card { name: tab.title().to_owned(), hint, color: tab.color, ssh: tab.ssh.is_some(), open: true, action: TabAction::Select(i) }
            })
            .collect();
        let card_of = |id: Uuid| {
            self.item(id).map(|item| Card { name: item.name, hint: item.hint.lines().last().unwrap_or_default().to_owned(), color: item.color, ssh: item.ssh, open: item.open.is_some(), action: TabAction::OpenItem(id) })
        };
        let profiles: Vec<Card> = self.config.profiles.iter().filter_map(|p| card_of(p.id)).collect();
        let hosts: Vec<Card> = self.config.ssh.iter().filter(|_| self.config.settings.home_hosts).filter_map(|h| card_of(h.id)).collect();
        let databases: Vec<Card> = self
            .config
            .databases
            .iter()
            .filter(|_| self.config.settings.home_databases)
            .map(|c| Card { name: c.name.clone(), hint: c.address(), color: c.color, ssh: false, open: self.tabs.iter().any(|tab| tab.db == Some(c.id)), action: TabAction::OpenDb(c.id) })
            .collect();

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page).layout(egui::Layout::top_down(egui::Align::Min)));
        egui::ScrollArea::vertical().id_salt("home-page").auto_shrink([false, false]).show(&mut child, |ui| {
            // Centered, not wider than reads well.
            let width = (ui.available_width() - 64.0).clamp(240.0, 1100.0);
            let side = ((ui.available_width() - width) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.add_space(40.0);
                    let hour = chrono::Timelike::hour(&chrono::Local::now());
                    let hello = if (5..18).contains(&hour) { t.home_hello_day } else { t.home_hello_evening };
                    ui.label(egui::RichText::new(hello).size(28.0).strong().color(theme.text));
                    // What just closed, else what the page is for.
                    let line = self.home.as_deref().filter(|l| !l.is_empty()).unwrap_or(t.home_subtitle);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(line).size(14.0).color(theme.text_muted));
                    ui.add_space(22.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(GAP);
                        let new_tab = format!("{}   {}", t.home_new_terminal, self.config.settings.shortcuts.new_tab.label());
                        for (label, what, accent) in [(new_tab.as_str(), TabAction::New, true), (t.home_new_host, TabAction::NewHost, false), (t.home_new_db, TabAction::NewDb, false)] {
                            let text = egui::RichText::new(format!("+  {label}")).size(13.5).color(if accent { theme.bg } else { theme.text });
                            let button = egui::Button::new(text).fill(if accent { theme.accent } else { theme.tab_active }).corner_radius(8.0).min_size(Vec2::new(0.0, 36.0));
                            if ui.add(button).clicked() {
                                action = Some(what);
                            }
                        }
                    });

                    for (title, cards) in [(t.home_open, &open_tabs), (t.home_profiles, &profiles), (t.home_hosts, &hosts), (t.home_databases, &databases)] {
                        if cards.is_empty() {
                            continue;
                        }
                        ui.add_space(28.0);
                        ui.label(egui::RichText::new(title.to_uppercase()).size(11.5).strong().color(theme.text_muted));
                        ui.add_space(10.0);
                        if let Some(a) = cards_grid(ui, width, cards, &theme) {
                            action = Some(a);
                        }
                    }
                    ui.add_space(32.0);
                });
            });
        });
        if action.is_some() {
            self.apply_tab_action(ui, action, &[]);
        }
    }
}

/// Cards in as many columns as fit; the action of the one clicked.
fn cards_grid(ui: &mut Ui, width: f32, cards: &[Card], theme: &crate::theme::Theme) -> Option<TabAction> {
    let columns = ((width + GAP) / (CARD_W + GAP)).floor().max(1.0) as usize;
    let card_w = (width - GAP * (columns - 1) as f32) / columns as f32;
    let rows = cards.len().div_ceil(columns);
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, rows as f32 * (CARD_H + GAP) - GAP), Sense::hover());
    let painter = ui.painter_at(area.expand(2.0));
    let mut clicked = None;
    for (k, card) in cards.iter().enumerate() {
        let at = area.min + Vec2::new((k % columns) as f32 * (card_w + GAP), (k / columns) as f32 * (CARD_H + GAP));
        let r = Rect::from_min_size(at, Vec2::new(card_w, CARD_H));
        let resp = ui.interact(r, ui.id().with(("home-card", area.min.y as i32, k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let fill = if resp.hovered() { theme.tab_active } else { theme.chrome_bg };
        painter.rect(r, 8.0, fill, Stroke::new(1.0, if resp.hovered() { theme.accent.gamma_multiply(0.5) } else { theme.tab_hover }), egui::StrokeKind::Inside);
        let dot = Pos2::new(r.min.x + 26.0, r.center().y);
        paint_badge(&painter, dot, &card.name, card.color, card.ssh, card.open, false, theme);
        if card.open {
            painter.circle_filled(Pos2::new(r.max.x - 14.0, r.min.y + 14.0), 3.5, theme.ansi[2]);
        }
        let text_x = r.min.x + 48.0;
        let max_w = r.max.x - text_x - 22.0;
        let name = painter.layout(card.name.clone(), FontId::proportional(14.0), theme.text, f32::INFINITY);
        let name = if name.size().x > max_w { painter.layout(elide(&card.name, max_w, 14.0), FontId::proportional(14.0), theme.text, f32::INFINITY) } else { name };
        painter.galley(Pos2::new(text_x, r.center().y - 17.0), name, theme.text);
        painter.text(Pos2::new(text_x, r.center().y + 10.0), Align2::LEFT_CENTER, elide(&card.hint, max_w, 12.0), FontId::proportional(12.0), theme.text_muted);
        if resp.clicked() {
            clicked = Some(card.action.clone());
        }
    }
    clicked
}

/// `text` cut with "…" to about `width` points at `size` (proportional font, roughly measured).
fn elide(text: &str, width: f32, size: f32) -> String {
    let fits = (width / (size * 0.55)).max(4.0) as usize;
    if text.chars().count() <= fits {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(fits.saturating_sub(1)).collect::<String>())
    }
}
