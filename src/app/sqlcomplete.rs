//! Completion of the SQL typed in the database view: keywords, functions, and the tables and columns
//! of the database selected (aliases understood), in a list under the word being typed.

use std::collections::HashMap;

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::{TextEditOutput, TextEditState};

use super::*;
use crate::db::TableInfo;

/// Keywords suggested (MySQL / MariaDB), with the phrases they usually come in.
const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "IS NULL", "IS NOT NULL", "NULL", "LIKE", "NOT LIKE", "REGEXP", "BETWEEN", "EXISTS", "NOT EXISTS", "AS", "ON", "USING",
    "JOIN", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN", "CROSS JOIN", "ORDER BY", "GROUP BY", "HAVING", "LIMIT", "OFFSET", "ASC", "DESC", "DISTINCT", "UNION", "UNION ALL", "WITH",
    "RECURSIVE", "CASE", "WHEN", "THEN", "ELSE", "END", "INSERT INTO", "INSERT IGNORE INTO", "REPLACE INTO", "VALUES", "ON DUPLICATE KEY UPDATE", "UPDATE", "SET", "DELETE FROM",
    "CREATE TABLE", "CREATE DATABASE", "CREATE INDEX", "CREATE UNIQUE INDEX", "CREATE VIEW", "CREATE OR REPLACE VIEW", "CREATE USER", "ALTER TABLE", "ALTER USER", "ADD COLUMN",
    "ADD INDEX", "ADD PRIMARY KEY", "ADD CONSTRAINT", "DROP COLUMN", "DROP INDEX", "MODIFY COLUMN", "CHANGE COLUMN", "RENAME TABLE", "RENAME COLUMN", "DROP TABLE", "DROP VIEW",
    "DROP DATABASE", "DROP USER", "TRUNCATE TABLE", "IF EXISTS", "IF NOT EXISTS", "PRIMARY KEY", "FOREIGN KEY", "REFERENCES", "ON DELETE CASCADE", "ON UPDATE CASCADE",
    "UNIQUE", "INDEX", "DEFAULT", "AUTO_INCREMENT", "NOT NULL", "UNSIGNED", "CURRENT_TIMESTAMP", "ENGINE", "CHARSET", "COLLATE", "COMMENT", "AFTER", "FIRST", "CONSTRAINT",
    "CASCADE", "INTERVAL", "TRUE", "FALSE", "ALL", "ANY", "DIV", "XOR", "WITH ROLLUP", "OVER", "PARTITION BY", "FOR UPDATE", "SHOW TABLES", "SHOW DATABASES",
    "SHOW COLUMNS FROM", "SHOW CREATE TABLE", "SHOW INDEX FROM", "SHOW PROCESSLIST", "SHOW FULL PROCESSLIST", "SHOW VARIABLES LIKE", "SHOW STATUS LIKE", "SHOW GRANTS FOR",
    "SHOW TABLE STATUS", "DESCRIBE", "EXPLAIN", "USE", "START TRANSACTION", "BEGIN", "COMMIT", "ROLLBACK", "GRANT", "REVOKE", "IDENTIFIED BY", "LOCK TABLES", "UNLOCK TABLES",
    "OPTIMIZE TABLE", "ANALYZE TABLE", "KILL", "INT", "BIGINT", "TINYINT", "SMALLINT", "MEDIUMINT", "DECIMAL", "FLOAT", "DOUBLE", "VARCHAR", "CHAR", "TEXT", "MEDIUMTEXT",
    "LONGTEXT", "BLOB", "LONGBLOB", "DATE", "DATETIME", "TIMESTAMP", "TIME", "BOOLEAN", "JSON", "ENUM",
];

