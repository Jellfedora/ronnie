//! The week's recap, for the meeting on Tuesdays: each evening a section of the day is added to a note
//! of the week (Tuesday to Tuesday) in the notes, and the week's total under it is written again.
//!
//! What it says comes from a journal kept here (`recap.json`: the time spent on each project, server
//! and database, Ronnie in front and in use, and in the games), from git (the user's own commits of the
//! day in those projects) and from Claude Code's sessions (`~/.claude/projects/*/*.jsonl`: the requests
//! typed, the tokens). Only the commits' subjects leave this computer, to Claude, to be translated
//! into French (and the European Central Bank is asked for the dollar's rate).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Datelike, Duration, Local, NaiveDate, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::i18n::Lang;

/// The days kept in the journal.
const KEEP_DAYS: i64 = 60;
/// Dollars to the euro when the European Central Bank's rate of the day can't be had (nor an older
/// one).
const USD_PER_EUR: f64 = 1.10;
/// Claude translating the commits into French: given up after this long (seconds).
const TRANSLATE_SECS: u64 = 120;
/// The branches whose commits on a remote are in production.
const PROD_BRANCHES: [&str; 4] = ["main", "master", "prod", "production"];
/// Tokens in a page, in a novel: for the fun of the day's figures.
const PAGE_TOKENS: u64 = 500;
const NOVEL_TOKENS: u64 = 100_000;

/// What was done, day by day.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Journal {
    /// By day ("2026-10-28").
    #[serde(default)]
    pub days: BTreeMap<String, Day>,
    /// The days whose section is in their week's note.
    #[serde(default)]
    pub written: BTreeSet<String>,
    /// Each week's note, by its Tuesday.
    #[serde(default)]
    pub notes: BTreeMap<String, Uuid>,
    /// What was written into the notes, as written: each day's section (by day), each week's total
    /// (by its Tuesday). Written again only where the note still says that: what the user changed stays.
    #[serde(default)]
    pub sections: BTreeMap<String, String>,
    #[serde(default)]
    pub totals: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Day {
    /// Seconds by project (its folder: the git repository, or the folder itself).
    #[serde(default)]
    pub projects: BTreeMap<String, u32>,
    /// Seconds by SSH server, by database (their names).
    #[serde(default)]
    pub servers: BTreeMap<String, u32>,
    #[serde(default)]
    pub databases: BTreeMap<String, u32>,
    /// Seconds in the games.
    #[serde(default)]
    pub games: u32,
    /// What its section counted, once written: for the week's total.
    #[serde(default)]
    pub facts: Option<Facts>,
    /// The seconds counted when its section was written: more since (the evening, after the recap's
    /// hour), it is written again the next day.
    #[serde(default)]
    pub written_seconds: Option<u32>,
}

impl Day {
    /// All the seconds counted.
    pub fn seconds(&self) -> u32 {
        self.projects.values().chain(self.servers.values()).chain(self.databases.values()).sum::<u32>() + self.games
    }
}

/// A day's counts.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Facts {
    pub commits: u32,
    pub prompts: u32,
    /// All the tokens Claude went through (read, from its cache, written), and those it wrote.
    pub tokens: u64,
    pub written_tokens: u64,
    /// The tokens by project name.
    #[serde(default)]
    pub project_tokens: BTreeMap<String, u64>,
    /// The lines the commits added and removed.
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub removed: u32,
    /// The first and the last commit's time ("08:12").
    #[serde(default)]
    pub first: Option<String>,
    #[serde(default)]
    pub last: Option<String>,
    /// What Claude's tokens would have cost at the API's prices, in euros (every account together).
    #[serde(default)]
    pub euros: f64,
    /// The same by account ("Pro", "Team Caplaser").
    #[serde(default)]
    pub accounts: BTreeMap<String, f64>,
    /// The electricity Claude took, roughly, in watt-hours.
    #[serde(default)]
    pub wh: f64,
    /// The time of the projects Ronnie didn't count, from Claude's sessions, by project name.
    #[serde(default)]
    pub estimated: BTreeMap<String, u32>,
}

/// Where the time goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Project(PathBuf),
    Server(String),
    Database(String),
    Games,
}

pub fn path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("recap.json"))
}

pub fn load() -> Journal {
    crate::config::load(path()).unwrap_or_default()
}

pub fn save(journal: &Journal) {
    if let Some(path) = path()
        && let Err(e) = crate::config::save(&path, journal)
    {
        crate::log::error(&format!("recap: {e:#}"));
    }
}

pub fn key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn parse_key(key: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(key, "%Y-%m-%d").ok()
}

impl Journal {
    /// `seconds` spent at `place` on `date`.
    pub fn add(&mut self, date: NaiveDate, place: &Place, seconds: u32) {
        let day = self.days.entry(key(date)).or_default();
        let (map, name) = match place {
            Place::Project(path) => (&mut day.projects, path.display().to_string()),
            Place::Server(name) => (&mut day.servers, name.clone()),
            Place::Database(name) => (&mut day.databases, name.clone()),
            Place::Games => {
                day.games += seconds;
                return;
            }
        };
        *map.entry(name).or_default() += seconds;
    }

    /// Ronnie was open on `date`: the day gets its section (with the commits and Claude's work) even
    /// if no time was counted in it, and so do the days of its week before it (the recap turned on in
    /// the week, Ronnie closed some days: caught up from git and Claude's sessions). The old days go.
    pub fn seen(&mut self, date: NaiveDate) {
        let mut day = week_start(date);
        while day <= date {
            self.days.entry(key(day)).or_default();
            day += Duration::days(1);
        }
        let oldest = key(date - Duration::days(KEEP_DAYS));
        self.days.retain(|k, _| *k >= oldest);
        self.written.retain(|k| *k >= oldest);
        self.sections.retain(|k, _| *k >= oldest);
        let oldest_week = key(week_start(date) - Duration::days(KEEP_DAYS));
        self.notes.retain(|k, _| *k >= oldest_week);
        self.totals.retain(|k, _| *k >= oldest_week);
    }

    /// The days to write now (at `hour` o'clock): those before today not written yet (Ronnie was closed
    /// at that hour) or worked on again after (the evening), and today once the hour has come. The
    /// oldest first, within the last two weeks.
    pub fn due(&self, now: chrono::NaiveDateTime, hour: u32) -> Vec<NaiveDate> {
        let today = now.date();
        let from = today - Duration::days(14);
        self.days
            .iter()
            .filter_map(|(k, day)| Some((parse_key(k)?, day)))
            .filter(|(d, day)| {
                if *d < from {
                    return false;
                }
                if self.written.contains(&key(*d)) {
                    return *d < today && day.written_seconds.is_some_and(|s| day.seconds() >= s + 60);
                }
                *d < today || now.hour() >= hour
            })
            .map(|(d, _)| d)
            .collect()
    }
}

/// The Tuesday the week of `date` starts on (the meeting's day: its work goes into the new week).
pub fn week_start(date: NaiveDate) -> NaiveDate {
    let back = (date.weekday().num_days_from_monday() + 7 - Weekday::Tue.num_days_from_monday()) % 7;
    date - Duration::days(back as i64)
}

const DAYS_FR: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const MONTHS_FR: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const DAYS_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn day_name(date: NaiveDate, lang: Lang) -> &'static str {
    let k = date.weekday().num_days_from_monday() as usize;
    if lang == Lang::Fr { DAYS_FR[k] } else { DAYS_EN[k] }
}

fn month_name(date: NaiveDate, lang: Lang) -> &'static str {
    let k = date.month0() as usize;
    if lang == Lang::Fr { MONTHS_FR[k] } else { MONTHS_EN[k] }
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The note's title: "Récap semaine du mardi 21 au mardi 28 octobre".
pub fn title(week: NaiveDate, lang: Lang) -> String {
    let end = week + Duration::days(7);
    match lang {
        Lang::Fr if week.month() == end.month() => format!("Récap semaine du mardi {} au mardi {} {}", week.day(), end.day(), month_name(end, lang)),
        Lang::Fr => format!("Récap semaine du mardi {} {} au mardi {} {}", week.day(), month_name(week, lang), end.day(), month_name(end, lang)),
        Lang::En => format!("Week recap, Tuesday {} {} to Tuesday {} {}", month_name(week, lang), week.day(), month_name(end, lang), end.day()),
    }
}

/// A day's heading: "## Mardi 28 octobre".
pub fn day_heading(date: NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::Fr => format!("## {} {} {}", capitalized(day_name(date, lang)), date.day(), month_name(date, lang)),
        Lang::En => format!("## {} {} {}", day_name(date, lang), month_name(date, lang), date.day()),
    }
}

fn total_heading(lang: Lang) -> &'static str {
    if lang == Lang::Fr { TOTAL_HEADINGS[0] } else { TOTAL_HEADINGS[1] }
}

