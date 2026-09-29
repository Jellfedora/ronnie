//! MariaDB / MySQL client for the database view: one connection per open view, in the background. The
//! view sends requests (databases, tables, structure, rows, SQL) and gets events back, and never waits
//! on the server. Exports and imports get a connection of their own (the view stays usable meanwhile),
//! and a query running too long can be stopped (KILL QUERY, from yet another connection). A server that
//! only listens on its own machine is reached through an SSH port forward.

use std::io::{Read, Write as _};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use mysql_async::prelude::*;
use mysql_async::{Conn, OptsBuilder, Row, Value};
use tokio::sync::mpsc as tmpsc;

/// Rows kept from one result of a SQL query typed by the user (the rest is read and dropped).
pub const MAX_QUERY_ROWS: usize = 1000;
/// Above this estimate, a table's rows aren't counted exactly (COUNT(*) could take long).
const EXACT_COUNT_MAX: u64 = 2_000_000;
/// What an export or import stopped by the user ends with.
pub const CANCELLED: &str = "\u{1}cancelled";
/// How long to wait for an SSH forward to open (a password may have to be typed first).
const TUNNEL_WAIT: Duration = Duration::from_secs(180);

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("db").enable_all().build().expect("tokio runtime"))
}

/// Where and as whom to connect.
#[derive(Clone, Debug)]
pub struct Target {
    /// The server's address; seen from the SSH host when going through `tunnel`.
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: Option<String>,
    /// The ssh command reaching the server's machine (it ends with "--", host); none: direct.
    pub tunnel: Option<crate::ssh::Launch>,
}

/// A value as shown in a grid.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Text(String),
    /// Binary data (not text): its size.
    Bytes(usize),
}

impl Cell {
    pub fn text(&self) -> String {
        match self {
            Cell::Null => "NULL".into(),
            Cell::Text(t) => t.clone(),
            Cell::Bytes(n) => format!("[BLOB · {n} o]"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DatabaseInfo {
    pub name: String,
    pub collation: String,
    pub tables: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TableInfo {
    pub name: String,
    pub view: bool,
    pub engine: String,
    /// An estimate for InnoDB.
    pub rows: u64,
    pub size: u64,
    pub collation: String,
    pub comment: String,
}

#[derive(Clone, Debug, Default)]
pub struct Structure {
    /// Name, type, null, key, default, extra, comment, then (not shown) character set and collation.
    pub columns: Vec<Vec<Cell>>,
    /// Name, columns, unique, type.
    pub indexes: Vec<Vec<Cell>>,
    pub create: String,
}

/// One result of a query: rows, or how many rows it changed.
#[derive(Clone, Debug, Default)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    /// More rows than kept.
    pub truncated: bool,
    pub affected: u64,
}

/// An account of the server.
#[derive(Clone, Debug, Default)]
pub struct UserInfo {
    pub user: String,
    pub host: String,
}

#[derive(Clone, Debug)]
pub enum Request {
    Databases,
    Tables(String),
    Structure { db: String, table: String },
    /// Rows of a table; `search`: only those where one of these columns contains this text. Without
    /// `order`, in the order of the primary key (pages would otherwise overlap).
    Rows { db: String, table: String, offset: u64, limit: u64, order: Option<(String, bool)>, search: Option<(String, Vec<String>)> },
    Query { db: Option<String>, sql: String },
    /// A change (create, drop, update...): Done with the rows it changed, and `tag` (the caller's).
    Exec { db: Option<String>, sql: String, tag: u32 },
    /// Writes database `db` (or only `tables` of it) to a .sql file (.sql.gz: compressed): tables and
    /// their rows, views, and for a whole database its procedures, functions, triggers and events.
    /// `check_fk`: a file that imports with foreign keys checked (tables in dependency order).
    Export { db: String, tables: Option<Vec<String>>, path: std::path::PathBuf, check_fk: bool },
    /// Writes the rows of a table to a .csv file.
    ExportCsv { db: String, table: String, path: std::path::PathBuf },
    /// Runs the statements of a .sql (or .sql.gz) file in `db` (or where it says), foreign keys checked
    /// or not.
    Import { db: Option<String>, path: std::path::PathBuf, check_fk: bool },
    /// The server's accounts.
    Users,
    /// What an account may do.
    Grants { user: String, host: String },
}

#[derive(Debug)]
pub enum Event {
    Connected { version: String },
    Databases(Vec<DatabaseInfo>),
    Tables { db: String, tables: Vec<TableInfo> },
    Structure { db: String, table: String, structure: Structure },
    Rows { db: String, table: String, offset: u64, result: QueryResult, total: u64, exact: bool },
    Query { results: Vec<QueryResult>, elapsed: Duration, error: Option<String> },
    /// A change went through; it changed this many rows. `tag`: the Exec's (0 after an import).
    Done { affected: u64, tag: u32 },
    /// A change (Exec) refused by the server, with its `tag`.
    Failed { error: String, tag: u32 },
    /// How far an export or import went (0 to 1), and what it is on.
    Progress { fraction: f32, text: String },
    /// An export or import finished: what it did, or why it stopped (CANCELLED: the user stopped it).
    Transfer(Result<String, String>),
    Users(Vec<UserInfo>),
    Grants { user: String, host: String, grants: Vec<String> },
    Error(String),
    /// The connection is over (couldn't be made, or lost).
    Closed(String),
}

/// An open connection. Dropping it closes it (and its SSH forward).
pub struct Connection {
    requests: tmpsc::UnboundedSender<Request>,
    events: Receiver<Event>,
    stop: Arc<tokio::sync::Notify>,
    stop_tunnel: Arc<tokio::sync::Notify>,
    /// How to reach the server from here (through the forward when there is one).
    direct: Target,
    /// Server-side ids of the view's connection and of the transfer's, for KILL QUERY.
    thread: Arc<AtomicU32>,
    transfer_thread: Arc<AtomicU32>,
    cancel_transfer: Arc<AtomicBool>,
    /// The ssh process of the forward: the askpass helper answers it (see askpass.rs).
    pub tunnel_pid: Option<u32>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.notify_one();
        self.stop_tunnel.notify_one();
        self.cancel_transfer.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone)]
struct Emitter {
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl Emitter {
    fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        self.ctx.request_repaint();
    }
}

/// Places where local servers keep their socket (Homebrew, Debian and friends, MAMP).
const SOCKETS: [&str; 5] = ["/tmp/mysql.sock", "/var/run/mysqld/mysqld.sock", "/tmp/mysqld.sock", "/opt/homebrew/var/mysql/mysql.sock", "/Applications/MAMP/tmp/mysql/mysql.sock"];

/// A server runs on this machine (its socket, or something on port 3306).
pub fn local_server() -> bool {
    SOCKETS.iter().any(|s| std::path::Path::new(s).exists()) || std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], 3306).into(), Duration::from_millis(150)).is_ok()
}

