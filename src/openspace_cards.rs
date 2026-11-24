//! Open Space, the cards and the missions.
//!
//! The cards are for the raids. A troop card makes one troop stronger (hit points and damage), a few
//! make the whole army stronger, and the action cards are played during a raid, once each: a hotfix
//! hitting an area, a stand-up healing, the code frozen… The action cards taken on a raid are the
//! deck's (2, more with the training room). A copy found again levels a card up (5 levels).
//! They come from the raids won, from the missions and from the internal mail (one every 20 hours).
//!
//! The missions: one after the other, from the first days to the boss's office at level 10, each
//! showing what to do next and giving something once done.
//!
//! Plain rules on the `OpenSpace` (see `crate::openspace`); drawn in `app::openspace_cards`.

use crate::openspace::{Kind, OpenSpace, Res, Spot, Tx, HQ_MAX};
use crate::openspace_battle::Troop;

/// A card's rarity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

pub const RARITIES: [Rarity; 5] = [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::Epic, Rarity::Legendary];

impl Rarity {
    pub fn name(self) -> Tx {
        match self {
            Rarity::Common => ("Commune", "Common"),
            Rarity::Uncommon => ("Peu commune", "Uncommon"),
            Rarity::Rare => ("Rare", "Rare"),
            Rarity::Epic => ("Épique", "Epic"),
            Rarity::Legendary => ("Légendaire", "Legendary"),
        }
    }

    /// How often it comes out of a draw, out of 100.
    fn weight(self) -> u64 {
        match self {
            Rarity::Common => 50,
            Rarity::Uncommon => 28,
            Rarity::Rare => 14,
            Rarity::Epic => 6,
            Rarity::Legendary => 2,
        }
    }
}

/// A card played during a raid, once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Interns dropped where it's played, even among the buildings.
    Reinforce,
    /// Hits the buildings of an area.
    Hotfix,
    /// Heals the troops of an area.
    Standup,
    /// The troops of an area faster and harder hitting for a while.
    Rush,
    /// The defenses of an area stop shooting for a while.
    Freeze,
    /// A huge blast.
    Friday,
}

impl Action {
    /// Its reach (tiles) where it's played.
    pub fn radius(self) -> f32 {
        match self {
            Action::Reinforce => 1.0,
            Action::Hotfix => 2.5,
            Action::Standup => 3.5,
            Action::Rush => 3.5,
            Action::Freeze => 3.5,
            Action::Friday => 4.5,
        }
    }

    /// What it does at `level`: a number the battle uses (damage, healing, seconds, interns).
    pub fn power(self, level: u8) -> f32 {
        let m = LEVEL_MULT[(level.clamp(1, 5) - 1) as usize] as f32;
        match self {
            Action::Reinforce => (4.0 + 2.0 * (level.max(1) - 1) as f32).round(),
            Action::Hotfix => 260.0 * m,
            Action::Standup => 220.0 * m,
            Action::Rush => 6.0 + 2.0 * (level.max(1) - 1) as f32,
            Action::Freeze => 4.0 + 1.0 * (level.max(1) - 1) as f32,
            Action::Friday => 900.0 * m,
        }
    }

    /// What it does, in words, at `level`.
    pub fn text(self, level: u8, fr: bool) -> String {
        let p = self.power(level);
        match self {
            Action::Reinforce => if fr { format!("Parachute {p:.0} stagiaires où tu veux, même au milieu des bâtiments.") } else { format!("Drops {p:.0} interns wherever you want, even among the buildings.") },
            Action::Hotfix => if fr { format!("Frappe les bâtiments d'une zone : {p:.0} dégâts.") } else { format!("Hits the buildings of an area: {p:.0} damage.") },
            Action::Standup => if fr { format!("Soigne les troupes d'une zone : {p:.0} PV.") } else { format!("Heals the troops of an area: {p:.0} HP.") },
            Action::Rush => if fr { format!("Les troupes d'une zone vont 2× plus vite et frappent +60 % pendant {p:.0} s.") } else { format!("The troops of an area go 2× faster and hit +60 % for {p:.0} s.") },
            Action::Freeze => if fr { format!("Les défenses d'une zone ne tirent plus pendant {p:.0} s.") } else { format!("The defenses of an area stop shooting for {p:.0} s.") },
            Action::Friday => if fr { format!("Une énorme explosion : {p:.0} dégâts sur une grande zone.") } else { format!("A huge blast: {p:.0} damage over a wide area.") },
        }
    }
}