/// The total's heading in each language: one written in the other stays the total (the language
/// changed in the week).
const TOTAL_HEADINGS: [&str; 2] = ["## Bilan de la semaine", "## The week in total"];

/// "3 h 40", "25 min".
fn duration(seconds: u32) -> String {
    let minutes = (seconds + 30) / 60;
    match minutes {
        0..60 => format!("{minutes} min"),
        _ if minutes.is_multiple_of(60) => format!("{} h", minutes / 60),
        _ => format!("{} h {:02}", minutes / 60, minutes % 60),
    }
}

/// "48,20 €", "€48.20".
fn euros(n: f64, lang: Lang) -> String {
    if lang == Lang::Fr { format!("{n:.2} €").replace('.', ",") } else { format!("€{n:.2}") }
}

/// "1,2 M", "850 k" (French decimals).
fn tokens(n: u64, lang: Lang) -> String {
    let text = match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{} k", n / 1_000),
        _ => format!("{:.1} M", n as f64 / 1_000_000.0),
    };
    if lang == Lang::Fr { text.replace('.', ",") } else { text }
}

/// A project's name: its folder's.
fn name_of(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_owned())
}

/// Two writings of a folder as one (Windows: any case, either slash).
fn same_key(path: &str) -> String {
    if cfg!(windows) { path.replace('/', "\\").trim_end_matches('\\').to_lowercase() } else { path.trim_end_matches('/').to_owned() }
}

/// "s" when more than one.
fn plural(n: impl Into<u64>) -> &'static str {
    if n.into() > 1 { "s" } else { "" }
}

/// "08:12" as "8 h 12".
fn clock(time: &str, lang: Lang) -> String {
    match (lang, time.split_once(':')) {
        (Lang::Fr, Some((h, m))) => format!("{} h {m}", h.parse::<u32>().unwrap_or(0)),
        _ => time.to_owned(),
    }
}

/// How far a commit went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    /// Only on this computer.
    Local,
    /// On a remote, on another branch than production's.
    Pushed,
    /// On a remote's production branch.
    Prod,
}

#[derive(Clone, Debug)]
struct Commit {
    subject: String,
    /// "08:12", the author's.
    time: String,
    /// The author's date (Unix seconds): with the subject, the same commit cherry-picked or rebased.
    authored: i64,
    stage: Stage,
    /// The branch it was found on.
    branch: String,
    added: u32,
    removed: u32,
}

/// A repository's day in git.
#[derive(Default, Debug)]
struct Git {
    commits: Vec<Commit>,
    /// It has a remote: a commit not on one is "not pushed yet".
    remote: bool,
    /// Today only: the files changed and not committed, the lines added and removed in them.
    uncommitted: Option<(u32, u32, u32)>,
}

/// A project's day.
#[derive(Default, Debug)]
struct Work {
    path: String,
    seconds: u32,
    git: Git,
    prompts: u32,
    tokens: u64,
    written_tokens: u64,
    /// At the API's prices, in dollars, by account.
    costs: BTreeMap<String, f64>,
    /// Electricity, in watt-hours.
    wh: f64,
    /// Claude's time at it, from its sessions.
    claude_seconds: u32,
}

impl Work {
    /// Its time: Ronnie's if counted, otherwise Claude's.
    fn time(&self) -> u32 {
        if self.seconds >= 60 { self.seconds } else { self.claude_seconds }
    }
}

/// A day's section, ready for the note, and what it counted.
pub struct Report {
    pub date: NaiveDate,
    pub section: String,
    pub facts: Facts,
    /// Nothing done that day: no project, server, database or game.
    pub empty: bool,
}

/// The section of `date`: its time from the journal's `day`, its commits from git (where they went:
/// production, another branch, not pushed yet, and today what isn't committed; in French, translated
/// by Claude), then the day's figures, Claude's from the sessions of every account of Claude Code on
/// this computer. The projects worked on in Ronnie that day and those Claude worked in (their time
/// then Claude's, when Ronnie didn't count it). Runs git and Claude, reaches the European Central
/// Bank: off the interface's thread.
pub fn report(date: NaiveDate, day: &Day, lang: Lang) -> Report {
    let mut works: HashMap<String, Work> = HashMap::new();
    for (path, seconds) in day.projects.iter().filter(|(_, s)| **s >= 60) {
        let work = works.entry(same_key(path)).or_insert_with(|| Work { path: path.clone(), ..Default::default() });
        work.seconds += seconds;
    }
    // Claude's own translations, run from the temporary folder, are no project.
    let temp = same_key(&std::env::temp_dir().display().to_string());
    for (cwd, session) in claude_day(&accounts(), date) {
        if same_key(&cwd).starts_with(&temp) {
            continue;
        }
        let root = crate::worklog::project_of(Path::new(&cwd)).display().to_string();
        let work = works.entry(same_key(&root)).or_insert_with(|| Work { path: root.clone(), ..Default::default() });
        work.prompts += session.prompts;
        work.tokens += session.tokens;
        work.written_tokens += session.written_tokens;
        for (account, cost) in session.costs {
            *work.costs.entry(account).or_default() += cost;
        }
        work.wh += session.wh;
        work.claude_seconds += session.active;
    }
    let today = Local::now().date_naive() == date;
    for work in works.values_mut() {
        work.git = git_day(Path::new(&work.path), date, today);
    }
    let mut cache = load_cache();
    let usd_per_eur = cache.usd_per_eur();
    if lang == Lang::Fr {
        let subjects: Vec<&str> = works.values().flat_map(|w| w.git.commits.iter().map(|c| c.subject.as_str())).collect();
        cache.translate(&subjects);
        for commit in works.values_mut().flat_map(|w| w.git.commits.iter_mut()) {
            if let Some(fr) = cache.fr.get(&commit.subject) {
                commit.subject = fr.clone();
            }
        }
    }
    save_cache(&cache);
    // A folder with nothing to say (Claude only asked a question there, in under a minute) stays out.
    let mut works: Vec<Work> = works.into_values().filter(|w| w.seconds >= 60 || w.claude_seconds >= 60 || !w.git.commits.is_empty() || w.git.uncommitted.is_some()).collect();
    works.sort_by(|a, b| b.time().cmp(&a.time()).then(b.git.commits.len().cmp(&a.git.commits.len())).then(b.tokens.cmp(&a.tokens)));

    let fr = lang == Lang::Fr;
    let mut out = vec![day_heading(date, lang), String::new()];
    let mut facts = Facts::default();
    for work in &works {
        let mut head = format!("**{}**", name_of(&work.path));
        if work.seconds >= 60 {
            head += &format!(" · {}", duration(work.seconds));
        } else if work.claude_seconds >= 60 {
            // Not counted in Ronnie (closed, the recap off, another terminal): Claude's time at it.
            head += &if fr { format!(" · ≈ {} avec Claude", duration(work.claude_seconds)) } else { format!(" · ≈ {} with Claude", duration(work.claude_seconds)) };
            facts.estimated.insert(name_of(&work.path), work.claude_seconds);
        }
        out.push(head);
        // The commits by how far they went (production first), then by branch, each group in order.
        let mut groups: Vec<((Stage, String), Vec<&Commit>)> = Vec::new();
        for commit in &work.git.commits {
            let key = (commit.stage, if commit.stage == Stage::Prod { String::new() } else { commit.branch.clone() });
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => list.push(commit),
                None => groups.push((key, vec![commit])),
            }
        }
        groups.sort_by_key(|((stage, _), _)| std::cmp::Reverse(*stage));
        for ((stage, branch), commits) in &groups {
            let branch = if branch.is_empty() { String::new() } else { format!(" ({branch})") };
            out.push(match (stage, work.git.remote, fr) {
                (Stage::Prod, _, true) => "🚀 En prod :".to_owned(),
                (Stage::Prod, _, false) => "🚀 In production:".to_owned(),
                (Stage::Pushed, _, true) => format!("🛠️ En dev{branch} :"),
                (Stage::Pushed, _, false) => format!("🛠️ In development{branch}:"),
                (Stage::Local, true, true) => format!("💻 Pas encore poussé{branch} :"),
                (Stage::Local, true, false) => format!("💻 Not pushed yet{branch}:"),
                (Stage::Local, false, true) => format!("💻 Commits{branch} :"),
                (Stage::Local, false, false) => format!("💻 Commits{branch}:"),
            });
            out.extend(commits.iter().map(|c| format!("- {}", c.subject)));
        }
        if let Some((files, added, removed)) = work.git.uncommitted {
            // New files only (not followed by git yet): no lines to say.
            let lines = if added + removed > 0 { format!(" (+{added} / −{removed})") } else { String::new() };
            out.push(if fr {
                format!("✏️ En cours, pas commité : {files} fichier{}{lines}", plural(files))
            } else {
                format!("✏️ Under way, not committed: {files} file{}{lines}", plural(files))
            });
        }
        out.push(String::new());
        facts.commits += work.git.commits.len() as u32;
        facts.prompts += work.prompts;
        facts.tokens += work.tokens;
        facts.written_tokens += work.written_tokens;
        for (account, cost) in &work.costs {
            facts.euros += cost / usd_per_eur;
            *facts.accounts.entry(account.clone()).or_default() += cost / usd_per_eur;
        }
        facts.wh += work.wh;
        if work.tokens > 0 {
            *facts.project_tokens.entry(name_of(&work.path)).or_default() += work.tokens;
        }
        for commit in &work.git.commits {
            facts.added += commit.added;
            facts.removed += commit.removed;
            if facts.first.as_ref().is_none_or(|t| commit.time < *t) {
                facts.first = Some(commit.time.clone());
            }
            if facts.last.as_ref().is_none_or(|t| commit.time > *t) {
                facts.last = Some(commit.time.clone());
            }
        }
    }
    let list = |map: &BTreeMap<String, u32>| {
        let mut items: Vec<(&String, &u32)> = map.iter().filter(|(_, s)| **s >= 60).collect();
        items.sort_by_key(|(_, s)| std::cmp::Reverse(**s));
        items.iter().map(|(name, s)| format!("{name} ({})", duration(**s))).collect::<Vec<_>>().join(", ")
    };
    let servers = list(&day.servers);
    if !servers.is_empty() {
        out.push(if fr { format!("**Serveurs** : {servers}") } else { format!("**Servers**: {servers}") });
    }
    let databases = list(&day.databases);
    if !databases.is_empty() {
        out.push(if fr { format!("**Bases de données** : {databases}") } else { format!("**Databases**: {databases}") });
    }
    let empty = works.is_empty() && servers.is_empty() && databases.is_empty() && day.games < 60;
    if empty {
        out.push(if fr {
            "Rien de noté pour l'instant : un projet apparaît après une minute de travail dans Ronnie, un commit ou une minute avec Claude.".to_owned()
        } else {
            "Nothing noted yet: a project shows after a minute of work in Ronnie, a commit or a minute with Claude.".to_owned()
        });
    } else {
        if out.last().is_some_and(|l| !l.is_empty()) {
            out.push(String::new());
        }
        out.extend(figures(&facts, day.seconds(), day.games, lang));
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    Report { date, section: out.join("\n"), facts, empty }
}