fn opts(target: &Target) -> OptsBuilder {
    let mut o = OptsBuilder::default()
        .ip_or_hostname(target.host.clone())
        .tcp_port(target.port)
        .user(Some(target.user.clone()))
        .pass(target.password.clone())
        .db_name(target.database.clone().filter(|d| !d.is_empty()))
        .prefer_socket(false);
    // "localhost" means the local socket, as for the mysql client (accounts like user@localhost may only
    // be allowed that way); 127.0.0.1 goes through TCP.
    if target.host == "localhost" && target.tunnel.is_none() {
        if let Some(socket) = SOCKETS.iter().find(|s| std::path::Path::new(s).exists()) {
            o = o.socket(Some(*socket));
        }
    }
    o
}

async fn connect(target: &Target) -> Result<Conn, String> {
    match tokio::time::timeout(Duration::from_secs(10), Conn::new(opts(target))).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timeout".into()),
    }
}

/// An SSH port forward being set up: ssh started, listening here on `port` once logged in.
struct Forward {
    child: tokio::process::Child,
    port: u16,
    /// ssh's error output, for when it fails.
    stderr: Arc<Mutex<String>>,
}

/// Starts ssh forwarding a free local port to the server (`target.host`, `target.port`, as seen from
/// the SSH host). Must be called within the runtime.
fn start_forward(target: &Target, launch: &crate::ssh::Launch) -> Result<Forward, String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(|e| e.to_string())?.port();
    let host = if target.host.contains(':') { format!("[{}]", target.host) } else { target.host.clone() };
    let mut args = launch.args.clone();
    // The command ends with "--", host.
    let at = args.len().saturating_sub(2);
    args.splice(at..at, ["-L".to_owned(), format!("127.0.0.1:{port}:{host}:{}", target.port)]);
    let mut cmd = tokio::process::Command::new(&launch.program);
    cmd.args(&args).envs(launch.env.iter().cloned()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| format!("ssh : {e}"))?;
    let stderr = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let words = stderr.clone();
        runtime().spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = [0u8; 2048];
            while let Ok(n) = err.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let mut w = words.lock().unwrap();
                w.push_str(&String::from_utf8_lossy(&buf[..n]));
                if w.len() > 4096 {
                    let cut = (w.len() - 4096..w.len()).find(|&i| w.is_char_boundary(i)).unwrap_or(0);
                    w.drain(..cut);
                }
            }
        });
    }
    Ok(Forward { child, port, stderr })
}

impl Forward {
    /// Waits until the forward accepts connections, or ssh gives up.
    async fn ready(&mut self) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if let Ok(Some(_)) = self.child.try_wait() {
                tokio::time::sleep(Duration::from_millis(200)).await;
                let words = self.stderr.lock().unwrap().trim().to_owned();
                return Err(if words.is_empty() { "ssh ended".into() } else { format!("ssh : {words}") });
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await.is_ok() {
                return Ok(());
            }
            if start.elapsed() > TUNNEL_WAIT {
                return Err("ssh : timeout".into());
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}

/// Where to connect once the forward (if any) is up.
fn direct(target: &Target, forward: Option<&Forward>) -> Target {
    match forward {
        Some(f) => Target { host: "127.0.0.1".into(), port: f.port, tunnel: None, ..target.clone() },
        None => target.clone(),
    }
}

/// Tries to connect: the server's version, or why not; and the ssh process of the forward, if any.
pub fn test(ctx: &egui::Context, target: Target) -> (Receiver<Result<String, String>>, Option<u32>) {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    let _guard = runtime().enter();
    let forward = match target.tunnel.as_ref().map(|l| start_forward(&target, l)).transpose() {
        Ok(f) => f,
        Err(e) => {
            let _ = tx.send(Err(e));
            return (rx, None);
        }
    };
    let pid = forward.as_ref().and_then(|f| f.child.id());
    runtime().spawn(async move {
        let mut forward = forward;
        let result = async {
            if let Some(f) = forward.as_mut() {
                f.ready().await?;
            }
            let mut conn = connect(&direct(&target, forward.as_ref())).await?;
            let v = version(&mut conn).await;
            let _ = conn.disconnect().await;
            Ok(v)
        }
        .await;
        let _ = tx.send(result);
        ctx.request_repaint();
        drop(forward);
    });
    (rx, pid)
}

async fn version(conn: &mut Conn) -> String {
    conn.query_first::<String, _>("SELECT VERSION()").await.ok().flatten().unwrap_or_default()
}

/// `name` quoted as an identifier.
pub fn ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// `text` as a string literal.
pub fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

/// An account, as `'user'@'host'`.
pub fn account(user: &str, host: &str) -> String {
    format!("{}@{}", literal(user), literal(host))
}

fn cell(value: Value) -> Cell {
    match value {
        Value::NULL => Cell::Null,
        Value::Bytes(b) => match String::from_utf8(b) {
            Ok(s) => Cell::Text(s),
            Err(e) => Cell::Bytes(e.into_bytes().len()),
        },
        Value::Int(i) => Cell::Text(i.to_string()),
        Value::UInt(u) => Cell::Text(u.to_string()),
        Value::Float(f) => Cell::Text(f.to_string()),
        Value::Double(d) => Cell::Text(d.to_string()),
        Value::Date(y, mo, d, h, mi, s, us) => Cell::Text(if us > 0 { format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}") } else { format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}") }),
        Value::Time(neg, d, h, mi, s, _) => Cell::Text(format!("{}{:02}:{mi:02}:{s:02}", if neg { "-" } else { "" }, d * 24 + h as u32)),
    }
}

/// Column `i` of `row` as text; NULL (or missing) is empty.
fn text(row: &Row, i: usize) -> String {
    match row.as_ref(i).map(|v| cell(v.clone())) {
        Some(Cell::Text(t)) => t,
        Some(c @ Cell::Bytes(_)) => c.text(),
        Some(Cell::Null) | None => String::new(),
    }
}

fn number(row: &Row, i: usize) -> u64 {
    text(row, i).parse().unwrap_or(0)
}

/// Runs `sql`, keeping up to `max` rows of each of its results.
async fn run(conn: &mut Conn, sql: &str, max: usize) -> mysql_async::Result<Vec<QueryResult>> {
    let mut out = Vec::new();
    let mut result = conn.query_iter(sql).await?;
    while !result.is_empty() {
        let columns: Vec<String> = result.columns_ref().iter().map(|c| c.name_str().into_owned()).collect();
        let mut rows = Vec::new();
        let mut truncated = false;
        result
            .for_each(|row: Row| {
                if rows.len() < max {
                    rows.push(row.unwrap().into_iter().map(cell).collect());
                } else {
                    truncated = true;
                }
            })
            .await?;
        out.push(QueryResult { columns, rows, truncated, affected: result.affected_rows() });
    }
    Ok(out)
}