/// What a card does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// One troop: more hit points and damage.
    Troop(Troop),
    /// Every troop: more hit points.
    ArmyHp,
    /// Every troop: more damage.
    ArmyDps,
    /// More brought back from the raids.
    Loot,
    /// Played during a raid.
    Action(Action),
}

pub struct Card {
    pub name: Tx,
    /// A line of flavour.
    pub quote: Tx,
    pub rarity: Rarity,
    pub effect: Effect,
    /// A passive card's bonus at level 1 (a share: 0.1 is 10 %).
    pub base: f64,
}

const fn card(name: Tx, quote: Tx, rarity: Rarity, effect: Effect, base: f64) -> Card {
    Card { name, quote, rarity, effect, base }
}

use Rarity::*;

/// Every card; its number is its place here (kept: only added to at the end).
pub const CARDS: [Card; 18] = [
    card(("Le badge d'accès", "The access badge"), ("Il ouvre même la salle serveur.", "It even opens the server room."), Common, Effect::Troop(Troop::Intern), 0.10),
    card(("Le clavier mécanique", "The mechanical keyboard"), ("Tout l'étage l'entend.", "The whole floor hears it."), Common, Effect::Troop(Troop::Dev), 0.10),
    card(("Le cahier de recette", "The test book"), ("327 cas, tous « à refaire ».", "327 cases, all « to redo »."), Common, Effect::Troop(Troop::Tester), 0.10),
    card(("Le terminal noir", "The black terminal"), ("sudo fais-le.", "sudo do it."), Common, Effect::Troop(Troop::Devops), 0.10),
    card(("La cravate", "The tie"), ("Portée une fois, au salon.", "Worn once, at the trade fair."), Common, Effect::Troop(Troop::Sales), 0.10),
    card(("Le tableau croisé", "The pivot table"), ("Personne ne sait d'où vient ce chiffre.", "Nobody knows where that number comes from."), Uncommon, Effect::Troop(Troop::Data), 0.12),
    card(("La palette de couleurs", "The colour palette"), ("« Tu peux le faire plus pop ? »", "« Can you make it pop more? »"), Uncommon, Effect::Troop(Troop::Designer), 0.12),
    card(("La télécommande", "The remote control"), ("Livraison par la fenêtre.", "Delivered through the window."), Uncommon, Effect::Troop(Troop::Drone), 0.12),
    card(("La mallette", "The briefcase"), ("1 500 € la journée, slides comprises.", "1,500 a day, slides included."), Rare, Effect::Troop(Troop::Consultant), 0.15),
    card(("La prime d'objectifs", "The target bonus"), ("Atteignable, paraît-il.", "Reachable, they say."), Uncommon, Effect::Loot, 0.08),
    card(("Le séminaire au vert", "The offsite retreat"), ("Accrobranche obligatoire.", "Tree-climbing mandatory."), Rare, Effect::ArmyHp, 0.06),
    card(("Le café de spécialité", "The specialty coffee"), ("Notes de fruits rouges, paraît-il.", "Notes of red berries, they say."), Epic, Effect::ArmyDps, 0.08),
    card(("Renfort de stagiaires", "Intern reinforcements"), ("Ils ont reçu leur badge ce matin.", "They got their badge this morning."), Uncommon, Effect::Action(Action::Reinforce), 0.0),
    card(("Hotfix", "Hotfix"), ("Testé en prod, comme d'habitude.", "Tested in prod, as usual."), Rare, Effect::Action(Action::Hotfix), 0.0),
    card(("Daily stand-up", "Daily stand-up"), ("« Rien de bloquant. »", "« Nothing blocking. »"), Rare, Effect::Action(Action::Standup), 0.0),
    card(("Café serré", "Double espresso"), ("Le troisième de la matinée.", "The third this morning."), Rare, Effect::Action(Action::Rush), 0.0),
    card(("Gel du code", "Code freeze"), ("Plus personne ne touche à rien.", "Nobody touches anything anymore."), Epic, Effect::Action(Action::Freeze), 0.0),
    card(("Mise en prod du vendredi", "Friday deploy"), ("17 h 58. Qu'est-ce qui pourrait mal tourner ?", "5:58 pm. What could go wrong?"), Legendary, Effect::Action(Action::Friday), 0.0),
];

