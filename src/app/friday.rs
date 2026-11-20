//! Mise en prod du vendredi, in the games (its rules in `crate::friday`): the server room drawn from a
//! little above, the weekend's clock on the wall, the tools under it (1 to 5, or a click), Space to
//! pause. The first time, its tutorial (the first on-call); out of a game, its welcome screen: how to
//! play, the level, the last weekend, the world's board on floor.
//!
//! Its words live here, in French and English (only its name and its arcade line are in i18n).

use super::four::HomeGame;
use super::*;
use crate::friday::{self as world, Card, Game, Level, News, Note, Role, Tip, Tool};

/// The world moves in steps this long, whatever the frame rate.
const STEP: f32 = 1.0 / 60.0;
/// The speeds, picked in turn.
const SPEEDS: [f32; 3] = [1.0, 2.0, 4.0];

pub(super) struct Friday {
    game: Option<Game>,
    level: Level,
    tool: Tool,
    speed: usize,
    paused: bool,
    acc: f32,
    rng: u64,
    /// The weekend just over: counted (the best kept, sent to floor), and whether it beat the best.
    counted: bool,
    best: bool,
    pub(super) floor: crate::floor::GameFloor,
}

impl Default for Friday {
    fn default() -> Self {
        Self { game: None, level: Level::Scaleup, tool: Tool::Hotfix, speed: 0, paused: false, acc: 0.0, rng: 0, counted: false, best: false, floor: crate::floor::GameFloor::new("mise-en-prod", false) }
    }
}

impl Friday {
    /// A real weekend under way (not the tutorial, not over).
    fn playing(&self) -> bool {
        self.game.as_ref().is_some_and(|g| g.over.is_none() && g.tutorial.is_none())
    }

    /// The home page is left: a weekend or a tutorial under way stops.
    pub(super) fn leave(&mut self) {
        if self.game.as_ref().is_some_and(|g| g.over.is_none()) {
            self.game = None;
            self.paused = false;
        }
    }
}

/// A word in the player's language.
fn tx(fr: bool, words: (&'static str, &'static str)) -> &'static str {
    if fr { words.0 } else { words.1 }
}

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn rgba(c: u32, a: f32) -> Color32 {
    let k = rgb(c);
    Color32::from_rgba_unmultiplied(k.r(), k.g(), k.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

const INK: u32 = 0xe6ebf3;
const MUTED: u32 = 0x93a0b5;
const AMBER: u32 = 0xffb547;
const LED: u32 = 0x4be08a;
const ALARM: u32 = 0xff5a5f;
const ICE: u32 = 0x9fe8ff;
const PAPER: u32 = 0xf2efe6;
/// The team's shirts.
const SHIRTS: [u32; 5] = [0xff9e6d, 0x7cc7ff, 0xc4a1ff, 0x8be07a, 0xd9c39a];

/// The world drawn into a rectangle of the page.
#[derive(Clone, Copy)]
struct View {
    origin: Pos2,
    k: f32,
}

impl View {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.k
    }
    fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.p(x, y), Vec2::new(w, h) * self.k)
    }
    fn font(&self, size: f32) -> FontId {
        FontId::proportional(size * self.k)
    }
    fn mono(&self, size: f32) -> FontId {
        FontId::monospace(size * self.k)
    }
    /// A point of the page, in the world.
    fn world(&self, at: Pos2) -> (f32, f32) {
        let v = (at - self.origin) / self.k;
        (v.x, v.y)
    }
}

/// A rectangle filled from `top` to `bottom`.
fn gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// The sky at an hour: night, dawn, day, sunset.
fn sky(hour: f32) -> (Color32, Color32) {
    let keys: [(f32, u32, u32); 9] = [(0.0, 0x05070f, 0x0b1230), (5.5, 0x0a1030, 0x1d2350), (7.0, 0xff9a6b, 0x5d79c8), (9.0, 0x7fb3ff, 0xcfe3ff), (17.0, 0x7fb3ff, 0xcfe3ff), (18.5, 0xff8a5c, 0x6a4c9c), (20.5, 0x141a40, 0x2a2a5e), (22.0, 0x05070f, 0x0b1230), (24.0, 0x05070f, 0x0b1230)];
    let i = keys.iter().rposition(|k| k.0 <= hour).unwrap_or(0).min(keys.len() - 2);
    let (a, b) = (keys[i], keys[i + 1]);
    let t = ((hour - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    (rgb(a.1).lerp_to_gamma(rgb(b.1), t), rgb(a.2).lerp_to_gamma(rgb(b.2), t))
}

// ---- The words ----------------------------------------------------------------------------------

fn level_name(fr: bool, level: Level) -> &'static str {
    match level {
        Level::Startup => tx(fr, ("Petite start-up", "Small start-up")),
        Level::Scaleup => tx(fr, ("Scale-up", "Scale-up")),
        Level::BlackFriday => tx(fr, ("Black Friday", "Black Friday")),
    }
}

fn level_note(fr: bool, level: Level) -> &'static str {
    match level {
        Level::Startup => tx(fr, ("3 serveurs, pour se faire la main", "3 servers, to get the hang of it")),
        Level::Scaleup => tx(fr, ("5 serveurs, la panne de l’hébergeur", "5 servers, the host’s outage")),
        Level::BlackFriday => tx(fr, ("Du trafic toute la nuit · score ×2,2", "Traffic all night · score ×2.2")),
    }
}

fn role_name(fr: bool, role: Role) -> &'static str {
    match role {
        Role::Lb => "Load balancer",
        Role::Api => "API",
        Role::Db => tx(fr, ("Base", "Database")),
        Role::Front => "Front",
        Role::Cache => "Cache",
    }
}

fn tool_name(fr: bool, tool: Tool) -> &'static str {
    match tool {
        Tool::Hotfix => "Hotfix",
        Tool::Rollback => "Rollback",
        Tool::Cache => "Cache",
        Tool::Oncall => tx(fr, ("Astreinte", "On call")),
        Tool::Coffee => tx(fr, ("Café", "Coffee")),
    }
}

fn tool_hint(fr: bool, tool: Tool) -> &'static str {
    match tool {
        Tool::Hotfix => tx(fr, ("Clique sur un serveur : éteint un incident (10 % de chances d’en créer un autre).", "Click a server: puts out one incident (a 10 % chance of making another).")),
        Tool::Rollback => tx(fr, ("Clique sur un serveur : nettoie tout d’un coup et le relève s’il est tombé.", "Click a server: cleans it all at once, and raises it if it fell.")),
        Tool::Cache => tx(fr, ("Clique sur un serveur : il absorbe les vagues de trafic et chasse l’escargot.", "Click a server: it soaks up the traffic waves and chases the snail off.")),
        Tool::Oncall => tx(fr, ("Clique sur un serveur : le collègue le plus en forme y va.", "Click a server: the freshest colleague goes there.")),
        Tool::Coffee => tx(fr, ("Clique n’importe où : toute l’équipe reprend des forces.", "Click anywhere: the whole team gets its strength back.")),
    }
}

/// A card: its name, what it does, what it gives, what it costs.
fn card_words(fr: bool, card: Card) -> (&'static str, &'static str, &'static str, &'static str) {
    match card {
        Card::Rust => (tx(fr, ("Tom réécrit tout en Rust", "Tom rewrites it all in Rust")), tx(fr, ("Le serveur le plus mal en point devient increvable.", "The worst-off server becomes unbreakable.")), tx(fr, ("ne tombe plus", "never falls again")), tx(fr, ("Tom absent 4 h", "Tom away 4 h"))),
        Card::Flag => (tx(fr, ("Feature flag", "Feature flag")), tx(fr, ("On coupe la fonctionnalité qui plante.", "Switch off the feature that crashes.")), tx(fr, ("un serveur à l’abri 2 h", "a server sheltered 2 h")), tx(fr, ("satisfaction −5 %", "satisfaction −5 %"))),
        Card::Intern => (tx(fr, ("Le stagiaire", "The intern")), tx(fr, ("Il est motivé, et il est gratuit.", "Keen, and free.")), tx(fr, ("un collègue de plus", "one more on the team")), tx(fr, ("crée parfois des bugs", "sometimes makes bugs"))),
        Card::Pizza => (tx(fr, ("Pizza pour l’équipe", "Pizza for the team")), tx(fr, ("Tout le monde reprend des forces.", "Everyone gets their strength back.")), tx(fr, ("énergie pleine", "full energy")), tx(fr, ("budget −20", "budget −20"))),
        Card::Monitoring => (tx(fr, ("Monitoring", "Monitoring")), tx(fr, ("On voit enfin venir les ennuis.", "At last you see trouble coming.")), tx(fr, ("alertes, fuites plus lentes", "alerts, slower leaks")), ""),
        Card::Senior => (tx(fr, ("Le vieux senior", "The old senior")), tx(fr, ("Michel répare tout, quand il est réveillé.", "Michel fixes anything, when awake.")), tx(fr, ("répare ×3", "fixes ×3")), tx(fr, ("dort de 23 h à 7 h", "asleep 11 pm to 7 am"))),
        Card::Funding => (tx(fr, ("Levée de fonds", "Funding round")), tx(fr, ("Les investisseurs y croient.", "The investors believe in it.")), tx(fr, ("budget +80", "budget +80")), ""),
        Card::Firewall => (tx(fr, ("Pare-feu", "Firewall")), tx(fr, ("Les bugs passent moins bien.", "Bugs get through less.")), tx(fr, ("bugs plus lents et plus rares", "slower, fewer bugs")), ""),
        Card::Autoscale => (tx(fr, ("Autoscaling", "Autoscaling")), tx(fr, ("Le load balancer grandit avec le trafic.", "The load balancer grows with the traffic.")), tx(fr, ("vagues ÷2, se soigne seul", "waves halved, heals itself")), ""),
        Card::Docs => (tx(fr, ("Documentation à jour", "Up-to-date docs")), tx(fr, ("Pour une fois, quelqu’un l’a écrite.", "For once, somebody wrote it.")), tx(fr, ("les hotfix ne ratent plus", "hotfixes never backfire")), ""),
        Card::Espresso => (tx(fr, ("Machine à grains", "Bean-to-cup machine")), tx(fr, ("Fini le café soluble.", "No more instant coffee.")), tx(fr, ("café à 8, +60 % d’énergie", "coffee at 8, +60 % energy")), ""),
        Card::Nap => (tx(fr, ("Salle de sieste", "Nap room")), tx(fr, ("Un canapé de plus, au calme.", "One more couch, somewhere quiet.")), tx(fr, ("fatigue moins, dort mieux", "tires less, sleeps better")), ""),
    }
}

