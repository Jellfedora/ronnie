//! Ronnie Corp: a software company to grow, idle. Alone in a garage at first, then offices, a team
//! (jobs, levels, traits, morale), contracts worked by skills in order of priority, SaaS products
//! (users, subscriptions, outages), technical debt, research, fittings, the management's policies,
//! events with choices, the stages from the garage to the stock exchange, and the company sold for a
//! lasting bonus (the stars).
//!
//! Only the rules here (the page is `app::tycoon`): the company is a plain value, saved with the
//! rewards (sealed) and synced with floor. It runs while Ronnie is open (`run`), and while it is
//! closed it is caught up when Ronnie opens again (`catch_up`): `OFFLINE_CAP` at most, at half speed,
//! with no events nor resignations, and never into the red (the catching up stops before).
//!
//! The texts are in both languages, here (`Tx`: French, English).

use serde::{Deserialize, Serialize};

/// A text in French and in English.
pub type Tx = (&'static str, &'static str);

pub fn tx(t: Tx, fr: bool) -> &'static str {
    if fr { t.0 } else { t.1 }
}

/// What's caught up while Ronnie was closed, at most (seconds).
pub const OFFLINE_CAP: f64 = 4.0 * 3600.0;
/// The time closed counts at this speed.
pub const OFFLINE_RATE: f64 = 0.5;
/// The highest level of an employee.
pub const MAX_LEVEL: u8 = 10;
/// The news kept.
const NEWS: usize = 40;
/// The cash kept every minute, for the curve.
pub const HISTORY: usize = 120;

pub const SKILLS: [Tx; 5] = [("Front", "Front"), ("Back", "Back"), ("Mobile", "Mobile"), ("Data", "Data"), ("Design", "Design")];

// ---------------------------------------------------------------------------------------------------
// The catalogue.

/// What a job does besides its skills.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    None,
    Founder,
    /// Fewer outages, less debt.
    Ops,
    /// Better deliveries, fewer bugs.
    Qa,
    /// More offers, better paid, accepts them by itself.
    Sales,
    /// The team around more productive.
    Lead,
    /// Morale, better candidates.
    Hr,
    /// Research, debt.
    Cto,
    /// Buys the fittings, moves the offices.
    Office,
    /// Launches the products.
    Po,
    /// Answers the events left waiting.
    Assistant,
}

pub struct Job {
    pub id: &'static str,
    pub name: Tx,
    pub about: Tx,
    /// Points made a second in each skill (`SKILLS`), at level 1.
    pub skills: [f64; 5],
    /// € a second, at level 1.
    pub salary: f64,
    /// The stage it can be hired from.
    pub stage: u8,
    pub special: Special,
}

pub const JOBS: [Job; 16] = [
    Job { id: "founder", name: ("Fondateur·rice", "Founder"), about: ("Toi. Touche-à-tout, pas de salaire, jamais viré·e.", "You. Jack of all trades, no salary, never fired."), skills: [1.2, 1.2, 0.6, 0.6, 0.6], salary: 0.0, stage: 99, special: Special::Founder },
    Job { id: "front", name: ("Dev front", "Front-end dev"), about: ("Des interfaces web, vite et bien.", "Web interfaces, fast and neat."), skills: [3.0, 0.4, 0.5, 0.0, 1.0], salary: 1.2, stage: 0, special: Special::None },
    Job { id: "back", name: ("Dev back", "Back-end dev"), about: ("API, bases de données, tout ce qui ne se voit pas.", "APIs, databases, all that can't be seen."), skills: [0.4, 3.0, 0.0, 1.0, 0.0], salary: 1.3, stage: 0, special: Special::None },
    Job { id: "design", name: ("Designer UX", "UX designer"), about: ("Rend les choses belles et utilisables.", "Makes things pretty and usable."), skills: [1.0, 0.0, 0.4, 0.0, 3.0], salary: 1.2, stage: 0, special: Special::None },
    Job { id: "mobile", name: ("Dev mobile", "Mobile dev"), about: ("Applis iPhone et Android.", "iPhone and Android apps."), skills: [0.8, 0.5, 3.0, 0.0, 0.6], salary: 1.4, stage: 0, special: Special::None },
    Job { id: "qa", name: ("Testeur·se QA", "QA tester"), about: ("Casse tout avant les clients : livraisons meilleures, moins de bugs.", "Breaks everything before the clients do: better deliveries, fewer bugs."), skills: [0.4, 0.4, 0.4, 0.0, 0.2], salary: 1.0, stage: 0, special: Special::Qa },
    Job { id: "sales", name: ("Commercial·e", "Salesperson"), about: ("⚙ Accepte les contrats tout·e seul·e. Et plus d'offres, mieux payées.", "⚙ Accepts the contracts alone. And more offers, better paid."), skills: [0.0; 5], salary: 1.4, stage: 0, special: Special::Sales },
    Job { id: "fullstack", name: ("Dev fullstack", "Full-stack dev"), about: ("Un peu de tout, beaucoup de café.", "A bit of everything, a lot of coffee."), skills: [2.0, 2.0, 0.6, 0.6, 0.6], salary: 1.7, stage: 1, special: Special::None },
    Job { id: "data", name: ("Data scientist", "Data scientist"), about: ("Statistiques, IA, tableaux de bord.", "Statistics, AI, dashboards."), skills: [0.0, 1.0, 0.0, 3.2, 0.2], salary: 1.7, stage: 1, special: Special::None },
    Job { id: "devops", name: ("DevOps", "DevOps"), about: ("Serveurs, déploiements : moins de pannes, moins de dette.", "Servers, deployments: fewer outages, less debt."), skills: [0.0, 1.5, 0.0, 0.5, 0.0], salary: 1.6, stage: 1, special: Special::Ops },
    Job { id: "lead", name: ("Chef·fe de projet", "Project manager"), about: ("⚙ Répartit les tâches de l'équipe tout·e seul·e. Et rend jusqu'à 6 personnes plus productives.", "⚙ Shares out the team's tasks alone. And makes up to 6 people more productive."), skills: [0.3, 0.3, 0.0, 0.0, 0.5], salary: 1.9, stage: 2, special: Special::Lead },
    Job { id: "hr", name: ("RH", "HR"), about: ("⚙ Recrute tout·e seul·e sur les places libres. Et le moral, de meilleurs candidats.", "⚙ Hires alone into the free seats. And morale, better candidates."), skills: [0.0; 5], salary: 1.4, stage: 1, special: Special::Hr },
    Job { id: "cto", name: ("CTO", "CTO"), about: ("⚙ Débloque les technos tout·e seul·e. Recherche ×1,5, combat la dette. Un·e seul·e.", "⚙ Unlocks the techs alone. Research ×1.5, fights the debt. Only one."), skills: [2.0, 2.4, 1.0, 2.0, 1.0], salary: 4.5, stage: 3, special: Special::Cto },
    Job { id: "office", name: ("Office manager", "Office manager"), about: ("⚙ Achète les aménagements et organise les déménagements tout·e seul·e.", "⚙ Buys the fittings and organises the moves alone."), skills: [0.0; 5], salary: 1.3, stage: 1, special: Special::Office },
    Job { id: "assistant", name: ("Assistant·e de direction", "Executive assistant"), about: ("⚙ Répond aux imprévus à ta place quand ils attendent plus d'une minute et demie.", "⚙ Answers the surprises for you when they wait more than a minute and a half."), skills: [0.0; 5], salary: 1.2, stage: 1, special: Special::Assistant },
    Job { id: "po", name: ("Product owner", "Product owner"), about: ("⚙ Lance les produits tout·e seul·e quand la trésorerie le permet.", "⚙ Launches the products alone when the cash allows."), skills: [0.3, 0.4, 0.3, 0.3, 0.8], salary: 2.0, stage: 2, special: Special::Po },
];

pub fn job(id: &str) -> &'static Job {
    JOBS.iter().find(|j| j.id == id).unwrap_or(&JOBS[1])
}

pub struct Trait {
    pub id: &'static str,
    pub name: Tx,
    pub about: Tx,
}

pub const TRAITS: [Trait; 10] = [
    Trait { id: "perfectionist", name: ("Perfectionniste", "Perfectionist"), about: ("−10 % de vitesse, très peu de dette.", "−10 % speed, very little debt.") },
    Trait { id: "fast", name: ("Rapide", "Fast"), about: ("+15 % de vitesse, plus de dette.", "+15 % speed, more debt.") },
    Trait { id: "nightowl", name: ("Oiseau de nuit", "Night owl"), about: ("Le crunch ne lui fait rien.", "Crunch doesn't bother them.") },
    Trait { id: "mentor", name: ("Mentor", "Mentor"), about: ("Toute l'équipe gagne +20 % d'XP.", "The whole team gains +20 % XP.") },
    Trait { id: "grumpy", name: ("Râleur·se", "Grumpy"), about: ("Moral −10, et −2 pour les autres.", "Morale −10, and −2 for the others.") },
    Trait { id: "motivated", name: ("Motivé·e", "Motivated"), about: ("Moral +10.", "Morale +10.") },
    Trait { id: "geek", name: ("Geek", "Geek"), about: ("Recherche ×2 quand il·elle cherche.", "Research ×2 when researching.") },
    Trait { id: "social", name: ("Boute-en-train", "Life of the party"), about: ("Moral +3 pour toute l'équipe.", "Morale +3 for the whole team.") },
    Trait { id: "selftaught", name: ("Autodidacte", "Self-taught"), about: ("+40 % d'XP.", "+40 % XP.") },
    Trait { id: "remote", name: ("Nomade", "Nomad"), about: ("Adore le télétravail (+12 de moral en full remote), le déteste interdit.", "Loves remote work (+12 morale fully remote), hates it banned.") },
];

pub fn trait_def(id: &str) -> Option<&'static Trait> {
    TRAITS.iter().find(|t| t.id == id)
}

pub struct Office {
    pub name: Tx,
    pub price: f64,
    /// € a second.
    pub rent: f64,
    /// People, the founder too.
    pub capacity: usize,
    /// Fittings.
    pub slots: usize,
    /// Morale.
    pub comfort: f64,
}

pub const OFFICES: [Office; 7] = [
    Office { name: ("Le garage de tes parents", "Your parents' garage"), price: 0.0, rent: 0.0, capacity: 2, slots: 1, comfort: -6.0 },
    Office { name: ("Un coin de coworking", "A coworking corner"), price: 3_000.0, rent: 1.0, capacity: 5, slots: 2, comfort: 0.0 },
    Office { name: ("Petit bureau en ville", "Small office in town"), price: 30_000.0, rent: 5.0, capacity: 10, slots: 3, comfort: 3.0 },
    Office { name: ("Plateau en open space", "Open-space floor"), price: 250_000.0, rent: 25.0, capacity: 20, slots: 5, comfort: 5.0 },
    Office { name: ("Un étage entier", "A whole floor"), price: 2_000_000.0, rent: 90.0, capacity: 40, slots: 7, comfort: 7.0 },
    Office { name: ("Ton propre immeuble", "Your own building"), price: 15_000_000.0, rent: 300.0, capacity: 80, slots: 10, comfort: 9.0 },
    Office { name: ("La tour de verre", "The glass tower"), price: 120_000_000.0, rent: 1_000.0, capacity: 160, slots: 14, comfort: 12.0 },
];

pub struct Perk {
    pub id: &'static str,
    pub name: Tx,
    pub about: Tx,
    pub price: f64,
    /// € a second.
    pub upkeep: f64,
    pub morale: f64,
    pub prod: f64,
}

pub const PERKS: [Perk; 14] = [
    Perk { id: "plants", name: ("Plantes vertes", "Green plants"), about: ("Moral +3.", "Morale +3."), price: 200.0, upkeep: 0.0, morale: 3.0, prod: 0.0 },
    Perk { id: "coffee", name: ("Machine à café", "Coffee machine"), about: ("Moral +5, productivité +2 %.", "Morale +5, productivity +2 %."), price: 400.0, upkeep: 0.02, morale: 5.0, prod: 0.02 },
    Perk { id: "babyfoot", name: ("Baby-foot", "Foosball table"), about: ("Moral +7, productivité −2 %.", "Morale +7, productivity −2 %."), price: 900.0, upkeep: 0.0, morale: 7.0, prod: -0.02 },
    Perk { id: "screens", name: ("Écrans 4K", "4K screens"), about: ("Productivité +6 %.", "Productivity +6 %."), price: 3_000.0, upkeep: 0.0, morale: 1.0, prod: 0.06 },
    Perk { id: "chairs", name: ("Chaises ergonomiques", "Ergonomic chairs"), about: ("Moral +4, productivité +3 %.", "Morale +4, productivity +3 %."), price: 5_000.0, upkeep: 0.0, morale: 4.0, prod: 0.03 },
    Perk { id: "fiber", name: ("Fibre pro", "Business fibre"), about: ("Productivité +5 %.", "Productivity +5 %."), price: 8_000.0, upkeep: 0.2, morale: 1.0, prod: 0.05 },
    Perk { id: "library", name: ("Bibliothèque tech", "Tech library"), about: ("XP +30 %.", "XP +30 %."), price: 12_000.0, upkeep: 0.0, morale: 1.0, prod: 0.0 },
    Perk { id: "meeting", name: ("Salle de réunion", "Meeting room"), about: ("Les chef·fe·s de projet comptent double.", "Project managers count twice."), price: 18_000.0, upkeep: 0.0, morale: 0.0, prod: 0.0 },
    Perk { id: "nap", name: ("Salle de sieste", "Nap room"), about: ("Moral +8, le crunch pèse moitié moins.", "Morale +8, crunch weighs half as much."), price: 30_000.0, upkeep: 0.0, morale: 8.0, prod: 0.0 },
    Perk { id: "servers", name: ("Salle serveurs", "Server room"), about: ("Recherche +25 %, pannes −20 %.", "Research +25 %, outages −20 %."), price: 90_000.0, upkeep: 4.0, morale: 0.0, prod: 0.0 },
    Perk { id: "gym", name: ("Salle de sport", "Gym"), about: ("Moral +6, deux fois moins de démissions.", "Morale +6, half the resignations."), price: 150_000.0, upkeep: 3.0, morale: 6.0, prod: 0.0 },
    Perk { id: "chef", name: ("Chef cuisinier", "In-house chef"), about: ("Moral +10, productivité +4 %.", "Morale +10, productivity +4 %."), price: 400_000.0, upkeep: 20.0, morale: 10.0, prod: 0.04 },
    Perk { id: "arcade", name: ("Salle d'arcade", "Arcade room"), about: ("Moral +8, candidats meilleurs.", "Morale +8, better candidates."), price: 1_500_000.0, upkeep: 10.0, morale: 8.0, prod: 0.0 },
    Perk { id: "rooftop", name: ("Rooftop", "Rooftop"), about: ("Moral +12, réputation +20 %.", "Morale +12, reputation +20 %."), price: 8_000_000.0, upkeep: 60.0, morale: 12.0, prod: 0.0 },
];

pub fn perk(id: &str) -> Option<&'static Perk> {
    PERKS.iter().find(|p| p.id == id)
}

pub struct Tech {
    pub id: &'static str,
    pub name: Tx,
    pub about: Tx,
    /// Research points.
    pub cost: f64,
    pub needs: &'static [&'static str],
}

pub const TECHS: [Tech; 14] = [
    Tech { id: "git", name: ("Git et intégration continue", "Git and CI"), about: ("Livraisons +10 % de qualité, dette −15 %.", "Deliveries +10 % quality, debt −15 %."), cost: 250.0, needs: &[] },
    Tech { id: "framework", name: ("Framework maison", "In-house framework"), about: ("Productivité +10 %.", "Productivity +10 %."), cost: 900.0, needs: &["git"] },
    Tech { id: "cloud", name: ("Passage au cloud", "Moving to the cloud"), about: ("Permet de lancer des produits SaaS.", "Lets you launch SaaS products."), cost: 1_500.0, needs: &[] },
    Tech { id: "designsys", name: ("Design system", "Design system"), about: ("Front et Design +20 %.", "Front and Design +20 %."), cost: 3_000.0, needs: &["framework"] },
    Tech { id: "agile", name: ("Méthode agile", "Agile method"), about: ("Délais des contrats +30 %.", "Contract deadlines +30 %."), cost: 2_500.0, needs: &[] },
    Tech { id: "tests", name: ("Tests automatisés", "Automated tests"), about: ("Bugs des produits −40 %, qualité +10 %.", "Product bugs −40 %, quality +10 %."), cost: 5_000.0, needs: &["git"] },
    Tech { id: "crossplat", name: ("Cross-platform", "Cross-platform"), about: ("Mobile +30 %.", "Mobile +30 %."), cost: 9_000.0, needs: &["framework"] },
    Tech { id: "observability", name: ("Observabilité", "Observability"), about: ("Pannes −60 %.", "Outages −60 %."), cost: 15_000.0, needs: &["cloud"] },
    Tech { id: "microservices", name: ("Microservices", "Microservices"), about: ("Back +30 %, mais dette +20 %.", "Back +30 %, but debt +20 %."), cost: 25_000.0, needs: &["cloud"] },
    Tech { id: "opensource", name: ("Open source", "Open source"), about: ("Réputation +20 %, candidats d'un niveau de plus.", "Reputation +20 %, candidates one level up."), cost: 40_000.0, needs: &["git"] },
    Tech { id: "ai", name: ("IA générative", "Generative AI"), about: ("Data +30 %, débloque les produits d'IA.", "Data +30 %, unlocks the AI products."), cost: 80_000.0, needs: &["cloud"] },
    Tech { id: "autoscale", name: ("Auto-scaling", "Auto-scaling"), about: ("Utilisateurs max +30 %, un produit de plus.", "Max users +30 %, one more product."), cost: 200_000.0, needs: &["observability"] },
    Tech { id: "blockchain", name: ("Blockchain", "Blockchain"), about: ("Ne sert à rien. Mais la valorisation prend +5 %.", "Useless. But the valuation takes +5 %."), cost: 300_000.0, needs: &[] },
    Tech { id: "quantum", name: ("Ordinateur quantique", "Quantum computer"), about: ("Productivité +25 %.", "Productivity +25 %."), cost: 3_000_000.0, needs: &["ai"] },
];