/// The copies needed for each level (1 to 5).
pub const COPIES: [u32; 5] = [1, 3, 6, 10, 15];
/// The bonus, at each level, times the level 1 one.
const LEVEL_MULT: [f64; 5] = [1.0, 1.4, 1.8, 2.3, 3.0];
/// The internal mail: a free card this often (seconds).
pub const MAIL_EVERY: i64 = 20 * 3_600;

/// A card's level with `copies` (0: not found yet).
pub fn card_level(copies: u32) -> u8 {
    COPIES.iter().filter(|&&n| copies >= n).count() as u8
}

/// A passive card's bonus at `level` (a share).
pub fn card_bonus(n: usize, level: u8) -> f64 {
    if level == 0 {
        return 0.0;
    }
    CARDS[n].base * LEVEL_MULT[(level.min(5) - 1) as usize]
}

pub fn is_action(n: usize) -> bool {
    matches!(CARDS[n].effect, Effect::Action(_))
}

/// What a mission gives once done.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reward {
    pub tickets: u32,
    pub coffee: u64,
    pub budget: u64,
    /// A card, at least this rare.
    pub card: Option<Rarity>,
}

/// A mission: its title, what to do, where to look, and how far it is (have, need).
pub struct Mission {
    pub title: Tx,
    pub text: Tx,
    pub spot: Spot,
    progress: fn(&OpenSpace) -> (u64, u64),
    pub reward: Reward,
}

impl Mission {
    pub fn progress(&self, s: &OpenSpace) -> (u64, u64) {
        let (have, need) = (self.progress)(s);
        (have.min(need), need)
    }
}

const fn give(tickets: u32, coffee: u64, budget: u64, card: Option<Rarity>) -> Reward {
    Reward { tickets, coffee, budget, card }
}

fn built(s: &OpenSpace, k: Kind) -> u64 {
    s.buildings.iter().filter(|b| b.kind == k && b.level >= 1).count() as u64
}

fn stars(s: &OpenSpace) -> u64 {
    s.campaign.iter().map(|&n| n as u64).sum()
}

macro_rules! m {
    ($title:expr, $text:expr, $spot:expr, $prog:expr, $reward:expr) => {
        Mission { title: $title, text: $text, spot: $spot, progress: $prog, reward: $reward }
    };
}

