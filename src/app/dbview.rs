//! The database view of a MariaDB / MySQL or SQL Server tab, phpMyAdmin style: the databases and their
//! tables on the left; on the right a database's tables, a table's content (paged, sortable) and
//! structure, and a SQL editor. Changes that destroy (dropping, emptying) are asked for first. The SQL
//! written for the changes follows the server's dialect.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::*;
use crate::db::{self, Cell, DatabaseInfo, Dialect, Event, ObjectKind, QueryResult, Request, Routine, Structure, TableInfo, Trigger, UserInfo};

mod form;
mod objects;
use objects::{CodeEdit, Group, ObjectDialog};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Database,
    Content,
    Structure,
    Sql,
    Users,
    /// A database's procedures and functions.
    Routines,
    /// A database's triggers, or a table's.
    Triggers,
}

/// What a change sent is, to tell how it went once done.
#[derive(Clone)]
enum Expect {
    /// A cell written: it must have changed one row.
    Cell,
    Delete,
    Insert,
    /// A table created or renamed: shown once done.
    Open(String, String),
    /// Accounts changed: listed again.
    Users,
    /// A procedure, function or trigger written: open under its name (maybe new) once done.
    Saved { db: String, kind: ObjectKind, name: String, fresh: bool },
    /// One dropped: closed if open.
    Dropped { db: String, kind: ObjectKind, name: String },
}

#[derive(Clone, Copy)]
enum PendingNotice {
    CellUnchanged,
    Deleted,
    Inserted,
    NoForeignKeys,
    CodeSaved,
}

/// How the last change went, shown above the page for a while.
struct Notice {
    text: String,
    warning: bool,
    at: Instant,
}

enum Status {
    Connecting,
    Ready(String),
    Closed(String),
}

/// Rows of a table, as shown.
struct RowsView {
    db: String,
    table: String,
    offset: u64,
    order: Option<(String, bool)>,
    result: QueryResult,
    total: u64,
    exact: bool,
}

/// A cell of a table's content being changed in place.
pub(super) struct CellEdit {
    row: usize,
    col: usize,
    text: String,
    fresh: bool,
}

/// A column's definition, as edited.
#[derive(Clone)]
struct ColumnEdit {
    original: Option<String>,
    name: String,
    kind: String,
    nullable: bool,
    default: String,
    default_null: bool,
    /// The default is an expression (CURRENT_TIMESTAMP, uuid()...), not a value.
    default_expr: bool,
    /// AUTO_INCREMENT and the like, kept as they were.
    extra: String,
    comment: String,
    /// The column's own character set and collation, kept (for text types).
    charset: String,
    collation: String,
}

/// A text type (which has a character set).
fn textual(kind: &str) -> bool {
    let k = kind.trim().to_lowercase();
    k.contains("char") || k.contains("text") || k.starts_with("enum") || k.starts_with("set")
}

/// A default that is an expression: CURRENT_TIMESTAMP, a function call, something in parentheses.
fn is_expression(default: &str) -> bool {
    let d = default.trim();
    let upper = d.to_uppercase();
    if upper == "CURRENT_TIMESTAMP" || upper == "NULL" || d.starts_with('(') {
        return true;
    }
    match d.find('(') {
        Some(i) => d.ends_with(')') && i > 0 && d[..i].chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        None => false,
    }
}

/// A default as information_schema gives it (MariaDB quotes values): the value, and whether it is an
/// expression.
fn parse_default(raw: &str, extra: &str) -> (String, bool) {
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        return (raw[1..raw.len() - 1].replace("''", "'").replace("\\\\", "\\"), false);
    }
    (raw.to_owned(), extra.to_uppercase().contains("DEFAULT_GENERATED") || is_expression(raw))
}

impl ColumnEdit {
    /// A computed column (its value comes from an expression): not edited here.
    fn generated(&self) -> bool {
        let e = self.extra.to_uppercase();
        e.contains("VIRTUAL") || e.contains("STORED") || e.contains("PERSISTENT")
    }

    /// The ALTER TABLE statement for it. SQL Server: the type, NULL and collation of an existing column
    /// (renamed first if its name changed; its default and comment are left as they are), in the
    /// database's context.
    fn sql(&self, dialect: Dialect, db_name: &str, table: &str) -> String {
        if dialect.mssql() {
            return self.mssql(dialect, db_name, table);
        }
        let mut def = format!("{} {}", db::ident(self.name.trim()), self.kind.trim());
        if textual(&self.kind) {
            if !self.charset.is_empty() {
                def.push_str(&format!(" CHARACTER SET {}", self.charset));
            }
            if !self.collation.is_empty() {
                def.push_str(&format!(" COLLATE {}", self.collation));
            }
        }
        def.push_str(if self.nullable { " NULL" } else { " NOT NULL" });
        let default = self.default.trim();
        if self.default_null && self.nullable {
            def.push_str(" DEFAULT NULL");
        } else if !default.is_empty() {
            def.push_str(&format!(" DEFAULT {}", if self.default_expr { default.to_owned() } else { db::literal(default) }));
        }
        let extra = self.extra.to_uppercase().replace("DEFAULT_GENERATED", "");
        let extra = extra.trim();
        if !extra.is_empty() {
            def.push(' ');
            def.push_str(extra);
        }
        if !self.comment.trim().is_empty() {
            def.push_str(&format!(" COMMENT {}", db::literal(self.comment.trim())));
        }
        let table = format!("{}.{}", db::ident(db_name), db::ident(table));
        match &self.original {
            Some(old) => format!("ALTER TABLE {table} CHANGE COLUMN {} {def}", db::ident(old)),
            None => format!("ALTER TABLE {table} ADD COLUMN {def}"),
        }
    }

    fn mssql(&self, dialect: Dialect, db_name: &str, table: &str) -> String {
        let q = |n: &str| dialect.ident(n);
        let full = dialect.table(db_name, table);
        let null = if self.nullable { " NULL" } else { " NOT NULL" };
        let Some(old) = &self.original else {
            let mut sql = format!("ALTER TABLE {full} ADD {} {}{null}", q(self.name.trim()), self.kind.trim());
            let default = self.default.trim();
            if !(self.default_null && self.nullable) && !default.is_empty() {
                sql.push_str(&format!(" DEFAULT {}", if self.default_expr { default.to_owned() } else { dialect.literal(default) }));
            }
            return sql;
        };
        let collate = if textual(&self.kind) && !self.collation.is_empty() { format!(" COLLATE {}", self.collation) } else { String::new() };
        let mut sql = format!("ALTER TABLE {full} ALTER COLUMN {} {}{collate}{null}", q(old), self.kind.trim());
        let name = self.name.trim();
        if name != old {
            sql.push_str(&format!(";\nEXEC sp_rename {}, {}, 'COLUMN'", dialect.literal(&format!("{}.{}", dialect.local_table(table), q(old))), dialect.literal(name)));
        }
        sql
    }
}

/// Column types offered (anything else can be typed).
const COLUMN_TYPES: [&str; 14] = ["INT", "BIGINT", "TINYINT(1)", "SMALLINT", "DECIMAL(10,2)", "DOUBLE", "VARCHAR(255)", "CHAR(36)", "TEXT", "LONGTEXT", "DATE", "DATETIME", "TIMESTAMP", "JSON"];
const MSSQL_COLUMN_TYPES: [&str; 14] = ["INT", "BIGINT", "BIT", "SMALLINT", "DECIMAL(10,2)", "FLOAT", "NVARCHAR(255)", "NVARCHAR(MAX)", "VARCHAR(255)", "UNIQUEIDENTIFIER", "DATE", "DATETIME2", "DATETIMEOFFSET", "VARBINARY(MAX)"];

fn column_types(dialect: Dialect) -> &'static [&'static str] {
    if dialect.mssql() { &MSSQL_COLUMN_TYPES } else { &COLUMN_TYPES }
}

/// A table name typed for SQL Server, in dbo unless a schema is given.
fn with_schema(dialect: Dialect, name: &str) -> String {
    let name = name.trim();
    if dialect.mssql() && !name.contains('.') { format!("dbo.{name}") } else { name.to_owned() }
}

/// What a cell's right-click menu asked.
#[derive(Clone, Copy, PartialEq)]
enum CellMenu {
    Copy,
    Edit,
    /// Edit in a window (long values).
    Window,
    Null,
}

/// A column of a table being created.
#[derive(Clone)]
struct NewColumn {
    name: String,
    kind: String,
    nullable: bool,
    default: String,
    primary: bool,
    auto: bool,
}

impl NewColumn {
    fn def(&self, dialect: Dialect) -> String {
        let mut def = format!("{} {}", dialect.ident(self.name.trim()), self.kind.trim());
        if self.auto && dialect.mssql() {
            def.push_str(" IDENTITY(1,1)");
        }
        def.push_str(if self.nullable && !self.primary { " NULL" } else { " NOT NULL" });
        let default = self.default.trim();
        if !default.is_empty() && !self.auto {
            def.push_str(&format!(" DEFAULT {}", if is_expression(default) { default.to_owned() } else { dialect.literal(default) }));
        }
        if self.auto && !dialect.mssql() {
            def.push_str(" AUTO_INCREMENT");
        }
        def
    }
}

/// CREATE TABLE for `name` in `db_name`.
fn create_table_sql(dialect: Dialect, db_name: &str, name: &str, columns: &[NewColumn]) -> String {
    let mut lines: Vec<String> = columns.iter().filter(|c| !c.name.trim().is_empty()).map(|c| format!("  {}", c.def(dialect))).collect();
    let keys: Vec<String> = columns.iter().filter(|c| c.primary && !c.name.trim().is_empty()).map(|c| dialect.ident(c.name.trim())).collect();
    if !keys.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", keys.join(", ")));
    }
    if dialect.mssql() {
        return format!("CREATE TABLE {} (\n{}\n)", dialect.table(db_name, &with_schema(dialect, name)), lines.join(",\n"));
    }
    format!("CREATE TABLE {}.{} (\n{}\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci", db::ident(db_name), db::ident(name.trim()), lines.join(",\n"))
}

/// How a value of a new row is given.
#[derive(Clone, Copy, PartialEq)]
enum FieldMode {
    Value,
    Null,
    /// The column's default (or the next AUTO_INCREMENT): left out of the INSERT.
    Default,
}

#[derive(Clone)]
struct InsertField {
    name: String,
    kind: String,
    mode: FieldMode,
    text: String,
    nullable: bool,
}

/// INSERT of one row in `db_name`.`table`.
fn insert_sql(dialect: Dialect, db_name: &str, table: &str, fields: &[InsertField]) -> String {
    let given: Vec<&InsertField> = fields.iter().filter(|f| f.mode != FieldMode::Default).collect();
    let columns: Vec<String> = given.iter().map(|f| dialect.ident(&f.name)).collect();
    let values: Vec<String> = given.iter().map(|f| if f.mode == FieldMode::Null { "NULL".to_owned() } else { dialect.literal(&f.text) }).collect();
    // Every value left to its default.
    if given.is_empty() && dialect.mssql() {
        return format!("INSERT INTO {} DEFAULT VALUES", dialect.table(db_name, table));
    }
    format!("INSERT INTO {} ({}) VALUES ({})", dialect.table(db_name, table), columns.join(", "), values.join(", "))
}

/// What an account may do on a database (or all of them).
#[derive(Clone, Copy, PartialEq)]
enum Access {
    Nothing,
    Read,
    /// Read and change rows.
    Write,
    All,
}

impl Access {
    fn privileges(self) -> Option<&'static str> {
        match self {
            Access::Nothing => None,
            Access::Read => Some("SELECT, SHOW VIEW"),
            Access::Write => Some("SELECT, INSERT, UPDATE, DELETE, SHOW VIEW"),
            Access::All => Some("ALL PRIVILEGES"),
        }
    }
}

/// GRANT of `access` on `database` (empty: all of them) to an account.
fn grant_sql(access: Access, database: &str, user: &str, host: &str) -> Option<String> {
    let scope = if database.is_empty() { "*.*".to_owned() } else { format!("{}.*", db::ident(database)) };
    access.privileges().map(|p| format!("GRANT {p} ON {scope} TO {}", db::account(user, host)))
}

#[derive(Clone)]
enum Dialog {
    NewDatabase { name: String, fresh: bool },
    /// Export (of a database, or one of its tables) or import: its options, then the file.
    Transfer { db: Option<String>, table: Option<String>, import: bool, gzip: bool, check_fk: bool },
    NewTable { db: String, name: String, columns: Vec<NewColumn>, fresh: bool },
    RenameTable { db: String, old: String, name: String, fresh: bool },
    /// A row added to the table shown.
    Insert(Vec<InsertField>),
    /// A long or multi-line value edited in a window.
    Cell { row: usize, col: usize, column: String, text: String },
    /// A new account: name, host, password (and whether it is shown), what it may do and where.
    NewUser { user: String, host: String, password: String, reveal: bool, access: Access, database: String },
    Password { user: String, host: String, password: String, reveal: bool },
    Grant { user: String, host: String, access: Access, database: String },
    /// A column added (`original` None) or changed.
    Column(ColumnEdit),
    /// A destructive statement, run once confirmed (`query`: as a query, its results shown).
    /// `fk`: dropping or emptying tables on MySQL, whether the foreign keys are checked (a box in the
    /// window; off, tables that refer to each other go too).
    Confirm { sql: String, db: Option<String>, reason: String, query: bool, expect: Option<Expect>, fk: Option<bool> },
    /// A change made through the interface, its SQL shown before it runs.
    Review(Change),
}

/// A change made through the interface (a cell written, a row added, a column changed...).
#[derive(Clone)]
struct Change {
    db: Option<String>,
    sql: String,
    expect: Option<Expect>,
    /// The rows shown are read again once it is sent.
    reload: bool,
    /// The account to select once it is sent (one just created).
    select_user: Option<(String, String)>,
    /// The dialog it came from, shown again if the change is cancelled.
    back: Option<Box<Dialog>>,
}

impl Change {
    fn new(sql: String, expect: Option<Expect>) -> Self {
        Self { db: None, sql, expect, reload: false, select_user: None, back: None }
    }
}

/// What a query gave: its results, how long it took, the error it stopped on.
type QueryOut = (Vec<QueryResult>, Duration, Option<String>);

/// A tab of the SQL page: its query, and the results of the last run.
#[derive(Default)]
struct SqlTab {
    id: u32,
    sql: String,
    out: Option<QueryOut>,
    colors: Option<(String, egui::text::LayoutJob)>,
    complete: super::sqlcomplete::Completer,
}

impl SqlTab {
    fn new(id: u32) -> Self {
        Self { id, ..Default::default() }
    }
}

pub(super) enum DbAction {
    None,
}

pub(super) struct DbView {
    name: String,
    target: db::Target,
    /// How the server writes SQL.
    dialect: Dialect,
    conn: Option<db::Connection>,
    status: Status,
    databases: Vec<DatabaseInfo>,
    filter: String,
    expanded: HashSet<String>,
    tables: HashMap<String, Vec<TableInfo>>,
    db: Option<String>,
    table: Option<String>,
    page: Page,
    rows: Option<RowsView>,
    rows_loading: bool,
    limit: u64,
    structure: Option<(String, String, Structure)>,
    /// The SQL page's tabs (never empty), the one shown, the id of the next one.
    sql_tabs: Vec<SqlTab>,
    sql_tab: usize,
    next_sql_tab: u32,
    sql_running: bool,
    /// Where the result of the query sent goes: a SQL tab (by id), else the query above a table.
    query_for: Option<u32>,
    /// The query field above a table's content, its result, and whether it is shown instead of the rows.
    table_sql: String,
    /// The query that read the rows shown, put in `table_sql`: replaced by the next one while it is
    /// left as it is.
    table_sql_auto: String,
    table_out: Option<QueryOut>,
    table_query: bool,
    editing: Option<CellEdit>,
    /// An export or import going on (what, how far, on what), then how it ended.
    transfer: Option<(String, f32, String)>,
    transfer_done: Option<Result<String, String>>,
    /// The last import (database, file, foreign keys checked): run again without the checks when they stopped it.
    last_import: Option<(Option<String>, std::path::PathBuf, bool)>,
    /// Import, export and dropping or emptying ticked tables with foreign keys checked.
    check_fk: bool,
    /// Exports compressed (.sql.gz).
    gzip: bool,
    /// Rows ticked (to delete them), by index in the page shown.
    selected: HashSet<usize>,
    /// Tables ticked on a database's page (to drop or empty them), and that database.
    ticked: (String, HashSet<String>),
    /// Text searched in the table's columns (applied with Enter), and what is typed.
    search: String,
    search_typed: String,
    /// The whole value of the cell clicked last.
    detail: Option<(String, String)>,
    dialog: Option<Dialog>,
    error: Option<String>,
    /// The connection's id (its query history), and the SSH host it goes through.
    id: uuid::Uuid,
    via: Option<String>,
    /// The ssh process of a forward just opened: the window must answer its prompts.
    new_tunnel_pid: Option<u32>,
    expect: HashMap<u32, Expect>,
    next_tag: u32,
    notice: Option<Notice>,
    /// The transfer going on is an import (an error then leaves what came before it applied).
    transfer_import: bool,
    /// Queries run and changes made, the latest first; the text searched in it.
    history: Vec<super::sql::Entry>,
    history_filter: String,
    /// The history entry of the query running, and of each change sent (by tag), with when it left.
    running_entry: Option<(u64, Instant)>,
    exec_entries: HashMap<u32, (u64, Instant)>,
    /// Colors of the SQL typed above a table, rebuilt when it changes.
    table_sql_colors: Option<(String, egui::text::LayoutJob)>,
    /// Columns of each database's tables (to complete the SQL typed), asked for once per database
    /// until the structure may have changed; the suggestions of the SQL field above a table.
    columns: HashMap<String, HashMap<String, Vec<(String, String)>>>,
    columns_asked: HashSet<String>,
    table_sql_complete: super::sqlcomplete::Completer,
    /// Show the SQL of a change made through the interface before it runs (a setting).
    pub confirm_changes: bool,
    /// Set when a change is done (the texts come with the next frame), and a table to open then.
    pending_notice: Option<(PendingNotice, u64)>,
    open_after: Option<(String, String)>,
    users: Option<Vec<UserInfo>>,
    users_asked: bool,
    user: Option<(String, String)>,
    grants: Option<(String, String, Vec<String>)>,
    /// Procedures and functions, and triggers, of each database listed.
    routines: HashMap<String, Vec<Routine>>,
    triggers: HashMap<String, Vec<Trigger>>,
    /// The procedure, function or trigger open in the code editor; the text searched in their lists.
    code: Option<CodeEdit>,
    object_filter: String,
    object_dialog: Option<ObjectDialog>,
}

