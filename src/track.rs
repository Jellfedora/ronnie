//! Project tracking ("Suivi"), kept on this computer: the projects (a client each), their hour
//! counters (the client's contracts), their quotes, their planned tasks, the time spent (a duration,
//! charged to a counter, to a quote, offered, or part of the maintenance), and the files joined.
//!
//! Blocking calls: the interface makes them in a thread (`app::track`), `ronnie track` directly.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A task: 1000 h at most (as the tracking takes): a block of hours already done, charged at once.
pub const TASK_MAX: u32 = 1000 * 60;

/// What someone can do in a project, from the least.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Looks only.
    Viewer,
    /// Fills in tasks, changes their own.
    Editor,
    /// Manages everything: the project, counters, quotes, all tasks, who has access.
    Admin,
    /// Made it: an admin who can also delete it, and never loses access.
    Owner,
}

impl Role {
    pub fn manages(self) -> bool {
        self >= Role::Admin
    }

    pub fn writes(self) -> bool {
        self >= Role::Editor
    }
}

/// What a task is charged to.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Billing {
    /// Taken off an hour counter (the client's contract).
    #[default]
    Counter,
    /// Part of a quote.
    Quote,
    /// Offered.
    Free,
    /// Part of the maintenance.
    Maintenance,
}

impl Billing {
    pub const ALL: [Billing; 4] = [Billing::Counter, Billing::Quote, Billing::Free, Billing::Maintenance];
}

/// A project as listed.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub client: Option<String>,
    pub color: Option<String>,
    pub role: Role,
    pub updated: i64,
    pub archived: Option<i64>,
    /// Minutes spent, all tasks.
    pub spent: u64,
    pub members: u32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub role: Role,
    #[serde(default)]
    pub me: bool,
}

/// An hour counter: minutes bought, minutes used by the tasks charged to it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Counter {
    pub id: String,
    pub label: String,
    pub minutes: u64,
    pub day: String,
    pub closed: Option<i64>,
    pub used: u64,
}

impl Counter {
    /// Minutes left (below zero: overdrawn).
    pub fn left(&self) -> i64 {
        self.minutes as i64 - self.used as i64
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Quote {
    pub id: String,
    pub number: Option<String>,
    pub label: String,
    /// Its description (Markdown).
    #[serde(default)]
    pub note: Option<String>,
    /// The minutes planned, if said.
    pub minutes: Option<u64>,
    pub day: String,
    pub closed: Option<i64>,
    pub used: u64,
}

/// Where a planned task is (six of them; an unknown one is « to do »).
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Todo,
    Waiting,
    Doing,
    Done,
    Live,
    Cancelled,
}

impl<'de> Deserialize<'de> for Status {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let word = String::deserialize(d)?;
        Ok(Status::ALL.into_iter().find(|s| s.word() == word).unwrap_or_default())
    }
}

impl Status {
    /// In their order.
    pub const ALL: [Status; 6] = [Status::Todo, Status::Waiting, Status::Doing, Status::Done, Status::Live, Status::Cancelled];

    pub fn word(self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::Waiting => "waiting",
            Status::Doing => "doing",
            Status::Done => "done",
            Status::Live => "live",
            Status::Cancelled => "cancelled",
        }
    }

    pub fn parse(word: &str) -> Option<Status> {
        match word.to_lowercase().as_str() {
            "todo" | "à faire" | "a faire" => Some(Status::Todo),
            "waiting" | "en attente" | "attente" => Some(Status::Waiting),
            "doing" | "en cours" => Some(Status::Doing),
            "done" | "fait" | "faite" | "terminé" | "termine" | "terminée" => Some(Status::Done),
            "live" | "en production" | "production" | "prod" => Some(Status::Live),
            "cancelled" | "canceled" | "annulé" | "annule" | "annulée" => Some(Status::Cancelled),
            _ => None,
        }
    }

    /// Done with: finished, in production or cancelled (out of the tasks going on).
    pub fn closed(self) -> bool {
        matches!(self, Status::Done | Status::Live | Status::Cancelled)
    }

    /// Finished (done or in production).
    pub fn finished(self) -> bool {
        matches!(self, Status::Done | Status::Live)
    }
}

/// A planned task of the project (of a quote, or not), maybe under another one.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub quote: Option<String>,
    pub parent: Option<String>,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    /// Its estimate.
    pub minutes: Option<u64>,
    pub status: Status,
    pub position: i64,
    /// Minutes logged on it (not on its sub-tasks).
    pub spent: u64,
    /// Its dates (AAAA-MM-JJ): asked, due, finished, in production.
    #[serde(default)]
    pub requested: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub finished: Option<String>,
    #[serde(default)]
    pub live: Option<String>,
    /// Who asked for it.
    #[serde(default)]
    pub requester: Option<String>,
    /// How far it is (%), when said; else from the time spent over the estimate.
    #[serde(default)]
    pub progress: Option<u8>,
    /// A short comment.
    #[serde(default)]
    pub comment: Option<String>,
    /// Where it is: `local`, `dev`, `test` or `prod`.
    #[serde(default)]
    pub env: Option<String>,
}

/// Where a planned task can be, in their order.
pub const ENVS: [&str; 4] = ["local", "dev", "test", "prod"];

impl Item {
    /// How far it is (%): said, else finished = 100, else the time spent over the estimate (99 at most).
    pub fn progress(&self) -> Option<u8> {
        if let Some(p) = self.progress {
            return Some(p);
        }
        if self.status.finished() {
            return Some(100);
        }
        self.minutes.filter(|m| *m > 0).map(|m| ((self.spent * 100 / m).min(99)) as u8)
    }

    /// Late: due before `today`, not done with.
    pub fn late(&self, today: &str) -> bool {
        !self.status.closed() && self.due.as_deref().is_some_and(|d| d < today)
    }
}

impl Quote {
    pub fn title(&self) -> String {
        match &self.number {
            Some(n) => format!("{n} · {}", self.label),
            None => self.label.clone(),
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub day: String,
    pub label: String,
    pub minutes: u32,
    pub billing: Billing,
    pub counter: Option<String>,
    pub quote: Option<String>,
    /// The planned task it was spent on.
    #[serde(default)]
    pub item: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub mine: bool,
}

/// A project with everything in it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub client: Option<String>,
    pub color: Option<String>,
    pub note: Option<String>,
    pub role: Role,
    pub updated: i64,
    pub archived: Option<i64>,
    pub members: Vec<Member>,
    pub counters: Vec<Counter>,
    pub quotes: Vec<Quote>,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub items: Vec<Item>,
    /// The files joined to it, its counters, quotes, planned tasks, time spent.
    #[serde(default)]
    pub files: Vec<TrackFile>,
}

/// A file joined in the project (a contract, a quote's PDF, a screenshot in a description), kept beside the tracking.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct TrackFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    #[serde(rename = "type", default)]
    pub mime: String,
    /// What it is joined to: `project`, `counter:<id>`, `quote:<id>`, `item:<id>`, `task:<id>`.
    pub on: String,
    pub author: Option<String>,
    #[serde(default)]
    pub mine: bool,
}

/// Where an image joined to a description is, in its Markdown: `track-files/<id>`.
pub const FILES_PREFIX: &str = "track-files/";

/// A file at most.
pub const FILE_MAX: u64 = 25 * 1024 * 1024;

/// The media type of a file, by its name (what it opens as).
pub fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("ppt") => "application/vnd.ms-powerpoint",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("odt") => "application/vnd.oasis.opendocument.text",
        Some("ods") => "application/vnd.oasis.opendocument.spreadsheet",
        Some("txt" | "md" | "csv") => "text/plain",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

/// A size said simply: « 840 o », « 12 Ko », « 3,4 Mo ».
pub fn size_text(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} o")
    } else if bytes < 1024 * 1024 {
        format!("{} Ko", bytes.div_ceil(1024))
    } else {
        format!("{:.1} Mo", bytes as f64 / (1024.0 * 1024.0)).replace('.', ",")
    }
}

/// A planned task with the time of its sub-tasks: estimated (its own, else theirs), spent, done.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rollup {
    pub estimate: u64,
    pub spent: u64,
    /// Tasks (sub-tasks counted, not those under them) done, of how many.
    pub done: usize,
    pub count: usize,
}

impl Project {
    /// The planned tasks under `parent` (None: the first level) of `quote` (None: the project's own),
    /// in their order.
    pub fn children(&self, quote: Option<&str>, parent: Option<&str>) -> Vec<&Item> {
        let mut items: Vec<&Item> = self.items.iter().filter(|i| i.quote.as_deref() == quote && i.parent.as_deref() == parent).collect();
        items.sort_by_key(|i| i.position);
        items
    }

    /// A planned task and all under it: the estimate (its own when given, else its sub-tasks'), the
    /// time spent, how many done.
    pub fn rollup(&self, item: &Item) -> Rollup {
        let kids: Vec<Rollup> = self.items.iter().filter(|i| i.parent.as_deref() == Some(item.id.as_str())).map(|i| self.rollup(i)).collect();
        let below: u64 = kids.iter().map(|r| r.estimate).sum();
        Rollup {
            estimate: item.minutes.unwrap_or(below),
            spent: item.spent + kids.iter().map(|r| r.spent).sum::<u64>(),
            done: usize::from(item.status.finished()) + kids.iter().map(|r| r.done).sum::<usize>(),
            count: usize::from(item.status != Status::Cancelled) + kids.iter().map(|r| r.count).sum::<usize>(),
        }
    }

    /// A quote's planned tasks together (the quote's own hours when given).
    pub fn quote_rollup(&self, quote: &Quote) -> Rollup {
        let tops: Vec<Rollup> = self.children(Some(&quote.id), None).iter().map(|i| self.rollup(i)).collect();
        Rollup {
            estimate: quote.minutes.unwrap_or_else(|| tops.iter().map(|r| r.estimate).sum()),
            spent: quote.used,
            done: tops.iter().map(|r| r.done).sum(),
            count: tops.iter().map(|r| r.count).sum(),
        }
    }