fn rarity_words(fr: bool, rarity: u8) -> (&'static str, u32) {
    match rarity {
        2 => (tx(fr, ("ÉPIQUE", "EPIC")), 0xb06cff),
        1 => (tx(fr, ("RARE", "RARE")), 0x5ab8ff),
        _ => (tx(fr, ("COMMUNE", "COMMON")), MUTED),
    }
}

/// A tip: its title and what to do.
fn tip_words(fr: bool, tip: Tip) -> (&'static str, &'static str) {
    match tip {
        Tip::Peak => (tx(fr, ("Le pic du soir", "The evening peak")), tx(fr, ("Une vague de paquets arrive d’Internet sur le load balancer. Chaque paquet de la vague l’abîme. Un Cache posé dessus les absorbe.", "A wave of packets comes from Internet onto the load balancer. Each one of the wave hurts it. A Cache put on it soaks them up."))),
        Tip::Phone => (tx(fr, ("Le client appelle", "The client is calling")), tx(fr, ("Le téléphone sonne sur le bureau. Un collègue libre va décrocher tout seul, mais pendant ce temps il ne répare rien. Sans réponse, la satisfaction baisse vite.", "The phone rings on the desk. A free colleague goes to answer it, but meanwhile fixes nothing. Unanswered, the satisfaction drops fast."))),
        Tip::Boss => (tx(fr, ("Le DSI passe voir", "The CIO drops by")), tx(fr, ("Tant qu’il traverse la salle, les incidents grossissent deux fois plus vite. Il n’y a pas de parade : tiens bon jusqu’à ce qu’il reparte.", "While he walks across the room, the incidents grow twice as fast. There is nothing to do about it: hold on until he leaves."))),
        Tip::Snail => (tx(fr, ("Une requête lente", "A slow query")), tx(fr, ("Un escargot s’est posé sur le câble de la base : les paquets font la queue et la base s’abîme. Un Cache sur la base le chasse, sinon il finit par partir tout seul.", "A snail sat on the database’s cable: the packets queue and the database gets hurt. A Cache on the database chases it off, else it leaves in the end."))),
        Tip::Host => (tx(fr, ("La panne de l’hébergeur", "The host’s outage")), tx(fr, ("Tout s’éteint quelques secondes et chaque serveur encaisse un coup. Regarde lesquels sont fragiles et soigne-les vite.", "Everything goes dark a few seconds and each server takes a blow. See which ones are weak and tend to them fast."))),
        Tip::Down => (tx(fr, ("Un serveur est tombé", "A server fell")), tx(fr, ("Tant qu’il est par terre, la satisfaction chute. Un Rollback le relève tout de suite ; sinon un collègue finit par le relancer, plus lentement.", "While it’s down, the satisfaction falls. A Rollback raises it at once; otherwise a colleague restarts it in the end, more slowly."))),
        Tip::Npm => (tx(fr, ("Une dépendance npm a disparu", "An npm dependency vanished")), tx(fr, ("Une rafale de bugs court sur tous les câbles. Envoie l’équipe en Astreinte là où ils arrivent, et garde des Hotfix pour les serveurs fragiles.", "A swarm of bugs runs along every cable. Send the team On call where they land, and keep Hotfixes for the weak servers."))),
        Tip::Cards => (tx(fr, ("Une carte entre deux vagues", "A card between two waves")), tx(fr, ("Le week-end fait une pause : choisis une carte parmi trois. En vert ce qu’elle apporte, en rouge ce qu’elle coûte. Les épiques sont rares.", "The weekend pauses: pick a card out of three. In green what it brings, in red what it costs. The epic ones are rare."))),
        Tip::Sleep => (tx(fr, ("Quelqu’un s’est endormi", "Someone fell asleep")), tx(fr, ("À force de réparer, l’équipe se fatigue (la barre sous chacun), surtout la nuit. Endormi, on ne répare plus : un Café réveille tout le monde.", "Fixing tires the team (the bar under each one), at night above all. Asleep, they fix nothing: a Coffee wakes everyone."))),
    }
}

fn news_text(fr: bool, news: News, g: &Game) -> String {
    match news {
        News::Peak => tx(fr, ("20:00 · Le pic du soir arrive sur le load balancer", "8 pm · The evening peak hits the load balancer")).to_owned(),
        News::Phone => tx(fr, ("Le client appelle…", "The client is calling…")).to_owned(),
        News::Boss => tx(fr, ("Le DSI « passe voir »", "The CIO “drops by”")).to_owned(),
        News::Host => tx(fr, ("21:00 · Panne de l’hébergeur !", "9 pm · The host is down!")).to_owned(),
        News::Npm => tx(fr, ("03:00 · Une dépendance npm a disparu", "3 am · An npm dependency vanished")).to_owned(),
        News::Snail => tx(fr, ("Une requête lente sur la base", "A slow query on the database")).to_owned(),
        News::Down(i) => format!("{} {}", g.servers.get(i).map_or("?", |s| role_name(fr, s.role)), tx(fr, ("est tombé !", "is down!"))),
        News::Asleep(k) => format!("{} {}", g.team.get(k).map_or("?", |m| m.name), tx(fr, ("s’est endormi", "fell asleep"))),
        News::Calmed => tx(fr, ("Client rassuré", "Client reassured")).to_owned(),
        News::Started => tx(fr, ("C’est parti : tiens jusqu’à lundi 9 h", "Here we go: hold on until Monday 9 am")).to_owned(),
    }
}

fn note_text(fr: bool, note: Note, g: &Game) -> (String, u32) {
    match note {
        Note::Fixed => (tx(fr, ("−1 incident", "−1 incident")).to_owned(), LED),
        Note::Relapse => (tx(fr, ("Oups, un nouveau bug", "Oops, a new bug")).to_owned(), ALARM),
        Note::Rollback => ("Rollback".to_owned(), 0x7cc7ff),
        Note::Cached => (tx(fr, ("Cache posé", "Cache on")).to_owned(), ICE),
        Note::SnailGone => (tx(fr, ("Escargot évacué", "Snail gone")).to_owned(), LED),
        Note::Calmed => (tx(fr, ("Client rassuré", "Client reassured")).to_owned(), LED),
        Note::Energy => (tx(fr, ("+ énergie", "+ energy")).to_owned(), LED),
        Note::NoBudget => (tx(fr, ("Pas assez de budget", "Not enough budget")).to_owned(), ALARM),
        Note::NobodyFree => (tx(fr, ("Personne de libre", "Nobody free")).to_owned(), ALARM),
        Note::OnIt(k) => (format!("{} {}", g.team.get(k).map_or("?", |m| m.name), tx(fr, ("y va", "is on it"))), AMBER),
        Note::WellDone => (tx(fr, ("Bien joué !", "Well done!")).to_owned(), LED),
    }
}

fn tutorial_text(fr: bool, step: usize) -> &'static str {
    match step {
        0 => tx(fr, ("Vendredi, 17 h 42. Tu viens de mettre en prod. Voici tes cinq serveurs : tant qu’ils sourient, tout va bien. Leur barre, dessous, dit leur santé.", "Friday, 5:42 pm. You just deployed. Here are your five servers: as long as they smile, all is well. The bar under each one is its health.")),
        1 => tx(fr, ("Ici, l’heure, la satisfaction du client et ton budget. Le budget remonte tout seul. Si la satisfaction tombe à 0 %, tu es convoqué lundi.", "Here, the time, the client’s satisfaction and your budget. The budget refills by itself. If the satisfaction falls to 0 %, you’re called in on Monday.")),
        2 => tx(fr, ("Une fuite mémoire grossit sous l’API. Choisis Hotfix (touche 1), puis clique sur l’API jusqu’à ce que la flaque disparaisse.", "A memory leak grows under the API. Pick Hotfix (key 1), then click the API until the puddle is gone.")),
        3 => tx(fr, ("Des bugs sont arrivés sur l’API par les câbles. Envoie quelqu’un : choisis Astreinte (touche 4), puis clique sur l’API.", "Bugs came onto the API along the cables. Send someone: pick On call (key 4), then click the API.")),
        4 => tx(fr, ("Un pic de trafic arrive d’Internet ! Pose un Cache (touche 3) sur le load balancer : il absorbera la vague.", "A traffic peak comes from Internet! Put a Cache (key 3) on the load balancer: it will soak up the wave.")),
        5 => tx(fr, ("Ton équipe fatigue : regarde la barre sous chacun. Choisis Café (touche 5), puis clique n’importe où dans la salle.", "Your team is tiring: see the bar under each one. Pick Coffee (key 5), then click anywhere in the room.")),
        _ => tx(fr, ("Il y aura aussi le client au téléphone, le DSI qui passe et des cartes à choisir entre les vagues : on te les expliquera la première fois. Tiens jusqu’à lundi 9 h. Bonne astreinte !", "There will also be the client on the phone, the CIO dropping by, and cards to pick between the waves: they’ll be explained the first time. Hold on until Monday 9 am. Good luck on call!")),
    }
}

