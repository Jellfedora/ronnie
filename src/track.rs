//! Project tracking ("Suivi"), kept on floor (`/v1/track`) with the account, in clear: the projects
//! (a client each), their hour counters (the client's contracts), their quotes, the tasks done (a
//! duration, charged to a counter, to a quote, offered, or part of the maintenance), shared with
//! colleagues as admin, to fill in tasks, or to look at.
//!
//! Blocking calls: the interface makes them in a thread (`app::track`), `ronnie track` directly.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::FloorAccount;
use crate::floor::{USER_AGENT, account_agent};

/// The error of a call refused because the account isn't allowed yet by floor's administrator.
pub const PENDING: &str = "pending";

/// A task: 1000 h at most (as floor takes): a block of hours already done, charged at once.
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
    pub const GIVEN: [Role; 3] = [Role::Admin, Role::Editor, Role::Viewer];

    pub fn word(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Editor => "editor",
            Role::Admin => "admin",
            Role::Owner => "owner",
        }
    }

    pub fn parse(word: &str) -> Option<Role> {
        match word.to_lowercase().as_str() {
            "viewer" | "lecture" | "read" => Some(Role::Viewer),
            "editor" | "saisie" | "write" => Some(Role::Editor),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }

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

/// Where a planned task is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Todo,
    Doing,
    Done,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::Doing => "doing",
            Status::Done => "done",
        }
    }

    pub fn parse(word: &str) -> Option<Status> {
        match word.to_lowercase().as_str() {
            "todo" | "à faire" | "a faire" => Some(Status::Todo),
            "doing" | "en cours" => Some(Status::Doing),
            "done" | "fait" | "faite" => Some(Status::Done),
            _ => None,
        }
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
            done: usize::from(item.status == Status::Done) + kids.iter().map(|r| r.done).sum::<usize>(),
            count: 1 + kids.iter().map(|r| r.count).sum::<usize>(),
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
}

/// The calls to floor's tracking, with an account.
#[derive(Clone)]
pub struct Client {
    url: String,
    token: String,
}

/// What a call to floor gave back, an error said as floor said it.
type Answer<T> = Result<T, String>;

impl Client {
    pub fn new(account: &FloorAccount) -> Client {
        Client { url: account.url.trim_end_matches('/').to_owned(), token: account.token.clone() }
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Answer<Option<Value>> {
        let agent = account_agent();
        let url = format!("{}/v1/track{path}", self.url);
        let auth = format!("Bearer {}", self.token);
        let res = match (method, body) {
            ("GET", _) => agent.get(&url).header("User-Agent", USER_AGENT).header("Authorization", &auth).call(),
            ("DELETE", _) => agent.delete(&url).header("User-Agent", USER_AGENT).header("Authorization", &auth).call(),
            ("POST", b) => agent.post(&url).header("User-Agent", USER_AGENT).header("Authorization", &auth).send_json(b.unwrap_or(Value::Null)),
            ("PUT", b) => agent.put(&url).header("User-Agent", USER_AGENT).header("Authorization", &auth).send_json(b.unwrap_or(Value::Null)),
            (_, b) => agent.patch(&url).header("User-Agent", USER_AGENT).header("Authorization", &auth).send_json(b.unwrap_or(Value::Null)),
        };
        let mut res = res.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        if status == 204 {
            return Ok(None);
        }
        let value: Value = res.body_mut().read_json().unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(Some(value));
        }
        if value.get("code").and_then(Value::as_str) == Some("pending") {
            return Err(PENDING.to_owned());
        }
        match value.get("error").and_then(Value::as_str) {
            Some(e) => Err(e.to_owned()),
            None => Err(format!("floor answered {status}")),
        }
    }