    /// Who worked on a planned task: the authors of the time logged on it and on its sub-tasks, each
    /// once, by name.
    pub fn workers(&self, item: &str) -> Vec<String> {
        let mut under = vec![item.to_owned()];
        let mut k = 0;
        while k < under.len() {
            let id = under[k].clone();
            under.extend(self.items.iter().filter(|i| i.parent.as_deref() == Some(id.as_str())).map(|i| i.id.clone()));
            k += 1;
        }
        let mut names: Vec<String> = self.tasks.iter().filter(|t| t.item.as_ref().is_some_and(|i| under.contains(i))).filter_map(|t| t.author.clone()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    /// The path of a planned task: its parents' titles, then its own.
    pub fn item_path(&self, id: &str) -> String {
        let mut names = Vec::new();
        let mut at = self.items.iter().find(|i| i.id == id);
        while let Some(i) = at {
            names.insert(0, i.title.clone());
            if names.len() > 4 {
                break;
            }
            at = i.parent.as_ref().and_then(|p| self.items.iter().find(|x| &x.id == p));
        }
        names.join(" › ")
    }

    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            name: self.name.clone(),
            client: self.client.clone(),
            color: self.color.clone(),
            role: self.role,
            updated: self.updated,
            archived: self.archived,
            spent: self.tasks.iter().map(|t| t.minutes as u64).sum(),
            members: self.members.len() as u32,
        }
    }

    /// The counter new tasks go to: the oldest still open with time left, else the last open one.
    pub fn current_counter(&self) -> Option<&Counter> {
        let open = || self.counters.iter().filter(|c| c.closed.is_none());
        open().find(|c| c.left() > 0).or_else(|| open().next_back())
    }

    /// Every open counter used up (none open with time left), while there is one: time to propose
    /// a new one.
    pub fn counters_spent(&self) -> bool {
        !self.counters.is_empty() && !self.counters.iter().any(|c| c.closed.is_none() && c.left() > 0)
    }

    /// Minutes by billing, all tasks.
    pub fn by_billing(&self, billing: Billing) -> u64 {
        self.tasks.iter().filter(|t| t.billing == billing).map(|t| t.minutes as u64).sum()
    }
}

/// A task as written (to add or change one).
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct TaskInput {
    pub day: String,
    pub label: String,
    pub minutes: u32,
    pub billing: Billing,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// The planned task it was spent on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

/// A planned task as written.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct ItemInput {
    pub title: String,
    pub body: Option<String>,
    pub minutes: Option<u64>,
    pub status: Status,
    pub requested: Option<String>,
    pub due: Option<String>,
    pub finished: Option<String>,
    pub live: Option<String>,
    pub requester: Option<String>,
    pub progress: Option<u8>,
    pub comment: Option<String>,
    pub env: Option<String>,
}

/// The tracking, kept on this computer: `track.json` beside the config, the joined files in
/// `track-files/<project>/<id>` (never their own name). Every call reads it, changes it and writes it
/// back, one at a time (the interface calls from threads, `ronnie track` from another process).
#[derive(Clone)]
pub struct Client {
    dir: std::path::PathBuf,
}

/// What a call gave back, or why not (said to the user).
type Answer<T> = Result<T, String>;

/// One call at a time in this process.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A counter or a quote: 100 000 h at most.
const HOURS_MAX: u64 = 100_000 * 60;
/// Markdown texts: so many characters at most.
const BODY_MAX: usize = 20_000;
/// Planned tasks: so many levels (sub-tasks included) at most.
const DEPTH_MAX: usize = 3;
/// The joined files of a project, all of them, at most.
const FILES_QUOTA: u64 = 500 * 1024 * 1024;

#[derive(Serialize, Deserialize, Default)]
struct Db {
    #[serde(default)]
    projects: Vec<StoredProject>,
    #[serde(default)]
    counters: Vec<StoredCounter>,
    #[serde(default)]
    quotes: Vec<StoredQuote>,
    #[serde(default)]
    tasks: Vec<StoredTask>,
    #[serde(default)]
    items: Vec<StoredItem>,
    #[serde(default)]
    files: Vec<StoredFile>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredProject {
    id: String,
    name: String,
    client: Option<String>,
    color: Option<String>,
    note: Option<String>,
    created: i64,
    updated: i64,
    archived: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredCounter {
    id: String,
    project: String,
    label: String,
    minutes: u64,
    day: String,
    closed: Option<i64>,
    created: i64,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredQuote {
    id: String,
    project: String,
    number: Option<String>,
    label: String,
    note: Option<String>,
    minutes: Option<u64>,
    day: String,
    closed: Option<i64>,
    created: i64,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredTask {
    id: String,
    project: String,
    day: String,
    label: String,
    minutes: u32,
    billing: Billing,
    counter: Option<String>,
    quote: Option<String>,
    item: Option<String>,
    created: i64,
    updated: i64,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredItem {
    id: String,
    project: String,
    quote: Option<String>,
    parent: Option<String>,
    title: String,
    body: Option<String>,
    minutes: Option<u64>,
    status: Status,
    position: i64,
    created: i64,
    updated: i64,
    requested: Option<String>,
    due: Option<String>,
    finished: Option<String>,
    live: Option<String>,
    requester: Option<String>,
    progress: Option<u8>,
    comment: Option<String>,
    env: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredFile {
    id: String,
    project: String,
    /// `project`, `counter:<id>`, `quote:<id>`, `item:<id>`, `task:<id>`.
    on: String,
    name: String,
    mime: String,
    size: u64,
    created: i64,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A text cleaned (no control characters but line breaks, trimmed), `max` characters at most;
/// `required`: not empty. None: no text.
fn text(value: Option<&Value>, field: &str, max: usize, required: bool) -> Answer<Option<String>> {
    let clean = match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.chars().filter(|c| *c == '\n' || !c.is_control()).collect::<String>().trim().to_owned(),
        Some(_) => return Err(format!("{field}: a text")),
    };
    if clean.is_empty() {
        return if required { Err(format!("{field}: required")) } else { Ok(None) };
    }
    if clean.chars().count() > max {
        return Err(format!("{field}: {max} characters at most"));
    }
    Ok(Some(clean))
}

fn required(value: Option<&Value>, field: &str, max: usize) -> Answer<String> {
    Ok(text(value, field, max, true)?.unwrap_or_default())
}

/// Minutes between 1 and `max` (None: none given, when not `needed`).
fn minutes(value: Option<&Value>, field: &str, max: u64, needed: bool) -> Answer<Option<u64>> {
    match value {
        None | Some(Value::Null) if !needed => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) if (1..=max).contains(&n) => Ok(Some(n)),
            _ => Err(format!("{field}: a number of minutes between 1 and {max}")),
        },
        None => Err(format!("{field}: required")),
    }
}

fn valid_day(day: &str) -> bool {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok() && day.len() == 10
}

fn day(value: Option<&Value>, field: &str) -> Answer<String> {
    match value.and_then(Value::as_str) {
        Some(d) if valid_day(d) => Ok(d.to_owned()),
        _ => Err(format!("{field}: a date YYYY-MM-DD")),
    }
}

/// An optional date: null (or none) clears it.
fn day_or_none(value: Option<&Value>, field: &str) -> Answer<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        v => day(v, field).map(Some),
    }
}

fn color(value: Option<&Value>) -> Answer<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) if s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit()) => Ok(Some(s.to_lowercase())),
        _ => Err("color: #rrggbb".into()),
    }
}

fn percent(value: Option<&Value>, field: &str) -> Answer<Option<u8>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().filter(|n| *n <= 100).map(|n| Some(n as u8)).ok_or_else(|| format!("{field}: a whole number between 0 and 100")),
    }
}

fn env_of(value: Option<&Value>) -> Answer<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) if ENVS.contains(&s.as_str()) => Ok(Some(s.clone())),
        _ => Err(format!("env: {}", ENVS.join(", "))),
    }
}

fn status_of(value: Option<&Value>) -> Answer<Status> {
    value.and_then(Value::as_str).and_then(|w| Status::ALL.into_iter().find(|s| s.word() == w)).ok_or_else(|| format!("status: {}", Status::ALL.map(Status::word).join(", ")))
}

/// The dates a state fills in by itself (never over those already there): done → finished; in
/// production → live, and finished if missing.
fn dates_of(status: Status, finished: &mut Option<String>, live: &mut Option<String>, today: &str) {
    if status.finished() && finished.is_none() {
        *finished = Some(today.to_owned());
    }
    if status == Status::Live && live.is_none() {
        *live = Some(today.to_owned());
    }
}

/// A safe image, by its content (PNG, JPEG, GIF, WebP): its type.
fn image_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

/// A file's name kept: no folder, no control or reserved character, not hidden, 120 characters at
/// most (its extension kept). None: nothing left.
fn clean_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let base: String = base.chars().filter(|c| !c.is_control() && !"\"<>|:*?".contains(*c)).collect();
    let base = base.split_whitespace().collect::<Vec<_>>().join(" ");
    if base.is_empty() || base.starts_with('.') {
        return None;
    }
    if base.chars().count() <= 120 {
        return Some(base);
    }
    let ext = std::path::Path::new(&base).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    Some(base.chars().take(120 - ext.chars().count()).collect::<String>() + &ext)
}

/// Only letters, digits and dashes: an id safe in a path.
fn safe_id(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
}

impl Db {
    fn project(&self, id: &str) -> Answer<&StoredProject> {
        self.projects.iter().find(|p| p.id == id).ok_or_else(|| "Project not found".to_owned())
    }

