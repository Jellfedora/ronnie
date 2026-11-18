//! What keeps the company going once it is big: the optimisations (research with no end, once every
//! tech is found), the board of directors (once listed: a goal each quarter, the trust, the share
//! price), the rivals (they take users from the products of their kind; bought, they bring theirs,
//! their team and their debt), the offices abroad (seats and a bonus each), the star employees (rare,
//! dear, coveted by the rivals), the products' roadmap (a focus and a price each, a pivot), and what a
//! sale leaves: the talents the stars buy, and the challenges of the next run.

use serde::{Deserialize, Serialize};

use super::{tx, Choice, Corp, Employee, EventDef, NewsKind, Product, Special, Tx, LISTED_STAGE, PRODUCTS, STAGES, TECHS};

// ---------------------------------------------------------------------------------------------------
// The optimisations.

pub struct Upgrade {
    pub id: &'static str,
    pub name: Tx,
    /// What one level gives.
    pub about: Tx,
    /// Research points for the first level (each next one ×`UPGRADE_STEP`).
    pub base: f64,
}

pub const UPGRADE_STEP: f64 = 1.45;

pub const UPGRADES: [Upgrade; 5] = [
    Upgrade { id: "perf", name: ("Optimisation du code", "Code optimisation"), about: ("+3 % de productivité par niveau.", "+3 % productivity per level."), base: 600_000.0 },
    Upgrade { id: "quality", name: ("Qualité logicielle", "Software quality"), about: ("−4 % de bugs et +2 % de qualité par niveau.", "−4 % bugs and +2 % quality per level."), base: 400_000.0 },
    Upgrade { id: "growth", name: ("Growth hacking", "Growth hacking"), about: ("+4 % d'utilisateurs par niveau.", "+4 % users per level."), base: 500_000.0 },
    Upgrade { id: "prospect", name: ("Prospection par IA", "AI prospecting"), about: ("+6 % d'offres et +2 % sur ce que tu encaisses par niveau.", "+6 % offers and +2 % on what you earn per level."), base: 500_000.0 },
    Upgrade { id: "infra", name: ("Infra sur mesure", "Custom infrastructure"), about: ("−6 % de pannes et −3 % de maintenance des produits par niveau.", "−6 % outages and −3 % product upkeep per level."), base: 400_000.0 },
];

pub fn upgrade(id: &str) -> Option<&'static Upgrade> {
    UPGRADES.iter().find(|u| u.id == id)
}

// ---------------------------------------------------------------------------------------------------
// The offices abroad.

pub struct BranchKind {
    pub id: &'static str,
    pub name: Tx,
    /// What it brings, per level.
    pub about: Tx,
    pub price: f64,
    /// € a second, per level.
    pub rent: f64,
    /// Seats, per level.
    pub seats: usize,
    pub stage: u8,
}

pub const BRANCH_MAX: u8 = 3;

pub const BRANCHES: [BranchKind; 7] = [
    BranchKind { id: "lyon", name: ("Lyon", "Lyon"), about: ("Les salaires de toute la boîte −4 % par niveau.", "The whole company's salaries −4 % per level."), price: 1_500_000.0, rent: 10.0, seats: 10, stage: 3 },
    BranchKind { id: "berlin", name: ("Berlin", "Berlin"), about: ("Des candidats d'un niveau de plus (deux au niveau 3).", "Candidates one level up (two at level 3)."), price: 4_000_000.0, rent: 20.0, seats: 15, stage: 3 },
    BranchKind { id: "bangalore", name: ("Bangalore", "Bangalore"), about: ("+4 % de productivité par niveau.", "+4 % productivity per level."), price: 10_000_000.0, rent: 25.0, seats: 30, stage: 4 },
    BranchKind { id: "london", name: ("Londres", "London"), about: ("+20 % d'offres et +3 % sur ce que tu encaisses par niveau.", "+20 % offers and +3 % on what you earn per level."), price: 25_000_000.0, rent: 80.0, seats: 15, stage: 4 },
    BranchKind { id: "newyork", name: ("New York", "New York"), about: ("+10 % d'utilisateurs par niveau.", "+10 % users per level."), price: 60_000_000.0, rent: 150.0, seats: 20, stage: 4 },
    BranchKind { id: "tokyo", name: ("Tokyo", "Tokyo"), about: ("Mobile +15 % et Design +10 % par niveau.", "Mobile +15 % and Design +10 % per level."), price: 150_000_000.0, rent: 200.0, seats: 20, stage: 5 },
    BranchKind { id: "sf", name: ("San Francisco", "San Francisco"), about: ("Recherche +25 % et valorisation +3 % par niveau.", "Research +25 % and valuation +3 % per level."), price: 400_000_000.0, rent: 500.0, seats: 25, stage: 5 },
];

pub fn branch_kind(id: &str) -> Option<&'static BranchKind> {
    BRANCHES.iter().find(|b| b.id == id)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub id: String,
    pub level: u8,
}

// ---------------------------------------------------------------------------------------------------
// The rivals.

