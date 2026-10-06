//! A procedure, function or trigger as a form (name, parameters, what it returns, its body, its
//! options), read from its CREATE statement and written back as one: MySQL / MariaDB or SQL Server.

use crate::db::{self, Dialect, ObjectKind};

/// A parameter: IN, OUT or INOUT (SQL Server: IN or OUT), its name, its type cut in three (INT,
/// VARCHAR + 255, DECIMAL + 10,2 + UNSIGNED), and on SQL Server its default value.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FormParam {
    pub mode: String,
    pub name: String,
    pub kind: String,
    pub size: String,
    pub options: String,
    pub default: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Form {
    pub kind: ObjectKind,
    /// SQL Server: the schema (dbo...).
    pub schema: String,
    pub name: String,
    pub params: Vec<FormParam>,
    /// What a function returns (SQL Server: may be TABLE, `@t TABLE (...)`: then all in `returns`).
    pub returns: String,
    pub returns_size: String,
    pub returns_options: String,
    /// What runs: BEGIN ... END, or a single statement.
    pub body: String,
    pub deterministic: bool,
    /// MySQL: `user@host` (empty: whoever saves it).
    pub definer: String,
    /// MySQL: DEFINER or INVOKER; NO SQL, READS SQL DATA...
    pub security: String,
    pub access: String,
    pub comment: String,
    /// SQL Server: what follows WITH (EXECUTE AS OWNER, SCHEMABINDING...).
    pub with: String,
    /// A trigger: its table, BEFORE / AFTER / INSTEAD OF, on INSERT, UPDATE, DELETE.
    pub table: String,
    pub timing: String,
    pub events: [bool; 3],
    /// MySQL: FOLLOWS or PRECEDES another trigger.
    pub order: String,
    pub order_of: String,
}

pub(super) const EVENTS: [&str; 3] = ["INSERT", "UPDATE", "DELETE"];
pub(super) const ACCESS: [&str; 5] = ["", "CONTAINS SQL", "NO SQL", "READS SQL DATA", "MODIFIES SQL DATA"];
pub(super) const MYSQL_TYPES: [&str; 20] = [
    "INT", "BIGINT", "TINYINT", "SMALLINT", "DECIMAL", "DOUBLE", "FLOAT", "BOOLEAN", "VARCHAR", "CHAR", "TEXT", "LONGTEXT", "DATE", "DATETIME", "TIMESTAMP", "TIME", "YEAR", "JSON", "BLOB", "ENUM",
];
pub(super) const MSSQL_TYPES: [&str; 19] = [
    "INT", "BIGINT", "SMALLINT", "TINYINT", "BIT", "DECIMAL", "NUMERIC", "FLOAT", "MONEY", "NVARCHAR", "VARCHAR", "NCHAR", "CHAR", "DATE", "DATETIME2", "DATETIME", "TIME", "UNIQUEIDENTIFIER", "VARBINARY",
];
pub(super) const MYSQL_OPTIONS: [&str; 5] = ["", "UNSIGNED", "UNSIGNED ZEROFILL", "CHARSET utf8mb4", "CHARSET latin1"];

impl Form {
    /// A new one, ready to fill.
    pub fn new(kind: ObjectKind, mssql: bool, name: &str, table: &str) -> Self {
        let body = match (mssql, kind) {
            (true, ObjectKind::Trigger) => "BEGIN\n  SET NOCOUNT ON;\n  \nEND",
            (true, _) => "BEGIN\n  SET NOCOUNT ON;\n  \nEND",
            (false, ObjectKind::Function) => "BEGIN\n  RETURN 0;\nEND",
            (false, _) => "BEGIN\n  \nEND",
        };
        Self {
            kind,
            schema: if mssql { "dbo".into() } else { String::new() },
            name: name.to_owned(),
            params: Vec::new(),
            returns: if kind == ObjectKind::Function { "INT".into() } else { String::new() },
            returns_size: String::new(),
            returns_options: String::new(),
            body: body.into(),
            deterministic: false,
            definer: String::new(),
            security: if mssql { String::new() } else { "DEFINER".into() },
            access: String::new(),
            comment: String::new(),
            with: String::new(),
            table: table.to_owned(),
            timing: if mssql { "AFTER" } else { "BEFORE" }.into(),
            events: [true, false, false],
            order: String::new(),
            order_of: String::new(),
        }
    }