    fn project_mut(&mut self, id: &str) -> Answer<&mut StoredProject> {
        self.projects.iter_mut().find(|p| p.id == id).ok_or_else(|| "Project not found".to_owned())
    }

    fn touch(&mut self, id: &str) {
        if let Ok(p) = self.project_mut(id) {
            p.updated = now_ms();
        }
    }

    fn counter_of(&self, project: &str, id: &str) -> bool {
        self.counters.iter().any(|c| c.id == id && c.project == project)
    }

    fn quote_of(&self, project: &str, id: &str) -> bool {
        self.quotes.iter().any(|q| q.id == id && q.project == project)
    }

    fn item_of(&self, project: &str, id: &str) -> Option<&StoredItem> {
        self.items.iter().find(|i| i.id == id && i.project == project)
    }

    /// What a task is charged to: the counter or the quote, of the same project.
    fn billing_of(&self, project: &str, b: &Value, old: Option<&StoredTask>) -> Answer<(Billing, Option<String>, Option<String>)> {
        let billing = match b.get("billing") {
            Some(v) => serde_json::from_value::<Billing>(v.clone()).map_err(|_| "billing: counter, quote, free, maintenance".to_owned())?,
            None => old.map(|t| t.billing).ok_or("billing: counter, quote, free, maintenance")?,
        };
        let pick = |key: &str, before: Option<&String>| match b.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => None,
            None => before.cloned(),
        };
        match billing {
            Billing::Counter => {
                let c = pick("counter", old.and_then(|t| t.counter.as_ref())).filter(|c| self.counter_of(project, c)).ok_or("counter: a counter of this project")?;
                Ok((billing, Some(c), None))
            }
            Billing::Quote => {
                let q = pick("quote", old.and_then(|t| t.quote.as_ref())).filter(|q| self.quote_of(project, q)).ok_or("quote: a quote of this project")?;
                Ok((billing, None, Some(q)))
            }
            _ => Ok((billing, None, None)),
        }
    }

    /// The planned task some time is spent on: of the same project (None: none).
    fn task_item(&self, project: &str, value: Option<&Value>) -> Answer<Option<String>> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(id)) if self.item_of(project, id).is_some() => Ok(Some(id.clone())),
            _ => Err("item: a planned task of this project".into()),
        }
    }

    /// A planned task's level (0: the first); None: `avoid` is above it (a task never goes under itself).
    fn depth_of(&self, project: &str, id: &str, avoid: Option<&str>) -> Option<usize> {
        let mut depth = 0;
        let mut at = Some(id.to_owned());
        while let Some(cur) = at {
            if Some(cur.as_str()) == avoid || depth > DEPTH_MAX + 1 {
                return None;
            }
            at = self.item_of(project, &cur).and_then(|i| i.parent.clone());
            depth += 1;
        }
        Some(depth - 1)
    }

    fn parent_of(&self, project: &str, parent: Option<&Value>, quote: Option<&str>, me: Option<&str>) -> Answer<Option<String>> {
        let id = match parent {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::String(id)) => id,
            Some(_) => return Err("parent: a task of this project".into()),
        };
        let p = self.item_of(project, id).ok_or("parent: a task of this project")?;
        if p.quote.as_deref() != quote {
            return Err("parent: a task of the same quote".into());
        }
        let depth = self.depth_of(project, id, me).ok_or("parent: not under itself")?;
        if depth + 1 >= DEPTH_MAX {
            return Err(format!("{DEPTH_MAX} levels of tasks at most"));
        }
        Ok(Some(id.clone()))
    }

    fn last_position(&self, project: &str, quote: Option<&str>, parent: Option<&str>) -> i64 {
        self.items.iter().filter(|i| i.project == project && i.quote.as_deref() == quote && i.parent.as_deref() == parent).map(|i| i.position).max().unwrap_or(-1)
    }

    /// What a file is joined to: the project, or one of its things (that must be there).
    fn target_of(&self, project: &str, on: &str) -> Answer<String> {
        if on.is_empty() || on == "project" {
            return Ok("project".into());
        }
        let ok = match on.split_once(':') {
            Some(("counter", id)) => self.counter_of(project, id),
            Some(("quote", id)) => self.quote_of(project, id),
            Some(("item", id)) => self.item_of(project, id).is_some(),
            Some(("task", id)) => self.tasks.iter().any(|t| t.id == id && t.project == project),
            _ => false,
        };
        if ok { Ok(on.to_owned()) } else { Err("on: project, or counter:, quote:, item:, task: and the id of one of this project's".into()) }
    }

    /// The planned tasks gone with what they were in (a quote, a task above), the time spent on them
    /// kept; and the files of what's gone (returned, to take off the disk).
    fn sweep(&mut self, project: &str) -> Vec<String> {
        loop {
            let gone: Vec<String> = self
                .items
                .iter()
                .filter(|i| i.project == project)
                .filter(|i| i.quote.as_ref().is_some_and(|q| !self.quote_of(project, q)) || i.parent.as_ref().is_some_and(|p| self.item_of(project, p).is_none()))
                .map(|i| i.id.clone())
                .collect();
            if gone.is_empty() {
                break;
            }
            self.items.retain(|i| !gone.contains(&i.id));
        }
        let items: Vec<String> = self.items.iter().map(|i| i.id.clone()).collect();
        for t in self.tasks.iter_mut().filter(|t| t.project == project) {
            if t.item.as_ref().is_some_and(|i| !items.contains(i)) {
                t.item = None;
            }
        }
        let orphans: Vec<String> = self.files.iter().filter(|f| f.project == project && self.target_of(project, &f.on).is_err()).map(|f| f.id.clone()).collect();
        self.files.retain(|f| !orphans.contains(&f.id));
        orphans
    }

    fn view(&self, id: &str) -> Answer<Project> {
        let p = self.project(id)?;
        let used = |pick: &dyn Fn(&StoredTask) -> bool| self.tasks.iter().filter(|t| t.project == p.id && pick(t)).map(|t| t.minutes as u64).sum::<u64>();
        let mut counters: Vec<&StoredCounter> = self.counters.iter().filter(|c| c.project == p.id).collect();
        counters.sort_by(|a, b| (&a.day, a.created).cmp(&(&b.day, b.created)));
        let mut quotes: Vec<&StoredQuote> = self.quotes.iter().filter(|q| q.project == p.id).collect();
        quotes.sort_by(|a, b| (&a.day, a.created).cmp(&(&b.day, b.created)));
        let mut items: Vec<&StoredItem> = self.items.iter().filter(|i| i.project == p.id).collect();
        items.sort_by_key(|i| (i.position, i.created));
        let mut tasks: Vec<&StoredTask> = self.tasks.iter().filter(|t| t.project == p.id).collect();
        tasks.sort_by(|a, b| (&b.day, b.created).cmp(&(&a.day, a.created)));
        let mut files: Vec<&StoredFile> = self.files.iter().filter(|f| f.project == p.id).collect();
        files.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
        Ok(Project {
            id: p.id.clone(),
            name: p.name.clone(),
            client: p.client.clone(),
            color: p.color.clone(),
            note: p.note.clone(),
            role: Role::Owner,
            updated: p.updated,
            archived: p.archived,
            members: Vec::new(),
            counters: counters.into_iter().map(|c| Counter { id: c.id.clone(), label: c.label.clone(), minutes: c.minutes, day: c.day.clone(), closed: c.closed, used: used(&|t| t.counter.as_deref() == Some(c.id.as_str())) }).collect(),
            quotes: quotes
                .into_iter()
                .map(|q| Quote { id: q.id.clone(), number: q.number.clone(), label: q.label.clone(), note: q.note.clone(), minutes: q.minutes, day: q.day.clone(), closed: q.closed, used: used(&|t| t.quote.as_deref() == Some(q.id.as_str())) })
                .collect(),
            tasks: tasks
                .into_iter()
                .map(|t| Task { id: t.id.clone(), day: t.day.clone(), label: t.label.clone(), minutes: t.minutes, billing: t.billing, counter: t.counter.clone(), quote: t.quote.clone(), item: t.item.clone(), author: None, mine: true })
                .collect(),
            items: items
                .into_iter()
                .map(|i| Item {
                    id: i.id.clone(),
                    quote: i.quote.clone(),
                    parent: i.parent.clone(),
                    title: i.title.clone(),
                    body: i.body.clone(),
                    minutes: i.minutes,
                    status: i.status,
                    position: i.position,
                    spent: used(&|t| t.item.as_deref() == Some(i.id.as_str())),
                    requested: i.requested.clone(),
                    due: i.due.clone(),
                    finished: i.finished.clone(),
                    live: i.live.clone(),
                    requester: i.requester.clone(),
                    progress: i.progress,
                    comment: i.comment.clone(),
                    env: i.env.clone(),
                })
                .collect(),
            files: files.into_iter().map(|f| TrackFile { id: f.id.clone(), name: f.name.clone(), size: f.size, mime: f.mime.clone(), on: f.on.clone(), author: None, mine: true }).collect(),
        })
    }
}

impl Client {
    /// The tracking of this computer (in Ronnie's config folder).
    pub fn local() -> Answer<Client> {
        crate::config::config_dir().map(|dir| Client { dir }).ok_or_else(|| "no config folder".to_owned())
    }

    #[cfg(test)]
    fn at(dir: &std::path::Path) -> Client {
        Client { dir: dir.to_owned() }
    }

    fn path(&self) -> std::path::PathBuf {
        self.dir.join("track.json")
    }

    fn file_path(&self, project: &str, file: &str) -> std::path::PathBuf {
        self.dir.join("track-files").join(safe_id(project)).join(safe_id(file))
    }

