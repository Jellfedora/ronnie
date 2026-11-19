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

mod group_tab;
mod life_tab;

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
    Group,
    Life,
    Players,
    Rewards,
}

const TABS: [(Tab, corp::Tx); 11] = [
    (Tab::Office, ("Bureau", "Office")),
    (Tab::Team, ("Équipe", "Team")),
    (Tab::Contracts, ("Contrats", "Contracts")),
    (Tab::Products, ("Produits", "Products")),
    (Tab::Research, ("Recherche", "Research")),
    (Tab::Premises, ("Locaux", "Premises")),
    (Tab::Board, ("Direction", "Management")),
    (Tab::Group, ("Groupe", "Group")),
    (Tab::Life, ("Vie privée", "Private life")),
    (Tab::Players, ("Joueurs", "Players")),
    (Tab::Rewards, ("Récompenses", "Rewards")),
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

/// The tour of what season 2 brought, for those who had done the tutorial already.
const WHATS_NEW: [Step; 5] = [
    Step {
        title: ("Ta vie privée", "Your private life"),
        text: ("Nouvel onglet : ton compte perso, ta maison, ta voiture, ta famille, tes placements et ton stress. Commence par te verser un salaire de fondateur·rice : sans lui, ton compte reste vide. Ta vie compte aussi pour la boîte : stressé·e, tu travailles moins ; en burn-out, plus du tout.", "New tab: your personal account, your home, your car, your family, your investments and your stress. Start by paying yourself a founder's salary: without it, your account stays empty. Your life matters for the company too: stressed, you work less; burnt out, not at all."),
        spot: Spot::Tab(Tab::Life),
        done: None,
    },
    Step {
        title: ("Tes produits ont un cap", "Your products have a focus"),
        text: ("Chaque produit en ligne a maintenant un cap (fonctionnalités, performance, croissance, monétisation), un prix (du gratuit avec pubs au luxe) et peut pivoter vers un autre produit.", "Each live product now has a focus (features, performance, growth, monetisation), a price (from free with ads to luxury) and can pivot to another product."),
        spot: Spot::Tab(Tab::Products),
        done: None,
    },
    Step {
        title: ("Les étoiles et les défis", "Stars and challenges"),
        text: ("Les étoiles gagnées en revendant se placent dans des talents (argent de départ, technos offertes, productivité…). Et en revendant, tu peux choisir un défi pour la boîte suivante : réussi, il rapporte des étoiles en plus.", "The stars won by selling now go into talents (starting money, free techs, productivity…). And when you sell, you can pick a challenge for the next company: won, it brings more stars."),
        spot: Spot::Tab(Tab::Board),
        done: None,
    },
    Step {
        title: ("Le groupe", "The group"),
        text: ("Dès la PME, des concurrents prennent des utilisateurs à tes produits : rachète-les. Ouvre des bureaux à l'étranger pour des places et des bonus. Et une fois en bourse, un conseil d'administration te fixe un objectif chaque trimestre.", "From the small business on, rivals take users from your products: buy them. Open offices abroad for seats and bonuses. And once listed, a board of directors sets you a goal each quarter."),
        spot: Spot::Tab(Tab::Group),
        done: None,
    },
    Step {
        title: ("La recherche ne s'arrête plus", "Research never stops"),
        text: ("Une fois toutes les technos trouvées, des optimisations sans fin prennent le relais. Et ton ou ta chef·fe de projet n'envoie plus qu'un cinquième de l'équipe en recherche.", "Once every tech is found, endless optimisations take over. And your project manager now sends only a fifth of the team to research."),
        spot: Spot::Tab(Tab::Research),
        done: None,
    },
];

/// Shown once, when the first rival comes.
const RIVAL_TIP: Step = Step {
    title: ("Un concurrent arrive !", "A rival shows up!"),
    text: ("Il prend une part du marché de son type de produit : tes produits du même type perdent des utilisateurs. Onglet Groupe : tu peux le racheter, avec ses utilisateurs et une partie de son équipe (et de sa dette).", "It takes a share of its kind of product's market: your products of the same kind lose users. Group tab: you can buy it, with its users and part of its team (and of its debt)."),
    spot: Spot::Tab(Tab::Group),
    done: None,
};

const STEPS: [Step; 11] = [
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
        title: ("Et toi, dans tout ça ?", "And what about you?"),
        text: ("Tu as aussi une vie : un compte perso, une maison, peut-être une famille, et un niveau de stress. Onglet Vie privée : verse-toi un salaire de fondateur·rice (« Modeste » suffit pour commencer), payé par la boîte. Attention : trop de stress, et tu travailles moins bien.", "You have a life too: a personal account, a home, maybe a family, and a stress level. Private life tab: pay yourself a founder's salary (\"Modest\" is fine to start with), paid by the company. Careful: too much stress and you work less well."),
        spot: Spot::Tab(Tab::Life),
        done: Some(|c| c.policy.founder_pay > 0),
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
    tab_rects: [Rect; 11],
    /// The challenge picked for the next run, at a sale.
    challenge: Option<&'static str>,
    /// The tutorial seen again (from its button): each step goes on with « Next ».
    replay: bool,
    pub(super) floor: crate::floor::GameFloor,
    /// What was sent to floor: the best valuation (k€), the furthest stage, the stars.
    sent: (u32, u8, u32),
    /// When the company as it is was last put to send (the Players tab of the others).
    now_sent: Option<f64>,
    /// What the last event's choice made, shown until closed.
    outcome: Option<Outcome>,
}

/// What an event's choice made: its text, and what moved.
struct Outcome {
    title: String,
    text: String,
    changes: Vec<(String, f64, bool)>,
    /// A notice (a new season), not an event's outcome: titled by itself, no list of what moved.
    notice: bool,
}

impl Default for Tycoon {
    fn default() -> Self {
        Self { tab: Tab::Office, live: None, written: None, wrote_at: 0.0, started: false, away: None, confirm: None, sell_confirm: false, scene: Default::default(), tab_rects: [Rect::NOTHING; 11], challenge: None, replay: false, floor: crate::floor::GameFloor::new("ronnie-corp", false).with_limit(50), sent: (0, 0, 0), now_sent: None, outcome: None }
    }
}

/// What goes to floor: the best valuation (k€), the furthest stage, the stars; and, `with_now`, the
/// company as it is (the Players tab). None below 1 k€ (floor refuses it).
fn corp_round(c: &Corp, with_now: bool) -> Option<crate::floor::GameRound> {
    let best = (c.legacy.best / 1_000.0).min(u32::MAX as f64) as u32;
    if best == 0 {
        return None;
    }
    // floor knows the stages up to the listing: the ones past it go as it.
    let mut details = serde_json::json!({ "valuation": best, "stage": c.best_stage().min(corp::FLOOR_STAGE), "stars": c.legacy.floor_stars().min(1_000_000) });
    if with_now {
        details["now"] = serde_json::json!({
            "valuation": (c.valuation() / 1_000.0).clamp(0.0, u32::MAX as f64) as u64,
            "stage": c.stage.min(corp::FLOOR_STAGE),
            "staff": c.staff.len().max(1),
            "products": c.products.len().min(100),
            "net": ((c.income - c.spend) * 60.0).clamp(-1e12, 1e12) as i64,
        });
    }
    Some(crate::floor::GameRound { score: best, details })
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

/// How many of a role the company has, how many are worth it (`Corp::role_useful`), and what each
/// one does (None: the devs).
fn role_use(c: &Corp, special: corp::Special) -> Option<(usize, usize, corp::Tx)> {
    use corp::Special::*;
    let useful = c.role_useful(special)?;
    let about = match special {
        Sales => ("Chacun·e : +40 % d'offres de contrats, +5 % sur tout ce que tu encaisses (produits compris). Utile jusqu'à 5.", "Each: +40 % contract offers, +5 % on all you earn (products too). Worth it up to 5."),
        Hr => ("Chacun·e : +3 de moral pour toute l'équipe, un candidat de plus. Utile jusqu'à 3.", "Each: +3 morale for the whole team, one more candidate. Worth it up to 3."),
        Lead => ("Chacun·e rend 6 personnes 15 % plus productives (un·e pour 6 qui produisent) et donne un contrat en cours de plus.", "Each makes 6 people 15 % more productive (one per 6 who make) and gives one more running contract."),
        Qa => ("Moins de bugs, meilleures livraisons : un·e pour 5 qui produisent.", "Fewer bugs, better deliveries: one per 5 who make."),
        Ops => ("Chacun·e : −15 % de pannes (et produit aussi). Utile jusqu'à 4.", "Each: −15 % outages (and makes too). Worth it up to 4."),
        Cto => ("Un·e seul·e possible.", "Only one possible."),
        _ => ("N'automatise qu'une chose : un·e seul·e suffit.", "Only automates one thing: one is enough."),
    };
    Some((c.count(special), useful, about))
}

/// « 2/5 utiles », green while worth it, orange past it; its hover says why.
fn role_use_label(ui: &mut Ui, c: &Corp, special: corp::Special, fr: bool, theme: &Theme) {
    let Some((have, useful, about)) = role_use(c, special) else { return };
    let (text, color) = if have > useful {
        (format!("{have}/{useful} {}", tx(("utiles : trop", "worth it: too many"), fr)), theme.ansi[3])
    } else {
        (format!("{have}/{useful} {}", tx(("utiles", "worth it"), fr)), theme.ansi[2])
    };
    ui.label(egui::RichText::new(text).size(12.0).color(color)).on_hover_text(tx(about, fr));
}

/// « ⭐ Star » beside a star employee, what they bring under the pointer.
fn star_badge(ui: &mut Ui, e: &corp::Employee, fr: bool, theme: &Theme) {
    if let Some(s) = e.star.as_deref().and_then(corp::star_def) {
        ui.label(egui::RichText::new(tx(("⭐ Star", "⭐ Star"), fr)).size(12.5).color(theme.ansi[3]).strong()).on_hover_text(tx(s.about, fr));
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
        // A company from an older season: started again, its records kept (put in the settings below:
        // one more action), and said.
        if c.season < corp::SEASON {
            *c = c.new_season(now);
            let fr = self.config.settings.language == Lang::Fr;
            self.tycoon.away = None;
            self.tycoon.outcome = Some(Outcome {
                title: tx(("Ronnie Corp, saison 2", "Ronnie Corp, season 2"), fr).to_owned(),
                text: tx(("Le jeu a beaucoup grandi : un conseil d'administration après la bourse, deux étapes de plus, des concurrents à racheter, des bureaux à l'étranger, des employés stars, ta vie privée… Toutes les boîtes repartent du garage, étoiles comprises, pour que tout le monde reparte à égalité. Tes records sont gardés : ta meilleure valorisation (le classement), l'étape la plus loin, tes reventes, et les récompenses déjà gagnées.", "The game has grown a lot: a board of directors after the listing, two more stages, rivals to buy, offices abroad, star employees, your private life… Every company starts again from the garage, stars included, so everyone starts level. Your records are kept: your best valuation (the ranking), the furthest stage, your sales, and the rewards already won."), fr).to_owned(),
                changes: Vec::new(),
                notice: true,
            });
            return;
        }
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
        let (stage, stars) = (c.best_stage(), c.legacy.floor_stars());
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
        let round = corp_round(&c, true);
        self.config.settings.tycoon = Some(c.clone());
        self.tycoon.written = Some(c);
        self.tycoon.wrote_at = time;
        self.save_config();
        let (was, was_stage, was_stars) = self.tycoon.sent;
        if (best > was || stage > was_stage || stars > was_stars) && best > 0 && crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            self.tycoon.sent = (best.max(was), stage.max(was_stage), stars.max(was_stars));
            if let Some(round) = round {
                self.tycoon.floor.finished(round);
            }
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
        // The company as it is, for the others' Players tab: sent again with the same record now and then.
        if self.tycoon.now_sent.is_none_or(|at| time - at > 110.0)
            && crate::floor::pseudo(account.as_ref()).is_some()
            && let Some(c) = &self.tycoon.live
        {
            self.tycoon.now_sent = Some(time);
            if let Some(round) = corp_round(c, true) {
                self.tycoon.floor.finished(round);
            }
        }
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
                Tab::Group => self.tycoon_group(ui, &mut c, fr, &theme),
                Tab::Life => self.tycoon_life(ui, &mut c, fr, &theme),
                Tab::Players => self.tycoon_players(ui, &c, fr, &theme),
                Tab::Rewards => self.tycoon_rewards(ui, &c, fr, &theme),
            }
            ui.add_space(20.0);
            });
        }
        self.tycoon_tutorial(ui.ctx(), rect, &mut c, fr, &theme);
        self.tycoon_event(ui.ctx(), &mut c, fr, &theme);
        self.tycoon_outcome(ui.ctx(), fr, &theme);
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
            if let Some(b) = &c.board {
                egui::Frame::new().fill(theme.chrome_bg).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(tx(("Action", "Share"), fr)).size(11.0).color(theme.text_muted));
                        ui.label(egui::RichText::new(format!("{:.1}", b.share)).size(15.0).color(if b.share >= 100.0 { theme.ansi[2] } else { theme.ansi[1] }).strong());
                    });
                });
            }
            // The founder's own account, and how they're doing.
            let l = &c.life;
            egui::Frame::new().fill(theme.chrome_bg).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.vertical(|ui| {
                    let mood = if l.burnout > 0.0 { " 🤒" } else if l.holiday > 0.0 { " 🌴" } else if l.stress > 70.0 { " 😰" } else { "" };
                    ui.label(egui::RichText::new(format!("{}{mood}", tx(("Compte perso", "Personal account"), fr))).size(11.0).color(theme.text_muted));
                    ui.label(egui::RichText::new(money(l.account, fr)).size(15.0).color(if l.account < 0.0 { theme.ansi[1] } else { theme.text }).strong());
                });
            })
            .response
            .on_hover_text(format!("{} {:.0} · {} {:.0} · {}", tx(("Santé", "Health"), fr), l.health, tx(("stress", "stress"), fr), l.stress, tx(("onglet Vie privée", "Private life tab"), fr)));
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
                    role_use_label(ui, c, e.job().special, fr, theme);
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
                        ui.label(egui::RichText::new(tx(("Un empire mondial. Il ne reste plus qu'à tenir le conseil, racheter les concurrents… ou revendre.", "A world empire. All that's left is to keep the board happy, buy the rivals… or sell."), fr)).size(13.0).color(theme.accent));
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
                    role_use_label(ui, c, e.job().special, fr, theme);
                    if let Some(tr) = e.trait_id.as_deref().and_then(corp::trait_def) {
                        ui.label(egui::RichText::new(format!("« {} »", tx(tr.name, fr))).size(12.0).color(theme.ansi[5])).on_hover_text(tx(tr.about, fr));
                    }
                    star_badge(ui, &e, fr, theme);
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
                    // How many the company has already (hiring one more: worth it?).
                    if let Some((have, useful, about)) = role_use(c, e.job().special) {
                        let (text, color) = if have >= useful { (format!("{} {have}/{useful}", tx(("tu en as déjà", "you already have"), fr)), theme.ansi[3]) } else { (format!("{} {have}/{useful}", tx(("tu en as", "you have"), fr)), theme.ansi[2]) };
                        ui.label(egui::RichText::new(text).size(12.0).color(color)).on_hover_text(tx(about, fr));
                    }
                    if let Some(tr) = e.trait_id.as_deref().and_then(corp::trait_def) {
                        ui.label(egui::RichText::new(format!("« {} »", tx(tr.name, fr))).size(12.0).color(theme.ansi[5])).on_hover_text(tx(tr.about, fr));
                    }
                    star_badge(ui, e, fr, theme);
                });
                muted(ui, format!("{} · {} {}", skills_text(&e.skills.map(|x| x * e.level_mult()), fr), tx(("salaire", "salary"), fr), per_min(e.salary, fr)), theme);
                let fee = c.hire_fee(e);
                let cto_taken = e.job == "cto" && c.staff.iter().any(|x| x.job == "cto");
                if !c.allowed(e) {
                    ui.label(egui::RichText::new(tx(("Interdit par ton défi en cours.", "Ruled out by your current challenge."), fr)).size(12.0).color(theme.ansi[3]));
                }
                if button(ui, !full && c.cash >= fee && !cto_taken && c.allowed(e), format!("{} ({})", tx(("Embaucher", "Hire"), fr), money(fee, fr)), theme) {
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
        let (mut focus, mut price, mut pivot) = (None, None, None);
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
                // The roadmap: a focus, a price, a pivot.
                if p.version > 0 {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(tx(("Cap", "Focus"), fr)).size(12.0).color(theme.text_muted));
                        for (k, (name, about)) in corp::FOCUSES.iter().enumerate() {
                            let on = p.focus as usize == k;
                            let b = egui::Button::new(egui::RichText::new(tx(*name, fr)).size(12.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(10.0);
                            if ui.add(b).on_hover_text(tx(*about, fr)).clicked() && !on {
                                focus = Some((p.id, k as u8));
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(tx(("Prix", "Price"), fr)).size(12.0).color(theme.text_muted));
                        for (k, (name, money_mult, users_mult)) in corp::PRICES.iter().enumerate() {
                            let level = k as i8 - 2;
                            let on = p.price == level;
                            let b = egui::Button::new(egui::RichText::new(tx(*name, fr)).size(12.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(10.0);
                            let tip = format!("{} ×{money_mult} · {} ×{users_mult}", tx(("revenu par utilisateur", "revenue per user"), fr), tx(("utilisateurs", "users"), fr));
                            if ui.add(b).on_hover_text(tip).clicked() && !on {
                                price = Some((p.id, level));
                            }
                        }
                    });
                    egui::CollapsingHeader::new(egui::RichText::new(tx(("Pivoter vers un autre produit…", "Pivot to another product…"), fr)).size(12.0)).id_salt(("pivot", p.id)).show(ui, |ui| {
                        muted(ui, tx(("La moitié du prix de lancement du nouveau produit : il garde la moitié de ses versions et un tiers de ses utilisateurs.", "Half the new product's launch price: it keeps half its versions and a third of its users."), fr), theme);
                        ui.horizontal_wrapped(|ui| {
                            for (k, kind) in corp::PRODUCTS.iter().enumerate() {
                                if k == p.kind as usize || c.products.iter().any(|q| q.kind as usize == k) {
                                    continue;
                                }
                                if ui.add_enabled(c.can_pivot(p.id, k), egui::Button::new(egui::RichText::new(format!("{} ({})", tx(kind.name, fr), money(c.pivot_cost(k), fr))).size(12.0)).corner_radius(6.0)).clicked() {
                                    pivot = Some((p.id, k));
                                }
                            }
                        });
                    });
                    let pressure = c.rival_pressure(p.kind);
                    if pressure > 0.0 {
                        ui.label(egui::RichText::new(format!("{} −{:.0} % {}", tx(("Concurrence :", "Competition:"), fr), pressure * 100.0, tx(("d'utilisateurs (onglet Groupe)", "users (Group tab)"), fr))).size(12.0).color(theme.ansi[3]));
                    }
                }
            });
            ui.add_space(6.0);
        }
        if let Some(id) = close {
            c.close_product(id);
        }
        if let Some((id, f)) = focus {
            c.set_focus(id, f);
        }
        if let Some((id, level)) = price {
            c.set_price(id, level);
        }
        if let Some((id, k)) = pivot {
            c.pivot(id, k);
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
                if c.challenge_is("contracts") {
                    missing.push(tx(("pas de produit dans ton défi en cours", "no product in your current challenge"), fr).to_owned());
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

        // Past the tree: the optimisations, with no end.
        ui.add_space(14.0);
        heading(ui, tx(("Optimisations", "Optimisations"), fr), theme);
        if !c.techs_done() {
            muted(ui, tx(("Une fois toutes les technos trouvées, la recherche continue ici : des améliorations sans fin, chaque niveau un peu plus cher. Le ou la CTO les débloque aussi tout·e seul·e.", "Once every tech is found, research goes on here: endless improvements, each level a bit dearer. The CTO unlocks them alone too."), fr), theme);
            return;
        }
        muted(ui, tx(("Sans fin : chaque niveau coûte 45 % de plus que le précédent. Le ou la CTO les débloque tout·e seul·e, le moins cher d'abord.", "Endless: each level costs 45 % more than the one before. The CTO unlocks them alone, the cheapest first."), fr), theme);
        ui.add_space(6.0);
        let mut up = None;
        for u in corp::UPGRADES.iter() {
            let level = c.upgrade_level(u.id);
            let cost = c.upgrade_cost(u.id);
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(u.name, fr)).size(14.0).color(theme.text).strong());
                    if level > 0 {
                        ui.label(egui::RichText::new(format!("{} {level}", tx(("niveau", "level"), fr))).size(12.5).color(theme.ansi[2]));
                    }
                    muted(ui, tx(u.about, fr), theme);
                });
                ui.horizontal(|ui| {
                    if button(ui, c.can_upgrade(u.id), format!("{} ({:.0} pts)", tx(("Niveau suivant", "Next level"), fr), cost), theme) {
                        up = Some(u.id);
                    }
                    if c.research < cost {
                        bar(ui, c.research / cost, theme.accent, 120.0, 7.0, theme);
                    }
                });
            });
            ui.add_space(6.0);
        }
        if let Some(id) = up {
            c.learn_upgrade(id);
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
            if c.has_role(Special::Lead) && p.auto.tasks {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tx(("Priorité du ou de la chef·fe de projet", "The project manager's priority"), fr)).size(13.0).color(theme.text).strong());
                let labels = [tx(("Contrats", "Contracts"), fr).to_owned(), tx(("Équilibré", "Balanced"), fr).to_owned(), tx(("Produits", "Products"), fr).to_owned()];
                if let Some(v) = segmented(ui, p.auto.focus.min(2), &labels, theme) {
                    p.auto.focus = v;
                }
                muted(ui, tx(match p.auto.focus {
                    0 => ("Les contrats d'abord : une personne sur les produits, plus seulement quand ils ont trop de bugs.", "The contracts first: one person on the products, more only when they have too many bugs."),
                    1 => ("La moitié de l'équipe sur les produits, le reste sur les contrats.", "Half the team on the products, the rest on the contracts."),
                    _ => ("Les produits d'abord : tout le monde dessus, sauf la recherche et une personne sur les contrats.", "The products first: everyone on them, but research and one person on the contracts."),
                }, fr), theme);
            }
        }
        c.set_policy(p);

        ui.add_space(14.0);
        heading(ui, tx(("Revendre la boîte", "Sell the company"), fr), theme);
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            muted(ui, tx(("Tu repars du garage, mais chaque étoile donne pour toujours +5 % de productivité, +3 % sur ce que tu encaisses et 1 000 € de plus au départ, et se place dans un talent (ci-dessous). Tu touches aussi 10 % de la vente sur ton compte perso, et ta vie privée te suit.", "You start again in the garage, but each star gives forever +5 % productivity, +3 % on what you earn and €1,000 more to start with, and goes into a talent (below). You also get 10 % of the sale on your personal account, and your private life comes along."), fr), theme);
            muted(ui, format!("{} ★ {} · {} {} · {} {}", tx(("Étoiles :", "Stars:"), fr), c.legacy.stars, tx(("boîtes revendues :", "companies sold:"), fr), c.legacy.sales, tx(("record :", "record:"), fr), money(c.legacy.best, fr)), theme);
            if let Some(ch) = c.challenge.as_deref().and_then(corp::challenge) {
                let left = if ch.id == "speed" { format!(" · {} {}", tx(("reste", "left"), fr), corp::duration(corp::SPEED_LIMIT - c.run_time, fr)) } else { String::new() };
                ui.label(egui::RichText::new(format!("{} « {} » : {} (+{} ★){left}", tx(("Défi en cours", "Current challenge"), fr), tx(ch.name, fr), tx(ch.about, fr), ch.reward)).size(12.5).color(theme.ansi[5]));
            }
            let stars = c.sell_stars();
            if stars == 0 {
                ui.label(egui::RichText::new(format!("{} {}.", tx(("Possible à partir de l'étape", "Possible from the stage"), fr), tx(corp::STAGES[corp::SELL_STAGE as usize].name, fr))).size(12.5).color(theme.ansi[3]));
            } else if self.tycoon.sell_confirm {
                ui.label(egui::RichText::new(tx(("Sûr ? L'équipe, les locaux, les produits et la trésorerie seront perdus.", "Sure? The team, premises, products and cash will be gone."), fr)).size(12.5).color(theme.ansi[1]));
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tx(("Un défi pour la prochaine boîte ? (des étoiles en plus si tu le réussis)", "A challenge for the next company? (more stars if you win it)"), fr)).size(13.0).color(theme.text).strong());
                ui.horizontal_wrapped(|ui| {
                    let none = self.tycoon.challenge.is_none();
                    if ui.add(egui::Button::new(egui::RichText::new(tx(("Aucun", "None"), fr)).size(12.0).color(if none { theme.bg } else { theme.text })).fill(if none { theme.accent } else { theme.tab_hover }).corner_radius(10.0)).clicked() {
                        self.tycoon.challenge = None;
                    }
                    for ch in corp::CHALLENGES.iter() {
                        let on = self.tycoon.challenge == Some(ch.id);
                        let won = c.legacy.challenges.iter().any(|x| x == ch.id);
                        let label = format!("{}{} (+{} ★)", if won { "✓ " } else { "" }, tx(ch.name, fr), ch.reward);
                        if ui.add(egui::Button::new(egui::RichText::new(label).size(12.0).color(if on { theme.bg } else { theme.text })).fill(if on { theme.accent } else { theme.tab_hover }).corner_radius(10.0)).on_hover_text(tx(ch.about, fr)).clicked() {
                            self.tycoon.challenge = Some(ch.id);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if button(ui, true, format!("{} (+{} ★, +{} {})", tx(("Vendre", "Sell"), fr), stars, money(c.sale_share(), fr), tx(("pour toi", "for you"), fr)), theme) {
                        c.sell_with(now, self.tycoon.challenge.take());
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

        // The talents.
        ui.add_space(14.0);
        heading(ui, &format!("{} ({} ★ {})", tx(("Talents", "Talents"), fr), c.talent_points(), tx(("à placer", "to spend"), fr)), theme);
        muted(ui, tx(("Chaque étoile se place une fois dans un talent, gardé pour toujours (le bonus de chaque étoile reste). Tu peux tout redistribuer quand tu veux.", "Each star goes once into a talent, kept forever (each star's bonus stays). You can share them out again whenever you like."), fr), theme);
        ui.add_space(6.0);
        let mut take = None;
        card(ui, theme, |ui| {
            ui.set_width(ui.available_width());
            for t in corp::TALENTS.iter() {
                let level = c.legacy.talent(t.id);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tx(t.name, fr)).size(13.5).color(if level > 0 { theme.ansi[2] } else { theme.text }).strong());
                    ui.label(egui::RichText::new(format!("{level}/{}", t.max)).size(12.5).color(theme.accent));
                    muted(ui, tx(t.about, fr), theme);
                    if level < t.max && ui.add_enabled(c.can_take_talent(t.id), egui::Button::new(egui::RichText::new(format!("+1 ({} ★)", t.cost)).size(12.0)).corner_radius(6.0)).clicked() {
                        take = Some(t.id);
                    }
                });
            }
            if !c.legacy.talents.is_empty() && small(ui, tx(("Tout redistribuer", "Share them out again"), fr)) {
                c.reset_talents();
            }
        });
        if let Some(id) = take {
            c.take_talent(id);
        }

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
        // One window at a time: the last choice's outcome first.
        if self.tycoon.outcome.is_some() {
            return;
        }
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
            // What moved, measured around the choice.
            let morale = |c: &Corp| if c.staff.is_empty() { 0.0 } else { c.staff.iter().map(|e| e.morale).sum::<f64>() / c.staff.len() as f64 };
            let users = |c: &Corp| c.products.iter().map(|p| p.users).sum::<f64>();
            let work = |c: &Corp| c.contracts.iter().map(|k| k.need.iter().sum::<f64>()).sum::<f64>();
            let pay = |c: &Corp| c.contracts.iter().map(|k| k.pay).sum::<f64>();
            let couple = |c: &Corp| c.life.partner.as_ref().map_or(0.0, |p| p.happiness);
            let life = |c: &Corp| (c.life.account + c.life.savings + c.life.crypto, couple(c), c.life.stress, c.life.health);
            let before = (c.cash, c.rep, morale(c), c.staff.len() as f64, users(c), c.debt, work(c), pay(c), c.boosts.len());
            let life_before = life(c);
            let label = tx(def.choices[k].label, fr).to_owned();
            if let Some(outcome) = c.choose(k) {
                let after = (c.cash, c.rep, morale(c), c.staff.len() as f64, users(c), c.debt, work(c), pay(c), c.boosts.len());
                let life_after = life(c);
                let text = def.choices[k].outcomes.get(outcome).map_or(String::new(), |o| tx(*o, fr).replace("{who}", &who));
                let mut changes: Vec<(String, f64, bool)> = Vec::new();
                // (what, by how much, the more the better)
                let mut add = |name: corp::Tx, d: f64, eps: f64, good: bool| {
                    if d.abs() >= eps {
                        changes.push((tx(name, fr).to_owned(), d, good));
                    }
                };
                add(("Trésorerie", "Cash"), after.0 - before.0, 1.0, true);
                add(("Réputation", "Reputation"), after.1 - before.1, 0.05, true);
                add(("Moral moyen", "Average morale"), after.2 - before.2, 0.1, true);
                add(("Effectif", "Headcount"), after.3 - before.3, 0.5, true);
                add(("Utilisateurs", "Users"), after.4 - before.4, 1.0, true);
                add(("Dette technique", "Technical debt"), after.5 - before.5, 0.05, false);
                add(("Travail des contrats", "Contracts' work"), after.6 - before.6, 1.0, false);
                add(("Prix des contrats", "Contracts' price"), after.7 - before.7, 1.0, true);
                add(("Compte perso", "Personal account"), life_after.0 - life_before.0, 1.0, true);
                add(("Bonheur du couple", "The couple's happiness"), life_after.1 - life_before.1, 0.1, true);
                add(("Ton stress", "Your stress"), life_after.2 - life_before.2, 0.1, false);
                add(("Ta santé", "Your health"), life_after.3 - life_before.3, 0.1, true);
                if after.8 > before.8 {
                    changes.push((tx(("Un effet pour un moment (voir le Bureau)", "An effect for a while (see the Office)"), fr).to_owned(), 0.0, true));
                }
                self.tycoon.outcome = Some(Outcome { title: format!("{} — {label}", tx(def.title, fr)), text, changes, notice: false });
            }
        }
    }

    /// The other players' companies, as floor last got them.
    fn tycoon_players(&mut self, ui: &mut Ui, c: &Corp, fr: bool, theme: &Theme) {
        heading(ui, tx(("Les autres boîtes", "The other companies"), fr), theme);
        muted(ui, tx(("Telles que chacun·e les a laissées la dernière fois que sa page Ronnie Corp était ouverte. Le rang : la meilleure valorisation jamais atteinte (reventes comprises).", "As each one left them the last time their Ronnie Corp page was open. The rank: the best valuation ever reached (sales included)."), fr), theme);
        ui.add_space(8.0);
        let pseudo = crate::floor::pseudo(self.config.settings.floor.as_ref()).map(str::to_owned);
        let board: Vec<crate::floor::GameEntry> = self.tycoon.floor.board().map(<[_]>::to_vec).unwrap_or_default();
        if pseudo.is_none() {
            muted(ui, self.t().game_need_pseudo, theme);
            return;
        }
        if self.tycoon.floor.board().is_none() {
            muted(ui, "…", theme);
            return;
        }
        if board.is_empty() {
            muted(ui, self.t().game_board_empty, theme);
            return;
        }
        let now_ms = unix_now() * 1000;
        for e in &board {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                let color = if e.me { theme.accent } else { theme.text };
                let name = if e.me { pseudo.clone().unwrap_or_default() } else { e.name.clone().unwrap_or_else(|| self.t().game_anonymous.to_owned()) };
                let stars = e.details.get("stars").and_then(|v| v.as_u64()).unwrap_or(0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("{}.", e.rank)).size(13.0).color(theme.text_muted));
                    ui.label(egui::RichText::new(name).size(14.0).strong().color(color));
                    if stars > 0 {
                        ui.label(egui::RichText::new(format!("★ {stars}")).size(12.5).color(theme.ansi[3]));
                    }
                    muted(ui, format!("{} {}", tx(("record", "record"), fr), money(e.score as f64 * 1_000.0, fr)), theme);
                    if let Some(at) = e.at.filter(|_| !e.me) {
                        let ago = ((now_ms - at).max(0) / 1000) as f64;
                        muted(ui, format!("· {} {}", tx(("vue il y a", "seen"), fr), if fr { corp::duration(ago, fr) } else { format!("{} ago", corp::duration(ago, fr)) }), theme);
                    }
                });
                // Mine from the game itself (always fresh); the others' from floor.
                let now = e.details.get("now");
                let figures: Option<(f64, u8, u64, u64, f64)> = if e.me {
                    Some((c.valuation(), c.stage, c.staff.len() as u64, c.products.len() as u64, (c.income - c.spend) * 60.0))
                } else {
                    now.map(|n| {
                        let f = |k: &str| n.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
                        (f("valuation") * 1_000.0, f("stage") as u8, f("staff") as u64, f("products") as u64, f("net"))
                    })
                };
                match figures {
                    Some((valuation, stage, staff, products, net)) => {
                        ui.horizontal_wrapped(|ui| {
                            let stat = |ui: &mut Ui, label: &str, value: String, color: Color32| {
                                ui.label(egui::RichText::new(label).size(12.0).color(theme.text_muted));
                                ui.label(egui::RichText::new(value).size(13.0).strong().color(color));
                                ui.add_space(10.0);
                            };
                            stat(ui, tx(("Étape", "Stage"), fr), tx(corp::STAGES[(stage as usize).min(corp::STAGES.len() - 1)].name, fr).to_owned(), theme.text);
                            stat(ui, tx(("Valorisation", "Valuation"), fr), money(valuation, fr), theme.accent);
                            stat(ui, tx(("Effectif", "Headcount"), fr), staff.to_string(), theme.text);
                            stat(ui, tx(("Produits", "Products"), fr), products.to_string(), theme.text);
                            stat(ui, tx(("Solde", "Net"), fr), format!("{}{}/min", if net >= 0.0 { "+" } else { "−" }, money(net.abs(), fr)), if net < 0.0 { theme.ansi[1] } else { theme.ansi[2] });
                        });
                    }
                    None => muted(ui, tx(("Pas encore de nouvelles de sa boîte (une version de Ronnie plus ancienne, ou pas rejoué depuis).", "No news of their company yet (an older Ronnie, or not played since)."), fr), theme),
                }
            });
            ui.add_space(6.0);
        }
    }

    /// What Ronnie Corp unlocks, and how far the player is from each.
    fn tycoon_rewards(&mut self, ui: &mut Ui, c: &Corp, fr: bool, theme: &Theme) {
        let t = self.t();
        let items = self.unlocks(super::four::HomeGame::Tycoon);
        let done = items.iter().filter(|u| u.done).count();
        heading(ui, &format!("{} ({done}/{})", tx(("Récompenses", "Rewards"), fr), items.len()), theme);
        muted(ui, format!("{} {}", tx(("Elles servent dans tout Ronnie, pas seulement ici.", "They're for all of Ronnie, not only here."), fr), t.corp_reward_where), theme);
        ui.add_space(8.0);
        // How far the best valuation is from each stage that unlocks something.
        let best = c.legacy.best.max(c.valuation());
        let toward = |k: u8| {
            let need = corp::STAGES[k as usize].valuation;
            (best / need).clamp(0.0, 1.0)
        };
        let stages = [corp::BILLS_STAGE, corp::WALL_STREET_STAGE, corp::UNICORN_STAGE, corp::LISTED_STAGE];
        for (k, u) in items.iter().enumerate() {
            card(ui, theme, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(if u.done { "✓" } else { "🔒" }).size(14.0).color(if u.done { theme.ansi[2] } else { theme.text_muted }));
                    ui.label(egui::RichText::new(&u.what).size(14.0).strong().color(if u.done { theme.ansi[2] } else { theme.text }));
                });
                if u.done {
                    muted(ui, if u.gives.is_empty() { t.unlock_done.to_owned() } else { u.gives.clone() }, theme);
                } else {
                    ui.label(egui::RichText::new(&u.how).size(12.5).color(theme.ansi[3]));
                    // The three first ones go by a stage, the frame by the listing (or a sale).
                    let stage = match k {
                        0..=2 => Some(stages[k]),
                        3 => Some(corp::LISTED_STAGE),
                        _ => None,
                    };
                    if let Some(st) = stage {
                        ui.horizontal(|ui| {
                            bar(ui, toward(st), theme.accent, 220.0, 8.0, theme);
                            muted(ui, format!("{} / {}", money(best, fr), money(corp::STAGES[st as usize].valuation, fr)), theme);
                        });
                    }
                }
            });
            ui.add_space(6.0);
        }
        ui.add_space(8.0);
        heading(ui, tx(("Et en revendant la boîte", "And by selling the company"), fr), theme);
        muted(ui, format!("{} ★ {} · {}", tx(("Étoiles :", "Stars:"), fr), c.legacy.stars, tx(("chacune donne pour toujours +5 % de productivité, +3 % sur ce que tu encaisses et 1 000 € de plus au départ, et se place dans un talent (onglet Direction). Les défis de revente en rapportent d'autres.", "each one gives forever +5 % productivity, +3 % on what you earn and €1,000 more to start with, and goes into a talent (Management tab). The sale challenges bring more."), fr)), theme);
    }

    /// What the last event's choice made, until closed.
    fn tycoon_outcome(&mut self, ctx: &egui::Context, fr: bool, theme: &Theme) {
        let Some(o) = &self.tycoon.outcome else { return };
        let mut close = false;
        let window = if o.notice { o.title.clone() } else { tx(("Ce que ton choix a donné", "What your choice did"), fr).to_owned() };
        egui::Window::new(window).id(egui::Id::new("tycoon-outcome")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).frame(egui::Frame::new().fill(theme.chrome_bg).stroke(Stroke::new(1.5, theme.accent)).corner_radius(12.0).inner_margin(18.0)).show(ctx, |ui| {
            ui.set_max_width(440.0);
            if !o.notice {
                muted(ui, &o.title, theme);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new(&o.text).size(14.0).color(theme.text));
            ui.add_space(10.0);
            if o.changes.is_empty() && !o.notice {
                muted(ui, tx(("Rien d'autre n'a bougé.", "Nothing else moved."), fr), theme);
            }
            for (name, d, good) in &o.changes {
                let (value, ok) = if *d == 0.0 {
                    (String::new(), *good)
                } else if name == tx(("Trésorerie", "Cash"), fr) || name == tx(("Prix des contrats", "Contracts' price"), fr) || name == tx(("Compte perso", "Personal account"), fr) {
                    (format!("{}{}", if *d > 0.0 { "+" } else { "−" }, money(d.abs(), fr)), (*d > 0.0) == *good)
                } else {
                    let n = if d.abs() >= 10.0 { format!("{:.0}", d.abs()) } else { format!("{:.1}", d.abs()) };
                    (format!("{}{n}", if *d > 0.0 { "+" } else { "−" }), (*d > 0.0) == *good)
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(name).size(13.0).color(theme.text));
                    if !value.is_empty() {
                        ui.label(egui::RichText::new(value).size(13.0).strong().color(if ok { theme.ansi[2] } else { theme.ansi[1] }));
                    }
                });
            }
            ui.add_space(10.0);
            if button(ui, true, "OK", theme) {
                close = true;
            }
        });
        if close {
            self.tycoon.outcome = None;
        }
    }

    /// The tutorial: a card at the bottom of the page, what it's about blinking; the steps to play go
    /// on by themselves once done. And its button, to see it again.
    fn tycoon_tutorial(&mut self, ctx: &egui::Context, page: Rect, c: &mut Corp, fr: bool, theme: &Theme) {
        let help = Rect::from_min_size(Pos2::new(page.max.x - 46.0, page.min.y + 14.0), Vec2::new(30.0, 30.0));
        let replay = egui::Area::new(egui::Id::new("tycoon-help")).fixed_pos(help.min).order(egui::Order::Foreground).show(ctx, |ui| {
            // Drawn by hand: a button wider than its text puts the text on the left.
            let (rect, resp) = ui.allocate_exact_size(help.size(), Sense::click());
            let visuals = ui.style().interact(&resp);
            ui.painter().rect_filled(rect, rect.height() / 2.0, visuals.weak_bg_fill);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "?", FontId::proportional(15.0), visuals.text_color());
            resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tx(("Revoir le tutoriel", "See the tutorial again"), fr)).clicked()
        });
        if replay.inner {
            c.tutorial = 0;
            self.tycoon.replay = true;
        }
        if c.tutorial as usize >= STEPS.len() {
            self.tycoon.replay = false;
            self.tycoon_tours(ctx, page, c, fr, theme);
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
        let k = c.tutorial as usize;
        let last = k + 1 == STEPS.len();
        let next_label = if step.done.is_none() || self.tycoon.replay {
            Some(if last { tx(("Terminer", "Finish"), fr) } else if k == 0 { tx(("C'est parti !", "Let's go!"), fr) } else { tx(("Suivant", "Next"), fr) })
        } else {
            None
        };
        let head = format!("{} · {}/{}", tx(("Tutoriel", "Tutorial"), fr), k + 1, STEPS.len());
        let skip_label = (!last).then(|| tx(("Passer le tutoriel", "Skip the tutorial"), fr));
        let (next, skip) = self.tour_card(ctx, page, &head, step, next_label, skip_label, fr, theme);
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

    /// Past the tutorial: the tour of what's new (once), and the one-off tips; never over a window.
    fn tycoon_tours(&mut self, ctx: &egui::Context, page: Rect, c: &mut Corp, fr: bool, theme: &Theme) {
        if self.tycoon.outcome.is_some() || c.event.is_some() || self.tycoon.away.is_some() {
            return;
        }
        if let Some(step) = WHATS_NEW.get(c.whats_new as usize) {
            let k = c.whats_new as usize;
            let last = k + 1 == WHATS_NEW.len();
            let head = format!("{} · {}/{}", tx(("Quoi de neuf", "What's new"), fr), k + 1, WHATS_NEW.len());
            let next_label = if last { tx(("Terminer", "Finish"), fr) } else { tx(("Suivant", "Next"), fr) };
            let skip_label = (!last).then(|| tx(("Passer", "Skip"), fr));
            let (next, skip) = self.tour_card(ctx, page, &head, step, Some(next_label), skip_label, fr, theme);
            if skip || (next && last) {
                c.whats_new = corp::TUTORIAL_DONE;
            } else if next {
                c.whats_new += 1;
            }
            return;
        }
        if !c.rivals.is_empty() && c.tips & corp::TIP_RIVAL == 0 {
            let head = tx(("Astuce", "Tip"), fr).to_owned();
            let (next, _) = self.tour_card(ctx, page, &head, &RIVAL_TIP, Some(tx(("Compris", "Got it"), fr)), None, fr, theme);
            if next {
                c.tips |= corp::TIP_RIVAL;
            }
        }
    }

    /// A step of a tour: what it's about blinking, a card at the bottom of the page. Whether « next »
    /// (`next_label`; None: done by playing) or « skip » was clicked.
    #[allow(clippy::too_many_arguments)]
    fn tour_card(&self, ctx: &egui::Context, page: Rect, head: &str, step: &Step, next_label: Option<&str>, skip_label: Option<&str>, fr: bool, theme: &Theme) -> (bool, bool) {
        let now = ctx.input(|i| i.time);
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
                ui.label(egui::RichText::new(head).size(11.5).color(theme.ansi[3]));
                ui.label(egui::RichText::new(tx(step.title, fr)).size(15.0).color(theme.text).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tx(step.text, fr)).size(13.0).color(theme.text));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    match next_label {
                        Some(label) => next = button(ui, true, label, theme),
                        None => {
                            ui.label(egui::RichText::new(tx(("À toi de jouer…", "Your turn…"), fr)).size(12.5).color(theme.text_muted).italics());
                        }
                    }
                    if let Some(label) = skip_label {
                        ui.add_space(12.0);
                        skip = small(ui, label);
                    }
                });
            });
        });
        (next, skip)
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
