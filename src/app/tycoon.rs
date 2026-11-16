//! Ronnie Corp, in the games (its rules in `crate::tycoon`): the company's figures at the top, then its
//! tabs (the office, the team, the contracts, the products, the research, the premises, the
//! management), an event waiting in a card over them, and what happened while away.
//!
//! The company runs whatever page is shown (`tycoon_tick`, each frame): it lives here, and is put in
//! the settings (sealed with the rewards, synced with floor) at each of the player's actions and every
//! minute, not at each second it runs (the sync would otherwise go on all the time).

use super::four::HomeGame;
use super::tycoon_scene;
use super::*;
use crate::tycoon::{self as corp, money, tx, Corp, Summary};

/// The company put in the settings at least this often (seconds).
const WRITE_EVERY: f64 = 60.0;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Office,
    Team,
    Contracts,
    Products,
    Research,
    Premises,
    Board,
}

const TABS: [(Tab, corp::Tx); 7] = [
    (Tab::Office, ("Bureau", "Office")),
    (Tab::Team, ("Équipe", "Team")),
    (Tab::Contracts, ("Contrats", "Contracts")),
    (Tab::Products, ("Produits", "Products")),
    (Tab::Research, ("Recherche", "Research")),
    (Tab::Premises, ("Locaux", "Premises")),
    (Tab::Board, ("Direction", "Management")),
];

/// What a step of the tutorial points at.
#[derive(Clone, Copy, PartialEq)]
enum Spot {
    None,
    Tab(Tab),
    /// The board of the contracts, on the office's wall.
    Board,
}

struct Step {
    title: corp::Tx,
    text: corp::Tx,
    spot: Spot,
    /// Done by playing (None: read, then « Next »).
    done: Option<fn(&Corp) -> bool>,
}

const STEPS: [Step; 10] = [
    Step {
        title: ("Bienvenue chez Ronnie Corp !", "Welcome to Ronnie Corp!"),
        text: ("Tu démarres seul·e dans le garage de tes parents, avec 1 500 €. Le but : faire grandir ta boîte, du garage jusqu'à la bourse. Tu vois ton bureau en vrai : c'est toi, avec la couronne.", "You start alone in your parents' garage, with €1,500. The goal: grow your company from the garage all the way to the stock exchange. That's your office, for real: you're the one with the crown."),
        spot: Spot::None,
        done: None,
    },
    Step {
        title: ("Ton premier contrat", "Your first contract"),
        text: ("L'argent vient d'abord des contrats. Ouvre l'onglet Contrats et accepte l'offre qui t'attend.", "Money first comes from contracts. Open the Contracts tab and accept the offer waiting for you."),
        spot: Spot::Tab(Tab::Contracts),
        done: Some(|c| !c.contracts.is_empty() || c.stats.contracts > 0),
    },
    Step {
        title: ("Au travail !", "Get to work!"),
        text: ("Personne n'a besoin de cliquer pour coder : chacun travaille tout seul. Le post-it sur le tableau se remplit. Attends la livraison (une trentaine de secondes).", "Nobody needs to click to code: everyone works by themselves. The post-it on the board fills up. Wait for the delivery (about thirty seconds)."),
        spot: Spot::Board,
        done: Some(|c| c.stats.contracts > 0),
    },
    Step {
        title: ("Livré, payé !", "Delivered, paid!"),
        text: ("Chaque livraison rapporte de l'argent et de la réputation. Plus de réputation : des contrats plus gros et mieux payés (les paliers). En retard : moitié prix, et la réputation baisse.", "Each delivery brings money and reputation. More reputation: bigger, better-paid contracts (the tiers). Late: half price, and the reputation drops."),
        spot: Spot::None,
        done: None,
    },
    Step {
        title: ("Ta première recrue", "Your first hire"),
        text: ("Seul·e, on va vite mais pas loin. Onglet Équipe : choisis un·e candidat·e et embauche-le·la. Regarde ses compétences : un contrat de site demande du Front et du Design, une API du Back…", "Alone you go fast, but not far. Team tab: pick a candidate and hire them. Look at their skills: a website needs Front and Design, an API needs Back…"),
        spot: Spot::Tab(Tab::Team),
        done: Some(|c| c.stats.hires > 0),
    },
    Step {
        title: ("Les tâches", "Tasks"),
        text: ("Chacun a une tâche : Contrats (rapporte), Produits (tes applis SaaS, plus tard), Recherche (débloque des technos) ou Refacto (fait baisser la dette technique, qui ralentit tout). Clique sur quelqu'un dans le bureau pour la changer.", "Everyone has a task: Contracts (brings money), Products (your SaaS apps, later), Research (unlocks techs) or Refactoring (lowers the technical debt, which slows everything). Click someone in the office to change it."),
        spot: Spot::Tab(Tab::Team),
        done: None,
    },
    Step {
        title: ("Ta première techno", "Your first tech"),
        text: ("Mets quelqu'un (toi, par exemple) sur Recherche quelques minutes, puis débloque « Git et intégration continue » dans l'onglet Recherche. « Passage au cloud » te permettra ensuite de lancer tes propres produits.", "Put someone (you, for instance) on Research for a few minutes, then unlock \"Git and CI\" in the Research tab. \"Moving to the cloud\" will then let you launch your own products."),
        spot: Spot::Tab(Tab::Research),
        done: Some(|c| !c.techs.is_empty()),
    },
    Step {
        title: ("Un peu de confort", "A little comfort"),
        text: ("Un bon moral, c'est jusqu'à +50 % de productivité ; trop bas, les gens démissionnent. Onglet Locaux : installe des plantes vertes (200 €). Plus tard, déménage pour avoir plus de places.", "Good morale means up to +50 % productivity; too low and people quit. Premises tab: put in some green plants (€200). Later, move to get more seats."),
        spot: Spot::Tab(Tab::Premises),
        done: Some(|c| !c.perks.is_empty()),
    },
    Step {
        title: ("La direction", "Management"),
        text: ("Télétravail, horaires (le crunch va vite mais épuise), salaires, marketing, formation : tout se règle dans Direction. Plus tard, tu pourras y revendre la boîte contre des étoiles, des bonus pour toujours.", "Remote work, hours (crunch is fast but exhausting), salaries, marketing, training: all set in Management. Later you can sell the company there for stars, bonuses for good."),
        spot: Spot::Tab(Tab::Board),
        done: None,
    },
    Step {
        title: ("C'est à toi !", "Over to you!"),
        text: ("Des imprévus arriveront (un « ! » rouge au-dessus de ta tête) : à toi de choisir. Et la boîte tourne même Ronnie fermé (4 h au plus, au ralenti) : un·e commercial·e acceptera les contrats pendant ce temps, et d'autres rôles (RH, office manager, CTO…) automatisent le reste : voir Direction › Automatisations. Le bouton « ? » en haut à droite relance ce tutoriel.", "Surprises will come up (a red \"!\" over your head): your call. And the company keeps running with Ronnie closed (4 h at most, slowed down): a salesperson will accept contracts meanwhile, and other roles (HR, office manager, CTO…) automate the rest: see Management › Automations. The \"?\" button at the top right replays this tutorial."),
        spot: Spot::None,
        done: None,
    },
];

pub(super) struct Tycoon {
    tab: Tab,
    /// The company running.
    pub(super) live: Option<Corp>,
    /// What was last put in the settings (another value there: synced, or edited elsewhere).
    written: Option<Corp>,
    wrote_at: f64,
    /// Caught up once since Ronnie opened (the first time, what was missed is the time closed).
    started: bool,
    /// What happened while away, to show.
    away: Option<Summary>,
    /// A firing or a sale to confirm.
    confirm: Option<u32>,
    sell_confirm: bool,
    pub(super) scene: super::tycoon_scene::Scene,
    tab_rects: [Rect; 7],
    /// The tutorial seen again (from its button): each step goes on with « Next ».
    replay: bool,
    pub(super) floor: crate::floor::GameFloor,
    /// What was sent to floor: the best valuation (k€), the furthest stage, the stars.
    sent: (u32, u8, u32),
}

impl Default for Tycoon {
    fn default() -> Self {
        Self { tab: Tab::Office, live: None, written: None, wrote_at: 0.0, started: false, away: None, confirm: None, sell_confirm: false, scene: Default::default(), tab_rects: [Rect::NOTHING; 7], replay: false, floor: crate::floor::GameFloor::new("ronnie-corp", false), sent: (0, 0, 0) }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Its icon: a small office tower with lit windows.
pub(super) fn paint_tycoon_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = scale;
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let tower = Rect::from_min_max(at(-5.5, -9.0), at(5.5, 9.0));
    painter.rect_filled(tower, 1.2 * s, theme.accent);
    let low = Rect::from_min_max(at(-10.0, -1.0), at(-5.5, 9.0));
    painter.rect_filled(low, 1.0 * s, theme.accent.gamma_multiply(0.6));
    for row in 0..4 {
        for col in 0..2 {
            let x = -3.2 + col as f32 * 3.8;
            let y = -6.8 + row as f32 * 3.6;
            let lit = (row + col) % 3 != 0;
            painter.rect_filled(Rect::from_min_size(at(x, y), Vec2::new(2.6, 2.0) * s), 0.3 * s, if lit { theme.bg } else { theme.accent.gamma_multiply(0.5) });
        }
    }
    painter.line_segment([at(0.0, -9.0), at(0.0, -12.0)], Stroke::new(0.9 * s, theme.text_muted));
}

/// A bar `frac` full.
fn bar(ui: &mut Ui, frac: f64, color: Color32, width: f32, height: f32, theme: &Theme) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, height / 2.0, theme.tab_hover);
    let w = rect.width() * frac.clamp(0.0, 1.0) as f32;
    if w > 0.5 {
        painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(w.max(height), height)), height / 2.0, color);
    }
    resp
}