    pub fn ready(&self) -> bool {
        !self.name.trim().is_empty() && (self.kind != ObjectKind::Trigger || (!self.table.is_empty() && self.events.iter().any(|e| *e))) && self.params.iter().all(|p| !p.name.trim().is_empty() && !p.kind.trim().is_empty())
    }

    /// The CREATE statement.
    pub fn sql(&self, dialect: Dialect) -> String {
        if dialect.mssql() { self.mssql_sql(dialect) } else { self.mysql_sql() }
    }

    fn mysql_sql(&self) -> String {
        let definer = definer_sql(&self.definer);
        let name = db::ident(self.name.trim());
        let body = if self.body.trim().is_empty() { "BEGIN\nEND".to_owned() } else { self.body.trim().to_owned() };
        if self.kind == ObjectKind::Trigger {
            let event = EVENTS.iter().zip(self.events).find(|(_, on)| *on).map_or("INSERT", |(e, _)| *e);
            let order = if self.order.is_empty() || self.order_of.trim().is_empty() { String::new() } else { format!("\n{} {}", self.order, db::ident(self.order_of.trim())) };
            return format!("CREATE {definer}TRIGGER {name} {} {event} ON {}\nFOR EACH ROW{order}\n{body}", self.timing, db::ident(&self.table));
        }
        let function = self.kind == ObjectKind::Function;
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let mode = if function || p.mode.is_empty() { String::new() } else { format!("{} ", p.mode) };
                format!("{mode}{} {}", db::ident(p.name.trim()), full_type(&p.kind, &p.size, &p.options))
            })
            .collect();
        let mut out = format!("CREATE {definer}{} {name}({})", self.kind.sql(), params.join(", "));
        if function {
            out.push_str(&format!(" RETURNS {}", full_type(&self.returns, &self.returns_size, &self.returns_options)));
        }
        if !self.comment.is_empty() {
            out.push_str(&format!("\nCOMMENT {}", db::literal(&self.comment)));
        }
        if self.deterministic {
            out.push_str("\nDETERMINISTIC");
        }
        if !self.access.is_empty() {
            out.push_str(&format!("\n{}", self.access));
        }
        if !self.security.is_empty() {
            out.push_str(&format!("\nSQL SECURITY {}", self.security));
        }
        out.push('\n');
        out.push_str(&body);
        out
    }

    fn mssql_sql(&self, d: Dialect) -> String {
        let schema = if self.schema.trim().is_empty() { "dbo" } else { self.schema.trim() };
        let name = format!("{}.{}", d.ident(schema), d.ident(self.name.trim()));
        let with = if self.with.trim().is_empty() { String::new() } else { format!("\nWITH {}", self.with.trim()) };
        let body = self.body.trim();
        if self.kind == ObjectKind::Trigger {
            let events: Vec<&str> = EVENTS.iter().zip(self.events).filter(|(_, on)| *on).map(|(e, _)| *e).collect();
            return format!("CREATE TRIGGER {name} ON {}{with}\n{} {}\nAS\n{body}", d.local_table(&self.table), self.timing, events.join(", "));
        }
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let at = if p.name.trim().starts_with('@') { p.name.trim().to_owned() } else { format!("@{}", p.name.trim()) };
                let mut s = format!("{at} {}", full_type(&p.kind, &p.size, ""));
                if !p.default.trim().is_empty() {
                    s.push_str(&format!(" = {}", p.default.trim()));
                }
                if p.mode.contains("OUT") {
                    s.push_str(" OUTPUT");
                }
                if !p.options.trim().is_empty() {
                    s.push_str(&format!(" {}", p.options.trim()));
                }
                s
            })
            .collect();
        if self.kind == ObjectKind::Function {
            return format!("CREATE FUNCTION {name} ({})\nRETURNS {}{with}\nAS\n{body}", params.join(", "), full_type(&self.returns, &self.returns_size, &self.returns_options));
        }
        let params = if params.is_empty() { String::new() } else { format!("\n  {}", params.join(",\n  ")) };
        format!("CREATE PROCEDURE {name}{params}{with}\nAS\n{body}")
    }
}