const RIVAL_NAMES: [&str; 16] = [
    "Codezilla", "DevNest", "Pixelia", "Nuage & Co", "Stackr", "Kodo", "Byteforge", "Lambda Lune", "Quokka Soft", "Orbitale", "Zéphyr Tech", "Hexagone Labs", "Mistral Apps", "Pingouin.io", "Caribou Data", "Tartine Software",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rival {
    pub id: u32,
    pub name: String,
    /// The kind of product it makes (`PRODUCTS`).
    pub kind: u8,
    pub users: f64,
    pub staff: u32,
    pub level: u8,
    /// Its technical debt, 0 to 100 (bought: a part of it comes with it).
    pub debt: f64,
}

impl Rival {
    pub fn kind(&self) -> &'static super::ProductKind {
        &PRODUCTS[(self.kind as usize).min(PRODUCTS.len() - 1)]
    }

    /// What buying it costs.
    pub fn price(&self) -> f64 {
        let k = self.kind();
        k.cost * 2.0 + self.users * k.price * 9_000.0 + self.staff as f64 * 4_000.0 * super::level_mult(self.level)
    }

    /// How much of its market it holds (the users it takes from a product of its kind).
    pub fn share(&self) -> f64 {
        (self.users / self.kind().market).clamp(0.0, 0.35)
    }
}

/// At most this many rivals at once.
pub const MAX_RIVALS: usize = 3;

// ---------------------------------------------------------------------------------------------------
// The board of directors.

/// A quarter, in seconds (played, Ronnie open: the board waits while it is closed).
pub const QUARTER: f64 = 1200.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    /// 0 to 100: at 0, the board lets the founder go.
    pub trust: f64,
    /// The share price, 100 when listed: the valuation follows it (×0.5 to ×2).
    pub share: f64,
    pub left: f64,
    /// The quarter's goal (`GOALS`), its target, and where it started from.
    pub goal: u8,
    pub target: f64,
    pub start: f64,
    pub quarters: u32,
    pub met: u32,
    /// The last quarter's result.
    #[serde(default)]
    pub last: Option<bool>,
}

pub const GOALS: [Tx; 4] = [
    ("Encaisser {t} ce trimestre", "Earn {t} this quarter"),
    ("Valorisation à {t}", "Valuation up to {t}"),
    ("{t} utilisateurs sur tes produits", "{t} users on your products"),
    ("Livrer {t} contrats ce trimestre", "Deliver {t} contracts this quarter"),
];

// ---------------------------------------------------------------------------------------------------
// The star employees.

pub struct StarDef {
    pub id: &'static str,
    pub name: &'static str,
    pub job: &'static str,
    /// What they bring besides their skills.
    pub about: Tx,
}

pub const STARS: [StarDef; 8] = [
    StarDef { id: "hacker", name: "Margot « la Machine » Lenoir", job: "back", about: ("Code comme dix. La dette technique monte deux fois moins vite.", "Codes like ten. The technical debt rises half as fast.") },
    StarDef { id: "designer", name: "Sacha Moreau-Kim", job: "design", about: ("Ses interfaces font +10 % d'utilisateurs.", "Their interfaces bring +10 % users.") },
    StarDef { id: "growth", name: "Yanis « Viral » Benali", job: "fullstack", about: ("+25 % d'offres, +5 % d'utilisateurs.", "+25 % offers, +5 % users.") },
    StarDef { id: "data", name: "Dr Inès Okafor", job: "data", about: ("Recherche +30 %, Data +20 %.", "Research +30 %, Data +20 %.") },
    StarDef { id: "ops", name: "Bastien « Zéro panne » Roy", job: "devops", about: ("Pannes −40 %.", "Outages −40 %.") },
    StarDef { id: "manager", name: "Camille Arnaud", job: "lead", about: ("Manageuse née : les chef·fe·s de projet comptent une fois de plus.", "A born manager: project managers count once more.") },
    StarDef { id: "closer", name: "Hugo « le Closer » Vidal", job: "sales", about: ("+15 % sur ce que tu encaisses.", "+15 % on what you earn.") },
    StarDef { id: "mobile", name: "Kenji Faure", job: "mobile", about: ("Mobile +30 %, moral de l'équipe +3.", "Mobile +30 %, the team's morale +3.") },
];

pub fn star_def(id: &str) -> Option<&'static StarDef> {
    STARS.iter().find(|s| s.id == id)
}

/// From this stage on, a star now and then among the candidates.
pub const STAR_STAGE: u8 = 2;

// ---------------------------------------------------------------------------------------------------
// The talents (bought with the stars, kept forever) and the challenges.

pub struct Talent {
    pub id: &'static str,
    pub name: Tx,
    pub about: Tx,
    pub max: u32,
    /// Stars a level.
    pub cost: u32,
}