/// The day's figures, for fun.
fn figures(facts: &Facts, seconds: u32, games: u32, lang: Lang) -> Vec<String> {
    let fr = lang == Lang::Fr;
    let mut out = vec![if fr { "**🎲 Les chiffres du jour**".to_owned() } else { "**🎲 The day in figures**".to_owned() }];
    if seconds >= 60 {
        out.push(if fr { format!("- {} devant Ronnie", duration(seconds)) } else { format!("- {} in front of Ronnie", duration(seconds)) });
    }
    if facts.commits > 0 {
        out.push(if fr {
            format!("- {} commit{}, +{} / −{} lignes", facts.commits, plural(facts.commits), facts.added, facts.removed)
        } else {
            format!("- {} commit{}, +{} / −{} lines", facts.commits, plural(facts.commits), facts.added, facts.removed)
        });
    }
    if let (Some(first), Some(last)) = (&facts.first, &facts.last) {
        out.push(match (fr, first == last) {
            (true, true) => format!("- Le commit du jour à {}", clock(first, lang)),
            (true, false) => format!("- Premier commit à {}, dernier à {}", clock(first, lang), clock(last, lang)),
            (false, true) => format!("- The day's commit at {first}"),
            (false, false) => format!("- First commit at {first}, last at {last}"),
        });
    }
    out.extend(claude_figures(facts, lang));
    if games >= 60 {
        out.push(if fr { format!("- {} de pause dans les jeux 🎸", duration(games)) } else { format!("- {} of break in the games 🎸", duration(games)) });
    }
    if out.len() == 1 {
        out.clear();
    }
    out
}

/// Claude's lines of the figures: the requests, the tokens and what they amount to, the project that
/// took the most.
fn claude_figures(facts: &Facts, lang: Lang) -> Vec<String> {
    let fr = lang == Lang::Fr;
    let mut out = Vec::new();
    if facts.prompts > 0 {
        out.push(if fr { format!("- {} demande{} à Claude", facts.prompts, plural(facts.prompts)) } else { format!("- {} request{} to Claude", facts.prompts, plural(facts.prompts)) });
    }
    if facts.tokens > 0 {
        let pages = facts.written_tokens / PAGE_TOKENS;
        let novels = facts.tokens / NOVEL_TOKENS;
        let novels = match (novels, fr) {
            (0, _) => String::new(),
            (n, true) => format!(", de quoi lire {n} roman{}", plural(n)),
            (n, false) => format!(", enough to read {n} novel{}", plural(n)),
        };
        out.push(if fr {
            format!("- {} tokens avalés par Claude, {} écrits par lui (≈ {pages} page{}{novels})", tokens(facts.tokens, lang), tokens(facts.written_tokens, lang), plural(pages))
        } else {
            format!("- {} tokens gone through Claude, {} written by it (≈ {pages} page{}{novels})", tokens(facts.tokens, lang), tokens(facts.written_tokens, lang), plural(pages))
        });
        // By account, when there are several.
        let split = if facts.accounts.len() > 1 {
            let mut by: Vec<(&String, &f64)> = facts.accounts.iter().collect();
            by.sort_by(|a, b| b.1.total_cmp(a.1));
            let colon = if lang == Lang::Fr { " :" } else { ":" };
            format!("{colon} {}", by.iter().map(|(name, n)| format!("{name} {}", euros(**n, lang))).collect::<Vec<_>>().join(", "))
        } else {
            String::new()
        };
        out.push(if fr {
            format!("- 💶 {} si ces tokens étaient payés à l'usage (tarif API){split}", euros(facts.euros, lang))
        } else {
            format!("- 💶 {} if these tokens were paid as used (API prices){split}", euros(facts.euros, lang))
        });
        if facts.wh > 0.0 {
            out.push(ecology(facts.wh, lang));
        }
        let top = facts.project_tokens.iter().max_by_key(|(_, n)| **n);
        if let Some((name, n)) = top.filter(|_| facts.project_tokens.len() > 1) {
            let percent = (*n as f64 * 100.0 / facts.tokens as f64).round() as u32;
            out.push(if fr { format!("- Le plus gourmand : {name} ({percent} % des tokens)") } else { format!("- The hungriest: {name} ({percent} % of the tokens)") });
        }
    }
    out
}

/// What a watt-hour of Claude amounts to: a hot bath (150 L heated by 30 °C: about 5 kWh), a
/// smartphone's charge, a kilometre in an electric car; the water to cool the data centre and make its
/// electricity (about 2 L a kWh), the CO₂ of the grids it runs on (about 400 g a kWh).
const BATH_WH: f64 = 5_000.0;
const PHONE_WH: f64 = 15.0;
const EV_KM_WH: f64 = 150.0;
const WATER_L_PER_KWH: f64 = 2.0;
const CO2_G_PER_KWH: f64 = 400.0;

/// "1,5" (French decimals), one figure after the point.
fn decimal(n: f64, lang: Lang) -> String {
    let text = format!("{n:.1}");
    if lang == Lang::Fr { text.replace('.', ",") } else { text }
}

/// The ecology line of `wh` watt-hours.
fn ecology(wh: f64, lang: Lang) -> String {
    let fr = lang == Lang::Fr;
    let kwh = wh / 1000.0;
    let energy = if wh >= 1000.0 { format!("{} kWh", decimal(kwh, lang)) } else { format!("{} Wh", wh.round()) };
    let (baths, phones, km) = (wh / BATH_WH, (wh / PHONE_WH).round(), (wh / EV_KM_WH).round());
    let (water, co2) = (kwh * WATER_L_PER_KWH, kwh * CO2_G_PER_KWH);
    let co2 = if co2 >= 1000.0 { format!("{} kg", decimal(co2 / 1000.0, lang)) } else { format!("{} g", co2.round()) };
    // The comparisons big enough to say something: not "0,0 bain", "0 km".
    let mut like = Vec::new();
    if baths >= 0.05 {
        like.push(match (fr, baths >= 2.0) {
            (true, true) => format!("{} bains chauds", decimal(baths, lang)),
            (true, false) => format!("{} bain chaud", decimal(baths, lang)),
            (false, true) => format!("{} hot baths", decimal(baths, lang)),
            (false, false) => format!("{} hot bath", decimal(baths, lang)),
        });
    }
    if phones >= 1.0 {
        like.push(if fr { format!("{phones} charge{} de smartphone", plural(phones as u64)) } else { format!("{phones} smartphone charge{}", plural(phones as u64)) });
    }
    if km >= 1.0 {
        like.push(if fr { format!("{km} km en voiture électrique") } else { format!("{km} km in an electric car") });
    }
    let or = if fr { " ou " } else { " or " };
    let like = match like.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{}{or}{last}", rest.join(", ")),
    };
    let water = if water >= 0.1 { format!("{} L", decimal(water, lang)) } else { format!("{} cL", (water * 100.0).round()) };
    match (fr, like.is_empty()) {
        (true, false) => format!("- 🌍 ≈ {energy} d'électricité : autant que {like} ; ≈ {water} d'eau et {co2} de CO₂"),
        (true, true) => format!("- 🌍 ≈ {energy} d'électricité ; ≈ {water} d'eau et {co2} de CO₂"),
        (false, false) => format!("- 🌍 ≈ {energy} of electricity: as much as {like}; ≈ {water} of water and {co2} of CO₂"),
        (false, true) => format!("- 🌍 ≈ {energy} of electricity; ≈ {water} of water and {co2} of CO₂"),
    }
}