/// `DEFINER=`user`@`host` ` from `user@host` (or CURRENT_USER).
fn definer_sql(definer: &str) -> String {
    let d = definer.trim();
    if d.is_empty() {
        return String::new();
    }
    if d.eq_ignore_ascii_case("CURRENT_USER") || d.eq_ignore_ascii_case("CURRENT_USER()") {
        return "DEFINER=CURRENT_USER ".into();
    }
    match d.rsplit_once('@') {
        Some((user, host)) => format!("DEFINER={}@{} ", db::ident(user), db::ident(host)),
        None => format!("DEFINER={} ", db::ident(d)),
    }
}

/// `VARCHAR(255) CHARSET utf8mb4`.
fn full_type(kind: &str, size: &str, options: &str) -> String {
    let mut s = kind.trim().to_owned();
    if !size.trim().is_empty() {
        s.push_str(&format!("({})", size.trim()));
    }
    if !options.trim().is_empty() {
        s.push(' ');
        s.push_str(options.trim());
    }
    s
}

/// A type cut in three: `decimal(10,2) unsigned` → (DECIMAL, 10,2, UNSIGNED).
fn split_type(text: &str) -> (String, String, String) {
    let t = text.trim();
    let end = t.find(|c: char| c == '(' || c.is_whitespace()).unwrap_or(t.len());
    let kind = t[..end].trim_matches(['[', ']']).to_owned();
    let mut rest = t[end..].trim_start();
    let mut size = String::new();
    if rest.starts_with('(') {
        let mut lex = Lex::new(rest, false);
        if let Some(inner) = lex.group() {
            size = inner.trim().to_owned();
            rest = &rest[lex.i..];
        }
    }
    let upper_simple = kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (if upper_simple { kind.to_ascii_uppercase() } else { kind }, size, rest.trim().to_owned())
}

/// A small reader of SQL text: words, names, parentheses, strings, skipping spaces and comments.
struct Lex<'a> {
    s: &'a str,
    i: usize,
    mssql: bool,
}

fn ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'#' | b'@') || c >= 0x80
}

impl<'a> Lex<'a> {
    fn new(s: &'a str, mssql: bool) -> Self {
        Self { s, i: 0, mssql }
    }

    fn skip(&mut self) {
        let b = self.s.as_bytes();
        loop {
            while self.i < b.len() && b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            let rest = &self.s[self.i..];
            if rest.starts_with("--") || (!self.mssql && rest.starts_with('#')) {
                self.i = rest.find('\n').map_or(b.len(), |n| self.i + n + 1);
            } else if let Some(inside) = rest.strip_prefix("/*") {
                self.i = inside.find("*/").map_or(b.len(), |n| self.i + n + 4);
            } else {
                break;
            }
        }
    }