pub const TALENTS: [Talent; 8] = [
    Talent { id: "seed", name: ("Business angels", "Business angels"), about: ("+20 000 € au départ de chaque boîte, par niveau.", "+€20,000 at the start of each company, per level."), max: 5, cost: 1 },
    Talent { id: "network", name: ("Carnet d'adresses", "Little black book"), about: ("+15 de réputation au départ, par niveau.", "+15 reputation at the start, per level."), max: 5, cost: 1 },
    Talent { id: "veille", name: ("Veille techno", "Tech watch"), about: ("Chaque boîte commence avec Git et le cloud.", "Each company starts with Git and the cloud."), max: 1, cost: 3 },
    Talent { id: "coworking", name: ("Coworking offert", "Free coworking"), about: ("Chaque boîte commence au coin de coworking.", "Each company starts in the coworking corner."), max: 1, cost: 3 },
    Talent { id: "mentor", name: ("Mentorat", "Mentoring"), about: ("XP +10 % par niveau.", "XP +10 % per level."), max: 5, cost: 2 },
    Talent { id: "seasoned", name: ("Rodé·e", "Seasoned"), about: ("Productivité +4 % par niveau.", "Productivity +4 % per level."), max: 5, cost: 2 },
    Talent { id: "nestegg", name: ("Bas de laine", "Nest egg"), about: ("+10 000 € sur ton compte perso à chaque revente, par niveau.", "+€10,000 on your personal account at each sale, per level."), max: 5, cost: 1 },
    Talent { id: "charisma", name: ("Charisme", "Charisma"), about: ("Confiance du conseil d'administration +10 au départ, par niveau.", "The board's trust +10 at the start, per level."), max: 3, cost: 2 },
];

pub fn talent(id: &str) -> Option<&'static Talent> {
    TALENTS.iter().find(|t| t.id == id)
}

pub struct Challenge {
    pub id: &'static str,
    pub name: Tx,
    /// The rule, and the goal.
    pub about: Tx,
    /// The stage to reach.
    pub goal: u8,
    /// Stars won on top.
    pub reward: u32,
}

pub const CHALLENGES: [Challenge; 6] = [
    Challenge { id: "no_sales", name: ("Sans commercial", "No salespeople"), about: ("Aucun·e commercial·e ne peut être recruté·e. Atteindre la Scale-up.", "No salesperson can be hired. Reach the Scale-up."), goal: 3, reward: 3 },
    Challenge { id: "remote", name: ("100 % télétravail", "100 % remote"), about: ("Full remote imposé. Atteindre la Scale-up.", "Fully remote, always. Reach the Scale-up."), goal: 3, reward: 2 },
    Challenge { id: "small", name: ("Petite équipe", "Small team"), about: ("12 personnes au plus, toi compris·e. Atteindre la Scale-up.", "12 people at most, you included. Reach the Scale-up."), goal: 3, reward: 4 },
    Challenge { id: "contracts", name: ("Que des contrats", "Contracts only"), about: ("Aucun produit. Atteindre la Scale-up.", "No products. Reach the Scale-up."), goal: 3, reward: 4 },
    Challenge { id: "chill", name: ("Zéro crunch", "Zero crunch"), about: ("Horaires Cool imposés. Atteindre la Scale-up.", "Relaxed hours, always. Reach the Scale-up."), goal: 3, reward: 3 },
    Challenge { id: "speed", name: ("Contre la montre", "Against the clock"), about: ("Atteindre la PME en moins de 90 minutes de jeu.", "Reach the small business stage in under 90 minutes of play."), goal: 2, reward: 3 },
];

/// The time the speed challenge gives (seconds played).
pub const SPEED_LIMIT: f64 = 5400.0;

pub fn challenge(id: &str) -> Option<&'static Challenge> {
    CHALLENGES.iter().find(|c| c.id == id)
}

// ---------------------------------------------------------------------------------------------------
// The products' roadmap.

pub const FOCUSES: [(Tx, Tx); 4] = [
    (("Fonctionnalités", "Features"), ("Tout le travail va vers la version suivante.", "All the work goes to the next version.")),
    (("Performance", "Performance"), ("Moitié moins vite vers la version suivante, maintenance −30 %, les bugs baissent deux fois plus vite.", "Half as fast to the next version, upkeep −30 %, bugs drop twice as fast.")),
    (("Croissance", "Growth"), ("Moitié moins vite vers la version suivante, +15 % d'utilisateurs.", "Half as fast to the next version, +15 % users.")),
    (("Monétisation", "Monetisation"), ("Moitié moins vite vers la version suivante, +15 % de revenus par utilisateur.", "Half as fast to the next version, +15 % revenue per user.")),
];

/// From −2 (free, with ads) to +2 (luxury): (name, revenue a user, users).
pub const PRICES: [(Tx, f64, f64); 5] = [
    (("Gratuit + pubs", "Free + ads"), 0.5, 1.4),
    (("Petit prix", "Cheap"), 0.8, 1.15),
    (("Normal", "Normal"), 1.0, 1.0),
    (("Premium", "Premium"), 1.5, 0.75),
    (("Luxe", "Luxury"), 2.2, 0.5),
];

impl Product {
    fn price_row(&self) -> &'static (Tx, f64, f64) {
        &PRICES[(self.price as i32 + 2).clamp(0, 4) as usize]
    }

    /// Revenue a user, against the normal price.
    pub fn price_mult(&self) -> f64 {
        self.price_row().1 * if self.focus == 3 { 1.15 } else { 1.0 }
    }

    /// The users it can reach, against the normal price.
    pub fn users_mult(&self) -> f64 {
        self.price_row().2 * if self.focus == 2 { 1.15 } else { 1.0 }
    }

    /// The share of its points going to the next version.
    pub fn build_share(&self) -> f64 {
        if self.focus == 0 { 1.0 } else { 0.5 }
    }
}

// ---------------------------------------------------------------------------------------------------
// The rules.

impl Corp {
    // --- the optimisations

