//! SQL Server client for the database view, behind the same requests and events as MariaDB / MySQL
//! (db.rs): one connection per open view, in the background, and one more for an export or import.
//! Tables are listed as `schema.table`. A query running too long is stopped by an attention signal on
//! its own connection. Scripts are cut into batches on `GO` lines, as SSMS and sqlcmd do.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{FutureExt as _, TryStreamExt as _};
use tiberius::{AuthMethod, Client, ColumnData, Config, FromSql, SqlBrowser as _};
use tokio::net::TcpStream;
use tokio::sync::mpsc as tmpsc;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt as _};

use crate::config::Engine;
use crate::db::{self, Cell, Connection, DatabaseInfo, Emitter, Event, Param, QueryResult, Request, Routine, Structure, TableInfo, Target, Trigger, CANCELLED, LOST, MAX_QUERY_ROWS, RESTORED};

type Conn = Client<Compat<TcpStream>>;
type Error = tiberius::error::Error;

/// Above this estimate, a table's rows aren't counted exactly.
const EXACT_COUNT_MAX: u64 = 2_000_000;
/// What a query stopped by the user ends with.
const STOPPED: &str = "Query cancelled";

/// `name` quoted as an identifier.
pub fn ident(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

/// `text` as a Unicode string literal (backslashes are nothing special here).
pub fn literal(text: &str) -> String {
    format!("N'{}'", text.replace('\'', "''"))
}

/// A table listed as `schema.table` (no dot: in dbo).
fn split(table: &str) -> (&str, &str) {
    table.split_once('.').unwrap_or(("dbo", table))
}

/// `[schema].[table]`.
pub fn local_table(table: &str) -> String {
    let (schema, name) = split(table);
    format!("{}.{}", ident(schema), ident(name))
}

/// `[db].[schema].[table]`.
pub fn table(db: &str, table: &str) -> String {
    format!("{}.{}", ident(db), local_table(table))
}

/// What the server said, without the driver's wrapping.
fn message(e: &Error) -> String {
    match e {
        Error::Server(t) => t.message().to_owned(),
        e => e.to_string(),
    }
}

/// The connection is gone (network, server restarted).
fn is_lost(e: &Error) -> bool {
    matches!(e, Error::Io { .. })
}

async fn connect(target: &Target) -> Result<Conn, String> {
    let mut config = Config::new();
    config.host(target.machine());
    config.application_name("Ronnie");
    config.authentication(AuthMethod::sql_server(&target.user, target.password.as_deref().unwrap_or_default()));
    if let Some(d) = target.database.as_deref().filter(|d| !d.is_empty()) {
        config.database(d);
    }
    if target.trust_cert {
        config.trust_cert();
    }
    // A named instance left on the default port: the SQL Server Browser tells its port.
    let named = target.instance().filter(|_| target.port == Engine::Sqlserver.default_port());
    match named {
        Some(instance) => config.instance_name(instance),
        None => config.port(target.port),
    }
    let attempt = async {
        let tcp = match named {
            Some(_) => TcpStream::connect_named(&config).await.map_err(|e| message(&e))?,
            None => TcpStream::connect(config.get_addr()).await.map_err(|e| e.to_string())?,
        };
        let _ = tcp.set_nodelay(true);
        match Client::connect(config.clone(), tcp.compat_write()).await {
            Ok(client) => Ok(client),
            // Sent to another server (Azure, availability groups).
            Err(Error::Routing { host, port }) => {
                config.host(&host);
                config.port(port);
                let tcp = TcpStream::connect(config.get_addr()).await.map_err(|e| e.to_string())?;
                let _ = tcp.set_nodelay(true);
                Client::connect(config, tcp.compat_write()).await.map_err(|e| message(&e))
            }
            Err(e) => Err(message(&e)),
        }
    };
    match tokio::time::timeout(Duration::from_secs(15), attempt).await {
        Ok(result) => result,
        Err(_) => Err("timeout".into()),
    }
}

/// A value as shown in a grid.
fn cell(data: ColumnData<'static>) -> Cell {
    use ColumnData as D;
    fn text(v: Option<impl ToString>) -> Cell {
        v.map_or(Cell::Null, |v| Cell::Text(v.to_string()))
    }
    fn converted<T>(data: &ColumnData<'static>, show: impl Fn(T) -> String) -> Cell
    where
        T: for<'a> FromSql<'a>,
    {
        match T::from_sql(data) {
            Ok(Some(v)) => Cell::Text(show(v)),
            Ok(None) => Cell::Null,
            Err(e) => Cell::Text(e.to_string()),
        }
    }
    match &data {
        D::U8(v) => text(*v),
        D::I16(v) => text(*v),
        D::I32(v) => text(*v),
        D::I64(v) => text(*v),
        D::F32(v) => text(*v),
        D::F64(v) => text(*v),
        D::Bit(v) => text(v.map(|b| if b { "1" } else { "0" })),
        D::String(v) => text(v.as_deref()),
        D::Guid(v) => text(v.map(|g| g.to_string().to_uppercase())),
        D::Binary(v) => v.as_ref().map_or(Cell::Null, |b| Cell::Bytes(b.len())),
        D::Numeric(v) => text(*v),
        D::Xml(v) => text(v.as_ref().map(|x| x.as_ref().to_string())),
        D::DateTime(_) | D::SmallDateTime(_) | D::DateTime2(_) => converted(&data, |d: chrono::NaiveDateTime| d.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
        D::Date(_) => converted(&data, |d: chrono::NaiveDate| d.format("%Y-%m-%d").to_string()),
        D::Time(_) => converted(&data, |d: chrono::NaiveTime| d.format("%H:%M:%S%.f").to_string()),
        D::DateTimeOffset(_) => converted(&data, |d: chrono::DateTime<chrono::FixedOffset>| d.format("%Y-%m-%d %H:%M:%S%.f %:z").to_string()),
    }
}

/// Binary data in hexadecimal (for a CSV file), anything else as shown.
fn csv_cell(data: ColumnData<'static>) -> String {
    match data {
        ColumnData::Binary(Some(b)) => b.iter().map(|x| format!("{x:02X}")).collect(),
        other => match cell(other) {
            Cell::Null => "NULL".into(),
            c => c.text(),
        },
    }
}

fn string(c: Option<&Cell>) -> String {
    match c {
        Some(Cell::Text(t)) => t.clone(),
        _ => String::new(),
    }
}

fn number(c: Option<&Cell>) -> u64 {
    string(c).parse().unwrap_or(0)
}

/// The rows of the first result of `sql`.
async fn select(conn: &mut Conn, sql: &str) -> Result<Vec<Vec<Cell>>, Error> {
    let rows = conn.simple_query(sql).await?.into_first_result().await?;
    Ok(rows.into_iter().map(|r| r.into_iter().map(cell).collect()).collect())
}

/// "SQL Server 2019 · 15.0.4322.2".
async fn version(conn: &mut Conn) -> String {
    let rows = select(conn, "SELECT @@VERSION, CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128))").await.unwrap_or_default();
    let Some(row) = rows.first() else { return String::new() };
    let full = string(row.first());
    let first = full.lines().next().unwrap_or_default();
    let name = first.split(" (").next().unwrap_or(first).trim_start_matches("Microsoft ").trim();
    match string(row.get(1)) {
        v if v.is_empty() => name.to_owned(),
        v => format!("{name} · {v}"),
    }
}

/// The batches of a script: its parts between `GO` lines (which the server doesn't know).
pub(crate) fn batches(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in script.lines() {
        let word = line.trim().trim_end_matches(';');
        if word.eq_ignore_ascii_case("go") {
            if !current.trim().is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.clear();
            continue;
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// A batch that changes rows (INSERT, UPDATE...): run so as to know how many. Anything else is run
/// for its results.
fn changes_rows(batch: &str) -> bool {
    let mut rest = batch.trim_start();
    // Comments before the first word.
    loop {
        if let Some(r) = rest.strip_prefix("--") {
            rest = r.split_once('\n').map_or("", |(_, r)| r).trim_start();
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, r)| r).trim_start();
        } else {
            break;
        }
    }
    let word: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    ["INSERT", "UPDATE", "DELETE", "MERGE", "TRUNCATE"].iter().any(|k| word.eq_ignore_ascii_case(k))
}

/// Runs `sql`, keeping up to `max` rows of each of its results.
async fn run(conn: &mut Conn, sql: &str, max: usize) -> Result<Vec<QueryResult>, Error> {
    let mut out = Vec::new();
    for batch in batches(sql) {
        if changes_rows(&batch) {
            let done = conn.execute(batch, &[]).await?;
            out.push(QueryResult { affected: done.total(), ..Default::default() });
            continue;
        }
        let mut stream = conn.simple_query(batch).await?;
        let mut results = 0;
        while let Some(item) = stream.try_next().await? {
            if let Some(columns) = item.as_metadata().map(|m| m.columns().iter().map(|c| c.name().to_owned()).collect()) {
                out.push(QueryResult { columns, ..Default::default() });
                results += 1;
            } else if let (Some(row), Some(result)) = (item.into_row(), out.last_mut()) {
                if result.rows.len() < max {
                    result.rows.push(row.into_iter().map(cell).collect());
                } else {
                    result.truncated = true;
                }
            }
        }
        // No result (CREATE, EXEC of a procedure that returns nothing...): done.
        if results == 0 {
            out.push(QueryResult::default());
        }
    }
    Ok(out)
}

/// Runs the batches of `sql` as changes: the rows they changed.
async fn exec(conn: &mut Conn, sql: &str) -> Result<u64, Error> {
    let mut affected = 0;
    for batch in batches(sql) {
        affected += conn.execute(batch, &[]).await?.total();
    }
    Ok(affected)
}

async fn use_db(conn: &mut Conn, db: Option<&str>) -> Result<(), Error> {
    if let Some(d) = db.filter(|d| !d.is_empty()) {
        conn.simple_query(format!("USE {}", ident(d))).await?.into_results().await?;
    }
    Ok(())
}

/// Databases every server has.
pub fn system_db(name: &str) -> bool {
    matches!(name, "master" | "model" | "msdb" | "tempdb")
}

/// A column's type as written in SQL (`nvarchar(50)`, `decimal(10,2)`...), from sys.columns `c` and
/// sys.types `ty`.
const TYPE_SQL: &str = "ty.name + CASE \
     WHEN ty.name IN ('varchar', 'char', 'varbinary', 'binary') THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length AS varchar(10)) END + ')' \
     WHEN ty.name IN ('nvarchar', 'nchar') THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length / 2 AS varchar(10)) END + ')' \
     WHEN ty.name IN ('decimal', 'numeric') THEN '(' + CAST(c.precision AS varchar(10)) + ',' + CAST(c.scale AS varchar(10)) + ')' \
     WHEN ty.name IN ('datetime2', 'time', 'datetimeoffset') THEN '(' + CAST(c.scale AS varchar(10)) + ')' \
     ELSE '' END";

/// Procedures (P, PC: CLR) and functions (FN, FS: scalar; IF, TF, FT: rows).
const ROUTINE_TYPES: &str = "('P', 'PC', 'FN', 'FS', 'IF', 'TF', 'FT')";

/// Types that can't be searched as text.
const NOT_SEARCHED: [&str; 9] = ["image", "varbinary", "binary", "timestamp", "rowversion", "geography", "geometry", "hierarchyid", "sql_variant"];

/// The columns of the primary key of `db`.`table`, in order.
async fn primary_key(conn: &mut Conn, db: &str, table: &str) -> Vec<String> {
    let d = ident(db);
    let sql = format!(
        "SELECT c.name FROM {d}.sys.indexes i JOIN {d}.sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
         JOIN {d}.sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
         WHERE i.is_primary_key = 1 AND i.object_id = OBJECT_ID({}) ORDER BY ic.key_ordinal",
        literal(&self::table(db, table))
    );
    select(conn, &sql).await.unwrap_or_default().iter().map(|r| string(r.first())).collect()
}

/// A CREATE TABLE for a table, from its columns and primary key (the server keeps none).
fn create_sql(table: &str, s: &Structure, key: &[String]) -> String {
    let mut lines: Vec<String> = s
        .columns
        .iter()
        .map(|c| {
            let mut line = format!("  {} {}", ident(&string(c.first())), string(c.get(1)));
            let extra = string(c.get(5));
            if extra == "IDENTITY" {
                line.push_str(" IDENTITY");
            }
            line.push_str(if string(c.get(2)) == "YES" { " NULL" } else { " NOT NULL" });
            if let Some(Cell::Text(default)) = c.get(4) {
                line.push_str(&format!(" DEFAULT {default}"));
            }
            line
        })
        .collect();
    if !key.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", key.iter().map(|k| ident(k)).collect::<Vec<_>>().join(", ")));
    }
    format!("CREATE TABLE {} (\n{}\n)", local_table(table), lines.join(",\n"))
}

async fn handle(conn: &mut Conn, request: Request, emit: &Emitter) -> Result<(), Error> {
    match request {
        Request::Databases => {
            let rows = select(
                conn,
                "SELECT d.name, COALESCE(d.collation_name, N''), \
                 CAST(COALESCE((SELECT SUM(CAST(f.size AS bigint)) FROM sys.master_files f WHERE f.database_id = d.database_id), 0) * 8192 AS bigint), \
                 CASE WHEN d.state = 0 AND HAS_DBACCESS(d.name) = 1 THEN 1 ELSE 0 END \
                 FROM sys.databases d ORDER BY d.name",
            )
            .await?;
            let mut dbs: Vec<DatabaseInfo> = rows.iter().map(|r| DatabaseInfo { name: string(r.first()), collation: string(r.get(1)), tables: 0, size: number(r.get(2)) }).collect();
            // Tables of each database that can be opened, in one go (none if one of them refuses).
            let open: Vec<&str> = rows.iter().filter(|r| number(r.get(3)) == 1).map(|r| r.first()).filter_map(|c| if let Some(Cell::Text(n)) = c { Some(n.as_str()) } else { None }).collect();
            if !open.is_empty() {
                let counts: Vec<String> = open
                    .iter()
                    .map(|d| format!("SELECT {}, COUNT(*) FROM {}.sys.objects WHERE type IN ('U', 'V') AND is_ms_shipped = 0", literal(d), ident(d)))
                    .collect();
                if let Ok(rows) = select(conn, &counts.join(" UNION ALL ")).await {
                    for r in rows {
                        let name = string(r.first());
                        if let Some(d) = dbs.iter_mut().find(|d| d.name == name) {
                            d.tables = number(r.get(1));
                        }
                    }
                }
            }
            emit.send(Event::Databases(dbs));
        }
        Request::Tables(db) => {
            let d = ident(&db);
            let sql = format!(
                "SELECT s.name + N'.' + o.name, RTRIM(o.type), \
                 COALESCE((SELECT SUM(p.rows) FROM {d}.sys.partitions p WHERE p.object_id = o.object_id AND p.index_id IN (0, 1)), 0), \
                 COALESCE((SELECT SUM(a.total_pages) FROM {d}.sys.partitions p JOIN {d}.sys.allocation_units a ON a.container_id = p.partition_id WHERE p.object_id = o.object_id), 0) * 8192, \
                 COALESCE(CAST(ep.value AS nvarchar(4000)), N'') \
                 FROM {d}.sys.objects o JOIN {d}.sys.schemas s ON s.schema_id = o.schema_id \
                 LEFT JOIN {d}.sys.extended_properties ep ON ep.major_id = o.object_id AND ep.minor_id = 0 AND ep.class = 1 AND ep.name = N'MS_Description' \
                 WHERE o.type IN ('U', 'V') AND o.is_ms_shipped = 0 ORDER BY 1"
            );
            let tables = select(conn, &sql)
                .await?
                .iter()
                .map(|r| TableInfo { name: string(r.first()), view: string(r.get(1)) == "V", engine: String::new(), rows: number(r.get(2)), size: number(r.get(3)), collation: String::new(), comment: string(r.get(4)) })
                .collect();
            emit.send(Event::Tables { db, tables });
        }
        Request::Columns(db) => {
            let d = ident(&db);
            let sql = format!(
                "SELECT TOP 50000 s.name, o.name, c.name, {TYPE_SQL} FROM {d}.sys.columns c JOIN {d}.sys.objects o ON o.object_id = c.object_id \
                 JOIN {d}.sys.schemas s ON s.schema_id = o.schema_id JOIN {d}.sys.types ty ON ty.user_type_id = c.user_type_id \
                 WHERE o.type IN ('U', 'V') AND o.is_ms_shipped = 0 ORDER BY s.name, o.name, c.column_id"
            );
            let mut columns: std::collections::HashMap<String, Vec<(String, String)>> = std::collections::HashMap::new();
            for r in select(conn, &sql).await? {
                let (schema, name, column) = (string(r.first()), string(r.get(1)), (string(r.get(2)), string(r.get(3))));
                // Named as listed (schema.table), and as usually written in queries (table alone).
                columns.entry(format!("{schema}.{name}")).or_default().push(column.clone());
                columns.entry(name).or_default().push(column);
            }
            emit.send(Event::Columns { db, columns });
        }
        Request::Structure { db, table } => {
            let (d, object) = (ident(&db), literal(&self::table(&db, &table)));
            let columns = select(
                conn,
                &format!(
                    "SELECT c.name, {TYPE_SQL}, CASE WHEN c.is_nullable = 1 THEN 'YES' ELSE 'NO' END, \
                     CASE WHEN EXISTS (SELECT 1 FROM {d}.sys.index_columns ic JOIN {d}.sys.indexes i ON i.object_id = ic.object_id AND i.index_id = ic.index_id \
                       WHERE ic.object_id = c.object_id AND ic.column_id = c.column_id AND i.is_primary_key = 1) THEN 'PRI' \
                     WHEN EXISTS (SELECT 1 FROM {d}.sys.index_columns ic JOIN {d}.sys.indexes i ON i.object_id = ic.object_id AND i.index_id = ic.index_id \
                       WHERE ic.object_id = c.object_id AND ic.column_id = c.column_id AND i.is_unique = 1) THEN 'UNI' ELSE '' END, \
                     dc.definition, CASE WHEN c.is_identity = 1 THEN 'IDENTITY' WHEN c.is_computed = 1 THEN 'COMPUTED' WHEN ty.name IN ('timestamp', 'rowversion') THEN 'ROWVERSION' ELSE '' END, \
                     COALESCE(CAST(ep.value AS nvarchar(4000)), N''), N'', COALESCE(c.collation_name, N'') \
                     FROM {d}.sys.columns c JOIN {d}.sys.types ty ON ty.user_type_id = c.user_type_id \
                     LEFT JOIN {d}.sys.default_constraints dc ON dc.object_id = c.default_object_id \
                     LEFT JOIN {d}.sys.extended_properties ep ON ep.major_id = c.object_id AND ep.minor_id = c.column_id AND ep.class = 1 AND ep.name = N'MS_Description' \
                     WHERE c.object_id = OBJECT_ID({object}) ORDER BY c.column_id"
                ),
            )
            .await?;
            let indexes = select(
                conn,
                &format!(
                    "SELECT i.name, STUFF((SELECT N', ' + c.name FROM {d}.sys.index_columns ic JOIN {d}.sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
                       WHERE ic.object_id = i.object_id AND ic.index_id = i.index_id AND ic.is_included_column = 0 ORDER BY ic.key_ordinal FOR XML PATH(''), TYPE).value('.', 'nvarchar(max)'), 1, 2, N''), \
                     CASE WHEN i.is_unique = 1 THEN 'UNIQUE' ELSE '' END, i.type_desc \
                     FROM {d}.sys.indexes i WHERE i.object_id = OBJECT_ID({object}) AND i.type > 0 ORDER BY i.index_id"
                ),
            )
            .await
            .unwrap_or_default();
            // A view's definition as written; a table's rebuilt.
            let definition = select(conn, &format!("SELECT m.definition FROM {d}.sys.sql_modules m WHERE m.object_id = OBJECT_ID({object})")).await.unwrap_or_default();
            let mut structure = Structure { columns, indexes, create: String::new() };
            structure.create = match definition.first().map(|r| string(r.first())).filter(|s| !s.is_empty()) {
                Some(view) => view.trim().to_owned(),
                None => create_sql(&table, &structure, &primary_key(conn, &db, &table).await),
            };
            emit.send(Event::Structure { db, table, structure });
        }
        Request::Rows { db, table, offset, limit, order, search } => {
            let order_by = match order {
                Some((c, asc)) => format!(" ORDER BY {} {}", ident(&c), if asc { "ASC" } else { "DESC" }),
                None => {
                    let keys = primary_key(conn, &db, &table).await;
                    // Paging needs an order: the key's, else the table's own.
                    if keys.is_empty() { " ORDER BY (SELECT NULL)".to_owned() } else { format!(" ORDER BY {}", keys.iter().map(|k| ident(k)).collect::<Vec<_>>().join(", ")) }
                }
            };
            let object = literal(&self::table(&db, &table));
            let filter = match search.as_ref().filter(|(text, columns)| !text.is_empty() && !columns.is_empty()) {
                Some((text, columns)) => {
                    // Columns that can be read as text; % _ [ typed are looked for as such.
                    let types = select(conn, &format!("SELECT c.name, TYPE_NAME(c.system_type_id) FROM {}.sys.columns c WHERE c.object_id = OBJECT_ID({object})", ident(&db))).await.unwrap_or_default();
                    let searched: Vec<&String> = columns.iter().filter(|c| types.iter().any(|t| string(t.first()) == **c && !NOT_SEARCHED.contains(&string(t.get(1)).as_str()))).collect();
                    let pattern = literal(&format!("%{}%", text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_").replace('[', "\\[")));
                    let any: Vec<String> = searched.iter().map(|c| format!("CAST({} AS nvarchar(max)) LIKE {pattern} ESCAPE N'\\'", ident(c))).collect();
                    Some(if any.is_empty() { " WHERE 1 = 0".to_owned() } else { format!(" WHERE {}", any.join(" OR ")) })
                }
                None => None,
            };
            let filter_sql = filter.clone().unwrap_or_default();
            let page = format!("{filter_sql}{order_by} OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY");
            let result = run(conn, &format!("SELECT * FROM {}{page}", self::table(&db, &table)), limit as usize).await?.into_iter().next().unwrap_or_default();
            let estimate = select(conn, &format!("SELECT COALESCE(SUM(p.rows), 0) FROM {}.sys.partitions p WHERE p.object_id = OBJECT_ID({object}) AND p.index_id IN (0, 1)", ident(&db)))
                .await
                .ok()
                .and_then(|r| r.first().map(|r| number(r.first())))
                .unwrap_or(0);
            let (total, exact) = if estimate <= EXACT_COUNT_MAX || filter.is_some() {
                let n = select(conn, &format!("SELECT COUNT_BIG(*) FROM {}{filter_sql}", self::table(&db, &table))).await.ok().and_then(|r| r.first().map(|r| number(r.first())));
                (n.unwrap_or(estimate), n.is_some())
            } else {
                (estimate, false)
            };
            let sql = format!("SELECT * FROM {}{page}", local_table(&table));
            emit.send(Event::Rows { db, table, offset, result, total, exact, sql });
        }
        Request::Query { db, sql } => {
            use_db(conn, db.as_deref()).await?;
            let start = Instant::now();
            match run(conn, &sql, MAX_QUERY_ROWS).await {
                Ok(results) => emit.send(Event::Query { results, elapsed: start.elapsed(), error: None }),
                Err(e) if is_lost(&e) => return Err(e),
                Err(e) => emit.send(Event::Query { results: Vec::new(), elapsed: start.elapsed(), error: Some(message(&e)) }),
            }
        }
        Request::Exec { db, sql, tag } => {
            use_db(conn, db.as_deref()).await?;
            match exec(conn, &sql).await {
                Ok(affected) => emit.send(Event::Done { affected, tag }),
                Err(e) if is_lost(&e) => return Err(e),
                Err(e) => emit.send(Event::Failed { error: message(&e), tag }),
            }
        }
        // Accounts are not managed here for SQL Server (the view doesn't offer it).
        Request::Users => emit.send(Event::Users(Vec::new())),
        Request::ForeignKeys { db, table } => emit.send(Event::ForeignKeys { db, table, keys: Vec::new() }),
        Request::Grants { user, host } => emit.send(Event::Grants { user, host, grants: Vec::new() }),
        Request::Routines(db) => {
            let d = ident(&db);
            let rows = select(
                conn,
                &format!(
                    "SELECT s.name + N'.' + o.name, RTRIM(o.type), CONVERT(nvarchar(19), o.create_date, 120), CONVERT(nvarchar(19), o.modify_date, 120), \
                     COALESCE(CAST(ep.value AS nvarchar(4000)), N''), CASE WHEN m.execute_as_principal_id IS NULL THEN N'CALLER' WHEN m.execute_as_principal_id = -2 THEN N'OWNER' ELSE N'USER' END, \
                     CASE WHEN m.is_schema_bound = 1 THEN 1 ELSE 0 END \
                     FROM {d}.sys.objects o JOIN {d}.sys.schemas s ON s.schema_id = o.schema_id LEFT JOIN {d}.sys.sql_modules m ON m.object_id = o.object_id \
                     LEFT JOIN {d}.sys.extended_properties ep ON ep.major_id = o.object_id AND ep.minor_id = 0 AND ep.class = 1 AND ep.name = N'MS_Description' \
                     WHERE o.type IN {ROUTINE_TYPES} AND o.is_ms_shipped = 0 ORDER BY 1"
                ),
            )
            .await?;
            let mut routines: Vec<Routine> = rows
                .iter()
                .map(|r| {
                    let code = string(r.get(1));
                    let function = !matches!(code.as_str(), "P" | "PC");
                    Routine {
                        name: string(r.first()),
                        function,
                        returns: if matches!(code.as_str(), "IF" | "TF" | "FT") { "TABLE".into() } else { String::new() },
                        code,
                        created: string(r.get(2)),
                        modified: string(r.get(3)),
                        comment: string(r.get(4)),
                        security: string(r.get(5)),
                        access: if number(r.get(6)) == 1 { "SCHEMABINDING".into() } else { String::new() },
                        ..Default::default()
                    }
                })
                .collect();
            // Parameter 0: what a scalar function returns.
            let params = select(
                conn,
                &format!(
                    "SELECT s.name + N'.' + o.name, c.parameter_id, c.name, {TYPE_SQL}, CAST(c.is_output AS int) \
                     FROM {d}.sys.parameters c JOIN {d}.sys.objects o ON o.object_id = c.object_id JOIN {d}.sys.schemas s ON s.schema_id = o.schema_id \
                     JOIN {d}.sys.types ty ON ty.user_type_id = c.user_type_id WHERE o.type IN {ROUTINE_TYPES} AND o.is_ms_shipped = 0 ORDER BY 1, c.parameter_id"
                ),
            )
            .await
            .unwrap_or_default();
            for p in &params {
                let name = string(p.first());
                let Some(r) = routines.iter_mut().find(|r| r.name == name) else { continue };
                if number(p.get(1)) == 0 {
                    r.returns = string(p.get(3));
                } else {
                    let mode = if number(p.get(4)) == 1 { "OUT" } else { "IN" };
                    r.params.push(Param { mode: mode.into(), name: string(p.get(2)), kind: string(p.get(3)) });
                }
            }
            emit.send(Event::Routines { db, routines });
        }
        Request::Triggers(db) => {
            let d = ident(&db);
            let rows = select(
                conn,
                &format!(
                    "SELECT s.name + N'.' + tr.name, s.name + N'.' + o.name, CAST(tr.is_disabled AS int), CAST(tr.is_instead_of_trigger AS int), \
                     CONVERT(nvarchar(19), tr.create_date, 120), CONVERT(nvarchar(19), tr.modify_date, 120), COALESCE(te.type_desc, N'') \
                     FROM {d}.sys.triggers tr JOIN {d}.sys.objects o ON o.object_id = tr.parent_id JOIN {d}.sys.schemas s ON s.schema_id = o.schema_id \
                     LEFT JOIN {d}.sys.trigger_events te ON te.object_id = tr.object_id \
                     WHERE tr.parent_class = 1 ORDER BY 2, 1, te.type"
                ),
            )
            .await?;
            let mut triggers: Vec<Trigger> = Vec::new();
            for r in &rows {
                let (name, event) = (string(r.first()), string(r.get(6)));
                if let Some(t) = triggers.last_mut().filter(|t| t.name == name) {
                    t.events = format!("{}, {event}", t.events);
                    continue;
                }
                triggers.push(Trigger {
                    name,
                    table: string(r.get(1)),
                    timing: if number(r.get(3)) == 1 { "INSTEAD OF".into() } else { "AFTER".into() },
                    events: event,
                    order: 0,
                    enabled: number(r.get(2)) == 0,
                    definer: String::new(),
                    created: string(r.get(4)),
                    modified: string(r.get(5)),
                });
            }
            emit.send(Event::Triggers { db, triggers });
        }
        Request::Definition { db, kind, name } => {
            let sql = format!("SELECT m.definition FROM {}.sys.sql_modules m WHERE m.object_id = OBJECT_ID({})", ident(&db), literal(&self::table(&db, &name)));
            let rows = match select(conn, &sql).await {
                Ok(rows) => rows,
                Err(e) if is_lost(&e) => return Err(e),
                Err(_) => Vec::new(),
            };
            let sql = rows.first().map(|r| string(r.first())).unwrap_or_default().trim().to_owned();
            emit.send(Event::Definition { db, kind, name, sql });
        }
        Request::Replace { db, check, drop, create, restore, tag } => {
            use_db(conn, Some(&db)).await?;
            let fail = |e: Error| if is_lost(&e) { Err(e) } else { Ok(message(&e)) };
            for s in check.iter().filter(|s| !s.trim().is_empty()) {
                if let Err(e) = exec(conn, s).await {
                    let error = fail(e)?;
                    if let Some(last) = check.last().filter(|l| *l != s) {
                        let _ = exec(conn, last).await;
                    }
                    emit.send(Event::Failed { error, tag });
                    return Ok(());
                }
            }
            if !drop.trim().is_empty()
                && let Err(e) = exec(conn, &drop).await
            {
                emit.send(Event::Failed { error: fail(e)?, tag });
                return Ok(());
            }
            match exec(conn, &create).await {
                Ok(_) => emit.send(Event::Done { affected: 0, tag }),
                Err(e) => {
                    let error = fail(e)?;
                    let mut back = drop.trim().is_empty();
                    if !back {
                        for s in restore.iter().filter(|s| !s.trim().is_empty()) {
                            if exec(conn, s).await.is_ok() {
                                back = true;
                                break;
                            }
                        }
                    }
                    emit.send(Event::Failed { error: format!("{}{error}", if back { RESTORED } else { LOST }), tag });
                }
            }
        }
        Request::Export { .. } | Request::ExportCsv { .. } | Request::Import { .. } => {}
    }
    Ok(())
}

/// Writes the rows of a table to a .csv file.
async fn export_csv(conn: &mut Conn, db: &str, table: &str, path: &std::path::Path, cancel: &AtomicBool) -> Result<String, String> {
    use std::io::Write as _;
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let mut out = db::export_writer(path)?;
    let mut stream = conn.simple_query(format!("SELECT * FROM {}", self::table(db, table))).await.map_err(|e| message(&e))?;
    let mut n = 0u64;
    while let Some(item) = stream.try_next().await.map_err(|e| message(&e))? {
        let line = match item.as_metadata().map(|m| m.columns().iter().map(|c| db::csv_field(c.name())).collect::<Vec<_>>()) {
            Some(head) => head.join(","),
            None => {
                let Some(row) = item.into_row() else { continue };
                n += 1;
                if n.is_multiple_of(1000) && cancel.load(Ordering::Relaxed) {
                    return Err(CANCELLED.to_owned());
                }
                row.into_iter().map(|c| db::csv_field(&csv_cell(c))).collect::<Vec<_>>().join(",")
            }
        };
        writeln!(out, "{line}").map_err(|e| err(&e))?;
    }
    out.flush().map_err(|e| err(&e))?;
    Ok(format!("{n} rows"))
}

/// A script's text: UTF-16 (as SSMS may save it) or UTF-8, compressed or not.
fn read_script(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read as _;
    let mut bytes = std::fs::read(path).map_err(|e| format!("{} : {e}", path.display()))?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gz")) {
        let mut plain = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut plain).map_err(|e| e.to_string())?;
        bytes = plain;
    }
    let utf16 = |big: bool, b: &[u8]| String::from_utf16_lossy(&b.chunks_exact(2).map(|p| if big { u16::from_be_bytes([p[0], p[1]]) } else { u16::from_le_bytes([p[0], p[1]]) }).collect::<Vec<_>>());
    Ok(match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => utf16(false, rest),
        [0xFE, 0xFF, rest @ ..] => utf16(true, rest),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        b => String::from_utf8_lossy(b).into_owned(),
    })
}

/// Runs the batches of a .sql file in `db` (or where it says).
async fn import(conn: &mut Conn, db: Option<&str>, path: &std::path::Path, emit: &Emitter, cancel: &AtomicBool) -> Result<String, String> {
    let script = read_script(path)?;
    use_db(conn, db).await.map_err(|e| message(&e))?;
    let all = batches(&script);
    for (i, batch) in all.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.to_owned());
        }
        emit.send(Event::Progress { fraction: i as f32 / all.len() as f32, text: format!("{} / {}", i + 1, all.len()) });
        if let Err(e) = conn.execute(batch.as_str(), &[]).await {
            let head: String = batch.trim().chars().take(120).collect();
            return Err(format!("#{} : {}\n{head}", i + 1, message(&e)));
        }
    }
    Ok(format!("{} batches", all.len()))
}

