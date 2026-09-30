//! SQL in the database view: laid out (formatting), colored as it is typed, and remembered — every
//! query run and every change made through the interface, with when, where and how it went.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::*;

/// The query laid out: one clause per line, keywords in capitals. Only spaces and case change, so it
/// runs the same.
pub(super) fn format(sql: &str) -> String {
    let options = sqlformat::FormatOptions {
        uppercase: Some(true),
        max_inline_arguments: Some(80),
        max_inline_top_level: Some(80),
        joins_as_top_level: true,
        ..Default::default()
    };
    let out = sqlformat::format(sql, &sqlformat::QueryParams::None, &options);
    // MySQL accounts ('user'@'host') lose a space the formatter puts before "@", and "GRANT SELECT"
    // stays on one line.
    let out = out.replace("' @'", "'@'").replace("` @`", "`@`").replace("' @`", "'@`").replace("` @'", "`@'");
    let mut lines: Vec<String> = Vec::new();
    for line in out.lines() {
        match lines.last_mut() {
            Some(last) if matches!(last.trim(), "GRANT" | "REVOKE") => {
                last.push(' ');
                last.push_str(line.trim_start());
            }
            _ => lines.push(line.to_owned()),
        }
    }
    lines.join("\n")
}

/// SQL colored like in the file editor.
pub(super) fn highlight(text: &str, theme: &Theme, font: &FontId) -> egui::text::LayoutJob {
    super::editor::highlight(text, super::editor::Syntax::detect("query.sql"), theme, font)
}

/// A layouter for a TextEdit holding SQL (colors rebuilt only when the text changes).
pub(super) fn layouter<'a>(theme: &'a Theme, font: FontId, cache: &'a mut Option<(String, egui::text::LayoutJob)>) -> impl FnMut(&Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley> + 'a {
    move |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let text = buf.as_str();
        if cache.as_ref().is_none_or(|(t, _)| t != text) {
            *cache = Some((text.to_owned(), highlight(text, theme, &font)));
        }
        let mut job = cache.as_ref().map(|(_, j)| j.clone()).unwrap_or_default();
        job.wrap.max_width = wrap_width;
        ui.painter().layout_job(job)
    }
}

/// How a query or change ended.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    /// Rows read (the last result with columns).
    Rows(u64),
    /// Rows changed.
    Affected(u64),
    Error(String),
}

/// A query run, or a change made through the interface.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub(super) struct Entry {
    /// Tells the entry apart while it runs (its outcome comes later).
    #[serde(default)]
    pub id: u64,
    pub sql: String,
    /// When (seconds since 1970); 0 for queries remembered before dates were.
    #[serde(default)]
    pub at: i64,
    /// The database selected then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    /// Made through the interface (a cell edited, rows deleted...), not typed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ui: bool,
}

impl Entry {
    pub fn new(sql: &str, db: Option<String>, ui: bool) -> Self {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        Self { id: now.as_nanos() as u64, sql: hide_passwords(sql.trim()), at: now.as_secs() as i64, db, ms: None, outcome: None, ui }
    }
}

/// Passwords aren't written to the history file: in a statement, every quoted value after IDENTIFIED or
/// PASSWORD (IDENTIFIED WITH plugin BY '…', SET PASSWORD FOR u = '…', PASSWORD('…')) is hidden.
fn hide_passwords(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    // Up to where `sql` was copied to `out`.
    let mut rest = 0;
    let mut secret = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"' | b'`') => {
                // The literal's end: a quote not doubled nor escaped.
                let mut j = i + 1;
                while j < bytes.len() {
                    match bytes[j] {
                        b'\\' if quote != b'`' => j += 2,
                        b if b == quote && bytes.get(j + 1) == Some(&quote) => j += 2,
                        b if b == quote => break,
                        _ => j += 1,
                    }
                }
                let end = (j + 1).min(bytes.len());
                // Names in backquotes are no secret.
                if secret && quote != b'`' {
                    out.push_str(&sql[rest..i]);
                    out.push(quote as char);
                    out.push('…');
                    out.push(quote as char);
                    rest = end;
                }
                i = end;
            }
            b';' => {
                secret = false;
                i += 1;
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &sql[start..i];
                secret |= word.eq_ignore_ascii_case("IDENTIFIED") || word.eq_ignore_ascii_case("PASSWORD");
            }
            _ => i += 1,
        }
    }
    out.push_str(&sql[rest..]);
    out
}