    /// `change` made to the tracking, written back (only when it went well); what it gave.
    fn with<T>(&self, change: impl FnOnce(&mut Db) -> Answer<T>) -> Answer<T> {
        let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut db: Db = match std::fs::read_to_string(self.path()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("track.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Db::default(),
            Err(e) => return Err(format!("track.json: {e}")),
        };
        let out = change(&mut db)?;
        crate::config::save(&self.path(), &db).map_err(|e| format!("{e:#}"))?;
        Ok(out)
    }

    fn read<T>(&self, look: impl FnOnce(&Db) -> Answer<T>) -> Answer<T> {
        let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let db: Db = match std::fs::read_to_string(self.path()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("track.json: {e}"))?,
            Err(_) => Db::default(),
        };
        look(&db)
    }

    /// `change` made to project `id`, its last change noted; the project as it is then.
    fn change(&self, id: &str, change: impl FnOnce(&mut Db) -> Answer<()>) -> Answer<Project> {
        let mut gone = Vec::new();
        let project = self.with(|db| {
            db.project(id)?;
            change(db)?;
            gone = db.sweep(id);
            db.touch(id);
            db.view(id)
        })?;
        for f in gone {
            let _ = std::fs::remove_file(self.file_path(id, &f));
        }
        Ok(project)
    }

    pub fn projects(&self) -> Answer<Vec<Summary>> {
        self.read(|db| {
            let mut list: Vec<Summary> = db.projects.iter().filter_map(|p| db.view(&p.id).ok()).map(|p| p.summary()).collect();
            list.sort_by_key(|s| (s.client.as_deref().unwrap_or_default().to_lowercase(), s.name.to_lowercase()));
            Ok(list)
        })
    }

    pub fn project(&self, id: &str) -> Answer<Project> {
        self.read(|db| db.view(id))
    }

    pub fn create(&self, name: &str, client: Option<&str>, color: Option<&str>) -> Answer<Project> {
        let b = serde_json::json!({ "name": name, "client": client, "color": color });
        self.with(|db| {
            let at = now_ms();
            let id = new_id();
            db.projects.push(StoredProject { id: id.clone(), name: required(b.get("name"), "name", 80)?, client: text(b.get("client"), "client", 80, false)?, color: self::color(b.get("color"))?, note: None, created: at, updated: at, archived: None });
            db.view(&id)
        })
    }

    /// The project's fields given in `fields` (name, client, color, note, archived) changed.
    pub fn update(&self, id: &str, fields: Value) -> Answer<Project> {
        self.change(id, |db| {
            let name = fields.get("name").map(|v| required(Some(v), "name", 80)).transpose()?;
            let client = fields.get("client").map(|v| text(Some(v), "client", 80, false)).transpose()?;
            let color = fields.get("color").map(|v| self::color(Some(v))).transpose()?;
            let note = fields.get("note").map(|v| text(Some(v), "note", BODY_MAX, false)).transpose()?;
            let p = db.project_mut(id)?;
            if let Some(n) = name {
                p.name = n;
            }
            if let Some(c) = client {
                p.client = c;
            }
            if let Some(c) = color {
                p.color = c;
            }
            if let Some(n) = note {
                p.note = n;
            }
            if let Some(a) = fields.get("archived") {
                p.archived = if a.as_bool().unwrap_or(false) { p.archived.or(Some(now_ms())) } else { None };
            }
            Ok(())
        })
    }

    pub fn delete(&self, id: &str) -> Answer<()> {
        self.with(|db| {
            db.project(id)?;
            db.projects.retain(|p| p.id != id);
            db.counters.retain(|c| c.project != id);
            db.quotes.retain(|q| q.project != id);
            db.tasks.retain(|t| t.project != id);
            db.items.retain(|i| i.project != id);
            db.files.retain(|f| f.project != id);
            Ok(())
        })?;
        let _ = std::fs::remove_dir_all(self.dir.join("track-files").join(safe_id(id)));
        Ok(())
    }

    pub fn add_counter(&self, id: &str, label: &str, minutes: u64, day: &str) -> Answer<Project> {
        let b = serde_json::json!({ "label": label, "minutes": minutes, "day": day });
        self.change(id, |db| {
            let c = StoredCounter { id: new_id(), project: id.to_owned(), label: required(b.get("label"), "label", 120)?, minutes: self::minutes(b.get("minutes"), "minutes", HOURS_MAX, true)?.unwrap_or_default(), day: self::day(b.get("day"), "day")?, closed: None, created: now_ms() };
            db.counters.push(c);
            Ok(())
        })
    }

    pub fn update_counter(&self, id: &str, counter: &str, fields: Value) -> Answer<Project> {
        self.change(id, |db| {
            let label = fields.get("label").map(|v| required(Some(v), "label", 120)).transpose()?;
            let minutes = fields.get("minutes").map(|v| self::minutes(Some(v), "minutes", HOURS_MAX, true)).transpose()?.flatten();
            let day = fields.get("day").map(|v| self::day(Some(v), "day")).transpose()?;
            let c = db.counters.iter_mut().find(|c| c.id == counter && c.project == id).ok_or("Counter not found")?;
            if let Some(l) = label {
                c.label = l;
            }
            if let Some(m) = minutes {
                c.minutes = m;
            }
            if let Some(d) = day {
                c.day = d;
            }
            if let Some(closed) = fields.get("closed") {
                c.closed = if closed.as_bool().unwrap_or(false) { c.closed.or(Some(now_ms())) } else { None };
            }
            Ok(())
        })
    }

    pub fn delete_counter(&self, id: &str, counter: &str) -> Answer<Project> {
        self.change(id, |db| {
            if !db.counter_of(id, counter) {
                return Err("Counter not found".into());
            }
            if db.tasks.iter().any(|t| t.counter.as_deref() == Some(counter)) {
                return Err("Tasks are taken off this counter: charge them elsewhere first".into());
            }
            db.counters.retain(|c| c.id != counter);
            Ok(())
        })
    }

    pub fn add_quote(&self, id: &str, number: Option<&str>, label: &str, minutes: Option<u64>, day: &str) -> Answer<Project> {
        self.add_quote_full(id, number, label, minutes, day, None)
    }

    pub fn add_quote_full(&self, id: &str, number: Option<&str>, label: &str, minutes: Option<u64>, day: &str, note: Option<&str>) -> Answer<Project> {
        let b = serde_json::json!({ "number": number, "label": label, "minutes": minutes, "day": day, "note": note });
        self.change(id, |db| {
            let q = StoredQuote {
                id: new_id(),
                project: id.to_owned(),
                number: text(b.get("number"), "number", 40, false)?,
                label: required(b.get("label"), "label", 120)?,
                note: text(b.get("note"), "note", BODY_MAX, false)?,
                minutes: self::minutes(b.get("minutes"), "minutes", HOURS_MAX, false)?,
                day: self::day(b.get("day"), "day")?,
                closed: None,
                created: now_ms(),
            };
            db.quotes.push(q);
            Ok(())
        })
    }

    pub fn update_quote(&self, id: &str, quote: &str, fields: Value) -> Answer<Project> {
        self.change(id, |db| {
            let number = fields.get("number").map(|v| text(Some(v), "number", 40, false)).transpose()?;
            let label = fields.get("label").map(|v| required(Some(v), "label", 120)).transpose()?;
            let minutes = fields.get("minutes").map(|v| self::minutes(Some(v), "minutes", HOURS_MAX, false)).transpose()?;
            let day = fields.get("day").map(|v| self::day(Some(v), "day")).transpose()?;
            let note = fields.get("note").map(|v| text(Some(v), "note", BODY_MAX, false)).transpose()?;
            let q = db.quotes.iter_mut().find(|q| q.id == quote && q.project == id).ok_or("Quote not found")?;
            if let Some(n) = number {
                q.number = n;
            }
            if let Some(l) = label {
                q.label = l;
            }
            if let Some(m) = minutes {
                q.minutes = m;
            }
            if let Some(d) = day {
                q.day = d;
            }
            if let Some(n) = note {
                q.note = n;
            }
            if let Some(closed) = fields.get("closed") {
                q.closed = if closed.as_bool().unwrap_or(false) { q.closed.or(Some(now_ms())) } else { None };
            }
            Ok(())
        })
    }

    pub fn delete_quote(&self, id: &str, quote: &str) -> Answer<Project> {
        self.change(id, |db| {
            if !db.quote_of(id, quote) {
                return Err("Quote not found".into());
            }
            if db.tasks.iter().any(|t| t.quote.as_deref() == Some(quote)) {
                return Err("Tasks are charged to this quote: charge them elsewhere first".into());
            }
            db.quotes.retain(|q| q.id != quote);
            Ok(())
        })
    }

    pub fn add_task(&self, id: &str, task: &TaskInput) -> Answer<Project> {
        let b = serde_json::to_value(task).map_err(|e| e.to_string())?;
        self.change(id, |db| {
            let (billing, counter, quote) = db.billing_of(id, &b, None)?;
            let item = db.task_item(id, b.get("item"))?;
            let at = now_ms();
            let t = StoredTask {
                id: new_id(),
                project: id.to_owned(),
                day: self::day(b.get("day"), "day")?,
                label: required(b.get("label"), "label", BODY_MAX)?,
                minutes: self::minutes(b.get("minutes"), "minutes", TASK_MAX as u64, true)?.unwrap_or_default() as u32,
                billing,
                counter,
                quote,
                item,
                created: at,
                updated: at,
            };
            db.tasks.push(t);
            Ok(())
        })
    }