/// The week's total, from the days of the journal in the week of `week`.
pub fn total(journal: &Journal, week: NaiveDate, lang: Lang) -> String {
    let fr = lang == Lang::Fr;
    let mut projects: BTreeMap<String, u32> = BTreeMap::new();
    let mut facts = Facts::default();
    let mut games = 0;
    // The busiest day, the earliest commit, the latest.
    let mut busiest: Option<(NaiveDate, u32)> = None;
    let mut early: Option<(NaiveDate, String)> = None;
    let mut late: Option<(NaiveDate, String)> = None;
    for k in 0..7 {
        let date = week + Duration::days(k);
        let Some(day) = journal.days.get(&key(date)) else { continue };
        for (path, seconds) in &day.projects {
            *projects.entry(name_of(path)).or_default() += seconds;
        }
        games += day.games;
        if day.seconds() >= 60 && busiest.is_none_or(|(_, s)| day.seconds() > s) {
            busiest = Some((date, day.seconds()));
        }
        if let Some(f) = &day.facts {
            facts.commits += f.commits;
            facts.prompts += f.prompts;
            facts.tokens += f.tokens;
            facts.written_tokens += f.written_tokens;
            facts.added += f.added;
            facts.removed += f.removed;
            facts.euros += f.euros;
            facts.wh += f.wh;
            for (name, n) in &f.accounts {
                *facts.accounts.entry(name.clone()).or_default() += n;
            }
            for (name, seconds) in &f.estimated {
                *projects.entry(name.clone()).or_default() += seconds;
            }
            for (name, n) in &f.project_tokens {
                *facts.project_tokens.entry(name.clone()).or_default() += n;
            }
            if let Some(first) = &f.first
                && early.as_ref().is_none_or(|(_, t)| first < t)
            {
                early = Some((date, first.clone()));
            }
            if let Some(last) = &f.last
                && late.as_ref().is_none_or(|(_, t)| last > t)
            {
                late = Some((date, last.clone()));
            }
        }
    }
    let mut out = vec![total_heading(lang).to_owned(), String::new()];
    let mut by: Vec<(String, u32)> = projects.into_iter().filter(|(_, s)| *s >= 60).collect();
    let time: u32 = by.iter().map(|(_, s)| s).sum();
    if !by.is_empty() {
        by.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
        let names = by.iter().map(|(n, s)| format!("{n} {}", duration(*s))).collect::<Vec<_>>().join(", ");
        let n = by.len() as u64;
        out.push(if fr { format!("- **Temps** : {} sur {n} projet{} ({names})", duration(time), plural(n)) } else { format!("- **Time**: {} on {n} project{} ({names})", duration(time), plural(n)) });
    }
    if facts.commits > 0 {
        out.push(if fr {
            format!("- **Commits** : {}, +{} / −{} lignes", facts.commits, facts.added, facts.removed)
        } else {
            format!("- **Commits**: {}, +{} / −{} lines", facts.commits, facts.added, facts.removed)
        });
    }
    if let Some((date, seconds)) = busiest {
        out.push(if fr { format!("- **Jour le plus chargé** : {} ({})", day_name(date, lang), duration(seconds)) } else { format!("- **Busiest day**: {} ({})", day_name(date, lang), duration(seconds)) });
    }
    if let Some((date, time)) = &early {
        out.push(if fr { format!("- **Lève-tôt** : un commit {} à {}", day_name(*date, lang), clock(time, lang)) } else { format!("- **Early bird**: a commit on {} at {time}", day_name(*date, lang)) });
    }
    if let Some((date, time)) = late.as_ref().filter(|l| Some(*l) != early.as_ref()) {
        out.push(if fr { format!("- **Couche-tard** : un commit {} à {}", day_name(*date, lang), clock(time, lang)) } else { format!("- **Night owl**: a commit on {} at {time}", day_name(*date, lang)) });
    }
    out.extend(claude_figures(&facts, lang));
    if games >= 60 {
        out.push(if fr { format!("- **Pause** : {} dans les jeux 🎸", duration(games)) } else { format!("- **Break**: {} in the games 🎸", duration(games)) });
    }
    if out.len() == 2 {
        out.push(if fr { "Rien de noté pour l'instant.".to_owned() } else { "Nothing noted yet.".to_owned() });
    }
    out.push(String::new());
    out.push(if fr { "_Ronnie complète cette note chaque soir ; ce bilan est réécrit à chaque fois._".to_owned() } else { "_Ronnie adds to this note each evening; this total is written again each time._".to_owned() });
    if facts.wh > 0.0 {
        out.push(if fr {
            "_🌍 Ordre de grandeur : Anthropic ne publie pas la consommation de Claude, Ronnie part des estimations publiques (≈ 0,3 à 0,5 Wh pour une réponse de chatbot)._".to_owned()
        } else {
            "_🌍 An order of magnitude: Anthropic publishes nothing of Claude's consumption, Ronnie goes by the public estimates (≈ 0.3 to 0.5 Wh for a chatbot's answer)._".to_owned()
        });
    }
    out.join("\n")
}

/// A note with a day written into it.
#[derive(Debug, PartialEq)]
pub struct Upserted {
    pub text: String,
    /// The day's section, the week's total went in (not where the user changed Ronnie's text).
    pub section: bool,
    pub total: bool,
}

/// The note's text with the day's `section` in it and the week's `total` after. Ronnie's former text
/// (`old_section`, `old_total`: as it wrote them) is replaced where the note still says it; a day the
/// user changed, a total the user changed, stay theirs (no other is added). A new day goes before the
/// total. Whatever else the user wrote, wherever, stays.
pub fn upsert(text: &str, title: &str, section: &str, old_section: Option<&str>, total: &str, old_total: Option<&str>) -> Upserted {
    let mut lines: Vec<String> = if text.trim().is_empty() { vec![format!("# {title}")] } else { text.lines().map(str::to_owned).collect() };
    let block = |s: &str| s.lines().map(str::to_owned).collect::<Vec<_>>();
    let find = |lines: &[String], block: &[String]| -> Option<usize> {
        if block.is_empty() || block.len() > lines.len() {
            return None;
        }
        (0..=lines.len() - block.len()).find(|&at| lines[at..at + block.len()].iter().zip(block).all(|(a, b)| a.trim_end() == b.trim_end()))
    };
    // Where a new day goes, and the total: in place of the former total, before the one the user
    // changed, or at the end.
    let (mut anchor, mut put_total) = (lines.len(), true);
    if let Some((at, len)) = old_total.map(block).and_then(|old| Some((find(&lines, &old)?, old.len()))) {
        lines.drain(at..at + len);
        anchor = at;
    } else if let Some(at) = lines.iter().position(|l| TOTAL_HEADINGS.contains(&l.trim_end())) {
        anchor = at;
        put_total = false;
    }
    let new = block(section);
    let heading = new.first().cloned().unwrap_or_default();
    let mut put_section = true;
    if let Some((at, len)) = old_section.map(block).and_then(|old| Some((find(&lines, &old)?, old.len()))) {
        lines.splice(at..at + len, new.clone());
        if anchor > at {
            anchor = anchor + new.len() - len;
        }
    } else if lines.iter().any(|l| l.trim_end() == heading) {
        put_section = false;
    } else {
        anchor += insert(&mut lines, anchor, &new);
    }
    if put_total {
        insert(&mut lines, anchor, &block(total));
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    Upserted { text: lines.join("\n") + "\n", section: put_section, total: put_total }
}

/// `block` into `lines` at `at`, a blank line on each side. The lines added.
fn insert(lines: &mut Vec<String>, at: usize, block: &[String]) -> usize {
    let mut new = Vec::new();
    if at > 0 && !lines[at - 1].trim().is_empty() {
        new.push(String::new());
    }
    new.extend(block.iter().cloned());
    if at < lines.len() && !lines[at].trim().is_empty() {
        new.push(String::new());
    }
    let n = new.len();
    lines.splice(at..at, new);
    n
}

/// Git in the repository at `root`: its output, None if it failed.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(["-c", "log.showSignature=false", "-c", "core.quotePath=false"]).args(args).env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0").stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The lines added and removed in a `--numstat` output (a binary file's are "-").
fn numstat<'a>(lines: impl Iterator<Item = &'a str>) -> (u32, u32) {
    lines.fold((0, 0), |(added, removed), line| {
        let mut cols = line.split('\t');
        let mut n = || cols.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
        (added + n(), removed + n())
    })
}

