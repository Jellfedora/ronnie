//! A mail catcher for local development: an SMTP server on this machine that accepts every mail
//! sent to it and keeps it, without delivering anything. The projects under development are told
//! to send their mails to `127.0.0.1:<port>` (no TLS; any login is accepted).
//!
//! The mails are kept as they came, one `.eml` file each, in `<config dir>/mail/messages/`, with
//! an index (`index.json`: who, what, read or not, a bit of the text for the search) beside.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read as _, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use mail_parser::{Address, MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};

/// The port mails are expected on, as Mailpit and MailHog do.
pub const DEFAULT_PORT: u16 = 1025;
/// A mail bigger than this is refused.
const MAX_SIZE: usize = 50 * 1024 * 1024;
/// How much of a mail's text the index keeps, for the search.
const SEARCH_TEXT: usize = 4000;
/// The mails kept at most: the oldest go beyond.
const MAX_KEPT: usize = 5000;

fn root() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("mail"))
}

fn messages_dir() -> Option<PathBuf> {
    root().map(|d| d.join("messages"))
}

/// A mail received, as the list shows it.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Summary {
    /// Its file's name, without ".eml".
    pub id: String,
    /// When it came (Unix seconds).
    pub received: i64,
    /// "Name <address>", from its From header (or the envelope's).
    pub from: String,
    /// Its recipients: the envelope's (Bcc included).
    pub to: Vec<String>,
    pub subject: String,
    pub size: u64,
    pub read: bool,
    pub attachments: usize,
    /// The start of its text, for the list's preview and the search.
    pub text: String,
}

impl Summary {
    /// Whether every word of `query` (lowercased) is found in it.
    pub fn matches(&self, words: &[String]) -> bool {
        if words.is_empty() {
            return true;
        }
        let hay = format!("{}\n{}\n{}\n{}", self.from, self.to.join(", "), self.subject, self.text).to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    }
}

/// The mails kept, the newest first.
pub struct Store {
    pub mails: Vec<Summary>,
    /// Changed each time the list does (for the page to follow).
    pub version: u64,
    /// The mails received since last taken (to tell of them).
    pub arrived: Vec<Summary>,
}

pub type Shared = Arc<Mutex<Store>>;

impl Store {
    /// The index read again (a mail whose file is gone left out).
    pub fn load() -> Shared {
        let mut mails: Vec<Summary> = root().and_then(|d| std::fs::read(d.join("index.json")).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if let Some(dir) = messages_dir() {
            mails.retain(|m| safe_id(&m.id) && dir.join(format!("{}.eml", m.id)).is_file());
        }
        mails.sort_by_key(|m| std::cmp::Reverse(m.received));
        Arc::new(Mutex::new(Store { mails, version: 1, arrived: Vec::new() }))
    }

    fn save(&mut self) {
        self.version += 1;
        let Some(dir) = root() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_vec(&self.mails) {
            // Written aside, then moved in place: never half an index.
            let tmp = dir.join("index.json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join("index.json"));
            }
        }
    }

    pub fn unread(&self) -> usize {
        self.mails.iter().filter(|m| !m.read).count()
    }

    pub fn set_read(&mut self, ids: &HashSet<String>, read: bool) {
        let mut changed = false;
        for m in self.mails.iter_mut().filter(|m| ids.contains(&m.id) && m.read != read) {
            m.read = read;
            changed = true;
        }
        if changed {
            self.save();
        }
    }

    pub fn delete(&mut self, ids: &HashSet<String>) {
        if let Some(dir) = messages_dir() {
            for id in ids.iter().filter(|id| safe_id(id)) {
                let _ = std::fs::remove_file(dir.join(format!("{id}.eml")));
            }
        }
        self.mails.retain(|m| !ids.contains(&m.id));
        self.save();
    }

    fn add(&mut self, summary: Summary) {
        self.mails.insert(0, summary.clone());
        if self.mails.len() > MAX_KEPT {
            let gone: HashSet<String> = self.mails.drain(MAX_KEPT..).map(|m| m.id).collect();
            if let Some(dir) = messages_dir() {
                for id in gone {
                    let _ = std::fs::remove_file(dir.join(format!("{id}.eml")));
                }
            }
        }
        self.arrived.push(summary);
        self.save();
    }
}

/// An id that can't leave the messages' folder.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The mail with this id, as it came.
pub fn raw(id: &str) -> Option<Vec<u8>> {
    if !safe_id(id) {
        return None;
    }
    std::fs::read(messages_dir()?.join(format!("{id}.eml"))).ok()
}

/// Its file (to open it with the mail app, or to find it).
pub fn path(id: &str) -> Option<PathBuf> {
    safe_id(id).then(|| messages_dir().map(|d| d.join(format!("{id}.eml")))).flatten()
}

/// "Name <address>", or the address alone.
pub fn format_address(address: Option<&Address>) -> Vec<String> {
    address
        .map(|a| {
            a.iter()
                .map(|a| match (a.name(), a.address()) {
                    (Some(name), Some(addr)) if !name.is_empty() => format!("{name} <{addr}>"),
                    (_, Some(addr)) => addr.to_owned(),
                    (Some(name), None) => name.to_owned(),
                    (None, None) => String::new(),
                })
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// What the list knows of a mail that just came.
fn summarize(id: String, raw: &[u8], envelope_from: &str, envelope_to: &[String]) -> Summary {
    let received = chrono::Utc::now().timestamp();
    let mut summary = Summary { id, received, from: envelope_from.to_owned(), to: envelope_to.to_vec(), size: raw.len() as u64, ..Default::default() };
    if let Some(message) = MessageParser::default().parse(raw) {
        if let Some(from) = format_address(message.from()).into_iter().next() {
            summary.from = from;
        }
        summary.subject = message.subject().unwrap_or_default().to_owned();
        summary.attachments = message.attachments().filter(|a| a.content_id().is_none() || a.attachment_name().is_some()).count();
        let text = message.body_text(0).unwrap_or_default();
        summary.text = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(SEARCH_TEXT).collect();
    }
    summary
}

/// The SMTP server, listening while it lives.
pub struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Server {
    /// The port let go of before it returns (it can be listened on again at once).
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        crate::log::info(&format!("Mail catcher stopped (port {})", self.port));
    }
}

/// Listens on `port` of this machine (IPv4 and, when it can, IPv6: "localhost" may be either).
/// `ctx` is woken up at each mail.
pub fn start(port: u16, store: Shared, ctx: egui::Context) -> Result<Server> {
    let v4 = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).with_context(|| format!("port {port}"))?;
    let v6 = TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port))).ok();
    let listeners: Vec<TcpListener> = std::iter::once(v4).chain(v6).collect();
    for l in &listeners {
        l.set_nonblocking(true)?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new().name("smtp".into()).spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            let mut idle = true;
            for l in &listeners {
                if let Ok((stream, _)) = l.accept() {
                    idle = false;
                    let (store, ctx) = (store.clone(), ctx.clone());
                    let _ = std::thread::Builder::new().name("smtp-session".into()).spawn(move || {
                        if let Err(e) = session(stream, &store, &ctx) {
                            crate::log::info(&format!("smtp: {e:#}"));
                        }
                    });
                }
            }
            if idle {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    })?;
    crate::log::info(&format!("Mail catcher listening on port {port}"));
    Ok(Server { port, stop, thread: Some(thread) })
}