/// The missions, in order.
pub const MISSIONS: [Mission; 44] = [
    m!(("Un deuxième alternant", "A second intern"), ("Deux chantiers à la fois, c'est deux fois plus vite. Construis un deuxième bureau des alternants.", "Two works at once is twice as fast. Build a second interns' desk."), Spot::Shop(Kind::Interns), |s| (built(s, Kind::Interns), 2), give(5, 0, 500, None)),
    m!(("La pause-café", "The coffee break"), ("Récolte 10 fois le café ou le budget de tes machines (clique sur les bulles).", "Collect your machines' coffee or budget 10 times (click the bubbles)."), Spot::Bubble, |s| (s.collects as u64, 10), give(0, 500, 500, None)),
    m!(("Mettre le budget à l'abri", "Keep the budget safe"), ("Un coffre-fort garde plus de budget. Construis-en un.", "A safe holds more budget. Build one."), Spot::Shop(Kind::Safe), |s| (built(s, Kind::Safe), 1), give(0, 0, 1_000, None)),
    m!(("Promotion !", "Promotion!"), ("Monte le bureau du boss au niveau 2 : tout ce qu'il débloque t'attend.", "Get the boss's office to level 2: all it unlocks is waiting."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 2), give(10, 0, 0, None)),
    m!(("Plus de café", "More coffee"), ("Construis une deuxième machine à café.", "Build a second coffee machine."), Spot::Shop(Kind::Coffee), |s| (built(s, Kind::Coffee), 2), give(0, 1_000, 0, None)),
    m!(("Plus de budget", "More budget"), ("Construis un deuxième serveur de prod.", "Build a second production server."), Spot::Shop(Kind::Server), |s| (built(s, Kind::Server), 2), give(0, 0, 1_500, None)),
    m!(("Les pare-feux", "Firewalls"), ("Entoure ton QG de pare-feux : pose-en 10.", "Surround your HQ with firewalls: put down 10."), Spot::Shop(Kind::Wall), |s| (built(s, Kind::Wall), 10), give(0, 0, 1_000, None)),
    m!(("Le linter", "The linter"), ("La tourelle linter tire de loin. Construis-en une.", "The linter turret shoots from afar. Build one."), Spot::Shop(Kind::Linter), |s| (built(s, Kind::Linter), 1), give(5, 0, 1_000, None)),
    m!(("Une petite armée", "A small army"), ("Forme 10 troupes dans la salle de recrutement.", "Train 10 troops in the recruiting room."), Spot::Army, |s| (s.trained as u64, 10), give(0, 1_500, 0, None)),
    m!(("Première victoire", "First victory"), ("Gagne un raid (au moins 1 étoile). Chaque victoire rapporte une carte !", "Win a raid (at least 1 star). Each victory gives a card!"), Spot::Attack, |s| (s.wins as u64, 1), give(5, 0, 0, Some(Rarity::Uncommon))),
    m!(("Tes cartes", "Your cards"), ("Ouvre « Cartes » et récupère le courrier interne : une carte gratuite toutes les 20 heures.", "Open « Cards » and get the internal mail: a free card every 20 hours."), Spot::Cards, |s| ((s.mail > 0) as u64, 1), give(5, 0, 0, None)),
    m!(("Le grand ménage", "Spring cleaning"), ("Dégage 5 obstacles (cartons, vieux PC…) : des tickets se cachent parfois dessous.", "Clear 5 obstacles (boxes, old PCs…): tickets sometimes hide under them."), Spot::None, |s| (s.cleared as u64, 5), give(0, 0, 1_000, None)),
    m!(("Les testeurs", "Testers"), ("Monte une salle de recrutement au niveau 3 : les testeurs cassent les pare-feux.", "Get a recruiting room to level 3: testers break firewalls."), Spot::Building(Kind::Recruit), |s| (s.top_level(Kind::Recruit) as u64, 3), give(0, 2_000, 0, None)),
    m!(("Cinq étoiles", "Five stars"), ("Gagne 5 étoiles dans la campagne.", "Earn 5 stars in the campaign."), Spot::Attack, |s| (stars(s), 5), give(10, 0, 0, Some(Rarity::Common))),
    m!(("Le QG au niveau 3", "HQ level 3"), ("Monte le bureau du boss au niveau 3.", "Get the boss's office to level 3."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 3), give(10, 0, 0, None)),
    m!(("La salle de réunion", "The meeting room"), ("Construis la salle de réunion : elle fait monter tes troupes de niveau.", "Build the meeting room: it levels your troops up."), Spot::Shop(Kind::Meeting), |s| (built(s, Kind::Meeting), 1), give(0, 2_000, 0, None)),
    m!(("Montée en compétences", "Upskilling"), ("Améliore une troupe dans la salle de réunion (bouton « Armée »).", "Upgrade a troop in the meeting room (« Army » button)."), Spot::Army, |s| (s.troop_levels.iter().map(|(_, l)| l.saturating_sub(1) as u64).sum(), 1), give(5, 0, 0, None)),
    m!(("L'alarme incendie", "The fire alarm"), ("L'alarme incendie frappe toute une zone. Construis-en une.", "The fire alarm hits a whole area. Build one."), Spot::Shop(Kind::Alarm), |s| (built(s, Kind::Alarm), 1), give(0, 0, 3_000, None)),
    m!(("Des pièges", "Traps"), ("Cache 2 bugs dans les passages entre tes pare-feux.", "Hide 2 bugs in the gaps between your firewalls."), Spot::Shop(Kind::Bug), |s| (built(s, Kind::Bug), 2), give(5, 0, 0, None)),
    m!(("Cinq victoires", "Five victories"), ("Gagne 5 raids.", "Win 5 raids."), Spot::Attack, |s| (s.wins as u64, 5), give(10, 0, 0, Some(Rarity::Rare))),
    m!(("Le QG au niveau 4", "HQ level 4"), ("Monte le bureau du boss au niveau 4.", "Get the boss's office to level 4."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 4), give(15, 0, 0, None)),
    m!(("Une muraille", "A great wall"), ("Pose 50 pare-feux.", "Put down 50 firewalls."), Spot::Shop(Kind::Wall), |s| (built(s, Kind::Wall), 50), give(0, 0, 10_000, None)),
    m!(("Les commerciaux", "Sales"), ("Monte une salle de recrutement au niveau 5 : les commerciaux encaissent les coups.", "Get a recruiting room to level 5: sales soak up the hits."), Spot::Building(Kind::Recruit), |s| (s.top_level(Kind::Recruit) as u64, 5), give(0, 10_000, 0, None)),
    m!(("Quinze étoiles", "Fifteen stars"), ("Gagne 15 étoiles dans la campagne.", "Earn 15 stars in the campaign."), Spot::Attack, |s| (stars(s), 15), give(15, 0, 0, Some(Rarity::Uncommon))),
    m!(("Le QG au niveau 5", "HQ level 5"), ("Monte le bureau du boss au niveau 5.", "Get the boss's office to level 5."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 5), give(20, 0, 0, None)),
    m!(("La salle de formation", "The training room"), ("Construis la salle de formation : une carte action de plus dans ton deck de raid.", "Build the training room: one more action card in your raid deck."), Spot::Shop(Kind::Training), |s| (built(s, Kind::Training), 1), give(10, 0, 0, Some(Rarity::Rare))),
    m!(("Un vrai deck", "A real deck"), ("Mets 3 cartes action dans ton deck de raid (« Cartes »).", "Put 3 action cards in your raid deck (« Cards »)."), Spot::Cards, |s| (s.deck.len() as u64, 3), give(10, 0, 0, None)),
    m!(("Cent recrues", "A hundred recruits"), ("Forme 100 troupes.", "Train 100 troops."), Spot::Army, |s| (s.trained as u64, 100), give(0, 30_000, 0, None)),
    m!(("Vingt-cinq étoiles", "Twenty-five stars"), ("Gagne 25 étoiles dans la campagne.", "Earn 25 stars in the campaign."), Spot::Attack, |s| (stars(s), 25), give(20, 0, 0, Some(Rarity::Rare))),
    m!(("Le QG au niveau 6", "HQ level 6"), ("Monte le bureau du boss au niveau 6.", "Get the boss's office to level 6."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 6), give(25, 0, 0, None)),
    m!(("Les data scientists", "Data scientists"), ("Monte une salle de recrutement au niveau 6 : leurs requêtes frappent toute une zone.", "Get a recruiting room to level 6: their queries hit a whole area."), Spot::Building(Kind::Recruit), |s| (s.top_level(Kind::Recruit) as u64, 6), give(0, 60_000, 0, None)),
    m!(("Vingt victoires", "Twenty victories"), ("Gagne 20 raids.", "Win 20 raids."), Spot::Attack, |s| (s.wins as u64, 20), give(20, 0, 0, Some(Rarity::Rare))),
    m!(("Le QG au niveau 7", "HQ level 7"), ("Monte le bureau du boss au niveau 7.", "Get the boss's office to level 7."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 7), give(30, 0, 0, None)),
    m!(("Quarante étoiles", "Forty stars"), ("Gagne 40 étoiles dans la campagne.", "Earn 40 stars in the campaign."), Spot::Attack, |s| (stars(s), 40), give(25, 0, 0, Some(Rarity::Epic))),
    m!(("Les designers", "Designers"), ("Monte une salle de recrutement au niveau 7 : les designers volent et soignent.", "Get a recruiting room to level 7: designers fly and heal."), Spot::Building(Kind::Recruit), |s| (s.top_level(Kind::Recruit) as u64, 7), give(0, 150_000, 0, None)),
    m!(("Le QG au niveau 8", "HQ level 8"), ("Monte le bureau du boss au niveau 8.", "Get the boss's office to level 8."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 8), give(35, 0, 0, None)),
    m!(("Le cluster", "The cluster"), ("Construis un cluster : des tirs très rapides, très loin.", "Build a cluster: very fast shots, very far."), Spot::Shop(Kind::Cluster), |s| (built(s, Kind::Cluster), 1), give(0, 0, 500_000, None)),
    m!(("Soixante étoiles", "Sixty stars"), ("Gagne 60 étoiles dans la campagne.", "Earn 60 stars in the campaign."), Spot::Attack, |s| (stars(s), 60), give(30, 0, 0, Some(Rarity::Epic))),
    m!(("Le QG au niveau 9", "HQ level 9"), ("Monte le bureau du boss au niveau 9.", "Get the boss's office to level 9."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, 9), give(40, 0, 0, None)),
    m!(("Le conseil d'administration", "The board of directors"), ("Construis le conseil d'administration : un rayon qui chauffe de plus en plus.", "Build the board of directors: a beam that grows stronger."), Spot::Shop(Kind::Board), |s| (built(s, Kind::Board), 1), give(0, 0, 1_000_000, None)),
    m!(("Quatre-vingt-dix étoiles", "Ninety stars"), ("Gagne 90 étoiles dans la campagne.", "Earn 90 stars in the campaign."), Spot::Attack, |s| (stars(s), 90), give(40, 0, 0, Some(Rarity::Epic))),
    m!(("Le sommet", "The top"), ("Monte le bureau du boss au niveau 10.", "Get the boss's office to level 10."), Spot::Building(Kind::Hq), |s| (s.hq_level() as u64, HQ_MAX as u64), give(50, 0, 0, Some(Rarity::Legendary))),
    m!(("Cent victoires", "A hundred victories"), ("Gagne 100 raids.", "Win 100 raids."), Spot::Attack, |s| (s.wins as u64, 100), give(50, 0, 0, Some(Rarity::Epic))),
    m!(("Maître du marché", "Market leader"), ("Gagne les 150 étoiles de la campagne.", "Earn all 150 stars of the campaign."), Spot::Attack, |s| (stars(s), 150), give(100, 0, 0, Some(Rarity::Legendary))),
];