/// A connection used for one export or import, beside the view's.
async fn transfer(direct: Target, request: Request, emit: Emitter, cancel: Arc<AtomicBool>) {
    let result = async {
        let mut conn = connect(&direct).await?;
        let result = match &request {
            Request::ExportCsv { db, table, path } => export_csv(&mut conn, db, table, path, &cancel).await,
            Request::Import { db, path, .. } => import(&mut conn, db.as_deref(), path, &emit, &cancel).await,
            // A .sql dump of a SQL Server database isn't offered.
            _ => Ok(String::new()),
        };
        let _ = conn.close().await;
        result
    }
    .await;
    if let (Err(_), Request::ExportCsv { path, .. }) = (&result, &request) {
        let _ = std::fs::remove_file(path);
    }
    let imported = matches!(request, Request::Import { .. });
    emit.send(Event::Transfer(result));
    if imported {
        emit.send(Event::Done { affected: 0, tag: 0 });
    }
}

/// Tries to connect: the server's version, or why not; and the ssh process of the forward, if any.
pub fn test(ctx: &egui::Context, target: Target) -> (Receiver<Result<String, String>>, Option<u32>) {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    let _guard = db::runtime().enter();
    let forward = match target.tunnel.as_ref().map(|l| db::start_forward(&target, l)).transpose() {
        Ok(f) => f,
        Err(e) => {
            let _ = tx.send(Err(e));
            return (rx, None);
        }
    };
    let pid = forward.as_ref().and_then(|f| f.child.id());
    db::runtime().spawn(async move {
        let mut forward = forward;
        let result = async {
            if let Some(f) = forward.as_mut() {
                f.ready().await?;
            }
            let mut conn = connect(&db::direct(&target, forward.as_ref())).await?;
            let v = version(&mut conn).await;
            let _ = conn.close().await;
            Ok(v)
        }
        .await;
        let _ = tx.send(result);
        ctx.request_repaint();
        drop(forward);
    });
    (rx, pid)
}