/// The address in "MAIL FROM:<a@b> SIZE=12".
fn path_arg(arg: &str) -> String {
    let arg = arg.trim();
    match (arg.find('<'), arg.find('>')) {
        (Some(a), Some(b)) if b > a => arg[a + 1..b].to_owned(),
        _ => arg.split_whitespace().next().unwrap_or_default().to_owned(),
    }
}

/// One client, from its greeting to its QUIT.
fn session(stream: TcpStream, store: &Shared, ctx: &egui::Context) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    let mut out = stream.try_clone()?;
    let mut input = BufReader::new(stream);
    let reply = |out: &mut TcpStream, text: &str| out.write_all(format!("{text}\r\n").as_bytes());
    reply(&mut out, "220 ronnie ESMTP mail catcher")?;
    let (mut from, mut to): (Option<String>, Vec<String>) = (None, Vec::new());
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.by_ref().take(64 * 1024).read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        let (verb, arg) = text.split_once(' ').unwrap_or((text, ""));
        match verb.to_ascii_uppercase().as_str() {
            "EHLO" => reply(&mut out, &format!("250-ronnie\r\n250-SIZE {MAX_SIZE}\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250-PIPELINING\r\n250 AUTH PLAIN LOGIN CRAM-MD5"))?,
            "HELO" => reply(&mut out, "250 ronnie")?,
            // Any login will do: it's only about catching the mails.
            "AUTH" => {
                let mut parts = arg.split_whitespace();
                let mechanism = parts.next().unwrap_or_default().to_ascii_uppercase();
                let initial = parts.next().is_some();
                let prompts = match mechanism.as_str() {
                    "PLAIN" if initial => 0,
                    "PLAIN" => 1,
                    "LOGIN" if initial => 1,
                    "LOGIN" => 2,
                    "CRAM-MD5" => 1,
                    _ => {
                        reply(&mut out, "504 Unrecognized authentication type")?;
                        continue;
                    }
                };
                for k in 0..prompts {
                    let challenge = match (mechanism.as_str(), k, initial) {
                        ("LOGIN", 0, false) => "VXNlcm5hbWU6",
                        ("LOGIN", _, _) => "UGFzc3dvcmQ6",
                        ("CRAM-MD5", _, _) => "PDEyMzQ1QHJvbm5pZT4=",
                        _ => "",
                    };
                    reply(&mut out, &format!("334 {challenge}"))?;
                    line.clear();
                    if input.by_ref().take(64 * 1024).read_until(b'\n', &mut line)? == 0 {
                        return Ok(());
                    }
                }
                reply(&mut out, "235 Authentication successful")?;
            }
            "MAIL" => {
                from = Some(path_arg(arg.split_once(':').map_or(arg, |(_, a)| a)));
                to.clear();
                reply(&mut out, "250 OK")?;
            }
            "RCPT" => {
                if from.is_none() {
                    reply(&mut out, "503 MAIL first")?;
                } else {
                    to.push(path_arg(arg.split_once(':').map_or(arg, |(_, a)| a)));
                    reply(&mut out, "250 OK")?;
                }
            }
            "DATA" => {
                if from.is_none() || to.is_empty() {
                    reply(&mut out, "503 RCPT first")?;
                    continue;
                }
                reply(&mut out, "354 End data with <CR><LF>.<CR><LF>")?;
                let mut data = Vec::new();
                let mut too_big = false;
                loop {
                    line.clear();
                    if input.read_until(b'\n', &mut line)? == 0 {
                        return Ok(());
                    }
                    if line == b".\r\n" || line == b".\n" {
                        break;
                    }
                    // A line starting with a dot had one more added (RFC 5321 4.5.2).
                    let start = usize::from(line.first() == Some(&b'.'));
                    if data.len() + line.len() > MAX_SIZE {
                        too_big = true;
                    } else {
                        data.extend_from_slice(&line[start..]);
                    }
                }
                if too_big {
                    reply(&mut out, "552 Message too big")?;
                } else {
                    match keep(&data, from.as_deref().unwrap_or_default(), &to, store) {
                        Ok(id) => reply(&mut out, &format!("250 OK: queued as {id}"))?,
                        Err(e) => {
                            crate::log::info(&format!("smtp: mail not kept: {e:#}"));
                            reply(&mut out, "451 Could not store the message")?;
                        }
                    }
                    ctx.request_repaint();
                }
                from = None;
                to.clear();
            }
            "RSET" => {
                from = None;
                to.clear();
                reply(&mut out, "250 OK")?;
            }
            "NOOP" => reply(&mut out, "250 OK")?,
            "VRFY" => reply(&mut out, "252 Cannot VRFY user, but will accept message")?,
            "STARTTLS" => reply(&mut out, "454 TLS not available")?,
            "QUIT" => {
                reply(&mut out, "221 Bye")?;
                return Ok(());
            }
            "" => {}
            _ => reply(&mut out, "502 Command not implemented")?,
        }
    }
}