fn day_name(fr: bool, day: u32) -> &'static str {
    match day {
        0 => tx(fr, ("Ven.", "Fri.")),
        1 => tx(fr, ("Sam.", "Sat.")),
        2 => tx(fr, ("Dim.", "Sun.")),
        _ => tx(fr, ("Lun.", "Mon.")),
    }
}

/// Where the three cards go on the field (the world's rectangles).
fn card_rects() -> [(f32, f32, f32, f32); 3] {
    let (w, h, gap) = (230.0, 250.0, 24.0);
    let x0 = world::W / 2.0 - (3.0 * w + 2.0 * gap) / 2.0;
    [0, 1, 2].map(|k| (x0 + k as f32 * (w + gap), 150.0, w, h))
}

impl App {
    pub(super) fn open_friday(&mut self) {
        self.open_game();
        self.home_which = HomeGame::Friday;
    }

    fn friday_fr(&self) -> bool {
        self.config.settings.language == crate::i18n::Lang::Fr
    }

    /// A new weekend at the level picked; the tutorial first, the very first time.
    fn friday_start(&mut self, tutorial: bool) {
        let seed = super::game::random(&mut self.friday.rng);
        let tips = self.config.settings.friday_tips;
        self.friday.game = Some(if tutorial { Game::tutorial(seed) } else { Game::new(self.friday.level, seed, tips) });
        if let Some(g) = self.friday.game.as_mut().filter(|_| !tutorial) {
            g.news = Some((News::Started, 4.0));
        }
        self.friday.tool = if tutorial { Tool::Hotfix } else { self.friday.tool };
        self.friday.paused = false;
        self.friday.acc = 0.0;
        self.friday.counted = false;
        self.friday.best = false;
    }

    pub(super) fn friday_page(&mut self, ui: &mut Ui, rect: Rect) {
        let playing = self.friday.playing();
        let in_game = self.friday.game.as_ref().is_some_and(|g| g.over.is_none());
        if playing {
            self.count_round(HomeGame::Friday);
        } else if self.friday.game.is_none()
            && let Some(p) = self.game_locked(HomeGame::Friday)
        {
            self.locked_band(ui, rect, HomeGame::Friday, &p);
        }
        self.fold_for_game(in_game);
        let t = self.t();
        let fr = self.friday_fr();
        let theme = self.theme.clone();
        let now = ui.input(|i| i.time);
        self.friday.floor.poll();
        if !in_game {
            self.friday.floor.sync(ui.ctx(), now, Some(60.0), self.config.settings.floor.as_ref());
        }
        if self.friday.game.is_none() {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::new(110.0, 28.0));
            if ui.put(back, egui::Button::new(egui::RichText::new(format!("←  {}", t.home_back)).size(13.0)).frame_when_inactive(false).corner_radius(6.0)).clicked() {
                self.home_game = false;
                return;
            }
        }
        paint_metal(ui.painter(), Pos2::new(rect.center().x, rect.min.y + 34.0), Align2::CENTER_CENTER, t.friday_name, 30.0, theme.accent, 1.0);