    fn rest(&mut self) -> &'a str {
        self.skip();
        &self.s[self.i..]
    }

    /// The next word, upper case, not read.
    fn peek(&mut self) -> String {
        self.skip();
        let b = self.s.as_bytes();
        let mut j = self.i;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
            j += 1;
        }
        self.s[self.i..j].to_ascii_uppercase()
    }

    fn word(&mut self) -> String {
        let w = self.peek();
        self.i += w.len();
        w
    }

    fn eat(&mut self, w: &str) -> bool {
        if self.peek() == w {
            self.i += w.len();
            true
        } else {
            false
        }
    }

    fn eat_char(&mut self, c: char) -> bool {
        self.skip();
        if self.s[self.i..].starts_with(c) {
            self.i += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<String> {
        self.skip();
        let b = self.s.as_bytes();
        let close = match b.get(self.i) {
            Some(b'`') => '`',
            Some(b'"') => '"',
            Some(b'[') => ']',
            _ => {
                let start = self.i;
                while self.i < b.len() && ident_char(b[self.i]) {
                    self.i += 1;
                }
                return (self.i > start).then(|| self.s[start..self.i].to_owned());
            }
        };
        let mut out = String::new();
        let mut j = self.i + 1;
        loop {
            let k = self.s[j..].find(close)? + j;
            out.push_str(&self.s[j..k]);
            // Doubled: the character itself.
            if self.s[k + 1..].starts_with(close) {
                out.push(close);
                j = k + 2;
            } else {
                self.i = k + 1;
                return Some(out);
            }
        }
    }

    /// A name, maybe qualified: its parts.
    fn name(&mut self) -> Option<Vec<String>> {
        let mut parts = vec![self.ident()?];
        while self.s[self.i..].starts_with('.') {
            self.i += 1;
            parts.push(self.ident()?);
        }
        Some(parts)
    }

    /// What is between the parenthesis here and the one closing it.
    fn group(&mut self) -> Option<&'a str> {
        self.skip();
        if !self.s[self.i..].starts_with('(') {
            return None;
        }
        let start = self.i + 1;
        let end = scan(self.s, start, |depth, _, c| depth == 0 && c == b')')?;
        self.i = end + 1;
        Some(&self.s[start..end])
    }

    /// A quoted string, unquoted.
    fn string(&mut self) -> Option<String> {
        self.skip();
        let rest = &self.s[self.i..];
        let rest = rest.strip_prefix('N').filter(|r| self.mssql && r.starts_with('\'')).unwrap_or(rest);
        let skipped = self.s.len() - self.i - rest.len();
        let mut chars = rest.char_indices();
        let (_, q) = chars.next().filter(|(_, c)| *c == '\'' || *c == '"')?;
        let mut out = String::new();
        let mut escaped = false;
        let mut prev_quote = false;
        for (k, c) in chars {
            if prev_quote {
                if c == q {
                    out.push(q);
                    prev_quote = false;
                    continue;
                }
                self.i += skipped + k;
                return Some(out);
            }
            if escaped {
                out.push(match c {
                    'n' => '\n',
                    't' => '\t',
                    '0' => '\0',
                    c => c,
                });
                escaped = false;
            } else if c == '\\' && !self.mssql {
                escaped = true;
            } else if c == q {
                prev_quote = true;
            } else {
                out.push(c);
            }
        }
        prev_quote.then(|| {
            self.i = self.s.len();
            out
        })
    }

    /// The text up to one of `stops` (words, at the same depth, outside strings), not read.
    fn until(&mut self, stops: &[&str]) -> &'a str {
        self.skip();
        let start = self.i;
        let b = self.s.as_bytes();
        let end = scan(self.s, start, |depth, k, _| {
            depth == 0
                && (k == 0 || !ident_char(b[k - 1]))
                && stops.iter().any(|w| b.len() >= k + w.len() && self.s[k..k + w.len()].eq_ignore_ascii_case(w) && b.get(k + w.len()).is_none_or(|c| !ident_char(*c)))
        })
        .unwrap_or(b.len());
        self.i = end;
        self.s[start..end].trim()
    }
}