    /// Every tech found: the optimisations open.
    pub fn techs_done(&self) -> bool {
        TECHS.iter().all(|t| self.has_tech(t.id))
    }

    pub fn upgrade_level(&self, id: &str) -> u32 {
        self.upgrades.get(id).copied().unwrap_or(0)
    }

    pub fn upgrade_cost(&self, id: &str) -> f64 {
        upgrade(id).map_or(f64::INFINITY, |u| u.base * UPGRADE_STEP.powi(self.upgrade_level(id) as i32))
    }

    pub fn can_upgrade(&self, id: &str) -> bool {
        upgrade(id).is_some() && self.techs_done() && self.research >= self.upgrade_cost(id)
    }

    pub fn learn_upgrade(&mut self, id: &str) -> bool {
        if !self.can_upgrade(id) {
            return false;
        }
        self.research -= self.upgrade_cost(id);
        *self.upgrades.entry(id.to_owned()).or_insert(0) += 1;
        let k = UPGRADES.iter().position(|u| u.id == id).unwrap_or(0);
        self.say(NewsKind::Upgrade, "", k as f64, self.upgrade_level(id) as f64);
        self.acted();
        true
    }

    // --- the offices abroad

    pub fn branch_level(&self, id: &str) -> u8 {
        self.branches.iter().find(|b| b.id == id).map_or(0, |b| b.level)
    }

    /// What opening it (level 0) or growing it costs.
    pub fn branch_cost(&self, id: &str) -> f64 {
        branch_kind(id).map_or(f64::INFINITY, |b| b.price * (1.0 + 1.5 * self.branch_level(id) as f64))
    }

    pub fn can_branch(&self, id: &str) -> bool {
        branch_kind(id).is_some_and(|b| self.stage >= b.stage && self.branch_level(id) < BRANCH_MAX && self.cash >= self.branch_cost(id))
    }

    pub fn open_branch(&mut self, id: &str) -> bool {
        if !self.can_branch(id) {
            return false;
        }
        self.cash -= self.branch_cost(id);
        match self.branches.iter_mut().find(|b| b.id == id) {
            Some(b) => b.level += 1,
            None => self.branches.push(Branch { id: id.to_owned(), level: 1 }),
        }
        let k = BRANCHES.iter().position(|b| b.id == id).unwrap_or(0);
        self.say(NewsKind::Branch, "", k as f64, self.branch_level(id) as f64);
        self.acted();
        true
    }

    pub fn branch_seats(&self) -> usize {
        self.branches.iter().filter_map(|b| branch_kind(&b.id).map(|k| k.seats * b.level as usize)).sum()
    }

    pub fn branch_rent(&self) -> f64 {
        self.branches.iter().filter_map(|b| branch_kind(&b.id).map(|k| k.rent * b.level as f64)).sum()
    }

    // --- the rivals

    pub(super) fn spawn_rival(&mut self) {
        let kinds: Vec<usize> = (0..PRODUCTS.len()).filter(|&k| PRODUCTS[k].stage <= self.stage && !self.rivals.iter().any(|r| r.kind as usize == k)).collect();
        if kinds.is_empty() {
            return;
        }
        let kind = kinds[self.pick(kinds.len())];
        let taken: Vec<&str> = self.rivals.iter().map(|r| r.name.as_str()).collect();
        let free: Vec<&str> = RIVAL_NAMES.iter().copied().filter(|n| !taken.contains(n)).collect();
        let name = free[self.pick(free.len())].to_owned();
        let market = PRODUCTS[kind].market;
        let users = market * (0.03 + 0.07 * self.rand());
        let staff = 4 + self.pick(6 * self.stage.max(1) as usize) as u32;
        let level = (1 + self.stage / 2 + self.pick(3) as u8).min(super::MAX_LEVEL);
        let debt = 10.0 + 60.0 * self.rand();
        let id = self.id();
        self.rivals.push(Rival { id, name: name.clone(), kind: kind as u8, users, staff, level, debt });
        self.say(NewsKind::Rival, &name, kind as f64, 0.0);
    }

    /// The rivals grow; now and then a new one.
    pub(super) fn run_rivals(&mut self, dt: f64) {
        if self.stage < super::SELL_STAGE {
            return;
        }
        for r in &mut self.rivals {
            let market = r.kind().market;
            let target = market * 0.35;
            r.users += (target - r.users) * (1.0 - (-0.0006 * dt).exp());
            r.debt = (r.debt + 0.002 * dt).min(100.0);
        }
        self.rival_in -= dt;
        if self.rival_in <= 0.0 {
            self.rival_in = 900.0 + 900.0 * self.rand();
            if self.rivals.len() < MAX_RIVALS {
                self.spawn_rival();
            } else if self.chance(0.3) {
                // One of them takes on people.
                let k = self.pick(self.rivals.len());
                self.rivals[k].staff += 1 + self.pick(4) as u32;
            }
        }
    }

    /// The share of a product's market the rivals of its kind take.
    pub fn rival_pressure(&self, kind: u8) -> f64 {
        self.rivals.iter().filter(|r| r.kind == kind).map(Rival::share).sum::<f64>().min(0.35) * 0.5
    }

    pub fn can_acquire(&self, id: u32) -> bool {
        self.rivals.iter().any(|r| r.id == id && self.cash >= r.price())
    }