        // The keys: only when nothing else has the keyboard.
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let (picked, pause) = ui.input(|i| {
            let pressed = |k: Key| free && i.key_pressed(k);
            let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];
            (keys.iter().position(|&k| pressed(k)), pressed(Key::Space) || pressed(Key::P))
        });
        if let Some(k) = picked {
            self.friday.tool = Tool::ALL[k];
        }
        if pause && in_game {
            self.friday.paused = !self.friday.paused;
            self.friday.acc = 0.0;
        }

        // The world's steps.
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let speed = SPEEDS[self.friday.speed];
        if !self.friday.paused
            && let Some(game) = self.friday.game.as_mut()
        {
            self.friday.acc += dt;
            while self.friday.acc >= STEP {
                self.friday.acc -= STEP;
                game.step(STEP, speed);
            }
        }
        // What it explained once: kept, not again.
        if let Some(seen) = self.friday.game.as_ref().filter(|g| g.tutorial.is_none()).map(|g| g.tips_seen)
            && seen != self.config.settings.friday_tips
        {
            self.config.settings.friday_tips = seen;
            self.save_config();
        }
        if self.friday.game.as_ref().is_some_and(|g| g.over.is_some()) && !self.friday.counted && self.friday.game.as_ref().is_some_and(|g| g.min > 0.0) {
            self.friday_over(now);
        }

        let Some(_) = &self.friday.game else {
            let body = Rect::from_min_max(Pos2::new(rect.min.x + 24.0, rect.min.y + 70.0), Pos2::new(rect.max.x - 24.0, rect.max.y - 16.0));
            self.friday_welcome(ui, body, fr);
            return;
        };
        ui.ctx().request_repaint();

        // The room, as big as the page lets it, the tools under it.
        let room = Vec2::new(rect.width() - 32.0, rect.height() - 64.0 - 64.0);
        let k = (room.x / world::W).min(room.y / world::H).max(0.2);
        let size = Vec2::new(world::W, world::H) * k;
        let field = Rect::from_min_size(Pos2::new(rect.center().x - size.x / 2.0, rect.min.y + 60.0), size);
        let view = View { origin: field.min, k };
        let resp = ui.interact(field, ui.id().with("friday-field"), Sense::click());
        {
            let game = self.friday.game.as_ref().expect("a game");
            let painter = ui.painter_at(field);
            paint_scene(&painter, view, game, now as f32, fr, self.friday.paused, self.friday.best);
        }
        if resp.clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            self.friday_click(view.world(at));
        }
        self.friday_bar(ui, Rect::from_min_max(Pos2::new(field.min.x, field.max.y + 8.0), Pos2::new(field.max.x, field.max.y + 52.0)), fr);
    }

    /// A click on the room: a tip read, a card picked, the tutorial going on, the end read, or the
    /// tool used there.
    fn friday_click(&mut self, (x, y): (f32, f32)) {
        let tool = self.friday.tool;
        let Some(game) = self.friday.game.as_mut() else { return };
        if game.over.is_some() {
            self.friday.game = None;
            return;
        }
        if self.friday.paused {
            self.friday.paused = false;
            return;
        }
        if game.tip.is_some() {
            game.dismiss_tip();
            return;
        }
        if game.pick.is_some() {
            if let Some(k) = card_rects().iter().position(|&(cx, cy, w, h)| x >= cx && x <= cx + w && y >= cy && y <= cy + h) {
                game.choose(k);
            }
            return;
        }
        if let Some(step) = game.tutorial {
            if Game::tutorial_tool(step).is_none() {
                if game.tutorial_next() {
                    // The tutorial done: never again by itself, the real weekend now.
                    self.config.settings.friday_tutorial = true;
                    self.save_config();
                    self.friday_start(false);
                }
                return;
            }
        }
        game.act(tool, x, y);
    }

    /// Under the room: the tools (their key, their cost), the hint of the one picked, the speed, pause, stop.
    fn friday_bar(&mut self, ui: &mut Ui, bar: Rect, fr: bool) {
        let theme = self.theme.clone();
        let Some(game) = &self.friday.game else { return };
        let asked = game.tutorial.and_then(Game::tutorial_tool);
        let mut x = bar.min.x;
        let mut picked = None;
        for (k, tool) in Tool::ALL.into_iter().enumerate() {
            let cost = game.cost(tool);
            let selected = self.friday.tool == tool;
            let afford = game.budget >= cost;
            let text = egui::RichText::new(format!("{}  {}  {}", k + 1, tool_name(fr, tool), cost as u32)).size(12.5).color(if selected { theme.bg } else if afford { theme.text } else { theme.text_muted });
            let pulse = asked == Some(tool) && !selected;
            let stroke = if pulse { Stroke::new(2.0, rgb(AMBER)) } else { Stroke::new(1.0, theme.tab_hover) };
            let button = egui::Button::new(text).fill(if selected { theme.accent } else { theme.chrome_bg }).stroke(stroke).corner_radius(8.0);
            let w = 112.0;
            let r = Rect::from_min_size(Pos2::new(x, bar.min.y), Vec2::new(w, 28.0));
            if ui.put(r, button).on_hover_text(tool_hint(fr, tool)).clicked() {
                picked = Some(tool);
            }
            x += w + 6.0;
        }
        if let Some(tool) = picked {
            self.friday.tool = tool;
        }
        let in_game = game.over.is_none();
        let tutorial = game.tutorial.is_some();
        // On the right: stop, pause, the speed.
        let mut right = bar.max.x;
        let mut button = |ui: &mut Ui, label: String, w: f32| {
            right -= w;
            let r = Rect::from_min_size(Pos2::new(right, bar.min.y), Vec2::new(w, 28.0));
            right -= 6.0;
            ui.put(r, egui::Button::new(egui::RichText::new(label).size(12.5)).corner_radius(8.0)).clicked()
        };
        let stop = in_game && button(ui, tx(fr, if tutorial { ("Passer", "Skip") } else { ("Abandonner", "Give up") }).to_owned(), 96.0);
        let pause = in_game && !tutorial && button(ui, tx(fr, if self.friday.paused { ("Reprendre", "Resume") } else { ("Pause", "Pause") }).to_owned(), 86.0);
        let faster = in_game && !tutorial && button(ui, format!("×{}", SPEEDS[self.friday.speed]), 46.0);
        ui.painter().text(Pos2::new(bar.min.x, bar.min.y + 40.0), Align2::LEFT_CENTER, tool_hint(fr, self.friday.tool), FontId::proportional(12.0), theme.text_muted);
        if faster {
            self.friday.speed = (self.friday.speed + 1) % SPEEDS.len();
        }
        if pause {
            self.friday.paused = !self.friday.paused;
        }
        if stop {
            if tutorial {
                self.config.settings.friday_tutorial = true;
                self.save_config();
                self.friday_start(false);
            } else if let Some(g) = self.friday.game.as_mut() {
                // Given up: called in, the weekend counted as it stood.
                g.over = Some(false);
            }
        }
    }

    /// The weekend over: the best kept, a weekend survived counted, sent to floor.
    fn friday_over(&mut self, now: f64) {
        self.friday.counted = true;
        let Some(g) = &self.friday.game else { return };
        let (score, survived, level, uptime) = (g.score(), g.over == Some(true), g.level, g.uptime());
        let settings = &mut self.config.settings;
        let best = score > settings.friday_best;
        settings.friday_best = settings.friday_best.max(score);
        if survived {
            settings.friday_survived += 1;
        }
        self.save_config();
        self.friday.best = best;
        if score > 0 && crate::floor::pseudo(self.config.settings.floor.as_ref()).is_some() {
            let level = match level {
                Level::Startup => "startup",
                Level::Scaleup => "scaleup",
                Level::BlackFriday => "black-friday",
            };
            self.friday.floor.finished(crate::floor::GameRound { score, details: serde_json::json!({ "score": score, "level": level, "survived": survived, "uptime": (uptime * 1000.0).round() / 10.0 }) });
            self.friday.floor.sync(&self.ctx.clone(), now, None, self.config.settings.floor.as_ref());
        }
    }

    /// The welcome screen: how to play, the level, play (the tutorial the first time), the tutorial
    /// again; beside it, the world's board.
    fn friday_welcome(&mut self, ui: &mut Ui, body: Rect, fr: bool) {
        let t = self.t();
        let theme = self.theme.clone();
        let wide = body.width() >= 780.0;
        let width = body.width().min(if wide { 900.0 } else { 440.0 });
        let left_w = if wide { 460.0 } else { width };
        let x0 = body.center().x - width / 2.0;
        let card = Rect::from_min_size(Pos2::new(x0, body.min.y), Vec2::new(left_w, 500.0f32.min(body.height())));
        let painter = ui.painter().clone();
        painter.rect(card, 12.0, theme.chrome_bg, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let left = card.min.x + 20.0;
        let mut y = card.min.y + 24.0;
        painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, tx(fr, ("Vendredi, 17 h 42 : tu viens de mettre en prod.", "Friday, 5:42 pm: you just deployed.")), FontId::proportional(16.0), theme.text);
        y += 22.0;
        painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, tx(fr, ("Tiens la prod jusqu’à lundi 9 h.", "Keep production up until Monday 9 am.")), FontId::proportional(13.0), theme.text_muted);
        y += 30.0;
        let lines = [
            tx(fr, ("Les incidents grossissent sur les serveurs ou arrivent par les câbles.", "Incidents grow on the servers or come along the cables.")),
            tx(fr, ("Choisis un outil (1 à 5) et clique sur un serveur : Hotfix, Rollback, Cache, Astreinte. Le Café, n’importe où.", "Pick a tool (1 to 5) and click a server: Hotfix, Rollback, Cache, On call. Coffee, anywhere.")),
            tx(fr, ("L’équipe répare seule tant qu’elle a de l’énergie ; la nuit, elle s’endort.", "The team fixes on its own while it has energy; at night, it falls asleep.")),
            tx(fr, ("Entre les vagues, une carte parmi trois. À 0 % de satisfaction : convoqué lundi.", "Between the waves, a card out of three. At 0 % satisfaction: called in on Monday.")),
        ];
        for line in lines {
            let galley = painter.layout(format!("•  {line}"), FontId::proportional(12.5), theme.text_muted, left_w - 40.0);
            let h = galley.size().y;
            painter.galley(Pos2::new(left, y - 8.0), galley, theme.text_muted);
            y += h + 8.0;
        }
        y += 12.0;
        // The level: Black Friday once a weekend survived (always in a build of dev).
        painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, tx(fr, ("Difficulté", "Difficulty")), FontId::proportional(14.0), theme.text);
        y += 22.0;
        let open = self.config.settings.friday_survived > 0 || cfg!(debug_assertions);
        for level in Level::ALL {
            let locked = level == Level::BlackFriday && !open;
            let selected = self.friday.level == level;
            let r = Rect::from_min_size(Pos2::new(left, y - 4.0), Vec2::new(left_w - 40.0, 40.0));
            let resp = ui.interact(r, ui.id().with(("friday-level", level as u8)), if locked { Sense::hover() } else { Sense::click() });
            let fill = if selected { theme.tab_active } else if resp.hovered() { theme.tab_hover.gamma_multiply(0.5) } else { theme.bg.gamma_multiply(0.0) };
            painter.rect(r, 8.0, fill, Stroke::new(if selected { 1.5 } else { 1.0 }, if selected { theme.accent } else { theme.tab_hover }), egui::StrokeKind::Inside);
            let color = if locked { theme.text_muted } else { theme.text };
            painter.text(Pos2::new(r.min.x + 12.0, r.center().y - 8.0), Align2::LEFT_CENTER, level_name(fr, level), FontId::proportional(13.5), color);
            let note = if locked { tx(fr, ("Survis une fois à un week-end pour le débloquer", "Survive a weekend once to unlock it")) } else { level_note(fr, level) };
            painter.text(Pos2::new(r.min.x + 12.0, r.center().y + 9.0), Align2::LEFT_CENTER, note, FontId::proportional(11.5), theme.text_muted);
            if resp.clicked() {
                self.friday.level = level;
            }
            y += 46.0;
        }
        // The last weekend.
        y += 4.0;
        if let Some(best) = (self.config.settings.friday_best > 0).then_some(self.config.settings.friday_best) {
            painter.text(Pos2::new(left, y), Align2::LEFT_CENTER, format!("{} {best}", tx(fr, ("Ton record :", "Your best:"))), FontId::proportional(13.0), theme.accent);
        }
        // Play (the tutorial first, the very first time), or when the games are back; the tutorial again.
        let locked = self.game_locked(HomeGame::Friday).map(|p| t.playtime_back_at.replace("{h}", &self.hour_text(p.end)));
        let button_y = card.max.y - 32.0;
        match locked {
            Some(text) => {
                painter.text(Pos2::new(left, button_y), Align2::LEFT_CENTER, text, FontId::proportional(13.0), theme.accent);
            }
            None => {
                let r = Rect::from_min_size(Pos2::new(left, button_y - 17.0), Vec2::new(140.0, 34.0));
                let b = egui::Button::new(egui::RichText::new(t.four_play).size(14.0).strong().color(theme.bg)).fill(theme.accent).corner_radius(8.0);
                if ui.put(r, b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    let first = !self.config.settings.friday_tutorial;
                    self.friday_start(first);
                }
                let r = Rect::from_min_size(Pos2::new(left + 152.0, button_y - 15.0), Vec2::new(120.0, 30.0));
                if ui.put(r, egui::Button::new(egui::RichText::new(tx(fr, ("Tutoriel", "Tutorial"))).size(13.0)).corner_radius(8.0)).clicked() {
                    self.friday_start(true);
                }
            }
        }
        // The world's board: beside, or under on a narrow page.
        let board = if wide { Rect::from_min_max(Pos2::new(card.max.x + 32.0, body.min.y + 6.0), Pos2::new(x0 + width, body.max.y)) } else { Rect::from_min_max(Pos2::new(x0, card.max.y + 20.0), Pos2::new(x0 + width, body.max.y)) };
        if board.height() > 60.0 {
            let best = (self.config.settings.friday_best > 0).then(|| format!("{} {}", tx(fr, ("Ton record :", "Your best:")), self.config.settings.friday_best));
            if self.game_board(ui, board, &theme, self.friday.floor.board(), best, |s| s.to_string()) {
                self.ask_pseudo();
            }
        }
    }
}

// ---- The room ----------------------------------------------------------------------------------

