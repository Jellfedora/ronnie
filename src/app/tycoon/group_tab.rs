//! The Group tab: the board of directors (once listed), the offices abroad, the rivals.

use super::*;

impl App {
    pub(super) fn tycoon_group(&mut self, ui: &mut Ui, c: &mut Corp, fr: bool, theme: &Theme) {
        // The board.
        heading(ui, tx(("Conseil d'administration", "Board of directors"), fr), theme);
        ui.add_space(6.0);
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            let Some(b) = c.board.clone() else {
                muted(ui, format!("{} {}. {}", tx(("Il siège dès l'étape", "It sits from the stage"), fr), Corp::listed_name(fr), tx(("Chaque trimestre (20 min de jeu, Ronnie ouvert), il te fixe un objectif. Réussi : sa confiance monte, le cours de l'action aussi (la valorisation le suit), et tu touches un bonus sur ton compte perso. Raté : la confiance baisse. À zéro, il te remercie, et ta boîte est revendue sans toi.", "Each quarter (20 min of play, Ronnie open), it sets you a goal. Met: its trust rises, so does the share price (the valuation follows it), and you get a bonus on your personal account. Missed: the trust drops. At zero, it lets you go, and your company is sold without you."), fr)), theme);
                return;
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tx(("Confiance", "Trust"), fr)).size(12.5).color(theme.text_muted));
                bar(ui, b.trust / 100.0, level_color(b.trust / 100.0, theme), 140.0, 8.0, theme).on_hover_text(format!("{:.0}/100", b.trust));
                ui.label(egui::RichText::new(format!("{:.0}", b.trust)).size(13.0).color(level_color(b.trust / 100.0, theme)).strong());
                ui.add_space(12.0);
                ui.label(egui::RichText::new(tx(("Cours de l'action", "Share price"), fr)).size(12.5).color(theme.text_muted));
                let color = if b.share >= 100.0 { theme.ansi[2] } else { theme.ansi[1] };
                ui.label(egui::RichText::new(format!("{:.1} ({}{:.0} %)", b.share, if b.share >= 100.0 { "+" } else { "" }, b.share - 100.0)).size(13.0).color(color).strong());
                muted(ui, format!("· {} ×{:.2}", tx(("valorisation", "valuation"), fr), c.share_mult()), theme);
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("{} {} : {}", tx(("Trimestre", "Quarter"), fr), b.quarters + 1, c.goal_text(fr))).size(14.0).color(theme.text).strong());
            if let Some((now, target)) = c.board_progress() {
                let money_goal = b.goal <= 1;
                ui.horizontal(|ui| {
                    bar(ui, now / target.max(1e-9), theme.accent, 220.0, 8.0, theme);
                    let shown = |x: f64| if money_goal { money(x, fr) } else { format!("{:.0}", x) };
                    muted(ui, format!("{} / {} · {} {}", shown(now), shown(target), tx(("reste", "left"), fr), corp::duration(b.left, fr)), theme);
                });
            }
            muted(ui, format!("{} {}/{} · {}", tx(("Objectifs tenus :", "Goals met:"), fr), b.met, b.quarters, tx(("le conseil ne se réunit que Ronnie ouvert.", "the board only meets with Ronnie open."), fr)), theme);
            if b.trust < 30.0 {
                ui.label(egui::RichText::new(tx(("Le conseil s'impatiente : encore un trimestre raté et tu pourrais être remercié·e. Des dividendes (Direction) le calment.", "The board is losing patience: one more missed quarter and you could be let go. Dividends (Management) calm it down."), fr)).size(12.5).color(theme.ansi[1]));
            }
        });

        // The offices abroad.
        ui.add_space(14.0);
        heading(ui, tx(("Bureaux à l'étranger", "Offices abroad"), fr), theme);
        muted(ui, tx(("Des places en plus pour l'équipe et un bonus chacun, qui grandit avec le bureau (3 niveaux).", "More seats for the team and a bonus each, growing with the office (3 levels)."), fr), theme);
        ui.add_space(6.0);
        let mut open = None;
        for b in corp::BRANCHES.iter() {
            let level = c.branch_level(b.id);
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(b.name, fr)).size(14.0).color(if level > 0 { theme.ansi[2] } else { theme.text }).strong());
                    if level > 0 {
                        ui.label(egui::RichText::new(format!("{} {level}/{}", tx(("niveau", "level"), fr), corp::BRANCH_MAX)).size(12.5).color(theme.accent));
                    }
                    muted(ui, tx(b.about, fr), theme);
                });
                muted(ui, format!("{} {} · {} {} {}", b.seats, tx(("places par niveau", "seats per level"), fr), tx(("loyer", "rent"), fr), per_min(b.rent, fr), tx(("par niveau", "per level"), fr)), theme);
                if c.stage < b.stage {
                    ui.label(egui::RichText::new(format!("{} {}", tx(("Dès l'étape", "From the stage"), fr), tx(corp::STAGES[b.stage as usize].name, fr))).size(12.0).color(theme.ansi[3]));
                } else if level < corp::BRANCH_MAX {
                    let label = if level == 0 { tx(("Ouvrir", "Open"), fr) } else { tx(("Agrandir", "Grow"), fr) };
                    if button(ui, c.can_branch(b.id), format!("{label} ({})", money(c.branch_cost(b.id), fr)), theme) {
                        open = Some(b.id);
                    }
                }
            });
            ui.add_space(4.0);
        }
        if let Some(id) = open {
            c.open_branch(id);
        }

        // The rivals.
        ui.add_space(14.0);
        heading(ui, tx(("Concurrents", "Rivals"), fr), theme);
        muted(ui, tx(("Chacun prend une part du marché de son type de produit : tes produits du même type ont moins d'utilisateurs. Racheté, il t'apporte ses utilisateurs, une partie de son équipe (selon tes places libres)… et une partie de sa dette technique.", "Each takes a share of its kind of product's market: your products of the same kind get fewer users. Bought, it brings you its users, part of its team (as many as you have free seats)… and part of its technical debt."), fr), theme);
        ui.add_space(6.0);
        if c.stage < corp::SELL_STAGE {
            muted(ui, format!("{} {}.", tx(("Les premiers arrivent à l'étape", "The first ones come at the stage"), fr), tx(corp::STAGES[corp::SELL_STAGE as usize].name, fr)), theme);
        } else if c.rivals.is_empty() {
            muted(ui, tx(("Pas de concurrent pour l'instant. Ça ne va pas durer.", "No rivals for now. It won't last."), fr), theme);
        }
        let mut buy = None;
        for r in &c.rivals {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(&r.name).size(14.0).color(theme.text).strong());
                    ui.label(egui::RichText::new(format!("{} {}", tx(("sur le marché de", "in the market of"), fr), tx(r.kind().name, fr))).size(12.5).color(theme.accent));
                    let mine = c.products.iter().any(|p| p.kind == r.kind);
                    if mine {
                        ui.label(egui::RichText::new(format!("−{:.0} % {}", c.rival_pressure(r.kind) * 100.0, tx(("d'utilisateurs pour ton produit", "users for your product"), fr))).size(12.0).color(theme.ansi[1]));
                    }
                });
                muted(ui, format!("{:.0} {} · {} {} ({} {}) · {} {:.0} %", r.users, tx(("utilisateurs", "users"), fr), r.staff, tx(("personnes", "people"), fr), tx(("niv.", "lvl"), fr), r.level, tx(("dette", "debt"), fr), r.debt), theme);
                if button(ui, c.can_acquire(r.id), format!("{} ({})", tx(("Racheter", "Buy it"), fr), money(r.price(), fr)), theme) {
                    buy = Some(r.id);
                }
            });
            ui.add_space(4.0);
        }
        if let Some(id) = buy {
            c.acquire(id);
        }
        let _ = self;
    }
}