    pub fn update_task(&self, id: &str, task: &str, input: &TaskInput) -> Answer<Project> {
        let mut b = serde_json::to_value(input).map_err(|e| e.to_string())?;
        // Charged elsewhere: the old counter or quote forgotten.
        b["counter"] = input.counter.clone().map_or(Value::Null, Value::String);
        b["quote"] = input.quote.clone().map_or(Value::Null, Value::String);
        b["item"] = input.item.clone().map_or(Value::Null, Value::String);
        self.change(id, |db| {
            let old = db.tasks.iter().find(|t| t.id == task && t.project == id).cloned().ok_or("Task not found")?;
            let (billing, counter, quote) = db.billing_of(id, &b, Some(&old))?;
            let item = db.task_item(id, b.get("item"))?;
            let day = self::day(b.get("day"), "day")?;
            let label = required(b.get("label"), "label", BODY_MAX)?;
            let minutes = self::minutes(b.get("minutes"), "minutes", TASK_MAX as u64, true)?.unwrap_or_default() as u32;
            let t = db.tasks.iter_mut().find(|t| t.id == task).ok_or("Task not found")?;
            (t.day, t.label, t.minutes, t.billing, t.counter, t.quote, t.item, t.updated) = (day, label, minutes, billing, counter, quote, item, now_ms());
            Ok(())
        })
    }

    pub fn delete_task(&self, id: &str, task: &str) -> Answer<Project> {
        self.change(id, |db| {
            if !db.tasks.iter().any(|t| t.id == task && t.project == id) {
                return Err("Task not found".into());
            }
            db.tasks.retain(|t| t.id != task);
            Ok(())
        })
    }

    /// A file joined to `on` (`project`, `task:<id>`...), under the id given (Ronnie picks it: an
    /// image's Markdown is written before it's kept).
    pub fn upload(&self, id: &str, on: &str, file: &str, name: &str, bytes: &[u8]) -> Answer<()> {
        let name = clean_name(name).ok_or("the file's name")?;
        if uuid::Uuid::parse_str(file).is_err() {
            return Err("the file's id".into());
        }
        if bytes.is_empty() {
            return Err("Empty file".into());
        }
        if bytes.len() as u64 > FILE_MAX {
            return Err(format!("File too big ({} MB at most)", FILE_MAX / 1024 / 1024));
        }
        // An image by its content; an « image » that isn't one isn't shown.
        let mime = match image_of(bytes) {
            Some(image) => image.to_owned(),
            None if mime_of(&name).starts_with("image/") => "application/octet-stream".to_owned(),
            None => mime_of(&name).to_owned(),
        };
        let path = self.file_path(id, file);
        self.with(|db| {
            db.project(id)?;
            let on = db.target_of(id, on)?;
            if db.files.iter().any(|f| f.id == file) {
                return Err("This file is already there".into());
            }
            let used: u64 = db.files.iter().filter(|f| f.project == id).map(|f| f.size).sum();
            if used + bytes.len() as u64 > FILES_QUOTA {
                return Err("No more room for this project's files".into());
            }
            if let Some(dir) = path.parent() {
                crate::config::create_private_dir(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            db.files.push(StoredFile { id: file.to_owned(), project: id.to_owned(), on, name, mime, size: bytes.len() as u64, created: now_ms() });
            db.touch(id);
            Ok(())
        })
    }

    /// A file joined elsewhere in the project (an image pasted before what it's in was made).
    pub fn move_file(&self, id: &str, file: &str, on: &str) -> Answer<()> {
        self.with(|db| {
            let on = db.target_of(id, on)?;
            let f = db.files.iter_mut().find(|f| f.id == file && f.project == id).ok_or("File not found")?;
            f.on = on;
            db.touch(id);
            Ok(())
        })
    }

    pub fn delete_file(&self, id: &str, file: &str) -> Answer<()> {
        self.with(|db| {
            if !db.files.iter().any(|f| f.id == file && f.project == id) {
                return Err("File not found".into());
            }
            db.files.retain(|f| f.id != file);
            db.touch(id);
            Ok(())
        })?;
        let _ = std::fs::remove_file(self.file_path(id, file));
        Ok(())
    }

    /// A file's bytes.
    pub fn download(&self, id: &str, file: &str) -> Answer<Vec<u8>> {
        self.read(|db| if db.files.iter().any(|f| f.id == file && f.project == id) { Ok(()) } else { Err("File not found".into()) })?;
        std::fs::read(self.file_path(id, file)).map_err(|e| e.to_string())
    }

    /// A planned task, of `quote` (None: the project's), under `parent`.
    pub fn add_item(&self, id: &str, quote: Option<&str>, parent: Option<&str>, item: &ItemInput) -> Answer<Project> {
        let b = serde_json::to_value(item).map_err(|e| e.to_string())?;
        self.change(id, |db| {
            if let Some(q) = quote
                && !db.quote_of(id, q)
            {
                return Err("quote: a quote of this project".into());
            }
            let parent = db.parent_of(id, parent.map(|p| Value::String(p.to_owned())).as_ref(), quote, None)?;
            let at = now_ms();
            let today = today();
            let status = match b.get("status") {
                None | Some(Value::Null) => Status::Todo,
                v => status_of(v)?,
            };
            let (mut finished, mut live) = (day_or_none(b.get("finished"), "finished")?, day_or_none(b.get("live"), "live")?);
            dates_of(status, &mut finished, &mut live, &today);
            let env = env_of(b.get("env"))?.or_else(|| (status == Status::Live).then(|| "prod".to_owned()));
            let position = db.last_position(id, quote, parent.as_deref()) + 1;
            let i = StoredItem {
                id: new_id(),
                project: id.to_owned(),
                quote: quote.map(str::to_owned),
                parent,
                title: required(b.get("title"), "title", 200)?,
                body: text(b.get("body"), "body", BODY_MAX, false)?,
                minutes: self::minutes(b.get("minutes"), "minutes", HOURS_MAX, false)?,
                status,
                position,
                created: at,
                updated: at,
                requested: day_or_none(b.get("requested"), "requested")?.or(Some(today)),
                due: day_or_none(b.get("due"), "due")?,
                finished,
                live,
                requester: text(b.get("requester"), "requester", 80, false)?,
                progress: percent(b.get("progress"), "progress")?,
                comment: text(b.get("comment"), "comment", 500, false)?,
                env,
            };
            db.items.push(i);
            Ok(())
        })
    }

    /// A planned task's fields given in `fields` (title, body, minutes, status, parent, position…).
    pub fn update_item(&self, id: &str, item: &str, fields: Value) -> Answer<Project> {
        self.change(id, |db| {
            let old = db.item_of(id, item).cloned().ok_or("Task not found")?;
            let has = |k: &str| fields.get(k).is_some();
            let parent = if has("parent") { db.parent_of(id, fields.get("parent"), old.quote.as_deref(), Some(item))? } else { old.parent.clone() };
            let moved = parent != old.parent;
            let position = match fields.get("position").and_then(Value::as_i64) {
                Some(p) => p,
                None if moved => db.last_position(id, old.quote.as_deref(), parent.as_deref()) + 1,
                None => old.position,
            };
            let status = if has("status") { status_of(fields.get("status"))? } else { old.status };
            let pick_day = |k: &str, before: &Option<String>| if has(k) { day_or_none(fields.get(k), k) } else { Ok(before.clone()) };
            let requested = pick_day("requested", &old.requested)?;
            let due = pick_day("due", &old.due)?;
            let (mut finished, mut live) = (pick_day("finished", &old.finished)?, pick_day("live", &old.live)?);
            // The day's dates: only when the state is written.
            if has("status") {
                dates_of(status, &mut finished, &mut live, &today());
            }
            let title = if has("title") { required(fields.get("title"), "title", 200)? } else { old.title.clone() };
            let body = if has("body") { text(fields.get("body"), "body", BODY_MAX, false)? } else { old.body.clone() };
            let minutes = if has("minutes") { self::minutes(fields.get("minutes"), "minutes", HOURS_MAX, false)? } else { old.minutes };
            let requester = if has("requester") { text(fields.get("requester"), "requester", 80, false)? } else { old.requester.clone() };
            let progress = if has("progress") { percent(fields.get("progress"), "progress")? } else { old.progress };
            let comment = if has("comment") { text(fields.get("comment"), "comment", 500, false)? } else { old.comment.clone() };
            let env = if has("env") { env_of(fields.get("env"))? } else if has("status") && status == Status::Live { Some("prod".into()) } else { old.env.clone() };
            let i = db.items.iter_mut().find(|i| i.id == item).ok_or("Task not found")?;
            *i = StoredItem { title, body, minutes, status, parent, position, updated: now_ms(), requested, due, finished, live, requester, progress, comment, env, ..old };
            Ok(())
        })
    }

    pub fn delete_item(&self, id: &str, item: &str) -> Answer<Project> {
        self.change(id, |db| {
            if db.item_of(id, item).is_none() {
                return Err("Task not found".into());
            }
            db.items.retain(|i| i.id != item);
            Ok(())
        })
    }
}

/// A duration as typed, in minutes: "1h30", "1 h 30", "1,5", "1.5" (hours), "90m", "90 min", "0:45".
/// A bare number is hours. None: unreadable, or zero.
pub fn parse_duration(text: &str) -> Option<u32> {
    let s: String = text.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect::<String>().replace(',', ".");
    if s.is_empty() {
        return None;
    }
    let number = |t: &str| -> Option<f64> { if t.is_empty() { Some(0.0) } else { t.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0) } };
    let minutes = if let Some(m) = s.strip_suffix("min").or_else(|| s.strip_suffix("mn")).or_else(|| s.strip_suffix('m')) {
        // "1h30m" too.
        match m.split_once('h') {
            Some((h, m)) => number(h)? * 60.0 + number(m)?,
            None => number(m)?,
        }
    } else if let Some((h, m)) = s.split_once('h').or_else(|| s.split_once(':')) {
        let m = number(m)?;
        if m >= 60.0 {
            return None;
        }
        number(h)? * 60.0 + m
    } else {
        number(&s)? * 60.0
    };
    let minutes = minutes.round();
    (minutes >= 1.0 && minutes <= u32::MAX as f64).then_some(minutes as u32)
}

/// Minutes said as hours: "1 h 30", "45 min", "12 h", "-2 h 15" (overdrawn).
pub fn format_minutes(minutes: i64) -> String {
    let sign = if minutes < 0 { "-" } else { "" };
    let m = minutes.unsigned_abs();
    match (m / 60, m % 60) {
        (0, m) => format!("{sign}{m} min"),
        (h, 0) => format!("{sign}{h} h"),
        (h, m) => format!("{sign}{h} h {m:02}"),
    }
}

/// Today, as the tracking takes days: "2026-11-18".
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// A day as typed ("18/11/2026", "18/11", "2026-11-18", "hier", "aujourd'hui"), as the tracking takes it.
pub fn parse_day(text: &str) -> Option<String> {
    use chrono::{Datelike as _, NaiveDate};
    let t = text.trim().to_lowercase();
    let now = chrono::Local::now().date_naive();
    match t.as_str() {
        "" | "today" | "aujourd'hui" | "aujourd’hui" | "auj" => {
            return Some(now.format("%Y-%m-%d").to_string());
        }
        "yesterday" | "hier" => {
            return Some((now - chrono::Duration::days(1)).format("%Y-%m-%d").to_string());
        }
        _ => {}
    }
    if let Ok(d) = NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    if let Ok(d) = NaiveDate::parse_from_str(&t, "%d/%m/%Y") {
        return Some(d.format("%Y-%m-%d").to_string());
    }
    let (d, m) = t.split_once('/')?;
    NaiveDate::from_ymd_opt(now.year(), m.parse().ok()?, d.parse().ok()?).map(|d| d.format("%Y-%m-%d").to_string())
}

/// A day shown the French way: "18/11/2026".
pub fn show_day(day: &str) -> String {
    match chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") {
        Ok(d) => d.format("%d/%m/%Y").to_string(),
        Err(_) => day.to_owned(),
    }
}