pub fn tech(id: &str) -> Option<&'static Tech> {
    TECHS.iter().find(|t| t.id == id)
}

pub struct ContractKind {
    pub name: Tx,
    /// How the work splits between the skills.
    pub weights: [f64; 5],
    /// The lowest tier it comes at.
    pub tier: u8,
}

pub const CONTRACTS: [ContractKind; 12] = [
    ContractKind { name: ("Site vitrine", "Showcase website"), weights: [1.0, 0.1, 0.0, 0.0, 0.6], tier: 0 },
    ContractKind { name: ("Refonte graphique", "Visual redesign"), weights: [0.5, 0.0, 0.0, 0.0, 1.0], tier: 0 },
    ContractKind { name: ("API REST", "REST API"), weights: [0.0, 1.0, 0.0, 0.2, 0.0], tier: 0 },
    ContractKind { name: ("Boutique en ligne", "Online shop"), weights: [1.0, 0.9, 0.0, 0.0, 0.5], tier: 1 },
    ContractKind { name: ("Appli mobile", "Mobile app"), weights: [0.2, 0.4, 1.0, 0.0, 0.5], tier: 1 },
    ContractKind { name: ("Intranet", "Intranet"), weights: [0.6, 1.0, 0.0, 0.1, 0.2], tier: 1 },
    ContractKind { name: ("Tableau de bord", "Dashboard"), weights: [0.6, 0.4, 0.0, 1.0, 0.3], tier: 2 },
    ContractKind { name: ("Jeu mobile", "Mobile game"), weights: [0.1, 0.2, 1.0, 0.0, 1.0], tier: 2 },
    ContractKind { name: ("Migration cloud", "Cloud migration"), weights: [0.0, 1.0, 0.0, 0.3, 0.0], tier: 3 },
    ContractKind { name: ("Moteur de recommandation", "Recommendation engine"), weights: [0.0, 0.6, 0.0, 1.0, 0.0], tier: 3 },
    ContractKind { name: ("Chatbot IA", "AI chatbot"), weights: [0.3, 0.5, 0.0, 1.0, 0.2], tier: 4 },
    ContractKind { name: ("Appli bancaire", "Banking app"), weights: [0.5, 1.0, 0.8, 0.3, 0.5], tier: 5 },
];

/// The reputation each tier of contracts needs.
pub const TIERS: [f64; 10] = [0.0, 10.0, 40.0, 120.0, 350.0, 900.0, 2_200.0, 5_000.0, 11_000.0, 24_000.0];

pub struct ProductKind {
    pub name: Tx,
    pub about: Tx,
    pub weights: [f64; 5],
    /// The first version's work.
    pub build: f64,
    /// € to launch it.
    pub cost: f64,
    /// The users it can reach.
    pub market: f64,
    /// € a second per user.
    pub price: f64,
    pub tech: &'static str,
    pub stage: u8,
}

pub const PRODUCTS: [ProductKind; 8] = [
    ProductKind { name: ("Facturly", "Facturly"), about: ("La facturation pour freelances.", "Invoicing for freelancers."), weights: [0.6, 1.0, 0.0, 0.2, 0.4], build: 3_000.0, cost: 5_000.0, market: 4_000.0, price: 0.0025, tech: "cloud", stage: 0 },
    ProductKind { name: ("Randonnée+", "Hike+"), about: ("L'appli des sentiers.", "The trails app."), weights: [0.2, 0.4, 1.0, 0.0, 0.6], build: 6_000.0, cost: 15_000.0, market: 30_000.0, price: 0.0006, tech: "cloud", stage: 1 },
    ProductKind { name: ("ArtisanCRM", "TradesCRM"), about: ("Un CRM pour les artisans.", "A CRM for tradespeople."), weights: [0.6, 1.0, 0.2, 0.3, 0.4], build: 15_000.0, cost: 40_000.0, market: 15_000.0, price: 0.003, tech: "cloud", stage: 1 },
    ProductKind { name: ("Miaou", "Meow"), about: ("Le réseau social des chats. Gratuit, avec des pubs.", "The social network for cats. Free, with ads."), weights: [0.6, 0.6, 1.0, 0.2, 0.8], build: 40_000.0, cost: 120_000.0, market: 600_000.0, price: 0.0005, tech: "cloud", stage: 2 },
    ProductKind { name: ("NoCodeur", "NoCoder"), about: ("Fais des applis sans coder (presque).", "Make apps without code (almost)."), weights: [1.0, 1.0, 0.0, 0.2, 0.6], build: 120_000.0, cost: 500_000.0, market: 60_000.0, price: 0.02, tech: "cloud", stage: 2 },
    ProductKind { name: ("Résumator", "Summarizer"), about: ("L'IA qui résume tes réunions.", "The AI that sums up your meetings."), weights: [0.3, 0.6, 0.2, 1.0, 0.3], build: 400_000.0, cost: 2_000_000.0, market: 250_000.0, price: 0.02, tech: "ai", stage: 3 },
    ProductKind { name: ("Néobanque", "Neobank"), about: ("Une banque dans la poche.", "A bank in your pocket."), weights: [0.5, 1.0, 1.0, 0.5, 0.5], build: 2_000_000.0, cost: 15_000_000.0, market: 2_000_000.0, price: 0.01, tech: "microservices", stage: 4 },
    ProductKind { name: ("Ronnie OS", "Ronnie OS"), about: ("Un système d'exploitation. Rien que ça.", "An operating system. No less."), weights: [0.6, 1.0, 0.6, 0.6, 0.6], build: 12_000_000.0, cost: 100_000_000.0, market: 20_000_000.0, price: 0.006, tech: "quantum", stage: 5 },
];

pub struct Stage {
    pub name: Tx,
    /// The valuation that reaches it.
    pub valuation: f64,
}

pub const STAGES: [Stage; 7] = [
    Stage { name: ("Garage", "Garage"), valuation: 0.0 },
    Stage { name: ("Start-up", "Start-up"), valuation: 50_000.0 },
    Stage { name: ("PME", "Small business"), valuation: 500_000.0 },
    Stage { name: ("Scale-up", "Scale-up"), valuation: 5_000_000.0 },
    Stage { name: ("Grosse boîte", "Big company"), valuation: 60_000_000.0 },
    Stage { name: ("Licorne", "Unicorn"), valuation: 1_000_000_000.0 },
    Stage { name: ("En bourse", "Listed"), valuation: 15_000_000_000.0 },
];

/// The stage from which the company can be sold.
pub const SELL_STAGE: u8 = 2;

/// The rewards, by the furthest stage reached: the pointer's trail of bank notes (small business), the
/// Wall Street theme (big company), the unicorn galloping by (unicorn), the golden ticker frame (listed,
/// or a first sale).
pub const BILLS_STAGE: u8 = 2;
pub const WALL_STREET_STAGE: u8 = 4;
pub const UNICORN_STAGE: u8 = 5;
pub const LISTED_STAGE: u8 = 6;

/// The stage a valuation reaches.
pub fn stage_of(valuation: f64) -> u8 {
    STAGES.iter().rposition(|s| valuation >= s.valuation).unwrap_or(0) as u8
}
/// The tutorial over.
pub const TUTORIAL_DONE: u8 = 255;

const FIRST_NAMES: [&str; 64] = [
    "Léa", "Hugo", "Chloé", "Lucas", "Manon", "Nathan", "Inès", "Théo", "Camille", "Louis", "Sarah", "Enzo", "Jade", "Mathis", "Zoé", "Tom",
    "Emma", "Rayan", "Lina", "Noah", "Alice", "Yanis", "Clara", "Adam", "Julie", "Maxime", "Lou", "Karim", "Anaïs", "Bastien", "Nina", "Samir",
    "Margaux", "Kevin", "Elsa", "Quentin", "Yasmine", "Romain", "Mila", "Sacha", "Agathe", "Malik", "Juliette", "Florian", "Océane", "Bilal", "Pauline", "Arthur",
    "Fatou", "Grégoire", "Salomé", "Mehdi", "Élise", "Victor", "Sofia", "Jules", "Amandine", "Thibault", "Leila", "Antoine", "Maëlle", "Dylan", "Héloïse", "Kenji",
];
const INITIALS: &str = "ABCDEFGHJKLMNOPRSTVW";
const CLIENT_NAMES: [&str; 32] = [
    "Dupont", "Martin", "Lefèvre", "Moreau", "Girard", "Bonnet", "Roux", "Fontaine", "Chevalier", "Garnier", "Faure", "Blanc", "Guerin", "Muller", "Henry", "Perrin",
    "Morel", "Mercier", "Lemaire", "Vidal", "Picard", "Renard", "Brunet", "Colin", "Arnaud", "Rolland", "Aubert", "Leclerc", "Marchand", "Barbier", "Caron", "Gauthier",
];
const CLIENT_FORMS: [[&str; 4]; 4] = [
    ["Boulangerie {n}", "Garage {n}", "Cabinet {n}", "Fleurs {n}"],
    ["{n} & Fils", "Studio {n}", "Atelier {n}", "{n} Immobilier"],
    ["Groupe {n}", "{n} Industries", "{n} Labs", "{n} Logistique"],
    ["Banque {n}", "{n} International", "{n} Holding", "Assurances {n}"],
];

// ---------------------------------------------------------------------------------------------------
// The company.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Task {
    #[default]
    Contracts,
    Products,
    Research,
    Refactor,
}

pub const TASKS: [(Task, Tx); 4] = [
    (Task::Contracts, ("Contrats", "Contracts")),
    (Task::Products, ("Produits", "Products")),
    (Task::Research, ("Recherche", "Research")),
    (Task::Refactor, ("Refacto", "Refactoring")),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Employee {
    pub id: u32,
    pub name: String,
    pub job: String,
    pub level: u8,
    pub xp: f64,
    pub morale: f64,
    /// € a second, before the policies.
    pub salary: f64,
    /// At level 1 (each one a bit different).
    pub skills: [f64; 5],
    #[serde(default, rename = "trait", skip_serializing_if = "Option::is_none")]
    pub trait_id: Option<String>,
    #[serde(default)]
    pub task: Task,
}

impl Employee {
    pub fn job(&self) -> &'static Job {
        job(&self.job)
    }

    pub fn has(&self, id: &str) -> bool {
        self.trait_id.as_deref() == Some(id)
    }

    pub fn level_mult(&self) -> f64 {
        level_mult(self.level)
    }

    pub fn xp_next(&self) -> f64 {
        300.0 * 1.7f64.powi(self.level as i32 - 1)
    }

    /// Makes skill points (some jobs don't: sales, HR).
    pub fn makes(&self) -> bool {
        self.skills.iter().sum::<f64>() > 0.0
    }

    pub fn founder(&self) -> bool {
        self.job == "founder"
    }
}

pub fn level_mult(level: u8) -> f64 {
    1.25f64.powi(level as i32 - 1)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    pub id: u32,
    pub kind: u8,
    pub tier: u8,
    pub client: String,
    pub need: [f64; 5],
    pub done: [f64; 5],
    pub pay: f64,
    /// The time given (seconds).
    pub time: f64,
    /// Offered: until it goes; accepted: until the deadline (below 0: late).
    pub left: f64,
}

impl Contract {
    pub fn kind(&self) -> &'static ContractKind {
        &CONTRACTS[(self.kind as usize).min(CONTRACTS.len() - 1)]
    }

    pub fn progress(&self) -> f64 {
        let need: f64 = self.need.iter().sum();
        if need <= 0.0 { 1.0 } else { self.done.iter().sum::<f64>() / need }
    }

    pub fn finished(&self) -> bool {
        self.need.iter().zip(self.done).all(|(n, d)| d >= n - 1e-6)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Product {
    pub id: u32,
    pub kind: u8,
    /// 0: the first one being built.
    pub version: u32,
    /// The work toward the next version.
    pub progress: f64,
    pub users: f64,
    /// 0 to 100.
    pub bugs: f64,
    /// Seconds of outage left.
    pub down: f64,
}

impl Product {
    pub fn kind(&self) -> &'static ProductKind {
        &PRODUCTS[(self.kind as usize).min(PRODUCTS.len() - 1)]
    }

    /// The work the next version needs.
    pub fn next_work(&self) -> f64 {
        self.kind().build * 1.7f64.powi(self.version as i32)
    }

    /// The share of the market each version reaches.
    pub fn quality(&self) -> f64 {
        1.0 - 0.6f64.powi(self.version as i32)
    }

    /// The work a second its users need.
    pub fn upkeep(&self) -> f64 {
        self.users.powf(0.8) * 0.002
    }

    pub fn revenue(&self) -> f64 {
        if self.version == 0 || self.down > 0.0 { 0.0 } else { self.users * self.kind().price }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    /// 0: none, 1: hybrid, 2: fully remote.
    pub remote: u8,
    /// 0: relaxed, 1: normal, 2: crunch.
    pub hours: u8,
    /// 0: low, 1: market, 2: generous.
    pub salary: u8,
    /// 0: none, 1: meal vouchers, 2: everything.
    pub benefits: u8,
    /// 0 to 4.
    pub marketing: u8,
    /// 0 to 2.
    pub training: u8,
    /// The sales accept the offers by themselves.
    pub auto_accept: bool,
    /// What the other roles do by themselves, when there is one.
    #[serde(default)]
    pub auto: Auto,
}

impl Default for Policy {
    fn default() -> Self {
        Self { remote: 0, hours: 1, salary: 1, benefits: 0, marketing: 0, training: 0, auto_accept: true, auto: Auto::default() }
    }
}

/// What the roles do by themselves (each one only with someone in that role).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Auto {
    /// HR: hires into the free seats.
    pub hire: bool,
    /// Project managers: share out the tasks.
    pub tasks: bool,
    /// CTO: unlocks the techs.
    pub research: bool,
    /// Office manager: buys the fittings.
    pub fittings: bool,
    /// Office manager: moves when full.
    pub moving: bool,
    /// Product owner: launches the products.
    pub products: bool,
    /// Executive assistant: answers the events left waiting.
    pub events: bool,
}

impl Default for Auto {
    fn default() -> Self {
        Self { hire: true, tasks: true, research: true, fittings: true, moving: true, products: true, events: true }
    }
}

/// The assistant answers an event after this long (seconds).
pub const ASSISTANT_WAIT: f64 = 90.0;

/// A while of something better or worse, after an event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Boost {
    pub kind: BoostKind,
    pub mult: f64,
    pub left: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoostKind {
    Prod,
    Offers,
    Users,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NewsKind {
    Start,
    Hired,
    Quit,
    Fired,
    LevelUp,
    Delivered,
    Late,
    Lost,
    Launched,
    Version,
    Outage,
    Tech,
    Stage,
    Moved,
    Perk,
    Event,
    Away,
    Sold,
    Unpaid,
}

/// A line of the company's news.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct News {
    pub at: i64,
    pub kind: NewsKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub who: String,
    #[serde(default)]
    pub n: f64,
    #[serde(default)]
    pub m: f64,
    /// What it did to the cash (an event).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cash: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub contracts: u32,
    pub late: u32,
    pub hires: u32,
    pub earned: f64,
    pub launched: u32,
    pub events: u32,
}

/// What stays when the company is sold.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Legacy {
    /// Each one: productivity +5 %, pay +3 %, and more cash to start with.
    pub stars: u32,
    pub sales: u32,
    /// The best valuation ever reached.
    pub best: f64,
    /// The most a sale brought (the giant cheque's).
    #[serde(default)]
    pub best_sale: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corp {
    /// When it was last run to (unix seconds).
    pub at: i64,
    /// The player's actions, counted: on a sync, the side that did the most wins (the time running
    /// alone doesn't make a side win).
    pub actions: u64,
    pub rng: u64,
    pub cash: f64,
    pub rep: f64,
    pub office: u8,
    pub staff: Vec<Employee>,
    pub candidates: Vec<Employee>,
    pub candidates_in: f64,
    pub offers: Vec<Contract>,
    pub offer_in: f64,
    pub contracts: Vec<Contract>,
    pub products: Vec<Product>,
    pub techs: Vec<String>,
    /// Research points not spent.
    pub research: f64,
    pub perks: Vec<String>,
    pub policy: Policy,
    /// Technical debt, 0 to 100.
    pub debt: f64,
    /// A passing mood of the whole team (events), fading.
    pub mood: f64,
    pub boosts: Vec<Boost>,
    /// The event waiting for the player's choice, and whom it is about.
    pub event: Option<String>,
    #[serde(default)]
    pub event_who: u32,
    pub event_in: f64,
    /// How long the event has been waiting (the assistant answers it after `ASSISTANT_WAIT`).
    #[serde(default)]
    pub event_age: f64,
    /// The automations run again in.
    #[serde(default)]
    pub auto_in: f64,
    pub stage: u8,
    pub next_id: u32,
    /// What comes in and goes out a second, smoothed.
    pub income: f64,
    pub spend: f64,
    pub history: Vec<f32>,
    pub history_in: f64,
    pub news: Vec<News>,
    pub stats: Stats,
    pub legacy: Legacy,
    /// The tutorial's step (`TUTORIAL_DONE`: over, or skipped).
    #[serde(default)]
    pub tutorial: u8,
    /// The roles whose automations are paused (job id → seconds left, `PAUSED_UNTIL_RESUMED` until resumed).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub paused: std::collections::BTreeMap<String, f64>,
}