impl OpenSpace {
    /// How many copies of card `n` were found.
    pub fn copies(&self, n: usize) -> u32 {
        self.cards.get(n).copied().unwrap_or(0)
    }

    pub fn card_level(&self, n: usize) -> u8 {
        card_level(self.copies(n))
    }

    fn passive(&self, want: impl Fn(Effect) -> bool) -> f64 {
        CARDS.iter().enumerate().filter(|(_, c)| want(c.effect)).map(|(n, _)| card_bonus(n, self.card_level(n))).sum()
    }

    /// What the cards add to troop `t`: hit points, damage (times).
    pub fn troop_boost(&self, t: Troop) -> (f32, f32) {
        let own = self.passive(|e| e == Effect::Troop(t));
        let hp = 1.0 + own + self.passive(|e| e == Effect::ArmyHp);
        let dps = 1.0 + own + self.passive(|e| e == Effect::ArmyDps);
        (hp as f32, dps as f32)
    }

    /// What the cards add to the raids' loot (times).
    pub fn loot_boost(&self) -> f64 {
        1.0 + self.passive(|e| e == Effect::Loot)
    }

    /// How many action cards the deck takes: 2, one more with the training room, another at its level 3.
    pub fn deck_size(&self) -> usize {
        let t = self.top_level(Kind::Training);
        2 + (t >= 1) as usize + (t >= 3) as usize
    }