/// The user's own commits of `date` in the repository at `root`, the oldest first, and how far each
/// went; `today`, what isn't committed. Nothing outside a repository.
fn git_day(root: &Path, date: NaiveDate, today: bool) -> Git {
    let mut day = Git::default();
    if !root.join(".git").exists() {
        return day;
    }
    let Some(email) = git(root, &["config", "user.email"]).map(|e| e.trim().to_owned()).filter(|e| !e.is_empty()) else { return day };
    // Written that day: by the author's date (a commit rebased or amended later keeps it), looked for
    // among those committed since, on every branch but not in the stash (its commits aren't work
    // done). The email as it is, not a pattern ("jean+work@..."), and whole: "<jean@x.fr>" isn't
    // "ajean@x.fr".
    let since = format!("--since={} 00:00:00", date.format("%Y-%m-%d"));
    let author = format!("--author=<{email}>");
    let args = ["log", "--exclude=refs/stash", "--all", "--source", "--no-merges", "--reverse", "--fixed-strings", &author, &since, "--date=format-local:%Y-%m-%d %H:%M", "--numstat", "--format=%x1e%H%x1f%ad%x1f%at%x1f%S%x1f%s"];
    let Some(out) = git(root, &args) else { return day };
    // Those on a remote, and on a production branch of one.
    let remotes: Vec<String> = git(root, &["for-each-ref", "--format=%(refname)", "refs/remotes"]).unwrap_or_default().lines().filter(|r| !r.ends_with("/HEAD")).map(str::to_owned).collect();
    day.remote = !remotes.is_empty();
    let reachable = |refs: Vec<&str>| -> BTreeSet<String> {
        if refs.is_empty() {
            return BTreeSet::new();
        }
        let mut args = vec!["log", "--format=%H", &since];
        args.extend(refs);
        git(root, &args).unwrap_or_default().lines().map(str::to_owned).collect()
    };
    let prod = reachable(remotes.iter().map(String::as_str).filter(|r| r.rsplit('/').next().is_some_and(|b| PROD_BRANCHES.contains(&b))).collect());
    let pushed = reachable(remotes.iter().map(String::as_str).collect());
    let day_prefix = date.format("%Y-%m-%d ").to_string();
    for chunk in out.split('\x1e') {
        let mut lines = chunk.lines();
        let Some(head) = lines.next() else { continue };
        let fields: Vec<&str> = head.splitn(5, '\x1f').collect();
        let [hash, at, authored, source, subject] = fields[..] else { continue };
        let Some(time) = at.strip_prefix(&day_prefix) else { continue };
        let subject = subject.trim();
        if subject.is_empty() {
            continue;
        }
        let (added, removed) = numstat(lines);
        let stage = if prod.contains(hash) {
            Stage::Prod
        } else if pushed.contains(hash) {
            Stage::Pushed
        } else {
            Stage::Local
        };
        let commit = Commit { subject: subject.to_owned(), time: time.to_owned(), authored: authored.parse().unwrap_or(0), stage, branch: branch_of(source), added, removed };
        // The same commit twice (cherry-picked, rebased onto another branch: its subject and author's
        // date kept): once, where it went farthest. Two commits named alike ("fix") stay two.
        match day.commits.iter_mut().find(|c| c.subject == commit.subject && c.authored == commit.authored) {
            Some(c) if commit.stage > c.stage => *c = commit,
            Some(_) => {}
            None => day.commits.push(commit),
        }
    }
    if today {
        let files = git(root, &["status", "--porcelain"]).map_or(0, |s| s.lines().count() as u32);
        if files > 0 {
            let (added, removed) = numstat(git(root, &["diff", "HEAD", "--numstat"]).unwrap_or_default().lines());
            day.uncommitted = Some((files, added, removed));
        }
    }
    day
}

/// The branch of the ref a commit was found on: "refs/heads/runner" and "refs/remotes/origin/runner"
/// are "runner".
fn branch_of(source: &str) -> String {
    if let Some(branch) = source.strip_prefix("refs/heads/") {
        return branch.to_owned();
    }
    if let Some(rest) = source.strip_prefix("refs/remotes/") {
        // Without its remote's name (the branch's own may hold slashes: "feature/cart").
        return rest.split_once('/').map_or(rest, |(_, branch)| branch).to_owned();
    }
    source.strip_prefix("refs/").unwrap_or(source).to_owned()
}

/// A Claude Code account on this computer: where it keeps its sessions, and its name for the note
/// ("Pro", "Team Caplaser").
#[derive(Clone, Debug)]
pub struct Account {
    pub projects: PathBuf,
    pub name: String,
}

/// Every Claude Code account on this computer: its own folder (`CLAUDE_CONFIG_DIR`, otherwise
/// `~/.claude`) and the other `~/.claude*` folders holding sessions (`~/.claude-pro`: another
/// account, used with its own `CLAUDE_CONFIG_DIR`).
pub fn accounts() -> Vec<Account> {
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    let mut dirs: Vec<PathBuf> = crate::claude::config_dir().into_iter().collect();
    if let Some(home) = &home
        && let Ok(entries) = std::fs::read_dir(home)
    {
        let mut others: Vec<PathBuf> = entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(".claude")).map(|e| e.path()).filter(|p| p.join("projects").is_dir()).collect();
        others.sort();
        dirs.extend(others);
    }
    let mut seen = BTreeSet::new();
    dirs.retain(|d| seen.insert(same_key(&d.display().to_string())));
    let mut accounts: Vec<Account> = dirs.iter().map(|d| Account { projects: d.join("projects"), name: account_name(d, home.as_deref()) }).collect();
    // Two of the same kind (two Pro accounts): told apart by their folder.
    for i in 0..accounts.len() {
        if accounts.iter().filter(|a| a.name == accounts[i].name).count() > 1 {
            let folder = dirs[i].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            accounts[i].name = format!("{} ({folder})", accounts[i].name);
        }
    }
    accounts
}

/// An account's name, from what Claude Code knows of it (`.claude.json`: in its folder, or beside it
/// for `~/.claude`): its plan, and for a team its organization's name. Its folder's otherwise.
fn account_name(dir: &Path, home: Option<&Path>) -> String {
    let mut files = vec![dir.join(".claude.json")];
    if let Some(home) = home
        && same_key(&dir.display().to_string()) == same_key(&home.join(".claude").display().to_string())
    {
        files.insert(0, home.join(".claude.json"));
    }
    let folder = || dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(account) = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).filter_map(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).map(|v| v["oauthAccount"].clone()).find(|a| a.is_object()) else {
        return folder();
    };
    let plan = match account["organizationType"].as_str().unwrap_or_default() {
        "claude_pro" => "Pro",
        "claude_max" => "Max",
        "claude_team" => "Team",
        "claude_enterprise" => "Enterprise",
        _ => return folder(),
    };
    let org = account["organizationName"].as_str().unwrap_or_default();
    // A personal plan's organization is "someone's Organization": not said.
    if matches!(plan, "Team" | "Enterprise") && !org.is_empty() { format!("{plan} {org}") } else { plan.to_owned() }
}

/// A day of Claude in a folder.
#[derive(Default, Debug, PartialEq)]
struct ClaudeDay {
    /// The requests typed.
    prompts: u32,
    tokens: u64,
    written_tokens: u64,
    /// At the API's prices, in dollars: all of it, and by account.
    cost: f64,
    costs: BTreeMap<String, f64>,
    /// The electricity it took, roughly, in watt-hours.
    wh: f64,
    /// Seconds Claude was at work or talked to: from one line to the next, when they're close.
    active: u32,
}

/// Two lines of a session further apart than this (seconds): a break, not counted.
const CLAUDE_GAP: i64 = 5 * 60;

/// A message of Claude.
struct Usage {
    cwd: String,
    account: String,
    /// The tokens: all, those Claude wrote.
    all: u64,
    written: u64,
    cost: f64,
    wh: f64,
}

/// What was read of the sessions: the same message in two of them (a session resumed copies the one
/// before) counts once.
#[derive(Default)]
struct Sessions {
    days: BTreeMap<String, ClaudeDay>,
    /// Each message's usage, by its id: a message streamed in several lines counts once, with its last
    /// usage.
    usage: HashMap<String, Usage>,
    /// The requests counted, by their line's id.
    prompts: std::collections::HashSet<String>,
    /// The times of the lines (Unix seconds), by folder: the same in a session resumed counts once.
    stamps: HashMap<String, BTreeSet<i64>>,
}