/// Functions suggested, written with their parentheses.
const FUNCTIONS: &[&str] = &[
    "COUNT", "SUM", "AVG", "MIN", "MAX", "GROUP_CONCAT", "CONCAT", "CONCAT_WS", "COALESCE", "IFNULL", "NULLIF", "IF", "GREATEST", "LEAST", "CAST", "CONVERT", "NOW", "CURDATE",
    "CURTIME", "DATE", "DATE_FORMAT", "DATE_ADD", "DATE_SUB", "DATEDIFF", "TIMESTAMPDIFF", "STR_TO_DATE", "YEAR", "MONTH", "DAY", "HOUR", "MINUTE", "SECOND", "WEEK",
    "UNIX_TIMESTAMP", "FROM_UNIXTIME", "LOWER", "UPPER", "LENGTH", "CHAR_LENGTH", "SUBSTRING", "SUBSTRING_INDEX", "TRIM", "REPLACE", "LEFT", "RIGHT", "LPAD", "RPAD",
    "LOCATE", "INSTR", "FIND_IN_SET", "FORMAT", "ROUND", "FLOOR", "CEIL", "ABS", "MOD", "RAND", "JSON_EXTRACT", "JSON_UNQUOTE", "JSON_OBJECT", "JSON_ARRAY", "JSON_CONTAINS",
    "UUID", "MD5", "SHA2", "LAST_INSERT_ID", "DATABASE", "VERSION", "ROW_NUMBER", "RANK", "DENSE_RANK", "LAG", "LEAD",
];

/// Words after which a table is named.
const TABLE_BEFORE: &[&str] = &["FROM", "JOIN", "STRAIGHT_JOIN", "INTO", "UPDATE", "TABLE", "TRUNCATE", "DESCRIBE"];

/// Suggestions kept at most.
const MAX: usize = 60;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Kind {
    Keyword,
    Function,
    Table,
    View,
    Column,
    Database,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Suggestion {
    pub label: String,
    /// What replaces the word typed, and how many characters before its end the cursor goes.
    pub insert: String,
    pub back: usize,
    /// A column's type (and table, when it isn't one the query names).
    pub detail: String,
    pub kind: Kind,
}

/// What the word at the cursor can become: the bytes it spans (replaced), and the suggestions.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Completion {
    pub start: usize,
    pub end: usize,
    pub items: Vec<Suggestion>,
}

/// What can be completed: the databases, the tables listed, the columns of the selected database.
pub(super) struct Schema<'a> {
    pub db: Option<&'a str>,
    pub databases: Vec<&'a str>,
    /// Tables listed, by database.
    pub tables: &'a HashMap<String, Vec<TableInfo>>,
    /// Columns (name, type) of the selected database's tables, by table.
    pub columns: Option<&'a HashMap<String, Vec<(String, String)>>>,
    /// The table open, whose columns come first while the query names none.
    pub table: Option<&'a str>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn is_keyword(word: &str) -> bool {
    KEYWORDS.iter().any(|k| k.split(' ').any(|w| w.eq_ignore_ascii_case(word)))
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Word { text: String, quoted: bool },
    Punct(char),
}

fn word_is(t: &Tok, words: &[&str]) -> bool {
    matches!(t, Tok::Word { text, quoted: false } if words.iter().any(|w| w.eq_ignore_ascii_case(text)))
}

/// Where the text scanned ends: in the SQL, in a literal or a comment, or in a name opened with a
/// backtick (the byte after it).
#[derive(Debug, PartialEq)]
enum End {
    Code,
    Literal,
    Backtick(usize),
}