/// The columns that tell a row apart: the primary key, else a unique key without NULLs.
fn row_key(s: &Structure) -> Vec<String> {
    let name = |c: &Vec<Cell>| match c.first() {
        Some(Cell::Text(n)) => Some(n.clone()),
        _ => None,
    };
    let primary: Vec<String> = s.columns.iter().filter(|c| c.get(3) == Some(&Cell::Text("PRI".into()))).filter_map(name).collect();
    if !primary.is_empty() {
        return primary;
    }
    let not_null = |col: &str| s.columns.iter().any(|c| name(c).as_deref() == Some(col) && c.get(2) == Some(&Cell::Text("NO".into())));
    s.indexes
        .iter()
        .filter(|i| i.get(2) == Some(&Cell::Text("UNIQUE".into())))
        .filter_map(|i| match i.get(1) {
            Some(Cell::Text(cols)) => Some(cols.split(", ").map(str::to_owned).collect::<Vec<_>>()),
            _ => None,
        })
        .find(|cols| cols.iter().all(|c| not_null(c)))
        .unwrap_or_default()
}

/// Databases every server has: shown, but dimmed and last.
fn system_db(dialect: Dialect, name: &str) -> bool {
    if dialect.mssql() {
        return crate::mssql::system_db(name);
    }
    matches!(name, "information_schema" | "mysql" | "performance_schema" | "sys")
}

impl DbView {
    pub fn new(ctx: &egui::Context, conn: &config::DbConnection, password: Option<String>, tunnel: Option<(crate::ssh::Launch, String)>) -> Self {
        let via = tunnel.as_ref().map(|(_, name)| name.clone());
        let target = db::Target { host: conn.host.clone(), port: conn.port, user: conn.user.clone(), password, database: conn.database.clone(), tunnel: tunnel.map(|(launch, _)| launch), engine: conn.engine, trust_cert: conn.trust_cert };
        let connection = db::Connection::open(ctx, target.clone());
        let new_tunnel_pid = connection.tunnel_pid;
        let mut view = Self {
            name: conn.name.clone(),
            target,
            dialect: Dialect(conn.engine),
            conn: Some(connection),
            status: Status::Connecting,
            databases: Vec::new(),
            filter: String::new(),
            expanded: HashSet::new(),
            tables: HashMap::new(),
            db: None,
            table: None,
            page: Page::Database,
            rows: None,
            rows_loading: false,
            limit: 100,
            structure: None,
            sql_tabs: vec![SqlTab::new(0)],
            sql_tab: 0,
            next_sql_tab: 1,
            sql_running: false,
            query_for: None,
            table_sql: String::new(),
            table_sql_auto: String::new(),
            table_out: None,
            table_query: false,
            editing: None,
            transfer: None,
            transfer_done: None,
            last_import: None,
            check_fk: true,
            gzip: true,
            selected: HashSet::new(),
            ticked: (String::new(), HashSet::new()),
            search: String::new(),
            search_typed: String::new(),
            detail: None,
            dialog: None,
            error: None,
            id: conn.id,
            via,
            new_tunnel_pid,
            expect: HashMap::new(),
            next_tag: 0,
            notice: None,
            transfer_import: false,
            history: super::sql::load(conn.id),
            history_filter: String::new(),
            running_entry: None,
            exec_entries: HashMap::new(),
            table_sql_colors: None,
            columns: HashMap::new(),
            columns_asked: HashSet::new(),
            table_sql_complete: Default::default(),
            confirm_changes: true,
            pending_notice: None,
            open_after: None,
            users: None,
            users_asked: false,
            user: None,
            grants: None,
            routines: HashMap::new(),
            triggers: HashMap::new(),
            code: None,
            object_filter: String::new(),
            object_dialog: None,
        };
        if let Some(d) = conn.database.clone().filter(|d| !d.is_empty()) {
            view.expanded.insert(d.clone());
            view.db = Some(d);
        }
        view
    }

    fn reconnect(&mut self, ctx: &egui::Context) {
        let conn = db::Connection::open(ctx, self.target.clone());
        self.new_tunnel_pid = conn.tunnel_pid;
        self.conn = Some(conn);
        self.status = Status::Connecting;
        self.error = None;
    }

    /// The ssh process of a forward opened since last asked.
    pub fn take_tunnel_pid(&mut self) -> Option<u32> {
        self.new_tunnel_pid.take()
    }

    /// Sends a change; `expect`: what it is, to tell how it went.
    fn exec(&mut self, db: Option<String>, sql: String, expect: Option<Expect>) {
        self.next_tag += 1;
        if let Some(e) = expect {
            self.expect.insert(self.next_tag, e);
        }
        let entry = self.remember(&sql, db.clone().or_else(|| self.db.clone()), true);
        if let Some(id) = entry {
            self.exec_entries.insert(self.next_tag, (id, Instant::now()));
        }
        self.send(Request::Exec { db, sql, tag: self.next_tag });
    }

    /// A change made through the interface: its SQL shown first (unless the setting says not to).
    fn change(&mut self, change: Change) {
        if self.confirm_changes {
            self.dialog = Some(Dialog::Review(change));
        } else {
            self.run_change(change);
        }
    }

    fn run_change(&mut self, change: Change) {
        let reload = change.reload.then(|| self.rows.as_ref().map(|r| (r.db.clone(), r.table.clone(), r.offset, r.order.clone()))).flatten();
        self.exec(change.db, change.sql, change.expect);
        if let Some(user) = change.select_user {
            self.user = Some(user);
        }
        // Read after the change (one request at a time).
        if let Some((d, tb, offset, order)) = reload {
            self.load_rows(d, tb, offset, order);
        }
    }

    /// Sets how the history entry `id` ended.
    fn finish_entry(&mut self, id: u64, since: Instant, outcome: super::sql::Outcome) {
        if let Some(e) = self.history.iter_mut().find(|e| e.id == id) {
            e.ms = Some(since.elapsed().as_millis() as u64);
            e.outcome = Some(outcome);
            super::sql::save(self.id, &self.history);
        }
    }

    fn notify(&mut self, text: String, warning: bool) {
        self.notice = Some(Notice { text, warning, at: Instant::now() });
    }

    fn send(&self, request: Request) {
        if let Some(conn) = &self.conn {
            conn.send(request);
        }
    }

    /// Handles what the server sent (every frame, for every tab).
    pub fn poll(&mut self) {
        let Some(conn) = &self.conn else { return };
        for event in conn.poll() {
            match event {
                Event::Connected { version } => {
                    self.status = Status::Ready(version);
                    self.columns_asked.clear();
                    self.send(Request::Databases);
                    for d in self.expanded.clone() {
                        self.ask_lists(d);
                    }
                    if let (Some(d), Some(t)) = (self.db.clone(), self.table.clone()) {
                        self.load_rows(d, t, 0, None);
                    }
                }
                Event::Databases(d) => self.databases = d,
                Event::Tables { db, tables } => {
                    self.tables.insert(db, tables);
                }
                Event::Columns { db, columns } => {
                    self.columns.insert(db, columns);
                }
                // Orphan rows asked: one query per foreign key, in a SQL tab of their own.
                Event::ForeignKeys { db, table, keys } => {
                    if keys.is_empty() {
                        self.pending_notice = Some((PendingNotice::NoForeignKeys, 0));
                    } else {
                        let mut tab = SqlTab::new(self.next_sql_tab);
                        self.next_sql_tab += 1;
                        tab.sql = db::orphans_sql(&db, &table, &keys);
                        let (id, sql) = (tab.id, tab.sql.clone());
                        self.sql_tabs.push(tab);
                        self.sql_tab = self.sql_tabs.len() - 1;
                        self.page = Page::Sql;
                        self.query_for = Some(id);
                        self.send_query(Some(db), sql);
                    }
                }
                Event::Structure { db, table, structure } => self.structure = Some((db, table, structure)),
                Event::Routines { db, routines } => {
                    self.routines.insert(db, routines);
                }
                Event::Triggers { db, triggers } => {
                    self.triggers.insert(db, triggers);
                }
                Event::Definition { db, kind, name, sql } => {
                    // Read again after a save: replaces the text unless it was changed meanwhile.
                    let mssql = self.dialect.mssql();
                    if let Some(c) = self.code.as_mut().filter(|c| c.db == db && c.kind == kind && c.name.as_deref() == Some(name.as_str())) {
                        c.loaded(sql, mssql);
                    }
                }
                Event::Rows { db, table, offset, result, total, exact, sql } => {
                    if self.db.as_deref() == Some(&db) && self.table.as_deref() == Some(&table) && (self.table_sql.trim().is_empty() || self.table_sql == self.table_sql_auto) {
                        self.table_sql_auto = super::sql::format(&sql);
                        self.table_sql = self.table_sql_auto.clone();
                    }
                    let order = self.rows.as_ref().filter(|r| r.db == db && r.table == table).and_then(|r| r.order.clone());
                    self.rows = Some(RowsView { db, table, offset, order, result, total, exact });
                    self.rows_loading = false;
                    self.selected.clear();
                    self.editing = None;
                }
                Event::Query { results, elapsed, error } => {
                    if let Some((id, since)) = self.running_entry.take() {
                        let outcome = match (&error, results.iter().rev().find(|r| !r.columns.is_empty())) {
                            (Some(e), _) => super::sql::Outcome::Error(e.clone()),
                            (None, Some(r)) => super::sql::Outcome::Rows(r.rows.len() as u64),
                            (None, None) => super::sql::Outcome::Affected(results.iter().map(|r| r.affected).sum()),
                        };
                        self.finish_entry(id, since, outcome);
                    }
                    // A change (UPDATE, DROP...), or a script stopped partway: the rows shown are reloaded,
                    // wherever it was typed.
                    let changed = error.is_some() || results.iter().any(|r| r.columns.is_empty());
                    let out = Some((results, elapsed, error));
                    match self.query_for.take() {
                        // A tab closed meanwhile: the result has nowhere to go.
                        Some(id) => {
                            if let Some(tab) = self.sql_tabs.iter_mut().find(|tab| tab.id == id) {
                                tab.out = out;
                            }
                        }
                        None => self.table_out = out,
                    }
                    self.sql_running = false;
                    // A query may have changed what is listed (and the columns, if it changed something).
                    if changed {
                        self.columns_asked.clear();
                        if let Some(r) = &self.rows {
                            let (d, tb, o, order) = (r.db.clone(), r.table.clone(), r.offset, r.order.clone());
                            self.load_rows(d, tb, o, order);
                        }
                    }
                    self.refresh_lists();
                }
                Event::Progress { fraction, text } => {
                    if let Some(t) = &mut self.transfer {
                        (t.1, t.2) = (fraction, text);
                    }
                }
                Event::Transfer(result) => {
                    self.transfer = None;
                    self.transfer_done = Some(result);
                }
                Event::Failed { mut error, tag } => {
                    if let Some(Expect::Saved { fresh, .. }) = self.expect.remove(&tag) {
                        if let Some(c) = &mut self.code {
                            c.saving = false;
                        }
                        // New: nothing was there before.
                        if fresh {
                            error = error.trim_start_matches(db::RESTORED).trim_start_matches(db::LOST).to_owned();
                        } else if error.starts_with(db::LOST) {
                            // The old one couldn't be put back: its code is kept in a SQL tab.
                            if let Some(original) = self.code.as_ref().map(|c| c.original.clone()) {
                                let page = self.page;
                                self.open_sql_tab(original, None);
                                self.page = page;
                            }
                        }
                    }
                    if let Some((id, since)) = self.exec_entries.remove(&tag) {
                        self.finish_entry(id, since, super::sql::Outcome::Error(error.clone()));
                    }
                    self.error = Some(error);
                    self.rows_loading = false;
                    // Part of it may have been applied (tables dropped before the one refused).
                    self.columns_asked.clear();
                    self.refresh();
                }
                Event::Done { affected, tag } => {
                    self.columns_asked.clear();
                    if let Some((id, since)) = self.exec_entries.remove(&tag) {
                        self.finish_entry(id, since, super::sql::Outcome::Affected(affected));
                    }
                    match self.expect.remove(&tag) {
                        Some(Expect::Cell) if affected == 0 => self.pending_notice = Some((PendingNotice::CellUnchanged, 0)),
                        Some(Expect::Cell) => {}
                        Some(Expect::Delete) => self.pending_notice = Some((PendingNotice::Deleted, affected)),
                        Some(Expect::Insert) => self.pending_notice = Some((PendingNotice::Inserted, affected)),
                        Some(Expect::Open(d, tb)) => self.open_after = Some((d, tb)),
                        Some(Expect::Saved { db, kind, name, .. }) => {
                            let dialect = self.dialect;
                            if let Some(c) = self.code.as_mut().filter(|c| c.db == db && c.saving) {
                                c.saved(dialect);
                                c.kind = kind;
                                c.name = Some(name.clone());
                            }
                            // As the server keeps it.
                            self.send(Request::Definition { db, kind, name });
                            self.pending_notice = Some((PendingNotice::CodeSaved, 0));
                        }
                        Some(Expect::Dropped { db, kind, name }) => {
                            if self.code.as_ref().is_some_and(|c| c.db == db && c.kind == kind && c.name.as_deref() == Some(name.as_str())) {
                                self.code = None;
                            }
                        }
                        Some(Expect::Users) => {
                            self.send(Request::Users);
                            if let Some((u, h)) = self.user.clone() {
                                self.send(Request::Grants { user: u, host: h });
                            }
                        }
                        None => {}
                    }
                    self.refresh_lists();
                    // A change of the table shown (its columns...): read it again.
                    if let (Some(d), Some(tb)) = (self.db.clone(), self.table.clone()) {
                        self.send(Request::Structure { db: d.clone(), table: tb.clone() });
                        if self.page == Page::Structure {
                            let (o, order) = self.rows.as_ref().map_or((0, None), |r| (r.offset, r.order.clone()));
                            self.load_rows(d, tb, o, order);
                        }
                    }
                }
                Event::Users(users) => self.users = Some(users),
                Event::Grants { user, host, grants } => self.grants = Some((user, host, grants)),
                Event::Error(e) => {
                    if let Some(c) = self.code.as_mut().filter(|c| c.loading) {
                        c.loading = false;
                    }
                    // Accounts that can't be listed (not allowed): the page says so instead of waiting.
                    if self.users_asked && self.users.is_none() {
                        self.users = Some(Vec::new());
                    }
                    if let (Some((u, h)), None) = (&self.user, &self.grants) {
                        self.grants = Some((u.clone(), h.clone(), Vec::new()));
                    }
                    self.error = Some(e);
                    self.rows_loading = false;
                    self.sql_running = false;
                }
                Event::Closed(e) => {
                    self.status = Status::Closed(e);
                    self.rows_loading = false;
                    self.sql_running = false;
                }
            }
        }
        if matches!(self.status, Status::Closed(_)) {
            self.conn = None;
        }
    }

    /// The columns of the selected database, asked for once a SQL field is typed in.
    fn want_columns(&mut self) {
        if let (Some(d), Status::Ready(_)) = (&self.db, &self.status) {
            if self.columns_asked.insert(d.clone()) {
                self.send(Request::Columns(d.clone()));
            }
        }
    }

    fn refresh_lists(&mut self) {
        self.send(Request::Databases);
        for d in self.expanded.clone() {
            self.ask_lists(d);
        }
    }

    /// The tables of database `d`, its procedures and functions, its triggers.
    fn ask_lists(&mut self, d: String) {
        self.send(Request::Tables(d.clone()));
        self.send(Request::Routines(d.clone()));
        self.send(Request::Triggers(d));
    }

    fn refresh(&mut self) {
        self.refresh_lists();
        if let Some(r) = &self.rows {
            let (d, t, o, order) = (r.db.clone(), r.table.clone(), r.offset, r.order.clone());
            self.load_rows(d, t, o, order);
        }
    }

    fn select_db(&mut self, name: &str) {
        if !self.tables.contains_key(name) {
            self.ask_lists(name.to_owned());
        }
        self.expanded.insert(name.to_owned());
        self.db = Some(name.to_owned());
        self.table = None;
        self.page = Page::Database;
        self.detail = None;
    }

    fn select_table(&mut self, db: &str, table: &str) {
        // Another table: its own query field and search.
        let other = self.table.as_deref() != Some(table) || self.db.as_deref() != Some(db);
        self.db = Some(db.to_owned());
        self.table = Some(table.to_owned());
        if !matches!(self.page, Page::Content | Page::Structure | Page::Triggers) {
            self.page = Page::Content;
        }
        self.detail = None;
        if other {
            self.table_sql.clear();
            self.table_sql_auto.clear();
            self.search.clear();
            self.search_typed.clear();
        }
        self.table_query = false;
        self.load_rows(db.to_owned(), table.to_owned(), 0, None);
        self.send(Request::Structure { db: db.to_owned(), table: table.to_owned() });
    }

    /// How to find row `row` of the page shown: by the table's primary key, or by all its values when it
    /// has none (binary values can't be matched then). None if it can't be told.
    fn row_condition(&self, db_name: &str, table: &str, row: usize) -> Option<String> {
        let rows = self.rows.as_ref().filter(|r| r.db == db_name && r.table == table)?;
        let values = rows.result.rows.get(row)?;
        let keys: Vec<String> = self.structure.as_ref().filter(|(a, b, _)| a == db_name && b == table).map(|(_, _, s)| row_key(s)).unwrap_or_default();
        let by_all = keys.is_empty();
        let d = self.dialect;
        // SQL Server can't compare these with "=".
        let kind = |name: &str| self.structure.as_ref().and_then(|(_, _, s)| s.columns.iter().find(|c| c.first() == Some(&Cell::Text(name.to_owned())))).and_then(|c| match c.get(1) {
            Some(Cell::Text(k)) => Some(k.to_lowercase()),
            _ => None,
        });
        let uncomparable = |name: &str| d.mssql() && kind(name).is_some_and(|k| matches!(k.as_str(), "text" | "ntext" | "image" | "xml") || k.ends_with("(max)") && k.contains("binary"));
        let mut conditions = Vec::new();
        for (i, name) in rows.result.columns.iter().enumerate() {
            if !by_all && !keys.contains(name) {
                continue;
            }
            if by_all && uncomparable(name) {
                continue;
            }
            conditions.push(match &values[i] {
                Cell::Null => format!("{} IS NULL", d.ident(name)),
                Cell::Text(v) => format!("{} = {}", d.ident(name), d.literal(v)),
                Cell::Bytes(_) if by_all => continue,
                Cell::Bytes(_) => return None,
            });
        }
        (!conditions.is_empty()).then(|| format!("({})", conditions.join(" AND ")))
    }