    /// The deck's action cards taken on a raid: each, its level.
    pub fn raid_cards(&self) -> Vec<(Action, u8)> {
        self.deck
            .iter()
            .take(self.deck_size())
            .filter_map(|&n| match CARDS.get(n).map(|c| c.effect) {
                Some(Effect::Action(a)) if self.copies(n) > 0 => Some((a, self.card_level(n))),
                _ => None,
            })
            .collect()
    }

    /// Card `n` put in the deck or taken out: whether it's in it now.
    pub fn toggle_deck(&mut self, n: usize) -> bool {
        if let Some(i) = self.deck.iter().position(|&d| d == n) {
            self.deck.remove(i);
            self.actions += 1;
            return false;
        }
        if !is_action(n) || self.copies(n) == 0 || self.deck.len() >= self.deck_size() {
            return false;
        }
        self.deck.push(n);
        self.actions += 1;
        true
    }

    /// A card drawn at random (`floor`: at least that rare): its number, added to the collection.
    pub fn draw_card(&mut self, floor: Rarity) -> usize {
        let pool: Vec<Rarity> = RARITIES.into_iter().filter(|&r| r >= floor).collect();
        let total: u64 = pool.iter().map(|r| r.weight()).sum();
        let mut roll = self.rand_u64() % total;
        let mut rarity = floor;
        for r in pool {
            if roll < r.weight() {
                rarity = r;
                break;
            }
            roll -= r.weight();
        }
        let of: Vec<usize> = (0..CARDS.len()).filter(|&n| CARDS[n].rarity == rarity).collect();
        let n = of[self.rand_u64() as usize % of.len()];
        self.add_card(n);
        n
    }