/// The words (names unquoted) and signs of `sql`, literals and comments left out.
fn scan(sql: &str) -> (Vec<Tok>, End) {
    let mut toks = Vec::new();
    let mut it = sql.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '\'' | '"' => {
                let mut closed = false;
                while let Some((_, d)) = it.next() {
                    if d == '\\' {
                        it.next();
                    } else if d == c {
                        // Doubled: the quote itself.
                        if it.peek().map(|p| p.1) == Some(c) {
                            it.next();
                        } else {
                            closed = true;
                            break;
                        }
                    }
                }
                if !closed {
                    return (toks, End::Literal);
                }
                toks.push(Tok::Punct('\''));
            }
            '`' => {
                let mut name = String::new();
                let mut closed = false;
                while let Some((_, d)) = it.next() {
                    if d == '`' {
                        if it.peek().map(|p| p.1) == Some('`') {
                            it.next();
                            name.push('`');
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        name.push(d);
                    }
                }
                if !closed {
                    return (toks, End::Backtick(i + 1));
                }
                toks.push(Tok::Word { text: name, quoted: true });
            }
            '#' => {
                if !it.by_ref().any(|(_, d)| d == '\n') {
                    return (toks, End::Literal);
                }
            }
            '-' if sql[i..].starts_with("--") && sql[i + 2..].chars().next().is_none_or(char::is_whitespace) => {
                if !it.by_ref().any(|(_, d)| d == '\n') {
                    return (toks, End::Literal);
                }
            }
            '/' if sql[i..].starts_with("/*") => {
                it.next();
                let mut star = false;
                let mut closed = false;
                for (_, d) in it.by_ref() {
                    if star && d == '/' {
                        closed = true;
                        break;
                    }
                    star = d == '*';
                }
                if !closed {
                    return (toks, End::Literal);
                }
            }
            c if is_ident(c) => {
                let mut end = i + c.len_utf8();
                while let Some(&(j, d)) = it.peek() {
                    if !is_ident(d) {
                        break;
                    }
                    end = j + d.len_utf8();
                    it.next();
                }
                toks.push(Tok::Word { text: sql[i..end].to_owned(), quoted: false });
            }
            c if c.is_whitespace() => {}
            c => toks.push(Tok::Punct(c)),
        }
    }
    (toks, End::Code)
}

/// A table the query names, and its alias.
#[derive(Debug, PartialEq)]
struct Ref {
    db: Option<String>,
    table: String,
    alias: Option<String>,
}

/// A name at `i`: quoted, or a word that isn't a keyword.
fn name_at(toks: &[Tok], i: usize) -> Option<String> {
    match toks.get(i)? {
        Tok::Word { text, quoted } if *quoted || !is_keyword(text) => Some(text.clone()),
        _ => None,
    }
}

/// The tables named after FROM, JOIN, UPDATE, INTO... (and after the commas of a FROM list).
fn references(toks: &[Tok]) -> Vec<Ref> {
    let mut refs = Vec::new();
    let mut in_list = false;
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let expect = if word_is(t, TABLE_BEFORE) {
            in_list = word_is(t, &["FROM", "UPDATE"]);
            true
        } else if matches!(t, Tok::Punct(',')) {
            in_list
        } else {
            if matches!(t, Tok::Punct('(' | ')')) || matches!(t, Tok::Word { text, quoted: false } if is_keyword(text) && !text.eq_ignore_ascii_case("AS")) {
                in_list = false;
            }
            false
        };
        i += 1;
        if !expect {
            continue;
        }
        let Some(first) = name_at(toks, i) else { continue };
        i += 1;
        let (db, table) = match (toks.get(i), name_at(toks, i + 1)) {
            (Some(Tok::Punct('.')), Some(table)) => {
                i += 2;
                (Some(first), table)
            }
            _ => (None, first),
        };
        if toks.get(i).is_some_and(|t| word_is(t, &["AS"])) {
            i += 1;
        }
        let alias = name_at(toks, i);
        if alias.is_some() {
            i += 1;
        }
        refs.push(Ref { db, table, alias });
    }
    refs
}

/// What the word at the cursor names.
#[derive(Debug, PartialEq)]
enum Context {
    Tables,
    Databases,
    /// After "name.": a column of that table (or alias), or a table of that database.
    Qualified(String),
    General,
}

fn context(before: &[Tok]) -> Context {
    match before.last() {
        Some(t) if word_is(t, TABLE_BEFORE) => Context::Tables,
        Some(t) if word_is(t, &["USE", "DATABASE", "SCHEMA"]) => Context::Databases,
        Some(Tok::Punct(',')) => {
            // In a FROM list: the last clause word is FROM (or UPDATE).
            let clause = before.iter().rev().find(|t| matches!(t, Tok::Punct('(' | ')')) || matches!(t, Tok::Word { text, quoted: false } if is_keyword(text) && !text.eq_ignore_ascii_case("AS")));
            if clause.is_some_and(|t| word_is(t, &["FROM", "UPDATE"])) { Context::Tables } else { Context::General }
        }
        _ => Context::General,
    }
}

