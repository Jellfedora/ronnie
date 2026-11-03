//! The chat: conversations between Ronnie's users, two of them (private) or a group, on floor (its
//! src/talk). A thread keeps a WebSocket to floor while the player has a pseudo: what is said comes in
//! as it is said, wherever Ronnie is. A connection lost comes back by itself, a little later each time.
//!
//! Text, and files: images (shown in the conversation) and documents from a short list (PDF, text,
//! Office), sent over HTTP and checked by floor (their extension and what they hold); no script, no
//! program, no archive. floor keeps them three months. Nothing secret goes through it: no password is
//! ever shared.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use tungstenite::stream::MaybeTlsStream;

use crate::config::FloorAccount;

/// A file sent: this many bytes at most.
pub const FILE_MAX: u64 = 10 * 1024 * 1024;
/// The files floor takes (their extension; it checks what they hold too).
pub const FILE_TYPES: [&str; 14] = ["png", "jpg", "jpeg", "gif", "webp", "pdf", "docx", "xlsx", "pptx", "txt", "log", "md", "csv", "json"];
/// Images shown in the conversation are made this large at most (pixels, their longest side).
const IMAGE_MAX: u32 = 1600;

/// The longest message floor keeps (characters).
pub const TEXT_MAX: usize = 2000;
/// A group's name, at most (characters).
pub const NAME_MAX: usize = 40;
/// floor is pinged this often (a connection gone silent is found).
const PING_EVERY: Duration = Duration::from_secs(30);
/// No answer for this long: the connection is taken for lost.
const SILENT: Duration = Duration::from_secs(75);
/// "Is typing" shown this long after the last word of it.
const TYPING_SHOWN: Duration = Duration::from_secs(4);
/// Told to the others at most this often while typing.
const TYPING_EVERY: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// Its avatar's version on floor (none: its initial).
    #[serde(default)]
    pub avatar: Option<String>,
}

/// A reaction to a message: an emoji, and the players who reacted so.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Reaction {
    pub emoji: String,
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Message {
    pub id: i64,
    /// Its author's id.
    pub from: String,
    /// Its author's pseudo.
    #[serde(default)]
    pub name: Option<String>,
    pub text: String,
    /// Milliseconds since 1970.
    pub at: i64,
    /// What happened in a group (`created`, `added`, `left`, `renamed`), its text the pseudos
    /// concerned or the new name.
    #[serde(default)]
    pub event: Option<String>,
    /// A file sent (an image, a document).
    #[serde(default)]
    pub file: Option<FileInfo>,
    /// The reactions to it: each emoji, with who reacted so (in order).
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    /// The message it answers, quoted over it.
    #[serde(default)]
    pub reply: Option<Quote>,
    /// When its author changed it (milliseconds since 1970).
    #[serde(default)]
    pub edited: Option<i64>,
    /// Its author took it back: empty, it stays in its place.
    #[serde(default)]
    pub deleted: bool,
    /// Pinned in its conversation, for a while.
    #[serde(default)]
    pub pinned: Option<Pin>,
}

/// A message pinned: till when (milliseconds since 1970), by whom (their id).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Pin {
    pub until: i64,
    pub by: String,
}

/// A message quoted by the one answering it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Quote {
    pub id: i64,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub text: String,
    /// Its file's name (a message of a file only).
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub deleted: bool,
}