/// A value for an INSERT: NULL, text quoted, binary data in hexadecimal.
fn sql_value(value: &Value) -> String {
    match value {
        Value::NULL => "NULL".into(),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) if !s.contains('\0') => literal(s),
            _ => {
                let mut hex = String::with_capacity(3 + b.len() * 2);
                hex.push_str("X'");
                for x in b {
                    hex.push_str(&format!("{x:02X}"));
                }
                hex.push('\'');
                hex
            }
        },
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(d) => d.to_string(),
        other => literal(&cell(other.clone()).text()),
    }
}

/// `CREATE ... DEFINER=`user`@`host` ...` without its definer: created as whoever imports it (the
/// account may not exist on another server).
fn strip_definer(sql: &str) -> String {
    let Some(i) = sql.find("DEFINER=") else { return sql.to_owned() };
    let rest = &sql[i + "DEFINER=".len()..];
    let mut quote = None;
    let mut end = rest.len();
    for (k, c) in rest.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if matches!(c, '`' | '\'' | '"') => quote = Some(c),
            None if c.is_whitespace() => {
                end = k;
                break;
            }
            None => {}
        }
    }
    format!("{}{}", &sql[..i], rest[end..].trim_start())
}

/// Where an export writes: a file, compressed for .gz.
fn export_writer(path: &std::path::Path) -> Result<Box<dyn std::io::Write + Send>, String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let gz = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gz"));
    Ok(if gz { Box::new(flate2::write::GzEncoder::new(std::io::BufWriter::new(file), flate2::Compression::default())) } else { Box::new(std::io::BufWriter::with_capacity(1 << 20, file)) })
}

/// Writes `db` (or its `only` tables) as SQL: each table (dropped first, then created) and its rows, the
/// views, then for a whole database its routines, triggers and events. Read within one consistent
/// snapshot: what is written while it runs doesn't mix in.
async fn export(conn: &mut Conn, db: &str, only: Option<&[String]>, path: &std::path::Path, check_fk: bool, emit: &Emitter, cancel: &AtomicBool) -> Result<String, String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let _ = conn.query_drop("SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ").await;
    conn.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT").await.map_err(|e| err(&e))?;
    let mut out = export_writer(path)?;
    let tables: Vec<(String, String)> = conn
        .query(format!("SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_TYPE, TABLE_NAME", literal(db)))
        .await
        .map_err(|e| err(&e))?;
    let wanted = |name: &str| only.is_none_or(|o| o.iter().any(|t| t == name));
    let (mut base, views): (Vec<_>, Vec<_>) = tables.into_iter().filter(|(n, _)| wanted(n)).partition(|(_, kind)| kind != "VIEW");
    writeln!(out, "-- Ronnie {} : export de {db}\n-- {}\n", crate::update::VERSION, chrono::Local::now().format("%Y-%m-%d %H:%M")).map_err(|e| err(&e))?;
    if check_fk {
        // Referenced tables first; all of them dropped up front, children before parents.
        let links: Vec<(String, String)> = conn
            .query(format!("SELECT TABLE_NAME, REFERENCED_TABLE_NAME FROM information_schema.REFERENTIAL_CONSTRAINTS WHERE CONSTRAINT_SCHEMA = {0} AND UNIQUE_CONSTRAINT_SCHEMA = {0}", literal(db)))
            .await
            .map_err(|e| err(&e))?;
        let names: Vec<String> = base.iter().map(|(n, _)| n.clone()).collect();
        let order = dependency_order(&names, &links);
        base.sort_by_key(|(n, _)| order.iter().position(|o| o == n));
        writeln!(out, "SET NAMES utf8mb4;\nSET FOREIGN_KEY_CHECKS = 1;\nSET SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO';\n").map_err(|e| err(&e))?;
        for (view, _) in &views {
            writeln!(out, "DROP VIEW IF EXISTS {};", ident(view)).map_err(|e| err(&e))?;
        }
        for (table, _) in base.iter().rev() {
            writeln!(out, "DROP TABLE IF EXISTS {};", ident(table)).map_err(|e| err(&e))?;
        }
        writeln!(out).map_err(|e| err(&e))?;
    } else {
        writeln!(out, "SET NAMES utf8mb4;\nSET FOREIGN_KEY_CHECKS = 0;\nSET SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO';\n").map_err(|e| err(&e))?;
    }
    let total = base.len().max(1);
    let mut rows_written = 0u64;
    for (k, (table, _)) in base.iter().enumerate() {
        emit.send(Event::Progress { fraction: k as f32 / total as f32, text: table.clone() });
        let q = format!("{}.{}", ident(db), ident(table));
        let create: Option<(String, String)> = conn.query_first(format!("SHOW CREATE TABLE {q}")).await.map_err(|e| err(&e))?;
        let Some((_, create)) = create else { continue };
        if check_fk {
            writeln!(out, "-- {table}\n{create};\n").map_err(|e| err(&e))?;
        } else {
            writeln!(out, "-- {table}\nDROP TABLE IF EXISTS {};\n{create};\n", ident(table)).map_err(|e| err(&e))?;
        }
        // Rows, 100 per INSERT, streamed (a big table never sits in memory).
        let mut result = conn.query_iter(format!("SELECT * FROM {q}")).await.map_err(|e| err(&e))?;
        let mut batch: Vec<String> = Vec::with_capacity(100);
        let mut failed = None;
        while let Some(row) = result.next().await.map_err(|e| err(&e))? {
            let values: Vec<String> = row.unwrap().iter().map(sql_value).collect();
            batch.push(format!("({})", values.join(", ")));
            rows_written += 1;
            if batch.len() == 100 {
                if cancel.load(Ordering::Relaxed) {
                    failed = Some(CANCELLED.to_owned());
                    break;
                }
                if let Err(e) = writeln!(out, "INSERT INTO {} VALUES\n{};", ident(table), batch.join(",\n")) {
                    failed = Some(e.to_string());
                    break;
                }
                batch.clear();
            }
        }
        drop(result);
        if let Some(e) = failed {
            return Err(e);
        }
        if !batch.is_empty() {
            writeln!(out, "INSERT INTO {} VALUES\n{};", ident(table), batch.join(",\n")).map_err(|e| err(&e))?;
        }
        writeln!(out).map_err(|e| err(&e))?;
    }
    // Views, those used by others first.
    let mut creates: Vec<(String, String)> = Vec::new();
    for (view, _) in &views {
        let create: Option<Row> = conn.query_first(format!("SHOW CREATE VIEW {}.{}", ident(db), ident(view))).await.map_err(|e| err(&e))?;
        if let Some(row) = create {
            creates.push((view.clone(), strip_definer(&text(&row, 1))));
        }
    }
    let names: Vec<String> = creates.iter().map(|(n, _)| n.clone()).collect();
    let links: Vec<(String, String)> = creates.iter().flat_map(|(v, sql)| names.iter().filter(|o| *o != v && sql.contains(&ident(o))).map(|o| (v.clone(), o.clone())).collect::<Vec<_>>()).collect();
    for name in dependency_order(&names, &links) {
        let Some((_, sql)) = creates.iter().find(|(n, _)| *n == name) else { continue };
        writeln!(out, "-- {name}\nDROP VIEW IF EXISTS {};\n{sql};\n", ident(&name)).map_err(|e| err(&e))?;
    }
    // Procedures and functions, then triggers (after the rows: they would fire on them), then events.
    let mut others = (0, 0, 0);
    let block = |out: &mut Box<dyn std::io::Write + Send>, drop: String, create: &str| writeln!(out, "{drop};\nDELIMITER ;;\n{};;\nDELIMITER ;\n", strip_definer(create)).map_err(|e| e.to_string());
    if only.is_none() {
        let routines: Vec<(String, String)> = conn.query(format!("SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = {}", literal(db))).await.unwrap_or_default();
        for (name, kind) in routines {
            let kind = if kind.eq_ignore_ascii_case("FUNCTION") { "FUNCTION" } else { "PROCEDURE" };
            let row: Option<Row> = conn.query_first(format!("SHOW CREATE {kind} {}.{}", ident(db), ident(&name))).await.unwrap_or_default();
            let create = row.map(|r| text(&r, 2)).unwrap_or_default();
            if !create.is_empty() {
                block(&mut out, format!("DROP {kind} IF EXISTS {}", ident(&name)), &create)?;
                others.0 += 1;
            }
        }
    }
    let triggers: Vec<(String, String)> = conn.query(format!("SELECT TRIGGER_NAME, EVENT_OBJECT_TABLE FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = {}", literal(db))).await.unwrap_or_default();
    for (name, _) in triggers.into_iter().filter(|(_, t)| wanted(t)) {
        let row: Option<Row> = conn.query_first(format!("SHOW CREATE TRIGGER {}.{}", ident(db), ident(&name))).await.unwrap_or_default();
        let create = row.map(|r| text(&r, 2)).unwrap_or_default();
        if !create.is_empty() {
            block(&mut out, format!("DROP TRIGGER IF EXISTS {}", ident(&name)), &create)?;
            others.1 += 1;
        }
    }
    if only.is_none() {
        let events: Vec<String> = conn.query(format!("SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA = {}", literal(db))).await.unwrap_or_default();
        for name in events {
            let row: Option<Row> = conn.query_first(format!("SHOW CREATE EVENT {}.{}", ident(db), ident(&name))).await.unwrap_or_default();
            let create = row.map(|r| text(&r, 3)).unwrap_or_default();
            if !create.is_empty() {
                block(&mut out, format!("DROP EVENT IF EXISTS {}", ident(&name)), &create)?;
                others.2 += 1;
            }
        }
    }
    if !check_fk {
        writeln!(out, "SET FOREIGN_KEY_CHECKS = 1;").map_err(|e| err(&e))?;
    }
    out.flush().map_err(|e| err(&e))?;
    drop(out);
    let _ = conn.query_drop("COMMIT").await;
    let mut summary = format!("{} tables, {} views, {rows_written} rows", base.len(), views.len());
    if others != (0, 0, 0) {
        summary.push_str(&format!(", {} routines, {} triggers, {} events", others.0, others.1, others.2));
    }
    Ok(summary)
}