/// The name right before a "." ending at `at`, and where it starts.
fn qualifier(sql: &str, at: usize) -> Option<(String, usize)> {
    let dot = sql[..at].strip_suffix('.')?.len();
    if let Some(head) = sql[..dot].strip_suffix('`') {
        let open = head.rfind('`')?;
        return Some((head[open + 1..].replace("``", "`"), open));
    }
    let start = sql[..dot].char_indices().rev().take_while(|(_, c)| is_ident(*c)).last().map(|(i, _)| i)?;
    Some((sql[start..dot].to_owned(), start))
}

/// A name as written in SQL: in backticks when it has to be (or `in_quotes`: closing the one opened).
fn written(name: &str, in_quotes: bool) -> String {
    if in_quotes {
        return format!("{}`", name.replace('`', "``"));
    }
    let plain = name.chars().all(is_ident) && !name.starts_with(|c: char| c.is_ascii_digit()) && !is_keyword(name);
    if plain && !name.is_empty() { name.to_owned() } else { crate::db::ident(name) }
}

fn find<'a, V>(map: &'a HashMap<String, V>, name: &str) -> Option<(&'a String, &'a V)> {
    map.get_key_value(name).or_else(|| map.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)))
}

/// Suggestions for the word at byte `cursor` of `sql`. Without `forced` (Ctrl+Space), only once a word
/// is started, or where a table, a database or a column after "." is expected.
pub(super) fn complete(sql: &str, cursor: usize, schema: &Schema, forced: bool) -> Option<Completion> {
    if cursor > sql.len() || !sql.is_char_boundary(cursor) {
        return None;
    }
    let (start, in_quotes) = match scan(&sql[..cursor]).1 {
        End::Literal => return None,
        End::Backtick(b) => (b, true),
        End::Code => (sql[..cursor].char_indices().rev().take_while(|(_, c)| is_ident(*c)).last().map_or(cursor, |(i, _)| i), false),
    };
    let prefix = &sql[start..cursor];
    if !in_quotes && prefix.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    // The rest of the word is replaced too (and a closing backtick already there).
    let mut end = cursor + sql[cursor..].find(|c: char| !is_ident(c)).unwrap_or(sql.len() - cursor);
    if in_quotes && sql[end..].starts_with('`') {
        end += 1;
    }
    let word_start = if in_quotes { start - 1 } else { start };
    let (context, from) = match qualifier(sql, word_start) {
        Some((q, at)) => (Context::Qualified(q), at),
        None => (context(&scan(&sql[..word_start]).0), word_start),
    };
    if !forced && prefix.is_empty() && !in_quotes && context == Context::General {
        return None;
    }
    // The tables named, the word typed left out (it isn't an alias).
    let refs = references(&scan(&format!("{} {}", &sql[..from], &sql[end..])).0);

    let lower = prefix.to_lowercase();
    // 0: starts with what is typed, 1: contains it (names only, from two letters).
    let score = |name: &str, anywhere: bool| {
        let n = name.to_lowercase();
        if n.starts_with(&lower) {
            Some(0)
        } else if anywhere && lower.chars().count() >= 2 && n.contains(&lower) {
            Some(1)
        } else {
            None
        }
    };
    let mut found: Vec<(u8, u8, Suggestion)> = Vec::new();
    let mut add = |group: u8, anywhere: bool, label: &str, insert: String, back: usize, detail: String, kind: Kind| {
        if let Some(s) = score(label, anywhere) {
            found.push((group, s, Suggestion { label: label.to_owned(), insert, back, detail, kind }));
        }
    };
    let columns = schema.columns;
    let tables = schema.db.and_then(|d| schema.tables.get(d));
    let add_tables = |add: &mut dyn FnMut(u8, bool, &str, String, usize, String, Kind), group: u8, list: Option<&Vec<TableInfo>>| {
        if let Some(list) = list {
            for tb in list {
                add(group, true, &tb.name, written(&tb.name, in_quotes), 0, String::new(), if tb.view { Kind::View } else { Kind::Table });
            }
        } else if let Some(columns) = columns {
            // Tables not listed yet: those whose columns came.
            let mut names: Vec<&String> = columns.keys().collect();
            names.sort();
            for name in names {
                add(group, true, name, written(name, in_quotes), 0, String::new(), Kind::Table);
            }
        }
    };
    let lowercase = !prefix.is_empty() && !prefix.chars().any(char::is_uppercase);
    let case = |k: &str| if lowercase { k.to_lowercase() } else { k.to_owned() };
    match &context {
        Context::Databases => {
            for d in &schema.databases {
                add(0, true, d, written(d, in_quotes), 0, String::new(), Kind::Database);
            }
        }
        Context::Tables => {
            add_tables(&mut add, 0, tables);
            if !in_quotes {
                for d in &schema.databases {
                    add(1, false, d, written(d, false), 0, String::new(), Kind::Database);
                }
                for k in KEYWORDS {
                    add(2, false, k, case(k), 0, String::new(), Kind::Keyword);
                }
            }
        }
        Context::Qualified(q) => {
            let named = refs.iter().find(|r| r.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(q))).or_else(|| refs.iter().find(|r| r.alias.is_none() && r.table.eq_ignore_ascii_case(q)));
            let this_db = named.is_none_or(|r| r.db.as_deref().is_none_or(|d| Some(d) == schema.db));
            let table = named.map_or(q.as_str(), |r| r.table.as_str());
            match columns.and_then(|c| find(c, table)).filter(|_| this_db) {
                Some((_, cols)) => {
                    for (name, kind) in cols {
                        add(0, true, name, written(name, in_quotes), 0, kind.clone(), Kind::Column);
                    }
                }
                // A database's tables.
                None => add_tables(&mut add, 0, find(schema.tables, q).map(|(_, t)| t)),
            }
        }
        Context::General => {
            // Columns of the tables the query names (or of the table open) first.
            let mut named: Vec<&str> = refs.iter().filter(|r| r.db.as_deref().is_none_or(|d| Some(d) == schema.db)).map(|r| r.table.as_str()).collect();
            if refs.is_empty() {
                named.extend(schema.table);
            }
            let mut seen: Vec<String> = Vec::new();
            if let Some(columns) = columns {
                let several = named.len() > 1;
                for tb in &named {
                    let Some((tb, cols)) = find(columns, tb) else { continue };
                    for (name, kind) in cols {
                        if !seen.contains(name) {
                            seen.push(name.clone());
                            add(0, true, name, written(name, in_quotes), 0, if several { format!("{kind} · {tb}") } else { kind.clone() }, Kind::Column);
                        }
                    }
                }
            }
            if !in_quotes {
                for k in KEYWORDS {
                    add(1, false, k, case(k), 0, String::new(), Kind::Keyword);
                }
                for f in FUNCTIONS {
                    add(2, false, &format!("{f}()"), format!("{}()", case(f)), 1, String::new(), Kind::Function);
                }
            }
            add_tables(&mut add, 3, tables);
            // Columns of the other tables, once a word is started.
            if let (Some(columns), false) = (columns, prefix.is_empty()) {
                let mut other: Vec<(&String, &Vec<(String, String)>)> = columns.iter().filter(|(tb, _)| !named.iter().any(|n| n.eq_ignore_ascii_case(tb))).collect();
                other.sort_by(|a, b| a.0.cmp(b.0));
                for (tb, cols) in other {
                    for (name, kind) in cols {
                        if !seen.contains(name) {
                            seen.push(name.clone());
                            add(4, true, name, written(name, in_quotes), 0, format!("{kind} · {tb}"), Kind::Column);
                        }
                    }
                }
            }
        }
    }
    found.sort_by_key(|(group, score, _)| (*score, *group));
    let mut items: Vec<Suggestion> = Vec::new();
    for (_, _, s) in found {
        if !items.iter().any(|i| i.insert == s.insert) {
            items.push(s);
        }
        if items.len() == MAX {
            break;
        }
    }
    // Nothing more than what is typed.
    if items.is_empty() || (!forced && items.iter().all(|s| s.label.eq_ignore_ascii_case(prefix))) {
        return None;
    }
    Some(Completion { start, end, items })
}