    fn cancel_query(&self) {
        if let Some(conn) = &self.conn {
            conn.cancel_query();
        }
    }

    /// Writes a new value (None: NULL) in cell (row, col) of the content shown, then reloads it.
    /// `back`: the dialog it was typed in, shown again if the change is cancelled.
    fn update_cell(&mut self, db_name: &str, table: &str, row: usize, col: usize, value: Option<String>, back: Option<Box<Dialog>>) {
        let Some(condition) = self.row_condition(db_name, table, row) else { return };
        let Some(rows) = self.rows.as_ref() else { return };
        let Some(column) = rows.result.columns.get(col) else { return };
        let d = self.dialect;
        let new_value = value.map_or_else(|| "NULL".to_owned(), |v| d.literal(&v));
        let sql = if d.mssql() {
            format!("UPDATE TOP (1) {} SET {} = {new_value} WHERE {condition}", d.table(db_name, table), d.ident(column))
        } else {
            format!("UPDATE {} SET {} = {new_value} WHERE {condition} LIMIT 1", d.table(db_name, table), d.ident(column))
        };
        self.change(Change { reload: true, back, ..Change::new(sql, Some(Expect::Cell)) });
    }

    fn new_table_dialog(&mut self, d: &str) {
        let (int, text) = if self.dialect.mssql() { ("INT", "NVARCHAR(255)") } else { ("INT UNSIGNED", "VARCHAR(255)") };
        let id = NewColumn { name: "id".into(), kind: int.into(), nullable: false, default: String::new(), primary: true, auto: true };
        let empty = NewColumn { name: String::new(), kind: text.into(), nullable: true, default: String::new(), primary: false, auto: false };
        self.dialog = Some(Dialog::NewTable { db: d.to_owned(), name: String::new(), columns: vec![id, empty], fresh: true });
    }

    /// A row to add, from the table's columns.
    fn insert_dialog(&mut self, d: &str, tb: &str) {
        let Some((_, _, s)) = self.structure.as_ref().filter(|(a, b, _)| a == d && b == tb) else { return };
        let text = |c: Option<&Cell>| match c {
            Some(Cell::Text(v)) => v.clone(),
            _ => String::new(),
        };
        let fields = s
            .columns
            .iter()
            .map(|c| {
                let nullable = text(c.get(2)) == "YES";
                let has_default = matches!(c.get(4), Some(Cell::Text(_)));
                let extra = text(c.get(5)).to_uppercase();
                // Filled by the server: AUTO_INCREMENT, generated (MySQL), IDENTITY, computed, rowversion (SQL Server).
                let auto = ["AUTO_INCREMENT", "GENERATED", "IDENTITY", "COMPUTED", "ROWVERSION"].iter().any(|k| extra.contains(k));
                let mode = if auto || has_default { FieldMode::Default } else if nullable { FieldMode::Null } else { FieldMode::Value };
                InsertField { name: text(c.first()), kind: text(c.get(1)), mode, text: String::new(), nullable }
            })
            .collect();
        self.dialog = Some(Dialog::Insert(fields));
    }

    /// Asks before deleting the rows ticked.
    fn confirm_delete_rows(&mut self, db_name: &str, table: &str, t: &Strings) {
        let mut rows: Vec<usize> = self.selected.iter().copied().collect();
        rows.sort_unstable();
        let conditions: Vec<String> = rows.iter().filter_map(|r| self.row_condition(db_name, table, *r)).collect();
        if conditions.is_empty() {
            return;
        }
        let (d, n) = (self.dialect, conditions.len());
        let sql = if d.mssql() {
            format!("DELETE TOP ({n}) FROM {} WHERE {}", d.table(db_name, table), conditions.join(" OR "))
        } else {
            format!("DELETE FROM {} WHERE {} LIMIT {n}", d.table(db_name, table), conditions.join(" OR "))
        };
        self.dialog = Some(Dialog::Confirm { sql, db: None, reason: t.db_delete_rows_reason.to_owned(), query: false, expect: Some(Expect::Delete), fk: None });
    }

    fn load_rows(&mut self, db: String, table: String, offset: u64, order: Option<(String, bool)>) {
        self.rows_loading = true;
        if let Some(r) = self.rows.as_mut().filter(|r| r.db == db && r.table == table) {
            r.order = order.clone();
        }
        // Searched in the columns of the rows shown (the table's).
        let columns: Vec<String> = self.rows.as_ref().filter(|r| r.db == db && r.table == table).map(|r| r.result.columns.clone()).unwrap_or_default();
        let search = (!self.search.is_empty()).then(|| (self.search.clone(), columns));
        self.send(Request::Rows { db, table, offset, limit: self.limit, order, search });
    }

    /// Export or import: the options asked first (SQL Server's scripts have none: the file at once).
    fn transfer_dialog(&mut self, db: Option<String>, table: Option<String>, import: bool, t: &Strings) {
        if import && self.dialect.mssql() {
            self.import(db.as_deref(), t);
        } else {
            self.dialog = Some(Dialog::Transfer { db, table, import, gzip: self.gzip, check_fk: self.check_fk });
        }
    }

    /// Writes database `name` (or only its `table`) to a .sql file chosen by the user (.sql.gz:
    /// compressed).
    fn export(&mut self, name: &str, table: Option<&str>, t: &Strings) {
        let what = table.unwrap_or(name);
        let ext = if self.gzip { "sql.gz" } else { "sql" };
        let default = format!("{what}-{}.{ext}", chrono::Local::now().format("%Y%m%d-%H%M"));
        let filter = if self.gzip { ("SQL (gzip)", ["gz"]) } else { ("SQL", ["sql"]) };
        let Some(path) = rfd::FileDialog::new().set_file_name(default).add_filter(filter.0, &filter.1).save_file() else { return };
        self.start_transfer(t.db_exporting.replace("{db}", what), false);
        self.send(Request::Export { db: name.to_owned(), tables: table.map(|t| vec![t.to_owned()]), path, check_fk: self.check_fk });
    }

    /// Writes the rows of a table to a .csv file chosen by the user.
    fn export_csv(&mut self, name: &str, table: &str, t: &Strings) {
        let default = format!("{table}-{}.csv", chrono::Local::now().format("%Y%m%d-%H%M"));
        let Some(path) = rfd::FileDialog::new().set_file_name(default).add_filter("CSV", &["csv"]).save_file() else { return };
        self.start_transfer(t.db_exporting.replace("{db}", table), false);
        self.send(Request::ExportCsv { db: name.to_owned(), table: table.to_owned(), path });
    }

    /// Runs a .sql (or .sql.gz) file chosen by the user in database `name` (or where the file says).
    fn import(&mut self, name: Option<&str>, t: &Strings) {
        let Some(path) = rfd::FileDialog::new().add_filter("SQL", &["sql", "gz", "zip"]).pick_file() else { return };
        self.import_file(name.map(str::to_owned), path, self.check_fk, t);
    }

    fn import_file(&mut self, db: Option<String>, path: std::path::PathBuf, check_fk: bool, t: &Strings) {
        self.start_transfer(t.db_importing.replace("{file}", &path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), true);
        self.last_import = Some((db.clone(), path.clone(), check_fk));
        self.send(Request::Import { db, path, check_fk });
    }

    fn start_transfer(&mut self, what: String, import: bool) {
        self.transfer = Some((what, 0.0, String::new()));
        self.transfer_done = None;
        self.transfer_import = import;
    }

    /// A query's rows written to a .csv file chosen by the user.
    fn save_csv(&mut self, result: &QueryResult, t: &Strings) {
        let default = format!("resultat-{}.csv", chrono::Local::now().format("%Y%m%d-%H%M"));
        let Some(path) = rfd::FileDialog::new().set_file_name(default).add_filter("CSV", &["csv"]).save_file() else { return };
        let written = std::fs::File::create(&path).and_then(|f| {
            let mut out = std::io::BufWriter::new(f);
            db::write_csv(&mut out, &result.columns, &result.rows)?;
            std::io::Write::flush(&mut out)
        });
        match written {
            Ok(()) => self.notify(format!("✓  {}", t.db_csv_saved.replace("{n}", &result.rows.len().to_string())), false),
            Err(e) => self.error = Some(format!("{} : {e}", path.display())),
        }
    }

    /// Remembers a query run or a change made (the latest first); its entry's id, for its outcome.
    fn remember(&mut self, sql: &str, db: Option<String>, ui: bool) -> Option<u64> {
        if sql.trim().is_empty() {
            return None;
        }
        let entry = super::sql::Entry::new(sql, db, ui);
        let id = entry.id;
        self.history.insert(0, entry);
        self.history.truncate(super::sql::HISTORY_MAX);
        super::sql::save(self.id, &self.history);
        Some(id)
    }

    fn clear_history(&mut self) {
        self.history.clear();
        super::sql::save(self.id, &self.history);
    }

    /// Sends a query typed, remembered with its outcome to come.
    fn send_query(&mut self, db: Option<String>, sql: String) {
        self.running_entry = self.remember(&sql, db.clone(), false).map(|id| (id, Instant::now()));
        self.sql_running = true;
        self.send(Request::Query { db, sql });
    }

    /// Runs the SQL typed in the SQL tab shown, asking first when it destroys a lot.
    fn run_sql(&mut self, t: &Strings) {
        let tab = &self.sql_tabs[self.sql_tab];
        let sql = tab.sql.trim().to_owned();
        self.query_for = Some(tab.id);
        self.run_query(sql, t);
    }