/// A value in a CSV file: quoted when it has to be.
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// Rows as CSV: a header line, then one line per row (NULL written as such).
pub fn write_csv(out: &mut dyn std::io::Write, columns: &[String], rows: &[Vec<Cell>]) -> std::io::Result<()> {
    writeln!(out, "{}", columns.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","))?;
    for row in rows {
        writeln!(out, "{}", row.iter().map(|c| csv_field(&c.text())).collect::<Vec<_>>().join(","))?;
    }
    Ok(())
}

/// Writes the rows of a table as CSV, streamed.
async fn export_csv(conn: &mut Conn, db: &str, table: &str, path: &std::path::Path, cancel: &AtomicBool) -> Result<String, String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let mut out = export_writer(path)?;
    let mut result = conn.query_iter(format!("SELECT * FROM {}.{}", ident(db), ident(table))).await.map_err(|e| err(&e))?;
    let columns: Vec<String> = result.columns_ref().iter().map(|c| c.name_str().into_owned()).collect();
    write_csv(&mut out, &columns, &[]).map_err(|e| err(&e))?;
    let mut n = 0u64;
    let mut failed = None;
    while let Some(row) = result.next().await.map_err(|e| err(&e))? {
        let cells: Vec<Cell> = row.unwrap().into_iter().map(|v| match v {
            // Binary data in hexadecimal.
            Value::Bytes(b) if std::str::from_utf8(&b).is_err() => Cell::Text(b.iter().map(|x| format!("{x:02X}")).collect()),
            v => cell(v),
        }).collect();
        if let Err(e) = writeln!(out, "{}", cells.iter().map(|c| csv_field(&c.text())).collect::<Vec<_>>().join(",")) {
            failed = Some(e.to_string());
            break;
        }
        n += 1;
        if n.is_multiple_of(1000) && cancel.load(Ordering::Relaxed) {
            failed = Some(CANCELLED.to_owned());
            break;
        }
    }
    drop(result);
    if let Some(e) = failed {
        return Err(e);
    }
    out.flush().map_err(|e| err(&e))?;
    Ok(format!("{n} rows"))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum SplitState {
    Normal,
    Quote(char),
    LineComment,
    /// A /* comment */; true: /*! ... */, kept (instructions for MySQL).
    Block(bool),
}

/// Cuts a SQL script into statements, at ";" (or the DELIMITER set) outside quotes and comments, as the
/// text comes (a big dump never has to sit in memory whole).
pub struct Splitter {
    buf: String,
    pos: usize,
    current: String,
    delimiter: String,
    state: SplitState,
    line_start: bool,
}

impl Default for Splitter {
    fn default() -> Self {
        Self { buf: String::new(), pos: 0, current: String::new(), delimiter: ";".into(), state: SplitState::Normal, line_start: true }
    }
}

impl Splitter {
    pub fn push(&mut self, text: &str) {
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.push_str(text);
    }

    fn take(&mut self) -> Option<String> {
        let statement = self.current.trim().to_owned();
        self.current.clear();
        (!statement.is_empty()).then_some(statement)
    }

