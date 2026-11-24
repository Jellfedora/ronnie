//! Open Space, the rules: the player's open space on a grid, its buildings (the boss's office, the
//! coffee machines and the production servers, their fridges and safes, the interns who build, the
//! army's rooms, the defenses and the firewalls), each one built then upgraded level after level, in
//! real time (seconds at first, days at the end), even with Ronnie closed.
//!
//! The boss's office (`Kind::Hq`) caps everything: how many of each building, and up to which level.
//! Coffee (☕) and budget (💶) are made by their machines, kept in them until collected, then stored
//! up to what the fridges and safes hold. Tickets (🎫, rare) finish a work at once.
//!
//! No UI here (see `app::openspace`): a plain serde value, saved in the settings, sealed with the
//! rewards and synced with floor (the side with the most actions wins, see `crate::progress`).
//! The texts are in both languages, here (`Tx`), as Ronnie Corp's.

use serde::{Deserialize, Serialize};

/// A text in French and in English.
pub type Tx = (&'static str, &'static str);

pub fn tx(t: Tx, fr: bool) -> &'static str {
    if fr { t.0 } else { t.1 }
}

/// The grid's side, in tiles.
pub const GRID: i32 = 40;
/// The boss's office's last level.
pub const HQ_MAX: u8 = 10;
/// Tickets given at the start.
pub const START_TICKETS: u32 = 50;

/// A building's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The boss's office.
    Hq,
    /// A coffee machine: makes coffee.
    Coffee,
    /// A production server: makes budget.
    Server,
    /// A fridge: stores coffee.
    Fridge,
    /// A safe: stores budget.
    Safe,
    /// The interns' desk: one more builder.
    Interns,
    /// The recruiting room: the troops.
    Recruit,
    /// The break room: room for the army.
    Lounge,
    /// The meeting room: the troops upgraded.
    Meeting,
    /// A firewall: a wall.
    Wall,
    /// The linter turret: shoots far, one target.
    Linter,
    /// The code review desk: hits hard, close.
    Review,
    /// The fire alarm: area damage, slow.
    Alarm,
    /// The antivirus: shoots down what flies.
    Antivirus,
    /// The honeypot: hidden, zaps up close.
    Honeypot,
    /// The cluster: very fast shots, very far.
    Cluster,
    /// The board of directors: a beam that grows stronger.
    Board,
    /// A hidden bug: explodes (a trap).
    Bug,
    /// A jamming printer: blocks the troops (a trap).
    Printer,
    /// The air conditioning on full blast: slows the troops (a trap).
    Aircon,
    /// The training room: the processes (the spells).
    Training,
    /// The lead dev's office: the hero.
    Heroes,
    /// Decorations: for the look.
    Plant,
    Foosball,
    Fountain,
    Sofa,
    Arcade,
    Statue,
    /// Obstacles, to clear: never built.
    Boxes,
    OldPc,
    DeadPlant,
    Chairs,
}

/// Every kind that can be built, in the shop's order.
pub const KINDS: [Kind; 28] = [
    Kind::Hq, Kind::Coffee, Kind::Server, Kind::Fridge, Kind::Safe, Kind::Interns, Kind::Recruit, Kind::Lounge, Kind::Meeting, Kind::Training, Kind::Heroes, Kind::Wall, Kind::Review, Kind::Linter, Kind::Alarm, Kind::Antivirus,
    Kind::Honeypot, Kind::Cluster, Kind::Board, Kind::Bug, Kind::Printer, Kind::Aircon, Kind::Plant, Kind::Fountain, Kind::Foosball, Kind::Sofa, Kind::Arcade, Kind::Statue,
];

/// The obstacles, and what clearing each costs (budget).
pub const OBSTACLES: [(Kind, u64); 4] = [(Kind::DeadPlant, 50), (Kind::OldPc, 250), (Kind::Boxes, 100), (Kind::Chairs, 150)];
/// How many obstacles at most, and how often one comes back (seconds).
const OBSTACLES_MAX: usize = 30;
const OBSTACLE_EVERY: i64 = 8 * 3_600;

/// A resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Res {
    Coffee,
    Budget,
}

/// The shop's sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Economy,
    Army,
    Defense,
    Traps,
    Decor,
    Obstacle,
}

pub const GROUPS: [(Group, Tx); 5] = [(Group::Economy, ("Économie", "Economy")), (Group::Army, ("Armée", "Army")), (Group::Defense, ("Défenses", "Defenses")), (Group::Traps, ("Pièges", "Traps")), (Group::Decor, ("Décorations", "Decorations"))];

/// The build times, by step (seconds): from 10 s to 3 days.
const TIMES: [u32; 16] = [10, 60, 300, 900, 1_800, 3_600, 7_200, 14_400, 28_800, 43_200, 64_800, 86_400, 129_600, 172_800, 216_000, 259_200];

/// The boss's office's upgrades: to level 2, 3… (seconds, budget).
const HQ_UPGRADES: [(u32, u64); 9] = [(300, 1_000), (1_800, 4_000), (10_800, 25_000), (28_800, 150_000), (43_200, 500_000), (86_400, 1_200_000), (129_600, 2_000_000), (172_800, 3_000_000), (259_200, 4_000_000)];

/// What a fridge or a safe holds, by level.
const STORAGE: [f64; 11] = [1_500.0, 3_000.0, 6_000.0, 12_000.0, 25_000.0, 45_000.0, 100_000.0, 225_000.0, 450_000.0, 850_000.0, 1_750_000.0];
/// What a coffee machine or a server makes in an hour, by level.
const PER_HOUR: [f64; 12] = [200.0, 400.0, 600.0, 800.0, 1_000.0, 1_300.0, 1_600.0, 1_900.0, 2_200.0, 2_800.0, 3_500.0, 4_200.0];
/// What a coffee machine or a server keeps before being full, by level.
const KEEPS: [f64; 12] = [1_000.0, 2_000.0, 3_000.0, 5_000.0, 10_000.0, 20_000.0, 30_000.0, 50_000.0, 75_000.0, 100_000.0, 150_000.0, 200_000.0];
/// A firewall's upgrades: level 1, 2… (budget). Built at once.
const WALL_COSTS: [u64; 10] = [50, 1_000, 5_000, 10_000, 20_000, 30_000, 50_000, 75_000, 100_000, 200_000];
/// The interns' desks: the first, the second… (budget).
const INTERNS_COSTS: [u64; 5] = [0, 1_000, 5_000, 25_000, 80_000];

/// What the boss's office stores by itself, of each resource, by its level.
pub fn hq_storage(level: u8) -> f64 {
    [1_000.0, 2_500.0, 10_000.0, 50_000.0, 100_000.0, 300_000.0, 500_000.0, 750_000.0, 1_000_000.0, 1_500_000.0][(level.clamp(1, HQ_MAX) - 1) as usize]
}

impl Kind {
    pub fn name(self) -> Tx {
        match self {
            Kind::Hq => ("Bureau du boss", "Boss's office"),
            Kind::Coffee => ("Machine à café", "Coffee machine"),
            Kind::Server => ("Serveur de prod", "Production server"),
            Kind::Fridge => ("Frigo", "Fridge"),
            Kind::Safe => ("Coffre-fort", "Safe"),
            Kind::Interns => ("Bureau des alternants", "Interns' desk"),
            Kind::Recruit => ("Salle de recrutement", "Recruiting room"),
            Kind::Lounge => ("Salle de pause", "Break room"),
            Kind::Meeting => ("Salle de réunion", "Meeting room"),
            Kind::Wall => ("Pare-feu", "Firewall"),
            Kind::Linter => ("Tourelle linter", "Linter turret"),
            Kind::Review => ("Bureau du code review", "Code review desk"),
            Kind::Alarm => ("Alarme incendie", "Fire alarm"),
            Kind::Antivirus => ("Antivirus", "Antivirus"),
            Kind::Honeypot => ("Pot de miel", "Honeypot"),
            Kind::Cluster => ("Le Cluster", "The Cluster"),
            Kind::Board => ("Comité de direction", "Board of directors"),
            Kind::Bug => ("Bug caché", "Hidden bug"),
            Kind::Printer => ("Imprimante qui bourre", "Jamming printer"),
            Kind::Aircon => ("Clim à fond", "Air con on full blast"),
            Kind::Training => ("Salle de formation", "Training room"),
            Kind::Heroes => ("Bureau du Lead Dev", "Lead dev's office"),
            Kind::Plant => ("Plante verte", "Green plant"),
            Kind::Foosball => ("Baby-foot", "Foosball table"),
            Kind::Fountain => ("Fontaine à eau", "Water cooler"),
            Kind::Sofa => ("Canapé", "Sofa"),
            Kind::Arcade => ("Borne d'arcade", "Arcade cabinet"),
            Kind::Statue => ("Statue du fondateur", "Founder's statue"),
            Kind::Boxes => ("Cartons de déménagement", "Moving boxes"),
            Kind::OldPc => ("Vieux PC", "Old PC"),
            Kind::DeadPlant => ("Plante morte", "Dead plant"),
            Kind::Chairs => ("Chaises cassées", "Broken chairs"),
        }
    }