    fn run_query(&mut self, sql: String, t: &Strings) {
        if sql.is_empty() {
            return;
        }
        if let Some(danger) = super::guard::check(&sql, false) {
            let reason = match danger {
                super::guard::Danger::DropDatabase => t.guard_drop.to_owned(),
                super::guard::Danger::EmptyTable => t.guard_table.to_owned(),
                super::guard::Danger::UpdateAll => t.guard_update.to_owned(),
                super::guard::Danger::DropTable => t.guard_drop_table.to_owned(),
                super::guard::Danger::DropColumn => t.guard_drop_column.to_owned(),
                _ => String::new(),
            };
            self.dialog = Some(Dialog::Confirm { sql, db: self.db.clone(), reason, query: true, expect: None, fk: None });
            return;
        }
        self.send_query(self.db.clone(), sql);
    }

    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) -> DbAction {
        let action = DbAction::None;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut ui;
        ui.painter().rect_filled(rect, 0.0, theme.bg);
        // What the last changes did (known once done).
        if let Some((kind, n)) = self.pending_notice.take() {
            match kind {
                PendingNotice::CellUnchanged => self.notify(format!("⚠  {}", t.db_cell_unchanged), true),
                PendingNotice::Deleted => self.notify(format!("✓  {}", t.db_rows_deleted.replace("{n}", &n.to_string())), false),
                PendingNotice::Inserted => self.notify(format!("✓  {}", t.db_row_inserted.replace("{n}", &n.to_string())), false),
                PendingNotice::NoForeignKeys => self.notify(format!("ⓘ  {}", t.db_orphans_none), false),
                PendingNotice::CodeSaved => self.notify(format!("✓  {}", t.db_code_saved), false),
            }
        }
        if let Some((d, tb)) = self.open_after.take() {
            self.select_table(&d, &tb);
        }

        // Toolbar: the connection and its state.
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 38.0));
        ui.painter().rect_filled(bar, 0.0, theme.chrome_bg);
        ui.painter().hline(bar.x_range(), bar.max.y, Stroke::new(1.0, theme.tab_hover));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(12.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            paint_db_icon(ui.painter(), icon.center(), theme.accent);
            ui.label(egui::RichText::new(&self.name).size(14.0).strong());
            ui.label(egui::RichText::new(format!("{}@{}", self.target.user, self.target.host)).size(12.0).color(theme.text_muted).monospace());
            if let Some(via) = &self.via {
                ui.label(egui::RichText::new(format!("🔒 {}", t.db_through.replace("{h}", via))).size(12.0).color(theme.text_muted));
            }
            ui.add_space(6.0);
            if matches!(self.status, Status::Connecting) {
                ui.add(egui::Spinner::new().size(13.0));
            }
            let (text, color) = match &self.status {
                Status::Connecting => (t.db_connecting.to_owned(), theme.text_muted),
                Status::Ready(v) => (format!("● {}  ·  {v}", t.db_connected), theme.ansi[2]),
                Status::Closed(_) => (format!("● {}", t.db_closed), theme.ansi[1]),
            };
            ui.label(egui::RichText::new(text).size(12.0).color(color));
            if let Status::Closed(reason) = &self.status {
                let tip = if db::refused(reason) && self.via.is_none() { format!("{reason}\n\n{}", t.db_refused_hint) } else { reason.clone() };
                ui.label(egui::RichText::new("ⓘ").color(theme.ansi[1])).on_hover_text(tip);
                if ui.button(format!("↻  {}", t.reconnect)).clicked() {
                    self.reconnect(ui.ctx());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("↻").on_hover_text(t.files_refresh).clicked() {
                    self.refresh();
                }
                let users = egui::Button::selectable(self.page == Page::Users, egui::RichText::new(format!("👤  {}", t.db_users)).size(12.5));
                // Accounts: MariaDB / MySQL only.
                if !self.dialect.mssql() && ui.add_enabled(matches!(self.status, Status::Ready(_)), users).clicked() {
                    self.page = if self.page == Page::Users { Page::Database } else { Page::Users };
                }
            });
        });

        let body = Rect::from_min_max(Pos2::new(rect.min.x, bar.max.y + 1.0), rect.max);
        let left = Rect::from_min_size(body.min, Vec2::new(260.0_f32.min(body.width() * 0.35), body.height()));
        let right = Rect::from_min_max(Pos2::new(left.max.x, body.min.y), body.max);
        ui.painter().rect_filled(left, 0.0, theme.chrome_bg.gamma_multiply(0.7));
        ui.painter().vline(left.max.x, left.y_range(), Stroke::new(1.0, theme.tab_hover));
        self.tree_ui(ui, left.shrink2(Vec2::new(10.0, 10.0)), theme, t);
        self.page_ui(ui, right.shrink2(Vec2::new(20.0, 14.0)), theme, t);
        self.dialog_ui(ui.ctx(), theme, t);
        self.object_dialog_ui(ui.ctx(), theme, t);
        action
    }

    /// The databases and, unfolded, their tables.
    fn tree_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) {
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut ui;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(t.db_filter).desired_width(ui.available_width() - 36.0).margin(Vec2::new(8.0, 5.0)));
            let plus = egui::Button::new(egui::RichText::new("+").size(15.0)).corner_radius(6.0).min_size(Vec2::splat(28.0));
            if ui.add_enabled(matches!(self.status, Status::Ready(_)), plus).on_hover_text(t.db_new_database).clicked() {
                self.dialog = Some(Dialog::NewDatabase { name: String::new(), fresh: true });
            }
        });
        ui.add_space(8.0);
        let filter = self.filter.to_lowercase();
        let mut dbs: Vec<DatabaseInfo> = self.databases.clone();
        // The user's databases first, the system ones after.
        let dialect = self.dialect;
        dbs.sort_by_key(|d| (system_db(dialect, &d.name), d.name.to_lowercase()));
        let mut select_db = None;
        let mut select_table = None;
        let mut toggle = None;
        let mut confirm = None;
        let (mut export, mut import) = (None, None);
        let (mut table_export, mut new_table, mut rename) = (None, None, None);
        let mut orphans: Option<(String, String)> = None;
        // A page of a database (or of a table) to show, a new procedure, function or trigger to write.
        let mut open_page: Option<(String, Option<String>, Page)> = None;
        let mut new_object: Option<(String, Option<String>, ObjectKind)> = None;
        egui::ScrollArea::vertical().id_salt("db-tree").auto_shrink([false, false]).show(ui, |ui| {
            for d in &dbs {
                let tables = self.tables.get(&d.name);
                let name_match = filter.is_empty() || d.name.to_lowercase().contains(&filter);
                let table_match = tables.is_some_and(|ts| ts.iter().any(|t| t.name.to_lowercase().contains(&filter)));
                if !name_match && !table_match {
                    continue;
                }
                let open = self.expanded.contains(&d.name) || (!filter.is_empty() && table_match);
                let selected = self.db.as_deref() == Some(d.name.as_str()) && self.table.is_none();
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
                if selected {
                    ui.painter().rect_filled(r, 6.0, theme.tab_active);
                    ui.painter().rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 6.0), Vec2::new(3.0, 16.0)), 1.5, theme.accent);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 6.0, theme.tab_hover.gamma_multiply(0.7));
                }
                let dim = system_db(dialect, &d.name);
                let color = if dim { theme.text_muted.gamma_multiply(0.8) } else { theme.text };
                let chevron = Rect::from_center_size(Pos2::new(r.min.x + 10.0, r.center().y), Vec2::splat(16.0));
                paint_chevron_small(ui.painter(), chevron.center(), open, theme.text_muted);
                paint_db_icon(ui.painter(), Pos2::new(r.min.x + 28.0, r.center().y), if dim { theme.text_muted } else { theme.accent });
                let mut job = egui::text::LayoutJob::simple_singleline(d.name.clone(), FontId::proportional(13.0), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 86.0);
                let g = ui.painter().layout_job(job);
                ui.painter().galley(Pos2::new(r.min.x + 40.0, r.center().y - g.size().y / 2.0), g, color);
                let count = ui.painter().layout_no_wrap(d.tables.to_string(), FontId::proportional(10.5), theme.text_muted);
                let pill = Rect::from_center_size(Pos2::new(r.max.x - 6.0 - (count.size().x + 12.0) / 2.0, r.center().y), Vec2::new(count.size().x + 12.0, 16.0));
                ui.painter().rect_filled(pill, 8.0, theme.tab_hover);
                ui.painter().galley(pill.center() - count.size() / 2.0, count, theme.text_muted);
                let resp = resp.on_hover_text(format!("{}  ·  {}  ·  {}", d.name, format_size(d.size, t), d.collation));
                let chevron_hit = resp.interact_pointer_pos().is_some_and(|p| p.x < r.min.x + 20.0);
                if resp.clicked() {
                    if chevron_hit { toggle = Some(d.name.clone()) } else { select_db = Some(d.name.clone()) }
                }
                resp.context_menu(|ui| {
                    if ui.button(t.db_open_database).clicked() {
                        select_db = Some(d.name.clone());
                        ui.close();
                    }
                    if ui.button(format!("+  {}", t.db_new_table)).clicked() {
                        new_table = Some(d.name.clone());
                        ui.close();
                    }
                    for kind in [ObjectKind::Procedure, ObjectKind::Function, ObjectKind::Trigger] {
                        let label = match kind {
                            ObjectKind::Procedure => t.db_new_procedure,
                            ObjectKind::Function => t.db_new_function,
                            ObjectKind::Trigger => t.db_new_trigger,
                        };
                        if ui.button(format!("+  {label}")).clicked() {
                            new_object = Some((d.name.clone(), None, kind));
                            ui.close();
                        }
                    }
                    ui.separator();
                    // SQL Server: scripts run, no dump written.
                    if !dialect.mssql() && ui.button(format!("⤓  {}", t.db_export)).clicked() {
                        export = Some(d.name.clone());
                        ui.close();
                    }
                    if ui.button(format!("⤒  {}", if dialect.mssql() { t.db_run_script } else { t.db_import })).clicked() {
                        import = Some(d.name.clone());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(egui::RichText::new(t.db_drop_database).color(theme.ansi[1])).clicked() {
                        confirm = Some(Dialog::Confirm { sql: format!("DROP DATABASE {}", dialect.ident(&d.name)), db: None, reason: t.guard_drop.to_owned(), query: false, expect: None, fk: None });
                        ui.close();
                    }
                });
                if !open {
                    continue;
                }
                match tables {
                    None => {
                        ui.horizontal(|ui| {
                            ui.add_space(40.0);
                            ui.spinner();
                        });
                    }
                    Some(ts) => {
                        for tb in ts.iter().filter(|tb| filter.is_empty() || name_match || tb.name.to_lowercase().contains(&filter)) {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 25.0), Sense::click());
                            let selected = self.db.as_deref() == Some(d.name.as_str()) && self.table.as_deref() == Some(tb.name.as_str());
                            if selected {
                                ui.painter().rect_filled(r, 6.0, theme.tab_active);
                                ui.painter().rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 5.0), Vec2::new(3.0, 15.0)), 1.5, theme.accent);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, 6.0, theme.tab_hover.gamma_multiply(0.7));
                            }
                            paint_table_icon(ui.painter(), Pos2::new(r.min.x + 34.0, r.center().y), tb.view, theme.text_muted);
                            let color = if selected { theme.text } else { theme.text.gamma_multiply(0.85) };
                            let mut job = egui::text::LayoutJob::simple_singleline(tb.name.clone(), FontId::proportional(12.5), color);
                            job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 100.0);
                            let g = ui.painter().layout_job(job);
                            ui.painter().galley(Pos2::new(r.min.x + 46.0, r.center().y - g.size().y / 2.0), g, color);
                            ui.painter().text(Pos2::new(r.max.x - 6.0, r.center().y), Align2::RIGHT_CENTER, short_count(tb.rows), FontId::proportional(10.5), theme.text_muted.gamma_multiply(0.8));
                            if resp.clicked() {
                                select_table = Some((d.name.clone(), tb.name.clone()));
                            }
                            resp.context_menu(|ui| {
                                if ui.button(t.db_page_content).clicked() {
                                    select_table = Some((d.name.clone(), tb.name.clone()));
                                    ui.close();
                                }
                                if !tb.view && ui.button(t.db_rename_table).clicked() {
                                    rename = Some((d.name.clone(), tb.name.clone()));
                                    ui.close();
                                }
                                ui.separator();
                                if !dialect.mssql() && ui.button(format!("⤓  {}", t.db_export_table)).clicked() {
                                    table_export = Some((d.name.clone(), tb.name.clone(), false));
                                    ui.close();
                                }
                                if ui.button(format!("⤓  {}", t.db_export_csv)).clicked() {
                                    table_export = Some((d.name.clone(), tb.name.clone(), true));
                                    ui.close();
                                }
                                if !tb.view && !dialect.mssql() && ui.button(format!("🔗  {}", t.db_orphans)).clicked() {
                                    orphans = Some((d.name.clone(), tb.name.clone()));
                                    ui.close();
                                }
                                if !tb.view {
                                    ui.separator();
                                    if ui.button(t.db_page_triggers).clicked() {
                                        open_page = Some((d.name.clone(), Some(tb.name.clone()), Page::Triggers));
                                        ui.close();
                                    }
                                    if ui.button(format!("+  {}", t.db_new_trigger)).clicked() {
                                        new_object = Some((d.name.clone(), Some(tb.name.clone()), ObjectKind::Trigger));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                let q = dialect.table(&d.name, &tb.name);
                                if !tb.view && ui.button(egui::RichText::new(t.db_truncate).color(theme.ansi[1])).clicked() {
                                    confirm = Some(Dialog::Confirm { sql: format!("TRUNCATE TABLE {q}"), db: None, reason: t.guard_table.to_owned(), query: false, expect: None, fk: (!dialect.mssql()).then_some(self.check_fk) });
                                    ui.close();
                                }
                                if ui.button(egui::RichText::new(t.db_drop_table).color(theme.ansi[1])).clicked() {
                                    let what = if tb.view { "VIEW" } else { "TABLE" };
                                    confirm = Some(Dialog::Confirm { sql: format!("DROP {what} {q}"), db: None, reason: t.db_drop_table_reason.to_owned(), query: false, expect: None, fk: (!dialect.mssql()).then_some(self.check_fk) });
                                    ui.close();
                                }
                            });
                        }
                        // Procedures and functions, triggers: a line each when there are some.
                        let counts = [
                            (Page::Routines, ObjectKind::Procedure, t.db_page_routines, self.routines.get(&d.name).map_or(0, Vec::len)),
                            (Page::Triggers, ObjectKind::Trigger, t.db_page_triggers, self.triggers.get(&d.name).map_or(0, Vec::len)),
                        ];
                        for (page, kind, label, n) in counts.into_iter().filter(|c| c.3 > 0 && filter.is_empty()) {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 25.0), Sense::click());
                            let selected = self.db.as_deref() == Some(d.name.as_str()) && self.table.is_none() && self.page == page;
                            if selected {
                                ui.painter().rect_filled(r, 6.0, theme.tab_active);
                                ui.painter().rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 5.0), Vec2::new(3.0, 15.0)), 1.5, theme.accent);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, 6.0, theme.tab_hover.gamma_multiply(0.7));
                            }
                            objects::paint_mini_badge(ui.painter(), Pos2::new(r.min.x + 34.0, r.center().y), kind, theme);
                            let color = if selected { theme.text } else { theme.text.gamma_multiply(0.85) };
                            ui.painter().text(Pos2::new(r.min.x + 46.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), color);
                            ui.painter().text(Pos2::new(r.max.x - 6.0, r.center().y), Align2::RIGHT_CENTER, n.to_string(), FontId::proportional(10.5), theme.text_muted.gamma_multiply(0.8));
                            if resp.clicked() {
                                open_page = Some((d.name.clone(), None, page));
                            }
                        }
                    }
                }
            }
            if matches!(self.status, Status::Connecting) {
                super::loading::inline(ui, theme, t.files_connecting_server);
            }
            if self.databases.is_empty() && matches!(self.status, Status::Ready(_)) {
                ui.label(egui::RichText::new(t.db_no_databases).size(12.5).color(theme.text_muted));
            }
        });
        if let Some(name) = toggle {
            if !self.expanded.remove(&name) {
                self.expanded.insert(name.clone());
                if !self.tables.contains_key(&name) {
                    self.ask_lists(name);
                }
            }
        }
        if let Some(name) = select_db {
            self.select_db(&name);
        }
        if let Some((d, tb)) = select_table {
            self.select_table(&d, &tb);
        }
        if let Some(c) = confirm {
            self.dialog = Some(c);
        }
        if let Some(name) = export {
            self.transfer_dialog(Some(name), None, false, t);
        }
        if let Some(name) = import {
            self.transfer_dialog(Some(name), None, true, t);
        }
        if let Some((d, tb)) = orphans {
            self.select_table(&d, &tb);
            self.send(Request::ForeignKeys { db: d, table: tb });
        }
        match table_export {
            Some((d, tb, true)) => self.export_csv(&d, &tb, t),
            Some((d, tb, false)) => self.transfer_dialog(Some(d), Some(tb), false, t),
            None => {}
        }
        if let Some(d) = new_table {
            self.new_table_dialog(&d);
        }
        if let Some((d, tb)) = rename {
            self.dialog = Some(Dialog::RenameTable { db: d, old: tb.clone(), name: tb, fresh: true });
        }
        if let Some((d, tb, page)) = open_page {
            match tb {
                Some(tb) => self.select_table(&d, &tb),
                None => self.select_db(&d),
            }
            self.page = page;
        }
        if let Some((d, tb, kind)) = new_object {
            match &tb {
                Some(tb) => self.select_table(&d, tb),
                None => self.select_db(&d),
            }
            self.page = if kind == ObjectKind::Trigger { Page::Triggers } else { Page::Routines };
            self.new_object(&d, tb, kind);
        }
    }

    /// The right side: where we are, the page's tabs, the page.
    fn page_ui(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, t: &Strings) {
        // The server hasn't answered yet: a spinner, nothing to use.
        if matches!(self.status, Status::Connecting) {
            let target = format!("{}@{}", self.target.user, self.target.host);
            let via = self.via.as_ref().map(|h| t.db_through.replace("{h}", h));
            super::loading::screen(ui, rect, theme, &t.db_connecting_to.replace("{name}", &self.name), Some(&target), via.as_deref());
            return;
        }
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut ui;
        // Breadcrumb and page tabs.
        ui.horizontal(|ui| {
            let crumb = match (&self.db, &self.table) {
                _ if self.page == Page::Users => format!("{}  ›  {}", self.name, t.db_users),
                (Some(d), Some(tb)) => format!("{d}  ›  {tb}"),
                (Some(d), None) => d.clone(),
                _ => self.name.clone(),
            };
            ui.label(egui::RichText::new(crumb).size(18.0).strong());
        });
        ui.add_space(8.0);
        // Triggers and routines, with how many there are.
        let counted = |label: &str, n: Option<usize>| match n {
            Some(n) if n > 0 => format!("{label}  {n}"),
            _ => label.to_owned(),
        };
        let triggers = |table: Option<&String>| self.db.as_ref().and_then(|d| self.triggers.get(d)).map(|ts| ts.iter().filter(|x| table.is_none_or(|tb| x.table == *tb)).count());
        let routines = self.db.as_ref().and_then(|d| self.routines.get(d)).map(Vec::len);
        let pages: Vec<(Page, String)> = match (&self.db, &self.table) {
            (Some(_), Some(tb)) => vec![
                (Page::Content, t.db_page_content.to_owned()),
                (Page::Structure, t.db_page_structure.to_owned()),
                (Page::Triggers, counted(t.db_page_triggers, triggers(Some(tb)))),
                (Page::Sql, t.db_page_sql.to_owned()),
            ],
            (Some(_), None) => vec![
                (Page::Database, t.db_page_tables.to_owned()),
                (Page::Routines, counted(t.db_page_routines, routines)),
                (Page::Triggers, counted(t.db_page_triggers, triggers(None))),
                (Page::Sql, t.db_page_sql.to_owned()),
            ],
            _ => vec![(Page::Sql, t.db_page_sql.to_owned())],
        };
        if !pages.iter().any(|(p, _)| *p == self.page) && self.page != Page::Users {
            self.page = pages[0].0;
        }
        ui.horizontal(|ui| {
            if self.page == Page::Users {
                return;
            }
            for (page, label) in &pages {
                let selected = self.page == *page;
                let text = egui::RichText::new(label).size(13.5).color(if selected { theme.text } else { theme.text_muted });
                let resp = ui.add(egui::Button::new(text).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::new(0.0, 28.0)));
                if selected {
                    let r = resp.rect;
                    ui.painter().hline(r.min.x + 6.0..=r.max.x - 6.0, r.max.y + 2.0, Stroke::new(2.0, theme.accent));
                }
                if resp.clicked() {
                    self.page = *page;
                }
            }
        });
        ui.add_space(6.0);
        let sep = ui.cursor().min.y;
        ui.painter().hline(rect.x_range(), sep, Stroke::new(1.0, theme.tab_hover));
        ui.add_space(12.0);
        if let Some(e) = self.error.clone() {
            // A procedure, function or trigger refused: whether the old one is still there.
            let e = if let Some(rest) = e.strip_prefix(db::RESTORED) {
                format!("{rest}\n{}", t.db_code_restored)
            } else if let Some(rest) = e.strip_prefix(db::LOST) {
                format!("{rest}\n{}", t.db_code_lost)
            } else {
                e
            };
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(format!("⚠  {e}")).size(12.5).color(theme.ansi[1])).wrap());
                if ui.small_button("✕").clicked() {
                    self.error = None;
                }
            });
            ui.add_space(6.0);
        }
        // How the last change went: for a few seconds (warnings until closed).
        if let Some(n) = &self.notice {
            let age = n.at.elapsed();
            if !n.warning && age > Duration::from_secs(5) {
                self.notice = None;
            } else {
                let (text, color) = (n.text.clone(), if n.warning { theme.ansi[3] } else { theme.ansi[2] });
                if !n.warning {
                    ui.ctx().request_repaint_after(Duration::from_secs(5).saturating_sub(age));
                }
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(color)).wrap());
                    if ui.small_button("✕").clicked() {
                        self.notice = None;
                    }
                });
                ui.add_space(6.0);
            }
        }
        // Export / import: progress, then how it ended.
        if let Some((what, fraction, on)) = self.transfer.clone() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(format!("{what}  ·  {on}")).size(12.5));
                if ui.small_button(format!("■  {}", t.db_stop)).clicked() {
                    if let Some(conn) = &self.conn {
                        conn.cancel_transfer();
                    }
                }
            });
            ui.add(egui::ProgressBar::new(fraction).desired_height(6.0).fill(theme.accent));
            ui.add_space(8.0);
        } else if let Some(done) = self.transfer_done.clone() {
            let (text, color) = match &done {
                Ok(summary) => (format!("✓  {}  ·  {summary}", t.db_transfer_done), theme.ansi[2]),
                Err(e) if e == db::CANCELLED => (format!("■  {}", t.db_cancelled), theme.ansi[3]),
                Err(e) if self.transfer_import => (format!("✗  {e}\n{}", t.db_import_partial), theme.ansi[1]),
                Err(e) => (format!("✗  {e}"), theme.ansi[1]),
            };
            // Stopped by a foreign key: once more with the checks off.
            let fk_failed = self.transfer_import && matches!(&done, Err(e) if e.contains("1452") || e.contains("1451") || e.to_lowercase().contains("foreign key constraint fails"));
            let retry = self.last_import.clone().filter(|(_, _, checked)| fk_failed && *checked);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(color)).wrap());
                if ui.small_button("✕").clicked() {
                    self.transfer_done = None;
                }
            });
            if let Some((db, path, _)) = retry
                && ui.button(format!("↻  {}", t.db_retry_without_fk)).clicked()
            {
                self.check_fk = false;
                self.import_file(db, path, false, t);
            }
            ui.add_space(6.0);
        }
        match self.page {
            Page::Database => self.database_page(ui, theme, t),
            Page::Content => self.content_page(ui, theme, t),
            Page::Structure => self.structure_page(ui, theme, t),
            Page::Sql => self.sql_page(ui, theme, t),
            Page::Users => self.users_page(ui, theme, t),
            Page::Routines => self.objects_page(ui, theme, t, Group::Routines),
            Page::Triggers => self.objects_page(ui, theme, t, Group::Triggers),
        }
    }

    /// The server's accounts; the one picked, with what it may do.
    fn users_page(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        if !self.users_asked {
            self.users_asked = true;
            self.send(Request::Users);
        }
        let mut new_user = false;
        ui.horizontal(|ui| {
            let n = self.users.as_ref().map_or(0, Vec::len);
            ui.label(egui::RichText::new(t.db_users_count.replace("{n}", &n.to_string())).size(12.5).color(theme.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add = egui::Button::new(egui::RichText::new(format!("+  {}", t.db_new_user)).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0);
                if ui.add(add).clicked() {
                    new_user = true;
                }
                if ui.button("↻").on_hover_text(t.files_refresh).clicked() {
                    self.send(Request::Users);
                }
            });
        });
        ui.add_space(8.0);
        if new_user {
            let database = self.db.clone().unwrap_or_default();
            self.dialog = Some(Dialog::NewUser { user: String::new(), host: "%".into(), password: String::new(), reveal: false, access: Access::Write, database });
        }
        let Some(users) = self.users.clone() else {
            ui.spinner();
            return;
        };
        let columns = vec![t.db_user.to_owned(), t.db_host.to_owned()];
        let rows: Vec<Vec<Cell>> = users.iter().map(|u| vec![Cell::Text(u.user.clone()), Cell::Text(u.host.clone())]).collect();
        let height = if self.user.is_some() { (ui.available_height() * 0.45).max(120.0) } else { ui.available_height() };
        let out = grid(ui, egui::Id::new("db-users"), theme, &columns, &rows, None, height);
        if let Some((row, _)) = out.clicked {
            let u = &users[row];
            self.user = Some((u.user.clone(), u.host.clone()));
            self.grants = None;
            self.send(Request::Grants { user: u.user.clone(), host: u.host.clone() });
        }
        let Some((user, host)) = self.user.clone() else { return };
        ui.add_space(14.0);
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{user}@{host}")).size(15.0).strong().monospace());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new(format!("🗑  {}", t.db_drop_user)).color(theme.ansi[1])).clicked() {
                    action = Some(0);
                }
                if ui.button(t.db_revoke_all).clicked() {
                    action = Some(1);
                }
                if ui.button(format!("+  {}", t.db_add_grant)).clicked() {
                    action = Some(2);
                }
                if ui.button(format!("🔑  {}", t.db_change_password)).clicked() {
                    action = Some(3);
                }
            });
        });
        let acct = db::account(&user, &host);
        match action {
            Some(0) => self.dialog = Some(Dialog::Confirm { sql: format!("DROP USER {acct}"), db: None, reason: t.db_drop_user_reason.to_owned(), query: false, expect: Some(Expect::Users), fk: None }),
            Some(1) => self.dialog = Some(Dialog::Confirm { sql: format!("REVOKE ALL PRIVILEGES, GRANT OPTION FROM {acct}"), db: None, reason: t.db_revoke_reason.to_owned(), query: false, expect: Some(Expect::Users), fk: None }),
            Some(2) => self.dialog = Some(Dialog::Grant { user: user.clone(), host: host.clone(), access: Access::Write, database: self.db.clone().unwrap_or_default() }),
            Some(3) => self.dialog = Some(Dialog::Password { user: user.clone(), host: host.clone(), password: String::new(), reveal: false }),
            _ => {}
        }
        ui.add_space(8.0);
        match self.grants.as_ref().filter(|(u, h, _)| *u == user && *h == host) {
            Some((_, _, grants)) => {
                Frame::NONE.fill(theme.chrome_bg).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().id_salt("db-grants").max_height(ui.available_height()).show(ui, |ui| {
                        for g in grants {
                            // The password's hash stays out of sight.
                            let shown = match g.find(" IDENTIFIED BY PASSWORD ") {
                                Some(i) => format!("{} IDENTIFIED BY PASSWORD '…'", &g[..i]),
                                None => g.clone(),
                            };
                            ui.add(egui::Label::new(egui::RichText::new(shown).monospace().size(12.5)).wrap().selectable(true));
                        }
                    });
                });
            }
            None => {
                ui.spinner();
            }
        }
    }

    fn database_page(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        let Some(d) = self.db.clone() else { return };
        let Some(tables) = self.tables.get(&d).cloned() else {
            ui.spinner();
            return;
        };
        let size: u64 = tables.iter().map(|x| x.size).sum();
        let (mut export, mut import, mut new_table) = (false, false, false);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}  ·  {}", t.db_tables_count.replace("{n}", &tables.len().to_string()), format_size(size, t))).size(12.5).color(theme.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = self.transfer.is_some();
                let mssql = self.dialect.mssql();
                let import_label = if mssql { t.db_run_script } else { t.db_import };
                if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new(format!("⤒  {import_label}")).size(12.5)).corner_radius(6.0)).clicked() {
                    import = true;
                }
                if !mssql {
                    if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new(format!("⤓  {}", t.db_export)).size(12.5)).corner_radius(6.0)).clicked() {
                        export = true;
                    }
                }
                ui.add_space(10.0);
                let add = egui::Button::new(egui::RichText::new(format!("+  {}", t.db_new_table)).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0);
                if ui.add(add).clicked() {
                    new_table = true;
                }
            });
        });
        if export {
            self.transfer_dialog(Some(d.clone()), None, false, t);
        }
        if import {
            self.transfer_dialog(Some(d.clone()), None, true, t);
        }
        if new_table {
            self.new_table_dialog(&d);
        }
        ui.add_space(8.0);
        // Tables ticked: drop or empty them all at once. Ticks don't follow to another database, and
        // forget tables that are gone.
        if self.ticked.0 != d {
            self.ticked = (d.clone(), HashSet::new());
        }
        self.ticked.1.retain(|name| tables.iter().any(|x| &x.name == name));
        if !self.ticked.1.is_empty() {
            let ticked: Vec<&TableInfo> = tables.iter().filter(|x| self.ticked.1.contains(&x.name)).collect();
            let emptied = ticked.iter().filter(|x| !x.view).count();
            let (mut drop, mut empty) = (false, false);
            Frame::NONE.fill(theme.accent.gamma_multiply(0.1)).stroke(Stroke::new(1.0, theme.accent.gamma_multiply(0.4))).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t.db_tables_selected.replace("{n}", &ticked.len().to_string())).size(12.5));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let del = egui::Button::new(egui::RichText::new(format!("🗑  {}", t.db_drop_tables.replace("{n}", &ticked.len().to_string()))).size(12.5).color(theme.bg)).fill(theme.ansi[1]).corner_radius(6.0);
                        if ui.add(del).clicked() {
                            drop = true;
                        }
                        // Views hold no rows: only the tables are emptied.
                        let label = egui::RichText::new(t.db_truncate_tables.replace("{n}", &emptied.to_string())).size(12.5).color(theme.ansi[1]);
                        if ui.add_enabled(emptied > 0, egui::Button::new(label).corner_radius(6.0)).clicked() {
                            empty = true;
                        }
                        if ui.small_button(t.cancel).clicked() {
                            self.ticked.1.clear();
                        }
                    });
                });
            });
            if drop || empty {
                let dialect = self.dialect;
                let names = |view: bool| ticked.iter().filter(|x| x.view == view).map(|x| dialect.table(&d, &x.name)).collect::<Vec<_>>();
                let mut statements = Vec::new();
                if drop {
                    for (view, what) in [(false, "TABLE"), (true, "VIEW")] {
                        let list = names(view);
                        if !list.is_empty() {
                            // Some may already be gone (a first try stopped by a foreign key).
                            statements.push(format!("DROP {what} IF EXISTS {}", list.join(", ")));
                        }
                    }
                } else {
                    statements.extend(names(false).into_iter().map(|q| format!("TRUNCATE TABLE {q}")));
                }
                let n = if drop { ticked.len() } else { emptied };
                let reason = if drop { t.db_drop_tables_reason } else { t.db_truncate_tables_reason }.replace("{n}", &n.to_string());
                self.dialog = Some(Dialog::Confirm { sql: statements.join(";\n"), db: None, reason, query: false, expect: None, fk: (!dialect.mssql()).then_some(self.check_fk) });
            }
            ui.add_space(6.0);
        }
        let columns: Vec<String> = [t.db_col_table, t.db_col_type, t.db_col_engine, t.db_col_rows, t.db_col_size, t.db_col_collation, t.db_col_comment].iter().map(|s| s.to_string()).collect();
        let rows: Vec<Vec<Cell>> = tables
            .iter()
            .map(|x| {
                vec![
                    Cell::Text(x.name.clone()),
                    Cell::Text(if x.view { "VIEW".into() } else { "TABLE".into() }),
                    Cell::Text(x.engine.clone()),
                    Cell::Text(format!("~{}", x.rows)),
                    Cell::Text(format_size(x.size, t)),
                    Cell::Text(x.collation.clone()),
                    Cell::Text(x.comment.clone()),
                ]
            })
            .collect();
        let mut ticked: HashSet<usize> = tables.iter().enumerate().filter(|(_, x)| self.ticked.1.contains(&x.name)).map(|(i, _)| i).collect();
        let extras = GridExtras { selection: Some(&mut ticked), ..Default::default() };
        let out = grid_with(ui, egui::Id::new(("db-tables", &d)), theme, &columns, &rows, None, ui.available_height(), extras);
        self.ticked.1 = ticked.into_iter().map(|i| tables[i].name.clone()).collect();
        if let Some((row, _)) = out.clicked {
            let name = tables[row].name.clone();
            self.select_table(&d, &name);
        }
    }

    fn content_page(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        let (Some(d), Some(tb)) = (self.db.clone(), self.table.clone()) else { return };
        let Some(rows) = self.rows.as_ref().filter(|r| r.db == d && r.table == tb) else {
            ui.spinner();
            return;
        };
        let (offset, total, exact, order, shown) = (rows.offset, rows.total, rows.exact, rows.order.clone(), rows.result.rows.len() as u64);
        let mut go = None;
        let mut sort = None;
        // A query on this table, typed right above its content.
        let mut run = false;
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, if ui.memory(|m| m.has_focus(egui::Id::new(("table-sql", &d, &tb)))) { theme.accent.gamma_multiply(0.7) } else { theme.tab_hover })).corner_radius(8.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.add_space(0.0);
                ui.label(egui::RichText::new("SQL").size(11.5).strong().color(theme.accent));
                let hint = format!("SELECT * FROM {} WHERE …", self.dialect.local_table(&tb));
                let id = egui::Id::new(("table-sql", &d, &tb));
                // Several lines: ⌘ Enter runs (Enter goes to the next line).
                if ui.memory(|m| m.has_focus(id)) && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter))) {
                    run = true;
                }
                self.table_sql_complete.keys(ui, id, &mut self.table_sql);
                let width = ui.available_width() - 72.0;
                // The same suggestions as on the SQL page (tables, this table's columns first, keywords).
                egui::ScrollArea::vertical().id_salt(("table-sql-editor", &d, &tb)).max_height(160.0).max_width(width).show(ui, |ui| {
                    let out = {
                        let mut layouter = super::sql::layouter(theme, FontId::monospace(13.0), &mut self.table_sql_colors);
                        egui::TextEdit::multiline(&mut self.table_sql).id(id).code_editor().hint_text(hint).frame(Frame::NONE).font(FontId::monospace(13.0)).layouter(&mut layouter).desired_width(width).desired_rows(4).show(ui)
                    };
                    let schema = super::sqlcomplete::Schema {
                        brackets: self.dialect.mssql(),
                        db: self.db.as_deref(),
                        databases: self.databases.iter().map(|d| d.name.as_str()).collect(),
                        tables: &self.tables,
                        columns: self.db.as_ref().and_then(|d| self.columns.get(d)),
                        table: self.table.as_deref(),
                    };
                    self.table_sql_complete.show(ui, id, &out, &mut self.table_sql, &schema, theme, t);
                });
                if ui.memory(|m| m.has_focus(id)) {
                    self.want_columns();
                }
                let play = egui::Button::new(egui::RichText::new("▶").size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(28.0, 24.0));
                let run_hint = format!("{}  ({})", t.db_run, if cfg!(target_os = "macos") { "⌘ ↩" } else { "Ctrl+↩" });
                if ui.add_enabled(!self.sql_running && !self.table_sql.trim().is_empty(), play).on_hover_text(run_hint).clicked() {
                    run = true;
                }
                match super::sql::history_menu(ui, &self.history, &mut self.history_filter, theme, t) {
                    Some(super::sql::Pick::Load(q)) => self.table_sql = q,
                    Some(super::sql::Pick::Clear) => self.clear_history(),
                    None => {}
                }
            });
        });
        ui.add_space(8.0);
        if run && !self.sql_running && !self.table_sql.trim().is_empty() {
            self.table_query = true;
            self.query_for = None;
            let sql = self.table_sql.trim().to_owned();
            self.run_query(sql, t);
        }
        // Its result, instead of the rows, until "back to the rows".
        if self.table_query {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.db_query_result).size(12.5).strong().color(theme.text_muted));
                if self.sql_running {
                    ui.spinner();
                    if ui.small_button(format!("■  {}", t.db_stop)).clicked() {
                        self.cancel_query();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(format!("✕  {}", t.db_back_to_rows)).clicked() {
                        self.table_query = false;
                    }
                });
            });
            ui.add_space(6.0);
            if self.table_query {
                let height = ui.available_height();
                self.results_ui(ui, theme, t, height, None);
                return;
            }
        }
        // Search, then paging.
        let mut search_now = None;
        let mut insert = false;
        let is_view = self.tables.get(&d).and_then(|ts| ts.iter().find(|x| x.name == tb)).is_some_and(|x| x.view);
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut self.search_typed).hint_text(format!("🔍  {}", t.db_search)).desired_width(240.0).margin(Vec2::new(8.0, 5.0)));
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                search_now = Some(self.search_typed.trim().to_owned());
            }
            // Emptied: the whole table again.
            if edit.changed() && self.search_typed.is_empty() && !self.search.is_empty() {
                search_now = Some(String::new());
            }
            if !self.search.is_empty() && ui.small_button("✕").clicked() {
                self.search_typed.clear();
                search_now = Some(String::new());
            }
            ui.add_space(8.0);
            let from = if shown == 0 { 0 } else { offset + 1 };
            let total_text = if exact { total.to_string() } else { format!("~{total}") };
            ui.label(egui::RichText::new(t.db_range.replace("{from}", &from.to_string()).replace("{to}", &(offset + shown).to_string()).replace("{total}", &total_text)).size(12.5).color(theme.text_muted));
            if self.rows_loading {
                ui.spinner();
                if ui.small_button(format!("■  {}", t.db_stop)).on_hover_text(t.db_stop_hint).clicked() {
                    self.cancel_query();
                }
            }
            if !is_view && ui.button(format!("+  {}", t.db_insert_row)).clicked() {
                insert = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let last = total.saturating_sub(1) / self.limit * self.limit;
                if ui.add_enabled(offset + shown < total, egui::Button::new("»")).clicked() {
                    go = Some(last);
                }
                if ui.add_enabled(offset + shown < total, egui::Button::new("›")).clicked() {
                    go = Some(offset + self.limit);
                }
                if ui.add_enabled(offset > 0, egui::Button::new("‹")).clicked() {
                    go = Some(offset.saturating_sub(self.limit));
                }
                if ui.add_enabled(offset > 0, egui::Button::new("«")).clicked() {
                    go = Some(0);
                }
                ui.add_space(8.0);
                for n in [500u64, 100, 50] {
                    if ui.add(egui::Button::selectable(self.limit == n, egui::RichText::new(n.to_string()).size(12.0))).on_hover_text(t.db_per_page).clicked() && self.limit != n {
                        self.limit = n;
                        go = Some(0);
                    }
                }
            });
        });
        ui.add_space(6.0);
        // Rows ticked: delete them.
        let mut delete_rows = false;
        if !self.selected.is_empty() {
            Frame::NONE.fill(theme.accent.gamma_multiply(0.1)).stroke(Stroke::new(1.0, theme.accent.gamma_multiply(0.4))).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t.db_selected.replace("{n}", &self.selected.len().to_string())).size(12.5));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let del = egui::Button::new(egui::RichText::new(format!("🗑  {}", t.db_delete_rows)).size(12.5).color(theme.bg)).fill(theme.ansi[1]).corner_radius(6.0);
                        if ui.add(del).clicked() {
                            delete_rows = true;
                        }
                        if ui.small_button(t.cancel).clicked() {
                            self.selected.clear();
                        }
                    });
                });
            });
            ui.add_space(6.0);
        } else {
            ui.label(egui::RichText::new(t.db_edit_hint).size(11.5).color(theme.text_muted.gamma_multiply(0.8)));
            ui.add_space(4.0);
        }
        let detail_h = if self.detail.is_some() { 120.0 } else { 0.0 };
        let height = ui.available_height() - detail_h;
        let mut change: Option<(usize, usize, Option<String>)> = None;
        let result = &self.rows.as_ref().unwrap().result;
        if result.rows.is_empty() {
            ui.label(egui::RichText::new(t.db_no_rows).size(13.0).color(theme.text_muted));
        } else {
            let sort_ref = order.as_ref().map(|(c, a)| (c.as_str(), *a));
            let extras = GridExtras { edit: self.editing.as_mut(), selection: Some(&mut self.selected), menus: Some(t) };
            let out = grid_with(ui, egui::Id::new(("db-rows", &d, &tb)), theme, &result.columns, &result.rows, sort_ref, height, extras);
            if let Some(col) = out.sort {
                let asc = !matches!(&order, Some((c, true)) if *c == col);
                sort = Some((col, asc));
            }
            if let Some((r, c)) = out.clicked {
                self.detail = Some((result.columns[c].clone(), result.rows[r][c].text()));
            }
            // Double click: the cell becomes a field (binary data stays as it is).
            // Several lines or a long text: in a window.
            let long = |r: usize, c: usize| matches!(&result.rows[r][c], Cell::Text(s) if s.contains('\n') || s.chars().count() > 200);
            let start_edit = |r: usize, c: usize| match &result.rows[r][c] {
                Cell::Bytes(_) => None,
                Cell::Null => Some(CellEdit { row: r, col: c, text: String::new(), fresh: true }),
                Cell::Text(s) => Some(CellEdit { row: r, col: c, text: s.clone(), fresh: true }),
            };
            let window = |r: usize, c: usize| match &result.rows[r][c] {
                Cell::Text(s) => Some(Dialog::Cell { row: r, col: c, column: result.columns[c].clone(), text: s.clone() }),
                Cell::Null => Some(Dialog::Cell { row: r, col: c, column: result.columns[c].clone(), text: String::new() }),
                Cell::Bytes(_) => None,
            };
            if let Some((r, c)) = out.double {
                if long(r, c) {
                    self.dialog = window(r, c);
                } else {
                    self.editing = start_edit(r, c);
                }
            }
            match out.menu {
                Some((r, c, CellMenu::Copy)) => ui.ctx().copy_text(result.rows[r][c].text()),
                Some((r, c, CellMenu::Edit)) if long(r, c) => self.dialog = window(r, c),
                Some((r, c, CellMenu::Edit)) => self.editing = start_edit(r, c),
                Some((r, c, CellMenu::Window)) => self.dialog = window(r, c),
                Some((r, c, CellMenu::Null)) => change = Some((r, c, None)),
                None => {}
            }
            match out.edit_done {
                Some(true) => change = self.editing.take().map(|e| (e.row, e.col, Some(e.text))),
                Some(false) => self.editing = None,
                None => {}
            }
        }
        self.detail_ui(ui, theme, t);
        if let Some((r, c, value)) = change {
            self.update_cell(&d, &tb, r, c, value, None);
        }
        if delete_rows {
            self.confirm_delete_rows(&d, &tb, t);
        }
        if insert {
            self.insert_dialog(&d, &tb);
        }
        if let Some(text) = search_now {
            self.search = text;
            self.load_rows(d.clone(), tb.clone(), 0, order.clone());
        }
        if let Some(s) = sort {
            self.load_rows(d.clone(), tb.clone(), 0, Some(s));
        } else if let Some(o) = go {
            self.load_rows(d, tb, o, order);
        }
    }

    /// The whole value of the cell clicked, with a copy button.
    fn detail_ui(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        let Some((column, value)) = self.detail.clone() else { return };
        ui.add_space(8.0);
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&column).size(12.0).strong().color(theme.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").clicked() {
                        self.detail = None;
                    }
                    if ui.small_button(t.db_copy).clicked() {
                        ui.ctx().copy_text(value.clone());
                    }
                });
            });
            egui::ScrollArea::vertical().id_salt("db-detail").max_height(70.0).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&value).monospace().size(12.5)).wrap().selectable(true));
            });
        });
    }

    fn structure_page(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        let (Some(d), Some(tb)) = (self.db.clone(), self.table.clone()) else { return };
        let Some((_, _, s)) = self.structure.as_ref().filter(|(a, b, _)| *a == d && *b == tb) else {
            ui.spinner();
            return;
        };
        let s = s.clone();
        let (mut new_column, mut edit_column) = (false, None);
        egui::ScrollArea::vertical().id_salt("db-structure").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.db_columns.to_uppercase()).size(11.5).strong().color(theme.text_muted));
                ui.label(egui::RichText::new(t.db_column_hint).size(11.5).color(theme.text_muted.gamma_multiply(0.8)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let add = egui::Button::new(egui::RichText::new(format!("+  {}", t.db_add_column)).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 26.0));
                    if ui.add(add).clicked() {
                        new_column = true;
                    }
                });
            });
            ui.add_space(6.0);
            let cols: Vec<String> = [t.db_col_name, t.db_col_type, t.db_col_null, t.db_col_key, t.db_col_default, t.db_col_extra, t.db_col_comment].iter().map(|s| s.to_string()).collect();
            let h = (s.columns.len() as f32 + 1.0) * 26.0 + 6.0;
            let out = grid(ui, egui::Id::new(("db-cols", &d, &tb)), theme, &cols, &s.columns, None, h);
            if let Some((row, _)) = out.double.or(out.clicked.filter(|_| ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)))) {
                edit_column = Some(row);
            }
            ui.add_space(16.0);
            ui.label(egui::RichText::new(t.db_indexes.to_uppercase()).size(11.5).strong().color(theme.text_muted));
            ui.add_space(6.0);
            let cols: Vec<String> = [t.db_col_name, t.db_columns, t.db_col_unique, t.db_col_type].iter().map(|s| s.to_string()).collect();
            let h = (s.indexes.len() as f32 + 1.0) * 26.0 + 6.0;
            grid(ui, egui::Id::new(("db-idx", &d, &tb)), theme, &cols, &s.indexes, None, h);
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("CREATE TABLE").size(11.5).strong().color(theme.text_muted));
                if ui.small_button(t.db_copy).clicked() {
                    ui.ctx().copy_text(s.create.clone());
                }
            });
            ui.add_space(6.0);
            Frame::NONE.fill(theme.chrome_bg).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(&s.create).monospace().size(12.5)).selectable(true));
            });
        });
        let text = |c: Option<&Cell>| match c {
            Some(Cell::Text(v)) => v.clone(),
            _ => String::new(),
        };
        if new_column {
            self.dialog = Some(Dialog::Column(ColumnEdit {
                original: None,
                name: String::new(),
                kind: if self.dialect.mssql() { "NVARCHAR(255)" } else { "VARCHAR(255)" }.into(),
                nullable: true,
                default: String::new(),
                default_null: true,
                default_expr: false,
                extra: String::new(),
                comment: String::new(),
                charset: String::new(),
                collation: String::new(),
            }));
        }
        if let Some(c) = edit_column.and_then(|r| s.columns.get(r)) {
            let name = text(c.first());
            let default = c.get(4);
            let extra = text(c.get(5));
            let (value, expr) = parse_default(&text(default), &extra);
            self.dialog = Some(Dialog::Column(ColumnEdit {
                original: Some(name.clone()),
                name,
                kind: text(c.get(1)),
                nullable: text(c.get(2)) == "YES",
                default_null: matches!(default, Some(Cell::Null) | None) || text(default) == "NULL",
                default: if value == "NULL" { String::new() } else { value },
                default_expr: expr,
                extra,
                comment: text(c.get(6)),
                charset: text(c.get(7)),
                collation: text(c.get(8)),
            }));
        }
    }

    fn sql_page(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        let run_key = ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter)));
        let format_key = ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::F)));
        self.sql_tabs_ui(ui, theme, t);
        ui.add_space(6.0);
        let editor_h = 220.0_f32.min(ui.available_height() * 0.45);
        let k = self.sql_tab;
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let tab = &mut self.sql_tabs[k];
            // Each tab its own editor (cursor, undo).
            let id = egui::Id::new(("db-sql", self.id, tab.id));
            tab.complete.keys(ui, id, &mut tab.sql);
            egui::ScrollArea::vertical().id_salt(("db-sql-editor", tab.id)).max_height(editor_h).show(ui, |ui| {
                let out = {
                    let mut layouter = super::sql::layouter(theme, FontId::monospace(13.0), &mut tab.colors);
                    egui::TextEdit::multiline(&mut tab.sql).id(id).code_editor().frame(Frame::NONE).desired_width(f32::INFINITY).desired_rows(6).hint_text(t.db_sql_hint).font(FontId::monospace(13.0)).layouter(&mut layouter).show(ui)
                };
                let schema = super::sqlcomplete::Schema {
                    brackets: self.dialect.mssql(),
                    db: self.db.as_deref(),
                    databases: self.databases.iter().map(|d| d.name.as_str()).collect(),
                    tables: &self.tables,
                    columns: self.db.as_ref().and_then(|d| self.columns.get(d)),
                    table: self.table.as_deref(),
                };
                tab.complete.show(ui, id, &out, &mut tab.sql, &schema, theme, t);
            });
            if ui.memory(|m| m.has_focus(id)) {
                self.want_columns();
            }
        });
        let mut format = format_key;
        ui.add_space(8.0);
        let mut run = run_key;
        let running_here = self.sql_running && self.query_for == Some(self.sql_tabs[k].id);
        ui.horizontal(|ui| {
            let hint = if cfg!(target_os = "macos") { "⌘ ↩" } else { "Ctrl+↩" };
            let button = egui::Button::new(egui::RichText::new(format!("▶  {}   {hint}", t.db_run)).size(13.0).color(theme.bg)).fill(theme.accent).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
            if ui.add_enabled(!self.sql_running && matches!(self.status, Status::Ready(_)), button).clicked() {
                run = true;
            }
            if running_here {
                ui.spinner();
                if ui.button(format!("■  {}", t.db_stop)).on_hover_text(t.db_stop_hint).clicked() {
                    self.cancel_query();
                }
            }
            let shortcut = if cfg!(target_os = "macos") { "⌘ ⇧ F" } else { "Ctrl+Shift+F" };
            let tidy = egui::Button::new(egui::RichText::new(format!("{}   {shortcut}", t.db_format)).size(13.0)).corner_radius(6.0).min_size(Vec2::new(0.0, 30.0));
            if ui.add_enabled(!self.sql_tabs[k].sql.trim().is_empty(), tidy).on_hover_text(t.db_format_hint).clicked() {
                format = true;
            }
            if let Some(d) = &self.db {
                ui.label(egui::RichText::new(t.db_in_database.replace("{db}", d)).size(12.0).color(theme.text_muted));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match super::sql::history_menu(ui, &self.history, &mut self.history_filter, theme, t) {
                    Some(super::sql::Pick::Load(q)) => self.sql_tabs[k].sql = q,
                    Some(super::sql::Pick::Clear) => self.clear_history(),
                    None => {}
                }
            });
        });
        let tab = &mut self.sql_tabs[k];
        if format && !tab.sql.trim().is_empty() {
            tab.sql = super::sql::format(&tab.sql);
        }
        if run && !self.sql_running {
            self.run_sql(t);
        }
        ui.add_space(10.0);
        let height = ui.available_height();
        self.results_ui(ui, theme, t, height, Some(k));
    }

    /// The SQL page's tabs, each its query and its results (to compare them), and "+" for a new one.
    fn sql_tabs_ui(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        let mut close = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let several = self.sql_tabs.len() > 1;
            let font = FontId::proportional(12.5);
            for (i, tab) in self.sql_tabs.iter().enumerate() {
                let selected = i == self.sql_tab;
                let running = self.sql_running && self.query_for == Some(tab.id);
                let label = t.db_sql_tab.replace("{n}", &(tab.id + 1).to_string());
                // The slot on the right is always kept (the ✕, or the dot of a query running): hovering
                // shows the ✕ in it, nothing moves.
                let slot = if several || running { 18.0 } else { 0.0 };
                let galley = ui.painter().layout_no_wrap(label, font.clone(), if selected { theme.text } else { theme.text_muted });
                let size = Vec2::new(12.0 + galley.size().x + slot + if slot > 0.0 { 4.0 } else { 12.0 }, 26.0);
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                let hovered = resp.hovered();
                let fill = if selected { theme.tab_active } else if hovered { theme.tab_hover } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(rect, 6.0, fill);
                if selected {
                    ui.painter().hline(rect.min.x + 8.0..=rect.max.x - 8.0, rect.max.y - 1.0, Stroke::new(2.0, theme.accent));
                }
                ui.painter().galley(Pos2::new(rect.min.x + 12.0, rect.center().y - galley.size().y / 2.0), galley, theme.text);
                let x_rect = Rect::from_center_size(Pos2::new(rect.max.x - 4.0 - slot / 2.0, rect.center().y), Vec2::splat(16.0));
                let over_x = several && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| x_rect.contains(p));
                if several && (hovered || selected) && !(running && !hovered) {
                    if over_x {
                        ui.painter().rect_filled(x_rect, 4.0, theme.text_muted.gamma_multiply(0.25));
                    }
                    let color = if over_x { theme.text } else { theme.text_muted.gamma_multiply(if hovered { 1.0 } else { 0.6 }) };
                    let (c, r) = (x_rect.center(), 3.5);
                    for (a, b) in [(Vec2::new(-r, -r), Vec2::new(r, r)), (Vec2::new(-r, r), Vec2::new(r, -r))] {
                        ui.painter().line_segment([c + a, c + b], Stroke::new(1.3, color));
                    }
                } else if running {
                    let pulse = (ui.input(|i| i.time) * 4.0).sin() as f32 * 0.35 + 0.65;
                    ui.painter().circle_filled(x_rect.center(), 3.5, theme.accent.gamma_multiply(pulse));
                    ui.ctx().request_repaint();
                }
                let resp = if over_x {
                    resp.on_hover_text(t.db_sql_tab_close)
                } else if tab.sql.trim().is_empty() {
                    resp
                } else {
                    resp.on_hover_text(egui::RichText::new(tab.sql.trim()).monospace().size(12.0))
                };
                if several && (resp.middle_clicked() || (resp.clicked() && over_x)) {
                    close = Some(i);
                } else if resp.clicked() {
                    self.sql_tab = i;
                }
            }
            let add = ui.add(egui::Button::new(egui::RichText::new("+").size(15.0)).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::new(26.0, 26.0))).on_hover_text(t.db_sql_tab_new);
            if add.clicked() {
                self.sql_tabs.push(SqlTab::new(self.next_sql_tab));
                self.next_sql_tab += 1;
                self.sql_tab = self.sql_tabs.len() - 1;
            }
        });
        if let Some(i) = close {
            self.sql_tabs.remove(i);
            if self.sql_tabs.is_empty() {
                self.sql_tabs.push(SqlTab::new(self.next_sql_tab));
                self.next_sql_tab += 1;
            }
            if self.sql_tab > i || self.sql_tab >= self.sql_tabs.len() {
                self.sql_tab = self.sql_tab.saturating_sub(1);
            }
        }
    }

    /// The last query's results: a grid per result with rows, a count of the rows changed otherwise,
    /// or the error.
    /// Those of a SQL tab (by index), else of the query above a table.
    fn results_ui(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings, height: f32, tab: Option<usize>) {
        let (out, salt) = match tab {
            Some(k) => (&self.sql_tabs[k].out, Some(self.sql_tabs[k].id)),
            None => (&self.table_out, None),
        };
        let Some((results, elapsed, error)) = out else { return };
        let (results, elapsed, error) = (results.clone(), *elapsed, error.clone());
        if let Some(e) = &error {
            Frame::NONE.fill(theme.ansi[1].gamma_multiply(0.12)).stroke(Stroke::new(1.0, theme.ansi[1].gamma_multiply(0.5))).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(e).monospace().size(12.5).color(theme.ansi[1])).wrap().selectable(true));
            });
            return;
        }
        let ms = elapsed.as_secs_f64() * 1000.0;
        let mut csv = None;
        egui::ScrollArea::vertical().id_salt(("db-sql-results", salt)).max_height(height).auto_shrink([false, false]).show(ui, |ui| {
            for (i, r) in results.iter().enumerate() {
                let head = if r.columns.is_empty() {
                    t.db_affected.replace("{n}", &r.affected.to_string())
                } else {
                    let mut h = t.db_result_rows.replace("{n}", &r.rows.len().to_string());
                    if r.truncated {
                        h.push_str(&format!("  {}", t.db_truncated.replace("{n}", &db::MAX_QUERY_ROWS.to_string())));
                    }
                    h
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("✓  {head}   ·   {ms:.1} ms")).size(12.5).color(theme.ansi[2]));
                    if !r.columns.is_empty() && !r.rows.is_empty() && ui.small_button("⤓  CSV").on_hover_text(t.db_export_csv).clicked() {
                        csv = Some(i);
                    }
                });
                if !r.columns.is_empty() && !r.rows.is_empty() {
                    ui.add_space(4.0);
                    let h = ((r.rows.len() as f32 + 1.0) * 26.0 + 20.0).min(360.0);
                    let out = grid(ui, egui::Id::new(("db-sql", salt, i)), theme, &r.columns, &r.rows, None, h);
                    if let Some((row, c)) = out.clicked {
                        self.detail = Some((r.columns[c].clone(), r.rows[row][c].text()));
                    }
                }
                ui.add_space(12.0);
            }
            self.detail_ui(ui, theme, t);
        });
        if let Some(r) = csv.and_then(|i| results.get(i)) {
            self.save_csv(r, t);
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context, theme: &Theme, t: &Strings) {
        let dialect = self.dialect;
        let Some(dialog) = &mut self.dialog else { return };
        let mut done: Option<bool> = None;
        let mut drop_column: Option<String> = None;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let width = match dialog {
            Dialog::NewTable { .. } | Dialog::Cell { .. } | Dialog::Insert(_) | Dialog::Review(_) => 660.0,
            _ => 440.0,
        };
        let modal = egui::Modal::new(egui::Id::new("db-dialog")).frame(frame).show(ctx, |ui| {
            ui.set_width(width);
            match dialog {
                Dialog::NewTable { db: d, name, columns, fresh } => {
                    ui.label(egui::RichText::new(format!("{}  ·  {d}", t.db_new_table)).size(17.0).strong());
                    ui.add_space(10.0);
                    let edit = ui.add(egui::TextEdit::singleline(name).hint_text(t.db_table_name).desired_width(f32::INFINITY).font(FontId::monospace(13.0)).margin(Vec2::new(8.0, 6.0)));
                    if *fresh {
                        edit.request_focus();
                        *fresh = false;
                    }
                    ui.add_space(10.0);
                    let mut remove = None;
                    egui::ScrollArea::vertical().id_salt("db-new-table").max_height(300.0).show(ui, |ui| {
                        egui::Grid::new("db-new-table-grid").num_columns(7).spacing([8.0, 6.0]).show(ui, |ui| {
                            for head in [t.db_col_name, t.db_col_type, "NULL", t.db_col_default, t.db_primary, if dialect.mssql() { "IDENTITY" } else { "A_I" }, ""] {
                                ui.label(egui::RichText::new(head).size(11.5).color(theme.text_muted));
                            }
                            ui.end_row();
                            for (i, c) in columns.iter_mut().enumerate() {
                                ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(140.0).font(FontId::monospace(12.5)));
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut c.kind).desired_width(120.0).font(FontId::monospace(12.5)));
                                    egui::ComboBox::from_id_salt(("db-new-type", i)).selected_text("").width(24.0).show_ui(ui, |ui| {
                                        for &kind in column_types(dialect) {
                                            if ui.selectable_label(c.kind.eq_ignore_ascii_case(kind), egui::RichText::new(kind).monospace()).clicked() {
                                                c.kind = kind.to_owned();
                                            }
                                        }
                                    });
                                });
                                ui.checkbox(&mut c.nullable, "");
                                ui.add(egui::TextEdit::singleline(&mut c.default).desired_width(100.0).font(FontId::monospace(12.5)).hint_text("—"));
                                ui.checkbox(&mut c.primary, "");
                                ui.checkbox(&mut c.auto, "");
                                if ui.small_button("✕").clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                        });
                    });
                    if let Some(i) = remove {
                        columns.remove(i);
                    }
                    ui.add_space(6.0);
                    if ui.button(format!("+  {}", t.db_add_column)).clicked() {
                        columns.push(NewColumn { name: String::new(), kind: if dialect.mssql() { "NVARCHAR(255)" } else { "VARCHAR(255)" }.into(), nullable: true, default: String::new(), primary: false, auto: false });
                    }
                    ui.add_space(10.0);
                    Frame::NONE.fill(theme.bg).corner_radius(6.0).inner_margin(10.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(create_table_sql(dialect, d, name, columns)).monospace().size(12.0).color(theme.text_muted)).wrap());
                    });
                }
                Dialog::RenameTable { old, name, fresh, .. } => {
                    // SQL Server: renamed within its schema, the name typed alone.
                    if dialect.mssql() && *fresh {
                        *name = name.rsplit('.').next().unwrap_or(name).to_owned();
                    }
                    ui.label(egui::RichText::new(format!("{}  ·  {old}", t.db_rename_table)).size(17.0).strong());
                    ui.add_space(10.0);
                    let edit = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY).font(FontId::monospace(13.0)).margin(Vec2::new(8.0, 6.0)));
                    if *fresh {
                        edit.request_focus();
                        *fresh = false;
                    }
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(true);
                    }
                }
                Dialog::Insert(fields) => {
                    ui.label(egui::RichText::new(format!("{}  ·  {}", t.db_insert_row, self.table.clone().unwrap_or_default())).size(17.0).strong());
                    ui.add_space(10.0);
                    egui::ScrollArea::vertical().id_salt("db-insert").max_height(380.0).show(ui, |ui| {
                        egui::Grid::new("db-insert-grid").num_columns(3).spacing([12.0, 8.0]).show(ui, |ui| {
                            for (i, f) in fields.iter_mut().enumerate() {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(&f.name).monospace().size(12.5));
                                    ui.label(egui::RichText::new(&f.kind).size(11.0).color(theme.text_muted));
                                });
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut f.mode, FieldMode::Value, egui::RichText::new(t.db_value).size(12.0));
                                    if f.nullable {
                                        ui.selectable_value(&mut f.mode, FieldMode::Null, egui::RichText::new("NULL").size(12.0));
                                    }
                                    ui.selectable_value(&mut f.mode, FieldMode::Default, egui::RichText::new(t.db_default_value).size(12.0));
                                });
                                let edit = ui.add_enabled(f.mode == FieldMode::Value, egui::TextEdit::singleline(&mut f.text).id(egui::Id::new(("db-insert-field", i))).desired_width(260.0).font(FontId::monospace(12.5)).margin(Vec2::new(6.0, 4.0)));
                                // Typing in a field means giving a value.
                                if edit.clicked() {
                                    f.mode = FieldMode::Value;
                                }
                                ui.end_row();
                            }
                        });
                    });
                }
                Dialog::Cell { column, text, .. } => {
                    ui.label(egui::RichText::new(column.as_str()).size(17.0).strong().monospace());
                    ui.add_space(10.0);
                    Frame::NONE.fill(theme.bg).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("db-cell-edit").max_height(380.0).show(ui, |ui| {
                            ui.add(egui::TextEdit::multiline(text).code_editor().frame(Frame::NONE).desired_width(f32::INFINITY).desired_rows(14).font(FontId::monospace(12.5)));
                        });
                    });
                }
                Dialog::NewUser { user, host, password, reveal, access, database } => {
                    ui.label(egui::RichText::new(t.db_new_user).size(17.0).strong());
                    ui.add_space(12.0);
                    egui::Grid::new("db-new-user").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new(t.db_user).color(theme.text_muted));
                        ui.add(egui::TextEdit::singleline(user).desired_width(280.0).margin(Vec2::new(8.0, 5.0)));
                        ui.end_row();
                        ui.label(egui::RichText::new(t.db_host).color(theme.text_muted));
                        ui.vertical(|ui| {
                            ui.add(egui::TextEdit::singleline(host).desired_width(280.0).margin(Vec2::new(8.0, 5.0)));
                            ui.label(egui::RichText::new(t.db_user_host_hint).size(11.0).color(theme.text_muted));
                        });
                        ui.end_row();
                        ui.label(egui::RichText::new(t.db_password).color(theme.text_muted));
                        password_field(ui, password, reveal);
                        ui.end_row();
                        ui.label(egui::RichText::new(t.db_access).color(theme.text_muted));
                        access_picker(ui, access, database, &self.databases, t);
                        ui.end_row();
                    });
                }
                Dialog::Password { user, host, password, reveal } => {
                    ui.label(egui::RichText::new(format!("{}  ·  {user}@{host}", t.db_change_password)).size(17.0).strong());
                    ui.add_space(12.0);
                    password_field(ui, password, reveal);
                }
                Dialog::Grant { user, host, access, database } => {
                    ui.label(egui::RichText::new(format!("{}  ·  {user}@{host}", t.db_add_grant)).size(17.0).strong());
                    ui.add_space(12.0);
                    access_picker(ui, access, database, &self.databases, t);
                    if let Some(sql) = grant_sql(*access, database, user, host) {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(sql).monospace().size(12.0).color(theme.text_muted));
                    }
                }

                Dialog::Transfer { db, table, import, gzip, check_fk } => {
                    let what = table.as_deref().or(db.as_deref()).unwrap_or_default();
                    ui.label(egui::RichText::new(format!("{}  ·  {what}", if *import { t.db_import } else { t.db_export })).size(17.0).strong());
                    ui.add_space(12.0);
                    if !*import {
                        ui.checkbox(gzip, egui::RichText::new(t.db_gzip).size(13.0));
                        option_hint(ui, theme, t.db_gzip_hint);
                        ui.add_space(8.0);
                    }
                    ui.checkbox(check_fk, egui::RichText::new(t.db_check_fk).size(13.0));
                    option_hint(ui, theme, if *import { t.db_check_fk_import_hint } else { t.db_check_fk_export_hint });
                }
                Dialog::NewDatabase { name, fresh } => {
                    ui.label(egui::RichText::new(t.db_new_database).size(17.0).strong());
                    ui.add_space(10.0);
                    let edit = ui.add(egui::TextEdit::singleline(name).hint_text(t.db_new_database_hint).desired_width(f32::INFINITY).margin(Vec2::new(8.0, 6.0)));
                    if *fresh {
                        edit.request_focus();
                        *fresh = false;
                    }
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(true);
                    }
                    if !dialect.mssql() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("utf8mb4 · utf8mb4_unicode_ci").size(11.5).color(theme.text_muted));
                    }
                }
                Dialog::Column(c) => {
                    ui.label(egui::RichText::new(if c.original.is_some() { t.db_edit_column } else { t.db_add_column }).size(17.0).strong());
                    ui.add_space(12.0);
                    egui::Grid::new("db-column").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new(t.db_col_name).color(theme.text_muted));
                        ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(280.0).font(FontId::monospace(13.0)).margin(Vec2::new(8.0, 5.0)));
                        ui.end_row();
                        ui.label(egui::RichText::new(t.db_col_type).color(theme.text_muted));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut c.kind).desired_width(180.0).font(FontId::monospace(13.0)).margin(Vec2::new(8.0, 5.0)));
                            egui::ComboBox::from_id_salt("db-column-type").selected_text("…").width(90.0).show_ui(ui, |ui| {
                                for &kind in column_types(dialect) {
                                    if ui.selectable_label(c.kind.eq_ignore_ascii_case(kind), egui::RichText::new(kind).monospace()).clicked() {
                                        c.kind = kind.to_owned();
                                    }
                                }
                            });
                        });
                        ui.end_row();
                        ui.label(egui::RichText::new(t.db_col_null).color(theme.text_muted));
                        ui.checkbox(&mut c.nullable, t.db_nullable);
                        ui.end_row();
                        // SQL Server keeps a column's default and comment apart: only set when it is added.
                        let fixed = dialect.mssql() && c.original.is_some();
                        if !fixed {
                        ui.label(egui::RichText::new(t.db_col_default).color(theme.text_muted));
                        ui.horizontal(|ui| {
                            let on = !(c.default_null && c.nullable);
                            ui.add_enabled(on, egui::TextEdit::singleline(&mut c.default).desired_width(150.0).hint_text(t.db_no_default).font(FontId::monospace(13.0)).margin(Vec2::new(8.0, 5.0)));
                            ui.add_enabled(c.nullable, egui::Checkbox::new(&mut c.default_null, "NULL"));
                            ui.add_enabled(on, egui::Checkbox::new(&mut c.default_expr, t.db_expression)).on_hover_text(t.db_expression_hint);
                        });
                        ui.end_row();
                        }
                        if !dialect.mssql() {
                            ui.label(egui::RichText::new(t.db_col_comment).color(theme.text_muted));
                            ui.add(egui::TextEdit::singleline(&mut c.comment).desired_width(280.0).margin(Vec2::new(8.0, 5.0)));
                            ui.end_row();
                        }
                        if textual(&c.kind) && !c.collation.is_empty() {
                            ui.label(egui::RichText::new(t.db_col_collation).color(theme.text_muted));
                            ui.label(egui::RichText::new(&c.collation).monospace().size(12.5).color(theme.text_muted));
                            ui.end_row();
                        }
                    });
                    if c.generated() {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(t.db_generated_column).size(12.5).color(theme.ansi[3]));
                    }
                    ui.add_space(10.0);
                    let (d, tb) = (self.db.clone().unwrap_or_default(), self.table.clone().unwrap_or_default());
                    Frame::NONE.fill(theme.bg).corner_radius(6.0).inner_margin(10.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(c.sql(dialect, &d, &tb)).monospace().size(12.0).color(theme.text_muted)).wrap());
                    });
                    if let Some(old) = &c.original {
                        ui.add_space(8.0);
                        let drop = egui::Button::new(egui::RichText::new(format!("🗑  {}", t.db_drop_column)).size(12.5).color(theme.ansi[1])).frame_when_inactive(false).corner_radius(6.0);
                        if ui.add(drop).clicked() {
                            drop_column = Some(old.clone());
                        }
                    }
                }
                Dialog::Review(change) => {
                    ui.label(egui::RichText::new(t.db_review_title).size(17.0).strong());
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t.db_review_hint).size(12.5).color(theme.text_muted));
                    ui.add_space(10.0);
                    sql_box(ui, theme, &change.sql);
                }
                Dialog::Confirm { sql, reason, fk, .. } => {
                    ui.label(egui::RichText::new(t.guard_title).size(18.0).strong().color(theme.ansi[1]));
                    ui.add_space(10.0);
                    sql_box(ui, theme, sql);
                    if !reason.is_empty() {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(reason.as_str()).size(13.0));
                    }
                    if let Some(check) = fk {
                        ui.add_space(8.0);
                        ui.checkbox(check, egui::RichText::new(t.db_check_fk).size(13.0));
                        option_hint(ui, theme, t.db_check_fk_drop_hint);
                    }
                }
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, fill) = match dialog {
                    Dialog::NewDatabase { .. } | Dialog::NewTable { .. } | Dialog::NewUser { .. } => (t.db_create, theme.accent),
                    Dialog::Insert(_) => (t.db_insert, theme.accent),
                    Dialog::Grant { .. } => (t.db_grant, theme.accent),
                    Dialog::Column(_) | Dialog::RenameTable { .. } | Dialog::Cell { .. } | Dialog::Password { .. } => (t.save, theme.accent),
                    Dialog::Confirm { .. } => (t.guard_run, theme.ansi[1]),
                    Dialog::Transfer { .. } => (t.db_choose_file, theme.accent),
                    Dialog::Review(_) => (t.db_run, theme.accent),
                };
                if ui.add(egui::Button::new(egui::RichText::new(label).size(13.5).color(theme.bg)).fill(fill).corner_radius(6.0).min_size(Vec2::new(110.0, 30.0))).clicked() {
                    done = Some(true);
                }
                if ui.add(egui::Button::new(egui::RichText::new(t.cancel).size(13.5)).corner_radius(6.0).min_size(Vec2::new(96.0, 30.0))).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() {
            done.get_or_insert(false);
        }
        // Dropping a column: asked again, like any destruction.
        if let Some(column) = drop_column {
            let (d, tb) = (self.db.clone().unwrap_or_default(), self.table.clone().unwrap_or_default());
            let sql = format!("ALTER TABLE {} DROP COLUMN {}", dialect.table(&d, &tb), dialect.ident(&column));
            self.dialog = Some(Dialog::Confirm { sql, db: None, reason: t.db_drop_column_reason.to_owned(), query: false, expect: None, fk: None });
            return;
        }
        let Some(ok) = done else { return };
        let Some(dialog) = self.dialog.take() else { return };
        if !ok {
            // A change not wanted: back to the dialog it was made in.
            if let Dialog::Review(change) = dialog {
                self.dialog = change.back.map(|b| *b);
            }
            return;
        }
        // Shown again if the change it makes is cancelled.
        let back = Some(Box::new(dialog.clone()));
        match dialog {
            Dialog::Review(change) => self.run_change(change),
            Dialog::Transfer { db, table, import, gzip, check_fk } => {
                // Kept for the next time.
                (self.gzip, self.check_fk) = (gzip, check_fk);
                if import {
                    self.import(db.as_deref(), t);
                } else if let Some(d) = db {
                    self.export(&d, table.as_deref(), t);
                }
            }
            Dialog::Column(c) => {
                let (d, tb) = (self.db.clone().unwrap_or_default(), self.table.clone().unwrap_or_default());
                if !c.name.trim().is_empty() && !c.kind.trim().is_empty() && !c.generated() {
                    // sp_rename works in the table's database.
                    let db = dialect.mssql().then(|| d.clone());
                    self.change(Change { back, db, ..Change::new(c.sql(dialect, &d, &tb), None) });
                }
            }
            Dialog::NewTable { db: d, name, columns, .. } => {
                if !name.trim().is_empty() && columns.iter().any(|c| !c.name.trim().is_empty()) {
                    let expect = Some(Expect::Open(d.clone(), with_schema(dialect, &name)));
                    self.change(Change { back, ..Change::new(create_table_sql(dialect, &d, &name, &columns), expect) });
                }
            }
            Dialog::RenameTable { db: d, old, name, .. } => {
                let name = name.trim().to_owned();
                if !name.is_empty() && name != old {
                    if dialect.mssql() {
                        // The new name stays in the table's schema.
                        let schema = old.split_once('.').map_or("dbo", |(s, _)| s);
                        let bare = name.rsplit('.').next().unwrap_or(&name).to_owned();
                        let sql = format!("EXEC sp_rename {}, {}", dialect.literal(&dialect.local_table(&old)), dialect.literal(&bare));
                        let open = Expect::Open(d.clone(), format!("{schema}.{bare}"));
                        self.change(Change { back, db: Some(d), ..Change::new(sql, Some(open)) });
                    } else {
                        let sql = format!("RENAME TABLE {0}.{1} TO {0}.{2}", db::ident(&d), db::ident(&old), db::ident(&name));
                        self.change(Change { back, ..Change::new(sql, Some(Expect::Open(d, name))) });
                    }
                }
            }
            Dialog::Insert(fields) => {
                if let (Some(d), Some(tb)) = (self.db.clone(), self.table.clone()) {
                    self.change(Change { reload: true, back, ..Change::new(insert_sql(dialect, &d, &tb, &fields), Some(Expect::Insert)) });
                }
            }
            Dialog::Cell { row, col, text, .. } => {
                if let (Some(d), Some(tb)) = (self.db.clone(), self.table.clone()) {
                    self.update_cell(&d, &tb, row, col, Some(text), back);
                }
            }
            Dialog::NewUser { user, host, password, access, database, .. } => {
                let (user, host) = (user.trim().to_owned(), if host.trim().is_empty() { "%".to_owned() } else { host.trim().to_owned() });
                if !user.is_empty() {
                    let mut sql = format!("CREATE USER {} IDENTIFIED BY {}", db::account(&user, &host), db::literal(&password));
                    if let Some(grant) = grant_sql(access, &database, &user, &host) {
                        sql.push_str(";\n");
                        sql.push_str(&grant);
                    }
                    self.change(Change { select_user: Some((user, host)), back, ..Change::new(sql, Some(Expect::Users)) });
                }
            }
            Dialog::Password { user, host, password, .. } => {
                let sql = format!("ALTER USER {} IDENTIFIED BY {}", db::account(&user, &host), db::literal(&password));
                self.change(Change { back, ..Change::new(sql, Some(Expect::Users)) });
            }
            Dialog::Grant { user, host, access, database } => {
                if let Some(sql) = grant_sql(access, &database, &user, &host) {
                    self.change(Change { back, ..Change::new(sql, Some(Expect::Users)) });
                }
            }
            Dialog::NewDatabase { name, .. } => {
                let name = name.trim();
                if !name.is_empty() {
                    let sql = if dialect.mssql() { format!("CREATE DATABASE {}", dialect.ident(name)) } else { format!("CREATE DATABASE {} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci", db::ident(name)) };
                    self.change(Change { back, ..Change::new(sql, None) });
                }
            }
            Dialog::Confirm { sql, db, query, expect, fk, .. } => {
                if matches!(expect, Some(Expect::Dropped { .. })) {
                    self.exec(db, sql, expect);
                } else if query {
                    self.send_query(db, sql);
                } else {
                    // What was just removed is no longer selected.
                    if sql.starts_with("DROP DATABASE") {
                        self.db = None;
                        self.table = None;
                    } else if sql.starts_with("DROP") || sql.contains("\nDROP ") {
                        self.table = None;
                    }
                    let reload = self.rows.as_ref().map(|r| (r.db.clone(), r.table.clone(), r.offset, r.order.clone()));
                    let keep_rows = sql.starts_with("DELETE FROM") || sql.starts_with("TRUNCATE") || sql.contains("\nTRUNCATE ");
                    if !keep_rows {
                        self.rows = None;
                    }
                    // Tables that refer to each other: MySQL refuses to drop or empty them unless the
                    // checks are off (for this session only; db.rs turns them back on even on failure).
                    let sql = if fk == Some(false) { format!("SET FOREIGN_KEY_CHECKS = 0;\n{sql};\nSET FOREIGN_KEY_CHECKS = 1") } else { sql };
                    self.exec(db, sql, expect);
                    if let (true, Some((d, tb, _, order))) = (keep_rows, reload) {
                        self.load_rows(d, tb, 0, order);
                    }
                }
            }
        }
    }
}