    /// The next complete statement, if the text so far has one; `eof`: no more text will come (what is
    /// left is a statement too).
    pub fn next(&mut self, eof: bool) -> Option<String> {
        loop {
            let rest = &self.buf[self.pos..];
            let mut chars = rest.chars();
            let Some(c) = chars.next() else {
                return if eof { self.take() } else { None };
            };
            let next = chars.next();
            let len = c.len_utf8();
            // More text needed to tell what this is.
            let wait = !eof;
            match self.state {
                SplitState::Quote(q) => {
                    if c == '\\' && q != '`' {
                        match next {
                            Some(n) => {
                                self.current.push(c);
                                self.current.push(n);
                                self.pos += len + n.len_utf8();
                            }
                            None if wait => return None,
                            None => {
                                self.current.push(c);
                                self.pos += len;
                            }
                        }
                        continue;
                    }
                    self.current.push(c);
                    self.pos += len;
                    if c == q {
                        self.state = SplitState::Normal;
                    }
                    continue;
                }
                SplitState::LineComment => {
                    self.pos += len;
                    if c == '\n' {
                        self.state = SplitState::Normal;
                        self.line_start = true;
                        self.current.push('\n');
                    }
                    continue;
                }
                SplitState::Block(keep) => {
                    if c == '*' {
                        match next {
                            Some('/') => {
                                if keep {
                                    self.current.push_str("*/");
                                }
                                self.pos += 2;
                                self.state = SplitState::Normal;
                                continue;
                            }
                            None if wait => return None,
                            _ => {}
                        }
                    }
                    if keep {
                        self.current.push(c);
                    }
                    self.pos += len;
                    continue;
                }
                SplitState::Normal => {}
            }
            // "DELIMITER $$" at the start of a line (mysql client command).
            if self.line_start {
                let head: String = rest.chars().take(10).collect();
                if head.chars().count() < 10 && !rest.contains('\n') && wait && "DELIMITER ".starts_with(&head.to_uppercase()) {
                    return None;
                }
                if head.eq_ignore_ascii_case("DELIMITER ") {
                    let end = match rest.find('\n') {
                        Some(e) => e,
                        None if wait => return None,
                        None => rest.len(),
                    };
                    let d = rest[10..end].trim().to_owned();
                    self.delimiter = if d.is_empty() { ";".into() } else { d };
                    self.pos += end;
                    continue;
                }
            }
            self.line_start = c == '\n';
            match c {
                '\'' | '"' | '`' => {
                    self.state = SplitState::Quote(c);
                    self.current.push(c);
                    self.pos += len;
                    continue;
                }
                '-' => match (next, rest.chars().nth(2)) {
                    (None, _) if wait => return None,
                    (Some('-'), None) if wait => return None,
                    (Some('-'), third) if third.is_none_or(char::is_whitespace) => {
                        self.state = SplitState::LineComment;
                        self.pos += 2;
                        continue;
                    }
                    _ => {}
                },
                '#' => {
                    self.state = SplitState::LineComment;
                    self.pos += len;
                    continue;
                }
                '/' => match (next, rest.chars().nth(2)) {
                    (None, _) if wait => return None,
                    (Some('*'), None) if wait => return None,
                    (Some('*'), third) => {
                        let keep = third == Some('!');
                        if keep {
                            self.current.push_str("/*");
                        }
                        self.state = SplitState::Block(keep);
                        self.pos += 2;
                        continue;
                    }
                    _ => {}
                },
                _ => {}
            }
            if rest.starts_with(self.delimiter.as_str()) {
                self.pos += self.delimiter.len();
                if let Some(statement) = self.take() {
                    return Some(statement);
                }
                continue;
            }
            if rest.len() < self.delimiter.len() && self.delimiter.starts_with(rest) && wait {
                return None;
            }
            self.current.push(c);
            self.pos += len;
        }
    }
}

/// The statements of a whole SQL script.
#[cfg(test)]
fn split_statements(script: &str) -> Vec<String> {
    let mut splitter = Splitter::default();
    splitter.push(script);
    std::iter::from_fn(|| splitter.next(true)).collect()
}

/// `tables` sorted so that each comes after the ones it references (`links`: table, referenced).
/// Cycles and self-references can't be ordered: those tables keep their place at the end.
fn dependency_order(tables: &[String], links: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tables.len());
    let mut left: Vec<&String> = tables.iter().collect();
    loop {
        let before = left.len();
        left.retain(|t| {
            let ready = links.iter().filter(|(from, to)| from == *t && to != *t && tables.contains(to)).all(|(_, to)| out.contains(to));
            if ready {
                out.push((*t).clone());
            }
            !ready
        });
        if left.is_empty() || left.len() == before {
            break;
        }
    }
    out.extend(left.into_iter().cloned());
    out
}

/// Text from bytes read in pieces: UTF-8, or Latin-1 once something isn't (older dumps).
#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
    latin1: bool,
}

impl Decoder {
    fn decode(&mut self, bytes: &[u8], eof: bool) -> String {
        self.pending.extend_from_slice(bytes);
        if !self.latin1 {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    let s = s.to_owned();
                    self.pending.clear();
                    return s;
                }
                // A character cut between two pieces: its end comes with the next one.
                Err(e) if e.error_len().is_none() && !eof => {
                    let valid = e.valid_up_to();
                    let s = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
                    self.pending.drain(..valid);
                    return s;
                }
                Err(_) => self.latin1 = true,
            }
        }
        let s = self.pending.iter().map(|&b| b as char).collect();
        self.pending.clear();
        s
    }
}

/// Counts the bytes read from the file (compressed ones for a .gz), for the progress.
struct Counting<R> {
    inner: R,
    read: Arc<AtomicU64>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Runs a .sql (or .sql.gz) file, statement by statement as it is read; stops at the first error.
async fn import(conn: &mut Conn, db: Option<&str>, path: &std::path::Path, check_fk: bool, emit: &Emitter, cancel: &AtomicBool) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0).max(1);
    let read = Arc::new(AtomicU64::new(0));
    let counting = Counting { inner: file, read: read.clone() };
    let gz = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gz"));
    let mut reader: Box<dyn Read + Send> = if gz { Box::new(flate2::read::MultiGzDecoder::new(std::io::BufReader::new(counting))) } else { Box::new(counting) };
    conn.query_drop(format!("SET FOREIGN_KEY_CHECKS = {}", u8::from(check_fk))).await.map_err(|e| e.to_string())?;
    if let Some(db) = db.filter(|d| !d.is_empty()) {
        conn.query_drop(format!("USE {}", ident(db))).await.map_err(|e| e.to_string())?;
    }
    let mut splitter = Splitter::default();
    let mut decoder = Decoder::default();
    let mut buf = vec![0u8; 1 << 20];
    let (mut count, mut eof) = (0usize, false);
    let mut last = Instant::now();
    loop {
        while let Some(statement) = splitter.next(eof) {
            if cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.to_owned());
            }
            count += 1;
            if last.elapsed() > Duration::from_millis(150) {
                last = Instant::now();
                let done = read.load(Ordering::Relaxed);
                emit.send(Event::Progress { fraction: (done as f32 / size as f32).min(1.0), text: format!("{count}") });
            }
            if let Err(e) = conn.query_drop(statement.as_str()).await {
                let head: String = statement.chars().take(120).collect();
                return Err(format!("#{count} : {e}\n{head}"));
            }
        }
        if eof {
            break;
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        eof = n == 0;
        splitter.push(&decoder.decode(&buf[..n], eof));
    }
    Ok(format!("{count} statements"))
}