impl Quote {
    /// A message, quoted.
    pub fn of(m: &Message) -> Self {
        Quote { id: m.id, from: Some(m.from.clone()), name: m.name.clone(), text: m.text.clone(), file: m.file.as_ref().map(|f| f.name.clone()), deleted: m.deleted }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct FileInfo {
    pub id: String,
    pub name: String,
    pub mime: String,
    /// `image` or `file`.
    pub kind: String,
    pub size: u64,
    /// floor no longer has it (kept three months).
    #[serde(default)]
    pub expired: bool,
}

impl FileInfo {
    pub fn image(&self) -> bool {
        self.kind == "image"
    }
}

/// A file of a conversation, in the list of all of them.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Media {
    #[serde(flatten)]
    pub file: FileInfo,
    pub at: i64,
    pub from: String,
    /// The message carrying it.
    pub message: i64,
}

/// What a link shows of itself (read by floor): its site, title, description, picture.
#[derive(Clone)]
pub struct Preview {
    pub site: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<Arc<egui::ColorImage>>,
}

#[derive(Deserialize)]
struct WirePreview {
    #[serde(default)]
    site: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    image: Option<String>,
}

/// A preview come from floor in the background: its link, and what it shows.
type PreviewCame = (String, Option<Preview>);

/// A link's preview: asked, or come (None: nothing to show).
pub enum PreviewLoad {
    Loading,
    Done(Option<Preview>),
}

/// A file fetched from floor: its bytes, and for an image, the picture (made smaller if it is big).
pub enum Fetched {
    Loading,
    Done { bytes: Arc<Vec<u8>>, image: Option<Arc<egui::ColorImage>> },
    Failed(String),
}

/// A file fetched in the background: its id, what came, an animated GIF's frames.
type Came = (String, Fetched, Option<crate::gif::Frames>);

/// A file being sent.
#[derive(Clone, Debug)]
pub struct Upload {
    pub room: String,
    pub name: String,
    /// Sent (None: still going; Some(Err): why not).
    pub done: Option<Result<(), String>>,
}

/// Whether floor takes this file, before sending it: its extension and its size (why not, if not).
pub fn acceptable(path: &Path) -> Result<(), FileRefusal> {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if !FILE_TYPES.contains(&ext.as_str()) {
        return Err(FileRefusal::Type);
    }
    match std::fs::metadata(path) {
        Ok(m) if m.len() == 0 => Err(FileRefusal::Empty),
        Ok(m) if m.len() > FILE_MAX => Err(FileRefusal::TooBig),
        Ok(_) => Ok(()),
        Err(_) => Err(FileRefusal::Unreadable),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FileRefusal {
    Type,
    Empty,
    TooBig,
    Unreadable,
}

/// A name safe to save a file under here: no folder, no character the system refuses.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').to_owned();
    if clean.is_empty() { "fichier".to_owned() } else { clean }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dm,
    Group,
}

#[derive(Clone, Debug, Deserialize)]
struct WireRoom {
    id: String,
    kind: Kind,
    #[serde(default)]
    name: Option<String>,
    members: Vec<Member>,
    #[serde(default)]
    messages: Option<Vec<Message>>,
    /// Its messages pinned (an older floor sends none).
    #[serde(default)]
    pins: Option<Vec<Message>>,
    /// How far each member read (an older floor sends none).
    #[serde(default)]
    reads: Option<HashMap<String, i64>>,
}

/// A conversation.
#[derive(Clone, Debug)]
pub struct Room {
    pub id: String,
    pub kind: Kind,
    /// A group's name (None: none given, its members make it).
    pub name: Option<String>,
    pub members: Vec<Member>,
    /// The messages known, the oldest first.
    pub messages: Vec<Message>,
    /// No older message on floor.
    pub done: bool,
    /// Older messages asked, not come yet.
    pub loading: bool,
    /// Who is typing in it, and when they last were.
    pub typing: Vec<(Member, Instant)>,
    /// Its messages pinned, the latest pin first (each till its time).
    pub pins: Vec<Message>,
    /// How far each member read: the last message they saw, by player id.
    pub reads: HashMap<String, i64>,
}

impl Room {
    /// The other one, in a conversation for two.
    pub fn peer(&self, me: &str) -> Option<&Member> {
        (self.kind == Kind::Dm).then(|| self.members.iter().find(|m| m.id != me)).flatten()
    }

    /// Its title: the other one's pseudo, the group's name, or its members'.
    pub fn title(&self, me: &str) -> String {
        match self.kind {
            Kind::Dm => self.peer(me).map(|m| m.name.clone()).unwrap_or_default(),
            Kind::Group => self.name.clone().unwrap_or_else(|| self.members.iter().filter(|m| m.id != me).map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")),
        }
    }

    /// The last message's id (0: none).
    pub fn last_id(&self) -> i64 {
        self.messages.last().map_or(0, |m| m.id)
    }

    /// The messages from others after `read` (the last one read).
    pub fn unread(&self, me: &str, read: i64) -> usize {
        self.messages.iter().rev().take_while(|m| m.id > read).filter(|m| m.from != me && m.event.is_none()).count()
    }

    /// Those typing now (not long ago).
    pub fn typing_now(&self) -> Vec<&str> {
        self.typing.iter().filter(|(_, at)| at.elapsed() < TYPING_SHOWN).map(|(m, _)| m.name.as_str()).collect()
    }

    fn from_wire(w: WireRoom) -> Self {
        let messages = w.messages.unwrap_or_default();
        Room { id: w.id, kind: w.kind, name: w.name, members: w.members, done: messages.len() < 60, messages, loading: false, typing: Vec::new(), pins: w.pins.unwrap_or_default(), reads: w.reads.unwrap_or_default() }
    }

    fn add(&mut self, m: Message) {
        if self.messages.last().is_none_or(|last| last.id < m.id) {
            self.typing.retain(|(who, _)| who.id != m.from);
            self.messages.push(m);
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "t")]
enum Wire {
    #[serde(rename = "welcome")]
    Welcome {
        me: Member,
        online: Vec<Member>,
        // Everyone with a pseudo, online or not (an older floor doesn't send it).
        #[serde(default)]
        players: Vec<Member>,
        rooms: Vec<WireRoom>,
    },
    #[serde(rename = "msg")]
    Msg { room: String, m: Message },
    #[serde(rename = "room")]
    Room {
        room: WireRoom,
        #[serde(default)]
        opened: bool,
    },
    #[serde(rename = "gone")]
    Gone { room: String },
    #[serde(rename = "online")]
    Online { players: Vec<Member> },
    #[serde(rename = "players")]
    Players { players: Vec<Member> },
    #[serde(rename = "typing")]
    Typing { room: String, from: Member },
    #[serde(rename = "history")]
    History { room: String, messages: Vec<Message>, done: bool },
    #[serde(rename = "reactions")]
    Reactions { room: String, message: i64, reactions: Vec<Reaction> },
    /// Someone read a conversation up to this message.
    #[serde(rename = "read")]
    Read { room: String, from: String, message: i64 },
    /// A conversation's messages pinned, the latest pin first.
    #[serde(rename = "pins")]
    Pins { room: String, pins: Vec<Message> },
    /// The messages found in a conversation, the most recent first.
    #[serde(rename = "found")]
    Found { room: String, q: String, messages: Vec<Message> },
    /// A conversation's files, the most recent first.
    #[serde(rename = "media")]
    Media { room: String, files: Vec<Media> },
    /// A message changed (edited, taken back), as it is now.
    #[serde(rename = "changed")]
    Changed { room: String, m: Message },
    #[serde(rename = "pong")]
    Pong {},
    #[serde(rename = "error")]
    Error { error: String },
}

/// From the thread.
enum Event {
    Message(Box<Wire>),
    Closed(Option<String>),
}

struct Link {
    tx: mpsc::Sender<serde_json::Value>,
    rx: mpsc::Receiver<Event>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    /// No pseudo (or no floor): no chat.
    #[default]
    Off,
    Connecting,
    Ready,
    /// Lost: tried again a little later (floor's word, when it gave one).
    Down(Option<String>),
}

/// What happened once, for the app.
#[derive(Default)]
pub struct News {
    /// Messages from others (room id, message), to tell of.
    pub arrived: Vec<(String, Message)>,
    /// A conversation the player opened (or created): to show.
    pub opened: Option<String>,
    /// Groups the player was put in by someone (room id, the message saying so).
    pub invited: Vec<(String, Message)>,
    /// Someone left a group of the player's (room id, the message saying so).
    pub departed: Vec<(String, Message)>,
    /// Someone sent the player a wizz (room id, the message saying so): the window shakes.
    pub wizzed: Vec<(String, Message)>,
    /// floor refused something (its word).
    pub refused: Option<String>,
    /// floor doesn't know the account: the pseudo is to pick again.
    pub forgotten: bool,
}

/// Kept by the app: the connection, and what floor sent.
#[derive(Default)]
pub struct Talk {
    /// The floor and the token it talks with (None: no pseudo, no chat).
    key: Option<(String, String)>,
    link: Option<Link>,
    retry_at: Option<Instant>,
    /// Falls in a row: the next try waits longer.
    falls: u32,
    last_ping: Option<Instant>,
    last_heard: Option<Instant>,
    last_typing: Option<(String, Instant)>,
    pub status: Status,
    pub me: Option<Member>,
    pub online: Vec<Member>,
    /// Everyone with a pseudo on floor, online or not: one may write to someone away.
    pub everyone: Vec<Member>,
    /// The conversations, the most recent first.
    pub rooms: Vec<Room>,
    /// The files fetched (or being), by id.
    pub files: HashMap<String, Fetched>,
    /// An animated GIF's frames, by file id, until the window makes them textures (it takes them).
    pub frames: HashMap<String, crate::gif::Frames>,
    fetched: Arc<Mutex<Vec<Came>>>,
    /// The files being sent, and those just sent or refused (until seen).
    pub uploads: Arc<Mutex<Vec<Upload>>>,
    /// Groups just come, put in by someone else: their first event (`created`, `added`) is told of.
    joining: std::collections::HashSet<String>,
    /// The last search's answer: its conversation, what was looked for, the messages found.
    pub found: Option<(String, String, Vec<Message>)>,
    /// The links' previews, by address.
    pub previews: HashMap<String, PreviewLoad>,
    previews_came: Arc<Mutex<Vec<PreviewCame>>>,
    /// How far the player said they read, by conversation (this session).
    told_read: HashMap<String, i64>,
    /// The files of the conversations asked for (`ask_media`), the most recent first.
    pub media: HashMap<String, Vec<Media>>,
    /// The names the player gave others, for themself only (by id): shown in place of their pseudos.
    nicknames: HashMap<String, String>,
    /// The pseudos so replaced, by id.
    pseudos: HashMap<String, String>,
}

/// Each name shown of someone (their id, the name): in the lists, the conversations, the messages,
/// those quoted, those typing.
fn each_name(online: &mut [Member], everyone: &mut [Member], rooms: &mut [Room], mut f: impl FnMut(&str, &mut String)) {
    for m in online.iter_mut().chain(everyone.iter_mut()) {
        f(&m.id, &mut m.name);
    }
    for r in rooms {
        for m in r.members.iter_mut().chain(r.typing.iter_mut().map(|(m, _)| m)) {
            f(&m.id, &mut m.name);
        }
        for m in &mut r.messages {
            if let Some(name) = &mut m.name {
                f(&m.from, name);
            }
            if let Some(q) = &mut m.reply
                && let (Some(from), Some(name)) = (&q.from, &mut q.name)
            {
                f(from, name);
            }
        }
    }
}

/// A text cut into its plain pieces and its links (`true`): words starting with http:// or https://,
/// without the punctuation closing a sentence after them.
pub fn links(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = ["https://", "http://"].iter().filter_map(|p| rest.find(p)).min() {
        // Inside a word: not a link (e.g. "xhttp://").
        if rest[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric()) {
            let skip = at + 1;
            out.push((&rest[..skip], false));
            rest = &rest[skip..];
            continue;
        }
        let end = rest[at..].find(char::is_whitespace).map_or(rest.len(), |e| at + e);
        let mut link = &rest[at..end];
        // Closing punctuation stays text; a ")" only when the link doesn't open one.
        while let Some(c) = link.chars().next_back() {
            let open = link.matches('(').count() > link.matches(')').count() - usize::from(c == ')');
            if ".,;:!?'\"»>]".contains(c) || (c == ')' && !open) {
                link = &link[..link.len() - c.len_utf8()];
            } else {
                break;
            }
        }
        if link.len() <= "https://".len() {
            out.push((&rest[..end], false));
            rest = &rest[end..];
            continue;
        }
        if at > 0 {
            out.push((&rest[..at], false));
        }
        out.push((link, true));
        rest = &rest[at + link.len()..];
    }
    if !rest.is_empty() {
        out.push((rest, false));
    }
    // Plain pieces next to each other, as one.
    let mut merged: Vec<(&str, bool)> = Vec::new();
    for (piece, link) in out {
        match merged.last_mut() {
            Some((last, false)) if !link && last.as_ptr() as usize + last.len() == piece.as_ptr() as usize => {
                let start = last.as_ptr() as usize - text.as_ptr() as usize;
                *last = &text[start..start + last.len() + piece.len()];
            }
            _ => merged.push((piece, link)),
        }
    }
    merged
}

/// floor's WebSocket address, from its HTTP one.
fn ws_url(base: &str) -> String {
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_owned()
    };
    format!("{base}/v1/talk/ws")
}

impl Talk {
    /// Connected with a pseudo (`on`: the chat isn't turned off), closed without; what floor sent taken.
    pub fn frame(&mut self, ctx: &egui::Context, account: Option<&FloorAccount>, on: bool) -> News {
        let key = crate::floor::url().zip(account.filter(|_| on && crate::floor::pseudo(account).is_some())).map(|(url, a)| (url, a.token.clone()));
        if self.key != key {
            // Another account, or none: what was known goes.
            *self = Talk { key: key.clone(), ..Talk::default() };
            if let Some(key) = key.clone() {
                self.open(ctx, key);
            }
        }
        let mut news = News::default();
        if key.is_none() {
            return news;
        }
        if let Some(at) = self.retry_at
            && Instant::now() >= at
            && let Some(key) = key.clone()
        {
            self.open(ctx, key);
        }
        if self.status == Status::Ready {
            if self.last_ping.is_none_or(|at| at.elapsed() >= PING_EVERY) {
                self.send(json!({ "t": "ping", "n": 0 }));
                self.last_ping = Some(Instant::now());
            }
            if self.last_heard.is_some_and(|at| at.elapsed() >= SILENT) {
                self.fell(ctx, None);
                return news;
            }
        }
        let Some(link) = &self.link else {
            // Waiting to try again: woken up then.
            if let Some(at) = self.retry_at {
                ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
            }
            return news;
        };
        let mut closed = None;
        let mut messages = Vec::new();
        loop {
            match link.rx.try_recv() {
                Ok(Event::Closed(why)) => closed = Some(why),
                Ok(Event::Message(wire)) => messages.push(*wire),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    closed.get_or_insert(None);
                    break;
                }
            }
        }
        let came = !messages.is_empty();
        for wire in messages {
            self.last_heard = Some(Instant::now());
            self.take(wire, &mut news);
        }
        if came && !self.nicknames.is_empty() {
            self.apply_nicknames();
            for (_, m) in &mut news.arrived {
                if let Some(nick) = self.nicknames.get(&m.from) {
                    m.name = Some(nick.clone());
                }
            }
        }
        if let Some(why) = closed {
            if why.as_deref() == Some("Joueur inconnu") || why.as_deref().is_some_and(|w| w.contains("pseudo")) {
                news.forgotten = true;
                *self = Talk { key: self.key.take(), ..Talk::default() };
            } else {
                self.fell(ctx, why);
            }
        }
        // "Is typing" fades by itself.
        if self.rooms.iter().any(|r| !r.typing.is_empty()) {
            ctx.request_repaint_after(Duration::from_secs(1));
            for r in &mut self.rooms {
                r.typing.retain(|(_, at)| at.elapsed() < TYPING_SHOWN);
            }
        }
        news
    }

    fn open(&mut self, ctx: &egui::Context, key: (String, String)) {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let (url, token, ctx) = (ws_url(&key.0), key.1.clone(), ctx.clone());
        std::thread::Builder::new().name("talk".into()).spawn(move || run(&url, &token, &commands, &events, &ctx)).ok();
        self.link = Some(Link { tx, rx });
        self.retry_at = None;
        self.last_ping = None;
        self.last_heard = Some(Instant::now());
        if self.status != Status::Ready {
            self.status = Status::Connecting;
        }
    }

    /// The connection lost: tried again in 2, 4, 8… seconds (15 at most).
    fn fell(&mut self, ctx: &egui::Context, why: Option<String>) {
        self.link = None;
        self.falls = (self.falls + 1).min(4);
        let wait = Duration::from_secs(1 << self.falls).min(Duration::from_secs(15));
        self.retry_at = Some(Instant::now() + wait);
        self.status = Status::Down(why);
        ctx.request_repaint_after(wait);
    }

    fn take(&mut self, wire: Wire, news: &mut News) {
        match wire {
            Wire::Welcome { me, online, players, rooms } => {
                self.status = Status::Ready;
                self.falls = 0;
                // Those known before the fall, for messages missed while away.
                let before: HashMap<String, i64> = self.rooms.iter().map(|r| (r.id.clone(), r.last_id())).collect();
                self.rooms = rooms.into_iter().map(Room::from_wire).collect();
                if self.me.is_some() {
                    for r in &self.rooms {
                        let seen = before.get(&r.id).copied().unwrap_or(0);
                        news.arrived.extend(r.messages.iter().filter(|m| m.id > seen && m.from != me.id && m.event.is_none()).map(|m| (r.id.clone(), m.clone())));
                    }
                }
                self.me = Some(me);
                self.online = online;
                self.everyone = players;
            }
            Wire::Msg { room, m } => {
                // A file: at the head of the list, if it is shown.
                if let (Some(file), Some(list)) = (&m.file, self.media.get_mut(&room)) {
                    list.insert(0, Media { file: file.clone(), at: m.at, from: m.from.clone(), message: m.id });
                }
                let mine = self.me.as_ref().is_some_and(|me| me.id == m.from);
                if !mine && m.event.is_none() {
                    news.arrived.push((room.clone(), m.clone()));
                }
                if matches!(m.event.as_deref(), Some("created" | "added")) && self.joining.remove(&room) && !mine {
                    news.invited.push((room.clone(), m.clone()));
                }
                if m.event.as_deref() == Some("wizz") && !mine {
                    news.wizzed.push((room.clone(), m.clone()));
                }
                if m.event.as_deref() == Some("left") && !mine {
                    news.departed.push((room.clone(), m.clone()));
                }
                if let Some(k) = self.rooms.iter().position(|r| r.id == room) {
                    let mut r = self.rooms.remove(k);
                    r.add(m);
                    // The most recent first.
                    self.rooms.insert(0, r);
                }
            }
            Wire::Room { room, opened } => {
                let id = room.id.clone();
                match self.rooms.iter_mut().find(|r| r.id == id) {
                    Some(r) => {
                        r.kind = room.kind;
                        r.name = room.name;
                        r.members = room.members;
                        if let Some(pins) = room.pins {
                            r.pins = pins;
                        }
                        if let Some(reads) = room.reads {
                            r.reads = reads;
                        }
                        // A pseudo changed: the messages already here take it too.
                        for m in &mut r.messages {
                            if let Some(x) = r.members.iter().find(|x| x.id == m.from) {
                                m.name = Some(x.name.clone());
                            }
                            if let Some(q) = &mut m.reply
                                && let Some(x) = r.members.iter().find(|x| Some(&x.id) == q.from.as_ref())
                            {
                                q.name = Some(x.name.clone());
                            }
                        }
                        if let Some(messages) = room.messages {
                            for m in messages {
                                r.add(m);
                            }
                        }
                    }
                    None => {
                        if room.kind == Kind::Group && !opened {
                            self.joining.insert(id.clone());
                        }
                        self.rooms.insert(0, Room::from_wire(room));
                    }
                }
                if opened {
                    news.opened = Some(id);
                }
            }
            Wire::Gone { room } => self.rooms.retain(|r| r.id != room),
            Wire::Online { players } => self.online = players,
            Wire::Players { players } => self.everyone = players,
            Wire::Typing { room, from } => {
                if let Some(r) = self.rooms.iter_mut().find(|r| r.id == room) {
                    r.typing.retain(|(m, _)| m.id != from.id);
                    r.typing.push((from, Instant::now()));
                }
            }
            Wire::History { room, messages, done } => {
                if let Some(r) = self.rooms.iter_mut().find(|r| r.id == room) {
                    let first = r.messages.first().map_or(i64::MAX, |m| m.id);
                    let mut older: Vec<Message> = messages.into_iter().filter(|m| m.id < first).collect();
                    older.append(&mut r.messages);
                    r.messages = older;
                    r.done = done;
                    r.loading = false;
                }
            }
            Wire::Reactions { room, message, reactions } => {
                if let Some(m) = self.rooms.iter_mut().find(|r| r.id == room).and_then(|r| r.messages.iter_mut().find(|m| m.id == message)) {
                    m.reactions = reactions;
                }
            }
            Wire::Read { room, from, message } => {
                if let Some(r) = self.rooms.iter_mut().find(|r| r.id == room) {
                    let seen = r.reads.entry(from).or_insert(0);
                    *seen = (*seen).max(message);
                }
            }
            Wire::Pins { room, pins } => {
                if let Some(r) = self.rooms.iter_mut().find(|r| r.id == room) {
                    // The messages here say it too.
                    for m in &mut r.messages {
                        m.pinned = pins.iter().find(|p| p.id == m.id).and_then(|p| p.pinned.clone());
                    }
                    r.pins = pins;
                }
            }
            Wire::Found { room, q, messages } => self.found = Some((room, q, messages)),
            Wire::Media { room, files } => {
                self.media.insert(room, files);
            }
            Wire::Changed { room, m } => {
                // Taken back: its file goes from the list.
                if m.deleted
                    && let Some(list) = self.media.get_mut(&room)
                {
                    list.retain(|x| x.message != m.id);
                }
                if let Some(r) = self.rooms.iter_mut().find(|r| r.id == room) {
                    // Those quoting it too.
                    for other in r.messages.iter_mut() {
                        if let Some(q) = other.reply.as_mut().filter(|q| q.id == m.id) {
                            *q = Quote::of(&m);
                        }
                    }
                    if let Some(old) = r.messages.iter_mut().find(|x| x.id == m.id) {
                        *old = m;
                    }
                }
            }
            Wire::Pong {} => {}
            Wire::Error { error } => news.refused = Some(error),
        }
    }

    fn send(&self, msg: serde_json::Value) {
        if let Some(link) = &self.link {
            let _ = link.tx.send(msg);
        }
    }

    pub fn ready(&self) -> bool {
        self.status == Status::Ready
    }

    pub fn room(&self, id: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.id == id)
    }

    pub fn is_online(&self, id: &str) -> bool {
        self.online.iter().any(|m| m.id == id)
    }

    /// A player's pseudo, from what floor sent last (everyone, those online, the conversations).
    pub fn name_of(&self, id: &str) -> Option<String> {
        let rooms = self.rooms.iter().flat_map(|r| r.members.iter());
        self.everyone.iter().chain(self.online.iter()).chain(self.me.iter()).chain(rooms).find(|m| m.id == id).map(|m| m.name.clone())
    }

    /// The names given to others (by id), for the player only: shown in place of their pseudos.
    pub fn set_nicknames(&mut self, all: &HashMap<String, String>) {
        // Back to the pseudos, then the new names.
        let pseudos = std::mem::take(&mut self.pseudos);
        each_name(&mut self.online, &mut self.everyone, &mut self.rooms, |id, name| {
            if let Some(p) = pseudos.get(id) {
                *name = p.clone();
            }
        });
        self.nicknames = all.clone();
        self.apply_nicknames();
    }

    fn apply_nicknames(&mut self) {
        let (nicknames, pseudos) = (&self.nicknames, &mut self.pseudos);
        each_name(&mut self.online, &mut self.everyone, &mut self.rooms, |id, name| {
            if let Some(nick) = nicknames.get(id)
                && name != nick
            {
                pseudos.insert(id.to_owned(), std::mem::replace(name, nick.clone()));
            }
        });
    }

    /// Someone's own pseudo, when the player gave them another name.
    pub fn pseudo_of(&self, id: &str) -> Option<&str> {
        self.nicknames.contains_key(id).then(|| self.pseudos.get(id).map(String::as_str)).flatten()
    }

    /// A player's avatar version, from what floor sent last (everyone, those online, the conversations).
    pub fn avatar_of(&self, id: &str) -> Option<String> {
        let rooms = self.rooms.iter().flat_map(|r| r.members.iter());
        self.everyone.iter().chain(self.online.iter()).chain(self.me.iter()).chain(rooms).find(|m| m.id == id).and_then(|m| m.avatar.clone())
    }

    /// Said in a conversation (floor sends it back to everyone in it, the player included).
    /// The player's reaction to a message, added (`on`) or taken back.
    pub fn react(&mut self, room: &str, message: i64, emoji: &str, on: bool) {
        self.send(json!({ "t": "react", "room": room, "message": message, "emoji": emoji, "on": on }));
    }

    /// `reply`: the message it answers, in the same conversation.
    pub fn say(&mut self, room: &str, text: &str, reply: Option<i64>) {
        self.last_typing = None;
        match reply {
            Some(id) => self.send(json!({ "t": "say", "room": room, "text": text, "reply": id })),
            None => self.send(json!({ "t": "say", "room": room, "text": text })),
        }
    }

    /// One of the player's messages, its text changed (floor sends it back to everyone: `changed`).
    pub fn edit(&mut self, room: &str, message: i64, text: &str) {
        self.send(json!({ "t": "edit", "room": room, "message": message, "text": text }));
    }

    /// A wizz to a conversation: the others' window shakes (floor sends it back as a `wizz` event).
    pub fn wizz(&mut self, room: &str) {
        self.send(json!({ "t": "wizz", "room": room }));
    }

    /// The player read a conversation up to this message: floor told (once each step).
    pub fn read(&mut self, room: &str, message: i64) {
        if message <= 0 || self.told_read.get(room).is_some_and(|&told| told >= message) || self.status != Status::Ready {
            return;
        }
        self.told_read.insert(room.to_owned(), message);
        self.send(json!({ "t": "read", "room": room, "message": message }));
    }

    /// A message pinned for so many hours (1, 3, 24, 168), or unpinned (0).
    pub fn pin(&mut self, room: &str, message: i64, hours: u32) {
        self.send(json!({ "t": "pin", "room": room, "message": message, "hours": hours }));
    }

    /// The messages of a conversation holding this text (floor sends them: `found`).
    pub fn search(&mut self, room: &str, q: &str) {
        self.send(json!({ "t": "search", "room": room, "q": q }));
    }

    /// A conversation's files, all of them (floor sends them: `media`).
    pub fn ask_media(&mut self, room: &str) {
        self.media.remove(room);
        self.send(json!({ "t": "media", "room": room }));
    }

    /// One of the player's messages, taken back.
    pub fn delete(&mut self, room: &str, message: i64) {
        self.send(json!({ "t": "delete", "room": room, "message": message }));
    }

    /// The others told the player is typing (not too often).
    pub fn typing(&mut self, room: &str) {
        if self.last_typing.as_ref().is_some_and(|(r, at)| r == room && at.elapsed() < TYPING_EVERY) {
            return;
        }
        self.last_typing = Some((room.to_owned(), Instant::now()));
        self.send(json!({ "t": "typing", "room": room }));
    }

    /// The conversation with this player (floor opens it: `News::opened`).
    pub fn dm(&self, with: &str) {
        self.send(json!({ "t": "dm", "with": with }));
    }

    /// A group with these players (floor opens it: `News::opened`).
    pub fn group(&self, name: &str, members: &[String]) {
        self.send(json!({ "t": "group", "name": name, "members": members }));
    }

    pub fn add(&self, room: &str, members: &[String]) {
        self.send(json!({ "t": "add", "room": room, "members": members }));
    }

    pub fn rename(&self, room: &str, name: &str) {
        self.send(json!({ "t": "rename", "room": room, "name": name }));
    }

    pub fn leave(&self, room: &str) {
        self.send(json!({ "t": "leave", "room": room }));
    }

    /// A file sent to a conversation, in the background (floor sends it back as a message).
    pub fn upload(&self, ctx: &egui::Context, room: &str, name: &str, bytes: Vec<u8>) {
        let Some((url, token)) = self.key.clone() else { return };
        let upload = Upload { room: room.to_owned(), name: name.to_owned(), done: None };
        if let Ok(mut list) = self.uploads.lock() {
            list.push(upload);
        }
        let (uploads, ctx, room, name) = (self.uploads.clone(), ctx.clone(), room.to_owned(), name.to_owned());
        std::thread::spawn(move || {
            let done = send_file(&url, &token, &room, &name, &bytes);
            if let Ok(mut list) = uploads.lock()
                && let Some(u) = list.iter_mut().find(|u| u.room == room && u.name == name && u.done.is_none())
            {
                u.done = Some(done);
            }
            ctx.request_repaint();
        });
    }

    /// A GIF found by the search, sent to a conversation (floor copies it): shown as being sent, like a file.
    pub fn send_gif(&self, ctx: &egui::Context, room: &str, gif: &crate::gif::Gif, query: &str) {
        let Some(key) = self.key.clone() else { return };
        let name = format!("{}.gif", if gif.title.is_empty() { "GIF" } else { gif.title.as_str() });
        if let Ok(mut list) = self.uploads.lock() {
            list.push(Upload { room: room.to_owned(), name: name.clone(), done: None });
        }
        let (uploads, ctx, room, gif, query) = (self.uploads.clone(), ctx.clone(), room.to_owned(), gif.clone(), query.to_owned());
        std::thread::spawn(move || {
            let done = crate::gif::send(&key, &room, &gif, &query);
            if let Ok(mut list) = uploads.lock()
                && let Some(u) = list.iter_mut().find(|u| u.room == room && u.name == name && u.done.is_none())
            {
                u.done = Some(done);
            }
            ctx.request_repaint();
        });
    }

    /// floor's address and the session's token, for the GIF search.
    pub fn key(&self) -> Option<crate::gif::Key> {
        self.key.clone()
    }

    /// A file of a message, fetched from floor if it isn't yet (see `files`).
    pub fn fetch(&mut self, ctx: &egui::Context, file: &FileInfo) {
        // What came in the background.
        if let Ok(mut came) = self.fetched.lock() {
            for (id, f, frames) in came.drain(..) {
                if let Some(frames) = frames {
                    self.frames.insert(id.clone(), frames);
                }
                self.files.insert(id, f);
            }
        }
        if self.files.contains_key(&file.id) || file.expired {
            return;
        }
        let Some((url, token)) = self.key.clone() else { return };
        self.files.insert(file.id.clone(), Fetched::Loading);
        let (fetched, ctx, id, image, gif) = (self.fetched.clone(), ctx.clone(), file.id.clone(), file.image(), file.mime == "image/gif");
        std::thread::spawn(move || {
            let mut frames = None;
            let got = match fetch_file(&url, &token, &id) {
                Ok(bytes) => {
                    let picture = if image { decode(&bytes).map(Arc::new) } else { None };
                    // An animated GIF: its frames too (shown at 360 px at most; 480 for a sharp screen).
                    if gif {
                        frames = crate::gif::frames(&bytes, 480, 12_000_000);
                    }
                    Fetched::Done { bytes: Arc::new(bytes), image: picture }
                }
                Err(e) => Fetched::Failed(e),
            };
            if let Ok(mut came) = fetched.lock() {
                came.push((id, got, frames));
            }
            ctx.request_repaint();
        });
    }

    /// A link's preview, asked of floor the first time (None meanwhile, and when there is nothing to show).
    pub fn preview(&mut self, ctx: &egui::Context, link: &str) -> Option<&Preview> {
        if let Ok(mut came) = self.previews_came.lock() {
            for (url, p) in came.drain(..) {
                self.previews.insert(url, PreviewLoad::Done(p));
            }
        }
        if !self.previews.contains_key(link) {
            let (url, token) = self.key.clone()?;
            self.previews.insert(link.to_owned(), PreviewLoad::Loading);
            let (came, ctx, link) = (self.previews_came.clone(), ctx.clone(), link.to_owned());
            std::thread::spawn(move || {
                let p = fetch_preview(&url, &token, &link);
                if let Ok(mut came) = came.lock() {
                    came.push((link, p));
                }
                ctx.request_repaint();
            });
        }
        match self.previews.get(link) {
            Some(PreviewLoad::Done(Some(p))) => Some(p),
            _ => None,
        }
    }

    /// Older messages of a conversation, if there are and they aren't asked already.
    pub fn more(&mut self, room: &str) {
        let Some(r) = self.rooms.iter_mut().find(|r| r.id == room) else { return };
        if r.done || r.loading || r.messages.is_empty() {
            return;
        }
        r.loading = true;
        let before = r.messages[0].id;
        self.send(json!({ "t": "more", "room": room, "before": before }));
    }
}

fn agent() -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(120))).http_status_as_error(false).build().new_agent()
}