    /// Bought: its users (into the product of its kind, or a new one), as many of its team as there are
    /// seats, a part of its debt.
    pub fn acquire(&mut self, id: u32) -> bool {
        if !self.can_acquire(id) {
            return false;
        }
        let k = self.rivals.iter().position(|r| r.id == id).expect("checked");
        let r = self.rivals.remove(k);
        self.cash -= r.price();
        let users = r.users * 0.7;
        let room = self.products.len() < self.max_products() && !self.challenge_is("contracts");
        match self.products.iter_mut().find(|p| p.kind == r.kind) {
            Some(p) => {
                p.users += users;
                if p.version == 0 {
                    p.version = 1;
                }
            }
            None if room => {
                let pid = self.id();
                let version = (1 + self.stage as u32 / 2).max(1);
                self.products.push(Product { id: pid, kind: r.kind, version, progress: 0.0, users, bugs: 30.0, down: 0.0, focus: 0, price: 0 });
            }
            None => {}
        }
        let jobs = ["front", "back", "mobile", "fullstack", "design", "data"];
        let mut came = 0;
        for _ in 0..r.staff {
            if self.staff.len() >= self.capacity() {
                break;
            }
            let job = jobs[self.pick(jobs.len())];
            let e = self.make_employee(job, r.level);
            self.staff.push(e);
            came += 1;
        }
        self.debt = (self.debt + r.debt * 0.3).min(100.0);
        self.rep += 5.0 * (1.0 + self.stage as f64);
        self.stats.acquired += 1;
        self.say(NewsKind::Acquired, &r.name, r.kind as f64, came as f64);
        self.acted();
        true
    }

    // --- the board

    fn new_quarter(&mut self) {
        let users: f64 = self.products.iter().map(|p| p.users).sum();
        let goals: Vec<u8> = (0..GOALS.len() as u8).filter(|&g| g != 2 || users > 100.0).collect();
        let goal = goals[self.pick(goals.len())];
        let (start, target) = match goal {
            0 => (self.stats.earned, (self.income * QUARTER * 1.1).max(1_000.0)),
            1 => (self.valuation(), self.valuation() * 1.06),
            2 => (users, (users * 1.08).ceil()),
            _ => (self.stats.contracts as f64, (4 + self.count(Special::Lead).min(6) + self.sales().min(4)) as f64),
        };
        if let Some(b) = self.board.as_mut() {
            b.goal = goal;
            b.start = start;
            b.target = target;
            b.left = QUARTER;
        }
    }

    /// The quarter's goal: where it is, and where it must go.
    pub fn board_progress(&self) -> Option<(f64, f64)> {
        let b = self.board.as_ref()?;
        Some(match b.goal {
            0 => (self.stats.earned - b.start, b.target),
            1 => (self.valuation(), b.target),
            2 => (self.products.iter().map(|p| p.users).sum(), b.target),
            _ => (self.stats.contracts as f64 - b.start, b.target),
        })
    }

    pub fn goal_text(&self, fr: bool) -> String {
        let Some(b) = &self.board else { return String::new() };
        let t = match b.goal {
            0 | 1 => super::money(b.target, fr),
            _ => format!("{:.0}", b.target),
        };
        tx(GOALS[(b.goal as usize).min(GOALS.len() - 1)], fr).replace("{t}", &t)
    }

    /// Listed: the board sits; each quarter, a goal met or missed.
    pub(super) fn run_board(&mut self, dt: f64, online: bool) {
        if self.board.is_none() {
            if self.stage < LISTED_STAGE {
                return;
            }
            let trust = 60.0 + 10.0 * self.legacy.talent("charisma") as f64;
            self.board = Some(Board { trust, share: 100.0, left: QUARTER, goal: 0, target: 0.0, start: 0.0, quarters: 0, met: 0, last: None });
            self.new_quarter();
        }
        // The board meets only while the founder is there.
        if !online {
            return;
        }
        let Some(b) = self.board.as_mut() else { return };
        b.left -= dt;
        if b.left > 0.0 {
            return;
        }
        let (now, target) = self.board_progress().unwrap_or((0.0, 1.0));
        let met = now >= target;
        let div = [-6.0, 3.0, 8.0][self.policy.dividends.min(2) as usize];
        let (up, noise) = (self.rand(), self.rand());
        let value = self.valuation();
        let b = self.board.as_mut().expect("checked");
        b.quarters += 1;
        b.last = Some(met);
        if met {
            b.met += 1;
            b.trust += 15.0 + div;
            b.share *= 1.06 + 0.08 * up;
        } else {
            b.trust += -20.0 + div;
            b.share *= 0.84 + 0.06 * noise;
        }
        b.trust = b.trust.clamp(0.0, 100.0);
        b.share = b.share.clamp(30.0, 400.0);
        let (trust, share) = (b.trust, b.share);
        if met {
            // The founder's bonus, in shares sold at once.
            let bonus = value * 0.001;
            self.life.account += bonus;
            self.say(NewsKind::Quarter, "", 1.0, bonus);
        } else {
            self.say(NewsKind::Quarter, "", 0.0, trust);
        }
        if trust <= 0.0 {
            self.ousted();
            return;
        }
        let _ = share;
        self.new_quarter();
    }