/// Opens the view's connection (after the SSH forward, if any), then serves its requests.
pub fn open(ctx: &egui::Context, target: Target) -> Connection {
    let (requests, mut incoming) = tmpsc::unbounded_channel::<Request>();
    let (tx, events) = mpsc::channel();
    let emit = Emitter { tx, ctx: ctx.clone() };
    let stop = Arc::new(tokio::sync::Notify::new());
    let stop_tunnel = Arc::new(tokio::sync::Notify::new());
    let cancel = Arc::new(tokio::sync::Notify::new());
    let cancel_transfer = Arc::new(AtomicBool::new(false));
    let _guard = db::runtime().enter();
    let (forward, failed) = match target.tunnel.as_ref().map(|l| db::start_forward(&target, l)) {
        Some(Ok(f)) => (Some(f), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };
    let tunnel_pid = forward.as_ref().and_then(|f| f.child.id());
    let direct = db::direct(&target, forward.as_ref());
    let (stopped, tunnel_stopped, cancelled, conn_target, t_cancel) = (stop.clone(), stop_tunnel.clone(), cancel.clone(), direct.clone(), cancel_transfer.clone());
    db::runtime().spawn(async move {
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
            db::runtime().spawn(async move {
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
        let v = version(&mut conn).await;
        emit.send(Event::Connected { version: v });
        loop {
            tokio::select! {
                request = incoming.recv() => {
                    let Some(request) = request else { break };
                    if matches!(request, Request::Export { .. } | Request::ExportCsv { .. } | Request::Import { .. }) {
                        t_cancel.store(false, Ordering::Relaxed);
                        db::runtime().spawn(transfer(conn_target.clone(), request, emit.clone(), t_cancel.clone()));
                        continue;
                    }
                    // A stop asked while nothing ran is forgotten.
                    let _ = cancelled.notified().now_or_never();
                    let start = Instant::now();
                    let outcome = tokio::select! {
                        outcome = handle(&mut conn, request.clone(), &emit) => Some(outcome),
                        _ = cancelled.notified() => None,
                    };
                    let mut outcome = match outcome {
                        Some(outcome) => outcome,
                        // Stopped by the user: the server is told, the connection goes on.
                        None => {
                            let _ = conn.cancel_query().await;
                            emit.send(match request {
                                Request::Query { .. } => Event::Query { results: Vec::new(), elapsed: start.elapsed(), error: Some(STOPPED.into()) },
                                Request::Exec { tag, .. } | Request::Replace { tag, .. } => Event::Failed { error: STOPPED.into(), tag },
                                _ => Event::Error(STOPPED.into()),
                            });
                            continue;
                        }
                    };
                    // Lost (a restart, the network): connected again, and asked again once.
                    if outcome.as_ref().is_err_and(is_lost) {
                        if let Ok(fresh) = connect(&conn_target).await {
                            conn = fresh;
                            outcome = handle(&mut conn, request, &emit).await;
                        }
                    }
                    if let Err(e) = outcome {
                        if is_lost(&e) {
                            emit.send(Event::Closed(message(&e)));
                            return;
                        }
                        emit.send(Event::Error(message(&e)));
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
        let _ = conn.close().await;
    });
    Connection {
        requests,
        events,
        stop,
        stop_tunnel,
        direct,
        thread: Arc::new(AtomicU32::new(0)),
        transfer_thread: Arc::new(AtomicU32::new(0)),
        cancel_transfer,
        cancel_signal: Some(cancel),
        tunnel_pid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_names_and_strings() {
        assert_eq!(ident("a]b"), "[a]]b]");
        assert_eq!(literal("it's \\ ok"), "N'it''s \\ ok'");
        assert_eq!(table("app", "sales.orders"), "[app].[sales].[orders]");
        assert_eq!(local_table("users"), "[dbo].[users]");
    }

    #[test]
    fn cuts_scripts_on_go() {
        let script = "CREATE TABLE t (a int)\nGO\n\ngo;\nINSERT INTO t VALUES (1)\n  GO  \nSELECT * FROM t";
        assert_eq!(batches(script), ["CREATE TABLE t (a int)\n", "INSERT INTO t VALUES (1)\n", "SELECT * FROM t\n"]);
        // A word starting with "go" is not a separator.
        assert_eq!(batches("SELECT 1 AS good\ngood").len(), 1);
        assert!(changes_rows("-- note\n/* x */ update t set a = 1"));
        assert!(!changes_rows("SELECT * FROM t"));
        assert!(!changes_rows("EXEC sp_who"));
    }

    fn target() -> Option<Target> {
        let var = |k: &str| std::env::var(k).ok();
        Some(Target {
            host: var("RONNIE_MSSQL_HOST")?,
            port: var("RONNIE_MSSQL_PORT").and_then(|p| p.parse().ok()).unwrap_or(1433),
            user: var("RONNIE_MSSQL_USER")?,
            password: var("RONNIE_MSSQL_PASSWORD"),
            database: None,
            tunnel: None,
            engine: Engine::Sqlserver,
            trust_cert: var("RONNIE_MSSQL_TRUST").is_some(),
        })
    }

    fn wait(conn: &Connection, mut done: impl FnMut(Event) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            for e in conn.poll() {
                if let Event::Closed(e) = &e {
                    panic!("{e}");
                }
                if done(e) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timeout");
    }

    /// Against a real server (RONNIE_MSSQL_HOST, _PORT, _USER, _PASSWORD, _TRUST):
    /// cargo test mssql -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_server() {
        let Some(target) = target() else { return };
        let ctx = egui::Context::default();
        let (rx, _) = test(&ctx, target.clone());
        let version = rx.recv_timeout(Duration::from_secs(20)).unwrap().unwrap();
        println!("version: {version}");
        let conn = Connection::open(&ctx, target);
        conn.send(Request::Databases);
        let mut dbs = Vec::new();
        wait(&conn, |e| if let Event::Databases(d) = e { dbs = d; true } else { false });
        println!("{} databases: {:?}", dbs.len(), dbs.iter().map(|d| (&d.name, d.tables)).collect::<Vec<_>>());
        assert!(dbs.iter().any(|d| d.name == "master"));
        conn.send(Request::Query { db: Some("tempdb".into()), sql: "SELECT 1 AS un, NULL AS rien, N'é' AS e, CAST(1.5 AS decimal(5,2)) AS d, GETDATE() AS now, NEWID() AS id\nGO\nSELECT 'deux' AS x WHERE 1 = 0".into() });
        let mut query = None;
        wait(&conn, |e| if let Event::Query { results, error, .. } = e { query = Some((results, error)); true } else { false });
        let (results, error) = query.unwrap();
        assert_eq!(error, None);
        println!("{:?}", results[0].rows);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].columns, ["un", "rien", "e", "d", "now", "id"]);
        assert_eq!(results[0].rows[0][..4], [Cell::Text("1".into()), Cell::Null, Cell::Text("é".into()), Cell::Text("1.50".into())]);
        assert_eq!(results[1].columns, ["x"]);
        assert!(results[1].rows.is_empty());
        // A user database: its tables, one table's structure and rows.
        let Some(user_db) = dbs.iter().find(|d| !system_db(&d.name) && d.tables > 0) else { return };
        conn.send(Request::Tables(user_db.name.clone()));
        let mut tables = Vec::new();
        wait(&conn, |e| if let Event::Tables { tables: t, .. } = e { tables = t; true } else { false });
        let Some(first) = tables.iter().find(|t| !t.view) else { return };
        println!("{}: {} tables, first {} (~{} rows)", user_db.name, tables.len(), first.name, first.rows);
        conn.send(Request::Structure { db: user_db.name.clone(), table: first.name.clone() });
        wait(&conn, |e| if let Event::Structure { structure, .. } = e { println!("{}", structure.create); !structure.columns.is_empty() } else { false });
        conn.send(Request::Rows { db: user_db.name.clone(), table: first.name.clone(), offset: 0, limit: 5, order: None, search: None });
        wait(&conn, |e| if let Event::Rows { result, total, sql, .. } = e { println!("{sql}: {} of {total}", result.rows.len()); true } else { false });
        // A refused change says so, and the connection goes on.
        conn.send(Request::Exec { db: Some("tempdb".into()), sql: "UPDATE no_such_table SET a = 1".into(), tag: 7 });
        wait(&conn, |e| matches!(e, Event::Failed { tag: 7, .. }));
        // A long query stopped.
        conn.send(Request::Query { db: None, sql: "WAITFOR DELAY '00:00:30'".into() });
        std::thread::sleep(Duration::from_millis(500));
        conn.cancel_query();
        wait(&conn, |e| matches!(e, Event::Query { error: Some(e), .. } if e == STOPPED));
        conn.send(Request::Query { db: None, sql: "SELECT 2".into() });
        wait(&conn, |e| matches!(e, Event::Query { error: None, results, .. } if results[0].rows[0][0] == Cell::Text("2".into())));
    }
}