const USER_AGENT: &str = concat!("ronnie/", env!("CARGO_PKG_VERSION"));

/// floor's word in an answer refused (its `error`), or the status.
fn refusal(mut res: ureq::http::Response<ureq::Body>) -> String {
    let status = res.status().as_u16();
    let said = res.body_mut().read_json::<serde_json::Value>().ok().and_then(|v| v["error"].as_str().map(str::to_owned));
    said.unwrap_or_else(|| format!("HTTP {status}"))
}

/// A path segment, as a URL wants it.
fn segment(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn send_file(url: &str, token: &str, room: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
    let res = agent()
        .post(format!("{url}/v1/talk/rooms/{}/files", segment(room)))
        .query("name", name)
        .header("User-Agent", USER_AGENT)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/octet-stream")
        .send(bytes)
        .map_err(|e| e.to_string())?;
    if res.status().is_success() { Ok(()) } else { Err(refusal(res)) }
}

fn fetch_file(url: &str, token: &str, id: &str) -> Result<Vec<u8>, String> {
    let mut res = agent().get(format!("{url}/v1/talk/files/{}", segment(id))).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}")).call().map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(refusal(res));
    }
    res.body_mut().with_config().limit(FILE_MAX + 1024).read_to_vec().map_err(|e| e.to_string())
}