/// What an option of a window does, under it.
fn option_hint(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.indent("hint", |ui| {
        ui.label(egui::RichText::new(text).size(11.5).color(theme.text_muted));
    });
}

/// SQL to be run, laid out and colored, in a box that scrolls when long.
fn sql_box(ui: &mut Ui, theme: &Theme, sql: &str) {
    Frame::NONE.fill(theme.bg).corner_radius(6.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt("db-sql-box").max_height(320.0).show(ui, |ui| {
            let mut job = super::sql::highlight(&super::sql::format(sql), theme, &FontId::monospace(13.0));
            job.wrap.max_width = ui.available_width();
            ui.add(egui::Label::new(job).selectable(true));
        });
    });
}

/// A password field with an eye to show it.
fn password_field(ui: &mut Ui, password: &mut String, reveal: &mut bool) {
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(password).password(!*reveal).desired_width(240.0).margin(Vec2::new(8.0, 5.0)));
        if ui.add(egui::Button::new(if *reveal { "🙈" } else { "👁" }).frame_when_inactive(false)).clicked() {
            *reveal = !*reveal;
        }
    });
}

/// What an account may do, and on which database (empty: all).
fn access_picker(ui: &mut Ui, access: &mut Access, database: &mut String, databases: &[DatabaseInfo], t: &Strings) {
    ui.vertical(|ui| {
        let label = |a: Access| match a {
            Access::Nothing => t.db_access_none,
            Access::Read => t.db_access_read,
            Access::Write => t.db_access_write,
            Access::All => t.db_access_all,
        };
        egui::ComboBox::from_id_salt("db-access").selected_text(label(*access)).width(240.0).show_ui(ui, |ui| {
            for a in [Access::Nothing, Access::Read, Access::Write, Access::All] {
                ui.selectable_value(access, a, label(a));
            }
        });
        if *access != Access::Nothing {
            let shown = if database.is_empty() { t.db_all_databases.to_owned() } else { database.clone() };
            egui::ComboBox::from_id_salt("db-access-db").selected_text(shown).width(240.0).show_ui(ui, |ui| {
                ui.selectable_value(database, String::new(), t.db_all_databases);
                for d in databases.iter().filter(|d| !system_db(Dialect(crate::config::Engine::Mysql), &d.name)) {
                    ui.selectable_value(database, d.name.clone(), &d.name);
                }
            });
        }
    });
}