/// A role paused until the player resumes it.
pub const PAUSED_UNTIL_RESUMED: f64 = -1.0;

/// What the techs, fittings, policies, team and stars make of the work.
#[derive(Clone, Debug)]
pub struct Mods {
    pub prod: f64,
    pub skill: [f64; 5],
    pub quality: f64,
    pub debt: f64,
    pub bugs: f64,
    pub outage: f64,
    pub research: f64,
    pub xp: f64,
    pub deadline: f64,
    pub rep: f64,
    pub users: f64,
    pub offers: f64,
    pub pay: f64,
    pub lead: f64,
    pub morale: f64,
    pub quits: f64,
    pub candidate_level: u8,
}

/// The result of a step, for the news of a time away.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub secs: f64,
    pub earned: f64,
    pub spent: f64,
    pub delivered: u32,
    pub levels: u32,
    /// The catching up stopped before the red.
    pub stopped: bool,
}

impl Corp {
    pub fn new(now: i64, seed: u64, legacy: Legacy) -> Self {
        let mut c = Corp {
            at: now,
            actions: 0,
            rng: seed | 1,
            cash: 1_500.0 + 1_000.0 * legacy.stars as f64,
            rep: 0.0,
            office: 0,
            staff: Vec::new(),
            candidates: Vec::new(),
            candidates_in: 0.0,
            offers: Vec::new(),
            offer_in: 3.0,
            contracts: Vec::new(),
            products: Vec::new(),
            techs: Vec::new(),
            research: 0.0,
            perks: Vec::new(),
            policy: Policy::default(),
            debt: 0.0,
            mood: 0.0,
            boosts: Vec::new(),
            event: None,
            event_who: 0,
            event_in: 300.0,
            event_age: 0.0,
            auto_in: 0.0,
            stage: 0,
            next_id: 1,
            income: 0.0,
            spend: 0.0,
            history: Vec::new(),
            history_in: 0.0,
            news: Vec::new(),
            stats: Stats::default(),
            legacy,
            tutorial: 0,
            paused: Default::default(),
        };
        let founder = Employee { id: c.id(), name: String::new(), job: "founder".into(), level: 1, xp: 0.0, morale: 70.0, salary: 0.0, skills: JOBS[0].skills, trait_id: None, task: Task::Contracts };
        c.staff.push(founder);
        c.refresh_candidates();
        // A first offer at once, small.
        let first = c.make_contract(0);
        c.offers.push(first);
        c.say(NewsKind::Start, "", 0.0, 0.0);
        c
    }