/// A connection used for one export or import, beside the view's.
async fn transfer(direct: Target, request: Request, emit: Emitter, thread: Arc<AtomicU32>, cancel: Arc<AtomicBool>) {
    let result = async {
        let mut conn = connect(&direct).await?;
        thread.store(conn.id(), Ordering::Relaxed);
        let result = match &request {
            Request::Export { db, tables, path, check_fk } => export(&mut conn, db, tables.as_deref(), path, *check_fk, &emit, &cancel).await,
            Request::ExportCsv { db, table, path } => export_csv(&mut conn, db, table, path, &cancel).await,
            Request::Import { db, path, check_fk } => import(&mut conn, db.as_deref(), path, *check_fk, &emit, &cancel).await,
            _ => Ok(String::new()),
        };
        thread.store(0, Ordering::Relaxed);
        let _ = conn.disconnect().await;
        result
    }
    .await;
    // Stopped: whatever the server said about the query killed.
    let result = if cancel.load(Ordering::Relaxed) && result.is_err() { Err(CANCELLED.to_owned()) } else { result };
    // Written to disk: the partial file of a failed export is removed.
    if let (Err(_), Request::Export { path, .. } | Request::ExportCsv { path, .. }) = (&result, &request) {
        let _ = std::fs::remove_file(path);
    }
    let imported = matches!(request, Request::Import { .. });
    emit.send(Event::Transfer(result));
    if imported {
        emit.send(Event::Done { affected: 0, tag: 0 });
    }
}