    /// The board lets the founder go: the company sold for them, half the stars.
    fn ousted(&mut self) {
        let value = self.valuation();
        let stars = (self.sell_stars() / 2).max(1);
        self.sell_as(self.at, None, stars, value * 0.02);
        self.say(NewsKind::Ousted, "", value, stars as f64);
    }

    /// The valuation follows the share price, once listed.
    pub fn share_mult(&self) -> f64 {
        self.board.as_ref().map_or(1.0, |b| (b.share / 100.0).clamp(0.5, 2.0))
    }

    // --- the stars among the candidates

    /// A star among the new candidates, now and then (one not in the team yet).
    pub(super) fn maybe_star_candidate(&mut self) {
        if self.stage < STAR_STAGE || !self.chance(0.12 + 0.03 * self.stage as f64) {
            return;
        }
        let free: Vec<usize> = (0..STARS.len()).filter(|&k| !self.staff.iter().chain(&self.candidates).any(|e| e.star.as_deref() == Some(STARS[k].id))).collect();
        if free.is_empty() {
            return;
        }
        let s = &STARS[free[self.pick(free.len())]];
        let level = (6 + self.stage).min(super::MAX_LEVEL);
        let mut e = self.make_employee(s.job, level);
        e.name = s.name.to_owned();
        e.skills = e.skills.map(|x| x * 2.0);
        e.salary *= 3.0;
        e.trait_id = None;
        e.star = Some(s.id.to_owned());
        if !self.candidates.is_empty() {
            let k = self.pick(self.candidates.len());
            self.candidates[k] = e;
        } else {
            self.candidates.push(e);
        }
    }

    // --- the challenges

    pub fn challenge_is(&self, id: &str) -> bool {
        self.challenge.as_deref() == Some(id)
    }

    /// The candidate can be hired under the challenge.
    pub fn allowed(&self, e: &Employee) -> bool {
        !(self.challenge_is("no_sales") && e.job().special == Special::Sales)
    }

    /// The challenge's rules kept, and won (or lost: out of time).
    pub(super) fn run_challenge(&mut self, dt: f64) {
        let Some(ch) = self.challenge.as_deref().and_then(challenge) else {
            self.challenge = None;
            return;
        };
        self.run_time += dt;
        match ch.id {
            "remote" => self.policy.remote = 2,
            "chill" => self.policy.hours = 0,
            _ => {}
        }
        if ch.id == "speed" && self.run_time > SPEED_LIMIT && self.stage < ch.goal {
            self.challenge = None;
            self.say(NewsKind::Challenge, "", CHALLENGES.iter().position(|c| c.id == ch.id).unwrap_or(0) as f64, 0.0);
            return;
        }
        if self.stage >= ch.goal {
            self.legacy.stars += ch.reward;
            if !self.legacy.challenges.iter().any(|c| c == ch.id) {
                self.legacy.challenges.push(ch.id.to_owned());
            }
            self.challenge = None;
            self.say(NewsKind::Challenge, "", CHALLENGES.iter().position(|c| c.id == ch.id).unwrap_or(0) as f64, ch.reward as f64);
        }
    }

    // --- the talents

    /// Stars not put in a talent yet.
    pub fn talent_points(&self) -> u32 {
        let spent: u32 = TALENTS.iter().map(|t| t.cost * self.legacy.talent(t.id)).sum();
        self.legacy.stars.saturating_sub(spent)
    }

    pub fn can_take_talent(&self, id: &str) -> bool {
        talent(id).is_some_and(|t| self.legacy.talent(id) < t.max && self.talent_points() >= t.cost)
    }

    pub fn take_talent(&mut self, id: &str) -> bool {
        if !self.can_take_talent(id) {
            return false;
        }
        *self.legacy.talents.entry(id.to_owned()).or_insert(0) += 1;
        self.acted();
        true
    }

    /// Every talent given back, to share the stars out again.
    pub fn reset_talents(&mut self) {
        if !self.legacy.talents.is_empty() {
            self.legacy.talents.clear();
            self.acted();
        }
    }

    // --- the products' roadmap

    pub fn set_focus(&mut self, id: u32, focus: u8) {
        if let Some(p) = self.products.iter_mut().find(|p| p.id == id)
            && p.focus != focus
            && (focus as usize) < FOCUSES.len()
        {
            p.focus = focus;
            self.acted();
        }
    }

    pub fn set_price(&mut self, id: u32, price: i8) {
        if let Some(p) = self.products.iter_mut().find(|p| p.id == id)
            && p.price != price
            && (-2..=2).contains(&price)
        {
            p.price = price;
            self.acted();
        }
    }

    /// Turning a product into another kind: half that kind's launch, its users a third, its versions
    /// half (one at least).
    pub fn pivot_cost(&self, kind: usize) -> f64 {
        PRODUCTS.get(kind).map_or(f64::INFINITY, |k| k.cost * 0.5)
    }

    pub fn can_pivot(&self, id: u32, kind: usize) -> bool {
        PRODUCTS.get(kind).is_some_and(|k| {
            self.has_tech(k.tech) && self.stage >= k.stage && self.cash >= self.pivot_cost(kind) && !self.products.iter().any(|q| q.kind as usize == kind) && self.products.iter().any(|p| p.id == id && p.version > 0)
        })
    }