    // --- randomness

    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// In [0, 1).
    pub fn rand(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn pick(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn chance(&mut self, p: f64) -> bool {
        self.rand() < p
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id - 1
    }

    // --- what is

    pub fn office(&self) -> &'static Office {
        &OFFICES[(self.office as usize).min(OFFICES.len() - 1)]
    }

    pub fn has_tech(&self, id: &str) -> bool {
        self.techs.iter().any(|t| t == id)
    }

    pub fn has_perk(&self, id: &str) -> bool {
        self.perks.iter().any(|p| p == id)
    }

    /// How many of a role are worth having (more do nothing more): None for the jobs with no such
    /// ceiling (the devs). Follows `mods` and `max_contracts`.
    pub fn role_useful(&self, special: Special) -> Option<usize> {
        let makers = self.staff.iter().filter(|e| e.makes()).count();
        let makers_no_qa = self.staff.iter().filter(|e| e.makes() && e.job().special != Special::Qa).count();
        Some(match special {
            Special::Sales => 5,
            Special::Hr => 3,
            Special::Lead => makers.div_ceil(6).max(1),
            Special::Qa => makers_no_qa.div_ceil(5).max(1),
            Special::Ops => 4,
            Special::Office | Special::Assistant | Special::Po | Special::Cto => 1,
            Special::None | Special::Founder => return None,
        })
    }

    pub fn count(&self, special: Special) -> usize {
        self.staff.iter().filter(|e| e.job().special == special).count()
    }

    pub fn sales(&self) -> usize {
        self.count(Special::Sales)
    }

    /// The people the office (and the remote work) takes.
    pub fn capacity(&self) -> usize {
        let base = self.office().capacity as f64;
        (base * [1.0, 1.25, 1.5][self.policy.remote.min(2) as usize]).floor() as usize
    }

    pub fn max_contracts(&self) -> usize {
        2 + self.count(Special::Lead) + self.stage as usize
    }

    pub fn max_products(&self) -> usize {
        1 + (self.stage as usize) / 2 + self.has_tech("autoscale") as usize
    }

    pub fn max_offers(&self) -> usize {
        3 + self.sales().min(3)
    }

    /// The best tier of contracts the reputation reaches.
    pub fn tier(&self) -> u8 {
        TIERS.iter().rposition(|&r| self.rep >= r).unwrap_or(0) as u8
    }

    /// A sum that means something for the company's size (events): about two minutes of its spending
    /// and one of its income.
    pub fn unit(&self) -> f64 {
        (self.spend * 120.0 + self.income * 60.0).max(300.0)
    }

    pub fn star_bonus(&self) -> f64 {
        self.legacy.stars as f64
    }

    pub fn mods(&self) -> Mods {
        let stars = self.star_bonus();
        let mut m = Mods {
            prod: 1.0 + 0.05 * stars,
            skill: [1.0; 5],
            quality: 1.0,
            debt: 1.0,
            bugs: 1.0,
            outage: 1.0,
            research: 1.0,
            xp: 1.0,
            deadline: 1.0,
            rep: 1.0,
            users: 1.0,
            offers: 1.0,
            pay: 1.0 + 0.03 * stars,
            lead: 1.0,
            morale: 0.0,
            quits: 1.0,
            candidate_level: 0,
        };
        for t in &self.techs {
            match t.as_str() {
                "git" => {
                    m.quality += 0.1;
                    m.debt *= 0.85;
                }
                "framework" => m.prod += 0.1,
                "designsys" => {
                    m.skill[0] += 0.2;
                    m.skill[4] += 0.2;
                }
                "agile" => m.deadline += 0.3,
                "tests" => {
                    m.bugs *= 0.6;
                    m.quality += 0.1;
                }
                "crossplat" => m.skill[2] += 0.3,
                "observability" => m.outage *= 0.4,
                "microservices" => {
                    m.skill[1] += 0.3;
                    m.debt *= 1.2;
                }
                "opensource" => {
                    m.rep += 0.2;
                    m.candidate_level += 1;
                }
                "ai" => m.skill[3] += 0.3,
                "autoscale" => m.users += 0.3,
                "quantum" => m.prod += 0.25,
                _ => {}
            }
        }
        for p in self.perks.iter().filter_map(|p| perk(p)) {
            m.prod += p.prod;
            m.morale += p.morale * 0.6;
            match p.id {
                "library" => m.xp += 0.3,
                "meeting" => m.lead = 2.0,
                "servers" => {
                    m.research += 0.25;
                    m.outage *= 0.8;
                }
                "gym" => m.quits *= 0.5,
                "arcade" => m.candidate_level += 1,
                "rooftop" => m.rep += 0.2,
                _ => {}
            }
        }
        let p = &self.policy;
        m.prod *= [0.85, 1.0, 1.25][p.hours.min(2) as usize];
        m.prod *= [1.0, 1.0, 0.95][p.remote.min(2) as usize];
        m.debt *= [0.7, 1.0, 2.0][p.hours.min(2) as usize];
        m.xp *= [1.0, 1.5, 2.2][p.training.min(2) as usize];
        m.offers *= 1.0 + 0.25 * p.marketing as f64;
        m.users *= 1.0 + 0.3 * p.marketing as f64;
        if p.salary == 2 {
            m.candidate_level += 1;
        }
        // The team's specials.
        let (qa, ops, sales, hr) = (self.count(Special::Qa), self.count(Special::Ops), self.sales(), self.count(Special::Hr));
        let makers = self.staff.iter().filter(|e| e.makes() && e.job().special != Special::Qa).count().max(1);
        m.quality += 0.25 * (qa as f64 * 5.0 / makers as f64).min(1.0);
        m.bugs *= 1.0 - 0.5 * (qa as f64 * 5.0 / makers as f64).min(1.0);
        m.outage *= 1.0 - (ops as f64 * 0.15).min(0.6);
        m.offers *= 1.0 + 0.4 * sales.min(5) as f64;
        m.pay *= 1.0 + 0.05 * sales.min(5) as f64;
        m.morale = m.morale.min(20.0) + (hr as f64 * 3.0).min(12.0);
        if hr > 0 {
            m.candidate_level += 1;
        }
        if self.count(Special::Cto) > 0 {
            m.research *= 1.5;
        }
        for b in &self.boosts {
            match b.kind {
                BoostKind::Prod => m.prod *= b.mult,
                BoostKind::Offers => m.offers *= b.mult,
                BoostKind::Users => m.users *= b.mult,
            }
        }
        m
    }

    /// The salaries, the rent, the fittings' upkeep, the policies: € a second.
    pub fn costs(&self) -> f64 {
        let p = &self.policy;
        let salaries: f64 = self.staff.iter().map(|e| e.salary).sum();
        let salaries = salaries * [0.8, 1.0, 1.25][p.salary.min(2) as usize] * (1.0 + [0.0, 0.06, 0.15][p.benefits.min(2) as usize] + [0.0, 0.05, 0.12][p.training.min(2) as usize]);
        let upkeep: f64 = self.perks.iter().filter_map(|p| perk(p)).map(|p| p.upkeep).sum();
        salaries + self.office().rent + upkeep + self.marketing_cost()
    }

    pub fn marketing_cost(&self) -> f64 {
        [0.0, 0.5, 3.0, 15.0, 80.0][self.policy.marketing.min(4) as usize] * (1.0 + self.stage as f64).powi(2)
    }

    pub fn product_revenue(&self) -> f64 {
        self.products.iter().map(Product::revenue).sum::<f64>() * self.mods().pay
    }

    pub fn valuation(&self) -> f64 {
        let profit = (self.income - self.spend).max(0.0);
        let team: f64 = self.staff.iter().map(|e| 1_500.0 * e.level_mult()).sum();
        let techs: f64 = self.techs.iter().filter_map(|t| tech(t)).map(|t| t.cost * 2.0).sum();
        let offices: f64 = OFFICES[..=self.office as usize].iter().map(|o| o.price * 0.5).sum();
        let v = self.cash.max(0.0) + profit * 6_000.0 + self.product_revenue() * 12_000.0 + self.rep * 200.0 + team + techs + offices;
        if self.has_tech("blockchain") { v * 1.05 } else { v }
    }

    /// The furthest stage ever reached, sales included (what the rewards go by).
    pub fn best_stage(&self) -> u8 {
        stage_of(self.legacy.best.max(self.valuation())).max(self.stage)
    }

    /// The stars selling it now brings.
    pub fn sell_stars(&self) -> u32 {
        if self.stage < SELL_STAGE {
            return 0;
        }
        ((self.valuation() / 100_000.0).powf(0.45)).floor().max(1.0) as u32
    }

    // --- what the player does (each one counted)

    fn acted(&mut self) {
        self.actions += 1;
    }

    pub fn hire_fee(&self, e: &Employee) -> f64 {
        e.salary * 240.0
    }

    pub fn hire(&mut self, candidate: usize) -> bool {
        let Some(e) = self.candidates.get(candidate) else { return false };
        let fee = self.hire_fee(e);
        if self.staff.len() >= self.capacity() || self.cash < fee || (e.job == "cto" && self.count(Special::Cto) > 0) {
            return false;
        }
        let e = self.candidates.remove(candidate);
        self.cash -= fee;
        self.stats.hires += 1;
        self.say(NewsKind::Hired, &e.name.clone(), e.level as f64, JOBS.iter().position(|j| j.id == e.job).unwrap_or(0) as f64);
        self.staff.push(e);
        self.acted();
        true
    }

    /// Fired: a month's salary in compensation (none for an intern).
    pub fn fire(&mut self, id: u32) -> bool {
        let Some(k) = self.staff.iter().position(|e| e.id == id && !e.founder()) else { return false };
        let e = self.staff.remove(k);
        self.cash -= e.salary * 60.0;
        self.mood -= 4.0;
        self.say(NewsKind::Fired, &e.name, 0.0, 0.0);
        self.acted();
        true
    }

    /// A role's automations stopped for `secs` (`PAUSED_UNTIL_RESUMED`: until resumed), or resumed (`None`).
    pub fn pause_role(&mut self, job: &str, secs: Option<f64>) {
        match secs {
            Some(s) => self.paused.insert(job.to_owned(), s),
            None => self.paused.remove(job),
        };
        self.acted();
    }

    /// The role's automations run (someone in it, not paused).
    fn works(&self, special: Special) -> bool {
        self.staff.iter().any(|e| e.job().special == special && !self.paused.contains_key(&e.job))
    }

    pub fn set_task(&mut self, id: u32, task: Task) {
        if let Some(e) = self.staff.iter_mut().find(|e| e.id == id)
            && e.task != task
        {
            e.task = task;
            self.acted();
        }
    }

    /// The candidates again (the ad posted again): some money.
    pub fn repost_cost(&self) -> f64 {
        200.0 * (1.0 + self.stage as f64).powi(3)
    }

    pub fn repost(&mut self) -> bool {
        let cost = self.repost_cost();
        if self.cash < cost {
            return false;
        }
        self.cash -= cost;
        self.refresh_candidates();
        self.acted();
        true
    }

    pub fn accept(&mut self, offer: usize) -> bool {
        if offer >= self.offers.len() || self.contracts.len() >= self.max_contracts() {
            return false;
        }
        let mut c = self.offers.remove(offer);
        c.left = c.time;
        self.contracts.push(c);
        self.acted();
        true
    }

    pub fn decline(&mut self, offer: usize) {
        if offer < self.offers.len() {
            self.offers.remove(offer);
            self.acted();
        }
    }

    /// Up (−1) or down (+1) in the order the contracts are worked.
    pub fn move_contract(&mut self, k: usize, by: isize) {
        let to = k as isize + by;
        if k < self.contracts.len() && to >= 0 && (to as usize) < self.contracts.len() {
            self.contracts.swap(k, to as usize);
            self.acted();
        }
    }

    /// Given up: the reputation suffers.
    pub fn drop_contract(&mut self, k: usize) {
        if k < self.contracts.len() {
            let c = self.contracts.remove(k);
            self.rep = (self.rep - 2.0 * (1.0 + c.tier as f64)).max(0.0);
            self.say(NewsKind::Lost, &c.client, 0.0, c.kind as f64);
            self.acted();
        }
    }

    pub fn can_move(&self) -> bool {
        let next = self.office as usize + 1;
        next < OFFICES.len() && self.cash >= OFFICES[next].price
    }

    pub fn move_office(&mut self) -> bool {
        if !self.can_move() {
            return false;
        }
        self.office += 1;
        self.cash -= self.office().price;
        self.say(NewsKind::Moved, "", self.office as f64, 0.0);
        self.acted();
        true
    }

    pub fn can_buy_perk(&self, id: &str) -> bool {
        perk(id).is_some_and(|p| !self.has_perk(id) && self.perks.len() < self.office().slots && self.cash >= p.price)
    }

    pub fn buy_perk(&mut self, id: &str) -> bool {
        if !self.can_buy_perk(id) {
            return false;
        }
        let p = perk(id).expect("checked");
        self.cash -= p.price;
        self.perks.push(p.id.to_owned());
        self.say(NewsKind::Perk, "", PERKS.iter().position(|q| q.id == id).unwrap_or(0) as f64, 0.0);
        self.acted();
        true
    }

    /// Sold back (half its price), to make room.
    pub fn sell_perk(&mut self, id: &str) {
        if let Some(k) = self.perks.iter().position(|p| p == id) {
            self.perks.remove(k);
            self.cash += perk(id).map_or(0.0, |p| p.price * 0.3);
            self.acted();
        }
    }

    pub fn can_research(&self, id: &str) -> bool {
        tech(id).is_some_and(|t| !self.has_tech(id) && self.research >= t.cost && t.needs.iter().all(|n| self.has_tech(n)))
    }

    pub fn learn(&mut self, id: &str) -> bool {
        if !self.can_research(id) {
            return false;
        }
        let t = tech(id).expect("checked");
        self.research -= t.cost;
        self.techs.push(t.id.to_owned());
        self.say(NewsKind::Tech, "", TECHS.iter().position(|x| x.id == id).unwrap_or(0) as f64, 0.0);
        self.acted();
        true
    }

    pub fn can_launch(&self, kind: usize) -> bool {
        PRODUCTS.get(kind).is_some_and(|p| {
            self.has_tech(p.tech) && self.stage >= p.stage && self.cash >= p.cost && self.products.len() < self.max_products() && !self.products.iter().any(|q| q.kind as usize == kind)
        })
    }

    pub fn launch(&mut self, kind: usize) -> bool {
        if !self.can_launch(kind) {
            return false;
        }
        self.cash -= PRODUCTS[kind].cost;
        let id = self.id();
        self.products.push(Product { id, kind: kind as u8, version: 0, progress: 0.0, users: 0.0, bugs: 0.0, down: 0.0 });
        self.acted();
        true
    }

    /// Closed down: its users leave.
    pub fn close_product(&mut self, id: u32) {
        if let Some(k) = self.products.iter().position(|p| p.id == id) {
            self.products.remove(k);
            self.acted();
        }
    }

    pub fn set_policy(&mut self, policy: Policy) {
        if policy != self.policy {
            self.policy = policy;
            self.acted();
        }
    }

    /// The choice made for the event waiting: what came of it (its outcome's index).
    pub fn choose(&mut self, choice: usize) -> Option<usize> {
        let id = self.event.take()?;
        let def = EVENTS.iter().find(|e| e.id == id)?;
        let c = def.choices.get(choice)?;
        let who = self.staff.iter().find(|e| e.id == self.event_who).map(|e| e.name.clone()).unwrap_or_default();
        let before = self.cash;
        let outcome = (c.apply)(self);
        let delta = self.cash - before;
        self.stats.events += 1;
        let k = EVENTS.iter().position(|e| e.id == id).unwrap_or(0);
        self.say(NewsKind::Event, &who, k as f64, (choice * 10 + outcome) as f64);
        if let Some(n) = self.news.last_mut() {
            n.cash = delta;
        }
        self.acted();
        Some(outcome)
    }

    /// Sold: the stars it brings added, and everything starts again in the garage.
    pub fn sell(&mut self, now: i64) -> u32 {
        let stars = self.sell_stars();
        if stars == 0 {
            return 0;
        }
        let mut legacy = self.legacy.clone();
        legacy.stars += stars;
        legacy.sales += 1;
        legacy.best = legacy.best.max(self.valuation());
        legacy.best_sale = legacy.best_sale.max(self.valuation());
        let actions = self.actions + 1;
        let tutorial = self.tutorial;
        let seed = self.next();
        let value = self.valuation();
        *self = Corp::new(now, seed, legacy);
        self.actions = actions;
        self.tutorial = tutorial;
        self.say(NewsKind::Sold, "", value, stars as f64);
        stars
    }

    // --- the news

    fn say(&mut self, kind: NewsKind, who: &str, n: f64, m: f64) {
        self.news.push(News { at: self.at, kind, who: who.to_owned(), n, m, cash: 0.0 });
        if self.news.len() > NEWS {
            self.news.remove(0);
        }
    }

    // --- making things up

    fn make_name(&mut self) -> String {
        let first = FIRST_NAMES[self.pick(FIRST_NAMES.len())];
        let initial = INITIALS.chars().nth(self.pick(INITIALS.len())).unwrap_or('A');
        format!("{first} {initial}.")
    }

    fn make_employee(&mut self, job_id: &str, level: u8) -> Employee {
        let j = job(job_id);
        let skills = j.skills.map(|s| s * (0.8 + 0.4 * self.rand()));
        let trait_id = self.chance(0.45).then(|| TRAITS[self.pick(TRAITS.len())].id.to_owned());
        let salary = j.salary * 1.15f64.powi(level as i32 - 1) * (0.9 + 0.2 * self.rand());
        let task = if j.special == Special::Qa { Task::Refactor } else { Task::Contracts };
        Employee { id: self.id(), name: self.make_name(), job: j.id.to_owned(), level, xp: 0.0, morale: 65.0 + 10.0 * self.rand(), salary, skills, trait_id, task }
    }

    pub fn refresh_candidates(&mut self) {
        let level_bonus = self.mods().candidate_level;
        let jobs: Vec<&'static str> = JOBS.iter().filter(|j| j.stage <= self.stage).map(|j| j.id).collect();
        let n = 3 + self.count(Special::Hr).min(3);
        let mut list = Vec::new();
        for _ in 0..n {
            let id = jobs[self.pick(jobs.len())];
            let top = (1 + self.stage + level_bonus).min(MAX_LEVEL);
            let level = 1 + self.pick(top as usize) as u8;
            list.push(self.make_employee(id, level.min(MAX_LEVEL)));
        }
        self.candidates = list;
        self.candidates_in = 300.0;
    }

    fn make_contract(&mut self, tier: u8) -> Contract {
        let kinds: Vec<usize> = (0..CONTRACTS.len()).filter(|&k| CONTRACTS[k].tier <= tier).collect();
        let kind = kinds[self.pick(kinds.len())];
        let size = 60.0 * 2.6f64.powi(tier as i32) * (0.7 + 0.6 * self.rand());
        let w = CONTRACTS[kind].weights;
        let total: f64 = w.iter().sum();
        let need = w.map(|x| (size * x / total).round());
        let per_point = 0.6 * 1.4f64.powi(tier as i32) * self.mods().pay;
        let pay = (need.iter().sum::<f64>() * per_point * (0.85 + 0.3 * self.rand())).round();
        let time = 600.0 * 1.2f64.powi(tier as i32) * self.mods().deadline;
        let forms = CLIENT_FORMS[((tier as usize) / 3).min(CLIENT_FORMS.len() - 1)];
        let form = forms[self.pick(forms.len())];
        let client = form.replace("{n}", CLIENT_NAMES[self.pick(CLIENT_NAMES.len())]);
        Contract { id: self.id(), kind: kind as u8, tier, client, need, done: [0.0; 5], pay, time, left: 900.0 }
    }

    /// An offer, of the reputation's tier or below.
    fn new_offer(&mut self) {
        let top = self.tier();
        // Mostly the best tier, now and then a smaller one.
        let tier = if top > 0 && self.chance(0.35) { top - 1 - self.pick(top.min(2) as usize) as u8 } else { top };
        let c = self.make_contract(tier.min(top));
        self.offers.push(c);
    }

    // --- running

    /// Runs the company `dt` seconds (`online`: Ronnie is open; else being caught up: no events nor
    /// resignations).
    pub fn step(&mut self, dt: f64, online: bool, sum: &mut Summary) {
        if dt <= 0.0 {
            return;
        }
        let m = self.mods();
        let p = self.policy.clone();
        let unpaid = self.cash < 0.0;

        // The morale.
        let social = self.staff.iter().filter(|e| e.has("social")).count() as f64 * 3.0;
        let grumpy = self.staff.iter().filter(|e| e.has("grumpy")).count() as f64 * 2.0;
        let crunch = if p.hours == 2 { if self.has_perk("nap") { 10.0 } else { 20.0 } } else { 0.0 };
        let base = 48.0 + self.office().comfort + m.morale + social.min(9.0) - grumpy.min(8.0) + self.mood
            + [-15.0, 0.0, 10.0][p.salary.min(2) as usize]
            + [0.0, 5.0, 10.0][p.benefits.min(2) as usize]
            + [8.0, 0.0, 0.0][p.hours.min(2) as usize]
            + [0.0, 4.0, 6.0][p.remote.min(2) as usize]
            - if unpaid { 40.0 } else { 0.0 }
            - (self.debt / 10.0);
        let k = 1.0 - (-dt / 120.0).exp();
        for e in &mut self.staff {
            let mut target = base;
            if e.has("grumpy") {
                target -= 8.0; // −10 for them, the −2 of their own counted above.
            }
            if e.has("motivated") {
                target += 10.0;
            }
            if e.has("nightowl") {
                target += crunch;
            }
            if e.has("remote") {
                target += [-6.0, 0.0, 12.0][p.remote.min(2) as usize];
            }
            target -= crunch;
            if e.founder() {
                target = target.max(40.0);
            }
            e.morale += (target.clamp(0.0, 100.0) - e.morale) * k;
        }
        self.mood *= (-dt / 900.0).exp();

        // The work, by task.
        let makers = self.staff.iter().filter(|e| e.makes()).count();
        let leads = self.count(Special::Lead) as f64 * 6.0 * m.lead;
        let lead_bonus = 1.0 + 0.15 * (leads / makers.max(1) as f64).min(1.0);
        let debt_drag = 1.0 - self.debt / 250.0;
        let mut pools: [[f64; 5]; 4] = [[0.0; 5]; 4];
        let mut debt_up = 0.0;
        let mut debt_down = 0.0;
        let mut research = 0.0;
        for e in &self.staff {
            let mut f = m.prod * (0.5 + e.morale / 100.0) * lead_bonus * debt_drag * e.level_mult();
            if e.has("fast") {
                f *= 1.15;
            }
            if e.has("perfectionist") {
                f *= 0.9;
            }
            let out: [f64; 5] = std::array::from_fn(|s| e.skills[s] * m.skill[s] * f);
            let task = if e.makes() { e.task } else { Task::Contracts };
            let total: f64 = out.iter().sum();
            match task {
                Task::Contracts | Task::Products => {
                    let pool = &mut pools[if task == Task::Contracts { 0 } else { 1 }];
                    for s in 0..5 {
                        pool[s] += out[s] * dt;
                    }
                    if e.makes() && e.job().special != Special::Qa {
                        let trait_debt = if e.has("fast") { 1.6 } else if e.has("perfectionist") { 0.3 } else { 1.0 };
                        debt_up += 0.006 * m.debt * trait_debt;
                    }
                }
                Task::Research => research += total * 0.5 * if e.has("geek") { 2.0 } else { 1.0 } * dt,
                Task::Refactor => debt_down += 0.03 * (1.0 + 0.15 * (e.level as f64 - 1.0)) * e.morale.max(20.0) / 70.0,
            }
            match e.job().special {
                Special::Ops => debt_down += 0.01,
                Special::Cto => debt_down += 0.03,
                _ => {}
            }
        }
        self.debt = (self.debt + (debt_up - debt_down) * dt).clamp(0.0, 100.0);

        // The contracts, in order: each skill's points to the first needing it.
        let mut pool = pools[0];
        for c in &mut self.contracts {
            for ((done, need), left) in c.done.iter_mut().zip(c.need).zip(pool.iter_mut()) {
                let take = (need - *done).max(0.0).min(*left);
                *done += take;
                *left -= take;
            }
            c.left -= dt;
        }
        // What nobody needed: a bit of research (the team keeps up to date).
        research += pool.iter().sum::<f64>() * 0.2;
        self.research += research * m.research;

        let mut k = 0;
        while k < self.contracts.len() {
            let c = &self.contracts[k];
            if c.finished() {
                let c = self.contracts.remove(k);
                let late = c.left < 0.0;
                let quality = (m.quality - self.debt / 150.0 - if p.hours == 2 { 0.1 } else { 0.0 }).clamp(0.3, 1.5);
                let pay = c.pay * if late { 0.5 } else { 1.0 } * (0.8 + 0.2 * quality);
                self.cash += pay;
                sum.earned += pay;
                self.stats.earned += pay;
                self.stats.contracts += 1;
                sum.delivered += 1;
                let rep = (1.0 + c.tier as f64) * quality * m.rep;
                if late {
                    self.stats.late += 1;
                    self.rep = (self.rep - rep * 0.5).max(0.0);
                    self.say(NewsKind::Late, &c.client, pay, c.kind as f64);
                } else {
                    self.rep += rep;
                    self.say(NewsKind::Delivered, &c.client, pay, c.kind as f64);
                }
                self.cash_in(pay, dt);
                continue;
            }
            // Twice the time: the client leaves.
            if c.left < -c.time {
                let c = self.contracts.remove(k);
                self.rep = (self.rep - 2.0 * (1.0 + c.tier as f64)).max(0.0);
                self.say(NewsKind::Lost, &c.client, 0.0, c.kind as f64);
                continue;
            }
            k += 1;
        }

        // The products: each one an equal share of the product team's points.
        let n = self.products.len();
        let mut revenue = 0.0;
        if n > 0 {
            let share = pools[1].map(|x| x / n as f64);
            let rolls: Vec<f64> = (0..n).map(|_| self.rand()).collect();
            let mut said = Vec::new();
            for (pr, roll) in self.products.iter_mut().zip(rolls) {
                let kind = pr.kind();
                let mut points: f64 = (0..5).map(|s| share[s] * kind.weights[s]).sum();
                if pr.version > 0 {
                    // Its users' upkeep first.
                    let upkeep = pr.upkeep() * dt;
                    let kept = if upkeep > 0.0 { (points / upkeep).min(1.0) } else { 1.0 };
                    points = (points - upkeep).max(0.0);
                    let worse = (1.0 - kept) * 0.08 * m.bugs;
                    let better = kept * 0.03;
                    pr.bugs = (pr.bugs + (worse - better) * dt + self.debt / 100.0 * 0.01 * m.bugs * dt).clamp(0.0, 100.0);
                    // Toward the market its version reaches, at the marketing's pace.
                    let target = kind.market * pr.quality() * m.users * (1.0 - pr.bugs / 200.0);
                    let rate = if target > pr.users { 0.002 * (1.0 + 0.3 * p.marketing as f64) } else { 0.004 };
                    pr.users += (target - pr.users) * (1.0 - (-rate * dt).exp());
                    pr.users -= pr.users * pr.bugs / 100.0 * 0.0005 * dt;
                    if pr.down > 0.0 {
                        pr.down -= dt;
                        pr.users -= pr.users * 0.0008 * dt;
                    } else {
                        let p_out = pr.bugs / 100.0 * 0.004 * m.outage * dt;
                        if roll < p_out {
                            pr.down = 90.0 + 120.0 * pr.bugs / 100.0;
                            said.push((NewsKind::Outage, pr.kind as f64));
                        }
                    }
                    pr.users = pr.users.max(0.0);
                    revenue += pr.revenue() * dt;
                }
                pr.progress += points;
                if pr.progress >= pr.next_work() {
                    pr.progress -= pr.next_work();
                    pr.version += 1;
                    pr.bugs = (pr.bugs - 20.0).max(0.0);
                    said.push((if pr.version == 1 { NewsKind::Launched } else { NewsKind::Version }, pr.kind as f64 + pr.version as f64 * 100.0));
                }
            }
            for (kind, n) in said {
                if kind == NewsKind::Launched {
                    self.stats.launched += 1;
                }
                self.say(kind, "", n.rem_euclid(100.0), (n / 100.0).floor());
            }
        }
        let revenue = revenue * m.pay;
        self.cash += revenue;
        sum.earned += revenue;
        self.stats.earned += revenue;
        self.cash_in(revenue, dt);

        // The costs.
        let costs = self.costs() * dt;
        self.cash -= costs;
        sum.spent += costs;
        let a = 1.0 - (-dt / 300.0).exp();
        self.spend += (costs / dt - self.spend) * a;
        self.income *= 1.0 - a;

        // The experience.
        let mentor = if self.staff.iter().any(|e| e.has("mentor")) { 1.2 } else { 1.0 };
        let mut ups = Vec::new();
        for e in &mut self.staff {
            if e.level >= MAX_LEVEL {
                continue;
            }
            e.xp += dt * m.xp * mentor * if e.has("selftaught") { 1.4 } else { 1.0 } * (0.5 + e.morale / 100.0);
            while e.level < MAX_LEVEL && e.xp >= e.xp_next() {
                e.xp -= e.xp_next();
                e.level += 1;
                e.salary *= 1.15;
                ups.push((e.name.clone(), e.level, e.founder()));
            }
        }
        for (name, level, founder) in ups {
            sum.levels += 1;
            self.say(NewsKind::LevelUp, if founder { "" } else { &name }, level as f64, 0.0);
        }

        // The offers and candidates.
        for o in &mut self.offers {
            o.left -= dt;
        }
        self.offers.retain(|o| o.left > 0.0);
        self.offer_in -= dt * m.offers;
        while self.offer_in <= 0.0 {
            if self.offers.len() < self.max_offers() {
                self.new_offer();
            }
            self.offer_in += 75.0 * (0.7 + 0.6 * self.rand());
        }
        if p.auto_accept && self.works(Special::Sales) {
            while !self.offers.is_empty() && self.contracts.len() < self.max_contracts() {
                let mut c = self.offers.remove(0);
                c.left = c.time;
                self.contracts.push(c);
            }
        }
        for left in self.paused.values_mut() {
            if *left > 0.0 {
                *left = (*left - dt).max(0.0);
            }
        }
        self.paused.retain(|_, left| *left == PAUSED_UNTIL_RESUMED || *left > 0.0);
        self.auto_in -= dt;
        if self.auto_in <= 0.0 {
            self.auto_in = 10.0;
            self.automate();
        }
        self.candidates_in -= dt;
        if self.candidates_in <= 0.0 {
            self.refresh_candidates();
        }
        for b in &mut self.boosts {
            b.left -= dt;
        }
        self.boosts.retain(|b| b.left > 0.0);

        if online {
            // Resignations.
            let mut gone = Vec::new();
            for e in self.staff.iter().filter(|e| !e.founder() && e.morale < 25.0) {
                gone.push((e.id, 0.0004 * (25.0 - e.morale) / 25.0 * m.quits * dt));
            }
            for (id, p) in gone {
                if self.chance(p)
                    && let Some(k) = self.staff.iter().position(|e| e.id == id)
                {
                    let e = self.staff.remove(k);
                    self.say(if unpaid { NewsKind::Unpaid } else { NewsKind::Quit }, &e.name, 0.0, 0.0);
                }
            }
            // The events.
            if self.event.is_none() {
                self.event_in -= dt;
                if self.event_in <= 0.0 {
                    self.raise_event();
                    self.event_in = 240.0 + 300.0 * self.rand();
                }
            } else {
                self.event_age += dt;
                if self.policy.auto.events && self.works(Special::Assistant) && self.event_age >= ASSISTANT_WAIT {
                    let actions = self.actions;
                    self.choose(0);
                    self.actions = actions;
                }
            }
        }

        // The stage.
        let v = self.valuation();
        while (self.stage as usize) + 1 < STAGES.len() && v >= STAGES[self.stage as usize + 1].valuation {
            self.stage += 1;
            self.say(NewsKind::Stage, "", self.stage as f64, 0.0);
        }
        self.legacy.best = self.legacy.best.max(v);

        self.history_in -= dt;
        if self.history_in <= 0.0 {
            self.history.push(self.cash as f32);
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
            self.history_in += 60.0;
        }
    }

    fn cash_in(&mut self, amount: f64, dt: f64) {
        let a = 1.0 - (-dt / 300.0).exp();
        self.income += amount / dt * a;
    }

    /// The cash the automations never spend: ten minutes of spending.
    pub fn reserve(&self) -> f64 {
        (self.costs() * 600.0).max(500.0)
    }

    /// Someone in the role.
    pub fn has_role(&self, special: Special) -> bool {
        self.count(special) > 0
    }

    /// What the roles do by themselves (not counted as the player's actions).
    fn automate(&mut self) {
        let actions = self.actions;
        let a = self.policy.auto.clone();
        let reserve = self.reserve();
        if a.hire && self.works(Special::Hr) {
            self.auto_hire(reserve);
        }
        if a.tasks && self.works(Special::Lead) {
            self.auto_tasks();
        }
        if a.research && self.works(Special::Cto) {
            let next = TECHS.iter().filter(|t| self.can_research(t.id)).min_by(|x, y| x.cost.total_cmp(&y.cost)).map(|t| t.id);
            if let Some(id) = next {
                self.learn(id);
            }
        }
        if self.works(Special::Office) {
            let next = OFFICES.get(self.office as usize + 1);
            if a.moving && self.staff.len() >= self.capacity() && next.is_some_and(|o| self.cash >= o.price + reserve * 2.0) {
                self.move_office();
            }
            if a.fittings && self.perks.len() < self.office().slots {
                let cash = self.cash;
                let next = PERKS.iter().filter(|p| !self.has_perk(p.id) && cash >= p.price * 2.0 + reserve).min_by(|x, y| x.price.total_cmp(&y.price)).map(|p| p.id);
                if let Some(id) = next {
                    self.buy_perk(id);
                }
            }
        }
        if a.products && self.works(Special::Po) {
            let cash = self.cash;
            let next = (0..PRODUCTS.len()).filter(|&k| self.can_launch(k) && cash >= PRODUCTS[k].cost + reserve * 2.0).min_by(|&x, &y| PRODUCTS[x].cost.total_cmp(&PRODUCTS[y].cost));
            if let Some(k) = next {
                self.launch(k);
            }
        }
        self.actions = actions;
    }

    /// HR: the candidate best for the work waiting, when the company can pay them.
    fn auto_hire(&mut self, reserve: f64) {
        if self.staff.len() >= self.capacity() {
            return;
        }
        let mut need = [0.0f64; 5];
        for c in self.contracts.iter().chain(&self.offers) {
            for (n, (want, done)) in need.iter_mut().zip(c.need.iter().zip(c.done)) {
                *n += (want - done).max(0.0);
            }
        }
        let total: f64 = need.iter().sum();
        let weights = if total > 0.0 { need.map(|x| 0.3 + x / total) } else { [1.0; 5] };
        let net = self.income - self.spend;
        // The roles still worth one more (HR doesn't pick the ones that steer or spend: CTO, office
        // manager, assistant, product owner, the player's call); first the salespeople, then the others.
        let wanted = |special: Special| -> Option<f64> {
            let first = match special {
                Special::Sales => 3.0,
                Special::Lead => 2.0,
                Special::Qa | Special::Ops | Special::Hr => 1.0,
                _ => return None,
            };
            (self.staff.len() >= 3 && self.count(special) < self.role_useful(special)?).then_some(first)
        };
        let pick = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let special = e.job().special;
                let fee = self.hire_fee(e);
                let affordable = self.cash - fee > reserve && (net > e.salary * 1.2 || self.cash - fee > reserve * 3.0);
                let fits = special == Special::None || wanted(special).is_some();
                affordable && fits
            })
            .map(|(k, e)| {
                let worth: f64 = (0..5).map(|s| e.skills[s] * weights[s]).sum::<f64>() * e.level_mult();
                // A role still missing comes before the devs (the most missing first, then the cheapest).
                match wanted(e.job().special) {
                    Some(first) => (k, 1e12 * first + 1.0 / e.salary.max(0.1)),
                    None => (k, worth / e.salary.max(0.1)),
                }
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k);
        if let Some(k) = pick {
            self.hire(k);
        }
    }