async fn handle(conn: &mut Conn, request: Request, emit: &Emitter) -> mysql_async::Result<()> {
    match request {
        Request::Databases => {
            let rows: Vec<Row> = conn
                .query(
                    "SELECT s.SCHEMA_NAME, s.DEFAULT_COLLATION_NAME, COUNT(t.TABLE_NAME), COALESCE(SUM(t.DATA_LENGTH + t.INDEX_LENGTH), 0) \
                     FROM information_schema.SCHEMATA s LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = s.SCHEMA_NAME \
                     GROUP BY s.SCHEMA_NAME, s.DEFAULT_COLLATION_NAME ORDER BY s.SCHEMA_NAME",
                )
                .await?;
            let dbs = rows.iter().map(|r| DatabaseInfo { name: text(r, 0), collation: text(r, 1), tables: number(r, 2), size: number(r, 3) }).collect();
            emit.send(Event::Databases(dbs));
        }
        Request::Tables(db) => {
            let sql = format!(
                "SELECT TABLE_NAME, TABLE_TYPE, COALESCE(ENGINE, ''), COALESCE(TABLE_ROWS, 0), COALESCE(DATA_LENGTH + INDEX_LENGTH, 0), COALESCE(TABLE_COLLATION, ''), COALESCE(TABLE_COMMENT, '') \
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
                literal(&db)
            );
            let rows: Vec<Row> = conn.query(sql).await?;
            let tables = rows
                .iter()
                .map(|r| TableInfo { name: text(r, 0), view: text(r, 1) == "VIEW", engine: text(r, 2), rows: number(r, 3), size: number(r, 4), collation: text(r, 5), comment: text(r, 6) })
                .collect();
            emit.send(Event::Tables { db, tables });
        }
        Request::Structure { db, table } => {
            let columns = run(
                conn,
                &format!(
                    "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT, CHARACTER_SET_NAME, COLLATION_NAME FROM information_schema.COLUMNS \
                     WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} ORDER BY ORDINAL_POSITION",
                    literal(&db),
                    literal(&table)
                ),
                10_000,
            )
            .await?
            .into_iter()
            .next()
            .map(|r| r.rows)
            .unwrap_or_default();
            // Indexes, one line each with its columns.
            let index_rows: Vec<Row> = conn.query(format!("SHOW INDEX FROM {}.{}", ident(&db), ident(&table))).await.unwrap_or_default();
            let mut indexes: Vec<Vec<Cell>> = Vec::new();
            for r in &index_rows {
                let (name, column, unique, kind) = (text(r, 2), text(r, 4), text(r, 1) == "0", text(r, 10));
                match indexes.iter_mut().find(|i| i[0] == Cell::Text(name.clone())) {
                    Some(i) => {
                        if let Cell::Text(cols) = &mut i[1] {
                            cols.push_str(", ");
                            cols.push_str(&column);
                        }
                    }
                    None => indexes.push(vec![Cell::Text(name), Cell::Text(column), Cell::Text(if unique { "UNIQUE".into() } else { String::new() }), Cell::Text(kind)]),
                }
            }
            let create: Option<Row> = conn.query_first(format!("SHOW CREATE TABLE {}.{}", ident(&db), ident(&table))).await.unwrap_or_default();
            let create = create.map(|r| text(&r, 1)).unwrap_or_default();
            emit.send(Event::Structure { db, table, structure: Structure { columns, indexes, create } });
        }
        Request::Rows { db, table, offset, limit, order, search } => {
            let order_by = match order {
                Some((c, asc)) => format!(" ORDER BY {} {}", ident(&c), if asc { "ASC" } else { "DESC" }),
                None => {
                    let keys: Vec<String> = conn
                        .query(format!(
                            "SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} AND CONSTRAINT_NAME = 'PRIMARY' ORDER BY ORDINAL_POSITION",
                            literal(&db),
                            literal(&table)
                        ))
                        .await
                        .unwrap_or_default();
                    if keys.is_empty() { String::new() } else { format!(" ORDER BY {}", keys.iter().map(|k| ident(k)).collect::<Vec<_>>().join(", ")) }
                }
            };
            // Searched in every column, as text (% and _ typed are looked for as such).
            let filter = search.as_ref().filter(|(text, columns)| !text.is_empty() && !columns.is_empty()).map(|(text, columns)| {
                let pattern = literal(&format!("%{}%", text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
                let any: Vec<String> = columns.iter().map(|c| format!("CAST({} AS CHAR) LIKE {pattern}", ident(c))).collect();
                format!(" WHERE {}", any.join(" OR "))
            });
            let filter_sql = filter.clone().unwrap_or_default();
            let sql = format!("SELECT * FROM {}.{}{filter_sql}{order_by} LIMIT {offset}, {limit}", ident(&db), ident(&table));
            let result = run(conn, &sql, limit as usize).await?.into_iter().next().unwrap_or_default();
            // Exact for most tables; the estimate for huge ones.
            let estimate: Option<u64> = conn
                .query_first(format!("SELECT COALESCE(TABLE_ROWS, 0) FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}", literal(&db), literal(&table)))
                .await
                .unwrap_or_default();
            let estimate = estimate.unwrap_or(0);
            let (total, exact) = if estimate <= EXACT_COUNT_MAX || filter.is_some() {
                let n: Option<u64> = conn.query_first(format!("SELECT COUNT(*) FROM {}.{}{filter_sql}", ident(&db), ident(&table))).await.unwrap_or_default();
                (n.unwrap_or(estimate), true)
            } else {
                (estimate, false)
            };
            emit.send(Event::Rows { db, table, offset, result, total, exact });
        }
        Request::Query { db, sql } => {
            if let Some(db) = db.filter(|d| !d.is_empty()) {
                conn.query_drop(format!("USE {}", ident(&db))).await?;
            }
            let start = Instant::now();
            match run(conn, &sql, MAX_QUERY_ROWS).await {
                Ok(results) => emit.send(Event::Query { results, elapsed: start.elapsed(), error: None }),
                Err(e) if is_lost(&e) => return Err(e),
                Err(e) => emit.send(Event::Query { results: Vec::new(), elapsed: start.elapsed(), error: Some(e.to_string()) }),
            }
        }
        Request::Exec { db, sql, tag } => {
            if let Some(db) = db.filter(|d| !d.is_empty()) {
                conn.query_drop(format!("USE {}", ident(&db))).await?;
            }
            match conn.query_drop(sql).await {
                Ok(()) => emit.send(Event::Done { affected: conn.affected_rows(), tag }),
                Err(e) if is_lost(&e) => return Err(e),
                Err(e) => emit.send(Event::Failed { error: e.to_string(), tag }),
            }
        }
        Request::Users => {
            let users: Vec<(String, String)> = conn.query("SELECT User, Host FROM mysql.user ORDER BY User, Host").await?;
            emit.send(Event::Users(users.into_iter().map(|(user, host)| UserInfo { user, host }).collect()));
        }
        Request::Grants { user, host } => {
            let grants: Vec<String> = conn.query(format!("SHOW GRANTS FOR {}", account(&user, &host))).await?;
            emit.send(Event::Grants { user, host, grants });
        }
        // Handled on a connection of their own.
        Request::Export { .. } | Request::ExportCsv { .. } | Request::Import { .. } => {}
    }
    Ok(())
}

/// The connection is gone (network, server restarted, idle timeout).
fn is_lost(e: &mysql_async::Error) -> bool {
    matches!(e, mysql_async::Error::Io(_) | mysql_async::Error::Driver(mysql_async::DriverError::ConnectionClosed))
}

impl Connection {
    pub fn open(ctx: &egui::Context, target: Target) -> Self {
        let (requests, mut incoming) = tmpsc::unbounded_channel::<Request>();
        let (tx, events) = mpsc::channel();
        let emit = Emitter { tx, ctx: ctx.clone() };
        let stop = Arc::new(tokio::sync::Notify::new());
        let stop_tunnel = Arc::new(tokio::sync::Notify::new());
        let thread = Arc::new(AtomicU32::new(0));
        let _guard = runtime().enter();
        let forward = target.tunnel.as_ref().map(|l| start_forward(&target, l));
        let (forward, failed) = match forward {
            Some(Ok(f)) => (Some(f), None),
            Some(Err(e)) => (None, Some(e)),
            None => (None, None),
        };
        let tunnel_pid = forward.as_ref().and_then(|f| f.child.id());
        let direct = direct(&target, forward.as_ref());
        let (stopped, tunnel_stopped, conn_thread, conn_target) = (stop.clone(), stop_tunnel.clone(), thread.clone(), direct.clone());
        let transfer_thread = Arc::new(AtomicU32::new(0));
        let cancel_transfer = Arc::new(AtomicBool::new(false));
        let (t_thread, t_cancel) = (transfer_thread.clone(), cancel_transfer.clone());
        runtime().spawn(async move {
            if let Some(e) = failed {
                return emit.send(Event::Closed(e));
            }
            // The forward first (the user may be typing a password), kept open until the view closes.
            let (ssh_ended_tx, mut ssh_ended) = tokio::sync::oneshot::channel::<String>();
            let mut tunnel_up = forward.is_some();
            if let Some(mut f) = forward {
                if let Err(e) = f.ready().await {
                    return emit.send(Event::Closed(e));
                }
                runtime().spawn(async move {
                    tokio::select! {
                        _ = f.child.wait() => {
                            let words = f.stderr.lock().unwrap().trim().to_owned();
                            let _ = ssh_ended_tx.send(if words.is_empty() { "ssh ended".into() } else { format!("ssh : {words}") });
                        }
                        _ = tunnel_stopped.notified() => {
                            let _ = f.child.kill().await;
                        }
                    }
                });
            }
            let mut conn = match connect(&conn_target).await {
                Ok(conn) => conn,
                Err(e) => return emit.send(Event::Closed(e)),
            };
            conn_thread.store(conn.id(), Ordering::Relaxed);
            let v = version(&mut conn).await;
            emit.send(Event::Connected { version: v });
            loop {
                tokio::select! {
                    request = incoming.recv() => {
                        let Some(request) = request else { break };
                        if matches!(request, Request::Export { .. } | Request::ExportCsv { .. } | Request::Import { .. }) {
                            t_cancel.store(false, Ordering::Relaxed);
                            runtime().spawn(transfer(conn_target.clone(), request, emit.clone(), t_thread.clone(), t_cancel.clone()));
                            continue;
                        }
                        let mut outcome = handle(&mut conn, request.clone(), &emit).await;
                        // Lost (the server's idle timeout, a restart): connected again, and asked again once.
                        if outcome.as_ref().is_err_and(is_lost) {
                            if let Ok(fresh) = connect(&conn_target).await {
                                conn = fresh;
                                conn_thread.store(conn.id(), Ordering::Relaxed);
                                outcome = handle(&mut conn, request, &emit).await;
                            }
                        }
                        if let Err(e) = outcome {
                            if is_lost(&e) {
                                emit.send(Event::Closed(e.to_string()));
                                return;
                            }
                            emit.send(Event::Error(e.to_string()));
                        }
                    }
                    words = &mut ssh_ended, if tunnel_up => {
                        tunnel_up = false;
                        if let Ok(words) = words {
                            emit.send(Event::Closed(words));
                            return;
                        }
                    }
                    _ = stopped.notified() => break,
                }
            }
            let _ = conn.disconnect().await;
        });
        Self { requests, events, stop, stop_tunnel, direct, thread, transfer_thread, cancel_transfer, tunnel_pid }
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Stops the query the view's connection runs (from another connection: that one is busy).
    pub fn cancel_query(&self) {
        kill(self.direct.clone(), self.thread.load(Ordering::Relaxed));
    }

    /// Stops the export or import going on.
    pub fn cancel_transfer(&self) {
        self.cancel_transfer.store(true, Ordering::Relaxed);
        kill(self.direct.clone(), self.transfer_thread.load(Ordering::Relaxed));
    }
}

/// KILL QUERY `id`, from a connection of its own.
fn kill(direct: Target, id: u32) {
    if id == 0 {
        return;
    }
    runtime().spawn(async move {
        if let Ok(mut conn) = connect(&direct).await {
            let _ = conn.query_drop(format!("KILL QUERY {id}")).await;
            let _ = conn.disconnect().await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_names_and_strings() {
        assert_eq!(ident("a`b"), "`a``b`");
        assert_eq!(literal("it's \\ ok"), "'it''s \\\\ ok'");
        assert_eq!(cell(Value::NULL), Cell::Null);
        assert_eq!(cell(Value::Bytes(b"abc".to_vec())), Cell::Text("abc".into()));
        assert_eq!(cell(Value::Bytes(vec![0xff, 0xfe])), Cell::Bytes(2));
    }

    #[test]
    fn orders_tables_by_foreign_keys() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let l = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        let tables = s(&["administrators", "company", "roles", "tree"]);
        let links = l(&[("administrators", "company"), ("administrators", "roles"), ("roles", "company"), ("tree", "tree")]);
        assert_eq!(dependency_order(&tables, &links), s(&["company", "roles", "tree", "administrators"]));
        // A cycle keeps its tables, at the end.
        let links = l(&[("company", "roles"), ("roles", "company")]);
        assert_eq!(dependency_order(&tables, &links), s(&["administrators", "tree", "company", "roles"]));
    }

    #[test]
    fn splits_scripts_read_in_pieces() {
        let script = "SET a = 'x;\\'y';\n-- note\nINSERT INTO t VALUES ('é;è'), (\"z\");\n/*!40101 SET NAMES utf8 */;\nDELIMITER ;;\nCREATE TRIGGER x BEFORE INSERT ON t FOR EACH ROW BEGIN SET @a = 1; END;;\nDELIMITER ;\nSELECT 1 -- end";
        let whole = split_statements(script);
        assert_eq!(whole.len(), 5, "{whole:#?}");
        assert_eq!(whole[3], "CREATE TRIGGER x BEFORE INSERT ON t FOR EACH ROW BEGIN SET @a = 1; END");
        // Any cut gives the same statements.
        for size in 1..12 {
            let mut splitter = Splitter::default();
            let mut out = Vec::new();
            let chars: Vec<char> = script.chars().collect();
            for piece in chars.chunks(size) {
                splitter.push(&piece.iter().collect::<String>());
                while let Some(s) = splitter.next(false) {
                    out.push(s);
                }
            }
            while let Some(s) = splitter.next(true) {
                out.push(s);
            }
            assert_eq!(out, whole, "pieces of {size}");
        }
    }

    #[test]
    fn decodes_utf8_cut_and_latin1() {
        let mut d = Decoder::default();
        let bytes = "été".as_bytes();
        assert_eq!(d.decode(&bytes[..1], false), "");
        assert_eq!(d.decode(&bytes[1..], true), "été");
        let mut d = Decoder::default();
        assert_eq!(d.decode(&[b'c', 0xE9], true), "cé");
    }

    #[test]
    fn removes_definers_and_writes_csv() {
        assert_eq!(strip_definer("CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`%` SQL SECURITY DEFINER VIEW `v` AS select 1"), "CREATE ALGORITHM=UNDEFINED SQL SECURITY DEFINER VIEW `v` AS select 1");
        assert_eq!(strip_definer("CREATE TABLE t (a int)"), "CREATE TABLE t (a int)");
        let mut out = Vec::new();
        write_csv(&mut out, &["a".into(), "b".into()], &[vec![Cell::Text("x,y".into()), Cell::Null], vec![Cell::Text("say \"hi\"".into()), Cell::Text("1".into())]]).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "a,b\n\"x,y\",NULL\n\"say \"\"hi\"\"\",1\n");
    }

    #[test]
    fn splits_sql_scripts() {
        let script = "-- a comment; with ;\nCREATE TABLE t (a TEXT);\nINSERT INTO t VALUES ('x;y', \"z\\\";\"), ('it''s');\n/*!40101 SET NAMES utf8 */;\n/* gone; */\nDELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3";
        let s = split_statements(script);
        assert_eq!(s.len(), 5, "{s:#?}");
        assert_eq!(s[1], "INSERT INTO t VALUES ('x;y', \"z\\\";\"), ('it''s')");
        assert_eq!(s[2], "/*!40101 SET NAMES utf8 */");
        assert_eq!(s[3], "CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END");
        assert_eq!(s[4], "SELECT 3");
    }

    /// Against the local server, when there is one (cargo test -- --ignored local_server).
    #[test]
    #[ignore]
    fn local_server() {
        let target = Target { host: "localhost".into(), port: 3306, user: std::env::var("USER").unwrap_or_default(), password: None, database: None, tunnel: None };
        let ctx = egui::Context::default();
        let (rx, _) = test(&ctx, target.clone());
        let version = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        assert!(!version.is_empty());
        let conn = Connection::open(&ctx, target);
        conn.send(Request::Databases);
        conn.send(Request::Query { db: None, sql: "SELECT 1 AS un, NULL AS rien; SELECT 'deux'".into() });
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut dbs, mut query) = (None, None);
        while (dbs.is_none() || query.is_none()) && Instant::now() < deadline {
            for e in conn.poll() {
                match e {
                    Event::Databases(d) => dbs = Some(d),
                    Event::Query { results, error, .. } => query = Some((results, error)),
                    Event::Closed(e) | Event::Error(e) => panic!("{e}"),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let dbs = dbs.expect("databases");
        assert!(dbs.iter().any(|d| d.name == "information_schema"));
        let (results, error) = query.expect("query");
        assert_eq!(error, None);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].columns, ["un", "rien"]);
        assert_eq!(results[0].rows[0], vec![Cell::Text("1".into()), Cell::Null]);
        assert_eq!(results[1].rows[0], vec![Cell::Text("deux".into())]);
    }

    /// Needs a local MariaDB/MySQL server (like `local_server`).
    #[test]
    #[ignore]
    fn refused_change_tells_its_tag() {
        let target = Target { host: "localhost".into(), port: 3306, user: std::env::var("USER").unwrap_or_default(), password: None, database: None, tunnel: None };
        let ctx = egui::Context::default();
        let conn = Connection::open(&ctx, target);
        conn.send(Request::Exec { db: None, sql: "UPDATE no_such_db.no_such_table SET a = 1".into(), tag: 7 });
        conn.send(Request::Exec { db: None, sql: "DO 1".into(), tag: 8 });
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut failed, mut done) = (None, None);
        while (failed.is_none() || done.is_none()) && Instant::now() < deadline {
            for e in conn.poll() {
                match e {
                    Event::Failed { tag, error } => failed = Some((tag, error)),
                    Event::Done { tag, .. } => done = Some(tag),
                    Event::Closed(e) | Event::Error(e) => panic!("{e}"),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let (tag, error) = failed.expect("failed");
        assert_eq!(tag, 7);
        assert!(!error.is_empty());
        assert_eq!(done, Some(8), "the connection goes on after a refused change");
    }
}