/// Red to green.
fn level_color(frac: f64, theme: &Theme) -> Color32 {
    if frac < 0.3 {
        theme.ansi[1]
    } else if frac < 0.6 {
        theme.ansi[3]
    } else {
        theme.ansi[2]
    }
}

fn card<R>(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(10.0).inner_margin(12.0).show(ui, add).inner
}

/// The team's blocks, in their order.
const TEAM_GROUPS: [corp::Tx; 3] = [("Tech", "Tech"), ("Pilotage", "Steering"), ("Support", "Support")];

/// Which block of the team a job goes in: the ones who make (founder, devs, design, data, DevOps, QA, CTO),
/// the ones who steer (project manager, product owner), the support roles (sales, HR, office manager, assistant).
fn team_group(special: corp::Special) -> u8 {
    use corp::Special::*;
    match special {
        Lead | Po => 1,
        Sales | Hr | Office | Assistant => 2,
        _ => 0,
    }
}

/// The roles that do something by themselves (and can be paused).
fn automates(special: corp::Special) -> bool {
    use corp::Special::*;
    matches!(special, Sales | Hr | Lead | Cto | Office | Po | Assistant)
}

fn heading(ui: &mut Ui, text: &str, theme: &Theme) {
    ui.label(egui::RichText::new(text).size(16.0).color(theme.text).strong());
}

fn muted(ui: &mut Ui, text: impl Into<String>, theme: &Theme) {
    ui.label(egui::RichText::new(text.into()).size(12.5).color(theme.text_muted));
}

/// The technical debt explained (under the pointer, over its figure): what it is, what it costs now,
/// what makes it rise, how to bring it down.
fn debt_help(ui: &mut Ui, c: &Corp, fr: bool, theme: &Theme) {
    ui.set_max_width(340.0);
    let line = |ui: &mut Ui, text: String, color: Color32| {
        ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(color)).wrap());
    };
    ui.label(egui::RichText::new(format!("{} {:.0} %", tx(("Dette technique :", "Technical debt:"), fr), c.debt)).size(14.0).color(theme.text).strong());
    line(ui, tx(("Le code fait à la va-vite qui s'accumule. Elle monte toute seule tant que l'équipe travaille sur des contrats ou des produits, et ralentit tout.", "Code written in a hurry piling up. It rises by itself while the team works on contracts or products, and slows everything down."), fr).into(), theme.text_muted);
    ui.add_space(6.0);
    line(ui, tx(("En ce moment :", "Right now:"), fr).into(), theme.text);
    let costs = [
        format!("{} −{:.0} %", tx(("• productivité", "• productivity"), fr), c.debt / 2.5),
        format!("{} −{:.0}", tx(("• moral de l'équipe", "• the team's morale"), fr), c.debt / 10.0),
        format!("{} −{:.0} % {}", tx(("• qualité des livraisons", "• quality of deliveries"), fr), c.debt / 1.5, tx(("(contrats moins bien payés)", "(contracts paid less)"), fr)),
        tx(("• plus de bugs dans tes produits", "• more bugs in your products"), fr).to_owned(),
    ];
    let color = if c.debt < 10.0 { theme.text_muted } else { theme.ansi[1] };
    for cost in costs {
        line(ui, cost, color);
    }
    ui.add_space(6.0);
    line(ui, tx(("Elle monte plus vite avec le crunch (×2), les profils « Rapide », les Microservices, ou en restant sur un vieux framework.", "It rises faster with crunch (×2), \"Fast\" people, Microservices, or staying on an old framework."), fr).into(), theme.text_muted);
    let refactoring = c.staff.iter().filter(|e| e.task == corp::Task::Refactor).count();
    line(
        ui,
        format!(
            "{} {refactoring} {}",
            tx(("Pour la faire baisser : mets des gens en Refacto (clique sur quelqu'un dans le bureau) :", "To bring it down: put people on Refactoring (click someone in the office):"), fr),
            tx(("en ce moment. Un·e DevOps ou un·e CTO la fait baisser en continu ; la techno Git et CI et les horaires Cool la font monter moins vite. Un·e chef·fe de projet en automatique envoie quelqu'un en Refacto au-delà de 40 %.", "right now. A DevOps or a CTO keeps lowering it; the Git and CI tech and relaxed hours make it rise slower. A project manager on automatic sends someone to Refactoring past 40 %."), fr)
        ),
        theme.text,
    );
}

fn button(ui: &mut Ui, enabled: bool, text: impl Into<String>, theme: &Theme) -> bool {
    let b = egui::Button::new(egui::RichText::new(text.into()).size(13.0).color(if enabled { theme.bg } else { theme.text_muted })).fill(if enabled { theme.accent } else { theme.tab_hover }).corner_radius(6.0);
    ui.add_enabled(enabled, b).clicked()
}

fn small(ui: &mut Ui, text: impl Into<String>) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(text.into()).size(12.0)).corner_radius(6.0)).clicked()
}

/// One choice among `labels`; the one picked.
fn segmented(ui: &mut Ui, current: u8, labels: &[String], theme: &Theme) -> Option<u8> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for (k, label) in labels.iter().enumerate() {
            let on = current as usize == k;
            let b = egui::Button::new(egui::RichText::new(label).size(12.5).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(12.0);
            if ui.add(b).clicked() && !on {
                picked = Some(k as u8);
            }
        }
    });
    picked
}

fn per_min(x: f64, fr: bool) -> String {
    format!("{}/min", money(x * 60.0, fr))
}

fn skills_text(v: &[f64; 5], fr: bool) -> String {
    let mut parts: Vec<(usize, f64)> = v.iter().copied().enumerate().filter(|(_, x)| *x > 0.05).collect();
    parts.sort_by(|a, b| b.1.total_cmp(&a.1));
    if parts.is_empty() {
        return "—".into();
    }
    parts.iter().map(|(s, x)| format!("{} {:.1}", tx(corp::SKILLS[*s], fr), x)).collect::<Vec<_>>().join(" · ")
}

fn ago(at: i64, now: i64, fr: bool) -> String {
    let s = (now - at).max(0);
    if s < 60 {
        tx(("à l'instant", "just now"), fr).into()
    } else if fr {
        format!("il y a {}", corp::duration(s as f64, fr))
    } else {
        format!("{} ago", corp::duration(s as f64, fr))
    }
}

impl App {
    pub(super) fn open_tycoon(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Tycoon;
    }

    /// Beside its name in the games' list: the company's valuation, an event waiting.
    pub(super) fn tycoon_note(&self) -> Option<String> {
        let c = self.tycoon.live.as_ref()?;
        let fr = self.fr();
        Some(if c.event.is_some() { format!("❗ {}", money(c.valuation(), fr)) } else { money(c.valuation(), fr) })
    }

    fn fr(&self) -> bool {
        self.config.settings.language == Lang::Fr
    }

    /// Each frame, whatever is shown: the company run to now, and put in the settings when due.
    pub(super) fn tycoon_tick(&mut self, ctx: &egui::Context) {
        let saved = &self.config.settings.tycoon;
        if saved.is_none() && self.tycoon.live.is_none() {
            return;
        }
        let now = unix_now();
        let time = ctx.input(|i| i.time);
        // Brought by a sync (or the config edited): taken, and run from where it was.
        if self.tycoon.written.as_ref() != saved.as_ref() {
            self.tycoon.live = saved.clone();
            self.tycoon.written = saved.clone();
            self.tycoon.wrote_at = time;
        }
        let first = !self.tycoon.started;
        self.tycoon.started = true;
        let Some(c) = self.tycoon.live.as_mut() else { return };
        if now > c.at {
            let sum = if first { c.catch_up(now) } else { c.run(now) };
            if sum.secs >= 600.0 {
                self.tycoon.away = Some(sum);
            }
        }
        let acted = self.tycoon.written.as_ref().map(|w| w.actions) != Some(c.actions);
        if acted || time - self.tycoon.wrote_at >= WRITE_EVERY {
            self.tycoon_write(ctx, time);
        }
        // Kept running now and then, even with nothing else to draw.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }

    /// The company put in the settings, saved; a better valuation, a further stage or more stars sent to
    /// floor (the same valuation sent again rewrites the details there: the golden ticker frame goes by
    /// them).
    fn tycoon_write(&mut self, ctx: &egui::Context, time: f64) {
        let Some(c) = self.tycoon.live.clone() else { return };
        let best = (c.legacy.best / 1_000.0).min(u32::MAX as f64) as u32;
        let (stage, stars) = (c.best_stage(), c.legacy.stars);
        // A reward just won: said.
        if let Some(w) = &self.tycoon.written {
            let t = self.t();
            let (was_stage, was_sales) = (w.best_stage(), w.legacy.sales);
            let crossed = |k: u8| was_stage < k && stage >= k;
            let mut won: Vec<String> = Vec::new();
            for (k, what) in [(corp::BILLS_STAGE, t.unlock_bills), (corp::WALL_STREET_STAGE, t.unlock_wallstreet), (corp::UNICORN_STAGE, t.unlock_unicorn)] {
                if crossed(k) {
                    won.push(what.to_owned());
                }
            }
            let ticker = t.unlock_frame.replace("{n}", super::frames::frame_name(t, "ticker"));
            if was_sales == 0 && c.legacy.sales > 0 {
                if was_stage < corp::LISTED_STAGE {
                    won.push(ticker);
                }
                won.push(t.unlock_cheque.to_owned());
            } else if crossed(corp::LISTED_STAGE) && was_sales == 0 {
                won.push(ticker);
            }
            if !won.is_empty() {
                self.push_toast(super::Toast { ok: true, title: t.corp_reward_title.to_owned(), body: format!("{}\n{}", won.join(" · "), t.corp_reward_where), tab: self.active, at: std::time::Instant::now(), mail: None, notice: 0, lead: None });
            }
        }
        self.config.settings.tycoon = Some(c.clone());
        self.tycoon.written = Some(c);
        self.tycoon.wrote_at = time;
        self.save_config();
        let (was, was_stage, was_stars) = self.tycoon.sent;
        if (best > was || stage > was_stage || stars > was_stars) && best > 0 && crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            self.tycoon.sent = (best.max(was), stage.max(was_stage), stars.max(was_stars));
            self.tycoon.floor.finished(crate::floor::GameRound { score: best, details: serde_json::json!({ "valuation": best, "stage": stage, "stars": stars }) });
            self.tycoon.floor.sync(ctx, time, None, self.config.settings.floor.as_ref());
        }
    }