    /// Project managers: someone to the refactoring when the debt rises, to the products when they
    /// suffer, to the research when there's nothing to deliver; and back.
    fn auto_tasks(&mut self) {
        let movable = |e: &Employee| !e.founder() && e.makes() && !matches!(e.job().special, Special::Qa | Special::Lead | Special::Cto | Special::Po);
        let on = |c: &Corp, t: Task| c.staff.iter().filter(|e| movable(e) && e.task == t).count();
        let mut moves: Vec<(Task, Task)> = Vec::new();
        if self.debt > 40.0 {
            moves.push((Task::Contracts, Task::Refactor));
        } else if self.debt < 10.0 && on(self, Task::Refactor) > 0 {
            moves.push((Task::Refactor, Task::Contracts));
        }
        let idle = self.contracts.is_empty() && self.offers.is_empty();
        let suffering = self.products.iter().any(|p| p.version == 0 || p.bugs > 25.0);
        if !self.products.is_empty() && (suffering || on(self, Task::Products) == 0) {
            moves.push((Task::Contracts, Task::Products));
        } else if (self.products.is_empty() && on(self, Task::Products) > 0) || (!idle && !suffering && on(self, Task::Products) > 1 && self.products.iter().all(|p| p.bugs < 5.0)) {
            moves.push((Task::Products, Task::Contracts));
        }
        let makers = self.staff.iter().filter(|e| movable(e)).count();
        // Every tech found: research gives nothing more, all back to work at once (the products
        // first, they always take more versions).
        let searching = TECHS.iter().any(|t| !self.has_tech(t.id));
        let back = if self.products.is_empty() { Task::Contracts } else { Task::Products };
        if !searching {
            moves.extend(std::iter::repeat_n((Task::Research, back), on(self, Task::Research)));
            if idle && !self.products.is_empty() && on(self, Task::Contracts) > 1 {
                moves.push((Task::Contracts, Task::Products));
            }
        } else if idle && on(self, Task::Contracts) > 1 {
            moves.push((Task::Contracts, Task::Research));
        } else if !idle && on(self, Task::Research) > (makers / 5).max(1) {
            moves.extend(std::iter::repeat_n((Task::Research, Task::Contracts), on(self, Task::Research) - (makers / 5).max(1)));
        } else if !idle && on(self, Task::Research) == 0 && makers >= 4 {
            moves.push((Task::Contracts, Task::Research));
        }
        for (from, to) in moves {
            // Keep someone on the contracts.
            if from == Task::Contracts && on(self, Task::Contracts) <= 1 {
                continue;
            }
            if let Some(e) = self.staff.iter_mut().filter(|e| movable(e) && e.task == from).min_by(|a, b| a.level.cmp(&b.level)) {
                e.task = to;
            }
        }
    }

    fn raise_event(&mut self) {
        let fits: Vec<usize> = (0..EVENTS.len()).filter(|&k| (EVENTS[k].fits)(self)).collect();
        if fits.is_empty() {
            return;
        }
        let k = fits[self.pick(fits.len())];
        let others: Vec<u32> = self.staff.iter().filter(|e| !e.founder()).map(|e| e.id).collect();
        self.event_who = if others.is_empty() { 0 } else { others[self.pick(others.len())] };
        self.event = Some(EVENTS[k].id.to_owned());
        self.event_age = 0.0;
    }

    /// Runs the company up to `now`, Ronnie open: in steps of a second at most.
    pub fn run(&mut self, now: i64) -> Summary {
        let mut sum = Summary::default();
        let secs = (now - self.at).max(0);
        // Hours unseen (the computer asleep): as if Ronnie had been closed.
        if secs > 300 {
            return self.catch_up(now);
        }
        let mut left = secs as f64;
        while left > 0.0 {
            let dt = left.min(1.0);
            self.step(dt, true, &mut sum);
            left -= dt;
        }
        self.at = now;
        sum.secs = secs as f64;
        sum
    }

    /// The time Ronnie was closed, caught up: `OFFLINE_CAP` at most, at `OFFLINE_RATE`, stopped
    /// before the cash would go into the red.
    pub fn catch_up(&mut self, now: i64) -> Summary {
        let mut sum = Summary::default();
        let away = (now - self.at).max(0) as f64;
        let mut left = away.min(OFFLINE_CAP) * OFFLINE_RATE;
        while left > 0.0 {
            let dt = left.min(10.0);
            let before = self.clone();
            let mut part = Summary::default();
            self.step(dt, false, &mut part);
            if self.cash < 0.0 && before.cash >= 0.0 {
                *self = before;
                sum.stopped = true;
                break;
            }
            sum.earned += part.earned;
            sum.spent += part.spent;
            sum.delivered += part.delivered;
            sum.levels += part.levels;
            left -= dt;
        }
        sum.secs = away;
        self.at = now;
        if away >= 600.0 {
            self.say(NewsKind::Away, "", away, sum.earned - sum.spent);
        }
        sum
    }
}

// ---------------------------------------------------------------------------------------------------
// The news, worded.

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

pub fn money(x: f64, fr: bool) -> String {
    let (sign, x) = if x < 0.0 { ("−", -x) } else { ("", x) };
    let (v, unit) = if x >= 1e12 {
        (x / 1e12, if fr { " Md k€" } else { "T €" })
    } else if x >= 1e9 {
        (x / 1e9, if fr { " Md€" } else { "B €" })
    } else if x >= 1e6 {
        (x / 1e6, if fr { " M€" } else { "M €" })
    } else if x >= 1e4 {
        (x / 1e3, if fr { " k€" } else { "k €" })
    } else {
        return format!("{sign}{} €", group(x.floor() as u64, fr));
    };
    let text = if v >= 100.0 { format!("{v:.0}") } else if v >= 10.0 { format!("{v:.1}") } else { format!("{v:.2}") };
    let text = if fr { text.replace('.', ",") } else { text };
    format!("{sign}{text}{unit}")
}

/// The giant cheque sent in the chat once the company was sold: a plain message, drawn as a cheque by
/// Ronnie (an older one shows the text as it is).
pub const CHEQUE_MARK: &str = "💸 Ronnie Corp · ";

pub fn cheque_text(amount: f64, fr: bool) -> String {
    format!("{CHEQUE_MARK}{}", money(amount, fr))
}

/// The amount a message's cheque says (None: not a cheque).
pub fn cheque_amount(text: &str) -> Option<&str> {
    let amount = text.strip_prefix(CHEQUE_MARK)?;
    (amount.len() <= 24 && !amount.contains('\n') && amount.chars().any(|c| c.is_ascii_digit())).then_some(amount)
}

fn group(n: u64, fr: bool) -> String {
    let s = n.to_string();
    let sep = if fr { '\u{202f}' } else { ',' };
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

pub fn duration(secs: f64, fr: bool) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{} h {:02}", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{} min", s / 60)
    } else if fr {
        format!("{s} s")
    } else {
        format!("{s}s")
    }
}

pub fn news_text(n: &News, fr: bool) -> String {
    let kind = |k: f64| tx(CONTRACTS[(k as usize).min(CONTRACTS.len() - 1)].name, fr);
    let product = |k: f64| tx(PRODUCTS[(k as usize).min(PRODUCTS.len() - 1)].name, fr);
    let you = if fr { "Toi" } else { "You" };
    let who = if n.who.is_empty() { you } else { n.who.as_str() };
    match n.kind {
        NewsKind::Start => tx(("Tout commence dans un garage. Comme les grands.", "It all starts in a garage. Like the greats."), fr).to_owned(),
        NewsKind::Hired => {
            let j = tx(JOBS[(n.m as usize).min(JOBS.len() - 1)].name, fr);
            if fr { format!("{who} rejoint l'équipe ({j}, niveau {}).", n.n) } else { format!("{who} joins the team ({j}, level {}).", n.n) }
        }
        NewsKind::Quit => if fr { format!("{who} a démissionné. Le moral était trop bas.") } else { format!("{who} quit. Morale was too low.") },
        NewsKind::Unpaid => if fr { format!("{who} est parti·e : les salaires ne tombaient plus.") } else { format!("{who} left: the salaries stopped coming.") },
        NewsKind::Fired => if fr { format!("{who} a été remercié·e.") } else { format!("{who} was let go.") },
        NewsKind::LevelUp => {
            if n.who.is_empty() {
                if fr { format!("Tu passes niveau {}.", n.n) } else { format!("You reach level {}.", n.n) }
            } else if fr {
                format!("{who} passe niveau {}.", n.n)
            } else {
                format!("{who} reaches level {}.", n.n)
            }
        }
        NewsKind::Delivered => if fr { format!("{} livré à {} : +{}.", kind(n.m), n.who, money(n.n, fr)) } else { format!("{} delivered to {}: +{}.", kind(n.m), n.who, money(n.n, fr)) },
        NewsKind::Late => if fr { format!("{} livré en retard à {} : +{} seulement.", kind(n.m), n.who, money(n.n, fr)) } else { format!("{} delivered late to {}: only +{}.", kind(n.m), n.who, money(n.n, fr)) },
        NewsKind::Lost => if fr { format!("{} a annulé son contrat ({}). La réputation en prend un coup.", n.who, kind(n.m)) } else { format!("{} cancelled their contract ({}). The reputation takes a hit.", n.who, kind(n.m)) },
        NewsKind::Launched => if fr { format!("{} est en ligne ! Les premiers utilisateurs arrivent.", product(n.n)) } else { format!("{} is live! The first users are coming.", product(n.n)) },
        NewsKind::Version => if fr { format!("{} passe en version {}.", product(n.n), n.m) } else { format!("{} reaches version {}.", product(n.n), n.m) },
        NewsKind::Outage => if fr { format!("Panne sur {} ! Les utilisateurs râlent.", product(n.n)) } else { format!("{} is down! The users are grumbling.", product(n.n)) },
        NewsKind::Tech => if fr { format!("Recherche : {}.", tx(TECHS[(n.n as usize).min(TECHS.len() - 1)].name, fr)) } else { format!("Research: {}.", tx(TECHS[(n.n as usize).min(TECHS.len() - 1)].name, fr)) },
        NewsKind::Stage => if fr { format!("Nouvelle étape : {} !", tx(STAGES[(n.n as usize).min(STAGES.len() - 1)].name, fr)) } else { format!("New stage: {}!", tx(STAGES[(n.n as usize).min(STAGES.len() - 1)].name, fr)) },
        NewsKind::Moved => if fr { format!("Déménagement : {}.", tx(OFFICES[(n.n as usize).min(OFFICES.len() - 1)].name, fr)) } else { format!("Moved: {}.", tx(OFFICES[(n.n as usize).min(OFFICES.len() - 1)].name, fr)) },
        NewsKind::Perk => if fr { format!("Nouvel aménagement : {}.", tx(PERKS[(n.n as usize).min(PERKS.len() - 1)].name, fr)) } else { format!("New fitting: {}.", tx(PERKS[(n.n as usize).min(PERKS.len() - 1)].name, fr)) },
        NewsKind::Event => {
            let e = &EVENTS[(n.n as usize).min(EVENTS.len() - 1)];
            let (choice, outcome) = ((n.m as usize) / 10, (n.m as usize) % 10);
            let text = e.choices.get(choice).and_then(|c| c.outcomes.get(outcome)).map_or("", |o| tx(*o, fr));
            let cash = if n.cash.abs() >= 1.0 { format!(" ({}{})", if n.cash > 0.0 { "+" } else { "" }, money(n.cash, fr)) } else { String::new() };
            format!("{} — {}{cash}", tx(e.title, fr), text.replace("{who}", who))
        }
        NewsKind::Away => {
            let net = money(n.m, fr);
            if fr { format!("Pendant ton absence ({}) : {} de solde.", duration(n.n, fr), net) } else { format!("While you were away ({}): {} net.", duration(n.n, fr), net) }
        }
        NewsKind::Sold => if fr { format!("Boîte revendue {} ! +{} étoile(s). Retour au garage, en plus fort.", money(n.n, fr), n.m) } else { format!("Company sold for {}! +{} star(s). Back to the garage, stronger.", money(n.n, fr), n.m) },
    }
}

// ---------------------------------------------------------------------------------------------------
// The events.

pub struct Choice {
    pub label: Tx,
    /// What came of it (`apply` says which).
    pub outcomes: &'static [Tx],
    pub apply: fn(&mut Corp) -> usize,
}

pub struct EventDef {
    pub id: &'static str,
    pub title: Tx,
    /// `{who}`: the employee it's about.
    pub text: Tx,
    pub fits: fn(&Corp) -> bool,
    pub choices: &'static [Choice],
}

impl Corp {
    fn morale_all(&mut self, by: f64) {
        for e in &mut self.staff {
            e.morale = (e.morale + by).clamp(0.0, 100.0);
        }
    }

    fn boost(&mut self, kind: BoostKind, mult: f64, secs: f64) {
        self.boosts.push(Boost { kind, mult, left: secs });
    }

    fn who_mut(&mut self) -> Option<&mut Employee> {
        let id = self.event_who;
        self.staff.iter_mut().find(|e| e.id == id && !e.founder())
    }

    fn who_leaves(&mut self) {
        let id = self.event_who;
        self.staff.retain(|e| e.id != id || e.founder());
    }