    pub fn about(self) -> Tx {
        match self {
            Kind::Hq => ("Le cœur de ton open space. Son niveau fixe combien de bâtiments tu peux avoir, et jusqu'à quel niveau. S'il tombe pendant un raid, l'étoile est perdue.", "The heart of your open space. Its level sets how many buildings you can have, and up to which level. If it falls in a raid, that star is lost."),
            Kind::Coffee => ("Fait du café, même Ronnie fermé. Il le garde jusqu'à ce que tu le récoltes : plein, il s'arrête.", "Makes coffee, even with Ronnie closed. It keeps it until you collect it: full, it stops."),
            Kind::Server => ("Rapporte du budget, même Ronnie fermé. Il le garde jusqu'à ce que tu le récoltes : plein, il s'arrête.", "Brings in budget, even with Ronnie closed. It keeps it until you collect it: full, it stops."),
            Kind::Fridge => ("Garde ton café. Plus il est grand, plus tu peux viser de grosses améliorations de troupes.", "Stores your coffee. The bigger it is, the bigger the troop upgrades you can aim for."),
            Kind::Safe => ("Garde ton budget. Plus il est grand, plus tu peux viser de gros chantiers.", "Stores your budget. The bigger it is, the bigger the builds you can aim for."),
            Kind::Interns => ("Un alternant de plus : un chantier de plus en même temps.", "One more intern: one more work at the same time."),
            Kind::Recruit => ("On y recrute les troupes pour les raids.", "Where the troops for raids are recruited."),
            Kind::Lounge => ("La place de ton armée : plus elle est grande, plus tu emmènes de monde en raid.", "Your army's room: the bigger, the more people you take on a raid."),
            Kind::Meeting => ("On y fait monter tes troupes de niveau.", "Where your troops level up."),
            Kind::Wall => ("Ralentit les attaquants. Construit tout de suite, sans alternant.", "Slows the attackers down. Built at once, no intern needed."),
            Kind::Linter => ("Tire de loin, une cible à la fois, au sol comme en l'air.", "Shoots from afar, one target at a time, ground and air."),
            Kind::Review => ("Frappe fort, de près, au sol.", "Hits hard, up close, on the ground."),
            Kind::Alarm => ("Arrose une zone, lentement : redoutable contre les groupes. Ne voit pas tout près.", "Sprays an area, slowly: deadly against groups. Blind up close."),
            Kind::Antivirus => ("Abat ce qui vole, très vite. Ne touche pas le sol.", "Shoots down what flies, very fast. Can't hit the ground."),
            Kind::Honeypot => ("Caché jusqu'à ce qu'on s'approche, puis électrocute.", "Hidden until someone comes close, then zaps."),
            Kind::Cluster => ("Une rafale continue, très loin, au sol comme en l'air.", "A non-stop burst, very far, ground and air."),
            Kind::Board => ("Un rayon qui fait de plus en plus mal tant qu'il vise la même cible.", "A beam that hurts more and more while it stays on the same target."),
            Kind::Bug => ("Piège invisible : explose quand on marche dessus.", "Invisible trap: explodes when stepped on."),
            Kind::Printer => ("Piège invisible : bloque les troupes qui passent un moment.", "Invisible trap: holds the troops passing by for a while."),
            Kind::Aircon => ("Piège invisible : ralentit tout le monde autour.", "Invisible trap: slows down everyone around."),
            Kind::Training => ("Une carte action de plus dans ton deck de raid (et encore une au niveau 3).", "One more action card in your raid deck (and another at level 3)."),
            Kind::Heroes => ("Le bureau de ton héros, le Lead Dev : il part en raid avec toi. Chaque niveau du bureau lui ouvre 5 niveaux de plus.", "Your hero's office, the lead dev: they come on raids with you. Each office level opens 5 more levels to them."),
            Kind::Plant | Kind::Foosball | Kind::Fountain | Kind::Sofa | Kind::Arcade | Kind::Statue => ("Une décoration : elle ne sert à rien… sauf à rendre ton open space plus beau.", "A decoration: useless… except to make your open space look better."),
            Kind::Boxes | Kind::OldPc | Kind::DeadPlant | Kind::Chairs => ("Ça encombre. Débarrasse-le pour faire de la place : ça rapporte un peu d'XP, et parfois des tickets RH.", "It's in the way. Clear it to make room: it gives a little XP, and sometimes HR tickets."),
        }
    }

    /// What the card shown the first time one is built says: how it works, and a tip.
    pub fn guide(self) -> Tx {
        match self {
            Kind::Hq => ("Le bureau du boss est le cœur de ton open space. Son niveau fixe combien de bâtiments de chaque sorte tu peux avoir, et jusqu'à quel niveau. Il garde aussi un peu de café et de budget.\n\nAstuce : améliore tout le reste avant lui, il coûte cher et immobilise un alternant longtemps.", "The boss's office is the heart of your open space. Its level sets how many buildings of each kind you can have, and up to which level. It also holds a little coffee and budget.\n\nTip: upgrade everything else before it, it costs a lot and ties up an intern for a long time."),
            Kind::Coffee => ("La machine à café produit du café en continu, même Ronnie fermé, et le garde jusqu'à ce que tu le récoltes (la bulle au-dessus). Pleine, elle s'arrête. Le café paie les troupes et les salles de l'armée.\n\nAstuce : plus son niveau est haut, plus elle produit vite et plus elle garde longtemps sans déborder.", "The coffee machine makes coffee non-stop, even with Ronnie closed, and keeps it until you collect it (the bubble over it). Full, it stops. Coffee pays for the troops and the army's rooms.\n\nTip: the higher its level, the faster it makes and the longer it keeps without overflowing."),
            Kind::Server => ("Le serveur de prod rapporte du budget en continu, même Ronnie fermé, et le garde jusqu'à ce que tu le récoltes. Plein, il s'arrête. Le budget paie presque tous les bâtiments.\n\nAstuce : ses LED clignotent tant qu'il tourne ; en rouge, il est en travaux.", "The production server brings in budget non-stop, even with Ronnie closed, and keeps it until you collect it. Full, it stops. Budget pays for nearly every building.\n\nTip: its LEDs blink while it runs; red, it is under work."),
            Kind::Fridge => ("Le frigo augmente ce que tu peux stocker de café. Tant que tes stocks sont pleins, ce que tu récoltes reste dans les machines.\n\nAstuce : en raid, les attaquants visent aussi tes frigos : protège-les derrière tes pare-feux.", "The fridge raises how much coffee you can store. While your stock is full, what you collect stays in the machines.\n\nTip: in raids, attackers go for your fridges too: shelter them behind your firewalls."),
            Kind::Safe => ("Le coffre-fort augmente ce que tu peux stocker de budget. Les gros chantiers (comme le bureau du boss) en demandent beaucoup : sans coffre, impossible de les payer.\n\nAstuce : mets-le au centre, c'est la cible préférée des raids.", "The safe raises how much budget you can store. Big works (like the boss's office) need a lot of it: without safes, you can't pay for them.\n\nTip: put it in the middle, it's the raids' favourite target."),
            Kind::Interns => ("Chaque bureau des alternants te donne un alternant de plus, donc un chantier de plus en même temps. Le premier est gratuit, les suivants coûtent de plus en plus cher.\n\nAstuce : c'est l'investissement le plus rentable du jeu : tout va deux fois plus vite avec deux alternants.", "Each interns' desk gives you one more intern, so one more work at the same time. The first is free, the next ones cost more and more.\n\nTip: it's the best investment in the game: everything goes twice as fast with two interns."),
            Kind::Recruit => ("La salle de recrutement est l'endroit où tu formeras tes troupes : stagiaires, devs, testeurs… Chaque niveau débloque une nouvelle troupe.","The recruiting room is where you'll train your troops: interns, devs, testers… Each level unlocks a new troop."),
            Kind::Lounge => ("La salle de pause, c'est la place de ton armée : plus elle est grande (et plus tu en as), plus tu emmènes de monde en raid.","The break room is your army's room: the bigger it is (and the more you have), the more people you take on a raid."),
            Kind::Meeting => ("La salle de réunion fait monter tes troupes de niveau : plus de points de vie, plus de dégâts. C'est long et ça coûte du café.","The meeting room levels your troops up: more hit points, more damage. It takes long and costs coffee."),
            Kind::Wall => ("Le pare-feu ralentit les attaquants : ils doivent le casser ou le contourner, pendant que tes défenses tirent. Il se construit tout de suite, sans alternant.\n\nAstuce : pose-les en ligne (ils se relient) pour fermer des compartiments autour de tes défenses et de tes stocks. Clique sur le sol plusieurs fois pour en poser à la suite.", "The firewall slows attackers down: they have to break it or go around, while your defenses shoot. It's built at once, no intern needed.\n\nTip: put them in lines (they join up) to close compartments around your defenses and storage. Click the floor several times to place them one after the other."),
            Kind::Linter => ("La tourelle linter tire de loin, une cible à la fois, au sol comme en l'air. Elle couvre une grande zone.\n\nAstuce : place-la derrière tes pare-feux, pour qu'elle tire pendant que les attaquants cassent les murs.", "The linter turret shoots from afar, one target at a time, ground and air. It covers a large area.\n\nTip: put it behind your firewalls, so it shoots while the attackers break the walls."),
            Kind::Review => ("Le bureau du code review frappe fort, de près, et seulement au sol. Rien ne passe sans sa validation.\n\nAstuce : près de ton QG et de tes stocks, c'est ta première vraie défense.", "The code review desk hits hard, up close, ground only. Nothing gets through without its approval.\n\nTip: near your HQ and your storage, it's your first real defense."),
            Kind::Alarm => ("L'alarme incendie arrose une zone entière, lentement : redoutable contre les groupes de troupes. Mais elle ne voit rien tout près d'elle.\n\nAstuce : au centre de ton open space, entourée d'autres défenses qui la protègent.", "The fire alarm sprays a whole area, slowly: deadly against groups of troops. But it can't see right next to it.\n\nTip: in the middle of your open space, with other defenses around to protect it."),
            Kind::Antivirus => ("L'antivirus abat les unités volantes (les drones livreurs…) à toute vitesse, mais ne peut rien contre ce qui marche.\n\nAstuce : réparti en deux ou trois coins, il couvre tout ton ciel.", "The antivirus shoots down flying units (delivery drones…) at full speed, but can't touch what walks.\n\nTip: spread in two or three corners, it covers your whole sky."),
            Kind::Honeypot => ("Le pot de miel ressemble à un serveur oublié… jusqu'à ce qu'un attaquant s'approche : il sort alors et l'électrocute. Les attaquants ne le voient pas avant.\n\nAstuce : cache-le près de ce que les attaquants viennent chercher (coffres, QG).", "The honeypot looks like a forgotten server… until an attacker comes close: then it pops out and zaps them. Attackers can't see it before.\n\nTip: hide it near what attackers come for (safes, HQ)."),
            Kind::Cluster => ("Le Cluster tire une rafale continue, très loin, au sol comme en l'air. Une des défenses les plus redoutables du jeu.\n\nAstuce : au cœur de ton open space, protégé par plusieurs rangées de pare-feux.", "The Cluster fires a non-stop burst, very far, ground and air. One of the deadliest defenses in the game.\n\nTip: at the heart of your open space, behind several rows of firewalls."),
            Kind::Board => ("Le Comité de direction fixe une cible avec un rayon qui fait de plus en plus mal tant qu'il ne la lâche pas. Les grosses troupes fondent.\n\nAstuce : il fait la différence contre les commerciaux et les consultants, les plus costauds.", "The board of directors locks a beam on a target that hurts more and more while it holds on. Big troops melt.\n\nTip: it makes the difference against salespeople and consultants, the toughest."),
            Kind::Bug => ("Le bug caché est un piège invisible : il explose quand des troupes marchent dessus, et blesse tout le groupe.\n\nAstuce : mets-le dans les passages entre tes pare-feux, là où les troupes se pressent.", "The hidden bug is an invisible trap: it explodes when troops step on it, and hurts the whole group.\n\nTip: put it in the gaps between your firewalls, where troops crowd."),
            Kind::Printer => ("L'imprimante qui bourre est un piège invisible : les troupes qui passent dessus restent bloquées un moment à essayer de la réparer.\n\nAstuce : devant une défense qui frappe fort, pour qu'elle ait le temps de tirer.", "The jamming printer is an invisible trap: troops passing over it get stuck a while trying to fix it.\n\nTip: in front of a hard-hitting defense, so it has time to shoot."),
            Kind::Aircon => ("La clim à fond est un piège invisible : elle ralentit toutes les troupes autour pendant un moment.\n\nAstuce : au milieu de tes défenses, elle les laisse tirer bien plus longtemps.", "The air con on full blast is an invisible trap: it slows every troop around for a while.\n\nTip: in the middle of your defenses, it lets them shoot much longer."),
            Kind::Training => ("La salle de formation agrandit ton deck de raid : une carte action de plus (Hotfix, Daily stand-up, Gel du code…), et encore une au niveau 3.\n\nChoisis tes cartes dans « Cartes ».", "The training room makes your raid deck bigger: one more action card (Hotfix, Daily stand-up, Code freeze…), and another at level 3.\n\nPick your cards in « Cards »."),
            Kind::Heroes => ("Le bureau du Lead Dev accueille ton héros. Il part en raid avec ton armée sans prendre de place : pose-le comme une troupe, puis déclenche « Mode crunch » quand ça chauffe (il récupère des PV, lui et les troupes autour foncent).\n\nS'il tombe, il se repose un moment avant le prochain raid. Entraîne-le ici pour le faire monter de niveau : chaque niveau du bureau lui en ouvre 5 de plus.", "The lead dev's office hosts your hero. They come on raids with your army without taking room: drop them like a troop, then fire « Crunch mode » when it gets hot (they get HP back, they and the troops around rush).\n\nIf they fall, they rest a while before the next raid. Train them here to level them up: each office level opens 5 more levels to them."),
            Kind::Plant | Kind::Foosball | Kind::Fountain | Kind::Sofa | Kind::Arcade | Kind::Statue => ("Les décorations ne servent à rien en raid : elles rendent juste ton open space plus beau, et montrent à tes collègues jusqu'où tu es allé·e.\n\nAstuce : tu peux les déplacer à tout moment, comme le reste.", "Decorations do nothing in raids: they just make your open space look better, and show your colleagues how far you've come.\n\nTip: you can move them anytime, like everything else."),
            Kind::Boxes | Kind::OldPc | Kind::DeadPlant | Kind::Chairs => ("Ton open space est encombré : cartons, vieux PC, plantes mortes, chaises cassées… Débarrasse-les pour faire de la place. Ça rapporte de l'XP, et parfois des tickets RH. De nouveaux reviennent avec le temps.", "Your open space is cluttered: boxes, old PCs, dead plants, broken chairs… Clear them to make room. It gives XP, and sometimes HR tickets. New ones come back over time."),
        }
    }