/// A link's preview from floor, its picture with it (None: nothing to show, or floor can't say).
fn fetch_preview(url: &str, token: &str, link: &str) -> Option<Preview> {
    let mut res = agent().get(format!("{url}/v1/talk/preview")).query("url", link).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}")).call().ok()?;
    if res.status() != 200 {
        return None;
    }
    let wire: WirePreview = serde_json::from_slice(&res.body_mut().with_config().limit(64 * 1024).read_to_vec().ok()?).ok()?;
    let image = wire.image.as_deref().and_then(|path| {
        let mut res = agent().get(format!("{url}/{}", path.trim_start_matches('/'))).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}")).call().ok()?;
        if !res.status().is_success() {
            return None;
        }
        let bytes = res.body_mut().with_config().limit(3 * 1024 * 1024 + 1024).read_to_vec().ok()?;
        let image = image::load_from_memory(&bytes).ok()?.thumbnail(720, 720).to_rgba8();
        Some(Arc::new(egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw())))
    });
    Some(Preview { site: wire.site, title: wire.title, description: wire.description, image })
}

/// An image's picture, made smaller when it is big (None: not an image this can read).
fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > IMAGE_MAX { image.thumbnail(IMAGE_MAX, IMAGE_MAX) } else { image };
    let rgba = image.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// The thread: connects, says hello, then sends what the app asks and hands it what floor sends.