/// What the user did in a grid.
struct GridOut {
    /// Header clicked: sort by this column.
    sort: Option<String>,
    /// Cell clicked: row, column.
    clicked: Option<(usize, usize)>,
    double: Option<(usize, usize)>,
    /// The cell being edited: Enter (true) or given up (false).
    edit_done: Option<bool>,
    menu: Option<(usize, usize, CellMenu)>,
}

/// What a grid can do besides showing: tick rows, edit a cell in place, a menu on cells.
#[derive(Default)]
struct GridExtras<'a> {
    edit: Option<&'a mut CellEdit>,
    selection: Option<&'a mut HashSet<usize>>,
    menus: Option<&'a Strings>,
}

/// A table of values: fixed header, rows laid out only when on screen, horizontal scroll for wide
/// tables. NULL is shown apart.
fn grid(ui: &mut Ui, id: egui::Id, theme: &Theme, columns: &[String], rows: &[Vec<Cell>], sort: Option<(&str, bool)>, height: f32) -> GridOut {
    grid_with(ui, id, theme, columns, rows, sort, height, GridExtras::default())
}

#[allow(clippy::too_many_arguments)]
fn grid_with(ui: &mut Ui, id: egui::Id, theme: &Theme, columns: &[String], rows: &[Vec<Cell>], sort: Option<(&str, bool)>, height: f32, mut extras: GridExtras) -> GridOut {
    let mut out = GridOut { sort: None, clicked: None, double: None, edit_done: None, menu: None };
    // Ticked rows: a column of boxes on the left.
    let check_w = if extras.selection.is_some() { 34.0 } else { 0.0 };
    let font = FontId::monospace(12.5);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    let row_h = 26.0;
    // Widths from the header and the first rows.
    let widths: Vec<f32> = (0..columns.len())
        .map(|c| {
            let longest = rows.iter().take(80).map(|r| r.get(c).map_or(4, |v| v.text().chars().count().min(60))).max().unwrap_or(0).max(columns[c].chars().count() + 2);
            (longest as f32 * char_w + 22.0).clamp(56.0, 380.0)
        })
        .collect();
    let total_w: f32 = (widths.iter().sum::<f32>() + check_w).max(ui.available_width());
    let frame = Frame::NONE.stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(8.0);
    frame.show(ui, |ui| {
        ui.set_max_height(height.max(80.0));
        // Solid bars: floating ones would cover the last row.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::horizontal().id_salt(id.with("h")).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(total_w);
            // Header.
            let (head, _) = ui.allocate_exact_size(Vec2::new(total_w, row_h + 2.0), Sense::hover());
            ui.painter().rect_filled(head, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, theme.chrome_bg);
            if let Some(sel) = extras.selection.as_deref_mut() {
                // Ticks them all, or none once all are.
                let all = !rows.is_empty() && sel.len() == rows.len();
                let some = !sel.is_empty() && !all;
                let r = Rect::from_min_size(head.min, Vec2::new(check_w, head.height()));
                if paint_checkbox(ui, r, id.with("check-all"), all, some, theme) {
                    if all { sel.clear() } else { *sel = (0..rows.len()).collect() }
                }
            }
            let mut x = head.min.x + check_w;
            for (c, name) in columns.iter().enumerate() {
                let r = Rect::from_min_size(Pos2::new(x, head.min.y), Vec2::new(widths[c], head.height()));
                let resp = ui.interact(r, id.with(("head", c)), Sense::click());
                let arrow = match sort {
                    Some((s, asc)) if s == name => if asc { " ▲" } else { " ▼" },
                    _ => "",
                };
                let color = if resp.hovered() { theme.accent } else { theme.text };
                let mut job = egui::text::LayoutJob::simple_singleline(format!("{name}{arrow}"), FontId::proportional(12.5), color);
                job.wrap = egui::text::TextWrapping::truncate_at_width(widths[c] - 16.0);
                let g = ui.painter().layout_job(job);
                ui.painter().galley(Pos2::new(r.min.x + 10.0, r.center().y - g.size().y / 2.0), g, color);
                if c > 0 {
                    ui.painter().vline(r.min.x, r.y_range(), Stroke::new(1.0, theme.tab_hover));
                }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    out.sort = Some(name.clone());
                }
                x += widths[c];
            }
            ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, theme.tab_hover));
            // Rows.
            egui::ScrollArea::vertical().id_salt(id.with("v")).auto_shrink([false, true]).max_height(ui.available_height()).show_rows(ui, row_h, rows.len(), |ui, range| {
                for i in range {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(total_w, row_h), Sense::click());
                    let ticked = extras.selection.as_deref().is_some_and(|s| s.contains(&i));
                    if ticked {
                        ui.painter().rect_filled(r, 0.0, theme.accent.gamma_multiply(0.16));
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, theme.accent.gamma_multiply(0.08));
                    } else if i % 2 == 1 {
                        ui.painter().rect_filled(r, 0.0, theme.chrome_bg.gamma_multiply(0.45));
                    }
                    if let Some(sel) = extras.selection.as_deref_mut() {
                        let b = Rect::from_min_size(r.min, Vec2::new(check_w, row_h));
                        if paint_checkbox(ui, b, id.with(("check", i)), ticked, false, theme) && !sel.remove(&i) {
                            sel.insert(i);
                        }
                    }
                    let mut x = r.min.x + check_w;
                    for (c, w) in widths.iter().enumerate() {
                        let cell_rect = Rect::from_min_size(Pos2::new(x, r.min.y), Vec2::new(*w, row_h));
                        // The cell being edited: a field in its place.
                        if let Some(e) = extras.edit.as_deref_mut().filter(|e| e.row == i && e.col == c) {
                            ui.painter().rect_filled(cell_rect.shrink(1.0), 3.0, theme.bg);
                            ui.painter().rect_stroke(cell_rect.shrink(1.0), 3.0, Stroke::new(1.5, theme.accent), egui::StrokeKind::Inside);
                            let mut field = ui.new_child(egui::UiBuilder::new().max_rect(cell_rect.shrink2(Vec2::new(8.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                            let te = field.add(egui::TextEdit::singleline(&mut e.text).font(font.clone()).frame(Frame::NONE).desired_width(cell_rect.width() - 16.0));
                            if e.fresh {
                                te.request_focus();
                                e.fresh = false;
                            } else if te.lost_focus() {
                                // Enter writes it; Escape or a click elsewhere gives up.
                                out.edit_done = Some(ui.input(|inp| inp.key_pressed(Key::Enter)));
                            }
                            x += w;
                            continue;
                        }
                        let value = rows[i].get(c).cloned().unwrap_or(Cell::Null);
                        let (text, color, italic) = match &value {
                            Cell::Null => ("NULL".to_owned(), theme.text_muted.gamma_multiply(0.7), true),
                            Cell::Bytes(_) => (value.text(), theme.ansi[5], false),
                            Cell::Text(s) => (s.replace(['\n', '\r'], " ⏎ "), theme.text, false),
                        };
                        let mut job = egui::text::LayoutJob::default();
                        job.append(&text, 0.0, egui::TextFormat { font_id: font.clone(), color, italics: italic, ..Default::default() });
                        job.wrap = egui::text::TextWrapping::truncate_at_width(w - 16.0);
                        let g = ui.painter().layout_job(job);
                        ui.painter().with_clip_rect(cell_rect).galley(Pos2::new(cell_rect.min.x + 10.0, cell_rect.center().y - g.size().y / 2.0), g, color);
                        if c > 0 {
                            ui.painter().vline(cell_rect.min.x, cell_rect.y_range(), Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.5)));
                        }
                        x += w;
                    }
                    let x0 = r.min.x + check_w;
                    let col_at = |p: Pos2| {
                        let mut acc = x0;
                        (p.x >= x0).then(|| widths.iter().position(|w| {
                            acc += w;
                            p.x < acc
                        })).flatten()
                    };
                    if let Some(col) = resp.interact_pointer_pos().and_then(col_at).filter(|c| *c < columns.len()) {
                        if resp.double_clicked() {
                            out.double = Some((i, col));
                        } else if resp.clicked() {
                            out.clicked = Some((i, col));
                        }
                        if resp.secondary_clicked() {
                            ui.data_mut(|d| d.insert_temp(id.with("menu-target"), (i, col)));
                        }
                    }
                    if let Some(t) = extras.menus {
                        resp.context_menu(|ui| {
                            let Some((mi, mc)) = ui.data(|d| d.get_temp::<(usize, usize)>(id.with("menu-target"))) else { return };
                            for (label, what) in [(t.db_copy, CellMenu::Copy), (t.edit, CellMenu::Edit), (t.db_edit_window, CellMenu::Window), (t.db_set_null, CellMenu::Null)] {
                                if ui.button(label).clicked() {
                                    out.menu = Some((mi, mc, what));
                                    ui.close();
                                }
                            }
                        });
                    }
                }
            });
        });
    });
    out
}