    pub fn is_obstacle(self) -> bool {
        self.group() == Group::Obstacle
    }

    /// Built and upgraded at once, without an intern.
    pub fn instant(self) -> bool {
        matches!(self.group(), Group::Traps | Group::Decor) || matches!(self, Kind::Wall | Kind::Interns)
    }

    /// Its side, in tiles (it is square).
    pub fn size(self) -> i32 {
        match self {
            Kind::Hq | Kind::Lounge => 4,
            Kind::Interns | Kind::Honeypot | Kind::Foosball | Kind::Sofa | Kind::Statue | Kind::Boxes | Kind::Chairs => 2,
            Kind::Wall | Kind::Bug | Kind::Printer | Kind::Aircon | Kind::Plant | Kind::Fountain | Kind::Arcade | Kind::OldPc | Kind::DeadPlant => 1,
            _ => 3,
        }
    }

    pub fn group(self) -> Group {
        match self {
            Kind::Recruit | Kind::Lounge | Kind::Meeting | Kind::Training | Kind::Heroes => Group::Army,
            Kind::Wall | Kind::Linter | Kind::Review | Kind::Alarm | Kind::Antivirus | Kind::Honeypot | Kind::Cluster | Kind::Board => Group::Defense,
            Kind::Bug | Kind::Printer | Kind::Aircon => Group::Traps,
            Kind::Plant | Kind::Foosball | Kind::Fountain | Kind::Sofa | Kind::Arcade | Kind::Statue => Group::Decor,
            Kind::Boxes | Kind::OldPc | Kind::DeadPlant | Kind::Chairs => Group::Obstacle,
            _ => Group::Economy,
        }
    }

    /// What it is built and upgraded with.
    pub fn paid_with(self) -> Res {
        match self {
            Kind::Server | Kind::Safe | Kind::Recruit | Kind::Lounge | Kind::Meeting | Kind::Training | Kind::Heroes => Res::Coffee,
            _ => Res::Budget,
        }
    }

    /// What it makes, if anything.
    pub fn makes(self) -> Option<Res> {
        match self {
            Kind::Coffee => Some(Res::Coffee),
            Kind::Server => Some(Res::Budget),
            _ => None,
        }
    }

    /// What it stores, if anything.
    pub fn stores(self) -> Option<Res> {
        match self {
            Kind::Fridge => Some(Res::Coffee),
            Kind::Safe => Some(Res::Budget),
            _ => None,
        }
    }