/// Writes a mail received and adds it to the list.
fn keep(raw: &[u8], from: &str, to: &[String], store: &Shared) -> Result<String> {
    let dir = messages_dir().context("no config folder")?;
    std::fs::create_dir_all(&dir)?;
    let id = format!("{}-{}", chrono::Utc::now().format("%Y%m%d%H%M%S%3f"), &uuid::Uuid::new_v4().simple().to_string()[..8]);
    std::fs::write(dir.join(format!("{id}.eml")), raw)?;
    let summary = summarize(id.clone(), raw, from, to);
    store.lock().unwrap().add(summary);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_session() {
        let dir = std::env::temp_dir().join(format!("ronnie-mail-test-{}", std::process::id()));
        // SAFETY: the only test touching the environment.
        unsafe { std::env::set_var("RONNIE_CONFIG_DIR", &dir) };
        let store = Store::load();
        let port = 20000 + (std::process::id() % 20000) as u16;
        let server = start(port, store.clone(), egui::Context::default()).unwrap();
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut line = String::new();
        let mut expect = |r: &mut BufReader<TcpStream>, code: &str| loop {
            line.clear();
            r.read_line(&mut line).unwrap();
            assert!(line.starts_with(code), "{line}");
            if line.as_bytes().get(3) != Some(&b'-') {
                break;
            }
        };
        expect(&mut r, "220");
        for (send, code) in [("EHLO test", "250"), ("AUTH PLAIN AGFiYwBkZWY=", "235"), ("MAIL FROM:<app@example.com>", "250"), ("RCPT TO:<bob@example.com>", "250"), ("DATA", "354")] {
            s.write_all(format!("{send}\r\n").as_bytes()).unwrap();
            expect(&mut r, code);
        }
        s.write_all(b"From: =?utf-8?q?App_=C3=A9t=C3=A9?= <app@example.com>\r\nTo: bob@example.com\r\nSubject: Hello\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHi Bob\r\n..dot\r\n.\r\n").unwrap();
        expect(&mut r, "250");
        s.write_all(b"QUIT\r\n").unwrap();
        expect(&mut r, "221");
        let store = store.lock().unwrap();
        let mail = &store.mails[0];
        assert_eq!(mail.subject, "Hello");
        assert_eq!(mail.from, "App été <app@example.com>");
        assert_eq!(mail.to, vec!["bob@example.com".to_owned()]);
        assert_eq!(mail.text, "Hi Bob .dot");
        drop(server);
        let _ = std::fs::remove_dir_all(dir);
    }
}
