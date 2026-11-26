//! Open Space, the world: the map every player shares on floor (`/v1/openspace`), each one's open space
//! on its lot between bot companies; the raids on them (trophies, loot, a shield for the one raided),
//! the day's quests on the map, the events (the « Audit surprise » boss everyone brings down), the
//! guilds (invitations, troops given, the week's quests and war) and the ranking. floor keeps the
//! world and decides what a raid brings; the raids themselves are played here, on the player's base
//! read from their synced progress, or on a base drawn like the campaign's (`rival_base`).
//!
//! `WorldLink` asks floor in the background (a thread per request) and keeps the last answers; the
//! page takes what came back each frame (`take`).

use crate::config::FloorAccount;
use crate::openspace::Building;
use crate::openspace_battle::Troop;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct GuildBrief {
    pub id: String,
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub color: u32,
    #[serde(default)]
    pub bot: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Player {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub hq: u8,
    pub trophies: i64,
    #[serde(default)]
    pub shield_until: i64,
    #[serde(default)]
    pub guild: Option<GuildBrief>,
    #[serde(default)]
    pub me: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Bot {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub hq: u8,
    /// The campaign level its base is drawn from.
    pub level: u32,
    pub trophies: i64,
    pub guild: GuildBrief,
    /// When it's rebuilt after the player's last raid (unix ms; 0: ready).
    #[serde(default)]
    pub ready_at: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    pub key: i64,
    pub starts: i64,
    pub ends: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Boss {
    pub x: i32,
    pub y: i32,
    pub hp: i64,
    pub max: i64,
    #[serde(default)]
    pub ready_at: i64,
    #[serde(default)]
    pub hit: bool,
    #[serde(default)]
    pub claimed: bool,
    #[serde(default)]
    pub reward: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Reward {
    #[serde(default)]
    pub coffee: f64,
    #[serde(default)]
    pub budget: f64,
    #[serde(default)]
    pub tickets: u32,
    #[serde(default)]
    pub troops: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Quest {
    pub id: String,
    /// convoy, abandoned, hackers, lost.
    pub kind: String,
    pub x: i32,
    pub y: i32,
    pub level: u32,
    pub raid: bool,
    #[serde(default)]
    pub ends: i64,
    pub reward: Reward,
    #[serde(default)]
    pub done: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Defense {
    pub id: i64,
    #[serde(default)]
    pub attacker: Option<String>,
    pub stars: u8,
    pub destroyed: f64,
    pub trophies: i64,
    pub coffee: f64,
    pub budget: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InviteGuild {
    pub id: String,
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub members: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Invite {
    pub guild: InviteGuild,
    #[serde(default)]
    pub from: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Me {
    pub x: i32,
    pub y: i32,
    pub hq: u8,
    pub trophies: i64,
    #[serde(default)]
    pub shield_until: i64,
    #[serde(default)]
    pub guild: Option<GuildBrief>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct World {
    pub size: i32,
    pub me: Me,
    pub players: Vec<Player>,
    pub bots: Vec<Bot>,
    #[serde(default)]
    pub event: Option<Event>,
    pub next_event: Event,
    #[serde(default)]
    pub boss: Option<Boss>,
    pub quests: Vec<Quest>,
    #[serde(default)]
    pub defenses: Vec<Defense>,
    #[serde(default)]
    pub invites: Vec<Invite>,
}

/// What floor gives to attack a target: a player's base, or the level of a bot's or the boss's.
#[derive(Clone, Debug, Deserialize)]
pub struct Base {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub level: Option<u32>,
    #[serde(default)]
    pub hq: Option<u8>,
    #[serde(default)]
    pub buildings: Option<Vec<Building>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Struck {
    pub trophies: i64,
    pub total: i64,
    pub coffee: f64,
    pub budget: f64,
    #[serde(default)]
    pub tickets: u32,
    #[serde(default)]
    pub event: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BoardPlayer {
    pub rank: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub hq: u8,
    pub trophies: i64,
    #[serde(default)]
    pub guild: Option<GuildBrief>,
    #[serde(default)]
    pub me: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BoardGuild {
    pub rank: u32,
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub color: u32,
    pub members: u32,
    #[serde(default)]
    pub trophies: i64,
    #[serde(default)]
    pub mine: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Board {
    pub players: Vec<BoardPlayer>,
    #[serde(default)]
    pub me: Option<BoardPlayer>,
    pub guilds: Vec<BoardGuild>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GuildRow {
    pub id: String,
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub open: bool,
    pub members: u32,
    #[serde(default)]
    pub trophies: i64,
    #[serde(default)]
    pub invited: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GuildHead {
    pub id: String,
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub color: u32,
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub leader: bool,
    pub trophies: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Member {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub bot: bool,
    pub hq: u8,
    pub trophies: i64,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub me: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Sent {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Donation {
    #[serde(default)]
    pub from: Option<String>,
    pub troops: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Gift {
    pub from: String,
    pub troops: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GuildQuest {
    /// stars, wins, raze, donate.
    pub id: String,
    pub target: u32,
    pub done: u32,
    pub complete: bool,
    pub claim: String,
    #[serde(default)]
    pub claimed: bool,
    #[serde(default)]
    pub reward: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Side {
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub bot: bool,
    pub score: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct War {
    pub ends: i64,
    pub us: Side,
    pub them: Side,
    /// win, loss, draw (once over).
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub claim: Option<String>,
    #[serde(default)]
    pub claimed: bool,
    #[serde(default)]
    pub reward: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Guild {
    pub guild: GuildHead,
    pub members: Vec<Member>,
    #[serde(default)]
    pub invites: Vec<Sent>,
    #[serde(default)]
    pub donations: Vec<Donation>,
    #[serde(default)]
    pub gift: Option<Gift>,
    #[serde(default)]
    pub next_gift: i64,
    #[serde(default)]
    pub donated_today: u32,
    pub quests: Vec<GuildQuest>,
    pub war: War,
    #[serde(default)]
    pub last_war: Option<War>,
}

/// The troops of a reward or a donation, as the army counts them (unknown names left out).
pub fn troops(list: &BTreeMap<String, u32>) -> Vec<(Troop, u32)> {
    list.iter().filter_map(|(k, n)| serde_json::from_value::<Troop>(serde_json::Value::String(k.clone())).ok().map(|t| (t, *n))).filter(|(t, n)| *n > 0 && *t != Troop::Lead).collect()
}

/// A troop's name on floor.
pub fn troop_key(t: Troop) -> String {
    serde_json::to_value(t).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// What to ask floor.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    World,
    Base(String),
    Attack { target: String, stars: u8, destroyed: f32 },
    Seen(Vec<i64>),
    Quest { id: String, stars: u8 },
    EventClaim,
    Board,
    Guilds,
    Guild,
    Create { name: String, tag: String, open: bool },
    Join(String),
    Decline(String),
    Leave,
    Invite(String),
    Kick(String),
    Open(bool),
    Donate { to: String, troops: Vec<(Troop, u32)> },
    TakeDonations,
    Claim(String),
}

/// What came back.
#[derive(Clone, Debug)]
pub enum Answer {
    World(Box<World>),
    Base(Box<Base>),
    Struck(Struck),
    Quest { id: String, reward: Reward },
    Tickets(u32),
    Board(Board),
    Guilds(Vec<GuildRow>),
    /// None: no guild.
    Guild(Option<Box<Guild>>),
    Troops(Vec<(Troop, u32)>),
    Done,
    /// floor refused (its message, its code) or couldn't be reached (`code` none, `message` empty).
    Failed { ask: Ask, message: String, code: Option<String> },
}

#[derive(Default)]
struct Shared {
    answers: Vec<(Ask, Answer)>,
    busy: usize,
}

#[derive(Default)]
pub struct WorldLink {
    shared: Arc<Mutex<Shared>>,
}

impl WorldLink {
    /// Asks floor, in the background.
    pub fn ask(&self, ctx: &egui::Context, account: &FloorAccount, ask: Ask) {
        let (shared, ctx, account) = (self.shared.clone(), ctx.clone(), account.clone());
        if let Ok(mut s) = shared.lock() {
            s.busy += 1;
        }
        std::thread::spawn(move || {
            let answer = call(&account, &ask).unwrap_or_else(|(message, code)| Answer::Failed { ask: ask.clone(), message, code });
            if let Ok(mut s) = shared.lock() {
                s.answers.push((ask, answer));
                s.busy -= 1;
            }
            ctx.request_repaint();
        });
    }

    /// The answers come back since the last call, in order.
    pub fn take(&self) -> Vec<(Ask, Answer)> {
        self.shared.lock().map(|mut s| std::mem::take(&mut s.answers)).unwrap_or_default()
    }
}

type Failure = (String, Option<String>);

fn call(account: &FloorAccount, ask: &Ask) -> Result<Answer, Failure> {
    use serde_json::json;
    let enc = |s: &str| s.replace('%', "%25").replace('/', "%2F").replace('?', "%3F").replace('#', "%23").replace(' ', "%20");
    let (method, path, body): (&str, String, Option<serde_json::Value>) = match ask {
        Ask::World => ("GET", "world".into(), None),
        Ask::Base(id) => ("GET", format!("bases/{}", enc(id)), None),
        Ask::Attack { target, stars, destroyed } => ("POST", "attacks".into(), Some(json!({ "target": target, "stars": stars, "destroyed": (*destroyed as f64).clamp(0.0, 1.0) }))),
        Ask::Seen(ids) => ("POST", "defenses/seen".into(), Some(json!({ "ids": ids }))),
        Ask::Quest { id, stars } => ("POST", format!("quests/{}", enc(id)), Some(json!({ "stars": stars }))),
        Ask::EventClaim => ("POST", "event/claim".into(), Some(json!({}))),
        Ask::Board => ("GET", "leaderboard".into(), None),
        Ask::Guilds => ("GET", "guilds".into(), None),
        Ask::Guild => ("GET", "guild".into(), None),
        Ask::Create { name, tag, open } => ("POST", "guilds".into(), Some(json!({ "name": name, "tag": tag, "open": open }))),
        Ask::Join(id) => ("POST", format!("guilds/{}/join", enc(id)), Some(json!({}))),
        Ask::Decline(id) => ("POST", format!("invites/{}/decline", enc(id)), Some(json!({}))),
        Ask::Leave => ("POST", "guild/leave".into(), Some(json!({}))),
        Ask::Invite(id) => ("POST", "guild/invites".into(), Some(json!({ "player": id }))),
        Ask::Kick(id) => ("POST", "guild/kick".into(), Some(json!({ "player": id }))),
        Ask::Open(open) => ("POST", "guild/open".into(), Some(json!({ "open": open }))),
        Ask::Donate { to, troops } => {
            let list: serde_json::Map<String, serde_json::Value> = troops.iter().map(|(t, n)| (troop_key(*t), json!(n))).collect();
            ("POST", "guild/donations".into(), Some(json!({ "to": to, "troops": list })))
        }
        Ask::TakeDonations => ("POST", "guild/donations/claim".into(), Some(json!({}))),
        Ask::Claim(claim) => ("POST", "guild/claim".into(), Some(json!({ "claim": claim }))),
    };
    let agent = crate::floor::account_agent();
    let url = format!("{}/v1/openspace/{path}", account.url);
    let auth = format!("Bearer {}", account.token);
    let res = match &body {
        None => agent.get(&url).header("User-Agent", crate::floor::USER_AGENT).header("Authorization", &auth).call(),
        Some(b) => {
            let req = if method == "POST" { agent.post(&url) } else { agent.put(&url) };
            req.header("User-Agent", crate::floor::USER_AGENT).header("Authorization", &auth).send_json(b)
        }
    };
    let mut res = res.map_err(|e| {
        crate::log::info(&format!("floor: {e}"));
        (String::new(), None)
    })?;
    let status = res.status().as_u16();
    let text = res.body_mut().read_to_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        #[derive(Deserialize)]
        struct Refusal {
            #[serde(default)]
            error: String,
            #[serde(default)]
            code: Option<String>,
        }
        if status >= 500 {
            return Err((String::new(), None));
        }
        let r: Refusal = serde_json::from_str(&text).unwrap_or(Refusal { error: status.to_string(), code: None });
        // No guild: an answer, not a failure.
        if *ask == Ask::Guild && r.code.as_deref() == Some("no-guild") {
            return Ok(Answer::Guild(None));
        }
        return Err((r.error, r.code));
    }
    let bad = |e: serde_json::Error| {
        crate::log::info(&format!("floor openspace: {e}"));
        (String::new(), None)
    };
    Ok(match ask {
        Ask::World => Answer::World(Box::new(serde_json::from_str(&text).map_err(bad)?)),
        Ask::Base(_) => Answer::Base(Box::new(serde_json::from_str(&text).map_err(bad)?)),
        Ask::Attack { .. } => Answer::Struck(serde_json::from_str(&text).map_err(bad)?),
        Ask::Quest { id, .. } => {
            #[derive(Deserialize)]
            struct Done {
                reward: Reward,
            }
            Answer::Quest { id: id.clone(), reward: serde_json::from_str::<Done>(&text).map_err(bad)?.reward }
        }
        Ask::EventClaim => {
            #[derive(Deserialize)]
            struct Won {
                tickets: u32,
            }
            Answer::Tickets(serde_json::from_str::<Won>(&text).map_err(bad)?.tickets)
        }
        Ask::Board => Answer::Board(serde_json::from_str(&text).map_err(bad)?),
        Ask::Guilds => {
            #[derive(Deserialize)]
            struct List {
                guilds: Vec<GuildRow>,
            }
            Answer::Guilds(serde_json::from_str::<List>(&text).map_err(bad)?.guilds)
        }
        Ask::Guild | Ask::Create { .. } | Ask::Join(_) | Ask::Invite(_) | Ask::Kick(_) | Ask::Open(_) | Ask::Donate { .. } => Answer::Guild(Some(Box::new(serde_json::from_str(&text).map_err(bad)?))),
        Ask::TakeDonations => {
            #[derive(Deserialize)]
            struct Took {
                troops: BTreeMap<String, u32>,
            }
            Answer::Troops(troops(&serde_json::from_str::<Took>(&text).map_err(bad)?.troops))
        }
        Ask::Claim(_) => {
            #[derive(Deserialize)]
            struct Won {
                tickets: u32,
            }
            Answer::Tickets(serde_json::from_str::<Won>(&text).map_err(bad)?.tickets)
        }
        Ask::Seen(_) | Ask::Decline(_) | Ask::Leave => Answer::Done,
    })
}

/// An event's name and what it does.
pub fn event_text(id: &str) -> ((&'static str, &'static str), (&'static str, &'static str)) {
    match id {
        "black-friday" => (("Soldes du Black Friday", "Black Friday sales"), ("Butin doublé sur les raids du monde.", "Loot doubled on the world's raids.")),
        "audit" => (("Audit surprise", "Surprise audit"), ("Un cabinet d'audit s'installe au centre de la carte : abattez-le tous ensemble pour des tickets.", "An audit firm moves in at the map's centre: bring it down all together for tickets.")),
        "hackathon" => (("Hackathon", "Hackathon"), ("Trophées gagnés ×1,5, et un ticket par raid à 3 étoiles.", "Trophies won ×1.5, and a ticket for each 3-star raid.")),
        "seminaire" => (("Séminaire d'équipe", "Team off-site"), ("Ce que tu fais compte double pour les quêtes de guilde.", "What you do counts double for the guild quests.")),
        _ => (("Événement", "Event"), ("", "")),
    }
}

/// A day's quest: its name and what it asks.
pub fn quest_text(kind: &str) -> ((&'static str, &'static str), (&'static str, &'static str)) {
    match kind {
        "convoy" => (("Convoi de café", "Coffee convoy"), ("Un concurrent fait livrer son café : intercepte le convoi.", "A rival is having its coffee delivered: intercept the convoy.")),
        "abandoned" => (("Bureau abandonné", "Abandoned office"), ("Une startup a fermé en laissant son budget : va le chercher.", "A startup closed leaving its budget behind: go and get it.")),
        "hackers" => (("Repaire de hackers", "Hackers' den"), ("Des hackers squattent un plateau : déloge-les, ils paient en tickets.", "Hackers squat a floor: drive them out, they pay in tickets.")),
        "lost" => (("Stagiaires perdus", "Lost interns"), ("Des stagiaires se sont perdus dans le quartier : va les chercher, ils rejoignent ton armée.", "Interns got lost in the district: go and get them, they join your army.")),
        _ => (("Quête", "Quest"), ("", "")),
    }
}

/// A guild quest's text (`{n}`: its target).
pub fn guild_quest_text(id: &str) -> (&'static str, &'static str) {
    match id {
        "stars" => ("Gagner {n} étoiles en raid dans le monde", "Win {n} stars raiding in the world"),
        "wins" => ("Gagner {n} raids dans le monde", "Win {n} raids in the world"),
        "raze" => ("Raser {n} bases à 100 %", "Raze {n} bases to 100 %"),
        "donate" => ("Donner {n} troupes", "Give {n} troops"),
        _ => ("{n}", "{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn troops_read_floor_names() {
        let list: BTreeMap<String, u32> = [("intern".to_owned(), 3), ("dev".to_owned(), 0), ("wizard".to_owned(), 2), ("lead".to_owned(), 1), ("devops".to_owned(), 1)].into_iter().collect();
        assert_eq!(troops(&list), vec![(Troop::Devops, 1), (Troop::Intern, 3)]);
        assert_eq!(troop_key(Troop::Devops), "devops");
    }

    /// Two players on a floor running (RONNIE_FLOOR_URL): their open spaces on the map, a raid from
    /// one on the other (trophies, loot, a shield, the raid suffered seen), a bot, a guild made, the
    /// other invited and in, troops given and taken, the ranking.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL"]
    fn the_world_on_floor() {
        let url = crate::floor::url().expect("RONNIE_FLOOR_URL");
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let agent = crate::floor::account_agent();
        let account = |name: String| {
            #[derive(Deserialize)]
            struct Made {
                token: String,
            }
            #[derive(Deserialize)]
            struct Me {
                id: String,
            }
            let made: Made = agent.post(format!("{url}/v1/accounts")).send_json(serde_json::json!({ "name": name, "password": "holy diver 83", "device": "test" })).unwrap().body_mut().read_json().unwrap();
            let me: Me = agent.get(format!("{url}/v1/players/me")).header("Authorization", format!("Bearer {}", made.token)).call().unwrap().body_mut().read_json().unwrap();
            let mut space = crate::openspace::OpenSpace::new(0, 7);
            space.coffee = 900.0;
            agent.put(format!("{url}/v1/progress")).header("Authorization", format!("Bearer {}", made.token)).send_json(serde_json::json!({ "data": { "openspace": space }, "rev": 0 })).unwrap();
            FloorAccount { url: url.clone(), id: me.id, token: made.token, name: Some(name), admin: false, avatar: None, frame: None, pending: false, title: None, titles: Default::default() }
        };
        let dio = account(format!("Dio{stamp}"));
        let ozzy = account(format!("Ozzy{stamp}"));
        let Answer::World(w) = call(&dio, &Ask::World).unwrap() else { panic!("world") };
        assert!(w.bots.len() > 10 && w.quests.len() == 3 && w.me.hq >= 1);
        let Answer::World(_) = call(&ozzy, &Ask::World).unwrap() else { panic!("world") };
        // Ozzy's base, as Ronnie reads it (his buildings); raided.
        let Answer::Base(base) = call(&dio, &Ask::Base(ozzy.id.clone())).unwrap() else { panic!("base") };
        assert!(base.buildings.as_ref().is_some_and(|b| b.iter().any(|b| b.kind == crate::openspace::Kind::Hq)));
        let Answer::Struck(st) = call(&dio, &Ask::Attack { target: ozzy.id.clone(), stars: 3, destroyed: 1.0 }).unwrap() else { panic!("attack") };
        assert!(st.trophies > 0 && st.coffee > 0.0);
        let refused = call(&dio, &Ask::Base(ozzy.id.clone())).unwrap_err();
        assert_eq!(refused.1.as_deref(), Some("shield"));
        let Answer::World(ow) = call(&ozzy, &Ask::World).unwrap() else { panic!("world") };
        assert_eq!(ow.defenses.len(), 1);
        assert!(matches!(call(&ozzy, &Ask::Seen(ow.defenses.iter().map(|d| d.id).collect())), Ok(Answer::Done)));
        // A bot.
        let bot = w.bots[0].id.clone();
        let Answer::Base(b) = call(&dio, &Ask::Base(bot.clone())).unwrap() else { panic!("bot") };
        assert!(b.level.is_some() && b.buildings.is_none());
        // A guild; Ozzy invited, in; troops given and taken.
        assert!(matches!(call(&dio, &Ask::Guild), Ok(Answer::Guild(None))));
        let Answer::Guild(Some(g)) = call(&dio, &Ask::Create { name: format!("Métal {stamp}"), tag: "MTL".into(), open: false }).unwrap() else { panic!("guild") };
        assert!(g.guild.leader && g.members.iter().any(|m| m.bot) && g.quests.len() == 3);
        call(&dio, &Ask::Invite(ozzy.id.clone())).unwrap();
        let Answer::World(ow) = call(&ozzy, &Ask::World).unwrap() else { panic!("world") };
        assert_eq!(ow.invites.len(), 1);
        call(&ozzy, &Ask::Join(g.guild.id.clone())).unwrap();
        call(&dio, &Ask::Donate { to: ozzy.id.clone(), troops: vec![(Troop::Intern, 3)] }).unwrap();
        let Answer::Troops(got) = call(&ozzy, &Ask::TakeDonations).unwrap() else { panic!("troops") };
        assert!(got.iter().any(|(t, n)| *t == Troop::Intern && *n >= 3));
        let Answer::Board(board) = call(&ozzy, &Ask::Board).unwrap() else { panic!("board") };
        assert!(board.players.iter().any(|p| p.id == dio.id) && board.guilds.iter().any(|g| g.mine));
        let Answer::Guilds(list) = call(&ozzy, &Ask::Guilds).unwrap() else { panic!("guilds") };
        assert!(list.iter().any(|row| row.id == g.guild.id));
        assert!(matches!(call(&ozzy, &Ask::Leave), Ok(Answer::Done)));
        assert!(matches!(call(&dio, &Ask::Leave), Ok(Answer::Done)));
    }

    #[test]
    fn a_world_reads() {
        let text = r#"{"now":1,"size":28,"me":{"id":"a","x":3,"y":4,"hq":3,"trophies":10,"shield_until":0,"guild":null,"name":"Dio","avatar":null},
          "players":[{"id":"a","name":"Dio","avatar":null,"frame":null,"x":3,"y":4,"hq":3,"trophies":10,"shield_until":0,"guild":null,"me":true}],
          "bots":[{"id":"b0","name":"Pixel Labs","x":1,"y":1,"hq":2,"level":7,"trophies":300,"guild":{"id":"bg0","name":"Hotfix","tag":"HFX","color":1,"bot":true},"ready_at":0}],
          "event":null,"next_event":{"id":"audit","key":3,"starts":5,"ends":9},"boss":null,
          "quests":[{"id":"1:0","kind":"lost","x":2,"y":2,"level":5,"raid":false,"ends":9,"reward":{"coffee":0,"budget":0,"tickets":0,"troops":{"intern":3}},"done":false}],
          "defenses":[],"invites":[]}"#;
        let w: World = serde_json::from_str(text).unwrap();
        assert_eq!((w.me.hq, w.bots[0].level, w.quests[0].reward.troops["intern"]), (3, 7, 3));
    }
}