/// Claude Code's work of `date` (local time), by folder, from the sessions of `accounts`: the
/// requests typed, the tokens, their cost. Only the sessions written to since that day are read.
fn claude_day(accounts: &[Account], date: NaiveDate) -> BTreeMap<String, ClaudeDay> {
    let mut sessions = Sessions::default();
    let Some(start) = date.and_hms_opt(0, 0, 0).and_then(|t| t.and_local_timezone(Local).single()) else { return sessions.days };
    let start: std::time::SystemTime = start.into();
    for account in accounts {
        let Ok(projects) = std::fs::read_dir(&account.projects) else { continue };
        for project in projects.flatten() {
            let Ok(files) = std::fs::read_dir(project.path()) else { continue };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|e| e != "jsonl") || file.metadata().and_then(|m| m.modified()).is_ok_and(|m| m < start) {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    read_session(&text, date, &account.name, &mut sessions);
                }
            }
        }
    }
    sessions.finish()
}

impl Sessions {
    /// The tokens counted into the days.
    fn finish(mut self) -> BTreeMap<String, ClaudeDay> {
        for usage in self.usage.into_values() {
            let day = self.days.entry(usage.cwd).or_default();
            day.tokens += usage.all;
            day.written_tokens += usage.written;
            day.cost += usage.cost;
            *day.costs.entry(usage.account).or_default() += usage.cost;
            day.wh += usage.wh;
        }
        for (cwd, stamps) in self.stamps {
            let gaps = stamps.iter().zip(stamps.iter().skip(1)).map(|(a, b)| b - a).filter(|gap| *gap <= CLAUDE_GAP);
            self.days.entry(cwd).or_default().active += gaps.sum::<i64>() as u32;
        }
        self.days
    }
}

/// The electricity a token takes in a data centre, in watt-hours, roughly: Anthropic publishes none.
/// The public estimates put a chatbot's answer (some 500 tokens written, by a model of the size of
/// Opus) around 0.3 to 0.5 Wh (Epoch AI 2025, Google 2025 for Gemini): about 0.001 Wh a token written.
/// A token read is taken in parallel with the others, about ten times cheaper; one read again from
/// the cache, about a hundred times.
const WH_WRITTEN: f64 = 0.001;
const WH_READ: f64 = 0.0001;
const WH_CACHED: f64 = 0.000_01;

/// A message's electricity in watt-hours, from its `usage`.
fn energy(usage: &serde_json::Value) -> f64 {
    let n = |k: &str| usage[k].as_u64().unwrap_or(0) as f64;
    n("output_tokens") * WH_WRITTEN + (n("input_tokens") + n("cache_creation_input_tokens")) * WH_READ + n("cache_read_input_tokens") * WH_CACHED
}

/// A model's prices, in dollars per million tokens: input, output, cache read (a cache write is the
/// input's, × 1.25 for five minutes, × 2 for an hour). None for an unknown one ("<synthetic>": Claude
/// Code's own messages, not billed).
fn prices(model: &str) -> Option<(f64, f64, f64)> {
    const PRICES: [(&str, (f64, f64, f64)); 16] = [
        ("fable-5-1", (10.0, 50.0, 0.25)),
        ("mythos-5-1", (10.0, 50.0, 0.25)),
        ("fable-5", (10.0, 50.0, 1.0)),
        ("mythos-5", (10.0, 50.0, 1.0)),
        ("opus-5-5", (4.0, 20.0, 0.20)),
        ("opus-5", (5.0, 25.0, 0.50)),
        ("opus-4-8", (5.0, 25.0, 0.50)),
        ("opus-4-7", (5.0, 25.0, 0.50)),
        ("opus-4-6", (5.0, 25.0, 0.50)),
        ("opus-4-5", (5.0, 25.0, 0.50)),
        ("opus-4", (15.0, 75.0, 1.50)),
        ("sonnet-5", (2.0, 10.0, 0.20)),
        ("sonnet-4", (3.0, 15.0, 0.30)),
        ("haiku-5-5", (0.10, 0.50, 0.01)),
        ("haiku-4-5", (1.0, 5.0, 0.10)),
        ("haiku-3-5", (0.80, 4.0, 0.08)),
    ];
    // The first that fits: "opus-5-5" before "opus-5".
    PRICES.iter().find(|(name, _)| model.contains(name)).map(|(_, p)| *p)
}

/// A message's cost in dollars, from its `usage`.
fn cost(model: &str, usage: &serde_json::Value) -> f64 {
    let Some((input, output, read)) = prices(model) else { return 0.0 };
    let n = |k: &str| usage[k].as_u64().unwrap_or(0) as f64;
    // The cache written for an hour costs more; without the detail, all of it for five minutes.
    let hour = usage["cache_creation"]["ephemeral_1h_input_tokens"].as_u64().map_or(0.0, |n| n as f64);
    let five = (n("cache_creation_input_tokens") - hour).max(0.0);
    let dollars = (n("input_tokens") * input + n("output_tokens") * output + n("cache_read_input_tokens") * read + five * input * 1.25 + hour * input * 2.0) / 1_000_000.0;
    // Fast mode: twice the price.
    if usage["speed"].as_str() == Some("fast") { dollars * 2.0 } else { dollars }
}

/// A session's lines of `date`, of `account`, into `sessions`.
fn read_session(text: &str, date: NaiveDate, account: &str, sessions: &mut Sessions) {
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(at) = v["timestamp"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()) else { continue };
        if at.with_timezone(&Local).date_naive() != date {
            continue;
        }
        let Some(cwd) = v["cwd"].as_str() else { continue };
        if matches!(v["type"].as_str(), Some("user" | "assistant")) {
            sessions.stamps.entry(cwd.to_owned()).or_default().insert(at.timestamp());
        }
        match v["type"].as_str() {
            Some("user") => {
                // Typed by the user: not a tool's result, a command's output, Claude Code's own notes.
                let flag = |k: &str| v[k].as_bool() == Some(true);
                let human = v["origin"]["kind"].as_str().is_none_or(|k| k == "human") && !flag("isMeta") && !flag("isSidechain") && !flag("isCompactSummary");
                let Some(prompt) = v["message"]["content"].as_str().filter(|_| human) else { continue };
                let prompt = prompt.trim();
                if prompt.is_empty() || prompt.starts_with('<') || prompt.starts_with('/') || prompt.starts_with('[') {
                    continue;
                }
                // Counted in a session before (this one resumes it).
                let id = v["uuid"].as_str().map_or_else(|| format!("{cwd}\n{}\n{prompt}", at.to_rfc3339()), str::to_owned);
                if !sessions.prompts.insert(id) {
                    continue;
                }
                sessions.days.entry(cwd.to_owned()).or_default().prompts += 1;
            }
            Some("assistant") => {
                let u = &v["message"]["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                let all = n("input_tokens") + n("output_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
                let id = v["message"]["id"].as_str().map(str::to_owned).unwrap_or_else(|| v["uuid"].as_str().unwrap_or_default().to_owned());
                let cost = cost(v["message"]["model"].as_str().unwrap_or_default(), u);
                sessions.usage.insert(id, Usage { cwd: cwd.to_owned(), account: account.to_owned(), all, written: n("output_tokens"), cost, wh: energy(u) });
            }
            _ => {}
        }
    }
}

/// What the recap keeps between two days (`recap-cache.json`): the commits translated into French,
/// the dollar's rate.
#[derive(Serialize, Deserialize, Default, Debug)]
struct Cache {
    /// Dollars to the euro, and the day the European Central Bank's rate was asked for.
    #[serde(default)]
    usd_per_eur: Option<f64>,
    #[serde(default)]
    rate_day: Option<String>,
    /// Each commit's French, by its subject; the day it was last wanted.
    #[serde(default)]
    fr: BTreeMap<String, String>,
    #[serde(default)]
    used: BTreeMap<String, String>,
}

fn cache_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("recap-cache.json"))
}

fn load_cache() -> Cache {
    crate::config::load(cache_path()).unwrap_or_default()
}

fn save_cache(cache: &Cache) {
    if let Some(path) = cache_path()
        && let Err(e) = crate::config::save(&path, cache)
    {
        crate::log::error(&format!("recap: {e:#}"));
    }
}

impl Cache {
    /// Dollars to the euro: the European Central Bank's rate, asked for once a day (days caught up
    /// together ask once); the last one had, otherwise an approximate one.
    fn usd_per_eur(&mut self) -> f64 {
        let day = key(Local::now().date_naive());
        if self.rate_day.as_deref() != Some(day.as_str()) {
            self.rate_day = Some(day);
            if let Some(rate) = ecb_rate() {
                self.usd_per_eur = Some(rate);
            }
        }
        self.usd_per_eur.unwrap_or(USD_PER_EUR)
    }