    /// How many the boss's office at `hq` allows, and up to which level (0: not yet).
    pub fn allowed(self, hq: u8) -> (u32, u8) {
        let i = (hq.clamp(1, HQ_MAX) - 1) as usize;
        let (count, level): ([u32; 10], [u8; 10]) = match self {
            Kind::Hq => ([1; 10], [HQ_MAX; 10]),
            Kind::Coffee | Kind::Server => ([1, 2, 3, 4, 5, 5, 6, 6, 7, 7], [2, 4, 6, 8, 10, 10, 11, 12, 12, 12]),
            Kind::Fridge | Kind::Safe => ([1, 1, 2, 2, 2, 3, 3, 4, 4, 4], [1, 3, 6, 8, 9, 10, 11, 11, 11, 11]),
            Kind::Interns => ([2, 2, 3, 3, 4, 4, 5, 5, 5, 5], [1; 10]),
            Kind::Recruit => ([1, 2, 2, 3, 3, 3, 4, 4, 4, 4], [3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
            Kind::Lounge => ([1, 1, 2, 2, 3, 3, 4, 4, 4, 4], [1, 2, 3, 4, 5, 6, 6, 7, 8, 9]),
            Kind::Meeting => ([0, 0, 1, 1, 1, 1, 1, 1, 1, 1], [0, 0, 1, 2, 3, 4, 5, 6, 7, 8]),
            Kind::Wall => ([0, 25, 50, 75, 100, 125, 175, 225, 250, 275], [0, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            Kind::Linter => ([0, 1, 1, 2, 3, 3, 4, 5, 6, 7], [0, 2, 3, 4, 6, 7, 8, 10, 11, 12]),
            Kind::Review => ([2, 2, 2, 2, 3, 3, 5, 5, 6, 7], [2, 3, 4, 5, 6, 7, 8, 10, 11, 12]),
            Kind::Alarm => ([0, 0, 1, 1, 1, 2, 3, 3, 4, 4], [0, 0, 1, 2, 3, 4, 5, 6, 7, 8]),
            Kind::Antivirus => ([0, 0, 0, 2, 2, 3, 3, 3, 4, 4], [0, 0, 0, 2, 3, 4, 5, 6, 7, 8]),
            Kind::Honeypot => ([0, 0, 0, 0, 1, 2, 3, 4, 4, 5], [0, 0, 0, 0, 2, 3, 4, 5, 6, 7]),
            Kind::Cluster => ([0, 0, 0, 0, 0, 0, 0, 2, 3, 4], [0, 0, 0, 0, 0, 0, 0, 2, 3, 4]),
            Kind::Board => ([0, 0, 0, 0, 0, 0, 0, 0, 2, 3], [0, 0, 0, 0, 0, 0, 0, 0, 2, 3]),
            Kind::Bug => ([0, 0, 2, 2, 4, 4, 6, 6, 6, 6], [0, 0, 1, 2, 3, 3, 4, 5, 6, 7]),
            Kind::Printer => ([0, 0, 0, 2, 2, 4, 4, 6, 6, 6], [0, 0, 0, 1, 1, 2, 2, 3, 3, 4]),
            Kind::Aircon => ([0, 0, 0, 0, 0, 1, 2, 2, 3, 3], [0, 0, 0, 0, 0, 1, 2, 3, 3, 4]),
            Kind::Training => ([0, 0, 0, 0, 1, 1, 1, 1, 1, 1], [0, 0, 0, 0, 2, 3, 4, 5, 5, 6]),
            Kind::Heroes => ([0, 0, 0, 0, 0, 0, 1, 1, 1, 1], [0, 0, 0, 0, 0, 0, 1, 2, 3, 4]),
            Kind::Plant => ([2, 4, 6, 8, 10, 10, 10, 10, 10, 10], [1; 10]),
            Kind::Fountain => ([1; 10], [1; 10]),
            Kind::Foosball => ([0, 1, 1, 1, 1, 1, 1, 1, 1, 1], [1; 10]),
            Kind::Sofa => ([0, 0, 2, 2, 2, 2, 2, 2, 2, 2], [1; 10]),
            Kind::Arcade => ([0, 0, 0, 0, 1, 1, 1, 1, 1, 1], [1; 10]),
            Kind::Statue => ([0, 0, 0, 0, 0, 0, 0, 1, 1, 1], [1; 10]),
            Kind::Boxes | Kind::OldPc | Kind::DeadPlant | Kind::Chairs => ([0; 10], [0; 10]),
        };
        (count[i], level[i])
    }

    /// The first boss's office level that allows it.
    pub fn unlocked_at(self) -> u8 {
        (1..=HQ_MAX).find(|&hq| self.allowed(hq).0 > 0).unwrap_or(HQ_MAX)
    }

    /// The highest level it can ever reach.
    pub fn max_level(self) -> u8 {
        self.allowed(HQ_MAX).1
    }

    /// Its level-1 cost, and how the next ones grow.
    fn base_cost(self) -> f64 {
        match self {
            Kind::Coffee | Kind::Server => 150.0,
            Kind::Fridge | Kind::Safe => 300.0,
            Kind::Recruit => 200.0,
            Kind::Lounge => 250.0,
            Kind::Meeting => 25_000.0,
            Kind::Linter => 1_000.0,
            Kind::Review => 250.0,
            Kind::Alarm => 8_000.0,
            Kind::Antivirus => 4_000.0,
            Kind::Honeypot => 15_000.0,
            Kind::Cluster => 300_000.0,
            Kind::Board => 800_000.0,
            Kind::Bug => 400.0,
            Kind::Printer => 2_000.0,
            Kind::Aircon => 25_000.0,
            Kind::Training => 50_000.0,
            Kind::Heroes => 120_000.0,
            Kind::Plant => 200.0,
            Kind::Fountain => 500.0,
            Kind::Foosball => 3_000.0,
            Kind::Sofa => 5_000.0,
            Kind::Arcade => 50_000.0,
            Kind::Statue => 400_000.0,
            Kind::Boxes | Kind::OldPc | Kind::DeadPlant | Kind::Chairs => 0.0,
            Kind::Hq | Kind::Wall | Kind::Interns => 0.0,
        }
    }

    /// Where its build times start in `TIMES`.
    fn time_step(self) -> usize {
        match self {
            Kind::Coffee | Kind::Server => 0,
            Kind::Fridge | Kind::Safe | Kind::Recruit | Kind::Review => 1,
            Kind::Lounge => 2,
            Kind::Linter => 3,
            Kind::Meeting | Kind::Alarm | Kind::Antivirus => 5,
            Kind::Honeypot => 6,
            Kind::Training => 7,
            Kind::Heroes => 8,
            Kind::Cluster => 9,
            Kind::Board => 10,
            _ if self.instant() || self.is_obstacle() => 0,
            _ => 0,
        }
    }

    /// A building's hit points at `level` (the raids).
    pub fn hp(self, level: u8) -> u32 {
        let base = match self {
            Kind::Hq => 1_500.0,
            Kind::Wall => 300.0,
            Kind::Linter | Kind::Review | Kind::Alarm | Kind::Antivirus | Kind::Honeypot => 400.0,
            Kind::Cluster | Kind::Board => 1_200.0,
            Kind::Bug | Kind::Printer | Kind::Aircon => 50.0,
            k if k.group() == Group::Decor || k.is_obstacle() => 100.0,
            Kind::Interns => 250.0,
            _ => 300.0,
        };
        (base * (1.0 + 0.3 * (level.max(1) - 1) as f64)).round() as u32
    }
}

/// Rounded to two significant figures (a price that reads well).
fn round_price(x: f64) -> u64 {
    if x < 100.0 {
        return x.round() as u64;
    }
    let p = 10f64.powi(x.log10().floor() as i32 - 1);
    ((x / p).round() * p) as u64
}

/// What getting `kind` to `level` costs (`level` 1: building it; `nth`: how many of it already, for
/// the interns' desks).
pub fn cost(kind: Kind, level: u8, nth: usize) -> (Res, u64) {
    let level = level.max(1);
    let amount = match kind {
        Kind::Hq => HQ_UPGRADES[(level.clamp(2, HQ_MAX) - 2) as usize].1,
        Kind::Wall => WALL_COSTS[(level.min(10) - 1) as usize],
        Kind::Interns => INTERNS_COSTS[nth.min(INTERNS_COSTS.len() - 1)],
        _ => round_price(kind.base_cost() * 2.0f64.powi(level as i32 - 1)),
    };
    (kind.paid_with(), amount)
}

/// How long getting `kind` to `level` takes (seconds; 0: at once).
pub fn build_time(kind: Kind, level: u8) -> u32 {
    let level = level.max(1);
    match kind {
        k if k.instant() || k.is_obstacle() => 0,
        Kind::Hq => HQ_UPGRADES[(level.clamp(2, HQ_MAX) - 2) as usize].0,
        _ => TIMES[(kind.time_step() + level as usize - 1).min(TIMES.len() - 1)],
    }
}

/// What a machine makes in an hour, and keeps, at `level`.
pub fn production(level: u8) -> (f64, f64) {
    let i = (level.clamp(1, 12) - 1) as usize;
    (PER_HOUR[i], KEEPS[i])
}

/// What a fridge or a safe holds at `level`.
pub fn storage(level: u8) -> f64 {
    STORAGE[(level.clamp(1, 11) - 1) as usize]
}

/// The experience a work of `secs` gives (as long works give more, but less than their time).
pub fn xp_for(secs: u32) -> u64 {
    (secs as f64).sqrt().round().max(1.0) as u64
}

/// The seniority reached with `xp`: its level (from 1), and the xp of the level, the next level's.
pub fn seniority(xp: u64) -> (u32, u64, u64) {
    let mut level = 1;
    let mut floor = 0;
    loop {
        let need = 30 + 20 * level as u64 * level as u64;
        if xp < floor + need {
            return (level, xp - floor, need);
        }
        floor += need;
        level += 1;
    }
}

/// The seniority's title at `level`.
pub fn seniority_title(level: u32) -> Tx {
    match level {
        0..=4 => ("Junior", "Junior"),
        5..=9 => ("Confirmé", "Mid-level"),
        10..=19 => ("Senior", "Senior"),
        20..=29 => ("Lead", "Lead"),
        30..=44 => ("Principal", "Principal"),
        _ => ("Légende", "Legend"),
    }
}

/// A work under way: the building gets one level at `until`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub until: i64,
    /// Its whole length (the progress bar).
    pub secs: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub id: u32,
    pub kind: Kind,
    /// Its top corner, on the grid.
    pub x: i32,
    pub y: i32,
    /// 0 while being built the first time.
    pub level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
    /// What a machine made and keeps.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stored: f64,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl Building {
    /// Making: a machine built, not being upgraded.
    pub fn producing(&self) -> bool {
        self.kind.makes().is_some() && self.level >= 1 && self.job.is_none()
    }

    /// How far its work is (0 to 1), at `now`.
    pub fn progress(&self, now: i64) -> Option<f64> {
        let job = self.job.as_ref()?;
        let left = (job.until - now).max(0) as f64;
        Some(if job.secs == 0 { 1.0 } else { 1.0 - left / job.secs as f64 })
    }
}

/// Why something can't be done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The boss's office must reach this level first.
    NeedHq(u8),
    /// As many as the boss's office allows.
    MaxCount,
    /// At the highest level the boss's office allows (or ever).
    MaxLevel,
    /// Missing this much.
    Short(Res, u64),
    /// Every intern busy.
    NoIntern,
    /// Already being built.
    Busy,
    /// Not there (out of the grid, or on another building).
    Place,
    /// The recruiting room must reach this level.
    NeedRecruit(u8),
    /// No room left in the break rooms.
    Room,
    /// Not enough tickets.
    Tickets(u32),
}

impl Why {
    pub fn text(self, fr: bool) -> String {
        match self {
            Why::NeedHq(l) => format!("{} {l}", tx(("Bureau du boss niveau", "Boss's office level"), fr)),
            Why::MaxCount => tx(("Maximum atteint pour ce niveau du bureau du boss", "Maximum reached for this boss's office level"), fr).into(),
            Why::MaxLevel => tx(("Niveau maximum pour l'instant", "Highest level for now"), fr).into(),
            Why::Short(r, n) => format!("{} {}", tx(("Il manque", "Missing"), fr), amount_text(n as f64, r, fr)),
            Why::NoIntern => tx(("Tous les alternants sont occupés", "Every intern is busy"), fr).into(),
            Why::Busy => tx(("Déjà en travaux", "Already under work"), fr).into(),
            Why::Place => tx(("Pas de place ici", "No room here"), fr).into(),
            Why::Tickets(n) => format!("{} {n} 🎫", tx(("Il manque", "Missing"), fr)),
            Why::NeedRecruit(l) => format!("{} {l}", tx(("Salle de recrutement niveau", "Recruiting room level"), fr)),
            Why::Room => tx(("Plus de place : construis ou améliore une salle de pause", "No room left: build or upgrade a break room"), fr).into(),
        }
    }
}

/// `n` short: 950, 12,5 k, 1,2 M.
pub fn short(n: f64, fr: bool) -> String {
    let n = n.max(0.0);
    let s = if n < 1_000.0 {
        format!("{:.0}", n.floor())
    } else if n < 1_000_000.0 {
        let k = n / 1_000.0;
        if k < 100.0 { format!("{:.1} k", (k * 10.0).floor() / 10.0) } else { format!("{:.0} k", k.floor()) }
    } else {
        format!("{:.2} M", (n / 10_000.0).floor() / 100.0)
    };
    if fr { s.replace('.', ",") } else { s }
}

pub fn amount_text(n: f64, r: Res, fr: bool) -> String {
    format!("{} {}", short(n, fr), match r {
        Res::Coffee => "☕",
        Res::Budget => "💶",
    })
}

/// A length, short: 45 s, 12 min, 3 h 20, 2 j 4 h.
pub fn duration_text(secs: i64, fr: bool) -> String {
    let s = secs.max(0);
    let (d, h, m) = (s / 86_400, s % 86_400 / 3_600, s % 3_600 / 60);
    if d > 0 {
        if h > 0 { format!("{d} {} {h} h", if fr { "j" } else { "d" }) } else { format!("{d} {}", if fr { "j" } else { "d" }) }
    } else if h > 0 {
        if m > 0 { format!("{h} h {m:02}") } else { format!("{h} h") }
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{s} s")
    }
}

/// What finishing a work of `left` seconds at once costs in tickets.
pub fn finish_cost(left: i64) -> u32 {
    if left <= 0 { 0 } else { 1 + (left / 600) as u32 }
}

/// What happened while running (the time away).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub secs: f64,
    pub coffee: f64,
    pub budget: f64,
    /// The works finished: the building, its new level.
    pub finished: Vec<(Kind, u8)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenSpace {
    /// Unix seconds it was last run to.
    pub at: i64,
    /// The player's actions (the sync keeps the side with the most).
    pub actions: u64,
    pub rng: u64,
    pub next_id: u32,
    pub coffee: f64,
    pub budget: f64,
    pub tickets: u32,
    pub xp: u64,
    pub buildings: Vec<Building>,
    /// The tutorial's step (`TUTORIAL.len()`: done).
    #[serde(default)]
    pub tuto: u8,
    /// The kinds already explained (the card shown the first time one is built).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<Kind>,
    /// How many times something was collected (the tutorial waits for the first).
    #[serde(default)]
    pub collects: u32,
    /// When the next obstacle comes back (unix seconds).
    #[serde(default)]
    pub next_obstacle: i64,
    /// The army: each troop, how many.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub army: Vec<(crate::openspace_battle::Troop, u32)>,
    /// The troops being trained, in order; the first one is done at `queue_until`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue: Vec<crate::openspace_battle::Troop>,
    #[serde(default)]
    pub queue_until: i64,
    /// The troops' levels (1 when not listed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub troop_levels: Vec<(crate::openspace_battle::Troop, u8)>,
    /// The troop being upgraded in the meeting room: until when, for how long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research: Option<(crate::openspace_battle::Troop, i64, u32)>,
    /// The campaign: the stars of each level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub campaign: Vec<u8>,
    #[serde(default)]
    pub raids: u32,
    #[serde(default)]
    pub wins: u32,
    /// The cards: how many copies of each (see `crate::openspace_cards`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<u32>,
    /// The raid deck: the action cards taken on a raid (their numbers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deck: Vec<usize>,
    /// The missions done (the one under way is the next).
    #[serde(default)]
    pub missions: u16,
    /// When the internal mail was last opened (unix seconds; 0: never).
    #[serde(default)]
    pub mail: i64,
    /// The obstacles cleared, the troops trained (the achievements count them).
    #[serde(default)]
    pub cleared: u32,
    #[serde(default)]
    pub trained: u32,
    /// The hero: its level (0: 1), its training (until when, for how long), resting until when.
    #[serde(default)]
    pub hero_level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero_training: Option<(i64, u32)>,
    #[serde(default)]
    pub hero_rest: i64,
}

/// What a tutorial step points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// The « Build » button, then `Kind` in the shop.
    Shop(Kind),
    /// A building of this kind on the open space.
    Building(Kind),
    /// A machine's bubble.
    Bubble,
    /// The « Army » button.
    Army,
    /// The « Attack » button.
    Attack,
    /// The « Cards » button.
    Cards,
    /// Nothing in particular.
    None,
}

/// A tutorial step: its title, its text, what it points at, and what ends it (None: « Next »).
pub struct Step {
    pub title: Tx,
    pub text: Tx,
    pub spot: Spot,
    pub done: Option<fn(&OpenSpace) -> bool>,
}

pub const TUTORIAL: [Step; 14] = [
    Step {
        title: ("Bienvenue dans ton open space !", "Welcome to your open space!"),
        text: ("Pour l'instant, il n'y a que toi et ton bureau du boss, au milieu d'un plateau vide. Ton but : en faire l'open space le plus puissant de la boîte. Tout se construit en temps réel et continue même Ronnie fermé.\n\nGlisse pour déplacer la vue, la molette pour zoomer.", "For now it's just you and your boss's office, in the middle of an empty floor. Your goal: make it the most powerful open space in the company. Everything is built in real time and goes on even with Ronnie closed.\n\nDrag to move the view, the wheel to zoom."),
        spot: Spot::Building(Kind::Hq),
        done: None,
    },
    Step {
        title: ("Il te faut de la main-d'œuvre", "You need hands"),
        text: ("Rien ne se construit sans alternant : chacun mène un chantier à la fois. Le premier bureau des alternants est gratuit et immédiat.\n\nClique sur « Construire », choisis « Bureau des alternants », puis clique sur le sol pour le poser.", "Nothing gets built without an intern: each one runs one work at a time. The first interns' desk is free and instant.\n\nClick « Build », pick « Interns' desk », then click on the floor to place it."),
        spot: Spot::Shop(Kind::Interns),
        done: Some(|s| s.count(Kind::Interns) >= 1),
    },
    Step {
        title: ("Le carburant : le café ☕", "The fuel: coffee ☕"),
        text: ("Le café sert à recruter tes troupes et à construire les salles de l'armée. Une machine à café en fait toute seule, même quand tu n'es pas là.\n\nConstruis une machine à café.", "Coffee recruits your troops and builds the army's rooms. A coffee machine makes it by itself, even while you're away.\n\nBuild a coffee machine."),
        spot: Spot::Shop(Kind::Coffee),
        done: Some(|s| s.count(Kind::Coffee) >= 1),
    },
    Step {
        title: ("Les chantiers prennent du temps", "Works take time"),
        text: ("Tu vois l'échafaudage et la barre au-dessus ? C'est le chantier : celui-ci ne dure que 10 secondes. Plus tard, ce sera des heures, puis des jours : tu lanceras tes chantiers le matin et tu les retrouveras finis le soir.\n\nAttends qu'elle soit finie.", "See the scaffolding and the bar above it? That's the work: this one only takes 10 seconds. Later it will be hours, then days: you'll start your works in the morning and find them done in the evening.\n\nWait for it to be done."),
        spot: Spot::Building(Kind::Coffee),
        done: Some(|s| s.buildings.iter().any(|b| b.kind == Kind::Coffee && b.level >= 1)),
    },
    Step {
        title: ("L'argent : le budget 💶", "The money: budget 💶"),
        text: ("Le budget paie tous les autres bâtiments et leurs améliorations. Il est rapporté par les serveurs de prod.\n\nConstruis un serveur de prod (il se paie en café).", "Budget pays for every other building and their upgrades. Production servers bring it in.\n\nBuild a production server (it is paid in coffee)."),
        spot: Spot::Shop(Kind::Server),
        done: Some(|s| s.count(Kind::Server) >= 1),
    },
    Step {
        title: ("Récolte !", "Collect!"),
        text: ("Tes machines gardent ce qu'elles produisent. Quand une bulle apparaît au-dessus, clique dessus pour récolter. Une machine pleine s'arrête : passe régulièrement !\n\nAstuce : un clic sur ☕ ou 💶 en haut récolte toutes les machines d'un coup.", "Your machines keep what they make. When a bubble shows over one, click it to collect. A full machine stops: drop by often!\n\nTip: a click on ☕ or 💶 at the top collects every machine at once."),
        spot: Spot::Bubble,
        done: Some(|s| s.collects >= 1),
    },
    Step {
        title: ("Fais monter de niveau", "Level up"),
        text: ("Chaque bâtiment s'améliore : une machine de plus haut niveau produit plus et garde plus. Clique sur ta machine à café, puis sur « Niveau 2 ».", "Every building can be upgraded: a higher-level machine makes more and keeps more. Click your coffee machine, then « Level 2 »."),
        spot: Spot::Building(Kind::Coffee),
        done: Some(|s| s.buildings.iter().any(|b| b.kind == Kind::Coffee && (b.level >= 2 || (b.level >= 1 && b.job.is_some())))),
    },
    Step {
        title: ("Défends-toi", "Defend yourself"),
        text: ("Bientôt, des boîtes concurrentes (puis tes collègues) viendront te piquer ton café et ton budget. Le bureau du code review frappe fort ceux qui s'approchent.\n\nConstruis-en un, près de ton QG.", "Soon, rival companies (then your colleagues) will come for your coffee and your budget. The code review desk hits hard whoever comes close.\n\nBuild one, near your HQ."),
        spot: Spot::Shop(Kind::Review),
        done: Some(|s| s.count(Kind::Review) >= 1),
    },
    Step {
        title: ("Stocker plus", "Store more"),
        text: ("Ton QG ne garde que 1 000 de chaque ressource. Pour viser de plus gros chantiers, il te faut un frigo (café) et un coffre-fort (budget). Pense-y dès que tes barres en haut sont pleines.", "Your HQ only holds 1,000 of each resource. To aim for bigger works you need a fridge (coffee) and a safe (budget). Think of it as soon as your bars at the top are full."),
        spot: Spot::Shop(Kind::Fridge),
        done: None,
    },
    Step {
        title: ("Ton armée : la salle de recrutement", "Your army: the recruiting room"),
        text: ("Pour aller piquer le café et le budget des boîtes concurrentes, il te faut une armée. Tout commence par la salle de recrutement : chaque niveau débloque une nouvelle troupe.\n\nConstruis-en une.", "To go and grab the coffee and budget of rival companies, you need an army. It all starts with the recruiting room: each level unlocks a new troop.\n\nBuild one."),
        spot: Spot::Shop(Kind::Recruit),
        done: Some(|s| s.count(Kind::Recruit) >= 1),
    },
    Step {
        title: ("Il faut les loger", "They need a place"),
        text: ("Tes troupes attendent les raids dans la salle de pause : sa taille fixe combien tu peux en emmener.\n\nConstruis une salle de pause.", "Your troops wait for raids in the break room: its size sets how many you can take along.\n\nBuild a break room."),
        spot: Spot::Shop(Kind::Lounge),
        done: Some(|s| s.count(Kind::Lounge) >= 1),
    },
    Step {
        title: ("Recrute !", "Recruit!"),
        text: ("Une fois les deux salles finies, ouvre « Armée » et recrute quelques stagiaires (ils coûtent du café). Chaque troupe prend un peu de temps à former.", "Once both rooms are done, open « Army » and recruit a few interns (they cost coffee). Each troop takes a little time to train."),
        spot: Spot::Army,
        done: Some(|s| !s.army.is_empty() || !s.queue.is_empty()),
    },
    Step {
        title: ("À l'attaque !", "Attack!"),
        text: ("Ouvre « Attaquer » : la campagne t'attend, 50 boîtes concurrentes de plus en plus coriaces. Pose tes troupes autour de leur open space (pas collées aux bâtiments) : elles se débrouillent seules. 1 étoile à 50 %, 1 pour leur QG, 1 à 100 %.", "Open « Attack »: the campaign awaits, 50 rival companies, tougher and tougher. Drop your troops around their open space (not right against the buildings): they go by themselves. 1 star at 50 %, 1 for their HQ, 1 at 100 %."),
        spot: Spot::Attack,
        done: Some(|s| s.raids > 0),
    },
    Step {
        title: ("Le bureau du boss décide de tout", "The boss's office rules it all"),
        text: ("Son niveau fixe combien de bâtiments tu peux avoir et jusqu'à quel niveau : quand tout est au max, c'est lui qu'il faut améliorer (de 5 minutes au niveau 2… à 3 jours au niveau 10). Chaque niveau débloque de nouveaux bâtiments, qui s'expliqueront la première fois que tu les poses.\n\nÀ toi de jouer ! Le « ? » en haut relance ce tutoriel.", "Its level sets how many buildings you can have and up to which level: when everything is maxed out, it's the one to upgrade (from 5 minutes for level 2… to 3 days for level 10). Each level unlocks new buildings, explained the first time you place them.\n\nOver to you! The « ? » at the top replays this tutorial."),
        spot: Spot::Building(Kind::Hq),
        done: None,
    },
];

impl OpenSpace {
    /// The tutorial's step under way, if any.
    pub fn step(&self) -> Option<&'static Step> {
        TUTORIAL.get(self.tuto as usize)
    }