    pub fn pivot(&mut self, id: u32, kind: usize) -> bool {
        if !self.can_pivot(id, kind) {
            return false;
        }
        self.cash -= self.pivot_cost(kind);
        if let Some(p) = self.products.iter_mut().find(|p| p.id == id) {
            let from = p.kind;
            p.kind = kind as u8;
            p.version = (p.version / 2).max(1);
            p.users *= 0.33;
            p.progress = 0.0;
            p.bugs = (p.bugs + 15.0).min(100.0);
            self.say(NewsKind::Pivot, "", from as f64, kind as f64);
        }
        self.acted();
        true
    }

    // --- the money between the company and the founder

    /// The founder's pay (€ a second): 0 to 3, growing with the company.
    pub fn founder_pay(&self) -> f64 {
        [0.0, 0.4, 1.5, 5.0][self.policy.founder_pay.min(3) as usize] * (1.0 + self.stage as f64).powi(2)
    }

    /// The share of the dividends going to the founder (the shareholders take the rest, once listed).
    pub fn founder_share(&self) -> f64 {
        if self.board.is_some() { 0.25 } else { 1.0 }
    }

    /// The dividends paid now (€ a second): a part of the profit, while the cash is above the reserve.
    pub fn dividends(&self) -> f64 {
        let profit = (self.income - self.spend).max(0.0);
        if self.cash <= self.reserve() { 0.0 } else { profit * [0.0, 0.1, 0.3][self.policy.dividends.min(2) as usize] }
    }

    /// Money put back into the company from the personal account.
    pub fn invest_in_company(&mut self, amount: f64) -> bool {
        let amount = amount.min(self.life.account);
        if amount <= 0.0 {
            return false;
        }
        self.life.account -= amount;
        self.cash += amount;
        self.acted();
        true
    }

    /// Money taken out of the company (an exceptional dividend, taxed 30 %), never below the reserve.
    pub fn take_from_company(&mut self, amount: f64) -> bool {
        let amount = amount.min(self.cash - self.reserve());
        if amount <= 0.0 {
            return false;
        }
        self.cash -= amount;
        self.life.account += amount * 0.7 * self.founder_share().max(0.25);
        if let Some(b) = self.board.as_mut() {
            b.trust = (b.trust - 2.0).max(0.0);
        }
        self.acted();
        true
    }

    /// Each second: the dividends, the board, the rivals, the challenge.
    pub(super) fn run_growth(&mut self, dt: f64, online: bool, sum: &mut super::Summary) {
        let pay = if self.cash >= 0.0 { self.founder_pay() * dt } else { 0.0 };
        self.life.account += pay;
        let div = self.dividends() * dt;
        if div > 0.0 {
            self.cash -= div;
            sum.spent += div;
            self.life.account += div * self.founder_share();
        }
        self.run_board(dt, online);
        self.run_rivals(dt);
        self.run_challenge(dt);
    }

    /// What the optimisations, offices, stars and talents make of the work (added to `mods`).
    pub(super) fn growth_mods(&self, m: &mut super::Mods) {
        for (id, &n) in &self.upgrades {
            let n = n as f64;
            match id.as_str() {
                "perf" => m.prod += 0.03 * n,
                "quality" => {
                    m.bugs *= 0.96f64.powf(n);
                    m.quality += 0.02 * n;
                }
                "growth" => m.users += 0.04 * n,
                "prospect" => {
                    m.offers *= 1.0 + 0.06 * n;
                    m.pay *= 1.0 + 0.02 * n;
                }
                "infra" => {
                    m.outage *= 0.94f64.powf(n);
                    m.upkeep *= 0.97f64.powf(n);
                }
                _ => {}
            }
        }
        for b in &self.branches {
            let n = b.level as f64;
            match b.id.as_str() {
                "berlin" => m.candidate_level += if b.level >= 3 { 2 } else { 1 },
                "bangalore" => m.prod += 0.04 * n,
                "london" => {
                    m.offers *= 1.0 + 0.2 * n;
                    m.pay *= 1.0 + 0.03 * n;
                }
                "newyork" => m.users += 0.1 * n,
                "tokyo" => {
                    m.skill[2] += 0.15 * n;
                    m.skill[4] += 0.1 * n;
                }
                "sf" => m.research += 0.25 * n,
                _ => {}
            }
        }
        for s in self.staff.iter().filter_map(|e| e.star.as_deref()) {
            match s {
                "hacker" => m.debt *= 0.5,
                "designer" => m.users += 0.1,
                "growth" => {
                    m.offers *= 1.25;
                    m.users += 0.05;
                }
                "data" => {
                    m.research += 0.3;
                    m.skill[3] += 0.2;
                }
                "ops" => m.outage *= 0.6,
                "manager" => m.lead += 1.0,
                "closer" => m.pay *= 1.15,
                "mobile" => {
                    m.skill[2] += 0.3;
                    m.morale += 3.0;
                }
                _ => {}
            }
        }
        m.prod += 0.04 * self.legacy.talent("seasoned") as f64;
        m.xp += 0.1 * self.legacy.talent("mentor") as f64;
        // The founder's hobbies and luxuries, for the company.
        if self.life.does("golf") {
            m.offers *= 1.1;
        }
        if self.life.does("charity") {
            m.rep += 0.1;
        }
        if self.life.has("foundation") {
            m.rep += 0.2;
        }
    }