    fn project_of(&self, method: &str, path: &str, body: Option<Value>) -> Answer<Project> {
        let value = self.call(method, path, body)?.ok_or("floor gave nothing back")?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    /// The accounts one can share with.
    pub fn people(&self) -> Answer<Vec<String>> {
        #[derive(Deserialize)]
        struct People {
            people: Vec<String>,
        }
        let value = self.call("GET", "/people", None)?.unwrap_or(Value::Null);
        Ok(serde_json::from_value::<People>(value).map_err(|e| e.to_string())?.people)
    }

    pub fn projects(&self) -> Answer<Vec<Summary>> {
        #[derive(Deserialize)]
        struct List {
            projects: Vec<Summary>,
        }
        let value = self.call("GET", "/projects", None)?.unwrap_or(Value::Null);
        Ok(serde_json::from_value::<List>(value).map_err(|e| e.to_string())?.projects)
    }

    pub fn project(&self, id: &str) -> Answer<Project> {
        self.project_of("GET", &format!("/projects/{id}"), None)
    }

    pub fn create(&self, name: &str, client: Option<&str>, color: Option<&str>) -> Answer<Project> {
        crate::telemetry::count("track.project");
        self.project_of("POST", "/projects", Some(serde_json::json!({ "name": name, "client": client, "color": color })))
    }

    /// The project's fields given in `fields` (name, client, color, note, archived) changed.
    pub fn update(&self, id: &str, fields: Value) -> Answer<Project> {
        self.project_of("PATCH", &format!("/projects/{id}"), Some(fields))
    }

    pub fn delete(&self, id: &str) -> Answer<()> {
        self.call("DELETE", &format!("/projects/{id}"), None).map(|_| ())
    }

    pub fn share(&self, id: &str, name: &str, role: Role) -> Answer<Project> {
        self.project_of("PUT", &format!("/projects/{id}/members"), Some(serde_json::json!({ "name": name, "role": role.word() })))
    }

    /// A member taken off (None: oneself, who no longer sees it).
    pub fn unshare(&self, id: &str, player: &str) -> Answer<Option<Project>> {
        match self.call("DELETE", &format!("/projects/{id}/members/{player}"), None)? {
            Some(v) => serde_json::from_value(v).map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    pub fn add_counter(&self, id: &str, label: &str, minutes: u64, day: &str) -> Answer<Project> {
        self.project_of("POST", &format!("/projects/{id}/counters"), Some(serde_json::json!({ "label": label, "minutes": minutes, "day": day })))
    }

    pub fn update_counter(&self, id: &str, counter: &str, fields: Value) -> Answer<Project> {
        self.project_of("PATCH", &format!("/projects/{id}/counters/{counter}"), Some(fields))
    }

    pub fn delete_counter(&self, id: &str, counter: &str) -> Answer<Project> {
        self.project_of("DELETE", &format!("/projects/{id}/counters/{counter}"), None)
    }

    pub fn add_quote(&self, id: &str, number: Option<&str>, label: &str, minutes: Option<u64>, day: &str) -> Answer<Project> {
        crate::telemetry::count("track.quote");
        self.project_of("POST", &format!("/projects/{id}/quotes"), Some(serde_json::json!({ "number": number, "label": label, "minutes": minutes, "day": day })))
    }

    pub fn update_quote(&self, id: &str, quote: &str, fields: Value) -> Answer<Project> {
        self.project_of("PATCH", &format!("/projects/{id}/quotes/{quote}"), Some(fields))
    }

    pub fn delete_quote(&self, id: &str, quote: &str) -> Answer<Project> {
        self.project_of("DELETE", &format!("/projects/{id}/quotes/{quote}"), None)
    }

    pub fn add_task(&self, id: &str, task: &TaskInput) -> Answer<Project> {
        crate::telemetry::count("track.task");
        self.project_of("POST", &format!("/projects/{id}/tasks"), Some(serde_json::to_value(task).map_err(|e| e.to_string())?))
    }

    pub fn update_task(&self, id: &str, task: &str, input: &TaskInput) -> Answer<Project> {
        let mut body = serde_json::to_value(input).map_err(|e| e.to_string())?;
        // Charged elsewhere: the old counter or quote forgotten.
        body["counter"] = input.counter.clone().map_or(Value::Null, Value::String);
        body["quote"] = input.quote.clone().map_or(Value::Null, Value::String);
        body["item"] = input.item.clone().map_or(Value::Null, Value::String);
        self.project_of("PATCH", &format!("/projects/{id}/tasks/{task}"), Some(body))
    }

    pub fn delete_task(&self, id: &str, task: &str) -> Answer<Project> {
        self.project_of("DELETE", &format!("/projects/{id}/tasks/{task}"), None)
    }

    /// A planned task, of `quote` (None: the project's), under `parent`.
    pub fn add_item(&self, id: &str, quote: Option<&str>, parent: Option<&str>, item: &ItemInput) -> Answer<Project> {
        let mut body = serde_json::to_value(item).map_err(|e| e.to_string())?;
        body["quote"] = quote.map_or(Value::Null, |q| Value::String(q.to_owned()));
        body["parent"] = parent.map_or(Value::Null, |p| Value::String(p.to_owned()));
        self.project_of("POST", &format!("/projects/{id}/items"), Some(body))
    }

    /// A planned task's fields given in `fields` (title, body, minutes, status, parent, position).
    pub fn update_item(&self, id: &str, item: &str, fields: Value) -> Answer<Project> {
        self.project_of("PATCH", &format!("/projects/{id}/items/{item}"), Some(fields))
    }

    pub fn delete_item(&self, id: &str, item: &str) -> Answer<Project> {
        self.project_of("DELETE", &format!("/projects/{id}/items/{item}"), None)
    }

    pub fn add_quote_full(&self, id: &str, number: Option<&str>, label: &str, minutes: Option<u64>, day: &str, note: Option<&str>) -> Answer<Project> {
        self.project_of("POST", &format!("/projects/{id}/quotes"), Some(serde_json::json!({ "number": number, "label": label, "minutes": minutes, "day": day, "note": note })))
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

/// Today, as floor takes days: "2026-11-18".
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// A day as typed ("18/11/2026", "18/11", "2026-11-18", "hier", "aujourd'hui"), as floor takes it.
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

/// The account of this machine, to call floor with (`ronnie track`).
fn cli_client() -> Result<Client, String> {
    let config: crate::config::Config = crate::config::load(crate::config::config_path()).map_err(|e| format!("{e:#}"))?;
    match config.settings.floor {
        Some(account) if account.name.is_some() => Ok(Client::new(&account)),
        _ => Err("Not signed in to floor: the user signs in in Ronnie (Settings > Account) first.".into()),
    }
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

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Editor => "editor (fills in tasks)",
        Role::Viewer => "viewer",
    }
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
    let mut out = vec![format!("{} — {} ({}, you: {})", p.client.as_deref().unwrap_or("no client"), p.name, short(&p.id), role_name(p.role))];
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
    out.push(format!("members: {}", p.members.iter().map(|m| format!("{} ({})", m.name, m.role.word())).collect::<Vec<_>>().join(", ")));
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

/// `ronnie track …`: the project tracking, straight with floor (the page in Ronnie follows).
pub fn cli(rest: &[&str]) -> Result<String, String> {
    cli_run(rest).map_err(|e| if e == PENDING { "The user's account waits for floor's administrator to allow it: no project tracking until then.".to_owned() } else { e })
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
                    "{}  {} / {}  [{}]  spent {}, {left}{}{}",
                    short(&s.id),
                    s.client.as_deref().unwrap_or("-"),
                    s.name,
                    s.role.word(),
                    format_minutes(s.spent as i64),
                    if p.counters_spent() { "  (counters used up)" } else { "" },
                    if s.archived.is_some() { "  (archived)" } else { "" }
                ));
            }
            Ok(format!("{}\n(`ronnie track show <project>` for one)", lines.join("\n")))
        }
        ["people"] => Ok(client.people()?.join("\n")),
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
            let status = Status::parse(status).ok_or("A status: todo, doing or done.")?;
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
                Some(s) => Status::parse(&s).ok_or("A status: todo, doing or done.")?,
                None => Status::Todo,
            };
            let title = words.join(" ");
            let input = ItemInput { title: title.clone(), body: o.get("body").cloned().flatten(), minutes, status };
            let after = client.add_item(&p.id, quote.as_deref(), parent.as_ref().map(|i| i.id.as_str()), &input)?;
            let made = after.items.iter().filter(|i| i.title == title).max_by_key(|i| i.position).map_or("?", |i| short(&i.id));
            Ok(format!("Planned task {title} made ({made}) in {}.", after.name))
        }
        ["share", project, pseudo, role] => {
            let role = Role::parse(role).ok_or("A role: admin, editor (fills in tasks) or viewer.")?;
            let p = open(project)?;
            client.share(&p.id, pseudo, role)?;
            Ok(format!("{pseudo} has access to {} ({}).", p.name, role_name(role)))
        }
        _ => Err(usage()),
    }
}

#[cfg(test)]
mod tests {
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
}