    /// The step under way done: on to the next (each step already done is skipped).
    pub fn tutorial_advance(&mut self) {
        while let Some(step) = self.step() {
            match step.done {
                Some(done) if done(self) => self.tuto += 1,
                _ => break,
            }
        }
    }

    /// « Next » on a step that waits for nothing.
    pub fn tutorial_next(&mut self) {
        if self.step().is_some() {
            self.tuto += 1;
            self.actions += 1;
            self.tutorial_advance();
        }
    }

    pub fn tutorial_skip(&mut self) {
        self.tuto = TUTORIAL.len() as u8;
        self.actions += 1;
    }

    pub fn tutorial_restart(&mut self) {
        self.tuto = 0;
        self.actions += 1;
    }

    /// A kind built for the first time: true once (its card to show).
    pub fn discover(&mut self, kind: Kind) -> bool {
        if self.seen.contains(&kind) {
            return false;
        }
        self.seen.push(kind);
        self.actions += 1;
        true
    }

    /// A new open space: the boss's office alone in the middle, enough to get going.
    pub fn new(now: i64, seed: u64) -> Self {
        let mut s = OpenSpace { at: now, actions: 0, rng: seed | 1, next_id: 1, coffee: 1_000.0, budget: 1_000.0, tickets: START_TICKETS, xp: 0, buildings: Vec::new(), tuto: 0, seen: vec![Kind::Hq], collects: 0, next_obstacle: 0, army: Vec::new(), queue: Vec::new(), queue_until: 0, troop_levels: Vec::new(), research: None, campaign: Vec::new(), raids: 0, wins: 0, cards: Vec::new(), deck: Vec::new(), missions: 0, mail: 0, cleared: 0, trained: 0, hero_level: 1, hero_training: None, hero_rest: 0 };
        let c = GRID / 2;
        let id = s.take_id();
        s.buildings.push(Building { id, kind: Kind::Hq, x: c - 2, y: c - 2, level: 1, job: None, stored: 0.0 });
        // The floor cluttered: obstacles everywhere but around the boss's office.
        for _ in 0..22 {
            s.spawn_obstacle();
        }
        s.next_obstacle = now + OBSTACLE_EVERY;
        s
    }