    /// A card added; a new action card goes in the deck if there's room.
    pub fn add_card(&mut self, n: usize) {
        if self.cards.len() < CARDS.len() {
            self.cards.resize(CARDS.len(), 0);
        }
        self.cards[n] += 1;
        if self.cards[n] == 1 && is_action(n) && self.deck.len() < self.deck_size() {
            self.deck.push(n);
        }
        self.actions += 1;
    }

    /// When the next free card is in the mail (unix seconds; 0: now).
    pub fn mail_at(&self) -> i64 {
        if self.mail == 0 { 0 } else { self.mail + MAIL_EVERY }
    }

    /// The internal mail opened: its card, if it's time.
    pub fn open_mail(&mut self, now: i64) -> Option<usize> {
        if now < self.mail_at() {
            return None;
        }
        self.mail = now;
        Some(self.draw_card(Rarity::Common))
    }

    /// The mission under way (None: all done).
    pub fn mission(&self) -> Option<&'static Mission> {
        MISSIONS.get(self.missions as usize)
    }

    /// Whether the mission under way is done (its reward to take).
    pub fn mission_done(&self) -> bool {
        self.mission().is_some_and(|m| {
            let (have, need) = m.progress(self);
            have >= need
        })
    }

    /// The mission under way's reward taken: the card it gave, if any.
    pub fn claim_mission(&mut self) -> Option<Option<usize>> {
        if !self.mission_done() {
            return None;
        }
        let r = self.mission()?.reward;
        self.missions += 1;
        self.tickets += r.tickets;
        self.coffee = (self.coffee + r.coffee as f64).min(self.capacity(Res::Coffee).max(self.coffee));
        self.budget = (self.budget + r.budget as f64).min(self.capacity(Res::Budget).max(self.budget));
        self.actions += 1;
        Some(r.card.map(|floor| self.draw_card(floor)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space() -> OpenSpace {
        OpenSpace::new(1_000_000, 42)
    }

    #[test]
    fn every_rarity_and_troop_has_cards() {
        for r in RARITIES {
            assert!(CARDS.iter().any(|c| c.rarity == r), "{r:?}");
        }
        for t in crate::openspace_battle::TROOPS {
            assert!(CARDS.iter().any(|c| c.effect == Effect::Troop(t)), "{t:?}");
        }
    }

    #[test]
    fn copies_level_a_card_up() {
        assert_eq!(card_level(0), 0);
        assert_eq!(card_level(1), 1);
        assert_eq!(card_level(2), 1);
        assert_eq!(card_level(3), 2);
        assert_eq!(card_level(15), 5);
        assert_eq!(card_level(99), 5);
        assert!(card_bonus(0, 5) > card_bonus(0, 1));
        assert!(Action::Hotfix.power(5) > Action::Hotfix.power(1));
    }

    #[test]
    fn troop_cards_boost_their_troop_and_the_army() {
        let mut s = space();
        assert_eq!(s.troop_boost(Troop::Dev), (1.0, 1.0));
        s.add_card(1);
        let (hp, dps) = s.troop_boost(Troop::Dev);
        assert!((hp - 1.10).abs() < 1e-6 && (dps - 1.10).abs() < 1e-6);
        assert_eq!(s.troop_boost(Troop::Intern), (1.0, 1.0));
        s.add_card(10);
        assert!(s.troop_boost(Troop::Intern).0 > 1.0);
        assert_eq!(s.troop_boost(Troop::Intern).1, 1.0);
    }

    #[test]
    fn the_deck_takes_new_action_cards_while_there_is_room() {
        let mut s = space();
        assert_eq!(s.deck_size(), 2);
        s.add_card(0);
        assert!(s.deck.is_empty());
        for n in [12, 13, 14] {
            s.add_card(n);
        }
        assert_eq!(s.deck, vec![12, 13]);
        assert_eq!(s.raid_cards().len(), 2);
        assert!(!s.toggle_deck(14));
        assert!(!s.toggle_deck(12));
        assert!(s.toggle_deck(14));
        assert_eq!(s.deck, vec![13, 14]);
        assert!(!s.toggle_deck(0));
    }

    #[test]
    fn draws_follow_the_floor_and_find_everything() {
        let mut s = space();
        for _ in 0..50 {
            let n = s.draw_card(Rarity::Rare);
            assert!(CARDS[n].rarity >= Rarity::Rare);
        }
        let mut s = space();
        for _ in 0..5_000 {
            s.draw_card(Rarity::Common);
        }
        assert!((0..CARDS.len()).all(|n| s.copies(n) > 0));
        assert!(s.copies(0) > s.copies(17) * 5);
    }

    #[test]
    fn the_mail_comes_every_20_hours() {
        let mut s = space();
        let now = 2_000_000;
        assert!(s.open_mail(now).is_some());
        assert!(s.open_mail(now + 3_600).is_none());
        assert!(s.open_mail(now + MAIL_EVERY).is_some());
    }

    #[test]
    fn missions_go_one_after_the_other() {
        let mut s = space();
        assert_eq!(s.mission().unwrap().title.1, "A second intern");
        assert!(!s.mission_done());
        assert_eq!(s.claim_mission(), None);
        s.collects = 10;
        // The second one done doesn't count while the first isn't.
        assert!(!s.mission_done());
        let id = s.next_id;
        s.buildings.push(crate::openspace::Building { id, kind: Kind::Interns, x: 2, y: 2, level: 1, job: None, stored: 0.0 });
        s.buildings.push(crate::openspace::Building { id: id + 1, kind: Kind::Interns, x: 5, y: 2, level: 1, job: None, stored: 0.0 });
        let tickets = s.tickets;
        assert_eq!(s.claim_mission(), Some(None));
        assert_eq!(s.tickets, tickets + 5);
        assert_eq!(s.mission().unwrap().title.1, "The coffee break");
        assert!(s.mission_done());
        // Every mission can be reached: its need is above zero.
        for m in &MISSIONS {
            assert!(m.progress(&space()).1 > 0);
        }
    }
}