/// From `start`, the first place where `stop(depth, place, byte)` holds, outside strings and quoted
/// names, parentheses counted.
fn scan(s: &str, start: usize, stop: impl Fn(usize, usize, u8) -> bool) -> Option<usize> {
    let b = s.as_bytes();
    let (mut depth, mut quote): (usize, Option<u8>) = (0, None);
    let mut k = start;
    while k < b.len() {
        let c = b[k];
        match quote {
            Some(q) => {
                if c == b'\\' && q != b']' && q != b'`' {
                    k += 1;
                } else if c == q {
                    quote = None;
                }
            }
            None => {
                if stop(depth, k, c) {
                    return Some(k);
                }
                match c {
                    b'\'' | b'"' | b'`' => quote = Some(c),
                    b'[' => quote = Some(b']'),
                    b'(' => depth += 1,
                    b')' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        k += 1;
    }
    None
}

/// The parts of a list, cut on commas outside parentheses and strings.
fn split_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = scan(s, from, |depth, _, c| depth == 0 && c == b',') {
        out.push(s[from..k].trim());
        from = k + 1;
    }
    if !s[from..].trim().is_empty() {
        out.push(s[from..].trim());
    }
    out.into_iter().filter(|p| !p.is_empty()).collect()
}

/// `user@host` from a DEFINER clause's value (`root`@`%`).
fn definer_of(value: &str) -> String {
    value.split('@').map(|p| p.trim_matches(['`', '\'', '"'])).collect::<Vec<_>>().join("@")
}

/// Reads a CREATE PROCEDURE, FUNCTION or TRIGGER. None when it can't be laid out as a form (it is then
/// edited as SQL).
pub(super) fn parse(sql: &str, mssql: bool) -> Option<Form> {
    let h = super::objects::head(sql)?;
    let mut f = Form::new(h.kind, mssql, h.parts.last()?, "");
    f.security.clear();
    if mssql {
        f.schema = if h.parts.len() >= 2 { h.parts[h.parts.len() - 2].clone() } else { "dbo".into() };
    } else if let Some(at) = sql[..h.name.start].to_ascii_uppercase().find("DEFINER") {
        let mut lex = Lex::new(&sql[at + 7..h.name.start], false);
        lex.eat_char('=');
        lex.skip();
        let value = lex.rest().split_whitespace().next().unwrap_or_default();
        f.definer = definer_of(value);
    }
    let mut lex = Lex::new(sql, mssql);
    lex.i = h.name.end;
    if mssql { parse_mssql(&mut lex, &mut f)? } else { parse_mysql(&mut lex, &mut f)? }
    Some(f)
}

fn parse_mysql(lex: &mut Lex, f: &mut Form) -> Option<()> {
    if f.kind == ObjectKind::Trigger {
        f.timing = lex.word();
        let event = lex.word();
        f.events = EVENTS.map(|e| e == event);
        if !matches!(f.timing.as_str(), "BEFORE" | "AFTER") || !f.events.iter().any(|e| *e) || !lex.eat("ON") {
            return None;
        }
        f.table = lex.name()?.pop()?;
        if !(lex.eat("FOR") && lex.eat("EACH") && lex.eat("ROW")) {
            return None;
        }
        let order = lex.peek();
        if order == "FOLLOWS" || order == "PRECEDES" {
            lex.word();
            f.order = order;
            f.order_of = lex.ident()?;
        }
        f.body = lex.rest().trim().to_owned();
        return Some(());
    }
    let function = f.kind == ObjectKind::Function;
    for p in split_commas(lex.group()?) {
        let mut pl = Lex::new(p, false);
        let mode = if function { String::new() } else { ["INOUT", "IN", "OUT"].into_iter().find(|m| pl.eat(m)).unwrap_or_default().to_owned() };
        let name = pl.ident()?;
        let (kind, size, options) = split_type(pl.rest());
        f.params.push(FormParam { mode: if function { mode } else if mode.is_empty() { "IN".into() } else { mode }, name, kind, size, options, default: String::new() });
    }
    if function {
        if !lex.eat("RETURNS") {
            return None;
        }
        let kind = lex.ident()?;
        let size = lex.group().map(str::to_owned).unwrap_or_default();
        let mut options = Vec::new();
        loop {
            match lex.peek().as_str() {
                w @ ("UNSIGNED" | "SIGNED" | "ZEROFILL" | "BINARY") => {
                    lex.word();
                    options.push(w.to_owned());
                }
                w @ ("CHARSET" | "COLLATE") => {
                    lex.word();
                    options.push(format!("{w} {}", lex.ident()?));
                }
                "CHARACTER" => {
                    lex.word();
                    lex.eat("SET");
                    options.push(format!("CHARSET {}", lex.ident()?));
                }
                _ => break,
            }
        }
        (f.returns, f.returns_size, f.returns_options) = (kind.to_ascii_uppercase(), size.trim().to_owned(), options.join(" "));
    }
    // Characteristics, in any order; then the body.
    loop {
        match lex.peek().as_str() {
            "COMMENT" => {
                lex.word();
                f.comment = lex.string()?;
            }
            "LANGUAGE" => {
                lex.word();
                lex.word();
            }
            "NOT" => {
                lex.word();
                if !lex.eat("DETERMINISTIC") {
                    return None;
                }
                f.deterministic = false;
            }
            "DETERMINISTIC" => {
                lex.word();
                f.deterministic = true;
            }
            "CONTAINS" | "NO" => {
                let w = lex.word();
                lex.eat("SQL");
                f.access = format!("{w} SQL");
            }
            "READS" | "MODIFIES" => {
                let w = lex.word();
                lex.eat("SQL");
                lex.eat("DATA");
                f.access = format!("{w} SQL DATA");
            }
            "SQL" => {
                lex.word();
                if !lex.eat("SECURITY") {
                    return None;
                }
                f.security = lex.word();
            }
            _ => break,
        }
    }
    f.body = lex.rest().trim().to_owned();
    Some(())
}

/// SQL Server: what follows WITH, up to `stops` (its EXECUTE AS kept whole).
fn with_clause(lex: &mut Lex, stops: &[&str]) -> String {
    let mut out = String::new();
    loop {
        let part = lex.until(stops);
        out.push_str(part);
        if part.to_ascii_uppercase().ends_with("EXECUTE") && lex.eat("AS") {
            out.push_str(" AS ");
            continue;
        }
        return out.trim().to_owned();
    }
}

/// A SQL Server parameter: `@a int = 5 OUTPUT READONLY`.
fn parse_mssql_param(p: &str) -> Option<FormParam> {
    let mut pl = Lex::new(p, true);
    let name = pl.ident()?;
    pl.eat("AS");
    let kind = pl.name()?.join(".");
    let size = pl.group().map(|s| s.trim().to_owned()).unwrap_or_default();
    let mut param = FormParam { mode: "IN".into(), name, kind: if kind.contains('.') { kind } else { kind.to_ascii_uppercase() }, size, ..Default::default() };
    let mut options = Vec::new();
    loop {
        if pl.eat_char('=') {
            param.default = pl.until(&["OUT", "OUTPUT", "READONLY", "VARYING"]).to_owned();
            continue;
        }
        match pl.word().as_str() {
            "OUT" | "OUTPUT" => param.mode = "OUT".into(),
            "" => break,
            w => options.push(w.to_owned()),
        }
    }
    param.options = options.join(" ");
    Some(param)
}

fn parse_mssql(lex: &mut Lex, f: &mut Form) -> Option<()> {
    match f.kind {
        ObjectKind::Trigger => {
            if !lex.eat("ON") {
                return None;
            }
            f.table = lex.name()?.join(".");
            if !f.table.contains('.') {
                f.table = format!("dbo.{}", f.table);
            }
            if lex.eat("WITH") {
                f.with = with_clause(lex, &["FOR", "AFTER", "INSTEAD"]);
            }
            f.timing = match lex.word().as_str() {
                "FOR" | "AFTER" => "AFTER".into(),
                "INSTEAD" if lex.eat("OF") => "INSTEAD OF".into(),
                _ => return None,
            };
            let events = lex.until(&["WITH", "NOT", "AS"]);
            f.events = EVENTS.map(|e| events.split(',').any(|x| x.trim().eq_ignore_ascii_case(e)));
            if lex.eat("WITH") {
                lex.until(&["NOT", "AS"]);
            }
            if lex.eat("NOT") {
                lex.until(&["AS"]);
            }
        }
        ObjectKind::Procedure => {
            let list = lex.until(&["WITH", "AS", "FOR"]);
            let list = list.strip_prefix('(').and_then(|l| l.strip_suffix(')')).unwrap_or(list);
            for p in split_commas(list) {
                f.params.push(parse_mssql_param(p)?);
            }
            if lex.eat("WITH") {
                f.with = with_clause(lex, &["FOR", "AS"]);
            }
            if lex.eat("FOR") {
                lex.until(&["AS"]);
            }
        }
        ObjectKind::Function => {
            for p in split_commas(lex.group()?) {
                f.params.push(parse_mssql_param(p)?);
            }
            if !lex.eat("RETURNS") {
                return None;
            }
            let returns = lex.until(&["WITH", "AS", "BEGIN", "RETURN"]);
            // A simple type cut in parts; TABLE or a table variable kept whole.
            if returns.to_ascii_uppercase().contains("TABLE") {
                (f.returns, f.returns_size, f.returns_options) = (returns.to_owned(), String::new(), String::new());
            } else {
                (f.returns, f.returns_size, f.returns_options) = split_type(returns);
            }
            if lex.eat("WITH") {
                f.with = with_clause(lex, &["AS", "BEGIN", "RETURN"]);
            }
        }
    }
    lex.eat("AS");
    f.body = lex.rest().trim().to_owned();
    (!f.body.is_empty()).then_some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Engine;

    const MY: Dialect = Dialect(Engine::Mysql);
    const MS: Dialect = Dialect(Engine::Sqlserver);

    #[test]
    fn reads_mysql_routines() {
        let sql = "CREATE DEFINER=`root`@`%` PROCEDURE `p`(IN `a` INT(11), OUT b decimal(10,2) unsigned, INOUT c VARCHAR(20) CHARSET utf8mb4)\n    READS SQL DATA\n    SQL SECURITY INVOKER\n    COMMENT 'l''essai, dit \\'x\\''\nBEGIN\n  SELECT a;\nEND";
        let f = parse(sql, false).unwrap();
        assert_eq!((f.kind, f.name.as_str(), f.definer.as_str()), (ObjectKind::Procedure, "p", "root@%"));
        assert_eq!(f.params[1], FormParam { mode: "OUT".into(), name: "b".into(), kind: "DECIMAL".into(), size: "10,2".into(), options: "unsigned".into(), default: String::new() });
        assert_eq!((f.params[2].mode.as_str(), f.params[2].options.as_str()), ("INOUT", "CHARSET utf8mb4"));
        assert_eq!((f.access.as_str(), f.security.as_str(), f.comment.as_str()), ("READS SQL DATA", "INVOKER", "l'essai, dit 'x'"));
        assert_eq!(f.body, "BEGIN\n  SELECT a;\nEND");
        // Written back and read again: the same.
        assert_eq!(parse(&f.sql(MY), false).unwrap(), f);

        let f = parse("CREATE FUNCTION f(x INT) RETURNS varchar(5) CHARSET latin1 DETERMINISTIC RETURN CONCAT(x, 'a')", false).unwrap();
        assert_eq!((f.returns.as_str(), f.returns_size.as_str(), f.returns_options.as_str()), ("VARCHAR", "5", "CHARSET latin1"));
        assert!(f.deterministic && f.params[0].mode.is_empty());
        assert_eq!(f.body, "RETURN CONCAT(x, 'a')");
        assert_eq!(f.sql(MY), "CREATE FUNCTION `f`(`x` INT) RETURNS VARCHAR(5) CHARSET latin1\nDETERMINISTIC\nRETURN CONCAT(x, 'a')");
    }

    #[test]
    fn reads_mysql_triggers() {
        let f = parse("CREATE DEFINER=`u`@`localhost` TRIGGER `t1` AFTER UPDATE ON `orders` FOR EACH ROW FOLLOWS `t0` SET @n = @n + 1", false).unwrap();
        assert_eq!((f.timing.as_str(), f.events, f.table.as_str()), ("AFTER", [false, true, false], "orders"));
        assert_eq!((f.order.as_str(), f.order_of.as_str(), f.body.as_str()), ("FOLLOWS", "t0", "SET @n = @n + 1"));
        assert_eq!(f.sql(MY), "CREATE DEFINER=`u`@`localhost` TRIGGER `t1` AFTER UPDATE ON `orders`\nFOR EACH ROW\nFOLLOWS `t0`\nSET @n = @n + 1");
        assert_eq!(parse(&f.sql(MY), false).unwrap(), f);
    }

    #[test]
    fn reads_sql_server_objects() {
        let f = parse("CREATE PROCEDURE [sales].[p]\n  @id int,\n  @name nvarchar(50) = N'x, y',\n  @n int OUTPUT\nWITH EXECUTE AS OWNER\nAS\nBEGIN\n  SELECT 1;\nEND", true).unwrap();
        assert_eq!((f.schema.as_str(), f.name.as_str(), f.with.as_str()), ("sales", "p", "EXECUTE AS OWNER"));
        assert_eq!(f.params[1], FormParam { mode: "IN".into(), name: "@name".into(), kind: "NVARCHAR".into(), size: "50".into(), options: String::new(), default: "N'x, y'".into() });
        assert_eq!(f.params[2].mode, "OUT");
        assert_eq!(parse(&f.sql(MS), true).unwrap(), f);

        let f = parse("create function dbo.rows_of (@id int) returns table as return (select * from t where id = @id)", true).unwrap();
        assert_eq!((f.returns.as_str(), f.body.as_str()), ("table", "return (select * from t where id = @id)"));
        assert_eq!(parse(&f.sql(MS), true).unwrap(), f);

        let f = parse("CREATE TRIGGER dbo.tr ON dbo.orders FOR INSERT, DELETE NOT FOR REPLICATION AS SET NOCOUNT ON;", true).unwrap();
        assert_eq!((f.table.as_str(), f.timing.as_str(), f.events), ("dbo.orders", "AFTER", [true, false, true]));
        assert_eq!(f.sql(MS), "CREATE TRIGGER [dbo].[tr] ON [dbo].[orders]\nAFTER INSERT, DELETE\nAS\nSET NOCOUNT ON;");
    }

    #[test]
    fn new_forms_make_valid_sql() {
        let mut f = Form::new(ObjectKind::Function, false, "f", "");
        f.params.push(FormParam { name: "x".into(), kind: "INT".into(), ..Default::default() });
        assert_eq!(parse(&f.sql(MY), false).unwrap(), f);
        let f = Form::new(ObjectKind::Trigger, true, "t", "dbo.orders");
        assert_eq!(parse(&f.sql(MS), true).unwrap(), f);
    }
    /// As MariaDB 10.6 gives them back (SHOW CREATE).
    #[test]
    fn reads_what_mariadb_gives() {
        let p = "CREATE DEFINER=`julienlecointe`@`localhost` PROCEDURE `p`(IN x INT UNSIGNED, OUT y VARCHAR(20) CHARSET utf8mb4)\n    READS SQL DATA\n    SQL SECURITY INVOKER\n    COMMENT 'c''est'\nBEGIN SET y = CONCAT(x, '!'); END";
        let f = parse(p, false).unwrap();
        assert_eq!((f.definer.as_str(), f.params[0].options.as_str(), f.params[1].size.as_str(), f.comment.as_str()), ("julienlecointe@localhost", "UNSIGNED", "20", "c'est"));
        assert_eq!(parse(&f.sql(MY), false).unwrap(), f);
        let fun = parse("CREATE DEFINER=`julienlecointe`@`localhost` FUNCTION `f`(x DECIMAL(10,2)) RETURNS int(11)\n    DETERMINISTIC\nRETURN x * 2", false).unwrap();
        assert_eq!((fun.returns.as_str(), fun.returns_size.as_str(), fun.params[0].size.as_str(), fun.body.as_str()), ("INT", "11", "10,2", "RETURN x * 2"));
        let tr = parse("CREATE DEFINER=`julienlecointe`@`localhost` TRIGGER tr BEFORE INSERT ON t FOR EACH ROW SET NEW.a = NEW.a + 1", false).unwrap();
        assert_eq!((tr.table.as_str(), tr.body.as_str()), ("t", "SET NEW.a = NEW.a + 1"));
    }
}