fn run(url: &str, token: &str, commands: &mpsc::Receiver<serde_json::Value>, events: &mpsc::Sender<Event>, ctx: &egui::Context) {
    let closed = |why: Option<String>| {
        let _ = events.send(Event::Closed(why));
        ctx.request_repaint();
    };
    let mut socket = match tungstenite::connect(url) {
        Ok((socket, _)) => socket,
        Err(e) => {
            crate::log::info(&format!("Talk: {url}: {e}"));
            return closed(None);
        }
    };
    // Short reads, to send the commands in between.
    let wait = Some(Duration::from_millis(40));
    let _ = match socket.get_mut() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(wait),
        MaybeTlsStream::Rustls(s) => s.get_mut().set_read_timeout(wait),
        _ => Ok(()),
    };
    if socket.send(tungstenite::Message::text(json!({ "t": "hello", "token": token }).to_string())).is_err() {
        return closed(None);
    }
    // floor's last word before closing (a refusal says why).
    let mut refused = None;
    loop {
        loop {
            match commands.try_recv() {
                Ok(msg) => {
                    if socket.send(tungstenite::Message::text(msg.to_string())).is_err() {
                        return closed(refused);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                // The app let go (no pseudo any more, another account).
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return;
                }
            }
        }
        match socket.read() {
            Ok(tungstenite::Message::Text(text)) => match serde_json::from_str::<Wire>(&text) {
                Ok(Wire::Error { error }) if refused.is_none() => {
                    refused = Some(error.clone());
                    let _ = events.send(Event::Message(Box::new(Wire::Error { error })));
                    ctx.request_repaint();
                }
                Ok(wire) => {
                    if events.send(Event::Message(Box::new(wire))).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
                Err(e) => crate::log::info(&format!("Talk: {e}")),
            },
            Ok(tungstenite::Message::Close(_)) => return closed(refused),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return closed(refused),
            Err(e) => {
                crate::log::info(&format!("Talk: {e}"));
                return closed(refused);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("a:b?.txt"), "a_b_.txt");
        assert_eq!(safe_name(".env"), "env");
        assert_eq!(safe_name(""), "fichier");
        assert_eq!(segment("g:1a-b"), "g%3A1a-b");
        let dir = std::env::temp_dir().join(format!("ronnie-talk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("photo.PNG");
        std::fs::write(&ok, b"x").unwrap();
        assert_eq!(acceptable(&ok), Ok(()));
        let script = dir.join("run.sh");
        std::fs::write(&script, b"x").unwrap();
        assert_eq!(acceptable(&script), Err(FileRefusal::Type));
        let empty = dir.join("vide.txt");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(acceptable(&empty), Err(FileRefusal::Empty));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn links_in_text() {
        assert_eq!(links("vu sur https://xmltvfr.fr/xmltv.php."), [("vu sur ", false), ("https://xmltvfr.fr/xmltv.php", true), (".", false)]);
        assert_eq!(links("(voir https://fr.wikipedia.org/wiki/Rust_(langage)) ok"), [("(voir ", false), ("https://fr.wikipedia.org/wiki/Rust_(langage)", true), (") ok", false)]);
        assert_eq!(links("http://a.b et https://c.d?x=1"), [("http://a.b", true), (" et ", false), ("https://c.d?x=1", true)]);
        assert_eq!(links("xhttp://a.b https://"), [("xhttp://a.b https://", false)]);
        assert_eq!(links("rien"), [("rien", false)]);
    }

    #[test]
    fn urls() {
        assert_eq!(ws_url("https://fedoheim.hopto.org/floor"), "wss://fedoheim.hopto.org/floor/v1/talk/ws");
        assert_eq!(ws_url("http://localhost:5002"), "ws://localhost:5002/v1/talk/ws");
    }

    fn msg(id: i64, from: &str, text: &str) -> Message {
        Message { id, from: from.into(), name: Some(from.into()), text: text.into(), at: 0, event: None, file: None, reactions: Vec::new(), reply: None, edited: None, deleted: false, pinned: None }
    }

    #[test]
    fn what_floor_sends() {
        let mut talk = Talk::default();
        let mut news = News::default();
        let welcome = r#"{"t":"welcome","me":{"id":"d","name":"Dio"},"online":[{"id":"d","name":"Dio"},{"id":"o","name":"Ozzy"}],
            "rooms":[{"id":"dm:d:o","kind":"dm","members":[{"id":"d","name":"Dio"},{"id":"o","name":"Ozzy"}],"messages":[{"id":1,"from":"o","name":"Ozzy","text":"Salut","at":5}]},
                     {"id":"g:1","kind":"group","name":null,"members":[{"id":"d","name":"Dio"},{"id":"o","name":"Ozzy"},{"id":"l","name":"Lemmy"}],"messages":[]}]}"#;
        talk.take(serde_json::from_str(welcome).unwrap(), &mut news);
        assert!(talk.ready());
        assert!(news.arrived.is_empty(), "nothing new at the first hello");
        assert_eq!(talk.rooms[0].title("d"), "Ozzy");
        assert_eq!(talk.rooms[1].title("d"), "Ozzy, Lemmy");
        assert_eq!(talk.rooms[0].unread("d", 0), 1);
        assert_eq!(talk.rooms[0].unread("d", 1), 0);
        assert!(talk.everyone.is_empty(), "an older floor: no list of everyone");
        talk.take(serde_json::from_str(r#"{"t":"players","players":[{"id":"d","name":"Dio"},{"id":"l","name":"Lemmy"}]}"#).unwrap(), &mut news);
        assert_eq!(talk.everyone.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Dio", "Lemmy"]);
        assert!(!talk.is_online("l"));

        // A message in the group: it comes first, it is told of; the player's own isn't.
        talk.take(serde_json::from_str(r#"{"t":"msg","room":"g:1","m":{"id":2,"from":"l","name":"Lemmy","text":"Ace","at":6}}"#).unwrap(), &mut news);
        talk.take(serde_json::from_str(r#"{"t":"msg","room":"g:1","m":{"id":3,"from":"d","name":"Dio","text":"of Spades","at":7}}"#).unwrap(), &mut news);
        assert_eq!(talk.rooms[0].id, "g:1");
        assert_eq!(news.arrived.len(), 1);
        assert_eq!(talk.rooms[0].unread("d", 0), 1);

        // A conversation's files.
        talk.take(serde_json::from_str(r#"{"t":"media","room":"g:1","files":[{"id":"f1","name":"a.png","mime":"image/png","kind":"image","size":3,"at":4,"from":"l","message":1}]}"#).unwrap(), &mut news);
        assert_eq!(talk.media["g:1"][0].file.name, "a.png");
        talk.take(serde_json::from_str(r#"{"t":"found","room":"g:1","q":"ace","messages":[{"id":2,"from":"l","name":"Lemmy","text":"Ace","at":6}]}"#).unwrap(), &mut news);
        assert!(talk.found.as_ref().is_some_and(|(r, q, m)| r == "g:1" && q == "ace" && m[0].id == 2));
        talk.take(serde_json::from_str(r#"{"t":"pins","room":"g:1","pins":[{"id":2,"from":"l","name":"Lemmy","text":"Ace","at":6,"pinned":{"until":99,"by":"d"}}]}"#).unwrap(), &mut news);
        assert_eq!(talk.rooms[0].pins.len(), 1);
        talk.take(serde_json::from_str(r#"{"t":"read","room":"g:1","from":"l","message":2}"#).unwrap(), &mut news);
        talk.take(serde_json::from_str(r#"{"t":"read","room":"g:1","from":"l","message":1}"#).unwrap(), &mut news);
        assert_eq!(talk.rooms[0].reads.get("l"), Some(&2));
        assert_eq!(talk.rooms[0].messages[0].pinned.as_ref().map(|p| p.until), Some(99));

        // An answer quoting one; that one changed, then taken back: the quote follows.
        talk.take(serde_json::from_str(r#"{"t":"msg","room":"g:1","m":{"id":4,"from":"l","name":"Lemmy","text":"Yes","at":8,"reply":{"id":3,"from":"d","name":"Dio","text":"of Spades"}}}"#).unwrap(), &mut news);
        assert_eq!(talk.rooms[0].messages[2].reply.as_ref().map(|q| q.text.as_str()), Some("of Spades"));
        talk.take(serde_json::from_str(r#"{"t":"changed","room":"g:1","m":{"id":3,"from":"d","name":"Dio","text":"of Hearts","at":7,"edited":9}}"#).unwrap(), &mut news);
        assert_eq!(talk.rooms[0].messages[1].text, "of Hearts");
        assert_eq!(talk.rooms[0].messages[1].edited, Some(9));
        assert_eq!(talk.rooms[0].messages[2].reply.as_ref().map(|q| q.text.as_str()), Some("of Hearts"));
        talk.take(serde_json::from_str(r#"{"t":"changed","room":"g:1","m":{"id":3,"from":"d","name":"Dio","text":"","at":7,"deleted":true}}"#).unwrap(), &mut news);
        assert!(talk.rooms[0].messages[1].deleted);
        assert!(talk.rooms[0].messages[2].reply.as_ref().is_some_and(|q| q.deleted));
        talk.rooms[0].messages.pop();

        // A name given to Lemmy, for oneself: everywhere, then taken back.
        talk.set_nicknames(&HashMap::from([("l".to_owned(), "Kilmister".to_owned())]));
        assert_eq!(talk.rooms[0].messages[0].name.as_deref(), Some("Kilmister"));
        assert_eq!(talk.rooms[0].title("d"), "Ozzy, Kilmister");
        assert_eq!(talk.pseudo_of("l"), Some("Lemmy"));
        talk.take(serde_json::from_str(r#"{"t":"msg","room":"g:1","m":{"id":5,"from":"l","name":"Lemmy","text":"!","at":9}}"#).unwrap(), &mut news);
        talk.apply_nicknames();
        assert_eq!(talk.rooms[0].messages[2].name.as_deref(), Some("Kilmister"));
        talk.rooms[0].messages.pop();
        talk.set_nicknames(&HashMap::new());
        assert_eq!(talk.rooms[0].title("d"), "Ozzy, Lemmy");
        assert_eq!(talk.pseudo_of("l"), None);

        // Put in a group by someone: told of once.
        talk.take(serde_json::from_str(r#"{"t":"room","room":{"id":"g:9","kind":"group","name":"Rainbow","members":[{"id":"d","name":"Dio"},{"id":"r","name":"Ritchie"}]}}"#).unwrap(), &mut news);
        talk.take(serde_json::from_str(r#"{"t":"msg","room":"g:9","m":{"id":50,"from":"r","name":"Ritchie","text":"Dio","at":9,"event":"created"}}"#).unwrap(), &mut news);
        assert_eq!(news.invited.iter().map(|(r, m)| (r.as_str(), m.name.as_deref())).collect::<Vec<_>>(), [("g:9", Some("Ritchie"))]);
        talk.take(serde_json::from_str(r#"{"t":"msg","room":"g:9","m":{"id":51,"from":"r","name":"Ritchie","text":"Cozy","at":9,"event":"added"}}"#).unwrap(), &mut news);
        assert_eq!(news.invited.len(), 1);
        talk.rooms.retain(|r| r.id != "g:9");

        // Older ones put before; the group renamed; left.
        talk.take(Wire::History { room: "g:1".into(), messages: vec![msg(1, "o", "old"), msg(2, "l", "dup")], done: true }, &mut news);
        assert_eq!(talk.rooms[0].messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["old", "Ace", ""]);
        talk.take(serde_json::from_str(r#"{"t":"room","room":{"id":"g:1","kind":"group","name":"Motörhead","members":[{"id":"d","name":"Ronnie James"}]}}"#).unwrap(), &mut news);
        assert_eq!(talk.rooms[0].title("d"), "Motörhead");
        assert_eq!(talk.rooms[0].messages[2].name.as_deref(), Some("Ronnie James"));
        assert_eq!(talk.rooms[0].messages.len(), 3);
        talk.take(serde_json::from_str(r#"{"t":"room","room":{"id":"g:2","kind":"group","members":[]},"opened":true}"#).unwrap(), &mut news);
        assert_eq!(news.opened.as_deref(), Some("g:2"));
        talk.take(Wire::Gone { room: "g:1".into() }, &mut news);
        assert!(talk.room("g:1").is_none());

        // Back after a fall: what came meanwhile is told of.
        let mut news = News::default();
        let again = r#"{"t":"welcome","me":{"id":"d","name":"Dio"},"online":[],"rooms":[{"id":"dm:d:o","kind":"dm","members":[],"messages":[{"id":1,"from":"o","text":"Salut","at":5},{"id":9,"from":"o","text":"t'es là ?","at":9}]}]}"#;
        talk.take(serde_json::from_str(again).unwrap(), &mut news);
        assert_eq!(news.arrived.iter().map(|(_, m)| m.id).collect::<Vec<_>>(), [9]);
    }

    /// Three users on a floor running (RONNIE_FLOOR_URL): online, a private conversation, a group.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL"]
    fn three_users_talk() {
        let url = crate::floor::url().expect("RONNIE_FLOOR_URL");
        let ctx = egui::Context::default();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let account = |name: String| {
            let made: serde_json::Value = ureq::post(format!("{url}/v1/players")).send_json(json!({ "name": name })).unwrap().body_mut().read_json().unwrap();
            FloorAccount { url: url.clone(), id: made["id"].as_str().unwrap().into(), token: made["token"].as_str().unwrap().into(), admin: false, avatar: None, name: Some(name) }
        };
        let accounts = [account(format!("Dio{stamp}")), account(format!("Ozzy{stamp}")), account(format!("Lemmy{stamp}"))];
        let mut talks = [Talk::default(), Talk::default(), Talk::default()];
        let mut news: [Vec<News>; 3] = Default::default();
        let until = |what: &str, talks: &mut [Talk; 3], news: &mut [Vec<News>; 3], done: &dyn Fn(&[Talk; 3]) -> bool| {
            let start = Instant::now();
            while !done(talks) {
                assert!(start.elapsed() < Duration::from_secs(10), "waiting for: {what}");
                for k in 0..3 {
                    news[k].push(talks[k].frame(&ctx, Some(&accounts[k]), true));
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        };
        until("all online", &mut talks, &mut news, &|t| t.iter().all(|t| t.ready() && t.online.len() >= 3));
        let (ozzy, lemmy) = (accounts[1].id.clone(), accounts[2].id.clone());

        // Dio writes to Ozzy: opened at Dio's, arrives at Ozzy's with the message.
        talks[0].dm(&ozzy);
        until("the conversation opened", &mut talks, &mut news, &|t| !t[0].rooms.is_empty());
        assert!(news[0].iter().any(|n| n.opened.is_some()));
        let dm = talks[0].rooms[0].id.clone();
        talks[0].say(&dm, "Holy Diver", None);
        until("the message at Ozzy's", &mut talks, &mut news, &|t| t[1].room(&dm).is_some_and(|r| r.messages.len() == 1));
        assert!(news[1].iter().any(|n| n.arrived.iter().any(|(r, m)| *r == dm && m.text == "Holy Diver")));
        assert!(!news[0].iter().any(|n| !n.arrived.is_empty()), "one's own message isn't told of");
        assert!(talks[2].rooms.is_empty(), "Lemmy sees nothing");

        // A group of three, renamed.
        talks[1].group("Heaven & Hell", &[accounts[0].id.clone(), lemmy]);
        until("the group everywhere", &mut talks, &mut news, &|t| t.iter().all(|t| t.rooms.iter().any(|r| r.kind == Kind::Group)));
        let group = talks[2].rooms.iter().find(|r| r.kind == Kind::Group).unwrap().id.clone();
        talks[2].say(&group, "Ace of Spades", None);
        until("the message in the group", &mut talks, &mut news, &|t| t[0].room(&group).is_some_and(|r| r.messages.iter().any(|m| m.text == "Ace of Spades")));
        talks[0].rename(&group, "Black Sabbath");
        until("renamed", &mut talks, &mut news, &|t| t[2].room(&group).is_some_and(|r| r.name.as_deref() == Some("Black Sabbath")));
        // An image sent to the group: it arrives as a message, its picture fetched and read.
        let png = {
            let mut out = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
            out.into_inner()
        };
        talks[1].upload(&ctx, &group, "riff.png", png);
        until("the image in the group", &mut talks, &mut news, &|t| t[2].room(&group).is_some_and(|r| r.messages.iter().any(|m| m.file.as_ref().is_some_and(|f| f.name == "riff.png"))));
        let file = talks[2].room(&group).unwrap().messages.iter().find_map(|m| m.file.clone()).unwrap();
        assert!(file.image());
        let start = Instant::now();
        loop {
            talks[2].fetch(&ctx, &file);
            if let Some(Fetched::Done { image: Some(picture), .. }) = talks[2].files.get(&file.id) {
                assert_eq!(picture.size, [40, 20]);
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "the picture");
            std::thread::sleep(Duration::from_millis(30));
        }
        // A script: refused by floor too, not only here.
        talks[1].upload(&ctx, &group, "run.sh", b"echo".to_vec());
        let start = Instant::now();
        while talks[1].uploads.lock().unwrap().iter().any(|u| u.done.is_none()) {
            assert!(start.elapsed() < Duration::from_secs(10), "the refusal");
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(matches!(&talks[1].uploads.lock().unwrap().last().unwrap().done, Some(Err(e)) if e.contains("type")));
        talks[0].leave(&group);
        until("left", &mut talks, &mut news, &|t| t[0].room(&group).is_none() && t[1].room(&group).is_some_and(|r| r.members.len() == 2));
    }
}