    /// The `subjects` not translated yet, translated by Claude (those it didn't do stay as they are,
    /// tried again next time). The translations not wanted for two months go.
    fn translate(&mut self, subjects: &[&str]) {
        let today = key(Local::now().date_naive());
        let mut seen = BTreeSet::new();
        let missing: Vec<&str> = subjects.iter().copied().filter(|s| !self.fr.contains_key(*s) && seen.insert(*s)).collect();
        if !missing.is_empty() {
            for (subject, fr) in missing.iter().zip(translated(&missing)) {
                if let Some(fr) = fr {
                    self.fr.insert((*subject).to_owned(), fr);
                }
            }
        }
        for subject in subjects {
            self.used.insert((*subject).to_owned(), today.clone());
        }
        let oldest = key(Local::now().date_naive() - Duration::days(KEEP_DAYS));
        self.used.retain(|_, day| *day >= oldest);
        let used = &self.used;
        self.fr.retain(|subject, _| used.contains_key(subject));
    }
}

/// The dollars to the euro of the European Central Bank's rates of the day.
fn ecb_rate() -> Option<f64> {
    let agent = ureq::config::Config::builder().timeout_global(Some(std::time::Duration::from_secs(10))).build().new_agent();
    let xml = agent.get("https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml").call().ok()?.body_mut().read_to_string().ok()?;
    let at = xml.find("currency='USD'")?;
    let rest = &xml[at..];
    let rate = rest.split("rate='").nth(1)?.split('\'').next()?;
    rate.parse::<f64>().ok().filter(|r| *r > 0.0)
}

/// Claude Code, as installed: on the PATH, or where its installer puts it.
fn claude_program() -> PathBuf {
    let name = if cfg!(windows) { "claude.exe" } else { "claude" };
    let local = directories::BaseDirs::new().map(|b| b.home_dir().join(".local").join("bin").join(name));
    local.filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("claude"))
}