/// Entries remembered per connection.
pub(super) const HISTORY_MAX: usize = 500;

/// Before 0.9.2, only the queries' text was kept.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Entry(Entry),
    Old(String),
}

fn history_path() -> Option<std::path::PathBuf> {
    config::config_dir().map(|d| d.join("db-history.json"))
}

fn read_all() -> HashMap<uuid::Uuid, Vec<Entry>> {
    let all: HashMap<uuid::Uuid, Vec<Stored>> = history_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    all.into_iter()
        .map(|(id, entries)| {
            let entries = entries
                .into_iter()
                .map(|e| match e {
                    Stored::Entry(e) => e,
                    Stored::Old(sql) => Entry { id: 0, sql, at: 0, db: None, ms: None, outcome: None, ui: false },
                })
                .collect();
            (id, entries)
        })
        .collect()
}

/// The history of connection `id`, the latest first.
pub(super) fn load(id: uuid::Uuid) -> Vec<Entry> {
    read_all().remove(&id).unwrap_or_default()
}

pub(super) fn save(id: uuid::Uuid, history: &[Entry]) {
    let Some(path) = history_path() else { return };
    let mut all = read_all();
    if history.is_empty() {
        all.remove(&id);
    } else {
        all.insert(id, history.to_vec());
    }
    let _ = config::save(&path, &all);
}

/// What was picked in the history.
pub(super) enum Pick {
    Load(String),
    Clear,
}