/// The suggestions shown under a SQL field, and how the keyboard moves in them.
#[derive(Default)]
pub(super) struct Completer {
    shown: Option<Completion>,
    /// The text and cursor (byte) the suggestions were made for.
    made_for: Option<(String, usize)>,
    pick: usize,
    /// Picked with the arrows: Enter then takes it (else it goes on as usual).
    chosen: bool,
    scroll: bool,
    /// Asked for with Ctrl+Space (shown even with nothing typed).
    forced: bool,
    /// Where the list was drawn: a click in it doesn't close it.
    rect: Option<Rect>,
}

fn byte_of(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(i, _)| i)
}

impl Completer {
    fn close(&mut self) {
        (self.shown, self.pick, self.chosen, self.forced, self.rect) = (None, 0, false, false, None);
    }

    /// Before the field: Ctrl+Space opens the list; while it shows, ↑ ↓ move in it, Tab (or Enter
    /// after the arrows) takes a suggestion, Escape closes it.
    pub fn keys(&mut self, ui: &Ui, id: egui::Id, text: &mut String) {
        if !ui.memory(|m| m.has_focus(id)) {
            return;
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Space)) {
            self.forced = true;
            self.made_for = None;
        }
        // Made for another text (a query loaded from the history...).
        if self.made_for.as_ref().is_some_and(|(t, _)| t != text) {
            self.shown = None;
        }
        let Some(n) = self.shown.as_ref().map(|s| s.items.len()) else { return };
        let (mut take, mut escape) = (false, false);
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                (self.pick, self.chosen, self.scroll) = ((self.pick + 1) % n, true, true);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                (self.pick, self.chosen, self.scroll) = ((self.pick + n - 1) % n, true, true);
            }
            take = i.consume_key(Modifiers::NONE, Key::Tab) || (self.chosen && i.consume_key(Modifiers::NONE, Key::Enter));
            escape = i.consume_key(Modifiers::NONE, Key::Escape);
        });
        if take {
            self.take(ui.ctx(), id, text, self.pick);
        } else if escape {
            self.close();
        }
    }

    fn take(&mut self, ctx: &egui::Context, id: egui::Id, text: &mut String, pick: usize) {
        let Some(shown) = self.shown.take() else { return };
        let Some(s) = shown.items.get(pick) else { return };
        if shown.end > text.len() || !text.is_char_boundary(shown.start) || !text.is_char_boundary(shown.end) {
            return;
        }
        text.replace_range(shown.start..shown.end, &s.insert);
        let at = text[..shown.start].chars().count() + s.insert.chars().count() - s.back;
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        state.store(ctx, id);
        self.close();
        // Not opened again for what was just written.
        self.made_for = Some((text.clone(), byte_of(text, at)));
        ctx.request_repaint();
    }

    /// After the field: the suggestions follow what is typed, and show under the word.
    pub fn show(&mut self, ui: &Ui, id: egui::Id, out: &TextEditOutput, text: &mut String, schema: &Schema, theme: &Theme, t: &Strings) {
        let ctx = ui.ctx().clone();
        let over_list = self.rect.zip(ctx.pointer_latest_pos()).is_some_and(|(r, p)| r.contains(p));
        let cursor = out.cursor_range.filter(|_| out.response.has_focus()).map(|r| byte_of(text, r.primary.index.0));
        match cursor {
            Some(cursor) => {
                let now = (text.as_str(), cursor);
                let same = self.made_for.as_ref().is_some_and(|(t, c)| (t.as_str(), *c) == now);
                // Typed, asked for, or the cursor moved while the list shows (it follows in the word).
                if out.response.changed() || (!same && (self.shown.is_some() || self.made_for.is_none())) {
                    let was = self.shown.take();
                    let fresh = complete(text, cursor, schema, self.forced);
                    let moved_away = !out.response.changed() && was.as_ref().zip(fresh.as_ref()).is_none_or(|(a, b)| a.start != b.start);
                    if moved_away && !(self.forced && self.made_for.is_none()) {
                        self.close();
                    } else {
                        // The one picked stays picked if it is still there.
                        let kept = was.as_ref().and_then(|w| w.items.get(self.pick)).and_then(|p| fresh.as_ref()?.items.iter().position(|s| s.insert == p.insert));
                        self.pick = kept.unwrap_or(0);
                        self.chosen &= kept.is_some();
                        self.shown = fresh;
                    }
                    self.made_for = Some((text.clone(), cursor));
                }
            }
            None if over_list => {}
            None => {
                self.close();
                return;
            }
        }
        let Some(shown) = &self.shown else {
            self.rect = None;
            return;
        };
        let anchor = out.galley.pos_from_cursor(CCursor::new(text[..shown.start.min(text.len())].chars().count())).translate(out.galley_pos.to_vec2());
        let width = 380.0;
        let row_h = 22.0;
        let mut take = None;
        let area = egui::Area::new(id.with("sql-complete")).order(egui::Order::Foreground).fixed_pos(anchor.left_bottom() + Vec2::new(-6.0, 4.0)).constrain(true).show(&ctx, |ui| {
            Frame::popup(ui.style()).fill(theme.chrome_bg).inner_margin(4.0).show(ui, |ui| {
                ui.set_width(width);
                egui::ScrollArea::vertical().max_height(row_h * 10.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, s) in shown.items.iter().enumerate() {
                        let (r, response) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click());
                        let painter = ui.painter();
                        if i == self.pick {
                            painter.rect_filled(r, 4.0, theme.accent.gamma_multiply(0.28));
                        } else if response.hovered() {
                            painter.rect_filled(r, 4.0, theme.tab_hover);
                        }
                        let (letter, color) = match s.kind {
                            Kind::Table => ("T", theme.ansi[4]),
                            Kind::View => ("V", theme.ansi[4]),
                            Kind::Column => ("C", theme.ansi[2]),
                            Kind::Keyword => ("K", theme.ansi[5]),
                            Kind::Function => ("F", theme.ansi[3]),
                            Kind::Database => ("D", theme.ansi[6]),
                        };
                        let badge = Rect::from_center_size(Pos2::new(r.min.x + 13.0, r.center().y), Vec2::splat(16.0));
                        painter.rect_filled(badge, 3.0, color.gamma_multiply(0.22));
                        painter.text(badge.center(), Align2::CENTER_CENTER, letter, FontId::monospace(10.5), color);
                        let detail = painter.layout_no_wrap(s.detail.clone(), FontId::monospace(11.5), theme.text_muted);
                        let detail_w = detail.size().x.min(width * 0.45);
                        let mut job = egui::text::LayoutJob::simple_singleline(s.label.clone(), FontId::monospace(12.5), theme.text);
                        job.wrap = egui::text::TextWrapping::truncate_at_width(width - 40.0 - detail_w - 12.0);
                        let label = painter.layout_job(job);
                        painter.galley(Pos2::new(r.min.x + 28.0, r.center().y - label.size().y / 2.0), label, theme.text);
                        if !s.detail.is_empty() {
                            let mut job = egui::text::LayoutJob::simple_singleline(s.detail.clone(), FontId::monospace(11.5), theme.text_muted);
                            job.wrap = egui::text::TextWrapping::truncate_at_width(detail_w);
                            let detail = painter.layout_job(job);
                            painter.galley(Pos2::new(r.max.x - 6.0 - detail.size().x, r.center().y - detail.size().y / 2.0), detail, theme.text_muted);
                        }
                        if i == self.pick && self.scroll {
                            ui.scroll_to_rect(r, None);
                        }
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            take = Some(i);
                        }
                    }
                });
                ui.add_space(3.0);
                ui.label(egui::RichText::new(t.sql_complete_hint).size(11.0).color(theme.text_muted));
            });
        });
        self.scroll = false;
        self.rect = Some(area.response.rect);
        if let Some(i) = take {
            self.take(&ctx, id, text, i);
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema<'a>(tables: &'a HashMap<String, Vec<TableInfo>>, columns: &'a HashMap<String, Vec<(String, String)>>) -> Schema<'a> {
        Schema { db: Some("shop"), databases: vec!["shop", "blog"], tables, columns: Some(columns), table: None }
    }

    fn fixtures() -> (HashMap<String, Vec<TableInfo>>, HashMap<String, Vec<(String, String)>>) {
        let table = |name: &str| TableInfo { name: name.into(), ..Default::default() };
        let tables = HashMap::from([("shop".to_owned(), vec![table("orders"), table("users"), table("user roles")]), ("blog".to_owned(), vec![table("posts")])]);
        let col = |n: &str, k: &str| (n.to_owned(), k.to_owned());
        let columns = HashMap::from([
            ("users".to_owned(), vec![col("id", "int"), col("name", "varchar(80)"), col("email", "varchar(255)")]),
            ("orders".to_owned(), vec![col("id", "int"), col("user_id", "int"), col("total", "decimal(10,2)")]),
        ]);
        (tables, columns)
    }

    /// The suggestions at the "|" of `sql`.
    fn at(sql: &str, forced: bool) -> Option<(String, Vec<String>)> {
        let (tables, columns) = fixtures();
        let cursor = sql.find('|').unwrap();
        let text = sql.replace('|', "");
        complete(&text, cursor, &schema(&tables, &columns), forced).map(|c| (text[c.start..c.end].to_owned(), c.items.into_iter().map(|s| s.insert).collect()))
    }

    #[test]
    fn tables_after_from() {
        let (word, items) = at("SELECT * FROM us|", false).unwrap();
        assert_eq!(word, "us");
        assert_eq!(items[0], "users");
        assert!(items.contains(&"`user roles`".to_owned()), "{items:?}");
        let (_, items) = at("SELECT * FROM |", false).unwrap();
        assert_eq!(&items[..3], ["orders", "users", "`user roles`"]);
        let (_, items) = at("select * from users, o|", false).unwrap();
        assert_eq!(items[0], "orders");
    }

    #[test]
    fn columns_of_the_tables_named() {
        let (_, items) = at("SELECT na| FROM users", false).unwrap();
        assert_eq!(items[0], "name");
        let (_, items) = at("SELECT u.| FROM users u JOIN orders o ON o.user_id = u.id", false).unwrap();
        assert_eq!(items, ["id", "name", "email"]);
        let (_, items) = at("SELECT * FROM users AS x JOIN orders o ON x.id = o.u|", false).unwrap();
        assert_eq!(items, ["user_id"]);
        let (_, items) = at("SELECT orders.t| FROM orders", false).unwrap();
        assert_eq!(items, ["total"]);
        // The table typed isn't taken for an alias.
        let (_, items) = at("SELECT * FROM users WHERE em|", false).unwrap();
        assert_eq!(items[0], "email");
        // A database's tables.
        let (_, items) = at("SELECT * FROM blog.|", false).unwrap();
        assert_eq!(items, ["posts"]);
    }

    #[test]
    fn keywords_and_functions() {
        let (_, items) = at("SEL|", false).unwrap();
        assert_eq!(items[0], "SELECT");
        let (_, items) = at("select * from users ord|", false).unwrap();
        assert_eq!(items[0], "order by");
        let (_, items) = at("SELECT COU|", false).unwrap();
        assert_eq!(items[0], "COUNT()");
    }

    #[test]
    fn quiet_where_nothing_is_to_complete() {
        assert_eq!(at("SELECT * FROM users WHERE name = 'us|'", false), None);
        assert_eq!(at("SELECT 1 -- us|", false), None);
        assert_eq!(at("SELECT |", false), None);
        assert_eq!(at("LIMIT 1|", false), None);
        assert!(at("SELECT |", true).is_some());
    }

    #[test]
    fn inside_backticks() {
        let (word, items) = at("SELECT * FROM `us|", false).unwrap();
        assert_eq!(word, "us");
        assert_eq!(items[0], "users`");
        let (word, items) = at("SELECT * FROM `user r|`", false).unwrap();
        assert_eq!(word, "user r`");
        assert_eq!(items, ["user roles`"]);
    }
}