fn paint_scene(painter: &egui::Painter, v: View, g: &Game, time: f32, fr: bool, paused: bool, best: bool) {
    let (day, hour) = g.clock();
    paint_room(painter, v, g, time, fr, day, hour);
    paint_cables(painter, v, g, time);
    // Back to front: the nearer ones over the others.
    let mut things: Vec<(f32, bool, usize)> = g.servers.iter().enumerate().map(|(i, s)| (s.y, true, i)).collect();
    things.extend(g.team.iter().enumerate().map(|(k, m)| (m.y, false, k)));
    things.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, server, i) in things {
        if server {
            paint_server(painter, v, g, i, time, fr);
        } else {
            paint_member(painter, v, &g.team[i], i, time);
        }
    }
    paint_boss(painter, v, g, time, fr);
    for s in &g.sparks {
        let color = [0xffd36b, ALARM, ICE, 0xd9a066][s.kind as usize % 4];
        painter.rect_filled(Rect::from_center_size(v.p(s.x, s.y), Vec2::splat(3.0 * v.k)), 0.0, rgba(color, (s.t * 2.0).min(1.0)));
    }
    // The host's outage: the room flickers, then goes dark.
    if g.blackout > 0.0 {
        let a = if g.blackout > 2.4 { if (time * 50.0).sin() > 0.0 { 0.75 } else { 0.2 } } else { 0.82 };
        painter.rect_filled(v.r(0.0, 0.0, world::W, world::H), 0.0, Color32::from_black_alpha((a * 255.0) as u8));
    }
    for f in &g.floats {
        let (text, color) = note_text(fr, f.note, g);
        painter.text(v.p(f.x, f.y), Align2::CENTER_CENTER, text, v.font(13.0), rgba(color, f.t.min(1.0)));
    }
    if let Some((news, left)) = g.news.filter(|_| g.tutorial.is_none()) {
        let text = news_text(fr, news, g);
        let galley = painter.layout_no_wrap(text, v.font(14.0), rgba(INK, (left * 2.0).min(1.0)));
        let w = galley.size().x + 28.0 * v.k;
        let r = Rect::from_center_size(v.p(world::W / 2.0, 175.0), Vec2::new(w, 30.0 * v.k));
        painter.rect(r, 15.0 * v.k, rgba(0x0d1118, 0.92 * (left * 2.0).min(1.0)), Stroke::new(1.5 * v.k, rgba(AMBER, (left * 2.0).min(1.0))), egui::StrokeKind::Inside);
        painter.galley(r.center() - galley.size() / 2.0, galley, rgb(INK));
    }
    if let Some(step) = g.tutorial {
        paint_tutorial(painter, v, g, step, time, fr);
    }
    if let Some(cards) = g.pick {
        paint_cards(painter, v, cards, fr);
    }
    if let Some(tip) = g.tip {
        paint_tip(painter, v, tip, fr);
    }
    if paused && g.over.is_none() {
        painter.rect_filled(v.r(0.0, 0.0, world::W, world::H), 0.0, Color32::from_black_alpha(140));
        painter.text(v.p(world::W / 2.0, world::H / 2.0), Align2::CENTER_CENTER, tx(fr, ("Pause", "Paused")), v.font(42.0), rgb(INK));
        painter.text(v.p(world::W / 2.0, world::H / 2.0 + 40.0), Align2::CENTER_CENTER, tx(fr, ("Espace ou clic pour reprendre", "Space or click to resume")), v.font(14.0), rgb(MUTED));
    }
    if g.over.is_some() {
        paint_post_mortem(painter, v, g, fr, best);
    }
}

/// The back wall (the window and its sky by the hour, the wall screen), the floor, the phone's desk,
/// the coffee machine, the couch, the alarm light when a server is down.
fn paint_room(painter: &egui::Painter, v: View, g: &Game, time: f32, fr: bool, day: u32, hour: f32) {
    let night = !(7.0..20.5).contains(&hour);
    painter.rect_filled(v.r(0.0, 0.0, world::W, world::FLOOR_Y), 0.0, rgb(0x1a2130));
    painter.rect_filled(v.r(0.0, world::FLOOR_Y - 14.0, world::W, 14.0), 0.0, rgb(0x141a26));
    // The window.
    let window = v.r(80.0, 26.0, 460.0, 124.0);
    let (top, bottom) = sky(hour);
    gradient(painter, window, top, bottom);
    let clip = painter.with_clip_rect(window);
    if night {
        for i in 0..40u32 {
            let (x, y, r) = (((i * 97) % 440) as f32 + 90.0, ((i * 53) % 100) as f32 + 34.0, (i % 3) as f32 * 0.5 + 0.6);
            let a = 0.5 + 0.5 * (time * 2.0 + i as f32).sin();
            clip.rect_filled(v.r(x, y, r, r), 0.0, rgba(0xffffff, a));
        }
    }
    for i in 0..18u32 {
        let (x, h) = (90.0 + i as f32 * 25.0, 30.0 + ((i * 37) % 50) as f32);
        clip.rect_filled(v.r(x, 150.0 - h, 22.0, h), 0.0, rgb(0x0c1020));
        if night {
            for k in 0..4u32 {
                if (i * 7 + k * 3) % 5 == 0 {
                    clip.rect_filled(v.r(x + 4.0 + (k % 2) as f32 * 9.0, 150.0 - h + 6.0 + (k / 2) as f32 * 10.0, 4.0, 4.0), 0.0, rgb(0xffd36b));
                }
            }
        }
    }
    painter.rect_stroke(window, 6.0 * v.k, Stroke::new(5.0 * v.k, rgb(0x2b3446)), egui::StrokeKind::Middle);
    painter.line_segment([v.p(310.0, 26.0), v.p(310.0, 150.0)], Stroke::new(3.0 * v.k, rgb(0x2b3446)));

    // The wall screen: the time, the satisfaction, the budget.
    let screen = v.r(600.0, 30.0, 300.0, 112.0);
    painter.rect(screen, 8.0 * v.k, rgb(0x0a0e15), Stroke::new(3.0 * v.k, rgb(0x2b3446)), egui::StrokeKind::Middle);
    painter.text(v.p(616.0, 46.0), Align2::LEFT_CENTER, format!("PROD · {}", level_name(fr, g.level).to_uppercase()), v.mono(11.0), rgb(MUTED));
    let total = world::START as f32 + g.min;
    let minute = (total % 1440.0) as u32;
    painter.text(v.p(616.0, 76.0), Align2::LEFT_CENTER, format!("{} {:02}:{:02}", day_name(fr, day), minute / 60, minute % 60), v.font(28.0), rgb(AMBER));
    let sat_color = if g.sat > 60.0 { LED } else if g.sat > 30.0 { AMBER } else { ALARM };
    painter.rect_filled(v.r(616.0, 102.0, 268.0, 12.0), 6.0 * v.k, rgb(0x1c2433));
    painter.rect_filled(v.r(616.0, 102.0, 268.0 * g.sat / 100.0, 12.0), 6.0 * v.k, rgb(sat_color));
    painter.text(v.p(616.0, 128.0), Align2::LEFT_CENTER, format!("{} {:.0} %", tx(fr, ("Satisfaction", "Satisfaction")), g.sat), v.mono(11.0), rgb(INK));
    painter.text(v.p(884.0, 128.0), Align2::RIGHT_CENTER, format!("Budget {:.0}", g.budget), v.mono(11.0), rgb(AMBER));
    // The cards taken, under the screen.
    let mut x = 600.0;
    for card in g.cards.iter().rev().take(6) {
        let (_, color) = rarity_words(fr, card.rarity());
        let name = card_words(fr, *card).0;
        let short: String = name.chars().take(14).collect();
        let galley = painter.layout_no_wrap(short, v.font(9.5), rgb(INK));
        let w = galley.size().x / v.k + 12.0;
        if x + w > 900.0 {
            break;
        }
        painter.rect(v.r(x, 150.0, w, 16.0), 5.0 * v.k, rgb(0x151b26), Stroke::new(1.0 * v.k, rgb(color)), egui::StrokeKind::Inside);
        painter.galley(v.p(x + 6.0, 158.0) - Vec2::new(0.0, galley.size().y / 2.0), galley, rgb(INK));
        x += w + 4.0;
    }

    // The floor: tiles.
    gradient(painter, v.r(0.0, world::FLOOR_Y, world::W, world::H - world::FLOOR_Y), rgb(0x1b2230), rgb(0x10141d));
    let line = Stroke::new(1.0 * v.k, rgba(0xffffff, 0.035));
    let mut y = world::FLOOR_Y + 30.0;
    while y < world::H {
        painter.line_segment([v.p(0.0, y), v.p(world::W, y)], line);
        y += 36.0;
    }
    let mut x = -400.0;
    while x < world::W + 400.0 {
        painter.line_segment([v.p(world::W / 2.0 + (x - world::W / 2.0) * 0.55, world::FLOOR_Y), v.p(x, world::H)], line);
        x += 70.0;
    }

    // The phone's desk: ringing, it shakes.
    painter.rect_filled(v.r(70.0, 196.0, 110.0, 26.0), 4.0 * v.k, rgb(0x2a3242));
    painter.rect_filled(v.r(78.0, 222.0, 6.0, 24.0), 0.0, rgb(0x3b4558));
    painter.rect_filled(v.r(166.0, 222.0, 6.0, 24.0), 0.0, rgb(0x3b4558));
    let ring = if g.phone.is_some() { (time * 30.0).sin() * 2.0 } else { 0.0 };
    painter.rect_filled(v.r(104.0 + ring, 186.0, 28.0, 14.0), 4.0 * v.k, if g.phone.is_some() { rgb(AMBER) } else { rgb(0x596579) });
    if g.phone.is_some() {
        painter.text(v.p(118.0, 172.0), Align2::CENTER_CENTER, "DRING", v.font(14.0), rgb(AMBER));
    }
    // The coffee machine, the couch.
    let (cx, cy) = world::COFFEE;
    painter.rect_filled(v.r(cx - 20.0, cy - 64.0, 40.0, 64.0), 5.0 * v.k, rgb(0x3a2a22));
    painter.rect_filled(v.r(cx - 12.0, cy - 54.0, 24.0, 10.0), 0.0, rgb(0xd9a066));
    painter.rect_filled(v.r(cx - 6.0, cy - 22.0, 12.0, 12.0), 0.0, rgb(INK));
    let (ux, uy) = world::COUCH;
    painter.rect_filled(v.r(ux - 76.0, uy - 42.0, 142.0, 22.0), 10.0 * v.k, rgb(0x464c80));
    painter.rect_filled(v.r(ux - 70.0, uy - 26.0, 130.0, 34.0), 10.0 * v.k, rgb(0x3a3f6b));

    // The alarm light, turning, while a server is down.
    if g.servers.iter().any(|s| s.down) {
        let a = time * 6.0;
        for start in [a, a + std::f32::consts::PI] {
            let mut points = vec![v.p(480.0, 8.0)];
            for k in 0..=8 {
                let t = start + 0.5 * k as f32 / 8.0;
                points.push(v.p(480.0 + t.cos() * 900.0, 8.0 + t.sin() * 900.0));
            }
            painter.add(egui::Shape::convex_polygon(points, rgba(0xff2a2a, 0.16), Stroke::NONE));
        }
        painter.circle_filled(v.p(480.0, 10.0), 9.0 * v.k, rgb(0xff3b3b));
    }
}