/// `subjects` in French, by Claude (Haiku, through Claude Code: the user's own account), in their
/// order; None where it didn't answer.
fn translated(subjects: &[&str]) -> Vec<Option<String>> {
    let mut prompt = String::from(
        "Traduis en français chacun de ces titres de commits git. Garde tels quels les noms propres, les noms de jeux, \
         de fichiers, de fonctions, le code et les touches. Un titre déjà en français reste tel quel. Réponds uniquement \
         par les traductions, une par ligne, chacune précédée de son numéro et d'un point (« 1. »), sans rien d'autre.\n\n",
    );
    for (i, subject) in subjects.iter().enumerate() {
        prompt += &format!("{}. {}\n", i + 1, subject.replace('\n', " "));
    }
    let mut out = vec![None; subjects.len()];
    // Without any tool (a commit's subject asks for nothing to be done), nor a session kept, from a
    // folder of no project all the same: it counts in no day's figures.
    let mut cmd = Command::new(claude_program());
    cmd.args(["-p", "--model", "haiku", "--output-format", "text", "--no-session-persistence", "--tools", ""]).current_dir(std::env::temp_dir()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = cmd.spawn() else { return out };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(prompt.as_bytes());
    }
    // Its answer read aside, given up after a while.
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stdout) = stdout {
            use std::io::Read;
            let _ = stdout.read_to_string(&mut text);
        }
        text
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TRANSLATE_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return out;
            }
        }
    }
    let Ok(text) = reader.join() else { return out };
    for line in text.lines() {
        let Some((n, fr)) = line.trim().split_once(". ") else { continue };
        let fr = fr.trim();
        if let Some(slot) = n.parse::<usize>().ok().and_then(|n| out.get_mut(n.wrapping_sub(1)))
            && !fr.is_empty()
        {
            *slot = Some(fr.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn a_week_runs_from_tuesday_to_tuesday() {
        // 2026-10-27 is a Tuesday.
        assert_eq!(week_start(d(2026, 10, 27)), d(2026, 10, 27));
        assert_eq!(week_start(d(2026, 10, 28)), d(2026, 10, 27));
        assert_eq!(week_start(d(2026, 11, 2)), d(2026, 10, 27));
        assert_eq!(week_start(d(2026, 11, 3)), d(2026, 11, 3));
        assert_eq!(title(d(2026, 10, 20), Lang::Fr), "Récap semaine du mardi 20 au mardi 27 octobre");
        assert_eq!(title(d(2026, 10, 27), Lang::Fr), "Récap semaine du mardi 27 octobre au mardi 3 novembre");
        assert_eq!(day_heading(d(2026, 10, 28), Lang::Fr), "## Mercredi 28 octobre");
    }

    #[test]
    fn durations_and_tokens_read_well() {
        assert_eq!(duration(25 * 60), "25 min");
        assert_eq!(duration(3 * 3600 + 40 * 60), "3 h 40");
        assert_eq!(duration(2 * 3600), "2 h");
        assert_eq!(tokens(1_234_567, Lang::Fr), "1,2 M");
        assert_eq!(tokens(850_400, Lang::Fr), "850 k");
    }

    #[test]
    fn the_days_due_wait_for_the_hour_and_catch_up() {
        let mut journal = Journal::default();
        journal.seen(d(2026, 10, 27));
        journal.seen(d(2026, 10, 28));
        let at = |h| d(2026, 10, 28).and_hms_opt(h, 0, 0).unwrap();
        // Yesterday not written (Ronnie closed at 18:00): now; today only from 18:00.
        assert_eq!(journal.due(at(9), 18), vec![d(2026, 10, 27)]);
        assert_eq!(journal.due(at(18), 18), vec![d(2026, 10, 27), d(2026, 10, 28)]);
        journal.written.insert(key(d(2026, 10, 27)));
        assert_eq!(journal.due(at(9), 18), Vec::<NaiveDate>::new());
        // Worked on in the evening after its section was written: again the next day.
        let day = journal.days.get_mut(&key(d(2026, 10, 27))).unwrap();
        day.written_seconds = Some(3600);
        day.projects.insert("C:\\www\\shop".into(), 3600 + 30);
        assert_eq!(journal.due(at(9), 18), Vec::<NaiveDate>::new());
        journal.days.get_mut(&key(d(2026, 10, 27))).unwrap().projects.insert("C:\\www\\shop".into(), 3600 + 600);
        assert_eq!(journal.due(at(9), 18), vec![d(2026, 10, 27)]);
        // Turned on a Friday: the week's Tuesday to Thursday caught up.
        let mut journal = Journal::default();
        journal.seen(d(2026, 10, 30));
        assert_eq!(journal.due(d(2026, 10, 30).and_hms_opt(9, 0, 0).unwrap(), 18), vec![d(2026, 10, 27), d(2026, 10, 28), d(2026, 10, 29)]);
    }

    #[test]
    fn a_day_goes_in_its_place_and_the_total_at_the_end() {
        let title = title(d(2026, 10, 27), Lang::Fr);
        let total = |n: u32| format!("## Bilan de la semaine\n\n- **Commits** : {n}");
        let tue = "## Mardi 27 octobre\n\n**ronnie** · 2 h";
        let done = upsert("", &title, tue, None, &total(1), None);
        assert_eq!(done, Upserted { text: format!("# {title}\n\n{tue}\n\n{}\n", total(1)), section: true, total: true });
        // The user's words, in a day and under the total, stay; the next day goes before the total,
        // the total is written again in its place.
        let text = done.text.replace("**ronnie** · 2 h", "**ronnie** · 2 h\nRéunion client, à suivre.") + "\nÀ voir avec le chef : le devis.\n";
        let wed = "## Mercredi 28 octobre\n\n**floor** · 1 h";
        let text = upsert(&text, &title, wed, None, &total(3), Some(&total(1))).text;
        assert!(text.contains(&format!("Réunion client, à suivre.\n\n{wed}\n\n{}\n\nÀ voir avec le chef : le devis.\n", total(3))), "{text}");
        assert_eq!(text.matches("## Bilan").count(), 1);
        // The same day again: in place of what Ronnie wrote, the user's line after it kept.
        let text = text.replace("**floor** · 1 h", "**floor** · 1 h\nÀ relancer.");
        let wed2 = "## Mercredi 28 octobre\n\n**floor** · 2 h";
        let text = upsert(&text, &title, wed2, Some(wed), &total(4), Some(&total(3))).text;
        assert!(text.contains("**floor** · 2 h\nÀ relancer.") && !text.contains("**floor** · 1 h"), "{text}");
        // Changed by the user: theirs, nothing added.
        let edited = text.replace("**floor** · 2 h", "**floor** · 2 h (dont 1 h de réunion)");
        let again = upsert(&edited, &title, "## Mercredi 28 octobre\n\n**floor** · 3 h", Some(wed2), &total(4), Some(&total(4)));
        assert!(!again.section && again.text.contains("(dont 1 h de réunion)") && !again.text.contains("· 3 h"), "{}", again.text);
        let edited = again.text.replace("- **Commits** : 4", "- **Commits** : 4 (plus ceux de Jessica)");
        let again = upsert(&edited, &title, "## Jeudi 29 octobre\n\n**shop** · 1 h", None, &total(5), Some(&total(4)));
        assert!(!again.total && again.text.contains("(plus ceux de Jessica)") && again.text.matches("## Bilan").count() == 1, "{}", again.text);
        // The new day before the total all the same.
        assert!(again.text.find("## Jeudi").unwrap() < again.text.find("## Bilan").unwrap(), "{}", again.text);
    }

    #[test]
    fn a_total_in_the_other_language_stays_the_total() {
        let text = "# Récap\n\n## Mardi 27 octobre\n\nx\n\n## Bilan de la semaine\n\n- old\n";
        let done = upsert(text, "Récap", "## Wednesday October 28\n\ny", None, "## The week in total\n\n- new", None);
        assert!(!done.total && done.text.matches("## ").count() == 3, "{}", done.text);
        assert!(done.text.find("## Wednesday").unwrap() < done.text.find("## Bilan").unwrap(), "{}", done.text);
    }

    #[test]
    fn a_day_without_work_is_empty() {
        let mut day = Day::default();
        // Under a minute in a project: nothing to say.
        day.projects.insert("C:\\www\\nowhere".into(), 30);
        let report = report(d(2026, 10, 31), &day, Lang::Fr);
        assert!(report.empty);
        assert_eq!(report.facts, Facts::default());
        day.servers.insert("Apache 12".into(), 600);
        assert!(!super::report(d(2026, 10, 31), &day, Lang::Fr).empty);
    }

    #[test]
    fn tokens_cost_what_the_api_charges() {
        let usage = serde_json::json!({
            "input_tokens": 1_000_000, "output_tokens": 100_000, "cache_read_input_tokens": 10_000_000,
            "cache_creation_input_tokens": 300_000, "cache_creation": {"ephemeral_1h_input_tokens": 200_000, "ephemeral_5m_input_tokens": 100_000}
        });
        // Opus 5.5: 4 + 2 (output at 20) + 2 (reads at 0.20) + 0.50 (5 min at 5) + 1.60 (1 h at 8).
        assert!((cost("claude-opus-5-5", &usage) - 10.10).abs() < 1e-9);
        // Sonnet 5, not Sonnet 4's prices: 2 + 1 + 2 + 0.25 + 0.80.
        assert!((cost("claude-sonnet-5", &usage) - 6.05).abs() < 1e-9);
        assert_eq!(cost("<synthetic>", &usage), 0.0);
        assert_eq!(euros(48.204, Lang::Fr), "48,20 €");
    }

    #[test]
    fn ecology_reads_well() {
        assert_eq!(ecology(1500.0, Lang::Fr), "- 🌍 ≈ 1,5 kWh d'électricité : autant que 0,3 bain chaud, 100 charges de smartphone ou 10 km en voiture électrique ; ≈ 3,0 L d'eau et 600 g de CO₂");
        // A small day: only the comparisons that say something.
        assert_eq!(ecology(40.0, Lang::Fr), "- 🌍 ≈ 40 Wh d'électricité : autant que 3 charges de smartphone ; ≈ 8 cL d'eau et 16 g de CO₂");
        assert_eq!(ecology(5.0, Lang::Fr), "- 🌍 ≈ 5 Wh d'électricité ; ≈ 1 cL d'eau et 2 g de CO₂");
    }

    #[test]
    fn commits_say_how_far_they_went() {
        let base = std::env::temp_dir().join(format!("ronnie-recap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (origin, repo) = (base.join("origin.git"), base.join("repo"));
        std::fs::create_dir_all(&repo).unwrap();
        let run = |dir: &Path, args: &[&str]| {
            let ok = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_AUTHOR_DATE", "2026-10-28T09:15:00").env("GIT_COMMITTER_DATE", "2026-10-28T09:15:00").output().unwrap().status.success();
            assert!(ok, "git {args:?}");
        };
        run(&base, &["init", "-q", "--bare", "origin.git"]);
        run(&repo, &["init", "-q", "-b", "main"]);
        run(&repo, &["config", "user.email", "me@example.com"]);
        run(&repo, &["config", "user.name", "Me"]);
        run(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        let commit = |file: &str, lines: &str, subject: &str| {
            std::fs::write(repo.join(file), lines).unwrap();
            run(&repo, &["add", file]);
            run(&repo, &["commit", "-q", "-m", subject]);
        };
        commit("a.txt", "1\n2\n3\n", "Le panier ne se vide plus");
        run(&repo, &["push", "-q", "origin", "main"]);
        run(&repo, &["checkout", "-q", "-b", "feature/stock"]);
        commit("b.txt", "1\n", "Le stock en direct");
        run(&repo, &["push", "-q", "origin", "feature/stock"]);
        commit("c.txt", "1\n2\n", "Les alertes de stock");
        // Two commits named alike, at other times: two.
        for (file, at) in [("d.txt", "2026-10-28T10:00:00"), ("e.txt", "2026-10-28T11:00:00")] {
            std::fs::write(repo.join(file), "1\n").unwrap();
            run(&repo, &["add", file]);
            run(&repo, &["commit", "-q", "-m", "fix", "--date", at]);
        }
        std::fs::write(repo.join("a.txt"), "1\n").unwrap();

        let day = git_day(&repo, d(2026, 10, 28), true);
        let other_day = git_day(&repo, d(2026, 10, 29), false);
        let _ = std::fs::remove_dir_all(&base);
        // In production, its branch isn't said (found from any of those holding it).
        let mut got: Vec<(&str, Stage, &str)> = day.commits.iter().map(|c| (c.subject.as_str(), c.stage, if c.stage == Stage::Prod { "" } else { c.branch.as_str() })).collect();
        // All at the same minute: in no order.
        got.sort();
        assert!(day.remote);
        assert_eq!(
            got,
            vec![
                ("Le panier ne se vide plus", Stage::Prod, ""),
                ("Le stock en direct", Stage::Pushed, "feature/stock"),
                ("Les alertes de stock", Stage::Local, "feature/stock"),
                ("fix", Stage::Local, "feature/stock"),
                ("fix", Stage::Local, "feature/stock"),
            ]
        );
        let cart = day.commits.iter().find(|c| c.stage == Stage::Prod).unwrap();
        assert_eq!((cart.added, cart.time.as_str()), (3, "09:15"));
        // a.txt down to one line, not committed.
        assert_eq!(day.uncommitted, Some((1, 0, 2)));
        // Another day: none of them, and what isn't committed only today.
        assert!(other_day.commits.is_empty() && other_day.uncommitted.is_none());
    }

    #[test]
    fn claude_sessions_give_the_requests_and_the_tokens() {
        let day = d(2026, 10, 28);
        let lines = [
            r#"{"type":"user","timestamp":"2026-10-28T09:00:00Z","cwd":"C:\\www\\shop","origin":{"kind":"human"},"message":{"role":"user","content":"corrige le panier qui se vide"}}"#,
            r#"{"type":"user","timestamp":"2026-10-28T09:00:05Z","cwd":"C:\\www\\shop","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
            r#"{"type":"user","timestamp":"2026-10-28T09:00:06Z","cwd":"C:\\www\\shop","isMeta":true,"message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-28T09:00:10Z","cwd":"C:\\www\\shop","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100}}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-28T09:00:11Z","cwd":"C:\\www\\shop","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":50,"cache_read_input_tokens":100}}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-27T09:00:11Z","cwd":"C:\\www\\shop","message":{"id":"m0","usage":{"input_tokens":999}}}"#,
        ];
        // The session resumed in another file: its lines again, then a new request.
        let resumed = [
            r#"{"type":"user","uuid":"u1","timestamp":"2026-10-28T09:00:00Z","cwd":"C:\\www\\shop","message":{"role":"user","content":"corrige le panier qui se vide"}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-28T09:00:11Z","cwd":"C:\\www\\shop","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":50,"cache_read_input_tokens":100}}}"#,
        ];
        let first = r#"{"type":"user","uuid":"u1","timestamp":"2026-10-28T09:00:00Z","cwd":"C:\\www\\shop","origin":{"kind":"human"},"message":{"role":"user","content":"corrige le panier qui se vide"}}"#;
        let mut sessions = Sessions::default();
        read_session(&[first].iter().chain(&lines[1..]).copied().collect::<Vec<_>>().join("\n"), day, "Pro", &mut sessions);
        read_session(&resumed.join("\n"), day, "Pro", &mut sessions);
        read_session(r#"{"type":"user","uuid":"u2","timestamp":"2026-10-28T15:00:00Z","cwd":"C:\\www\\shop","message":{"role":"user","content":"et maintenant la page produit"}}"#, day, "Pro", &mut sessions);
        let days = sessions.finish();
        assert_eq!(days.len(), 1);
        let shop = &days["C:\\www\\shop"];
        assert_eq!(shop.prompts, 2);
        // 09:00:00 to 09:00:11 at work; the request at 15:00, after a break, adds nothing.
        assert_eq!(shop.active, 11);
        // The message streamed twice, and copied by the session resumed, counts once, with its last
        // usage; yesterday's not at all.
        assert_eq!((shop.tokens, shop.written_tokens), (160, 50));
    }
}