    pub(super) fn tycoon_page(&mut self, ui: &mut Ui, rect: Rect) {
        self.fold_for_game(false);
        let t = self.t();
        let fr = self.fr();
        let theme = self.theme.clone();
        let time = ui.input(|i| i.time);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        ui.painter_at(rect).rect_filled(rect, 0.0, theme.bg);
        let account = self.config.settings.floor.clone();
        self.tycoon.floor.poll();
        self.tycoon.floor.sync(ui.ctx(), time, Some(120.0), account.as_ref());

        let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
        if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
            self.home_game = false;
            return;
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 34.0), Align2::CENTER_CENTER, t.tycoon_name, 30.0, theme.accent, 1.0);

        let Some(mut c) = self.tycoon.live.clone() else {
            self.tycoon_welcome(ui, rect);
            return;
        };
        let now = unix_now();
        let body = Rect::from_min_max(Pos2::new(rect.min.x + 20.0, rect.min.y + 60.0), Pos2::new(rect.max.x - 20.0, rect.max.y - 12.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut child;
        self.tycoon_figures(ui, &c, fr, &theme);
        ui.add_space(8.0);
        // The tabs.
        ui.horizontal_wrapped(|ui| {
            for (k, (tab, label)) in TABS.into_iter().enumerate() {
                let on = self.tycoon.tab == tab;
                let mut text = tx(label, fr).to_owned();
                if tab == Tab::Contracts && !c.offers.is_empty() {
                    text = format!("{text} ({})", c.offers.len());
                }
                let b = egui::Button::new(egui::RichText::new(text).size(13.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.chrome_bg }).corner_radius(14.0).min_size(Vec2::new(90.0, 28.0));
                let resp = ui.add(b);
                self.tycoon.tab_rects[k] = resp.rect;
                if resp.clicked() {
                    self.tycoon.tab = tab;
                    self.tycoon.confirm = None;
                    self.tycoon.sell_confirm = false;
                }
            }
        });
        ui.add_space(8.0);
        if self.tycoon.tab == Tab::Office {
            self.tycoon_floor(ui, &mut c, now, fr, &theme);
        } else {
            egui::ScrollArea::vertical().id_salt(("tycoon", self.tycoon.tab as u8)).auto_shrink([false; 2]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match self.tycoon.tab {
                Tab::Office => {}
                Tab::Team => self.tycoon_team(ui, &mut c, fr, &theme),
                Tab::Contracts => self.tycoon_contracts(ui, &mut c, fr, &theme),
                Tab::Products => self.tycoon_products(ui, &mut c, fr, &theme),
                Tab::Research => self.tycoon_research(ui, &mut c, fr, &theme),
                Tab::Premises => self.tycoon_premises(ui, &mut c, fr, &theme),
                Tab::Board => self.tycoon_board(ui, &mut c, now, fr, &theme),
            }
            ui.add_space(20.0);
            });
        }
        self.tycoon_tutorial(ui.ctx(), rect, &mut c, fr, &theme);
        self.tycoon_event(ui.ctx(), &mut c, fr, &theme);
        self.tycoon_away(ui.ctx(), fr, &theme);
        if self.tycoon.live.as_ref() != Some(&c) {
            self.tycoon.live = Some(c);
        }
    }

    /// No company yet: what it is, and a button to start it.
    fn tycoon_welcome(&mut self, ui: &mut Ui, rect: Rect) {
        let fr = self.fr();
        let t = self.t();
        let theme = self.theme.clone();
        let area = Rect::from_center_size(rect.center(), Vec2::new((rect.width() - 48.0).min(520.0), 300.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Center)));
        let ui = &mut child;
        paint_tycoon_icon(ui.painter(), Pos2::new(area.center().x, area.min.y + 20.0), &theme, 3.0);
        ui.add_space(60.0);
        ui.label(egui::RichText::new(t.tycoon_arcade).size(14.0).color(theme.text));
        ui.add_space(12.0);
        muted(ui, tx(("La boîte tourne même quand Ronnie est fermé (jusqu'à 4 h, au ralenti). Embauche, décroche des contrats, lance tes produits, déménage… et revends-la pour repartir plus fort.", "The company keeps running while Ronnie is closed (up to 4 h, slowed down). Hire, land contracts, launch products, move offices… and sell it to start again stronger."), fr), &theme);
        ui.add_space(20.0);
        if button(ui, true, tx(("Ouvrir le garage", "Open the garage"), fr), &theme) {
            let now = unix_now();
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
            self.tycoon.live = Some(Corp::new(now, seed, Default::default()));
            self.tycoon.started = true;
            let time = ui.input(|i| i.time);
            self.tycoon_write(&ui.ctx().clone(), time);
        }
    }

    /// The figures, always at the top.
    fn tycoon_figures(&self, ui: &mut Ui, c: &Corp, fr: bool, theme: &Theme) {
        let net = c.income - c.spend;
        let stage = &corp::STAGES[c.stage as usize];
        let items: Vec<(String, String, Color32)> = vec![
            (tx(("Trésorerie", "Cash"), fr).into(), money(c.cash, fr), if c.cash < 0.0 { theme.ansi[1] } else { theme.text }),
            (tx(("Solde", "Net"), fr).into(), format!("{}{}", if net >= 0.0 { "+" } else { "" }, per_min(net, fr)), if net < 0.0 { theme.ansi[1] } else { theme.ansi[2] }),
            (tx(("Valorisation", "Valuation"), fr).into(), money(c.valuation(), fr), theme.accent),
            (tx(("Étape", "Stage"), fr).into(), tx(stage.name, fr).into(), theme.text),
            (tx(("Réputation", "Reputation"), fr).into(), format!("{:.0}", c.rep), theme.text),
            (tx(("Équipe", "Team"), fr).into(), format!("{}/{}", c.staff.len(), c.capacity()), theme.text),
            (tx(("Dette tech.", "Tech debt"), fr).into(), format!("{:.0} %", c.debt), level_color(1.0 - c.debt / 100.0, theme)),
        ];
        let last = items.len() - 1;
        ui.horizontal_wrapped(|ui| {
            for (k, (label, value, color)) in items.into_iter().enumerate() {
                let shown = egui::Frame::new().fill(theme.chrome_bg).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(label).size(11.0).color(theme.text_muted));
                        ui.label(egui::RichText::new(value).size(15.0).color(color).strong());
                    });
                });
                // The debt explained under the pointer, with what it costs right now.
                if k == last {
                    shown.response.on_hover_ui(|ui| debt_help(ui, c, fr, theme));
                }
            }
            if c.legacy.stars > 0 {
                egui::Frame::new().fill(theme.chrome_bg).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(tx(("Étoiles", "Stars"), fr)).size(11.0).color(theme.text_muted));
                        ui.label(egui::RichText::new(format!("★ {}", c.legacy.stars)).size(15.0).color(theme.ansi[3]).strong());
                    });
                });
            }
        });
    }

    /// The office's tab: the scene drawn; beside it (or under it, narrow) the stage and the news; under
    /// it, who's selected.
    fn tycoon_floor(&mut self, ui: &mut Ui, c: &mut Corp, now: i64, fr: bool, theme: &Theme) {
        let avail = ui.available_rect_before_wrap();
        let side = if avail.width() > 980.0 { 310.0 } else { 0.0 };
        let selected = self.tycoon.scene.selected.and_then(|id| c.staff.iter().find(|e| e.id == id).cloned());
        let under = if selected.is_some() { 96.0 } else if side == 0.0 { 64.0 } else { 0.0 };
        let scene_rect = Rect::from_min_max(avail.min, Pos2::new(avail.max.x - if side > 0.0 { side + 12.0 } else { 0.0 }, avail.max.y - under));
        let link = self.tycoon_scene(ui, scene_rect, c, fr, theme);
        let below = Rect::from_min_max(Pos2::new(scene_rect.min.x, scene_rect.max.y + 8.0), Pos2::new(scene_rect.max.x, avail.max.y));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(below).layout(egui::Layout::top_down(egui::Align::Min)));
        if let Some(e) = selected {
            let mut task = None;
            let mut more = false;
            card(&mut child, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let name = if e.founder() { tx(("Toi", "You"), fr).to_owned() } else { e.name.clone() };
                    ui.label(egui::RichText::new(name).size(14.0).color(theme.text).strong());
                    ui.label(egui::RichText::new(format!("{} · {} {}", tx(e.job().name, fr), tx(("niv.", "lvl"), fr), e.level)).size(12.5).color(theme.accent));
                    if let Some(tr) = e.trait_id.as_deref().and_then(corp::trait_def) {
                        ui.label(egui::RichText::new(format!("« {} »", tx(tr.name, fr))).size(12.0).color(theme.ansi[5])).on_hover_text(tx(tr.about, fr));
                    }
                    ui.label(egui::RichText::new(tx(("Moral", "Morale"), fr)).size(11.5).color(theme.text_muted));
                    bar(ui, e.morale / 100.0, level_color(e.morale / 100.0, theme), 80.0, 7.0, theme);
                    muted(ui, skills_text(&e.skills.map(|x| x * e.level_mult()), fr), theme);
                });
                ui.horizontal_wrapped(|ui| {
                    if e.makes() {
                        for (t, label) in corp::TASKS {
                            let on = e.task == t;
                            let b = egui::Button::new(egui::RichText::new(tx(label, fr)).size(12.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(10.0);
                            if ui.add(b).clicked() && !on {
                                task = Some(t);
                            }
                        }
                    } else {
                        muted(ui, tx(e.job().about, fr), theme);
                    }
                    if small(ui, tx(("Voir l'équipe →", "See the team →"), fr)) {
                        more = true;
                    }
                });
            });
            if let Some(t) = task {
                c.set_task(e.id, t);
            }
            if more {
                self.tycoon.tab = Tab::Team;
            }
        } else if side == 0.0 {
            for n in c.news.iter().rev().take(3) {
                child.label(egui::RichText::new(corp::news_text(n, fr)).size(12.0).color(theme.text_muted));
            }
        }
        if side > 0.0 {
            let column = Rect::from_min_max(Pos2::new(avail.max.x - side, avail.min.y), avail.max);
            let mut col = ui.new_child(egui::UiBuilder::new().max_rect(column).layout(egui::Layout::top_down(egui::Align::Min)));
            egui::ScrollArea::vertical().id_salt("tycoon-side").auto_shrink([false; 2]).show(&mut col, |ui| {
                ui.set_width(ui.available_width());
                self.tycoon_office(ui, c, now, fr, theme);
            });
        }
        match link {
            Some(tycoon_scene::Link::Team) => self.tycoon.tab = Tab::Team,
            Some(tycoon_scene::Link::Contracts) => self.tycoon.tab = Tab::Contracts,
            Some(tycoon_scene::Link::Products) => self.tycoon.tab = Tab::Products,
            Some(tycoon_scene::Link::Research) => self.tycoon.tab = Tab::Research,
            Some(tycoon_scene::Link::Premises) => self.tycoon.tab = Tab::Premises,
            None => {}
        }
    }

    /// Beside the scene: the premises, the next stage, the money, the news.
    fn tycoon_office(&mut self, ui: &mut Ui, c: &Corp, now: i64, fr: bool, theme: &Theme) {
        let wide = false;
        let left = |ui: &mut Ui| {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, tx(c.office().name, fr), theme);
                muted(ui, format!("{} · {} {}/{} · {} {}", per_min(c.office().rent, fr), tx(("places", "seats"), fr), c.staff.len(), c.capacity(), tx(("emplacements", "slots"), fr), c.office().slots), theme);
                ui.add_space(10.0);
                // Toward the next stage.
                let next = corp::STAGES.get(c.stage as usize + 1);
                match next {
                    Some(next) => {
                        let from = corp::STAGES[c.stage as usize].valuation;
                        let frac = ((c.valuation() - from) / (next.valuation - from)).clamp(0.0, 1.0);
                        ui.label(egui::RichText::new(format!("{} → {} ({})", tx(corp::STAGES[c.stage as usize].name, fr), tx(next.name, fr), money(next.valuation, fr))).size(13.0).color(theme.text));
                        bar(ui, frac, theme.accent, ui.available_width(), 8.0, theme);
                    }
                    None => {
                        ui.label(egui::RichText::new(tx(("Tu es en bourse. Il ne reste plus qu'à revendre… ou à continuer.", "You're listed. All that's left is to sell… or keep going."), fr)).size(13.0).color(theme.accent));
                    }
                }
                ui.add_space(10.0);
                let tier = c.tier();
                let tier_text = match corp::TIERS.get(tier as usize + 1) {
                    Some(need) => format!("{} {} · {} {:.0}", tx(("Contrats de palier", "Contracts up to tier"), fr), tier + 1, tx(("palier suivant à", "next tier at"), fr), need),
                    None => format!("{} {}", tx(("Contrats de palier", "Contracts up to tier"), fr), tier + 1),
                };
                muted(ui, tier_text, theme);
                let costs = c.costs();
                muted(ui, format!("{} {} · {} {}", tx(("Entrées", "In"), fr), per_min(c.income, fr), tx(("dépenses", "out"), fr), per_min(costs, fr)), theme);
                if c.cash < 0.0 {
                    ui.label(egui::RichText::new(tx(("Trésorerie à découvert : les salaires ne sont plus payés, le moral s'effondre et les gens partent.", "Cash in the red: salaries go unpaid, morale collapses and people leave."), fr)).size(12.5).color(theme.ansi[1]));
                }
            });
        };
        let right = |ui: &mut Ui| {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, tx(("Les nouvelles", "News"), fr), theme);
                ui.add_space(6.0);
                for n in c.news.iter().rev() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(ago(n.at, now, fr)).size(11.0).color(theme.text_muted));
                        ui.label(egui::RichText::new(corp::news_text(n, fr)).size(12.5).color(theme.text));
                    });
                }
            });
        };
        if wide {
            ui.columns(2, |cols| {
                left(&mut cols[0]);
                right(&mut cols[1]);
            });
        } else {
            left(ui);
            ui.add_space(10.0);
            right(ui);
        }
        let _ = self;
    }

    fn tycoon_team(&mut self, ui: &mut Ui, c: &mut Corp, fr: bool, theme: &Theme) {
        heading(ui, &format!("{} ({}/{})", tx(("L'équipe", "The team"), fr), c.staff.len(), c.capacity()), theme);
        ui.add_space(6.0);
        let mut fire = None;
        let mut tasks = Vec::new();
        let mut pauses: Vec<(String, Option<f64>)> = Vec::new();
        // In three blocks (the ones who make, the ones who steer, the support roles), each sorted by job then level.
        let rank = |e: &corp::Employee| (team_group(e.job().special), corp::JOBS.iter().position(|j| j.id == e.job).unwrap_or(99), std::cmp::Reverse(e.level), e.id);
        let mut ids: Vec<(u8, usize, std::cmp::Reverse<u8>, u32)> = c.staff.iter().map(rank).collect();
        ids.sort();
        let mut shown = None;
        for (group, _, _, id) in ids {
            let Some(e) = c.staff.iter().find(|e| e.id == id).cloned() else { continue };
            if shown != Some(group) {
                shown = Some(group);
                let members: Vec<&corp::Employee> = c.staff.iter().filter(|x| team_group(x.job().special) == group).collect();
                ui.add_space(if group == 0 { 2.0 } else { 12.0 });
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("{} ({})", tx(TEAM_GROUPS[group as usize], fr), members.len())).size(14.0).color(theme.accent).strong());
                    // How the ones who make are shared out between the tasks.
                    let split: Vec<String> = corp::TASKS.iter().filter_map(|(task, label)| {
                        let n = members.iter().filter(|x| x.makes() && x.task == *task).count();
                        (n > 0).then(|| format!("{} {}", tx(*label, fr), n))
                    }).collect();
                    if !split.is_empty() {
                        muted(ui, split.join(" · "), theme);
                    }
                });
                ui.add_space(4.0);
            }
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let name = if e.founder() { tx(("Toi", "You"), fr).to_owned() } else { e.name.clone() };
                    ui.label(egui::RichText::new(name).size(14.0).color(theme.text).strong());
                    ui.label(egui::RichText::new(format!("{} · {} {}", tx(e.job().name, fr), tx(("niv.", "lvl"), fr), e.level)).size(12.5).color(theme.accent));
                    if let Some(tr) = e.trait_id.as_deref().and_then(corp::trait_def) {
                        ui.label(egui::RichText::new(format!("« {} »", tx(tr.name, fr))).size(12.0).color(theme.ansi[5])).on_hover_text(tx(tr.about, fr));
                    }
                    if e.salary > 0.0 {
                        muted(ui, per_min(e.salary, fr), theme);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(("Moral", "Morale"), fr)).size(11.5).color(theme.text_muted));
                    bar(ui, e.morale / 100.0, level_color(e.morale / 100.0, theme), 90.0, 7.0, theme).on_hover_text(format!("{:.0}", e.morale));
                    ui.label(egui::RichText::new("XP").size(11.5).color(theme.text_muted));
                    let xp = if e.level >= corp::MAX_LEVEL { 1.0 } else { e.xp / e.xp_next() };
                    bar(ui, xp, theme.accent, 90.0, 7.0, theme);
                    muted(ui, skills_text(&e.skills.map(|x| x * e.level_mult()), fr), theme);
                });
                ui.horizontal_wrapped(|ui| {
                    if e.makes() {
                        for (task, label) in corp::TASKS {
                            let on = e.task == task;
                            let b = egui::Button::new(egui::RichText::new(tx(label, fr)).size(12.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(10.0);
                            if ui.add(b).clicked() && !on {
                                tasks.push((e.id, task));
                            }
                        }
                    } else {
                        muted(ui, tx(e.job().about, fr), theme);
                    }
                    // The roles that play alone: paused for a while, so the player can do it by hand.
                    if automates(e.job().special) {
                        ui.add_space(12.0);
                        let tip = tx(("Met en pause ce que ce rôle fait tout seul (tou·te·s les « {} »), pour le faire toi-même.", "Pauses what this role does alone (every “{}”), to do it yourself."), fr).replace("{}", tx(e.job().name, fr));
                        match c.paused.get(&e.job).copied() {
                            Some(left) => {
                                let text = if left == corp::PAUSED_UNTIL_RESUMED { tx(("⏸ en pause", "⏸ paused"), fr).to_owned() } else { format!("{} {}", tx(("⏸ en pause encore", "⏸ paused for"), fr), corp::duration(left, fr)) };
                                ui.label(egui::RichText::new(text).size(12.0).color(theme.ansi[3]));
                                if small(ui, tx(("Reprendre", "Resume"), fr)) {
                                    pauses.push((e.job.clone(), None));
                                }
                            }
                            None => {
                                if ui.add(egui::Button::new(egui::RichText::new(tx(("⏸ 5 min", "⏸ 5 min"), fr)).size(12.0)).corner_radius(6.0)).on_hover_text(&tip).clicked() {
                                    pauses.push((e.job.clone(), Some(300.0)));
                                }
                                if ui.add(egui::Button::new(egui::RichText::new(tx(("⏸ jusqu'à reprise", "⏸ until resumed"), fr)).size(12.0)).corner_radius(6.0)).on_hover_text(&tip).clicked() {
                                    pauses.push((e.job.clone(), Some(corp::PAUSED_UNTIL_RESUMED)));
                                }
                            }
                        }
                    }
                    if !e.founder() {
                        ui.add_space(12.0);
                        if self.tycoon.confirm == Some(e.id) {
                            if small(ui, format!("{} ({})", tx(("Confirmer", "Confirm"), fr), money(e.salary * 60.0, fr))) {
                                fire = Some(e.id);
                            }
                            if small(ui, tx(("Garder", "Keep"), fr)) {
                                self.tycoon.confirm = None;
                            }
                        } else if small(ui, tx(("Renvoyer", "Let go"), fr)) {
                            self.tycoon.confirm = Some(e.id);
                        }
                    }
                });
            });
            ui.add_space(6.0);
        }
        for (id, task) in tasks {
            c.set_task(id, task);
        }
        for (job, secs) in pauses {
            c.pause_role(&job, secs);
        }
        if let Some(id) = fire {
            c.fire(id);
            self.tycoon.confirm = None;
        }
        muted(ui, tx(("Contrats : les points vont aux contrats dans l'ordre. Produits : construit et maintient tes produits. Recherche : des points pour l'arbre tech. Refacto : fait baisser la dette technique.", "Contracts: points go to the contracts in order. Products: builds and maintains your products. Research: points for the tech tree. Refactoring: lowers the technical debt."), fr), theme);

        ui.add_space(16.0);
        heading(ui, tx(("Candidats", "Candidates"), fr), theme);
        ui.horizontal_wrapped(|ui| {
            muted(ui, format!("{} {}", tx(("Nouveaux candidats dans", "New candidates in"), fr), corp::duration(c.candidates_in, fr)), theme);
            let cost = c.repost_cost();
            if button(ui, c.cash >= cost, format!("{} ({})", tx(("Republier l'annonce", "Repost the ad"), fr), money(cost, fr)), theme) {
                c.repost();
            }
        });
        ui.add_space(6.0);
        let full = c.staff.len() >= c.capacity();
        if full {
            ui.label(egui::RichText::new(tx(("Plus de place : déménage (onglet Locaux) ou passe au télétravail (Direction).", "No room left: move (Premises tab) or go remote (Management)."), fr)).size(12.5).color(theme.ansi[3]));
        }
        let mut hire = None;
        for (k, e) in c.candidates.iter().enumerate() {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(&e.name).size(14.0).color(theme.text).strong());
                    ui.label(egui::RichText::new(format!("{} · {} {}", tx(e.job().name, fr), tx(("niv.", "lvl"), fr), e.level)).size(12.5).color(theme.accent)).on_hover_text(tx(e.job().about, fr));
                    if let Some(tr) = e.trait_id.as_deref().and_then(corp::trait_def) {
                        ui.label(egui::RichText::new(format!("« {} »", tx(tr.name, fr))).size(12.0).color(theme.ansi[5])).on_hover_text(tx(tr.about, fr));
                    }
                });
                muted(ui, format!("{} · {} {}", skills_text(&e.skills.map(|x| x * e.level_mult()), fr), tx(("salaire", "salary"), fr), per_min(e.salary, fr)), theme);
                let fee = c.hire_fee(e);
                let cto_taken = e.job == "cto" && c.staff.iter().any(|x| x.job == "cto");
                if button(ui, !full && c.cash >= fee && !cto_taken, format!("{} ({})", tx(("Embaucher", "Hire"), fr), money(fee, fr)), theme) {
                    hire = Some(k);
                }
            });
            ui.add_space(6.0);
        }
        if let Some(k) = hire {
            c.hire(k);
        }
    }

    fn contract_line(ui: &mut Ui, k: &corp::Contract, fr: bool, theme: &Theme, active: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(tx(k.kind().name, fr)).size(14.0).color(theme.text).strong());
            ui.label(egui::RichText::new(&k.client).size(12.5).color(theme.text_muted));
            ui.label(egui::RichText::new(format!("{} {}", tx(("palier", "tier"), fr), k.tier + 1)).size(12.0).color(theme.accent));
            ui.label(egui::RichText::new(money(k.pay, fr)).size(13.0).color(theme.ansi[2]).strong());
            if active {
                ui.label(egui::RichText::new(format!("{:.0} %", k.progress() * 100.0)).size(12.0).color(theme.accent));
            }
            if active {
                let (text, color) = if k.left >= 0.0 { (format!("{} {}", tx(("reste", "left"), fr), corp::duration(k.left, fr)), if k.left < k.time * 0.2 { theme.ansi[3] } else { theme.text_muted }) } else { (format!("{} {}", tx(("en retard de", "late by"), fr), corp::duration(-k.left, fr)), theme.ansi[1]) };
                ui.label(egui::RichText::new(text).size(12.0).color(color));
            } else {
                ui.label(egui::RichText::new(format!("{} {} · {} {}", tx(("délai", "deadline"), fr), corp::duration(k.time, fr), tx(("expire dans", "expires in"), fr), corp::duration(k.left, fr))).size(12.0).color(theme.text_muted));
            }
        });
        ui.horizontal_wrapped(|ui| {
            for s in 0..5 {
                if k.need[s] <= 0.0 {
                    continue;
                }
                ui.label(egui::RichText::new(format!("{} {:.0}", tx(corp::SKILLS[s], fr), k.need[s])).size(11.5).color(theme.text_muted));
                if active {
                    bar(ui, k.done[s] / k.need[s], theme.accent, 70.0, 6.0, theme);
                }
            }
        });
    }

    fn tycoon_contracts(&mut self, ui: &mut Ui, c: &mut Corp, fr: bool, theme: &Theme) {
        heading(ui, &format!("{} ({}/{})", tx(("En cours", "Under way"), fr), c.contracts.len(), c.max_contracts()), theme);
        muted(ui, tx(("Travaillés dans l'ordre : chaque compétence va au premier contrat qui en a besoin. En retard : moitié prix et réputation en baisse ; deux fois le délai : le client part.", "Worked in order: each skill goes to the first contract needing it. Late: half price and reputation down; twice the deadline: the client leaves."), fr), theme);
        ui.add_space(6.0);
        if c.contracts.is_empty() {
            muted(ui, tx(("Aucun contrat. Accepte une offre ci-dessous.", "No contract. Accept an offer below."), fr), theme);
        }
        let n = c.contracts.len();
        let mut act: Option<(usize, i8)> = None;
        for (k, x) in c.contracts.iter().enumerate() {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                Self::contract_line(ui, x, fr, theme, true);
                ui.horizontal(|ui| {
                    if k > 0 && small(ui, "↑") {
                        act = Some((k, -1));
                    }
                    if k + 1 < n && small(ui, "↓") {
                        act = Some((k, 1));
                    }
                    if small(ui, tx(("Abandonner", "Give up"), fr)) {
                        act = Some((k, 0));
                    }
                });
            });
            ui.add_space(6.0);
        }
        match act {
            Some((k, 0)) => c.drop_contract(k),
            Some((k, by)) => c.move_contract(k, by as isize),
            None => {}
        }

        ui.add_space(14.0);
        heading(ui, &format!("{} ({})", tx(("Offres", "Offers"), fr), c.offers.len()), theme);
        if c.sales() > 0 {
            let mut auto = c.policy.auto_accept;
            if ui.checkbox(&mut auto, egui::RichText::new(tx(("Les commerciaux acceptent les offres tout seuls", "The sales team accepts offers by itself"), fr)).size(12.5).color(theme.text)).changed() {
                let mut p = c.policy.clone();
                p.auto_accept = auto;
                c.set_policy(p);
            }
        } else {
            muted(ui, tx(("Un·e commercial·e apporterait plus d'offres, mieux payées, et les accepterait pendant ton absence.", "A salesperson would bring more offers, better paid, and accept them while you're away."), fr), theme);
        }
        ui.add_space(6.0);
        let full = c.contracts.len() >= c.max_contracts();
        let mut pick: Option<(usize, bool)> = None;
        for (k, x) in c.offers.iter().enumerate() {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                Self::contract_line(ui, x, fr, theme, false);
                ui.horizontal(|ui| {
                    if button(ui, !full, tx(("Accepter", "Accept"), fr), theme) {
                        pick = Some((k, true));
                    }
                    if small(ui, tx(("Refuser", "Decline"), fr)) {
                        pick = Some((k, false));
                    }
                });
            });
            ui.add_space(6.0);
        }
        if c.offers.is_empty() {
            muted(ui, tx(("Pas d'offre pour l'instant. La réputation et le marketing en amènent plus.", "No offers for now. Reputation and marketing bring more."), fr), theme);
        }
        match pick {
            Some((k, true)) => {
                c.accept(k);
            }
            Some((k, false)) => c.decline(k),
            None => {}
        }
        let _ = self;
    }

    fn tycoon_products(&mut self, ui: &mut Ui, c: &mut Corp, fr: bool, theme: &Theme) {
        heading(ui, &format!("{} ({}/{})", tx(("Tes produits", "Your products"), fr), c.products.len(), c.max_products()), theme);
        muted(ui, tx(("Mets des gens sur « Produits » dans l'Équipe : ils construisent la version suivante et maintiennent celle en ligne. Pas assez de maintenance : les bugs montent, les pannes arrivent, les utilisateurs partent.", "Put people on \"Products\" in the Team: they build the next version and maintain the live one. Not enough upkeep: bugs rise, outages come, users leave."), fr), theme);
        ui.add_space(6.0);
        let mut close = None;
        let pay = c.mods().pay;
        for p in &c.products {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(p.kind().name, fr)).size(14.0).color(theme.text).strong());
                    if p.version == 0 {
                        ui.label(egui::RichText::new(tx(("en construction", "being built"), fr)).size(12.5).color(theme.ansi[3]));
                    } else {
                        ui.label(egui::RichText::new(format!("v{}", p.version)).size(12.5).color(theme.accent));
                        ui.label(egui::RichText::new(format!("{:.0} {}", p.users, tx(("utilisateurs", "users"), fr))).size(12.5).color(theme.text));
                        ui.label(egui::RichText::new(format!("+{}", per_min(p.revenue() * pay, fr))).size(12.5).color(theme.ansi[2]));
                        if p.down > 0.0 {
                            ui.label(egui::RichText::new(format!("{} ({})", tx(("EN PANNE", "DOWN"), fr), corp::duration(p.down, fr))).size(12.5).color(theme.ansi[1]).strong());
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(("Version suivante", "Next version"), fr)).size(11.5).color(theme.text_muted));
                    bar(ui, p.progress / p.next_work(), theme.accent, 140.0, 7.0, theme).on_hover_text(format!("{:.0} / {:.0}", p.progress, p.next_work()));
                    if p.version > 0 {
                        ui.label(egui::RichText::new("Bugs").size(11.5).color(theme.text_muted));
                        bar(ui, p.bugs / 100.0, level_color(1.0 - p.bugs / 100.0, theme), 90.0, 7.0, theme).on_hover_text(format!("{:.0} %", p.bugs));
                        muted(ui, format!("{} {:.1} pts/s", tx(("maintenance", "upkeep"), fr), p.upkeep()), theme);
                    }
                    if small(ui, tx(("Fermer", "Shut down"), fr)) {
                        close = Some(p.id);
                    }
                });
            });
            ui.add_space(6.0);
        }
        if let Some(id) = close {
            c.close_product(id);
        }
        ui.add_space(14.0);
        heading(ui, tx(("Lancer un produit", "Launch a product"), fr), theme);
        ui.add_space(6.0);
        let mut launch = None;
        for (k, kind) in corp::PRODUCTS.iter().enumerate() {
            if c.products.iter().any(|p| p.kind as usize == k) {
                continue;
            }
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(kind.name, fr)).size(14.0).color(theme.text).strong());
                    muted(ui, tx(kind.about, fr), theme);
                });
                muted(ui, format!("{} {:.0} · {} {:.0} pts · {} {}/min", tx(("marché", "market"), fr), kind.market, tx(("v1 :", "v1:"), fr), kind.build, tx(("jusqu'à", "up to"), fr), money(kind.market * kind.price * 60.0, fr)), theme);
                let mut missing = Vec::new();
                if !c.has_tech(kind.tech) {
                    missing.push(format!("{} {}", tx(("recherche", "research"), fr), corp::tech(kind.tech).map_or("", |t| tx(t.name, fr))));
                }
                if c.stage < kind.stage {
                    missing.push(format!("{} {}", tx(("étape", "stage"), fr), tx(corp::STAGES[kind.stage as usize].name, fr)));
                }
                if c.products.len() >= c.max_products() {
                    // Where the next slot comes from: the research if not had yet, the next stage that adds one, or closing a product.
                    let mut ways = Vec::new();
                    if !c.has_tech("autoscale") {
                        ways.push(format!("{} {}", tx(("recherche", "research"), fr), corp::tech("autoscale").map_or("", |t| tx(t.name, fr))));
                    }
                    if let Some(s) = (c.stage as usize + 1..corp::STAGES.len()).find(|&s| s / 2 > c.stage as usize / 2) {
                        ways.push(format!("{} {}", tx(("étape", "stage"), fr), tx(corp::STAGES[s].name, fr)));
                    }
                    ways.push(tx(("fermer un produit", "close a product"), fr).to_owned());
                    missing.push(format!("{} ({})", tx(("une place de produit", "a product slot"), fr), ways.join(tx((" ou ", " or "), fr))));
                }
                if !missing.is_empty() {
                    ui.label(egui::RichText::new(format!("{} {}", tx(("Il faut :", "Needs:"), fr), missing.join(", "))).size(12.0).color(theme.ansi[3]));
                }
                if button(ui, c.can_launch(k), format!("{} ({})", tx(("Lancer", "Launch"), fr), money(kind.cost, fr)), theme) {
                    launch = Some(k);
                }
            });
            ui.add_space(6.0);
        }
        if let Some(k) = launch {
            c.launch(k);
        }
        let _ = self;
    }

    fn tycoon_research(&mut self, ui: &mut Ui, c: &mut Corp, fr: bool, theme: &Theme) {
        heading(ui, &format!("{} : {:.0} pts", tx(("Points de recherche", "Research points"), fr), c.research), theme);
        muted(ui, tx(("Gagnés par les gens sur « Recherche » (×2 pour les geeks, ×1,5 avec un·e CTO), et un peu par le travail que personne n'attend.", "Earned by people on \"Research\" (×2 for geeks, ×1.5 with a CTO), and a little from work nobody is waiting for."), fr), theme);
        ui.add_space(6.0);
        let mut learn = None;
        for t in corp::TECHS.iter() {
            let have = c.has_tech(t.id);
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(t.name, fr)).size(14.0).color(if have { theme.ansi[2] } else { theme.text }).strong());
                    muted(ui, tx(t.about, fr), theme);
                });
                if have {
                    ui.label(egui::RichText::new(tx(("✓ Acquis", "✓ Learned"), fr)).size(12.0).color(theme.ansi[2]));
                    return;
                }
                let missing: Vec<&str> = t.needs.iter().filter(|n| !c.has_tech(n)).filter_map(|n| corp::tech(n)).map(|x| tx(x.name, fr)).collect();
                if !missing.is_empty() {
                    ui.label(egui::RichText::new(format!("{} {}", tx(("Il faut d'abord :", "First needs:"), fr), missing.join(", "))).size(12.0).color(theme.ansi[3]));
                }
                ui.horizontal(|ui| {
                    if button(ui, c.can_research(t.id), format!("{} ({:.0} pts)", tx(("Débloquer", "Unlock"), fr), t.cost), theme) {
                        learn = Some(t.id);
                    }
                    if missing.is_empty() && c.research < t.cost {
                        bar(ui, c.research / t.cost, theme.accent, 120.0, 7.0, theme);
                    }
                });
            });
            ui.add_space(6.0);
        }
        if let Some(id) = learn {
            c.learn(id);
        }
        let _ = self;
    }

    fn tycoon_premises(&mut self, ui: &mut Ui, c: &mut Corp, fr: bool, theme: &Theme) {
        heading(ui, tx(("Les locaux", "Premises"), fr), theme);
        ui.add_space(6.0);
        let mut moved = false;
        for (k, o) in corp::OFFICES.iter().enumerate() {
            let here = k == c.office as usize;
            let past = k < c.office as usize;
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(o.name, fr)).size(14.0).color(if here { theme.accent } else if past { theme.text_muted } else { theme.text }).strong());
                    muted(ui, format!("{} {} · {} {} · {} {} · {}", o.capacity, tx(("places", "seats"), fr), o.slots, tx(("emplacements", "slots"), fr), tx(("confort", "comfort"), fr), o.comfort, per_min(o.rent, fr)), theme);
                    if here {
                        ui.label(egui::RichText::new(tx(("← tu es ici", "← you are here"), fr)).size(12.0).color(theme.accent));
                    }
                });
                if k == c.office as usize + 1 && button(ui, c.can_move(), format!("{} ({})", tx(("Déménager", "Move"), fr), money(o.price, fr)), theme) {
                    moved = true;
                }
            });
            ui.add_space(4.0);
        }
        if moved {
            c.move_office();
        }
        ui.add_space(14.0);
        heading(ui, &format!("{} ({}/{})", tx(("Aménagements", "Fittings"), fr), c.perks.len(), c.office().slots), theme);
        ui.add_space(6.0);
        let mut buy = None;
        let mut sell = None;
        for p in corp::PERKS.iter() {
            let have = c.has_perk(p.id);
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(p.name, fr)).size(14.0).color(if have { theme.ansi[2] } else { theme.text }).strong());
                    muted(ui, tx(p.about, fr), theme);
                    if p.upkeep > 0.0 {
                        muted(ui, format!("{} {}", tx(("entretien", "upkeep"), fr), per_min(p.upkeep, fr)), theme);
                    }
                });
                if have {
                    if small(ui, format!("{} (+{})", tx(("Revendre", "Sell"), fr), money(p.price * 0.3, fr))) {
                        sell = Some(p.id);
                    }
                } else if button(ui, c.can_buy_perk(p.id), format!("{} ({})", tx(("Installer", "Install"), fr), money(p.price, fr)), theme) {
                    buy = Some(p.id);
                }
            });
            ui.add_space(4.0);
        }
        if let Some(id) = buy {
            c.buy_perk(id);
        }
        if let Some(id) = sell {
            c.sell_perk(id);
        }
        let _ = self;
    }

    fn tycoon_board(&mut self, ui: &mut Ui, c: &mut Corp, now: i64, fr: bool, theme: &Theme) {
        heading(ui, tx(("Politique de la maison", "House rules"), fr), theme);
        ui.add_space(6.0);
        let mut p = c.policy.clone();
        let s = |a: &[corp::Tx]| a.iter().map(|x| tx(*x, fr).to_owned()).collect::<Vec<_>>();
        let rows: [(corp::Tx, corp::Tx, Vec<String>, u8); 6] = [
            (("Télétravail", "Remote work"), ("Hybride : moral +4, 25 % de places en plus. Full remote : moral +6, 50 % de places en plus, productivité −5 %.", "Hybrid: morale +4, 25 % more seats. Fully remote: morale +6, 50 % more seats, productivity −5 %."), s(&[("Au bureau", "At the office"), ("Hybride", "Hybrid"), ("Full remote", "Fully remote")]), p.remote),
            (("Horaires", "Hours"), ("Cool : −15 % de productivité, moral +8, moins de dette. Crunch : +25 % de productivité, moral −20, dette ×2.", "Relaxed: −15 % productivity, morale +8, less debt. Crunch: +25 % productivity, morale −20, debt ×2."), s(&[("Cool", "Relaxed"), ("Normaux", "Normal"), ("Crunch", "Crunch")]), p.hours),
            (("Salaires", "Salaries"), ("Bas : −20 % de coût, moral −15. Généreux : +25 % de coût, moral +10, meilleurs candidats.", "Low: −20 % cost, morale −15. Generous: +25 % cost, morale +10, better candidates."), s(&[("Bas", "Low"), ("Du marché", "Market"), ("Généreux", "Generous")]), p.salary),
            (("Avantages", "Benefits"), ("Tickets resto : moral +5 (+6 % de masse salariale). Tout compris : moral +10 (+15 %).", "Meal vouchers: morale +5 (+6 % payroll). Everything: morale +10 (+15 %)."), s(&[("Aucun", "None"), ("Tickets resto", "Meal vouchers"), ("Tout compris", "Everything")]), p.benefits),
            (("Marketing", "Marketing"), ("Plus d'offres (+25 % par cran) et plus d'utilisateurs (+30 %). Le coût grandit avec la boîte.", "More offers (+25 % per notch) and more users (+30 %). The cost grows with the company."), (0..5u8).map(|k| if k == 0 { tx(("Aucun", "None"), fr).to_owned() } else { "★".repeat(k as usize) }).collect(), p.marketing),
            (("Formation", "Training"), ("XP ×1,5 (+5 % de masse salariale) ou ×2,2 (+12 %).", "XP ×1.5 (+5 % payroll) or ×2.2 (+12 %)."), s(&[("Aucune", "None"), ("Un peu", "Some"), ("Beaucoup", "Lots")]), p.training),
        ];
        for (k, (name, about, labels, current)) in rows.into_iter().enumerate() {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(tx(name, fr)).size(14.0).color(theme.text).strong());
                muted(ui, tx(about, fr), theme);
                if k == 4 {
                    muted(ui, format!("{} {}", tx(("Coût actuel :", "Current cost:"), fr), per_min(c.marketing_cost(), fr)), theme);
                }
                if let Some(v) = segmented(ui, current, &labels, theme) {
                    match k {
                        0 => p.remote = v,
                        1 => p.hours = v,
                        2 => p.salary = v,
                        3 => p.benefits = v,
                        4 => p.marketing = v,
                        _ => p.training = v,
                    }
                }
            });
            ui.add_space(6.0);
        }
        c.set_policy(p);

        ui.add_space(14.0);
        heading(ui, tx(("Automatisations", "Automations"), fr), theme);
        muted(ui, format!("{} {}.", tx(("Recrute les bons rôles et ils jouent à ta place, même Ronnie fermé. Ils gardent toujours une réserve de", "Hire the right roles and they play for you, even with Ronnie closed. They always keep a reserve of"), fr), money(c.reserve(), fr)), theme);
        ui.add_space(6.0);
        let mut p = c.policy.clone();
        {
            use corp::Special;
            let rows: [(&str, Special, corp::Tx, &mut bool); 8] = [
                ("sales", Special::Sales, ("Accepter les offres de contrats", "Accept the contract offers"), &mut p.auto_accept),
                ("hr", Special::Hr, ("Recruter sur les places libres", "Hire into the free seats"), &mut p.auto.hire),
                ("lead", Special::Lead, ("Répartir les tâches (refacto, produits, recherche)", "Share out the tasks (refactoring, products, research)"), &mut p.auto.tasks),
                ("cto", Special::Cto, ("Débloquer les technos", "Unlock the techs"), &mut p.auto.research),
                ("office", Special::Office, ("Installer des aménagements", "Install fittings"), &mut p.auto.fittings),
                ("office", Special::Office, ("Déménager quand c'est plein", "Move when full"), &mut p.auto.moving),
                ("po", Special::Po, ("Lancer des produits", "Launch products"), &mut p.auto.products),
                ("assistant", Special::Assistant, ("Répondre aux imprévus qui attendent", "Answer the surprises left waiting"), &mut p.auto.events),
            ];
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                for (job, special, label, flag) in rows {
                    let j = corp::job(job);
                    let has = c.has_role(special);
                    ui.horizontal_wrapped(|ui| {
                        ui.add_enabled(has, egui::Checkbox::new(flag, egui::RichText::new(tx(label, fr)).size(13.0).color(if has { theme.text } else { theme.text_muted })));
                        if has && c.paused.contains_key(job) {
                            ui.label(egui::RichText::new(format!("⏸ {} {}", tx(j.name, fr), tx(("en pause (onglet Équipe)", "paused (Team tab)"), fr))).size(12.0).color(theme.ansi[3]));
                        } else if has {
                            ui.label(egui::RichText::new(format!("⚙ {}", tx(j.name, fr))).size(12.0).color(theme.ansi[2]));
                        } else {
                            let when = if c.stage < j.stage { format!(" ({} {})", tx(("dès l'étape", "from stage"), fr), tx(corp::STAGES[j.stage as usize].name, fr)) } else { String::new() };
                            ui.label(egui::RichText::new(format!("{} {}{when}", tx(("Il faut un·e", "Needs a"), fr), tx(j.name, fr))).size(12.0).color(theme.ansi[3]));
                        }
                    });
                }
            });
        }
        c.set_policy(p);

        ui.add_space(14.0);
        heading(ui, tx(("Revendre la boîte", "Sell the company"), fr), theme);
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            muted(ui, tx(("Tu repars du garage, mais chaque étoile donne pour toujours +5 % de productivité, +3 % sur ce que tu encaisses et 1 000 € de plus au départ.", "You start again in the garage, but each star gives forever +5 % productivity, +3 % on what you earn and €1,000 more to start with."), fr), theme);
            muted(ui, format!("{} ★ {} · {} {} · {} {}", tx(("Étoiles :", "Stars:"), fr), c.legacy.stars, tx(("boîtes revendues :", "companies sold:"), fr), c.legacy.sales, tx(("record :", "record:"), fr), money(c.legacy.best, fr)), theme);
            let stars = c.sell_stars();
            if stars == 0 {
                ui.label(egui::RichText::new(format!("{} {}.", tx(("Possible à partir de l'étape", "Possible from the stage"), fr), tx(corp::STAGES[corp::SELL_STAGE as usize].name, fr))).size(12.5).color(theme.ansi[3]));
            } else if self.tycoon.sell_confirm {
                ui.label(egui::RichText::new(tx(("Sûr ? L'équipe, les locaux, les produits et la trésorerie seront perdus.", "Sure? The team, premises, products and cash will be gone."), fr)).size(12.5).color(theme.ansi[1]));
                ui.horizontal(|ui| {
                    if button(ui, true, format!("{} (+{} ★)", tx(("Vendre", "Sell"), fr), stars), theme) {
                        c.sell(now);
                        self.tycoon.sell_confirm = false;
                        self.tycoon.tab = Tab::Office;
                    }
                    if small(ui, tx(("Annuler", "Cancel"), fr)) {
                        self.tycoon.sell_confirm = false;
                    }
                });
            } else if button(ui, true, format!("{} {} (+{} ★)", tx(("Vendre pour", "Sell for"), fr), money(c.valuation(), fr), stars), theme) {
                self.tycoon.sell_confirm = true;
            }
        });

        // The world's board.
        ui.add_space(14.0);
        heading(ui, tx(("Classement mondial (meilleure valorisation)", "World ranking (best valuation)"), fr), theme);
        ui.add_space(4.0);
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        match (pseudo, self.tycoon.floor.board()) {
            (None, _) => muted(ui, self.t().game_need_pseudo, theme),
            (_, None) => muted(ui, "…", theme),
            (_, Some([])) => muted(ui, self.t().game_board_empty, theme),
            (Some(me), Some(board)) => {
                egui::Grid::new("tycoon-board").num_columns(3).spacing([16.0, 4.0]).show(ui, |ui| {
                    for e in board.iter().take(10) {
                        let color = if e.me { theme.accent } else { theme.text };
                        ui.label(egui::RichText::new(format!("{}.", e.rank)).size(12.5).color(theme.text_muted));
                        ui.label(egui::RichText::new(if e.me { me.as_str() } else { e.name.as_deref().unwrap_or(self.t().game_anonymous) }).size(12.5).color(color));
                        ui.label(egui::RichText::new(money(e.score as f64 * 1_000.0, fr)).size(12.5).color(color));
                        ui.end_row();
                    }
                });
            }
        }
    }

    /// The event waiting, in a card over the page.
    fn tycoon_event(&mut self, ctx: &egui::Context, c: &mut Corp, fr: bool, theme: &Theme) {
        let Some(def) = c.event.as_deref().and_then(corp::event) else { return };
        let who = c.staff.iter().find(|e| e.id == c.event_who).map(|e| e.name.clone()).unwrap_or_else(|| tx(("Quelqu'un", "Someone"), fr).to_owned());
        let mut chosen = None;
        egui::Window::new(tx(def.title, fr)).id(egui::Id::new("tycoon-event")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).frame(egui::Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.accent)).corner_radius(12.0).inner_margin(18.0)).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.label(egui::RichText::new(tx(def.text, fr).replace("{who}", &who)).size(14.0).color(theme.text));
            ui.add_space(12.0);
            for (k, choice) in def.choices.iter().enumerate() {
                if button(ui, true, tx(choice.label, fr), theme) {
                    chosen = Some(k);
                }
                ui.add_space(4.0);
            }
        });
        if let Some(k) = chosen {
            c.choose(k);
        }
    }

    /// The tutorial: a card at the bottom of the page, what it's about blinking; the steps to play go
    /// on by themselves once done. And its button, to see it again.
    fn tycoon_tutorial(&mut self, ctx: &egui::Context, page: Rect, c: &mut Corp, fr: bool, theme: &Theme) {
        let help = Rect::from_min_size(Pos2::new(page.max.x - 46.0, page.min.y + 14.0), Vec2::new(30.0, 28.0));
        let replay = egui::Area::new(egui::Id::new("tycoon-help")).fixed_pos(help.min).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.add(egui::Button::new(egui::RichText::new("?").size(15.0).strong()).corner_radius(14.0).min_size(help.size())).on_hover_text(tx(("Revoir le tutoriel", "See the tutorial again"), fr)).clicked()
        });
        if replay.inner {
            c.tutorial = 0;
            self.tycoon.replay = true;
        }
        if c.tutorial as usize >= STEPS.len() {
            self.tycoon.replay = false;
            return;
        }
        // The steps already done skipped (unless seen again).
        while !self.tycoon.replay && (c.tutorial as usize) < STEPS.len() && STEPS[c.tutorial as usize].done.is_some_and(|d| d(c)) {
            c.tutorial += 1;
        }
        let Some(step) = STEPS.get(c.tutorial as usize) else {
            c.tutorial = corp::TUTORIAL_DONE;
            return;
        };
        let now = ctx.input(|i| i.time);
        // What it's about, blinking.
        let target = match step.spot {
            Spot::Tab(tab) if self.tycoon.tab != tab => TABS.iter().position(|(t, _)| *t == tab).map(|k| self.tycoon.tab_rects[k]),
            Spot::Board if self.tycoon.tab != Tab::Office => Some(self.tycoon.tab_rects[0]),
            Spot::Board => self.tycoon.scene.board,
            _ => None,
        };
        if let Some(r) = target.filter(|r| r.is_positive()) {
            let pulse = 0.5 + 0.5 * (now * 5.0).sin() as f32;
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tycoon-tuto-spot")));
            painter.rect_stroke(r.expand(3.0 + pulse * 3.0), 14.0, Stroke::new(2.5, theme.ansi[3].gamma_multiply(0.5 + pulse * 0.5)), egui::StrokeKind::Outside);
            painter.text(Pos2::new(r.center().x, r.max.y + 8.0 + pulse * 4.0), Align2::CENTER_TOP, "▲", FontId::proportional(16.0), theme.ansi[3]);
            ctx.request_repaint();
        }
        let width = (page.width() - 40.0).min(460.0);
        let mut next = false;
        let mut skip = false;
        egui::Area::new(egui::Id::new("tycoon-tuto")).order(egui::Order::Foreground).anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -18.0)).show(ctx, |ui| {
            egui::Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.ansi[3])).corner_radius(12.0).inner_margin(14.0).show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} · {}/{}", tx(("Tutoriel", "Tutorial"), fr), c.tutorial + 1, STEPS.len())).size(11.5).color(theme.ansi[3]));
                });
                ui.label(egui::RichText::new(tx(step.title, fr)).size(15.0).color(theme.text).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tx(step.text, fr)).size(13.0).color(theme.text));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let last = c.tutorial as usize + 1 == STEPS.len();
                    if step.done.is_none() || self.tycoon.replay {
                        let label = if last { tx(("Terminer", "Finish"), fr) } else if c.tutorial == 0 { tx(("C'est parti !", "Let's go!"), fr) } else { tx(("Suivant", "Next"), fr) };
                        next = button(ui, true, label, theme);
                    } else {
                        ui.label(egui::RichText::new(tx(("À toi de jouer…", "Your turn…"), fr)).size(12.5).color(theme.text_muted).italics());
                    }
                    if !last {
                        ui.add_space(12.0);
                        skip = small(ui, tx(("Passer le tutoriel", "Skip the tutorial"), fr));
                    }
                });
            });
        });
        if skip {
            c.tutorial = corp::TUTORIAL_DONE;
            self.tycoon.replay = false;
        } else if next {
            c.tutorial += 1;
            if c.tutorial as usize >= STEPS.len() {
                c.tutorial = corp::TUTORIAL_DONE;
                self.tycoon.replay = false;
            }
        }
    }

    /// What happened while Ronnie was closed.
    fn tycoon_away(&mut self, ctx: &egui::Context, fr: bool, theme: &Theme) {
        let Some(sum) = self.tycoon.away.clone() else { return };
        let mut ok = false;
        egui::Window::new(tx(("Pendant ton absence", "While you were away"), fr)).id(egui::Id::new("tycoon-away")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).frame(egui::Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.accent)).corner_radius(12.0).inner_margin(18.0)).show(ctx, |ui| {
            ui.set_max_width(400.0);
            let counted = sum.secs.min(corp::OFFLINE_CAP);
            muted(ui, format!("{} {} · {} {} ({})", tx(("Absent·e", "Away"), fr), corp::duration(sum.secs, fr), tx(("rattrapé", "caught up"), fr), corp::duration(counted, fr), tx(("au ralenti", "slowed down"), fr)), theme);
            ui.add_space(8.0);
            for (label, value, color) in [
                (tx(("Encaissé", "Earned"), fr), money(sum.earned, fr), theme.ansi[2]),
                (tx(("Dépensé", "Spent"), fr), money(sum.spent, fr), theme.ansi[1]),
                (tx(("Contrats livrés", "Contracts delivered"), fr), sum.delivered.to_string(), theme.text),
                (tx(("Niveaux gagnés", "Levels gained"), fr), sum.levels.to_string(), theme.text),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(label).size(13.0).color(theme.text_muted));
                    ui.label(egui::RichText::new(value).size(14.0).color(color).strong());
                });
            }
            if sum.stopped {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tx(("La trésorerie allait passer dans le rouge : la boîte s'est mise en pause.", "The cash was about to go into the red: the company paused."), fr)).size(12.5).color(theme.ansi[3]));
            }
            ui.add_space(10.0);
            ok = button(ui, true, "OK", theme);
        });
        if ok {
            self.tycoon.away = None;
        }
    }
}