/// A tick box in `r`; returns whether it was clicked. `some`: a dash (part of the rows ticked).
fn paint_checkbox(ui: &mut Ui, r: Rect, id: egui::Id, on: bool, some: bool, theme: &Theme) -> bool {
    let resp = ui.interact(r, id, Sense::click());
    let b = Rect::from_center_size(r.center(), Vec2::splat(14.0));
    if on || some {
        ui.painter().rect_filled(b, 3.5, theme.accent);
    } else {
        ui.painter().rect_stroke(b, 3.5, Stroke::new(1.3, if resp.hovered() { theme.accent } else { theme.text_muted.gamma_multiply(0.8) }), egui::StrokeKind::Inside);
    }
    let stroke = Stroke::new(1.8, theme.bg);
    let c = b.center();
    if on {
        ui.painter().line_segment([c + Vec2::new(-3.5, 0.2), c + Vec2::new(-1.0, 2.8)], stroke);
        ui.painter().line_segment([c + Vec2::new(-1.0, 2.8), c + Vec2::new(3.8, -2.6)], stroke);
    } else if some {
        ui.painter().hline(c.x - 3.5..=c.x + 3.5, c.y, stroke);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// "12", "3,4 k", "1,2 M".
fn short_count(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..1_000_000 => format!("{:.1} k", n as f64 / 1000.0),
        _ => format!("{:.1} M", n as f64 / 1_000_000.0),
    }
}

fn format_size(bytes: u64, t: &Strings) -> String {
    let units = [t.unit_b, t.unit_kb, t.unit_mb, t.unit_gb, t.unit_tb];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if unit == 0 { bytes.to_string() } else { format!("{value:.1}") };
    let text = if t.decimal_comma { text.replace('.', ",") } else { text };
    format!("{text} {}", units[unit])
}

/// A small cylinder (a database).
pub(super) fn paint_db_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let (w, h) = (11.0, 13.0);
    let top = c.y - h / 2.0;
    let stroke = Stroke::new(1.3, color);
    let body = Rect::from_min_max(Pos2::new(c.x - w / 2.0, top + 2.0), Pos2::new(c.x + w / 2.0, top + h - 2.0));
    painter.rect_filled(body, 0.0, color.gamma_multiply(0.18));
    painter.line_segment([body.left_top(), body.left_bottom()], stroke);
    painter.line_segment([body.right_top(), body.right_bottom()], stroke);
    for y in [top + 2.0, top + h / 2.0, top + h - 2.0] {
        let ellipse = egui::Shape::ellipse_stroke(Pos2::new(c.x, y), Vec2::new(w / 2.0, 2.0), stroke);
        painter.add(ellipse);
    }
}