    /// A pseudo-random number (xorshift), from the open space's own seed.
    pub(crate) fn rand_u64(&mut self) -> u64 {
        self.rand()
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    pub fn obstacles(&self) -> usize {
        self.buildings.iter().filter(|b| b.kind.is_obstacle()).count()
    }

    /// An obstacle put on a free spot (not right around the boss's office): whether it found one.
    fn spawn_obstacle(&mut self) -> bool {
        let n = self.rand() as usize % OBSTACLES.len();
        let kind = OBSTACLES[n].0;
        let c = GRID / 2;
        for _ in 0..40 {
            let x = 1 + (self.rand() % (GRID as u64 - 3)) as i32;
            let y = 1 + (self.rand() % (GRID as u64 - 3)) as i32;
            if (x - c).abs() < 5 && (y - c).abs() < 5 {
                continue;
            }
            if self.fits(kind, x, y, None) {
                let id = self.take_id();
                self.buildings.push(Building { id, kind, x, y, level: 1, job: None, stored: 0.0 });
                return true;
            }
        }
        false
    }

    /// What clearing obstacle `id` costs (budget).
    pub fn clear_cost(kind: Kind) -> u64 {
        OBSTACLES.iter().find(|(k, _)| *k == kind).map_or(0, |(_, c)| *c)
    }

    /// Clears obstacle `id`: the tickets found under it.
    pub fn clear(&mut self, id: u32) -> Result<u32, Why> {
        let i = self.buildings.iter().position(|b| b.id == id && b.kind.is_obstacle()).ok_or(Why::Place)?;
        let price = Self::clear_cost(self.buildings[i].kind);
        if self.budget < price as f64 {
            return Err(Why::Short(Res::Budget, price - self.budget as u64));
        }
        self.budget -= price as f64;
        self.buildings.remove(i);
        self.xp += 6;
        self.cleared += 1;
        // Now and then, tickets under it: 0 most of the time, up to 5.
        let roll = self.rand() % 100;
        let found = match roll {
            0..=54 => 0,
            55..=79 => 1,
            80..=91 => 2,
            92..=97 => 3,
            _ => 5,
        };
        self.tickets += found;
        self.actions += 1;
        Ok(found)
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn get(&self, id: u32) -> Option<&Building> {
        self.buildings.iter().find(|b| b.id == id)
    }

    pub fn hq_level(&self) -> u8 {
        self.buildings.iter().find(|b| b.kind == Kind::Hq).map_or(1, |b| b.level.max(1))
    }

    /// How many of `kind` (being built included).
    pub fn count(&self, kind: Kind) -> usize {
        self.buildings.iter().filter(|b| b.kind == kind).count()
    }

    /// The builders: the interns' desks built.
    pub fn interns(&self) -> usize {
        self.buildings.iter().filter(|b| b.kind == Kind::Interns && b.level >= 1).count()
    }

    /// The works under way.
    pub fn busy(&self) -> usize {
        self.buildings.iter().filter(|b| b.job.is_some()).count()
    }

    /// What can be stored of `r`.
    pub fn capacity(&self, r: Res) -> f64 {
        let hq = hq_storage(self.hq_level());
        hq + self.buildings.iter().filter(|b| b.kind.stores() == Some(r) && b.level >= 1).map(|b| storage(b.level)).sum::<f64>()
    }

    pub fn amount(&self, r: Res) -> f64 {
        match r {
            Res::Coffee => self.coffee,
            Res::Budget => self.budget,
        }
    }

    fn amount_mut(&mut self, r: Res) -> &mut f64 {
        match r {
            Res::Coffee => &mut self.coffee,
            Res::Budget => &mut self.budget,
        }
    }

    /// Can `kind` go with its top corner at (x, y) (`moving`: that building, ignored)?
    pub fn fits(&self, kind: Kind, x: i32, y: i32, moving: Option<u32>) -> bool {
        let s = kind.size();
        if x < 1 || y < 1 || x + s > GRID - 1 || y + s > GRID - 1 {
            return false;
        }
        !self.buildings.iter().any(|b| Some(b.id) != moving && b.x < x + s && x < b.x + b.kind.size() && b.y < y + s && y < b.y + b.kind.size())
    }

    /// Why a new `kind` can't be built now (the place aside).
    pub fn can_build(&self, kind: Kind) -> Result<(), Why> {
        let hq = self.hq_level();
        let (count, _) = kind.allowed(hq);
        if count == 0 {
            return Err(Why::NeedHq(kind.unlocked_at()));
        }
        if self.count(kind) as u32 >= count {
            let next = (hq + 1..=HQ_MAX).find(|&h| kind.allowed(h).0 > count);
            return Err(next.map_or(Why::MaxCount, Why::NeedHq));
        }
        let (r, price) = cost(kind, 1, self.count(kind));
        if self.amount(r) < price as f64 {
            return Err(Why::Short(r, price - self.amount(r) as u64));
        }
        if build_time(kind, 1) > 0 && self.busy() >= self.interns() {
            return Err(Why::NoIntern);
        }
        Ok(())
    }

    /// Builds a new `kind` at (x, y): its id.
    pub fn build(&mut self, kind: Kind, x: i32, y: i32, now: i64) -> Result<u32, Why> {
        self.can_build(kind)?;
        if !self.fits(kind, x, y, None) {
            return Err(Why::Place);
        }
        let (r, price) = cost(kind, 1, self.count(kind));
        *self.amount_mut(r) -= price as f64;
        let id = self.take_id();
        let secs = build_time(kind, 1);
        let (level, job) = if secs == 0 { (1, None) } else { (0, Some(Job { until: now + secs as i64, secs })) };
        if secs == 0 {
            self.xp += 1;
        }
        self.buildings.push(Building { id, kind, x, y, level, job, stored: 0.0 });
        self.actions += 1;
        Ok(id)
    }

    /// Why building `id` can't be upgraded now.
    pub fn can_upgrade(&self, id: u32) -> Result<(), Why> {
        let b = self.get(id).ok_or(Why::Place)?;
        if b.job.is_some() {
            return Err(Why::Busy);
        }
        let hq = self.hq_level();
        if b.level >= b.kind.max_level() {
            return Err(Why::MaxLevel);
        }
        if b.level >= b.kind.allowed(hq).1 {
            let next = (hq + 1..=HQ_MAX).find(|&h| b.kind.allowed(h).1 > b.level);
            return Err(next.map_or(Why::MaxLevel, Why::NeedHq));
        }
        let (r, price) = cost(b.kind, b.level + 1, 0);
        if self.amount(r) < price as f64 {
            return Err(Why::Short(r, price - self.amount(r) as u64));
        }
        if build_time(b.kind, b.level + 1) > 0 && self.busy() >= self.interns() {
            return Err(Why::NoIntern);
        }
        Ok(())
    }

    /// Upgrades building `id` one level (at once for a firewall).
    pub fn upgrade(&mut self, id: u32, now: i64) -> Result<(), Why> {
        self.can_upgrade(id)?;
        // A machine's last coffee or budget is collected first (it stops during the work).
        self.collect(id);
        let i = self.buildings.iter().position(|b| b.id == id).ok_or(Why::Place)?;
        let (kind, level) = (self.buildings[i].kind, self.buildings[i].level);
        let (r, price) = cost(kind, level + 1, 0);
        *self.amount_mut(r) -= price as f64;
        let secs = build_time(kind, level + 1);
        if secs == 0 {
            self.buildings[i].level += 1;
            self.xp += 1;
        } else {
            self.buildings[i].job = Some(Job { until: now + secs as i64, secs });
        }
        self.actions += 1;
        Ok(())
    }

    /// Stops the work on `id`: half its price back; a building never finished goes.
    pub fn cancel(&mut self, id: u32) -> bool {
        let Some(i) = self.buildings.iter().position(|b| b.id == id && b.job.is_some()) else { return false };
        let b = &self.buildings[i];
        if b.kind == Kind::Hq && b.level == 0 {
            return false;
        }
        let nth = self.count(b.kind).saturating_sub(1);
        let (r, price) = cost(b.kind, b.level + 1, nth);
        let back = price as f64 / 2.0;
        if b.level == 0 {
            self.buildings.remove(i);
        } else {
            self.buildings[i].job = None;
        }
        let cap = self.capacity(r);
        let have = self.amount_mut(r);
        *have = (*have + back).min(cap.max(*have));
        self.actions += 1;
        true
    }

    /// Finishes the work on `id` at once, with tickets.
    pub fn finish_now(&mut self, id: u32, now: i64) -> Result<(), Why> {
        let b = self.get(id).ok_or(Why::Place)?;
        let Some(job) = &b.job else { return Err(Why::Place) };
        let price = finish_cost(job.until - now);
        if self.tickets < price {
            return Err(Why::Tickets(price - self.tickets));
        }
        self.tickets -= price;
        let i = self.buildings.iter().position(|b| b.id == id).ok_or(Why::Place)?;
        self.complete(i);
        self.actions += 1;
        Ok(())
    }

    /// Moves `id` to (x, y).
    pub fn move_to(&mut self, id: u32, x: i32, y: i32) -> bool {
        let Some(kind) = self.get(id).map(|b| b.kind) else { return false };
        if kind.is_obstacle() {
            return false;
        }
        if !self.fits(kind, x, y, Some(id)) {
            return false;
        }
        if let Some(b) = self.buildings.iter_mut().find(|b| b.id == id) {
            if (b.x, b.y) == (x, y) {
                return false;
            }
            b.x = x;
            b.y = y;
        }
        self.actions += 1;
        true
    }

    /// Collects what machine `id` made, as much as there is room for: how much.
    pub fn collect(&mut self, id: u32) -> f64 {
        let Some(i) = self.buildings.iter().position(|b| b.id == id) else { return 0.0 };
        let Some(r) = self.buildings[i].kind.makes() else { return 0.0 };
        let room = (self.capacity(r) - self.amount(r)).max(0.0);
        let took = self.buildings[i].stored.min(room).floor();
        if took < 1.0 {
            return 0.0;
        }
        self.buildings[i].stored -= took;
        *self.amount_mut(r) += took;
        self.actions += 1;
        self.collects += 1;
        took
    }

    /// Collects every machine making `r`: how much.
    pub fn collect_all(&mut self, r: Res) -> f64 {
        let ids: Vec<u32> = self.buildings.iter().filter(|b| b.kind.makes() == Some(r)).map(|b| b.id).collect();
        ids.into_iter().map(|id| self.collect(id)).sum()
    }

    /// A work done: one level more.
    fn complete(&mut self, i: usize) {
        let b = &mut self.buildings[i];
        if let Some(job) = b.job.take() {
            b.level += 1;
            self.xp += xp_for(job.secs);
        }
    }

    /// The machines making for `dt` seconds.
    fn produce(&mut self, dt: f64, sum: &mut Summary) {
        if dt <= 0.0 {
            return;
        }
        for b in self.buildings.iter_mut().filter(|b| b.producing()) {
            let (per_hour, keeps) = production(b.level);
            let made = (per_hour * dt / 3_600.0).min((keeps - b.stored).max(0.0));
            b.stored += made;
            match b.kind.makes() {
                Some(Res::Coffee) => sum.coffee += made,
                Some(Res::Budget) => sum.budget += made,
                None => {}
            }
        }
    }

    /// Run to `now`: the works finished in their order, the machines making in between.
    pub fn run(&mut self, now: i64) -> Summary {
        let mut sum = Summary::default();
        if now <= self.at {
            return sum;
        }
        sum.secs = (now - self.at) as f64;
        loop {
            let next = self.buildings.iter().enumerate().filter_map(|(i, b)| b.job.as_ref().map(|j| (j.until, i))).filter(|&(until, _)| until <= now).min();
            let Some((until, i)) = next else { break };
            let until = until.max(self.at);
            self.produce((until - self.at) as f64, &mut sum);
            self.at = until;
            self.complete(i);
            let b = &self.buildings[i];
            sum.finished.push((b.kind, b.level));
        }
        self.produce((now - self.at) as f64, &mut sum);
        self.run_army(now);
        while self.next_obstacle > 0 && self.next_obstacle <= now {
            self.next_obstacle += OBSTACLE_EVERY;
            if self.obstacles() < OBSTACLES_MAX {
                self.spawn_obstacle();
            }
        }
        self.at = now;
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_000_000;

    /// An open space a little further on: two interns, a coffee machine.
    fn space() -> OpenSpace {
        let mut s = OpenSpace::new(T0, 42);
        s.buildings.retain(|b| !b.kind.is_obstacle());
        s.budget += 1_000.0;
        s.build(Kind::Interns, 14, 19, T0).unwrap();
        s.build(Kind::Interns, 24, 19, T0).unwrap();
        let id = s.next_id;
        s.next_id += 1;
        s.buildings.push(Building { id, kind: Kind::Coffee, x: 18, y: 24, level: 1, job: None, stored: 0.0 });
        s.budget = 500.0;
        s.coffee = 500.0;
        s
    }

    #[test]
    fn starts_with_the_boss_alone() {
        let s = OpenSpace::new(T0, 42);
        assert_eq!(s.buildings.iter().filter(|b| !b.kind.is_obstacle()).count(), 1);
        assert!(s.obstacles() >= 15, "a cluttered floor");
        assert_eq!(s.hq_level(), 1);
        assert_eq!(s.interns(), 0);
        assert_eq!(s.tickets, START_TICKETS);
        assert!(s.fits(Kind::Hq, s.buildings[0].x, s.buildings[0].y, Some(s.buildings[0].id)));
        // The first interns' desk: free, at once, with no intern.
        assert_eq!(s.can_build(Kind::Interns), Ok(()));
        assert_eq!(s.can_build(Kind::Coffee), Err(Why::NoIntern));
    }

    #[test]
    fn the_tutorial_can_be_played_through() {
        let mut s = OpenSpace::new(T0, 42);
        s.buildings.retain(|b| !b.kind.is_obstacle());
        assert_eq!(s.tuto, 0);
        s.tutorial_next();
        assert_eq!(s.tuto, 1);
        s.build(Kind::Interns, 14, 19, T0).unwrap();
        s.tutorial_advance();
        let coffee = s.build(Kind::Coffee, 18, 24, T0).unwrap();
        s.tutorial_advance();
        assert_eq!(s.tuto, 3, "waits for the coffee machine to be built");
        s.run(T0 + 10);
        s.tutorial_advance();
        let server = s.build(Kind::Server, 24, 13, T0 + 10).unwrap();
        s.tutorial_advance();
        assert_eq!(s.tuto, 5);
        s.run(T0 + 3_600);
        assert!(s.collect(coffee) >= 1.0 || s.collect(server) >= 1.0);
        s.tutorial_advance();
        assert_eq!(s.tuto, 6);
        s.upgrade(coffee, T0 + 3_600).unwrap();
        s.tutorial_advance();
        assert_eq!(s.tuto, 7);
        s.budget = 1_000.0;
        s.run(T0 + 3_700);
        s.build(Kind::Review, 10, 10, T0 + 3_700).unwrap();
        s.tutorial_advance();
        assert_eq!(s.tuto, 8);
        s.tutorial_skip();
        assert!(s.step().is_none());
    }


    #[test]
    fn obstacles_are_cleared_and_come_back() {
        let mut s = OpenSpace::new(T0, 42);
        let n = s.obstacles();
        let id = s.buildings.iter().find(|b| b.kind == Kind::DeadPlant || b.kind == Kind::Boxes).unwrap().id;
        let xp = s.xp;
        s.clear(id).unwrap();
        assert_eq!(s.obstacles(), n - 1);
        assert!(s.xp > xp);
        assert!(!s.move_to(s.buildings.iter().find(|b| b.kind.is_obstacle()).unwrap().id, 2, 2));
        s.run(T0 + 3 * 86_400);
        assert!(s.obstacles() > n - 1, "new ones came back");
        assert!(s.obstacles() <= 30);
    }

    #[test]
    fn traps_and_decorations_are_instant() {
        let mut s = space();
        s.budget = 1e6;
        let plant = s.build(Kind::Plant, 3, 3, T0).unwrap();
        assert_eq!(s.get(plant).unwrap().level, 1);
        assert_eq!(s.can_upgrade(plant), Err(Why::MaxLevel));
        assert_eq!(Kind::Bug.unlocked_at(), 3);
        assert!(Kind::Board.unlocked_at() == 9 && Kind::Cluster.unlocked_at() == 8);
    }
    #[test]
    fn each_kind_is_explained_once() {
        let mut s = OpenSpace::new(T0, 42);
        s.buildings.retain(|b| !b.kind.is_obstacle());
        assert!(!s.discover(Kind::Hq));
        assert!(s.discover(Kind::Coffee));
        assert!(!s.discover(Kind::Coffee));
    }
    #[test]
    fn a_building_goes_up_in_real_time() {
        let mut s = space();
        let budget = s.budget;
        let id = s.build(Kind::Review, 5, 5, T0).unwrap();
        assert_eq!(s.budget, budget - cost(Kind::Review, 1, 0).1 as f64);
        assert_eq!(s.get(id).unwrap().level, 0);
        let secs = build_time(Kind::Review, 1) as i64;
        s.run(T0 + secs - 1);
        assert_eq!(s.get(id).unwrap().level, 0);
        let sum = s.run(T0 + secs);
        assert_eq!(s.get(id).unwrap().level, 1);
        assert_eq!(sum.finished, vec![(Kind::Review, 1)]);
        assert!(s.xp > 0);
    }

    #[test]
    fn no_two_buildings_on_the_same_tiles() {
        let mut s = space();
        let hq = s.buildings[0].clone();
        assert_eq!(s.build(Kind::Review, hq.x + 1, hq.y + 1, T0), Err(Why::Place));
        assert_eq!(s.build(Kind::Review, 0, 5, T0), Err(Why::Place));
        assert_eq!(s.build(Kind::Review, GRID - 3, 5, T0), Err(Why::Place));
        assert!(s.build(Kind::Review, GRID - 4, 5, T0).is_ok());
    }

    #[test]
    fn as_many_works_as_interns() {
        let mut s = space();
        s.budget = 1e6;
        s.coffee = 1e6;
        s.build(Kind::Review, 2, 2, T0).unwrap();
        s.build(Kind::Review, 2, 6, T0).unwrap();
        assert_eq!(s.can_build(Kind::Server), Err(Why::NoIntern));
        // A firewall needs no intern… once the boss's office allows them.
        assert_eq!(s.can_build(Kind::Wall), Err(Why::NeedHq(2)));
    }

    #[test]
    fn the_boss_office_caps_counts_and_levels() {
        let mut s = space();
        s.budget = 1e6;
        s.coffee = 1e6;
        let coffee = s.buildings.iter().find(|b| b.kind == Kind::Coffee).unwrap().id;
        assert_eq!(s.can_build(Kind::Coffee), Err(Why::NeedHq(2)));
        s.upgrade(coffee, T0).unwrap();
        s.run(T0 + 3_600);
        assert_eq!(s.get(coffee).unwrap().level, 2);
        assert_eq!(s.can_upgrade(coffee), Err(Why::NeedHq(2)));
    }

    #[test]
    fn machines_make_until_full_and_collecting_stops_at_the_storage() {
        let mut s = space();
        let coffee = s.buildings.iter().find(|b| b.kind == Kind::Coffee).unwrap().id;
        s.run(T0 + 1_800);
        let made = s.get(coffee).unwrap().stored;
        assert!((made - 100.0).abs() < 1e-6, "{made}");
        // A day: full at what it keeps.
        s.run(T0 + 86_400);
        assert_eq!(s.get(coffee).unwrap().stored, production(1).1);
        let cap = s.capacity(Res::Coffee);
        let took = s.collect(coffee);
        assert_eq!(s.coffee, cap);
        assert!(took > 0.0 && s.get(coffee).unwrap().stored > 0.0, "the rest kept in the machine");
    }

    #[test]
    fn a_machine_upgraded_stops_making_until_done() {
        let mut s = space();
        let coffee = s.buildings.iter().find(|b| b.kind == Kind::Coffee).unwrap().id;
        s.upgrade(coffee, T0).unwrap();
        let secs = build_time(Kind::Coffee, 2) as i64;
        s.run(T0 + secs);
        assert_eq!(s.get(coffee).unwrap().stored, 0.0);
        s.run(T0 + secs + 3_600);
        assert!((s.get(coffee).unwrap().stored - production(2).0).abs() < 1e-6);
    }

    #[test]
    fn works_finish_in_order_while_away() {
        let mut s = space();
        s.budget = 1e6;
        s.coffee = 1e6;
        let a = s.build(Kind::Review, 2, 2, T0).unwrap();
        let hq = s.buildings[0].id;
        s.upgrade(hq, T0).unwrap();
        let sum = s.run(T0 + 7 * 86_400);
        assert_eq!(sum.finished.len(), 2);
        assert_eq!(s.get(a).unwrap().level, 1);
        assert_eq!(s.hq_level(), 2);
        assert_eq!(s.busy(), 0);
    }

    #[test]
    fn cancelling_gives_half_back_and_a_new_building_goes() {
        let mut s = space();
        let before = s.budget;
        let id = s.build(Kind::Review, 2, 2, T0).unwrap();
        assert!(s.cancel(id));
        assert!(s.get(id).is_none());
        assert_eq!(s.budget, before - cost(Kind::Review, 1, 0).1 as f64 / 2.0);
    }

    #[test]
    fn tickets_finish_a_work() {
        let mut s = space();
        let hq = s.buildings[0].id;
        s.budget = 1e6;
        s.upgrade(hq, T0).unwrap();
        let price = finish_cost(build_time(Kind::Hq, 2) as i64);
        s.finish_now(hq, T0).unwrap();
        assert_eq!(s.hq_level(), 2);
        assert_eq!(s.tickets, START_TICKETS - price);
    }

    #[test]
    fn moving_needs_room() {
        let mut s = space();
        let coffee = s.buildings.iter().find(|b| b.kind == Kind::Coffee).unwrap().clone();
        let hq = s.buildings[0].clone();
        assert!(!s.move_to(coffee.id, hq.x, hq.y));
        assert!(s.move_to(coffee.id, 3, 3));
        assert_eq!((s.get(coffee.id).unwrap().x, s.get(coffee.id).unwrap().y), (3, 3));
    }

    #[test]
    fn every_kind_is_reachable_and_its_prices_grow() {
        for kind in KINDS {
            let at = kind.unlocked_at();
            assert!(kind.allowed(at).1 >= 1, "{kind:?}");
            let mut last = 0;
            for level in 2..=kind.max_level() {
                let (_, price) = cost(kind, level, 0);
                assert!(price >= last, "{kind:?} {level}");
                last = price;
                assert!(build_time(kind, level) <= 259_200);
            }
            // What it costs fits what can be stored at the boss's office level that allows it.
            for hq in 1..=HQ_MAX {
                let (count, top) = kind.allowed(hq);
                if count > 0 && top >= 1 && kind != Kind::Hq {
                    let (r, price) = cost(kind, top, 0);
                    let fridges = if r == Res::Coffee { Kind::Fridge } else { Kind::Safe };
                    let (n, l) = fridges.allowed(hq);
                    let cap = hq_storage(hq) + n as f64 * storage(l);
                    assert!(price as f64 <= cap, "{kind:?} level {top} at hq {hq}: {price} > {cap}");
                }
            }
        }
    }

    #[test]
    fn the_boss_office_upgrade_fits_the_storage() {
        for hq in 1..HQ_MAX {
            let (_, price) = cost(Kind::Hq, hq + 1, 0);
            let (n, l) = Kind::Safe.allowed(hq);
            assert!(price as f64 <= hq_storage(hq) + n as f64 * storage(l), "hq {hq}");
        }
    }

    #[test]
    fn seniority_climbs() {
        assert_eq!(seniority(0).0, 1);
        let (l, _, _) = seniority(10_000);
        assert!(l > 5);
    }

    #[test]
    fn survives_a_round_trip() {
        let mut s = space();
        s.build(Kind::Review, 2, 2, T0).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        let back: OpenSpace = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert!(json.len() < 2_000);
    }
}