/// The tracking of this computer (`ronnie track`).
fn cli_client() -> Result<Client, String> {
    Client::local()
}

/// The project named by `word`: its id (or its start), its name, or "client/name" (no case).
fn find_project<'a>(projects: &'a [Summary], word: &str) -> Result<&'a Summary, String> {
    let w = word.trim().to_lowercase();
    let exact: Vec<&Summary> =
        projects.iter().filter(|p| p.id == word || p.name.to_lowercase() == w || p.client.as_ref().is_some_and(|c| format!("{}/{}", c.to_lowercase(), p.name.to_lowercase()) == w)).collect();
    let found = if exact.is_empty() {
        projects
            .iter()
            .filter(|p| match w.split_once('/') {
                // "client/name", both in part.
                Some((c, n)) => p.client.as_ref().is_some_and(|pc| pc.to_lowercase().contains(c.trim())) && p.name.to_lowercase().contains(n.trim()),
                None => (w.len() >= 4 && p.id.starts_with(&w)) || p.name.to_lowercase().contains(&w),
            })
            .collect()
    } else {
        exact
    };
    match found.as_slice() {
        [p] => Ok(p),
        [] => Err(format!("{word}: no such project (see `ronnie track`)")),
        many => Err(format!("{word}: several projects ({}): give \"client/name\" or the id", many.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

fn find_counter<'a>(project: &'a Project, word: &str) -> Result<&'a Counter, String> {
    let w = word.to_lowercase();
    let found: Vec<&Counter> = project.counters.iter().filter(|c| c.id == word || (w.len() >= 4 && c.id.starts_with(&w)) || c.label.to_lowercase() == w).collect();
    match found.as_slice() {
        [c] => Ok(c),
        [] => Err(format!("{word}: no such counter in {}", project.name)),
        _ => Err(format!("{word}: several counters: give the id (`ronnie track show`)")),
    }
}

fn find_quote<'a>(project: &'a Project, word: &str) -> Result<&'a Quote, String> {
    let w = word.to_lowercase();
    let found: Vec<&Quote> =
        project.quotes.iter().filter(|q| q.id == word || (w.len() >= 4 && q.id.starts_with(&w)) || q.label.to_lowercase() == w || q.number.as_ref().is_some_and(|n| n.to_lowercase() == w)).collect();
    match found.as_slice() {
        [q] => Ok(q),
        [] => Err(format!("{word}: no such quote in {}", project.name)),
        _ => Err(format!("{word}: several quotes: give the id (`ronnie track show`)")),
    }
}

fn find_item<'a>(project: &'a Project, word: &str) -> Result<&'a Item, String> {
    let w = word.to_lowercase();
    let found: Vec<&Item> = project.items.iter().filter(|i| i.id == word || (w.len() >= 4 && i.id.starts_with(&w)) || i.title.to_lowercase() == w).collect();
    match found.as_slice() {
        [i] => Ok(i),
        [] => Err(format!("{word}: no such planned task in {} (`ronnie track show`)", project.name)),
        _ => Err(format!("{word}: several planned tasks: give the id (`ronnie track show`)")),
    }
}

/// The planned tasks of a quote (or the project's own), as an indented tree.
fn items_tree(p: &Project, quote: Option<&str>, parent: Option<&str>, depth: usize, out: &mut Vec<String>) {
    for i in p.children(quote, parent) {
        let r = p.rollup(i);
        let est = if r.estimate > 0 { format_minutes(r.estimate as i64) } else { "not estimated".into() };
        out.push(format!("{}{}  [{}] {}  ({} of {est})", "    ".repeat(depth + 1), short(&i.id), i.status.word(), i.title, format_minutes(r.spent as i64)));
        if let Some(body) = i.body.as_deref().filter(|b| !b.trim().is_empty()) {
            for line in body.lines().take(6) {
                out.push(format!("{}  | {line}", "    ".repeat(depth + 2)));
            }
        }
        items_tree(p, quote, Some(&i.id), depth + 1, out);
    }
}

fn short(id: &str) -> &str {
    &id[..8.min(id.len())]
}

/// The options given to `ronnie track`, by name (None: a flag, or `--counter` alone).
type Options = std::collections::HashMap<String, Option<String>>;

/// Words and their options: `--name value` (or a lone `--flag`, given in `flags`).
fn split_options<'a>(rest: &[&'a str], flags: &[&str]) -> Result<(Vec<&'a str>, Options), String> {
    let (mut words, mut options) = (Vec::new(), std::collections::HashMap::new());
    let mut k = 0;
    while k < rest.len() {
        let w = rest[k];
        if let Some(name) = w.strip_prefix("--") {
            // A lone flag, or one whose value is optional (`--counter` alone: the current one).
            let next = rest.get(k + 1).filter(|v| !v.starts_with("--"));
            if flags.contains(&name) {
                options.insert(name.to_owned(), None);
            } else if name == "counter" {
                options.insert(name.to_owned(), next.map(|v| (*v).to_owned()));
                k += usize::from(next.is_some());
            } else {
                let v = next.ok_or_else(|| format!("--{name}: a value after it"))?;
                options.insert(name.to_owned(), Some((*v).to_owned()));
                k += 1;
            }
        } else {
            words.push(w);
        }
        k += 1;
    }
    Ok((words, options))
}

fn charged(project: &Project, t: &Task) -> String {
    match t.billing {
        Billing::Counter => format!("counter {}", t.counter.as_ref().and_then(|c| project.counters.iter().find(|x| &x.id == c)).map_or("?", |c| c.label.as_str())),
        Billing::Quote => format!("quote {}", t.quote.as_ref().and_then(|q| project.quotes.iter().find(|x| &x.id == q)).map_or("?".into(), Quote::title)),
        Billing::Free => "offered".into(),
        Billing::Maintenance => "maintenance".into(),
    }
}

/// The project, said for Claude.
fn describe(p: &Project, all: bool) -> String {
    let mut out = vec![format!("{} — {} ({})", p.client.as_deref().unwrap_or("no client"), p.name, short(&p.id))];
    if let Some(note) = &p.note {
        out.push(format!("note: {note}"));
    }
    out.push(format!(
        "spent: {} (counters {}, quotes {}, offered {}, maintenance {})",
        format_minutes(p.tasks.iter().map(|t| t.minutes as i64).sum()),
        format_minutes(p.by_billing(Billing::Counter) as i64),
        format_minutes(p.by_billing(Billing::Quote) as i64),
        format_minutes(p.by_billing(Billing::Free) as i64),
        format_minutes(p.by_billing(Billing::Maintenance) as i64)
    ));
    out.push("counters:".into());
    for c in &p.counters {
        out.push(format!(
            "  {}  {}  since {}  bought {}, used {}, left {}{}",
            short(&c.id),
            c.label,
            show_day(&c.day),
            format_minutes(c.minutes as i64),
            format_minutes(c.used as i64),
            format_minutes(c.left()),
            if c.closed.is_some() {
                "  [closed]"
            } else if c.left() <= 0 {
                "  [USED UP: propose a new counter]"
            } else {
                ""
            }
        ));
    }
    if p.counters.is_empty() {
        out.push("  (none)".into());
    }
    out.push("quotes:".into());
    for q in &p.quotes {
        let estimate = p.quote_rollup(q).estimate;
        let planned = if estimate > 0 { format_minutes(estimate as i64) } else { "not estimated".into() };
        out.push(format!("  {}  {}  {}  planned {planned}, spent {}{}", short(&q.id), q.title(), show_day(&q.day), format_minutes(q.used as i64), if q.closed.is_some() { "  [closed]" } else { "" }));
        if let Some(note) = q.note.as_deref().filter(|n| !n.trim().is_empty()) {
            for line in note.lines().take(12) {
                out.push(format!("      | {line}"));
            }
        }
        items_tree(p, Some(&q.id), None, 0, &mut out);
    }
    if p.items.iter().any(|i| i.quote.is_none()) {
        out.push("planned tasks outside quotes:".into());
        items_tree(p, None, None, 0, &mut out);
    }
    if p.quotes.is_empty() {
        out.push("  (none)".into());
    }
    out.push("tasks (newest first):".into());
    let shown = if all { p.tasks.len() } else { 30.min(p.tasks.len()) };
    for t in &p.tasks[..shown] {
        out.push(format!(
            "  {}  {}  {:>8}  {}  [{}]  {}",
            short(&t.id),
            show_day(&t.day),
            format_minutes(t.minutes as i64),
            t.author.as_deref().unwrap_or("?"),
            charged(p, t),
            t.label.replace('\n', " / ")
        ));
    }
    if p.tasks.is_empty() {
        out.push("  (none)".into());
    } else if shown < p.tasks.len() {
        out.push(format!("  … {} more (--all)", p.tasks.len() - shown));
    }
    out.join("\n")
}