/// The cables (and the packets, the bugs, the snail on them), Internet at the left.
fn paint_cables(painter: &egui::Painter, v: View, g: &Game, time: f32) {
    for e in 0..g.edges.len() {
        let points: Vec<Pos2> = (0..=24).map(|k| {
            let (x, y) = g.edge_point(e, k as f32 / 24.0);
            v.p(x, y)
        }).collect();
        painter.add(egui::Shape::line(points, Stroke::new(5.0 * v.k, rgb(0x2f3a50))));
    }
    for p in g.packets.iter().filter(|p| p.t >= 0.0) {
        let (x, y) = g.edge_point(p.edge, p.t);
        painter.circle_filled(v.p(x, y), if p.wave { 4.0 } else { 2.6 } * v.k, rgb(if p.wave { 0x5ab8ff } else { ICE }));
    }
    for (i, b) in g.crawlers.iter().enumerate().filter(|(_, b)| b.t >= 0.0) {
        let (x, y) = g.edge_point(b.edge, b.t);
        paint_bug(painter, v, x, y, time * 20.0 + i as f32);
    }
    if let Some(e) = g.snail {
        let (x, y) = g.edge_point(e, 0.5);
        painter.rect_filled(v.r(x - 14.0, y - 6.0, 26.0, 7.0), 4.0 * v.k, rgb(0xd9c39a));
        painter.circle_filled(v.p(x + 2.0, y - 11.0), 9.0 * v.k, rgb(0xb8864a));
        painter.circle_stroke(v.p(x + 2.0, y - 11.0), 4.5 * v.k, Stroke::new(2.0 * v.k, rgb(0x7a5530)));
        painter.line_segment([v.p(x - 12.0, y - 6.0), v.p(x - 16.0, y - 14.0)], Stroke::new(2.0 * v.k, rgb(0xd9c39a)));
    }
    let (nx, ny) = world::NET;
    for (dx, dy, r) in [(0.0, 0.0, 22.0), (-18.0, 6.0, 15.0), (18.0, 6.0, 16.0)] {
        painter.circle_filled(v.p(nx + dx, ny - 14.0 + dy), r * v.k, rgb(0x26324a));
    }
    painter.text(v.p(nx, ny + 22.0), Align2::CENTER_CENTER, "Internet", v.mono(11.0), rgb(MUTED));
}

/// A bug on the move, its legs going.
fn paint_bug(painter: &egui::Painter, v: View, x: f32, y: f32, phase: f32) {
    let leg = phase.sin() * 2.0;
    for sx in [-4.0, 0.0, 4.0] {
        painter.line_segment([v.p(x + sx, y - 3.0), v.p(x + sx + leg, y + 4.0)], Stroke::new(1.5 * v.k, rgb(0x2d4a1e)));
    }
    painter.add(egui::Shape::ellipse_filled(v.p(x, y - 4.0), Vec2::new(7.0, 4.5) * v.k, rgb(0x7fd65a)));
    painter.rect_filled(v.r(x + 4.0, y - 6.0, 2.0, 2.0), 0.0, rgb(0x10141d));
}