    fn users_times(&mut self, by: f64) {
        for p in &mut self.products {
            p.users *= by;
        }
    }

    fn live_products(&self) -> bool {
        self.products.iter().any(|p| p.version > 0)
    }

    fn has_others(&self) -> bool {
        self.staff.iter().any(|e| !e.founder())
    }
}

pub fn event(id: &str) -> Option<&'static EventDef> {
    EVENTS.iter().find(|e| e.id == id)
}

pub const EVENTS: [EventDef; 38] = [
    EventDef {
        id: "scope",
        title: ("Le client change tout", "The client changes everything"),
        text: ("La veille de la livraison, un client veut « juste deux trois petites modifs »… en fait, tout refaire.", "The day before delivery, a client wants \"just a couple of small tweaks\"… actually, to redo it all."),
        fits: |c| !c.contracts.is_empty(),
        choices: &[
            Choice { label: ("Accepter (+50 % de travail, +60 % de prix)", "Accept (+50 % work, +60 % price)"), outcomes: &[("Le client est ravi, l'équipe un peu moins.", "The client is delighted, the team a bit less.")], apply: |c| {
                if let Some(k) = c.contracts.first_mut() {
                    for s in 0..5 {
                        k.need[s] *= 1.5;
                    }
                    k.pay *= 1.6;
                    k.left += k.time * 0.3;
                }
                c.morale_all(-4.0);
                0
            } },
            Choice { label: ("Refuser poliment", "Politely refuse"), outcomes: &[("Le client grogne, mais signe le PV de recette.", "The client grumbles, but signs off.")], apply: |c| {
                c.rep = (c.rep - 1.0 - c.tier() as f64).max(0.0);
                0
            } },
        ],
    },
    EventDef {
        id: "outage_night",
        title: ("La prod est tombée à 3 h du matin", "Production went down at 3 a.m."),
        text: ("Ton téléphone sonne. Les serveurs sont en feu, au sens figuré. Enfin, on espère.", "Your phone rings. The servers are on fire, figuratively. Hopefully."),
        fits: |c| c.live_products(),
        choices: &[
            Choice { label: ("Nuit blanche pour tout réparer", "All-nighter to fix it all"), outcomes: &[("Réparé avant le café. L'équipe a des cernes.", "Fixed before coffee. The team has dark circles.")], apply: |c| {
                for p in &mut c.products {
                    p.bugs = (p.bugs - 30.0).max(0.0);
                    p.down = 0.0;
                }
                c.morale_all(-8.0);
                0
            } },
            Choice { label: ("On verra lundi", "It'll wait till Monday"), outcomes: &[("Des utilisateurs sont partis voir ailleurs.", "Some users went elsewhere.")], apply: |c| {
                c.users_times(0.88);
                0
            } },
        ],
    },
    EventDef {
        id: "investor",
        title: ("Un business angel frappe à la porte", "A business angel knocks on the door"),
        text: ("Il croit en toi. Et en ton chèque qu'il tient à la main. Mais il voudra donner son avis sur tout.", "He believes in you. And in the cheque in his hand. But he'll want a say in everything."),
        fits: |c| c.stage >= 1,
        choices: &[
            Choice { label: ("Prendre son chèque", "Take the cheque"), outcomes: &[("La trésorerie respire. Les réunions du lundi, moins.", "The cash breathes. Monday meetings, less so.")], apply: |c| {
                c.cash += c.unit() * 8.0;
                c.mood -= 5.0;
                0
            } },
            Choice { label: ("Rester indépendant", "Stay independent"), outcomes: &[("L'équipe est fière de rester libre.", "The team is proud to stay free.")], apply: |c| {
                c.mood += 5.0;
                0
            } },
        ],
    },
    EventDef {
        id: "fair",
        title: ("Un grand salon tech", "A big tech fair"),
        text: ("Tout le monde y sera. Un stand coûte cher, mais les visiteurs aussi cherchent du travail.", "Everyone will be there. A stand is pricey, but visitors are also looking for work."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Prendre un stand", "Get a stand"), outcomes: &[("Plein de cartes de visite et de CV.", "Lots of business cards and CVs.")], apply: |c| {
                c.cash -= c.unit() * 1.5;
                c.rep += 3.0 + 2.0 * c.tier() as f64;
                c.refresh_candidates();
                0
            } },
            Choice { label: ("Rester au bureau", "Stay at the office"), outcomes: &[("Pendant que les autres trinquent, vous codez.", "While the others toast, you code.")], apply: |c| {
                c.boost(BoostKind::Prod, 1.1, 300.0);
                0
            } },
        ],
    },
    EventDef {
        id: "hackathon",
        title: ("Hackathon ce week-end", "Hackathon this weekend"),
        text: ("48 heures, des pizzas, et un prix pour le meilleur projet.", "48 hours, pizzas, and a prize for the best project."),
        fits: |_| true,
        choices: &[
            Choice { label: ("On y va !", "Let's go!"), outcomes: &[("Vous gagnez le premier prix !", "You win first prize!"), ("Pas de prix, mais tout le monde a appris plein de choses.", "No prize, but everyone learned a lot.")], apply: |c| {
                for e in &mut c.staff {
                    e.xp += e.xp_next() * 0.3;
                }
                c.morale_all(4.0);
                if c.chance(0.4) {
                    c.cash += c.unit() * 2.0;
                    c.rep += 4.0;
                    0
                } else {
                    1
                }
            } },
            Choice { label: ("Un week-end de repos", "A weekend off"), outcomes: &[("Tout le monde revient reposé.", "Everyone comes back rested.")], apply: |c| {
                c.morale_all(5.0);
                0
            } },
        ],
    },
    EventDef {
        id: "coffee",
        title: ("La machine à café a rendu l'âme", "The coffee machine died"),
        text: ("Un silence glaçant tombe sur l'open space.", "A chilling silence falls over the office."),
        fits: |_| true,
        choices: &[
            Choice { label: ("En racheter une tout de suite", "Buy a new one right away"), outcomes: &[("Crise évitée.", "Crisis averted.")], apply: |c| {
                c.cash -= (c.unit() * 0.2).max(150.0);
                0
            } },
            Choice { label: ("Passer au thé", "Switch to tea"), outcomes: &[("Ambiance… zen. Très zen.", "The mood is… zen. Very zen.")], apply: |c| {
                c.mood -= 10.0;
                c.boost(BoostKind::Prod, 0.92, 300.0);
                0
            } },
        ],
    },
    EventDef {
        id: "raise",
        title: ("Une demande d'augmentation", "A pay rise request"),
        text: ("{who} frappe à ton bureau : « On peut parler de mon salaire ? »", "{who} knocks on your door: \"Can we talk about my salary?\""),
        fits: |c| c.has_others(),
        choices: &[
            Choice { label: ("Accorder +15 %", "Grant +15 %"), outcomes: &[("{who} repart ravi·e.", "{who} leaves delighted.")], apply: |c| {
                if let Some(e) = c.who_mut() {
                    e.salary *= 1.15;
                    e.morale = (e.morale + 25.0).min(100.0);
                }
                0
            } },
            Choice { label: ("Refuser", "Refuse"), outcomes: &[("{who} fait la tête.", "{who} sulks."), ("{who} a posé sa démission.", "{who} handed in their notice.")], apply: |c| {
                if c.chance(0.3) {
                    c.who_leaves();
                    1
                } else {
                    if let Some(e) = c.who_mut() {
                        e.morale = (e.morale - 25.0).max(0.0);
                    }
                    0
                }
            } },
        ],
    },
    EventDef {
        id: "poach",
        title: ("Tentative de débauchage", "A poaching attempt"),
        text: ("Un géant de la tech propose un pont d'or à {who}.", "A tech giant offers {who} a golden bridge."),
        fits: |c| c.has_others(),
        choices: &[
            Choice { label: ("Contre-offre (+30 %)", "Counter-offer (+30 %)"), outcomes: &[("{who} reste, et le fait savoir.", "{who} stays, and lets everyone know.")], apply: |c| {
                if let Some(e) = c.who_mut() {
                    e.salary *= 1.3;
                    e.morale = (e.morale + 15.0).min(100.0);
                }
                0
            } },
            Choice { label: ("Lui rappeler l'ambiance", "Remind them of the vibe"), outcomes: &[("Ça a marché : {who} reste.", "It worked: {who} stays."), ("{who} est parti·e chez le géant.", "{who} left for the giant.")], apply: |c| {
                let stay = 0.3 + c.mods().morale / 60.0;
                if c.chance(stay.min(0.85)) {
                    0
                } else {
                    c.who_leaves();
                    1
                }
            } },
            Choice { label: ("Laisser partir", "Let them go"), outcomes: &[("Bonne route, {who}.", "Farewell, {who}.")], apply: |c| {
                c.who_leaves();
                0
            } },
        ],
    },
    EventDef {
        id: "security",
        title: ("Faille de sécurité", "Security hole"),
        text: ("Un chercheur signale une faille dans du code que vous avez livré.", "A researcher reports a hole in code you shipped."),
        fits: |c| c.stats.contracts >= 2,
        choices: &[
            Choice { label: ("Tout le monde sur le patch", "Everyone on the patch"), outcomes: &[("Patché, et annoncé proprement. Les clients apprécient.", "Patched, and announced properly. The clients appreciate it.")], apply: |c| {
                c.boost(BoostKind::Prod, 0.6, 180.0);
                c.rep += 2.0;
                0
            } },
            Choice { label: ("Croiser les doigts", "Cross fingers"), outcomes: &[("Personne n'a rien vu. Ouf.", "Nobody noticed. Phew."), ("La faille est exploitée. Ça se paie, en argent et en réputation.", "The hole is exploited. It costs money and reputation.")], apply: |c| {
                if c.chance(0.5) {
                    0
                } else {
                    c.cash -= c.unit() * 2.0;
                    c.rep *= 0.9;
                    1
                }
            } },
        ],
    },
    EventDef {
        id: "press",
        title: ("Un journaliste veut vous interviewer", "A journalist wants an interview"),
        text: ("Pour un dossier sur « les boîtes qui montent ».", "For a piece on \"companies on the rise\"."),
        fits: |c| c.rep >= 10.0,
        choices: &[
            Choice { label: ("Accorder l'interview", "Give the interview"), outcomes: &[("L'article fait le buzz.", "The article goes viral.")], apply: |c| {
                c.rep += 3.0 * (1.0 + c.stage as f64);
                c.boost(BoostKind::Offers, 1.5, 600.0);
                0
            } },
            Choice { label: ("Pas le temps", "No time"), outcomes: &[("Le dossier paraît sans vous.", "The piece comes out without you.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "intern",
        title: ("Un·e stagiaire très motivé·e", "A very motivated intern"),
        text: ("Une candidature spontanée, trois pages de lettre de motivation, et des cookies faits maison.", "An unsolicited application, a three-page cover letter, and home-made cookies."),
        fits: |c| c.staff.len() < c.capacity(),
        choices: &[
            Choice { label: ("Le·la prendre", "Take them"), outcomes: &[("Apprend vite… et mange tous les gâteaux.", "Learns fast… and eats all the cakes.")], apply: |c| {
                let jobs = ["front", "back", "design", "mobile"];
                let j = jobs[c.pick(jobs.len())];
                let mut e = c.make_employee(j, 1);
                e.salary *= 0.35;
                e.trait_id = Some("motivated".into());
                c.say(NewsKind::Hired, &e.name.clone(), 1.0, JOBS.iter().position(|x| x.id == j).unwrap_or(0) as f64);
                c.staff.push(e);
                c.stats.hires += 1;
                0
            } },
            Choice { label: ("Pas de place pour l'instant", "No room for now"), outcomes: &[("Les cookies restent, au moins.", "The cookies stay, at least.")], apply: |c| {
                c.mood += 1.0;
                0
            } },
        ],
    },
    EventDef {
        id: "party",
        title: ("La fête de fin d'année", "The end-of-year party"),
        text: ("L'équipe demande s'il y aura une fête cette année. Les regards sont lourds de sens.", "The team asks whether there'll be a party this year. The looks are meaningful."),
        fits: |c| c.has_others(),
        choices: &[
            Choice { label: ("Grosse soirée", "A big night out"), outcomes: &[("Légendaire. On en parlera encore dans dix ans.", "Legendary. People will talk about it for ten years.")], apply: |c| {
                c.cash -= c.unit() * 2.0;
                c.mood += 20.0;
                0
            } },
            Choice { label: ("Des pizzas au bureau", "Pizzas at the office"), outcomes: &[("Simple et efficace.", "Simple and effective.")], apply: |c| {
                c.cash -= c.unit() * 0.3;
                c.mood += 6.0;
                0
            } },
            Choice { label: ("Rien du tout", "Nothing at all"), outcomes: &[("Ambiance glaciale à la machine à café.", "Icy mood at the coffee machine.")], apply: |c| {
                c.mood -= 10.0;
                0
            } },
        ],
    },
    EventDef {
        id: "audit",
        title: ("Contrôle fiscal surprise", "Surprise tax audit"),
        text: ("Un monsieur en costume gris demande à voir « deux trois papiers ».", "A man in a grey suit asks to see \"a couple of papers\"."),
        fits: |c| c.stage >= 1,
        choices: &[
            Choice { label: ("Tout est en règle… normalement", "It's all in order… normally"), outcomes: &[("Le contrôleur repart bredouille.", "The auditor leaves empty-handed."), ("Une petite erreur de TVA. Elle coûte cher.", "A small VAT mistake. An expensive one.")], apply: |c| {
                if c.chance(0.7) {
                    0
                } else {
                    c.cash -= c.unit() * 3.0;
                    1
                }
            } },
            Choice { label: ("Appeler un expert-comptable", "Call an accountant"), outcomes: &[("Rien à signaler.", "Nothing to report.")], apply: |c| {
                c.cash -= c.unit() * 0.8;
                0
            } },
        ],
    },
    EventDef {
        id: "framework",
        title: ("La nouvelle version du framework casse tout", "The framework's new version breaks everything"),
        text: ("Les notes de version font 40 pages. Le mot « breaking » y apparaît 112 fois.", "The release notes are 40 pages long. The word \"breaking\" shows up 112 times."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Migrer maintenant", "Migrate now"), outcomes: &[("Migration faite. Le code est tout propre.", "Migration done. The code is squeaky clean.")], apply: |c| {
                c.boost(BoostKind::Prod, 0.8, 300.0);
                c.debt = (c.debt - 15.0).max(0.0);
                0
            } },
            Choice { label: ("Rester sur l'ancienne", "Stay on the old one"), outcomes: &[("Ça tient… pour l'instant.", "It holds… for now.")], apply: |c| {
                c.debt = (c.debt + 12.0).min(100.0);
                0
            } },
        ],
    },
    EventDef {
        id: "influencer",
        title: ("Un influenceur veut tester ton produit", "An influencer wants to try your product"),
        text: ("Deux millions d'abonnés, et un avis très tranché sur tout.", "Two million followers, and strong opinions on everything."),
        fits: |c| c.live_products(),
        choices: &[
            Choice { label: ("Lui offrir un compte", "Give them an account"), outcomes: &[("Il adore ! Les inscriptions s'envolent.", "They love it! Sign-ups soar."), ("Il déteste. Et il le dit, en vidéo.", "They hate it. And say so, on video.")], apply: |c| {
                if c.chance(0.75) {
                    c.users_times(1.25);
                    c.boost(BoostKind::Users, 1.3, 900.0);
                    0
                } else {
                    c.users_times(0.9);
                    1
                }
            } },
            Choice { label: ("Non merci", "No thanks"), outcomes: &[("Il ira tester le concurrent.", "They'll try the competitor instead.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "strike",
        title: ("Grève des transports", "Transport strike"),
        text: ("Plus un métro, plus un train. La moitié de l'équipe est coincée chez elle.", "No metro, no trains. Half the team is stuck at home."),
        fits: |c| c.has_others(),
        choices: &[
            Choice { label: ("Télétravail pour tous", "Everyone works from home"), outcomes: &[("Ça marche plutôt bien, en fait.", "It works rather well, actually.")], apply: |c| {
                if c.policy.remote == 0 {
                    c.boost(BoostKind::Prod, 0.9, 300.0);
                }
                c.morale_all(3.0);
                0
            } },
            Choice { label: ("Organiser du covoiturage", "Organise carpooling"), outcomes: &[("Des amitiés naissent dans les bouchons.", "Friendships are born in traffic jams.")], apply: |c| {
                c.cash -= c.unit() * 0.2;
                c.mood += 3.0;
                0
            } },
        ],
    },
    EventDef {
        id: "award",
        title: ("Le prix de l'innovation", "The innovation award"),
        text: ("Les candidatures sont ouvertes. Les frais de dossier aussi.", "Applications are open. So are the fees."),
        fits: |c| c.techs.len() >= 2,
        choices: &[
            Choice { label: ("Candidater", "Apply"), outcomes: &[("Vous gagnez ! Trophée en plexiglas et chèque géant.", "You win! Plexiglass trophy and giant cheque."), ("Mention honorable. C'est déjà ça.", "Honourable mention. That's something.")], apply: |c| {
                c.cash -= c.unit() * 0.5;
                if c.chance(0.4) {
                    c.rep += 5.0 * (1.0 + c.stage as f64);
                    c.cash += c.unit() * 3.0;
                    0
                } else {
                    c.rep += 1.0;
                    1
                }
            } },
            Choice { label: ("Pas pour nous", "Not for us"), outcomes: &[("Le trophée ira à d'autres.", "The trophy goes to someone else.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "unpaid",
        title: ("Un client ne paie pas", "A client won't pay"),
        text: ("Trois relances, zéro virement. Il « attend le retour de son comptable ».", "Three reminders, zero payment. He's \"waiting to hear from his accountant\"."),
        fits: |c| c.stats.contracts >= 3,
        choices: &[
            Choice { label: ("Envoyer un avocat", "Send a lawyer"), outcomes: &[("Le virement arrive, avec des intérêts.", "The payment arrives, with interest."), ("Le client a disparu. L'avocat, lui, a envoyé sa facture.", "The client vanished. The lawyer sent their bill, though.")], apply: |c| {
                c.cash -= c.unit() * 0.6;
                if c.chance(0.7) {
                    c.cash += c.unit() * 2.0;
                    0
                } else {
                    1
                }
            } },
            Choice { label: ("Laisser tomber", "Let it go"), outcomes: &[("Tant pis. On note le nom.", "Never mind. Name noted.")], apply: |c| {
                c.cash -= c.unit() * 0.5;
                0
            } },
        ],
    },
    EventDef {
        id: "cat",
        title: ("Un chat s'est installé au bureau", "A cat moved into the office"),
        text: ("Il dort sur le serveur. Il a l'air chez lui.", "It sleeps on the server. It looks at home."),
        fits: |_| true,
        choices: &[
            Choice { label: ("L'adopter", "Adopt it"), outcomes: &[("Il s'appelle Segfault. Tout le monde l'adore.", "Its name is Segfault. Everyone loves it.")], apply: |c| {
                c.mood += 12.0;
                0
            } },
            Choice { label: ("Le confier à un refuge", "Take it to a shelter"), outcomes: &[("Il a trouvé une famille. L'équipe est un peu triste.", "It found a family. The team is a bit sad.")], apply: |c| {
                c.mood -= 2.0;
                0
            } },
        ],
    },
    EventDef {
        id: "heatwave",
        title: ("Canicule", "Heatwave"),
        text: ("38 °C dans l'open space. Les ventilateurs brassent de l'air chaud.", "38 °C in the office. The fans stir hot air."),
        fits: |c| c.policy.remote < 2,
        choices: &[
            Choice { label: ("Installer la clim", "Install air conditioning"), outcomes: &[("Ah, la fraîcheur.", "Ah, cool air.")], apply: |c| {
                c.cash -= c.unit() * 1.2;
                c.mood += 4.0;
                0
            } },
            Choice { label: ("Glaces pour tout le monde", "Ice cream for everyone"), outcomes: &[("Moral sauvé, productivité fondue.", "Morale saved, productivity melted.")], apply: |c| {
                c.cash -= c.unit() * 0.1;
                c.boost(BoostKind::Prod, 0.85, 600.0);
                c.mood += 2.0;
                0
            } },
        ],
    },
    EventDef {
        id: "apikey",
        title: ("Une clé secrète a fuité", "A secret key leaked"),
        text: ("Quelqu'un a poussé une clé d'API sur un dépôt public. Oups.", "Someone pushed an API key to a public repository. Oops."),
        fits: |c| c.has_others(),
        choices: &[
            Choice { label: ("Tout révoquer cette nuit", "Revoke everything tonight"), outcomes: &[("Les clés sont changées. Leçon retenue.", "The keys are changed. Lesson learned.")], apply: |c| {
                c.morale_all(-4.0);
                c.rep += 1.0;
                0
            } },
            Choice { label: ("Ni vu ni connu", "No one will notice"), outcomes: &[("Personne n'a rien vu.", "Nobody noticed."), ("Quelqu'un a vu. Facture cloud astronomique.", "Someone noticed. Astronomical cloud bill.")], apply: |c| {
                if c.chance(0.5) {
                    0
                } else {
                    c.cash -= c.unit() * 4.0;
                    c.rep *= 0.92;
                    1
                }
            } },
        ],
    },
    EventDef {
        id: "lowball",
        title: ("Une offre de rachat… modeste", "A… modest buyout offer"),
        text: ("Un fonds propose de racheter la boîte pour le prix d'une voiture d'occasion.", "A fund offers to buy the company for the price of a used car."),
        fits: |c| c.stage >= 2,
        choices: &[
            Choice { label: ("Rire très fort", "Laugh very loudly"), outcomes: &[("L'équipe a adoré ta réponse.", "The team loved your answer.")], apply: |c| {
                c.mood += 6.0;
                0
            } },
            Choice { label: ("Négocier, pour le principe", "Negotiate, on principle"), outcomes: &[("Ça fuite dans la presse : on parle de vous.", "It leaks to the press: people talk about you.")], apply: |c| {
                c.rep += 3.0;
                0
            } },
        ],
    },
    EventDef {
        id: "burnout",
        title: ("Épuisement", "Exhaustion"),
        text: ("{who} s'est endormi·e sur son clavier. Deux fois.", "{who} fell asleep on their keyboard. Twice."),
        fits: |c| c.policy.hours == 2 && c.has_others(),
        choices: &[
            Choice { label: ("Une semaine de congés", "A week off"), outcomes: &[("{who} revient en pleine forme.", "{who} comes back in great shape.")], apply: |c| {
                if let Some(e) = c.who_mut() {
                    e.morale = 85.0;
                }
                c.boost(BoostKind::Prod, 0.95, 300.0);
                0
            } },
            Choice { label: ("Encore un petit effort", "Just a little more effort"), outcomes: &[("{who} tient le coup. Pour l'instant.", "{who} holds on. For now."), ("{who} a craqué et démissionné.", "{who} broke down and quit.")], apply: |c| {
                if c.chance(0.5) {
                    c.who_leaves();
                    1
                } else {
                    if let Some(e) = c.who_mut() {
                        e.morale = (e.morale - 15.0).max(0.0);
                    }
                    0
                }
            } },
        ],
    },
    EventDef {
        id: "opensource",
        title: ("Un projet open source demande de l'aide", "An open source project asks for help"),
        text: ("La bibliothèque sur laquelle repose tout votre code est maintenue par une seule personne. Épuisée.", "The library all your code relies on is maintained by one person. Exhausted."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Contribuer", "Contribute"), outcomes: &[("La communauté vous remercie, et vous apprenez plein de choses.", "The community thanks you, and you learn a lot.")], apply: |c| {
                c.boost(BoostKind::Prod, 0.9, 300.0);
                c.research += 100.0 * (1.0 + c.stage as f64).powi(3);
                c.rep += 2.0;
                0
            } },
            Choice { label: ("Faire un don", "Donate"), outcomes: &[("Le mainteneur peut s'acheter un nouveau clavier.", "The maintainer can buy a new keyboard.")], apply: |c| {
                c.cash -= c.unit() * 0.5;
                c.rep += 1.0;
                0
            } },
            Choice { label: ("Pas le temps", "No time"), outcomes: &[("Croisons les doigts pour qu'il tienne.", "Let's hope they hold on.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "crypto",
        title: ("Payé en cryptos ?", "Paid in crypto?"),
        text: ("Un client propose de payer sa facture en « RonnieCoin ». Il jure que ça va monter.", "A client offers to pay in \"RonnieCoin\". He swears it'll go up."),
        fits: |c| c.stats.contracts >= 1,
        choices: &[
            Choice { label: ("Accepter", "Accept"), outcomes: &[("Le RonnieCoin a fait ×3 !", "RonnieCoin went ×3!"), ("Le RonnieCoin vaut maintenant… un sandwich.", "RonnieCoin is now worth… a sandwich.")], apply: |c| {
                if c.chance(0.45) {
                    c.cash += c.unit() * 3.0;
                    0
                } else {
                    c.cash += c.unit() * 0.1;
                    1
                }
            } },
            Choice { label: ("Des euros, merci", "Euros, thanks"), outcomes: &[("Ennuyeux, mais sûr.", "Boring, but safe.")], apply: |c| {
                c.cash += c.unit();
                0
            } },
        ],
    },
    EventDef {
        id: "seminar",
        title: ("Séminaire d'équipe", "Team retreat"),
        text: ("Les RH (ou toi) proposent une journée pour « souder l'équipe ».", "HR (or you) suggest a day to \"bring the team together\"."),
        fits: |c| c.staff.len() >= 3,
        choices: &[
            Choice { label: ("Escape game", "Escape game"), outcomes: &[("Sortis en 42 minutes. Record battu.", "Out in 42 minutes. Record beaten.")], apply: |c| {
                c.cash -= c.unit() * 0.8;
                c.mood += 9.0;
                0
            } },
            Choice { label: ("Atelier post-it", "Post-it workshop"), outcomes: &[("Personne n'a compris l'exercice.", "Nobody understood the exercise.")], apply: |c| {
                c.mood += 2.0;
                0
            } },
        ],
    },
    // Those that can bring money, by the choice made.
    EventDef {
        id: "side_gig",
        title: ("Une mission express", "A rush job"),
        text: ("Un ancien client a besoin d'un site pour lundi. Il paie le triple. Il faudrait bosser ce week-end.", "A former client needs a website by Monday. Triple pay. It means working this weekend."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Bosser le week-end", "Work the weekend"), outcomes: &[("Livré dimanche soir. Le virement est arrivé.", "Delivered Sunday night. The payment came in.")], apply: |c| {
                c.cash += c.unit() * 3.0;
                c.morale_all(-6.0);
                0
            } },
            Choice { label: ("Proposer un tarif d'urgence", "Quote a rush rate"), outcomes: &[("Il accepte ! Un week-end très rentable.", "He accepts! A very profitable weekend."), ("Trop cher pour lui. Il ira voir ailleurs.", "Too pricey for him. He'll go elsewhere.")], apply: |c| {
                if c.chance(0.5) {
                    c.cash += c.unit() * 5.0;
                    c.morale_all(-6.0);
                    0
                } else {
                    1
                }
            } },
            Choice { label: ("Refuser, c'est le week-end", "Refuse, it's the weekend"), outcomes: &[("L'équipe apprécie.", "The team appreciates it.")], apply: |c| {
                c.mood += 3.0;
                0
            } },
        ],
    },
    EventDef {
        id: "domain",
        title: ("Ton nom de domaine intéresse", "Your domain name is wanted"),
        text: ("Tu possèdes depuis des années un nom de domaine qu'une grande marque veut absolument.", "For years you've owned a domain name a big brand absolutely wants."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Vendre tout de suite", "Sell right away"), outcomes: &[("Vendu. Facile.", "Sold. Easy.")], apply: |c| {
                c.cash += c.unit() * 3.0;
                0
            } },
            Choice { label: ("Faire monter les enchères", "Raise the stakes"), outcomes: &[("Ils ont cédé : jackpot !", "They gave in: jackpot!"), ("Ils ont pris un autre nom. Tu en tires une misère.", "They picked another name. You get peanuts.")], apply: |c| {
                if c.chance(0.5) {
                    c.cash += c.unit() * 8.0;
                    0
                } else {
                    c.cash += c.unit() * 0.5;
                    1
                }
            } },
            Choice { label: ("Le garder", "Keep it"), outcomes: &[("Il est joli, ce nom.", "It's a nice name.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "license",
        title: ("Une licence de ton code", "A licence of your code"),
        text: ("Une entreprise veut utiliser la bibliothèque maison que vous avez écrite.", "A company wants to use the in-house library you wrote."),
        fits: |c| !c.techs.is_empty(),
        choices: &[
            Choice { label: ("Vendre une licence", "Sell a licence"), outcomes: &[("Contrat signé, chèque encaissé.", "Contract signed, cheque cashed.")], apply: |c| {
                c.cash += c.unit() * 5.0;
                0
            } },
            Choice { label: ("La publier en open source", "Publish it as open source"), outcomes: &[("Des milliers d'étoiles sur la forge. Tout le monde vous connaît.", "Thousands of stars on the forge. Everyone knows you.")], apply: |c| {
                c.rep += 4.0 + 3.0 * c.stage as f64;
                c.boost(BoostKind::Offers, 1.4, 600.0);
                0
            } },
        ],
    },
    EventDef {
        id: "grant",
        title: ("Une subvention à l'innovation", "An innovation grant"),
        text: ("La région finance les projets innovants. Il y a un dossier de 60 pages à remplir.", "The region funds innovative projects. There's a 60-page application to fill in."),
        fits: |c| c.stage >= 1,
        choices: &[
            Choice { label: ("Monter le dossier", "Put the application together"), outcomes: &[("Dossier accepté, subvention versée !", "Application accepted, grant paid!"), ("Refusé « faute de budget ». Tout ça pour ça.", "Refused \"for lack of budget\". All that for nothing.")], apply: |c| {
                c.boost(BoostKind::Prod, 0.9, 300.0);
                if c.chance(0.65) {
                    c.cash += c.unit() * 6.0;
                    0
                } else {
                    1
                }
            } },
            Choice { label: ("Trop de paperasse", "Too much paperwork"), outcomes: &[("Le formulaire reste vierge.", "The form stays blank.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "rent_devs",
        title: ("Une banque veut te louer des devs", "A bank wants to borrow your devs"),
        text: ("Elle est en retard sur un projet et paie très bien. Mais ce sera autant de temps en moins pour vos clients.", "It's late on a project and pays very well. But that's time taken from your own clients."),
        fits: |c| c.staff.len() >= 3,
        choices: &[
            Choice { label: ("Louer l'équipe un temps", "Lend the team for a while"), outcomes: &[("La banque paie rubis sur l'ongle.", "The bank pays in full and on time.")], apply: |c| {
                c.cash += c.unit() * 5.0;
                c.boost(BoostKind::Prod, 0.75, 600.0);
                0
            } },
            Choice { label: ("Non, nos clients d'abord", "No, our clients first"), outcomes: &[("Vos clients ne sauront jamais à quoi ils ont échappé.", "Your clients will never know what they escaped.")], apply: |c| {
                c.rep += 1.0;
                0
            } },
        ],
    },
    EventDef {
        id: "pitch",
        title: ("Un concours de pitch", "A pitch contest"),
        text: ("Trois minutes pour convaincre un jury d'investisseurs. Le premier prix est gros.", "Three minutes to win over a panel of investors. The first prize is big."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Pitcher toi-même", "Pitch it yourself"), outcomes: &[("Standing ovation. Premier prix !", "Standing ovation. First prize!"), ("Ta démo a planté en direct. Au moins, on se souvient de toi.", "Your demo crashed live. At least they remember you.")], apply: |c| {
                if c.chance(0.35) {
                    c.cash += c.unit() * 10.0;
                    c.rep += 5.0;
                    0
                } else {
                    c.rep += 1.0;
                    1
                }
            } },
            Choice { label: ("Envoyer une vidéo", "Send a video"), outcomes: &[("Prix du public : un petit chèque.", "Audience award: a small cheque.")], apply: |c| {
                c.cash += c.unit();
                0
            } },
            Choice { label: ("Passer ton tour", "Sit this one out"), outcomes: &[("Une autre fois.", "Another time.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "old_gear",
        title: ("Le vieux matériel du placard", "The old gear in the cupboard"),
        text: ("Des écrans, des claviers et deux serveurs dorment au fond d'un placard.", "Screens, keyboards and two servers are sleeping at the back of a cupboard."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Tout revendre", "Sell it all"), outcomes: &[("Ça fait de la place, et un peu d'argent.", "It frees up space, and a bit of money.")], apply: |c| {
                c.cash += c.unit() * 1.5;
                0
            } },
            Choice { label: ("Le donner à une école", "Give it to a school"), outcomes: &[("Les élèves sont ravis. La presse locale aussi.", "The pupils are delighted. So is the local press.")], apply: |c| {
                c.rep += 3.0;
                c.mood += 4.0;
                0
            } },
        ],
    },
    EventDef {
        id: "bug_bounty",
        title: ("Une chasse aux bugs récompensée", "A bug bounty"),
        text: ("Un grand éditeur paie qui trouve des failles dans son logiciel.", "A big software maker pays whoever finds holes in its software."),
        fits: |c| c.staff.len() >= 2,
        choices: &[
            Choice { label: ("Lancer l'équipe dessus", "Put the team on it"), outcomes: &[("Trois failles trouvées, trois primes encaissées !", "Three holes found, three bounties cashed!"), ("Rien trouvé. Leur code est solide.", "Nothing found. Their code is solid.")], apply: |c| {
                c.boost(BoostKind::Prod, 0.85, 300.0);
                if c.chance(0.6) {
                    c.cash += c.unit() * 4.0;
                    c.rep += 2.0;
                    0
                } else {
                    1
                }
            } },
            Choice { label: ("Pas le temps", "No time"), outcomes: &[("D'autres empocheront les primes.", "Others will pocket the bounties.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "youtuber",
        title: ("Un youtubeur tech te contacte", "A tech YouTuber gets in touch"),
        text: ("Il fait des vidéos sur les outils de dev et propose un partenariat.", "He makes videos on dev tools and offers a partnership."),
        fits: |c| c.rep >= 5.0,
        choices: &[
            Choice { label: ("Le payer pour une vidéo", "Pay him for a video"), outcomes: &[("La vidéo cartonne : les offres affluent.", "The video is a hit: the offers pour in.")], apply: |c| {
                c.cash -= c.unit();
                c.boost(BoostKind::Offers, 1.8, 900.0);
                c.boost(BoostKind::Users, 1.3, 900.0);
                0
            } },
            Choice { label: ("Te faire payer pour tes outils", "Get paid for your tools"), outcomes: &[("Il paie pour présenter vos outils maison.", "He pays to show off your in-house tools.")], apply: |c| {
                c.cash += c.unit() * 2.0;
                0
            } },
        ],
    },
    EventDef {
        id: "tax_credit",
        title: ("Le crédit d'impôt recherche", "The research tax credit"),
        text: ("Ta recherche pourrait te rapporter un crédit d'impôt. Encore faut-il le demander.", "Your research could earn you a tax credit. You still have to claim it."),
        fits: |c| c.techs.len() >= 2,
        choices: &[
            Choice { label: ("Faire la demande", "Claim it"), outcomes: &[("Accordé. Ta recherche est en partie remboursée.", "Granted. Your research is partly refunded.")], apply: |c| {
                c.cash += c.unit() * 2.0 * c.techs.len().min(6) as f64;
                0
            } },
            Choice { label: ("Laisser tomber", "Never mind"), outcomes: &[("Des euros qui dorment.", "Euros left sleeping.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "stock_tip",
        title: ("Un tuyau en bourse", "A stock tip"),
        text: ("Ton beau-frère jure qu'une action va « exploser ». Il a l'air très sûr de lui.", "Your brother-in-law swears a stock is about to \"explode\". He looks very sure."),
        fits: |c| c.cash > c.unit() * 3.0,
        choices: &[
            Choice { label: ("Placer une grosse somme", "Put in a big sum"), outcomes: &[("Elle a explosé… vers le haut ! Jackpot.", "It exploded… upwards! Jackpot."), ("Elle a explosé… vers le bas.", "It exploded… downwards.")], apply: |c| {
                let stake = c.unit() * 2.0;
                if c.chance(0.4) {
                    c.cash += stake * 2.5;
                    0
                } else {
                    c.cash -= stake;
                    1
                }
            } },
            Choice { label: ("Placer un petit peu", "Put in a little"), outcomes: &[("Petit gain, petit sourire.", "Small gain, small smile."), ("Petite perte, petite grimace.", "Small loss, small grimace.")], apply: |c| {
                let stake = c.unit() * 0.5;
                if c.chance(0.5) {
                    c.cash += stake;
                    0
                } else {
                    c.cash -= stake * 0.6;
                    1
                }
            } },
            Choice { label: ("Non merci", "No thanks"), outcomes: &[("Le beau-frère boude.", "The brother-in-law sulks.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "white_label",
        title: ("Ton produit en marque blanche ?", "Your product, white-labelled?"),
        text: ("Un grand groupe veut revendre ton produit sous son propre nom.", "A big group wants to resell your product under its own name."),
        fits: |c| c.live_products(),
        choices: &[
            Choice { label: ("Signer", "Sign"), outcomes: &[("Un gros chèque, et quelques utilisateurs qui passent chez eux.", "A big cheque, and a few users who move to them.")], apply: |c| {
                c.cash += c.unit() * 6.0;
                c.users_times(0.92);
                0
            } },
            Choice { label: ("Refuser, c'est notre bébé", "Refuse, it's our baby"), outcomes: &[("L'équipe est fière.", "The team is proud.")], apply: |c| {
                c.mood += 4.0;
                0
            } },
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn corp() -> Corp {
        Corp::new(1_000_000, 42, Legacy::default())
    }

    #[test]
    fn hr_hires_the_roles_only_up_to_what_is_worth_it() {
        let mut c = corp();
        c.office = 4;
        c.cash = 1e9;
        c.income = 1e6;
        for job in ["back", "back", "front", "front", "sales", "sales", "sales", "sales"] {
            let e = c.make_employee(job, 1);
            c.staff.push(e);
        }
        assert_eq!(c.role_useful(Special::Sales), Some(5));
        let candidates: Vec<Employee> = ["sales", "sales", "office", "cto", "po", "assistant"].iter().map(|j| c.make_employee(j, 1)).collect();
        c.candidates = candidates;
        for _ in 0..6 {
            c.auto_hire(0.0);
        }
        assert_eq!(c.sales(), 5, "a fifth salesperson, not a sixth");
        for special in [Special::Office, Special::Cto, Special::Po, Special::Assistant] {
            assert_eq!(c.count(special), 0, "the player's call");
        }
        // A missing role before a dev; none past its ceiling.
        let candidates: Vec<Employee> = ["back", "lead", "lead"].iter().map(|j| c.make_employee(j, 1)).collect();
        c.candidates = candidates;
        c.auto_hire(0.0);
        assert_eq!(c.count(Special::Lead), 1);
        c.auto_hire(0.0);
        assert_eq!(c.count(Special::Lead), 1, "one per 6 who make: 6 here");
        assert_eq!(c.staff.iter().filter(|e| e.job == "back").count(), 3);
    }

    #[test]
    fn starts_alone_in_the_garage() {
        let c = corp();
        assert_eq!(c.staff.len(), 1);
        assert!(c.staff[0].founder());
        assert_eq!(c.office, 0);
        assert_eq!(c.offers.len(), 1);
        assert!(c.cash > 0.0);
    }

    #[test]
    fn a_contract_gets_done_and_paid() {
        let mut c = corp();
        assert!(c.accept(0));
        let cash = c.cash;
        let mut sum = Summary::default();
        for _ in 0..600 {
            c.step(1.0, false, &mut sum);
        }
        assert!(c.stats.contracts >= 1, "delivered");
        assert!(c.cash > cash, "paid");
        assert!(c.rep > 0.0);
    }

    #[test]
    fn work_goes_in_priority_order() {
        let mut c = corp();
        c.offers.clear();
        let a = c.make_contract(0);
        let b = c.make_contract(0);
        c.contracts = vec![a, b];
        let mut sum = Summary::default();
        c.step(5.0, false, &mut sum);
        assert!(c.contracts[0].progress() > 0.0);
        // The second one only gets what the first doesn't need.
        let first_needs: Vec<bool> = (0..5).map(|s| c.contracts[0].need[s] > c.contracts[0].done[s]).collect();
        for (needs, done) in first_needs.iter().zip(c.contracts[1].done) {
            if *needs {
                assert_eq!(done, 0.0);
            }
        }
    }

    #[test]
    fn catching_up_is_capped_and_never_goes_into_the_red() {
        let mut c = corp();
        // A team too costly: it would go into the red.
        for _ in 0..1 {
            let e = c.make_employee("back", 5);
            c.staff.push(e);
        }
        c.cash = 100.0;
        let sum = c.catch_up(c.at + 24 * 3600);
        assert!(c.cash >= 0.0);
        assert!(sum.stopped);
        assert_eq!(c.at, 1_000_000 + 24 * 3600);

        let mut c = corp();
        c.cash = 1e9;
        let before = c.history.len();
        c.catch_up(c.at + 48 * 3600);
        // OFFLINE_CAP at OFFLINE_RATE: two hours, so 120 points at most.
        assert!(c.history.len() - before <= (OFFLINE_CAP * OFFLINE_RATE / 60.0) as usize + 1);
    }

    #[test]
    fn hiring_and_firing() {
        let mut c = corp();
        c.cash = 1e6;
        let n = c.staff.len();
        assert!(c.hire(0));
        assert_eq!(c.staff.len(), n + 1);
        // The garage takes two.
        assert!(!c.hire(0));
        let id = c.staff[1].id;
        assert!(c.fire(id));
        let founder = c.staff[0].id;
        assert!(!c.fire(founder));
        assert!(c.actions >= 2);
    }

    #[test]
    fn moving_perks_and_research() {
        let mut c = corp();
        c.cash = 1e7;
        assert!(c.buy_perk("coffee"));
        assert!(!c.buy_perk("plants"), "the garage has one slot");
        assert!(c.move_office());
        assert!(c.buy_perk("plants"));
        c.research = 1e9;
        assert!(!c.learn("framework"), "needs git");
        assert!(c.learn("git"));
        assert!(c.learn("framework"));
        assert!(c.mods().prod > 1.1);
    }

    #[test]
    fn a_product_launches_and_earns() {
        let mut c = corp();
        c.cash = 1e6;
        c.research = 1e9;
        assert!(!c.launch(0), "needs the cloud");
        assert!(c.learn("cloud"));
        assert!(c.launch(0));
        c.staff[0].task = Task::Products;
        c.staff[0].skills = [50.0; 5];
        let mut sum = Summary::default();
        for _ in 0..3000 {
            c.step(1.0, false, &mut sum);
        }
        assert!(c.products[0].version >= 1);
        assert!(c.products[0].users > 0.0);
        assert!(c.product_revenue() > 0.0);
    }

    #[test]
    fn every_event_choice_applies() {
        for (k, e) in EVENTS.iter().enumerate() {
            for choice in 0..e.choices.len() {
                let mut c = corp();
                c.cash = 1e6;
                let x = c.make_employee("front", 3);
                c.staff.push(x);
                c.offers.clear();
                let ct = c.make_contract(0);
                c.contracts.push(ct);
                c.event = Some(e.id.to_owned());
                c.event_who = c.staff[1].id;
                let outcome = c.choose(choice).unwrap_or_else(|| panic!("event {k} choice {choice}"));
                assert!(outcome < e.choices[choice].outcomes.len(), "{} {choice}", e.id);
                assert!(c.event.is_none());
                assert!(!news_text(c.news.last().unwrap(), true).is_empty());
                assert!(c.cash.is_finite());
            }
        }
    }

    #[test]
    fn a_long_run_stays_sane() {
        let mut c = corp();
        c.policy.auto_accept = true;
        c.cash = 50_000.0;
        let x = c.make_employee("sales", 1);
        c.staff.push(x);
        let mut sum = Summary::default();
        for k in 0..20_000 {
            c.step(1.0, true, &mut sum);
            if c.event.is_some() {
                let pick = k % 2;
                c.choose(pick);
            }
        }
        assert!(c.cash.is_finite() && c.rep.is_finite() && c.debt.is_finite());
        assert!(c.staff.iter().all(|e| e.morale.is_finite() && (0.0..=100.0).contains(&e.morale)));
        assert!(c.stats.contracts > 0);
        assert!(c.valuation() > 0.0);
        assert!(c.news.len() <= NEWS);
    }

    #[test]
    fn the_roles_play_by_themselves() {
        let mut c = corp();
        c.office = 3;
        c.stage = 2;
        c.cash = 5_000_000.0;
        c.research = 1e6;
        c.learn("cloud");
        for job in ["hr", "lead", "cto", "office", "po", "sales", "assistant"] {
            let e = c.make_employee(job, 1);
            c.staff.push(e);
        }
        let actions = c.actions;
        let staff = c.staff.len();
        c.event = Some("cat".into());
        let mut sum = Summary::default();
        for _ in 0..600 {
            c.step(1.0, true, &mut sum);
        }
        assert!(c.staff.len() > staff, "HR hired");
        assert!(!c.perks.is_empty(), "the office manager bought fittings");
        assert!(c.techs.len() > 1, "the CTO researched");
        assert!(!c.products.is_empty(), "the product owner launched");
        assert!(c.stats.events >= 1, "the assistant answered");
        assert!(c.cash > 0.0);
        assert_eq!(c.actions, actions, "not the player's actions");

        // Switched off: nothing.
        let mut c = corp();
        c.cash = 1e6;
        c.office = 2;
        let e = c.make_employee("hr", 1);
        c.staff.push(e);
        c.policy.auto.hire = false;
        let staff = c.staff.len();
        for _ in 0..300 {
            c.step(1.0, false, &mut sum);
        }
        assert_eq!(c.staff.len(), staff);
    }

    #[test]
    fn nobody_left_on_research_once_everything_is_found() {
        let mut c = corp();
        c.stage = 3;
        for t in TECHS {
            c.techs.push(t.id.to_owned());
        }
        c.launch(0);
        let lead = c.make_employee("lead", 1);
        c.staff.push(lead);
        for k in 0..30 {
            let mut e = c.make_employee(if k % 2 == 0 { "front" } else { "back" }, 1);
            e.task = if k < 25 { Task::Research } else { Task::Contracts };
            c.staff.push(e);
        }
        // Nothing to deliver: the project manager sends nobody to research.
        c.contracts.clear();
        c.offers.clear();
        c.auto_tasks();
        assert!(c.staff.iter().all(|e| e.task != Task::Research), "all back from research at once");
        for _ in 0..20 {
            c.auto_tasks();
        }
        assert!(c.staff.iter().all(|e| e.task != Task::Research));
    }

    #[test]
    fn selling_keeps_the_stars() {
        let mut c = corp();
        assert_eq!(c.sell(c.at), 0, "not before a small business");
        c.tutorial = TUTORIAL_DONE;
        c.stage = SELL_STAGE;
        c.cash = 5e6;
        let stars = c.sell(c.at + 10);
        assert!(stars >= 1);
        assert_eq!(c.legacy.stars, stars);
        assert_eq!(c.tutorial, TUTORIAL_DONE, "not seen again after a sale");
        assert_eq!(c.staff.len(), 1);
        assert!(c.cash > 1_500.0);
        assert!(c.mods().prod > 1.0);
    }

    #[test]
    fn saved_and_read_back() {
        let mut c = corp();
        c.cash = 1234.5;
        let text = serde_json::to_string(&c).unwrap();
        let back: Corp = serde_json::from_str(&text).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::from_str::<serde_json::Value>(&text).unwrap());
        assert_eq!(back.staff.len(), c.staff.len());
    }

    /// A player of sorts, for the pace: `cargo test balance -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn balance() {
        let mut c = corp();
        let mut sum = Summary::default();
        let mut reached = vec![0u64; STAGES.len()];
        for t in 0..(40 * 3600u64) {
            c.step(1.0, true, &mut sum);
            if c.event.is_some() {
                c.choose(0);
            }
            if t % 10 != 0 {
                continue;
            }
            while c.accept(0) {}
            let burn = c.costs() * 300.0;
            if c.staff.len() < c.capacity() {
                let pick = (0..c.candidates.len()).find(|&k| {
                    let e = &c.candidates[k];
                    let want_sales = c.sales() == 0 && c.staff.len() >= 2;
                    (e.job == "sales") == want_sales || (!want_sales && e.makes())
                });
                if let Some(k) = pick
                    && c.cash > c.hire_fee(&c.candidates[k]) + burn
                {
                    c.hire(k);
                }
            } else if c.office as usize + 1 < OFFICES.len() && c.cash > OFFICES[c.office as usize + 1].price * 1.3 + burn {
                c.move_office();
            }
            for p in PERKS.iter() {
                if c.cash > p.price * 3.0 + burn {
                    c.buy_perk(p.id);
                }
            }
            for t in TECHS.iter() {
                c.learn(t.id);
            }
            for (k, p) in PRODUCTS.iter().enumerate() {
                if c.cash > p.cost * 2.0 + burn {
                    c.launch(k);
                }
            }
            let n = c.staff.len();
            let ids: Vec<u32> = c.staff.iter().map(|e| e.id).collect();
            for (k, id) in ids.into_iter().enumerate() {
                let task = if !c.products.is_empty() && k % 3 == 1 { Task::Products } else if k % 7 == 6 { Task::Research } else if c.debt > 40.0 && k == n - 1 { Task::Refactor } else { Task::Contracts };
                c.set_task(id, task);
            }
            if reached[c.stage as usize] == 0 {
                reached[c.stage as usize] = t;
                println!("{:>6} min  stage {} ({})  cash {}  staff {}  office {}  rep {:.0}  tier {}  techs {}  products {}  debt {:.0}  income {:.1}/s spend {:.1}/s", t / 60, c.stage, STAGES[c.stage as usize].name.0, money(c.cash, true), c.staff.len(), c.office, c.rep, c.tier(), c.techs.len(), c.products.len(), c.debt, c.income, c.spend);
            }
            if t % 3600 == 0 {
                println!("  h{:<3} cash {}  val {}  staff {}  office {}  rep {:.0} tier {}  techs {}  products {:?}  morale {:.0}  debt {:.0}", t / 3600, money(c.cash, true), money(c.valuation(), true), c.staff.len(), c.office, c.rep, c.tier(), c.techs.len(), c.products.iter().map(|p| (p.version, p.users as u64)).collect::<Vec<_>>(), c.staff.iter().map(|e| e.morale).sum::<f64>() / c.staff.len() as f64, c.debt);
            }
        }
    }

    #[test]
    fn money_worded() {
        assert_eq!(money(950.0, false), "950 €");
        assert_eq!(money(12_500.0, true), "12,5 k€");
        assert_eq!(money(3_400_000.0, false), "3.40M €");
        assert_eq!(money(-20.0, true), "−20 €");
    }

    #[test]
    fn the_rewards_go_by_the_furthest_stage() {
        assert_eq!(stage_of(0.0), 0);
        assert_eq!(stage_of(499_999.0), 1);
        assert_eq!(stage_of(500_000.0), BILLS_STAGE);
        assert_eq!(stage_of(2e9), UNICORN_STAGE);
        assert_eq!(stage_of(1e15), LISTED_STAGE);
        // Sold at the small business stage: back in the garage, the stage reached kept.
        let mut c = Corp::new(1_000, 7, Legacy::default());
        c.stage = BILLS_STAGE;
        c.cash = 600_000.0;
        assert!(c.sell(2_000) > 0);
        assert_eq!(c.stage, 0);
        assert_eq!(c.best_stage(), BILLS_STAGE);
        assert!(c.legacy.best_sale >= 600_000.0);
    }

    #[test]
    fn the_cheque_reads_back() {
        let text = cheque_text(12_300_000.0, true);
        assert_eq!(cheque_amount(&text), Some("12,3 M€"));
        assert_eq!(cheque_amount("💸 Ronnie Corp · pas un montant"), None);
        assert_eq!(cheque_amount("Ronnie Corp · 12 M€"), None);
    }
}