/// `ronnie track …`: the project tracking, straight in its file (the page in Ronnie follows).
pub fn cli(rest: &[&str]) -> Result<String, String> {
    cli_run(rest)
}

fn cli_run(rest: &[&str]) -> Result<String, String> {
    let client = cli_client()?;
    let usage = || crate::drive::USAGE.to_owned();
    let open = |word: &str| -> Result<Project, String> {
        let list = client.projects()?;
        let id = find_project(&list, word)?.id.clone();
        client.project(&id)
    };
    let day_of = |o: &Options| -> Result<String, String> {
        match o.get("day").cloned().flatten() {
            Some(d) => parse_day(&d).ok_or_else(|| format!("{d}: a day as 18/11/2026, 18/11, 2026-11-18 or hier")),
            None => Ok(today()),
        }
    };
    match rest {
        [] => {
            let list = client.projects()?;
            if list.is_empty() {
                return Ok("No project yet (`ronnie track new <name> --client C`).".into());
            }
            let mut lines = Vec::new();
            for s in &list {
                let p = client.project(&s.id)?;
                let left = match p.current_counter() {
                    Some(c) => format!("counter {} left", format_minutes(c.left())),
                    None => "no open counter".into(),
                };
                lines.push(format!(
                    "{}  {} / {}  spent {}, {left}{}{}",
                    short(&s.id),
                    s.client.as_deref().unwrap_or("-"),
                    s.name,
                    format_minutes(s.spent as i64),
                    if p.counters_spent() { "  (counters used up)" } else { "" },
                    if s.archived.is_some() { "  (archived)" } else { "" }
                ));
            }
            Ok(format!("{}\n(`ronnie track show <project>` for one)", lines.join("\n")))
        }
        ["show", rest @ ..] => {
            let (words, o) = split_options(rest, &["all"])?;
            let project = open(&words.join(" "))?;
            Ok(describe(&project, o.contains_key("all")))
        }
        ["new", rest @ ..] => {
            let (words, o) = split_options(rest, &[])?;
            if words.is_empty() {
                return Err(usage());
            }
            let p = client.create(&words.join(" "), o.get("client").cloned().flatten().as_deref(), o.get("color").cloned().flatten().as_deref())?;
            Ok(format!("Project {} made ({}).", p.name, short(&p.id)))
        }
        ["task", "rm", project, task] => {
            let p = open(project)?;
            let t = p.tasks.iter().find(|t| t.id.starts_with(*task)).ok_or_else(|| format!("{task}: no such task in {}", p.name))?;
            client.delete_task(&p.id, &t.id)?;
            Ok(format!("Task deleted: {} ({}).", t.label, format_minutes(t.minutes as i64)))
        }
        ["task", project, duration, rest @ ..] => {
            let (words, o) = split_options(rest, &["free", "maintenance"])?;
            if words.is_empty() {
                return Err("A description of the task, after its duration.".into());
            }
            let minutes = parse_duration(duration).ok_or_else(|| format!("{duration}: a duration as 1h30, 1,5, 45m"))?;
            if minutes > TASK_MAX {
                return Err("A task: 1000 h at most.".into());
            }
            let p = open(project)?;
            let mut task = TaskInput { day: day_of(&o)?, label: words.join(" "), minutes, ..TaskInput::default() };
            // On a planned task: charged to its quote unless said otherwise.
            let item = match o.get("item").cloned().flatten() {
                Some(i) => Some(find_item(&p, &i)?.clone()),
                None => None,
            };
            task.item = item.as_ref().map(|i| i.id.clone());
            let item_quote = item.as_ref().and_then(|i| i.quote.clone());
            if o.contains_key("free") {
                task.billing = Billing::Free;
            } else if o.contains_key("maintenance") {
                task.billing = Billing::Maintenance;
            } else if let Some(q) = o.get("quote").cloned().flatten() {
                task.billing = Billing::Quote;
                task.quote = Some(find_quote(&p, &q)?.id.clone());
            } else if let (Some(q), false) = (item_quote, o.contains_key("counter")) {
                task.billing = Billing::Quote;
                task.quote = Some(q);
            } else {
                task.billing = Billing::Counter;
                task.counter = Some(match o.get("counter").cloned().flatten() {
                    Some(c) => find_counter(&p, &c)?.id.clone(),
                    None => p.current_counter().ok_or("No open counter: say how it's charged (--quote, --free, --maintenance) or make a counter first.")?.id.clone(),
                });
            }
            let after = client.add_task(&p.id, &task)?;
            let mut said = format!("Logged in {}: {} ({}, {}).", after.name, task.label.lines().next().unwrap_or(""), format_minutes(minutes as i64), show_day(&task.day));
            if let Some(c) = task.counter.as_ref().and_then(|id| after.counters.iter().find(|c| &c.id == id)) {
                said.push_str(&format!(" Counter {}: {} left.", c.label, format_minutes(c.left())));
                if c.left() <= 0 {
                    said.push_str(" It is used up: propose a new counter to the user.");
                }
            }
            Ok(said)
        }
        ["counter", "close", project, counter] => {
            let p = open(project)?;
            let c = find_counter(&p, counter)?;
            client.update_counter(&p.id, &c.id, serde_json::json!({ "closed": true }))?;
            Ok(format!("Counter {} closed.", c.label))
        }
        ["counter", project, hours, rest @ ..] => {
            let (words, o) = split_options(rest, &[])?;
            let minutes = parse_duration(hours).ok_or_else(|| format!("{hours}: hours as 10, 7,5, 10h"))?;
            let p = open(project)?;
            let day = day_of(&o)?;
            let label = if words.is_empty() { format!("Contrat {} du {}", format_minutes(minutes as i64), show_day(&day)) } else { words.join(" ") };
            client.add_counter(&p.id, &label, minutes as u64, &day)?;
            Ok(format!("Counter {label} made in {}.", p.name))
        }
        ["quote", project, rest @ ..] => {
            let (words, o) = split_options(rest, &[])?;
            if words.is_empty() {
                return Err(usage());
            }
            let p = open(project)?;
            let minutes = match o.get("hours").cloned().flatten() {
                Some(h) => Some(parse_duration(&h).ok_or_else(|| format!("{h}: hours as 5, 7,5, 5h"))? as u64),
                None => None,
            };
            let label = words.join(" ");
            client.add_quote(&p.id, o.get("number").cloned().flatten().as_deref(), &label, minutes, &day_of(&o)?)?;
            Ok(format!("Quote {label} made in {}.", p.name))
        }
        ["item", "status", project, item, status] => {
            let status = Status::parse(status).ok_or("A status: todo, waiting, doing, done, live or cancelled.")?;
            let p = open(project)?;
            let i = find_item(&p, item)?;
            client.update_item(&p.id, &i.id, serde_json::json!({ "status": status.word() }))?;
            Ok(format!("{}: {}.", i.title, status.word()))
        }
        ["item", "rm", project, item] => {
            let p = open(project)?;
            let i = find_item(&p, item)?;
            client.delete_item(&p.id, &i.id)?;
            Ok(format!("Planned task {} deleted (its sub-tasks too; the time logged on it stays).", i.title))
        }
        ["item", project, rest @ ..] => {
            let (words, o) = split_options(rest, &[])?;
            if words.is_empty() {
                return Err(usage());
            }
            let p = open(project)?;
            let parent = match o.get("parent").cloned().flatten() {
                Some(w) => Some(find_item(&p, &w)?.clone()),
                None => None,
            };
            // Under a parent: its quote; else the quote said (none: the project's own).
            let quote = match (&parent, o.get("quote").cloned().flatten()) {
                (Some(parent), _) => parent.quote.clone(),
                (None, Some(q)) => Some(find_quote(&p, &q)?.id.clone()),
                (None, None) => None,
            };
            let minutes = match o.get("hours").cloned().flatten() {
                Some(h) => Some(parse_duration(&h).ok_or_else(|| format!("{h}: hours as 4, 2h30, 45m"))? as u64),
                None => None,
            };
            let status = match o.get("status").cloned().flatten() {
                Some(s) => Status::parse(&s).ok_or("A status: todo, waiting, doing, done, live or cancelled.")?,
                None => Status::Todo,
            };
            let title = words.join(" ");
            let input = ItemInput { title: title.clone(), body: o.get("body").cloned().flatten(), minutes, status, requested: Some(today()), ..ItemInput::default() };
            let after = client.add_item(&p.id, quote.as_deref(), parent.as_ref().map(|i| i.id.as_str()), &input)?;
            let made = after.items.iter().filter(|i| i.title == title).max_by_key(|i| i.position).map_or("?", |i| short(&i.id));
            Ok(format!("Planned task {title} made ({made}) in {}.", after.name))
        }
        _ => Err(usage()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn six_states_and_an_unknown_one_read_as_to_do() {
        let read = |w: &str| serde_json::from_value::<Status>(serde_json::json!(w)).unwrap();
        assert_eq!(Status::ALL.map(|st| read(st.word())), Status::ALL);
        assert_eq!(read("archived-someday"), Status::Todo);
        assert_eq!(Status::parse("En production"), Some(Status::Live));
        assert_eq!(Status::parse("annulé"), Some(Status::Cancelled));
        assert!(Status::Cancelled.closed() && !Status::Cancelled.finished() && Status::Live.finished() && !Status::Waiting.closed());
    }

    #[test]
    fn a_task_says_how_far_and_whether_late() {
        let item: Item = serde_json::from_value(serde_json::json!({
            "id": "i", "quote": null, "parent": null, "title": "T", "minutes": 240, "status": "doing", "position": 0, "spent": 60, "due": "2026-11-10"
        }))
        .unwrap();
        assert_eq!(item.progress(), Some(25));
        assert!(item.late("2026-11-19") && !item.late("2026-11-10"));
        let said = Item { progress: Some(80), ..item.clone() };
        assert_eq!(said.progress(), Some(80));
        let live = Item { status: Status::Live, ..item };
        assert_eq!((live.progress(), live.late("2026-12-01")), (Some(100), false));
    }

    #[test]
    fn joined_files_named_typed_and_sized() {
        assert_eq!(super::mime_of("Contrat signé.PDF"), "application/pdf");
        assert_eq!(super::mime_of("devis.docx"), "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
        assert_eq!(super::mime_of("sans-extension"), "application/octet-stream");
        assert_eq!(super::size_text(900), "900 o");
        assert_eq!(super::size_text(12 * 1024), "12 Ko");
        assert_eq!(super::size_text(3_565_158), "3,4 Mo");
        assert_eq!(super::clean_name("../../.ssh/Contrat:signé?.pdf").as_deref(), Some("Contratsigné.pdf"));
        assert_eq!(super::clean_name(".cache"), None);
    }

    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("1h30"), Some(90));
        assert_eq!(parse_duration("1 h 30"), Some(90));
        assert_eq!(parse_duration("2h"), Some(120));
        assert_eq!(parse_duration("1,5"), Some(90));
        assert_eq!(parse_duration("0.25"), Some(15));
        assert_eq!(parse_duration("45m"), Some(45));
        assert_eq!(parse_duration("45 min"), Some(45));
        assert_eq!(parse_duration("1h15m"), Some(75));
        assert_eq!(parse_duration("0:45"), Some(45));
        assert_eq!(parse_duration("2"), Some(120));
        assert_eq!(parse_duration("0"), None);
        assert_eq!(parse_duration("1h75"), None);
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn minutes_said() {
        assert_eq!(format_minutes(90), "1 h 30");
        assert_eq!(format_minutes(45), "45 min");
        assert_eq!(format_minutes(600), "10 h");
        assert_eq!(format_minutes(-135), "-2 h 15");
        assert_eq!(format_minutes(65), "1 h 05");
    }

    #[test]
    fn days() {
        assert_eq!(parse_day("2026-11-18").as_deref(), Some("2026-11-18"));
        assert_eq!(parse_day("18/11/2026").as_deref(), Some("2026-11-18"));
        assert_eq!(parse_day("31/02/2026"), None);
        assert_eq!(show_day("2026-11-18"), "18/11/2026");
        assert_eq!(parse_day("hier").map(|d| d.len()), Some(10));
    }

    #[test]
    fn planned_tasks_add_up() {
        let item = |id: &str, parent: Option<&str>, minutes: Option<u64>, spent: u64, status: Status| Item {
            id: id.into(), quote: Some("q".into()), parent: parent.map(Into::into), title: id.into(), body: None, minutes, status, position: 0, spent,
            requested: None, due: None, finished: None, live: None, requester: None, progress: None, comment: None, env: None,
        };
        let p = Project {
            id: "p".into(), name: "P".into(), client: None, color: None, note: None, role: Role::Owner, updated: 0, archived: None, members: vec![], counters: vec![], tasks: vec![],
            quotes: vec![Quote { id: "q".into(), number: None, label: "Q".into(), note: None, minutes: None, day: "2026-01-01".into(), closed: None, used: 330 }],
            items: vec![
                item("a", None, None, 0, Status::Todo),
                item("a1", Some("a"), Some(180), 150, Status::Done),
                item("a2", Some("a"), Some(120), 60, Status::Doing),
                item("b", None, Some(600), 120, Status::Todo),
            ],
            files: vec![],
        };
        let a = p.rollup(&p.items[0]);
        // No estimate of its own: its sub-tasks'.
        assert_eq!((a.estimate, a.spent, a.done, a.count), (300, 210, 1, 3));
        let q = p.quote_rollup(&p.quotes[0]);
        assert_eq!((q.estimate, q.spent, q.done, q.count), (900, 330, 1, 4));
        assert_eq!(p.item_path("a2"), "a › a2");
    }

    #[test]
    fn the_counter_to_use() {
        let counter = |id: &str, minutes, used, closed| Counter { id: id.into(), label: id.into(), minutes, day: "2026-01-01".into(), closed, used };
        let mut p = Project {
            id: "p".into(),
            name: "P".into(),
            client: None,
            color: None,
            note: None,
            role: Role::Owner,
            updated: 0,
            archived: None,
            members: vec![],
            counters: vec![counter("a", 600, 600, None), counter("b", 600, 30, None)],
            quotes: vec![],
            tasks: vec![],
            items: vec![],
            files: vec![],
        };
        assert_eq!(p.current_counter().map(|c| c.id.as_str()), Some("b"));
        assert!(!p.counters_spent());
        p.counters[1].used = 700;
        assert_eq!(p.current_counter().map(|c| c.id.as_str()), Some("b"));
        assert!(p.counters_spent());
        p.counters[1].closed = Some(1);
        p.counters[0].closed = Some(1);
        assert_eq!(p.current_counter(), None);
    }

    /// The tracking kept on this computer, the whole way: a project, a counter, a quote with its
    /// planned tasks (a sub-task, a level too deep refused), time spent and charged, a file joined,
    /// what can't go (a counter used) and what goes with its quote.
    #[test]
    fn a_project_kept_here() {
        let dir = std::env::temp_dir().join(format!("ronnie-track-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let c = Client::at(&dir);
        assert!(c.projects().unwrap().is_empty());
        assert!(c.create("  ", None, None).is_err(), "a name");
        let p = c.create("Site", Some("Acme"), Some("#AABBCC")).unwrap();
        assert_eq!((p.color.as_deref(), p.role), (Some("#aabbcc"), Role::Owner));
        let id = p.id.clone();
        let p = c.add_counter(&id, "Contrat", 600, "2026-11-01").unwrap();
        let counter = p.counters[0].id.clone();
        let p = c.add_quote(&id, Some("D-1"), "Refonte", Some(1200), "2026-11-02").unwrap();
        let quote = p.quotes[0].id.clone();
        let p = c.add_item(&id, Some(&quote), None, &ItemInput { title: "Accueil".into(), status: Status::Todo, ..ItemInput::default() }).unwrap();
        let top = p.items[0].id.clone();
        let p = c.add_item(&id, Some(&quote), Some(&top), &ItemInput { title: "Menu".into(), status: Status::Live, ..ItemInput::default() }).unwrap();
        let sub = p.items.iter().find(|i| i.title == "Menu").unwrap();
        assert_eq!((sub.env.as_deref(), sub.finished.is_some(), sub.live.is_some()), (Some("prod"), true, true));
        let sub = sub.id.clone();
        let p = c.add_item(&id, Some(&quote), Some(&sub), &ItemInput { title: "Lien".into(), ..ItemInput::default() }).unwrap();
        let deep = p.items.iter().find(|i| i.title == "Lien").unwrap().id.clone();
        assert!(c.add_item(&id, Some(&quote), Some(&deep), &ItemInput { title: "Trop".into(), ..ItemInput::default() }).is_err(), "three levels at most");
        assert!(c.update_item(&id, &top, serde_json::json!({ "parent": deep })).is_err(), "not under itself");
        let task = |minutes, billing, counter: Option<&str>, quote: Option<&str>, item: Option<&str>| TaskInput { day: "2026-11-05".into(), label: "Travail".into(), minutes, billing, counter: counter.map(str::to_owned), quote: quote.map(str::to_owned), item: item.map(str::to_owned) };
        let p = c.add_task(&id, &task(90, Billing::Counter, Some(&counter), None, None)).unwrap();
        let p2 = c.add_task(&id, &task(60, Billing::Quote, None, Some(&quote), Some(&sub))).unwrap();
        assert!(c.add_task(&id, &task(30, Billing::Counter, Some("nope"), None, None)).is_err());
        assert_eq!((p.counters[0].used, p2.quotes[0].used, p2.items.iter().find(|i| i.id == sub).unwrap().spent), (90, 60, 60));
        assert_eq!(p2.summary().spent, 150);
        assert!(c.delete_counter(&id, &counter).is_err(), "tasks are taken off it");
        // A file joined to the quote: kept, read back; it goes with what it's joined to.
        let file = uuid::Uuid::new_v4().to_string();
        c.upload(&id, &format!("quote:{quote}"), &file, "devis.pdf", b"%PDF-1.4").unwrap();
        assert_eq!(c.download(&id, &file).unwrap(), b"%PDF-1.4");
        assert!(c.upload(&id, "item:nope", &uuid::Uuid::new_v4().to_string(), "x.txt", b"x").is_err());
        let worked = c.project(&id).unwrap().tasks.iter().find(|t| t.quote.is_some()).unwrap().id.clone();
        c.delete_task(&id, &worked).unwrap();
        let p = c.delete_quote(&id, &quote).unwrap();
        assert!(p.quotes.is_empty() && p.items.is_empty() && p.files.is_empty(), "its tasks and files go with it");
        assert!(c.download(&id, &file).is_err());
        // Archived, then gone.
        assert!(c.update(&id, serde_json::json!({ "archived": true })).unwrap().archived.is_some());
        assert_eq!(c.projects().unwrap().len(), 1);
        c.delete(&id).unwrap();
        assert!(c.projects().unwrap().is_empty() && c.project(&id).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

}