    /// The stage the board sits from, for its teaser.
    pub fn listed_name(fr: bool) -> &'static str {
        tx(STAGES[LISTED_STAGE as usize].name, fr)
    }

    fn star_of_event(&mut self) -> Option<&mut Employee> {
        let id = self.event_who;
        self.staff.iter_mut().find(|e| e.id == id && e.star.is_some())
    }
}

/// The big company's surprises: a rival after a star, a price war, a buyer for a product, an activist
/// shareholder.
pub const GROWTH_EVENTS: [EventDef; 4] = [
    EventDef {
        id: "poach",
        title: ("Un concurrent drague ta star", "A rival is courting your star"),
        text: ("Un concurrent propose à {who} le double de son salaire. {who} hésite et vient t'en parler.", "A rival offers {who} twice their salary. {who} is torn and comes to talk to you."),
        fits: |c| !c.rivals.is_empty() && c.staff.iter().any(|e| e.star.is_some()),
        choices: &[
            Choice { label: ("L'augmenter de 30 %", "Give them a 30 % raise"), outcomes: &[("{who} reste, ravi·e.", "{who} stays, delighted.")], apply: |c| {
                if let Some(e) = c.star_of_event() {
                    e.salary *= 1.3;
                    e.morale = (e.morale + 15.0).min(100.0);
                }
                0
            } },
            Choice { label: ("Lui parler du projet", "Talk about the project"), outcomes: &[("{who} croit au projet et reste.", "{who} believes in the project and stays."), ("{who} part chez le concurrent.", "{who} leaves for the rival.")], apply: |c| {
                let morale = c.star_of_event().map_or(0.0, |e| e.morale);
                if c.chance((morale / 100.0).clamp(0.2, 0.85)) {
                    0
                } else {
                    c.who_leaves();
                    1
                }
            } },
            Choice { label: ("Le·la laisser partir", "Let them go"), outcomes: &[("{who} s'en va. Bonne route.", "{who} leaves. Farewell.")], apply: |c| {
                c.who_leaves();
                0
            } },
        ],
    },
    EventDef {
        id: "price_war",
        title: ("Guerre des prix", "Price war"),
        text: ("Un concurrent casse ses prix pour te prendre tes utilisateurs.", "A rival slashes its prices to steal your users."),
        fits: |c| !c.rivals.is_empty() && c.live_products(),
        choices: &[
            Choice { label: ("Suivre : baisser les prix", "Follow: cut prices"), outcomes: &[("Tu gardes tes utilisateurs, mais tu gagnes moins pendant un moment.", "You keep your users, but earn less for a while.")], apply: |c| {
                c.boost(super::BoostKind::Users, 1.15, 900.0);
                c.cash -= c.unit() * 1.5;
                0
            } },
            Choice { label: ("Miser sur la qualité", "Bet on quality"), outcomes: &[("Une partie de tes utilisateurs part, les autres restent fidèles.", "Some of your users leave, the others stay loyal.")], apply: |c| {
                c.users_times(0.9);
                c.rep += 2.0;
                0
            } },
        ],
    },
    EventDef {
        id: "buyer",
        title: ("Une offre pour un de tes produits", "An offer for one of your products"),
        text: ("Un géant du secteur veut racheter un de tes produits. Le chèque est gros.", "An industry giant wants to buy one of your products. The cheque is big."),
        fits: |c| c.products.iter().filter(|p| p.version > 0).count() >= 2,
        choices: &[
            Choice { label: ("Vendre le plus petit", "Sell the smallest"), outcomes: &[("Vendu. La trésorerie fait un bond.", "Sold. The cash jumps.")], apply: |c| {
                if let Some(k) = (0..c.products.len()).filter(|&k| c.products[k].version > 0).min_by(|&a, &b| c.products[a].revenue().total_cmp(&c.products[b].revenue())) {
                    let p = c.products.remove(k);
                    c.cash += p.revenue() * 15_000.0 + p.kind().cost;
                }
                0
            } },
            Choice { label: ("Pas à vendre", "Not for sale"), outcomes: &[("L'équipe produit est fière.", "The product team is proud.")], apply: |c| {
                c.mood += 3.0;
                0
            } },
        ],
    },
    EventDef {
        id: "activist",
        title: ("Un actionnaire activiste", "An activist shareholder"),
        text: ("Un fonds achète des actions et exige plus de dividendes, tout de suite.", "A fund buys shares and demands more dividends, right now."),
        fits: |c| c.board.is_some(),
        choices: &[
            Choice { label: ("Verser un dividende exceptionnel", "Pay a one-off dividend"), outcomes: &[("Le fonds se calme. Le conseil apprécie.", "The fund calms down. The board approves.")], apply: |c| {
                c.cash -= c.unit() * 3.0;
                if let Some(b) = c.board.as_mut() {
                    b.trust = (b.trust + 10.0).min(100.0);
                }
                0
            } },
            Choice { label: ("Tenir bon", "Stand firm"), outcomes: &[("Bras de fer dans la presse. Le cours baisse un peu.", "A tug of war in the press. The share price dips.")], apply: |c| {
                if let Some(b) = c.board.as_mut() {
                    b.trust = (b.trust - 8.0).max(0.0);
                    b.share *= 0.95;
                }
                0
            } },
        ],
    },
];