/// A server: its leak under it, its rack seen from a little above, its face (two LEDs for eyes, the
/// grille for a mouth) by its health, its bays blinking, sweat, smoke when down, the bugs on it, its
/// name and health.
fn paint_server(painter: &egui::Painter, v: View, g: &Game, i: usize, time: f32, fr: bool) {
    let s = &g.servers[i];
    let (x, y, w, h) = (s.x, s.y, 64.0, 112.0);
    if s.leak > 0.0 {
        let r = 18.0 + s.leak * 46.0;
        painter.add(egui::Shape::ellipse_filled(v.p(x, y + 4.0), Vec2::new(r, r * 0.32) * v.k, rgba(0xb06cff, 0.35 + s.leak * 0.35)));
        painter.add(egui::Shape::ellipse_filled(v.p(x - r * 0.3, y + 1.0), Vec2::new(r * 0.2, r * 0.06) * v.k, rgba(0xdcbeff, 0.5)));
    }
    painter.add(egui::Shape::ellipse_filled(v.p(x + 6.0, y + 2.0), Vec2::new(42.0, 9.0) * v.k, Color32::from_black_alpha(90)));
    let shake = if s.hp < 25.0 && !s.down { (time * 40.0).sin() * 1.2 } else { 0.0 };
    let (left, top) = (x - w / 2.0 + shake, y - h);
    painter.add(egui::Shape::convex_polygon(vec![v.p(left, top), v.p(left + 14.0, top - 10.0), v.p(left + w + 14.0, top - 10.0), v.p(left + w, top)], rgb(0x3a4560), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(vec![v.p(left + w, top), v.p(left + w + 14.0, top - 10.0), v.p(left + w + 14.0, y - 10.0), v.p(left + w, y)], rgb(0x1e2534), Stroke::NONE));
    let body = if s.down { rgb(0x20242d) } else { rgb(0x2c3446).lerp_to_gamma(rgb(0x7cc7ff), s.flash * 0.6) };
    let edge = if s.cache > 0.0 { Stroke::new(2.5 * v.k, rgb(ICE)) } else if s.rust { Stroke::new(2.5 * v.k, rgb(0xdea584)) } else { Stroke::new(1.5 * v.k, rgb(0x3b465e)) };
    painter.rect(v.r(left, top, w, h), 4.0 * v.k, body, edge, egui::StrokeKind::Inside);
    // Behind a feature flag: a ring around it.
    if s.flag > 0.0 {
        painter.rect_stroke(v.r(left - 6.0, top - 16.0, w + 26.0, h + 22.0), 8.0 * v.k, Stroke::new(1.5 * v.k, rgba(0x5ab8ff, 0.5 + 0.3 * (time * 3.0).sin())), egui::StrokeKind::Middle);
    }
    // The face.
    let ok = s.hp > 60.0;
    let hurt = s.hp > 25.0;
    let eye = if s.down || !hurt { ALARM } else if ok { LED } else { AMBER };
    let ey = top + 30.0;
    let blink = (time * 1.3 + x).sin() > 0.97;
    for ex in [left + 20.0, left + w - 20.0] {
        if s.down {
            let st = Stroke::new(3.0 * v.k, rgb(eye));
            painter.line_segment([v.p(ex - 5.0, ey - 5.0), v.p(ex + 5.0, ey + 5.0)], st);
            painter.line_segment([v.p(ex + 5.0, ey - 5.0), v.p(ex - 5.0, ey + 5.0)], st);
        } else {
            painter.circle_filled(v.p(ex, ey), 9.0 * v.k, rgba(eye, 0.18));
            if blink {
                painter.rect_filled(v.r(ex - 6.0, ey - 1.0, 12.0, 2.0), 0.0, rgb(eye));
            } else {
                painter.circle_filled(v.p(ex, ey), 5.5 * v.k, rgb(eye));
            }
        }
    }
    let my = top + 54.0;
    let mouth = Stroke::new(2.5 * v.k, if s.down { rgb(0x555c6a) } else { rgb(0x8fa0bb) });
    if s.down || (hurt && !ok) {
        painter.line_segment([v.p(left + 18.0, my + if s.down { 4.0 } else { 0.0 }), v.p(left + w - 18.0, my + if s.down { 4.0 } else { 0.0 })], mouth);
    } else if ok {
        let points: Vec<Pos2> = (0..=10).map(|k| {
            let a = (0.2 + 0.6 * k as f32 / 10.0) * std::f32::consts::PI;
            v.p(left + w / 2.0 + a.cos() * 13.0, my - 6.0 + a.sin() * 13.0)
        }).collect();
        painter.add(egui::Shape::line(points, mouth));
    } else {
        let points: Vec<Pos2> = (0..=8).map(|k| v.p(left + 16.0 + k as f32 * (w - 32.0) / 8.0, my + (k as f32 * 1.6 + time * 8.0).sin() * 3.0)).collect();
        painter.add(egui::Shape::line(points, mouth));
    }
    // The drive bays.
    for k in 0..3 {
        let ky = top + 72.0 + k as f32 * 12.0;
        painter.rect_filled(v.r(left + 8.0, ky, w - 16.0, 8.0), 0.0, rgb(0x20283a));
        let on = !s.down && (time * (7.0 + k as f32 * 3.0) + x).sin() > 0.2;
        painter.rect_filled(v.r(left + w - 16.0, ky + 2.0, 4.0, 4.0), 0.0, rgb(if on { LED } else { 0x2d3a2f }));
    }
    // Sweat, smoke.
    if !s.down && s.hp < 60.0 {
        let d = (time * 0.8 + x).rem_euclid(1.0);
        painter.add(egui::Shape::ellipse_filled(v.p(left + w + 4.0, top + 18.0 + d * 24.0), Vec2::new(3.0, 4.5) * v.k, rgb(ICE)));
    }
    if s.down {
        for k in 0..5 {
            let d = (time * 0.35 + k as f32 / 5.0).rem_euclid(1.0);
            painter.circle_filled(v.p(x + (k as f32 * 2.0 + time).sin() * 10.0, top - 10.0 - d * 60.0), (8.0 + d * 16.0) * v.k, rgba(0x78808c, 0.45 * (1.0 - d)));
        }
    }
    // The bugs on it.
    for k in 0..s.bugs {
        let (bx, by) = (left + 10.0 + (k % 3) as f32 * 22.0, top + 8.0 + (k / 3) as f32 * 100.0 + (time * 6.0 + k as f32).sin() * 1.5);
        painter.add(egui::Shape::ellipse_filled(v.p(bx, by), Vec2::new(6.0, 4.0) * v.k, rgb(0x7fd65a)));
    }
    // Monitoring: a "!" over it as soon as something starts.
    if g.cards.contains(&Card::Monitoring) && s.troubled() && !s.down {
        painter.circle_filled(v.p(x + 7.0, top - 26.0), 9.0 * v.k, rgb(AMBER));
        painter.text(v.p(x + 7.0, top - 26.0), Align2::CENTER_CENTER, "!", v.font(13.0), rgb(0x1b1f2a));
    }
    // Its name and health.
    painter.text(v.p(x, y + 20.0), Align2::CENTER_CENTER, role_name(fr, s.role), v.font(12.0), rgb(INK));
    painter.rect_filled(v.r(x - 26.0, y + 28.0, 52.0, 5.0), 3.0 * v.k, rgb(0x1c2433));
    painter.rect_filled(v.r(x - 26.0, y + 28.0, 52.0 * s.hp.max(0.0) / 100.0, 5.0), 3.0 * v.k, rgb(eye));
}

/// Someone of the team: walking, at work (a laptop, a bubble filling), on the phone, asleep (z's).
fn paint_member(painter: &egui::Painter, v: View, m: &world::Member, k: usize, time: f32) {
    let (x, y) = (m.x, m.y);
    let shirt = rgb(SHIRTS[m.look as usize % SHIRTS.len()]);
    let skin = rgb(0xf2c9a0);
    if m.away > 0.0 {
        return;
    }
    painter.add(egui::Shape::ellipse_filled(v.p(x, y + 2.0), Vec2::new(12.0, 4.0) * v.k, Color32::from_black_alpha(76)));
    if m.asleep {
        painter.rect_filled(v.r(x - 16.0, y - 10.0, 30.0, 12.0), 6.0 * v.k, shirt);
        painter.circle_filled(v.p(x + 18.0, y - 6.0), 7.0 * v.k, skin);
        let z = (time * 0.6 + k as f32 * 0.3).rem_euclid(1.0);
        painter.text(v.p(x + 22.0 + z * 10.0, y - 18.0 - z * 22.0), Align2::CENTER_CENTER, "z", v.font(13.0), rgba(MUTED, 1.0 - z));
        painter.text(v.p(x + 30.0 + z * 8.0, y - 30.0 - z * 26.0), Align2::CENTER_CENTER, "Z", v.font(15.0), rgba(MUTED, 1.0 - z));
        return;
    }
    let bob = if m.walking { (time * 10.0).sin().abs() * 3.0 } else { 0.0 };
    let legs = if m.walking { (time * 10.0).sin() * 4.0 } else { 0.0 };
    let leg = Stroke::new(4.0 * v.k, rgb(0x2a3242));
    painter.line_segment([v.p(x - 4.0, y - 12.0), v.p(x - 4.0 + legs, y)], leg);
    painter.line_segment([v.p(x + 4.0, y - 12.0), v.p(x + 4.0 - legs, y)], leg);
    painter.rect_filled(v.r(x - 10.0, y - 34.0 - bob, 20.0, 24.0), 7.0 * v.k, shirt);
    painter.circle_filled(v.p(x, y - 42.0 - bob), 8.0 * v.k, skin);
    let eye = rgb(0x1b1f2a);
    if m.energy < 0.3 {
        painter.rect_filled(v.r(x - 4.0, y - 43.0 - bob, 3.0, 1.5), 0.0, eye);
        painter.rect_filled(v.r(x + 1.0, y - 43.0 - bob, 3.0, 1.5), 0.0, eye);
    } else {
        painter.rect_filled(v.r(x - 4.0, y - 44.0 - bob, 2.0, 2.5), 0.0, eye);
        painter.rect_filled(v.r(x + 2.0, y - 44.0 - bob, 2.0, 2.5), 0.0, eye);
    }
    // At work: the laptop, a bubble filling (a wrench, or the phone).
    if !m.walking && (m.job.is_some() || m.work > 0.0 || m.phoning) {
        painter.rect_filled(v.r(x - 14.0, y - 26.0, 16.0, 3.0), 0.0, rgb(0x596579));
        let bubble = v.r(x - 14.0, y - 76.0, 28.0, 22.0);
        painter.rect_filled(bubble, 7.0 * v.k, rgb(INK));
        if m.phoning {
            painter.rect_filled(v.r(x - 6.0, y - 70.0, 12.0, 6.0), 3.0 * v.k, rgb(0x596579));
        } else {
            painter.line_segment([v.p(x - 5.0, y - 59.0), v.p(x + 4.0, y - 69.0)], Stroke::new(2.5 * v.k, rgb(0x596579)));
            painter.circle_stroke(v.p(x + 5.0, y - 70.0), 3.0 * v.k, Stroke::new(2.0 * v.k, rgb(0x596579)));
        }
        let need = if m.phoning { 2.5 } else { 1.2 };
        painter.rect_filled(v.r(x - 12.0, y - 52.0, 24.0 * (m.work / need).min(1.0), 3.0), 0.0, rgb(AMBER));
    }
    // The energy bar, the name.
    painter.rect_filled(v.r(x - 12.0, y + 8.0, 24.0, 3.0), 0.0, rgb(0x1c2433));
    painter.rect_filled(v.r(x - 12.0, y + 8.0, 24.0 * m.energy, 3.0), 0.0, rgb(if m.energy > 0.4 { LED } else { AMBER }));
    painter.text(v.p(x, y + 20.0), Align2::CENTER_CENTER, m.name, v.mono(10.0), rgb(MUTED));
}

/// The DSI, a silhouette walking across with his tie, and what he says.
fn paint_boss(painter: &egui::Painter, v: View, g: &Game, time: f32, fr: bool) {
    let Some(x) = g.boss else { return };
    let y = 250.0 + (time * 4.0).sin().abs() * 2.0;
    let dark = Color32::from_rgba_unmultiplied(8, 10, 16, 217);
    painter.rect_filled(v.r(x - 13.0, y - 52.0, 26.0, 40.0), 8.0 * v.k, dark);
    painter.circle_filled(v.p(x, y - 62.0), 10.0 * v.k, dark);
    painter.rect_filled(v.r(x - 9.0, y - 12.0, 7.0, 14.0), 0.0, dark);
    painter.rect_filled(v.r(x + 2.0, y - 12.0, 7.0, 14.0), 0.0, dark);
    painter.rect_filled(v.r(x - 2.0, y - 50.0, 4.0, 18.0), 0.0, rgb(ALARM));
    painter.text(v.p(x, y - 84.0), Align2::CENTER_CENTER, tx(fr, ("« Ça avance ? »", "“How’s it going?”")), v.font(11.0), rgb(INK));
}

/// A bubble of paper, its lines wrapped: (rect of the world) the lines, a footer.
fn paint_bubble(painter: &egui::Painter, v: View, r: (f32, f32, f32, f32), head: &str, body: &str, foot: &str, foot_color: u32) {
    let rect = v.r(r.0, r.1, r.2, r.3);
    painter.rect_filled(rect.translate(Vec2::new(0.0, 4.0 * v.k)), 12.0 * v.k, Color32::from_black_alpha(90));
    painter.rect_filled(rect, 12.0 * v.k, rgb(PAPER));
    painter.text(v.p(r.0 + 16.0, r.1 + 22.0), Align2::LEFT_CENTER, head, v.mono(11.0), rgb(0x7a5a1e));
    let galley = painter.layout(body.to_owned(), v.font(14.5), rgb(0x1f2430), (r.2 - 32.0) * v.k);
    painter.galley(v.p(r.0 + 16.0, r.1 + 36.0), galley, rgb(0x1f2430));
    painter.text(v.p(r.0 + 16.0, r.1 + r.3 - 16.0), Align2::LEFT_CENTER, foot, v.font(12.0), rgb(foot_color));
}

/// The bubble's height for `body` in a bubble `w` wide.
fn bubble_height(painter: &egui::Painter, v: View, body: &str, w: f32) -> f32 {
    let galley = painter.layout(body.to_owned(), v.font(14.5), Color32::WHITE, (w - 32.0) * v.k);
    galley.size().y / v.k + 70.0
}

/// The tutorial: the room dimmed around what is shown, a bubble beside it.
fn paint_tutorial(painter: &egui::Painter, v: View, g: &Game, step: usize, time: f32, fr: bool) {
    let (fx, fy, fw, fh) = g.tutorial_focus(step);
    let whole = fw >= world::W;
    let dim = Color32::from_rgba_unmultiplied(4, 6, 10, 158);
    if whole {
        painter.rect_filled(v.r(0.0, 0.0, world::W, world::H), 0.0, dim);
    } else {
        // Around the hole: four bands.
        painter.rect_filled(v.r(0.0, 0.0, world::W, fy), 0.0, dim);
        painter.rect_filled(v.r(0.0, fy + fh, world::W, world::H - fy - fh), 0.0, dim);
        painter.rect_filled(v.r(0.0, fy, fx, fh), 0.0, dim);
        painter.rect_filled(v.r(fx + fw, fy, world::W - fx - fw, fh), 0.0, dim);
        painter.rect_stroke(v.r(fx, fy, fw, fh), 14.0 * v.k, Stroke::new(2.5 * v.k, rgba(AMBER, 0.55 + 0.45 * (time * 4.0).sin())), egui::StrokeKind::Middle);
    }
    let body = tutorial_text(fr, step);
    let bw = 360.0;
    let bh = bubble_height(painter, v, body, bw);
    let (bx, by) = if whole {
        (world::W / 2.0 - bw / 2.0, world::H / 2.0 - bh / 2.0)
    } else if fx + fw + 16.0 + bw < world::W {
        (fx + fw + 16.0, fy.clamp(12.0, world::H - bh - 12.0))
    } else if fx - 16.0 - bw > 0.0 {
        (fx - 16.0 - bw, fy.clamp(12.0, world::H - bh - 12.0))
    } else {
        (fx.clamp(12.0, world::W - bw - 12.0), if fy + fh + 12.0 + bh < world::H { fy + fh + 12.0 } else { (fy - bh - 12.0).max(12.0) })
    };
    let head = format!("{} · {} / {}", tx(fr, ("PREMIÈRE ASTREINTE", "FIRST ON-CALL")), step + 1, world::TUTORIAL_STEPS);
    let asks = Game::tutorial_tool(step).is_some();
    let (foot, color) = if !asks {
        (tx(fr, ("Clique pour continuer", "Click to go on")), 0x2f6f4a)
    } else if g.tutorial_well_done() {
        (tx(fr, ("✓ Bien joué", "✓ Well done")), 0x2f6f4a)
    } else {
        (tx(fr, ("À toi de jouer", "Your turn")), 0xb0461e)
    };
    paint_bubble(painter, v, (bx, by, bw, bh), &head, body, foot, color);
}

/// The three cards to pick from.
fn paint_cards(painter: &egui::Painter, v: View, cards: [Card; 3], fr: bool) {
    painter.rect_filled(v.r(0.0, 0.0, world::W, world::H), 0.0, Color32::from_black_alpha(170));
    painter.text(v.p(world::W / 2.0, 110.0), Align2::CENTER_CENTER, tx(fr, ("Pause entre deux vagues : choisis une carte", "A break between two waves: pick a card")), v.font(20.0), rgb(INK));
    for (card, (x, y, w, h)) in cards.into_iter().zip(card_rects()) {
        let (name, desc, up, down) = card_words(fr, card);
        let (rarity, color) = rarity_words(fr, card.rarity());
        let r = v.r(x, y, w, h);
        painter.rect(r, 12.0 * v.k, rgb(0x1c2433), Stroke::new(2.0 * v.k, rgb(color)), egui::StrokeKind::Inside);
        painter.text(v.p(x + 16.0, y + 24.0), Align2::LEFT_CENTER, rarity, v.mono(11.0), rgb(color));
        let title = painter.layout(name.to_owned(), v.font(17.0), rgb(INK), (w - 32.0) * v.k);
        let title_h = title.size().y / v.k;
        painter.galley(v.p(x + 16.0, y + 40.0), title, rgb(INK));
        let text = painter.layout(desc.to_owned(), v.font(12.5), rgb(MUTED), (w - 32.0) * v.k);
        let text_h = text.size().y / v.k;
        painter.galley(v.p(x + 16.0, y + 48.0 + title_h), text, rgb(MUTED));
        let mut ly = y + 64.0 + title_h + text_h;
        for (line, c, sign) in [(up, LED, "+"), (down, ALARM, "−")] {
            if line.is_empty() {
                continue;
            }
            let galley = painter.layout(format!("{sign} {line}"), v.font(12.5), rgb(c), (w - 32.0) * v.k);
            let lh = galley.size().y / v.k;
            painter.galley(v.p(x + 16.0, ly), galley, rgb(c));
            ly += lh + 6.0;
        }
        painter.text(v.p(x + w / 2.0, y + h - 20.0), Align2::CENTER_CENTER, tx(fr, ("Clique pour la prendre", "Click to take it")), v.font(11.5), rgb(MUTED));
    }
}

/// A tip, the game waiting: in the middle of the room.
fn paint_tip(painter: &egui::Painter, v: View, tip: Tip, fr: bool) {
    painter.rect_filled(v.r(0.0, 0.0, world::W, world::H), 0.0, Color32::from_black_alpha(120));
    let (title, body) = tip_words(fr, tip);
    let w = 420.0;
    let h = bubble_height(painter, v, body, w);
    paint_bubble(painter, v, (world::W / 2.0 - w / 2.0, world::H / 2.0 - h / 2.0, w, h), &title.to_uppercase(), body, tx(fr, ("Compris : clique pour reprendre", "Got it: click to go on")), 0x2f6f4a);
}

/// Monday morning: the post-mortem, stamped.
fn paint_post_mortem(painter: &egui::Painter, v: View, g: &Game, fr: bool, best: bool) {
    painter.rect_filled(v.r(0.0, 0.0, world::W, world::H), 0.0, Color32::from_black_alpha(190));
    let (x, y, w, h) = (world::W / 2.0 - 230.0, 70.0, 460.0, 400.0);
    painter.rect_filled(v.r(x, y, w, h), 6.0 * v.k, rgb(PAPER));
    let ink = rgb(0x1f2430);
    painter.text(v.p(x + 24.0, y + 34.0), Align2::LEFT_CENTER, format!("Post-mortem · {}", level_name(fr, g.level)), v.font(19.0), ink);
    let survived = g.over == Some(true);
    let (stamp, color) = if survived { (tx(fr, ("SURVÉCU", "SURVIVED")), 0x2f8f55) } else { (tx(fr, ("CONVOQUÉ", "CALLED IN")), 0xc0392b) };
    let sr = v.r(x + w - 190.0, y + 56.0, 160.0, 46.0);
    painter.rect_stroke(sr, 6.0 * v.k, Stroke::new(3.0 * v.k, rgb(color)), egui::StrokeKind::Inside);
    painter.text(sr.center(), Align2::CENTER_CENTER, stamp, v.font(22.0), rgb(color));
    let (day, _) = g.clock();
    let total = world::START as f32 + g.min;
    let minute = (total % 1440.0) as u32;
    let lines = [
        (tx(fr, ("Tenu jusqu’à", "Held until")), format!("{} {:02}:{:02}", day_name(fr, day), minute / 60, minute % 60)),
        (tx(fr, ("Disponibilité", "Uptime")), format!("{:.1} %", g.uptime() * 100.0)),
        (tx(fr, ("Incidents éteints", "Incidents put out")), g.stats.fixed.to_string()),
        (tx(fr, ("Serveurs tombés", "Servers down")), g.stats.downs.to_string()),
        (tx(fr, ("Satisfaction client", "Client satisfaction")), format!("{:.0} %", g.sat)),
        (tx(fr, ("Heures de sommeil de l’équipe", "The team’s hours of sleep")), format!("{:.0} h", g.stats.sleep / 60.0)),
        (tx(fr, ("Cafés", "Coffees")), g.stats.coffees.to_string()),
    ];
    let mut ly = y + 128.0;
    for (label, value) in lines {
        painter.text(v.p(x + 24.0, ly), Align2::LEFT_CENTER, label, v.mono(13.0), ink);
        painter.text(v.p(x + w - 24.0, ly), Align2::RIGHT_CENTER, value, v.mono(13.0), ink);
        ly += 26.0;
    }
    painter.line_segment([v.p(x + 24.0, ly - 6.0), v.p(x + w - 24.0, ly - 6.0)], Stroke::new(1.0 * v.k, rgba(0x1f2430, 0.3)));
    ly += 10.0;
    painter.text(v.p(x + 24.0, ly), Align2::LEFT_CENTER, "Score", v.font(18.0), ink);
    painter.text(v.p(x + w - 24.0, ly), Align2::RIGHT_CENTER, g.score().to_string(), v.font(22.0), ink);
    if best {
        painter.text(v.p(x + w / 2.0, ly + 30.0), Align2::CENTER_CENTER, tx(fr, ("Nouveau record !", "New best!")), v.font(14.0), rgb(0xb0461e));
    }
    painter.text(v.p(world::W / 2.0, y + h + 26.0), Align2::CENTER_CENTER, tx(fr, ("Clique pour revenir", "Click to go back")), v.font(13.0), rgb(MUTED));
}

/// The game's icon: a small server rack with two LED eyes and a smile.
pub(super) fn paint_friday_icon(painter: &egui::Painter, c: Pos2, theme: &Theme, scale: f32) {
    let s = scale;
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let body = Rect::from_min_max(at(-6.5, -8.5), at(6.5, 8.5));
    painter.rect(body, 2.0 * s, theme.chrome_bg, Stroke::new(1.3 * s, theme.accent), egui::StrokeKind::Inside);
    for x in [-2.8, 2.8] {
        painter.circle_filled(at(x, -3.5), 1.4 * s, rgb(LED));
    }
    let smile: Vec<Pos2> = (0..=6).map(|k| {
        let a = (0.2 + 0.6 * k as f32 / 6.0) * std::f32::consts::PI;
        at(a.cos() * 3.0, -0.5 + a.sin() * 2.6)
    }).collect();
    painter.add(egui::Shape::line(smile, Stroke::new(1.0 * s, theme.text_muted)));
    for y in [4.5, 6.5] {
        painter.line_segment([at(-4.0, y), at(4.0, y)], Stroke::new(0.9 * s, theme.text_muted));
    }
}