/// A small grid (a table), dashed for a view.
fn paint_table_icon(painter: &egui::Painter, c: Pos2, view: bool, color: Color32) {
    let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
    let stroke = Stroke::new(1.1, color);
    if view {
        painter.rect_stroke(r, 2.0, Stroke::new(1.0, color.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    } else {
        painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
        painter.hline(r.x_range(), r.min.y + 3.5, stroke);
        painter.vline(r.center().x, r.min.y + 3.5..=r.max.y, Stroke::new(1.0, color.gamma_multiply(0.7)));
    }
}

fn paint_chevron_small(painter: &egui::Painter, c: Pos2, open: bool, color: Color32) {
    let s = 3.5;
    let points = if open { vec![c + Vec2::new(-s, -s / 2.0), c + Vec2::new(s, -s / 2.0), c + Vec2::new(0.0, s)] } else { vec![c + Vec2::new(-s / 2.0, -s), c + Vec2::new(s, 0.0), c + Vec2::new(-s / 2.0, s)] };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    const MYSQL: Dialect = Dialect(crate::config::Engine::Mysql);
    const MSSQL: Dialect = Dialect(crate::config::Engine::Sqlserver);

    #[test]
    fn writes_column_changes() {
        let column = |name: &str, kind: &str, default: &str, extra: &str| {
            let (default, default_expr) = parse_default(default, extra);
            ColumnEdit { original: Some(name.into()), name: name.into(), kind: kind.into(), nullable: false, default, default_null: false, default_expr, extra: extra.into(), comment: String::new(), charset: String::new(), collation: String::new() }
        };
        let mut c = column("nom", "VARCHAR(100)", "'x''y'", "");
        c.name = "name".into();
        c.charset = "latin1".into();
        c.collation = "latin1_swedish_ci".into();
        c.comment = "Nom d'usage".into();
        assert_eq!(c.sql(MYSQL, "app", "users"), "ALTER TABLE `app`.`users` CHANGE COLUMN `nom` `name` VARCHAR(100) CHARACTER SET latin1 COLLATE latin1_swedish_ci NOT NULL DEFAULT 'x''y' COMMENT 'Nom d''usage'");
        c.original = None;
        c.nullable = true;
        c.default_null = true;
        c.kind = "INT".into();
        c.comment.clear();
        assert_eq!(c.sql(MYSQL, "app", "users"), "ALTER TABLE `app`.`users` ADD COLUMN `name` INT NULL DEFAULT NULL");
        let id = column("id", "int(11)", "", "auto_increment");
        assert_eq!(id.sql(MYSQL, "app", "users"), "ALTER TABLE `app`.`users` CHANGE COLUMN `id` `id` int(11) NOT NULL AUTO_INCREMENT");
        // MariaDB and MySQL spell expressions differently; both stay expressions.
        let ts = column("at", "TIMESTAMP(3)", "current_timestamp(3)", "on update current_timestamp(3)");
        assert_eq!(ts.sql(MYSQL, "a", "b"), "ALTER TABLE `a`.`b` CHANGE COLUMN `at` `at` TIMESTAMP(3) NOT NULL DEFAULT current_timestamp(3) ON UPDATE CURRENT_TIMESTAMP(3)");
        let mysql = column("at", "datetime", "CURRENT_TIMESTAMP", "DEFAULT_GENERATED");
        assert_eq!(mysql.sql(MYSQL, "a", "b"), "ALTER TABLE `a`.`b` CHANGE COLUMN `at` `at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP");
        let uuid = column("u", "char(36)", "uuid()", "");
        assert!(uuid.default_expr);
        assert!(column("g", "int", "", "VIRTUAL GENERATED").generated());
    }

    #[test]
    fn writes_sql_server_changes() {
        let mut c = ColumnEdit { original: Some("nom".into()), name: "name".into(), kind: "nvarchar(100)".into(), nullable: false, default: String::new(), default_null: false, default_expr: false, extra: String::new(), comment: String::new(), charset: String::new(), collation: "French_CI_AS".into() };
        assert_eq!(c.sql(MSSQL, "app", "sales.users"), "ALTER TABLE [app].[sales].[users] ALTER COLUMN [nom] nvarchar(100) COLLATE French_CI_AS NOT NULL;\nEXEC sp_rename N'[sales].[users].[nom]', N'name', 'COLUMN'");
        c.original = None;
        c.nullable = true;
        c.default = "x'y".into();
        assert_eq!(c.sql(MSSQL, "app", "users"), "ALTER TABLE [app].[dbo].[users] ADD [name] nvarchar(100) NULL DEFAULT N'x''y'");
        let id = NewColumn { name: "id".into(), kind: "INT".into(), nullable: false, default: String::new(), primary: true, auto: true };
        let label = NewColumn { name: "label".into(), kind: "NVARCHAR(255)".into(), nullable: true, default: "a\\b".into(), primary: false, auto: false };
        assert_eq!(create_table_sql(MSSQL, "app", "items", &[id, label]), "CREATE TABLE [app].[dbo].[items] (\n  [id] INT IDENTITY(1,1) NOT NULL,\n  [label] NVARCHAR(255) NULL DEFAULT N'a\\b',\n  PRIMARY KEY ([id])\n)");
        let field = |name: &str, mode: FieldMode, text: &str| InsertField { name: name.into(), kind: String::new(), mode, text: text.into(), nullable: true };
        assert_eq!(insert_sql(MSSQL, "app", "dbo.items", &[field("id", FieldMode::Default, ""), field("label", FieldMode::Value, "é"), field("n", FieldMode::Null, "")]), "INSERT INTO [app].[dbo].[items] ([label], [n]) VALUES (N'é', NULL)");
        assert_eq!(insert_sql(MSSQL, "app", "dbo.items", &[field("id", FieldMode::Default, "")]), "INSERT INTO [app].[dbo].[items] DEFAULT VALUES");
        assert_eq!(with_schema(MSSQL, "items"), "dbo.items");
        assert_eq!(with_schema(MYSQL, "items"), "items");
    }
}