/// The history button and its list: searched, each entry with when, where and how it went.
pub(super) fn history_menu(ui: &mut Ui, history: &[Entry], filter: &mut String, theme: &Theme, t: &Strings) -> Option<Pick> {
    use chrono::TimeZone as _;
    let mut picked = None;
    let button = ui.add_enabled(!history.is_empty(), egui::Button::new("🕘").frame_when_inactive(false)).on_hover_text(t.db_history);
    egui::Popup::menu(&button).width(560.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t.db_history).size(13.5).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(t.db_history_clear).size(12.0).color(theme.ansi[1])).frame_when_inactive(false)).clicked() {
                    picked = Some(Pick::Clear);
                    ui.close();
                }
            });
        });
        ui.add(egui::TextEdit::singleline(filter).hint_text(t.db_history_search).desired_width(f32::INFINITY).margin(Vec2::new(6.0, 4.0)));
        ui.add_space(4.0);
        let needle = filter.trim().to_lowercase();
        let font = FontId::monospace(12.0);
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            let shown = history.iter().filter(|e| needle.is_empty() || e.sql.to_lowercase().contains(&needle) || e.db.as_deref().is_some_and(|d| d.to_lowercase().contains(&needle)));
            for entry in shown.take(200) {
                let line: String = entry.sql.split_whitespace().collect::<Vec<_>>().join(" ");
                let short: String = if line.chars().count() > 90 { format!("{}…", line.chars().take(90).collect::<String>()) } else { line };
                let mut job = highlight(&short, theme, &font);
                job.wrap.max_width = f32::INFINITY;
                // When, where, how it went.
                let mut meta = Vec::new();
                if entry.at > 0 {
                    if let Some(d) = chrono::Local.timestamp_opt(entry.at, 0).single() {
                        meta.push(d.format("%d/%m %H:%M").to_string());
                    }
                }
                if let Some(d) = &entry.db {
                    meta.push(d.clone());
                }
                if entry.ui {
                    meta.push(t.db_history_ui.to_owned());
                }
                let (status, color) = match &entry.outcome {
                    Some(Outcome::Rows(n)) => (t.db_result_rows.replace("{n}", &n.to_string()), theme.ansi[2]),
                    Some(Outcome::Affected(n)) => (t.db_affected.replace("{n}", &n.to_string()), theme.ansi[2]),
                    Some(Outcome::Error(_)) => (t.db_history_failed.to_owned(), theme.ansi[1]),
                    None => (String::new(), theme.text_muted),
                };
                let ms = entry.ms.map(|ms| format!("{ms} ms")).unwrap_or_default();
                let response = ui
                    .scope(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(meta.join("  ·  ")).size(11.5).color(theme.text_muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(ms).size(11.5).color(theme.text_muted));
                                ui.label(egui::RichText::new(status).size(11.5).color(color));
                            });
                        });
                        ui.add(egui::Label::new(job).truncate().selectable(false));
                    })
                    .response
                    .interact(Sense::click());
                if response.hovered() {
                    ui.painter().rect_stroke(response.rect.expand(3.0), 4.0, Stroke::new(1.0, theme.accent.gamma_multiply(0.6)), egui::StrokeKind::Outside);
                }
                let hover = match &entry.outcome {
                    Some(Outcome::Error(e)) => format!("{}\n\n{e}", entry.sql),
                    _ => entry.sql.clone(),
                };
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(egui::RichText::new(hover).monospace().size(12.0)).clicked() {
                    picked = Some(Pick::Load(entry.sql.clone()));
                    ui.close();
                }
                ui.separator();
            }
        });
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mysql() {
        assert_eq!(format("select a, b from `t` where id = 1"), "SELECT a, b\nFROM `t`\nWHERE id = 1");
        let user = format("CREATE USER 'bob'@'%' IDENTIFIED BY 'x';\nGRANT SELECT ON `db`.* TO 'bob'@'%'");
        assert!(user.contains("'bob'@'%'") && user.contains("GRANT SELECT ON"), "{user}");
    }

    #[test]
    fn hides_passwords() {
        assert_eq!(hide_passwords("CREATE USER 'a'@'%' IDENTIFIED BY 'p''w\\'d'; GRANT ALL"), "CREATE USER 'a'@'%' IDENTIFIED BY '…'; GRANT ALL");
        assert_eq!(hide_passwords("alter user 'a'@'%' identified by 'x'"), "alter user 'a'@'%' identified by '…'");
        assert_eq!(hide_passwords("SELECT 1"), "SELECT 1");
        assert_eq!(
            hide_passwords("CREATE USER 'a'@'%' IDENTIFIED WITH caching_sha2_password BY \"x\"; SELECT 'b'"),
            "CREATE USER 'a'@'%' IDENTIFIED WITH caching_sha2_password BY \"…\"; SELECT 'b'"
        );
        assert_eq!(hide_passwords("ALTER USER a IDENTIFIED\n  BY 'x'"), "ALTER USER a IDENTIFIED\n  BY '…'");
        assert_eq!(hide_passwords("SET PASSWORD FOR `a`@`%` = 'x'"), "SET PASSWORD FOR `a`@`%` = '…'");
        assert_eq!(hide_passwords("SELECT PASSWORD('x'), 'é'"), "SELECT PASSWORD('…'), '…'");
        // A column named like it isn't the keyword.
        assert_eq!(hide_passwords("SELECT password_hash FROM u WHERE n = 'x'"), "SELECT password_hash FROM u WHERE n = 'x'");
    }

    #[test]
    fn reads_old_history() {
        let old: Vec<Stored> = serde_json::from_str(r#"["SELECT 1", {"id": 3, "sql": "SELECT 2", "at": 5, "ui": true}]"#).unwrap();
        assert!(matches!(&old[0], Stored::Old(s) if s == "SELECT 1"));
        assert!(matches!(&old[1], Stored::Entry(e) if e.sql == "SELECT 2" && e.ui));
    }
}
