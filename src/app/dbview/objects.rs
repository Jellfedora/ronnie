//! Procedures, functions and triggers of a database: listed beside their code, which is edited and
//! written again safely (MySQL: the new version tried under another name first, the old one put back
//! if the server refuses it; SQL Server: ALTER), run with values for their parameters, dropped, and
//! for SQL Server triggers, turned off and on.

use std::ops::Range;

use super::*;
use super::form::{self, Form, FormParam};
use crate::app::sqlcomplete;
use crate::db::{ObjectKind, Routine};

/// Which objects a page lists.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Group {
    Routines,
    Triggers,
}

/// A procedure, function or trigger open in the editor: as a form, or as its CREATE statement.
pub(super) struct CodeEdit {
    pub db: String,
    pub kind: ObjectKind,
    /// None: a new one, not created yet.
    pub name: Option<String>,
    /// As the server gave it (empty while read, or when it can't be), and as a form.
    pub original: String,
    original_form: Option<Form>,
    /// The CREATE statement, edited when `sql_mode`; the form otherwise (None: it can't be one).
    pub text: String,
    form: Option<Form>,
    sql_mode: bool,
    pub loading: bool,
    /// Sent to the server, not answered yet.
    pub saving: bool,
    colors: Option<(String, egui::text::LayoutJob)>,
    complete: sqlcomplete::Completer,
    focus: bool,
}

impl CodeEdit {
    fn existing(db: &str, kind: ObjectKind, name: &str) -> Self {
        Self {
            db: db.to_owned(),
            kind,
            name: Some(name.to_owned()),
            original: String::new(),
            original_form: None,
            text: String::new(),
            form: None,
            sql_mode: false,
            loading: true,
            saving: false,
            colors: None,
            complete: Default::default(),
            focus: false,
        }
    }

    /// A new one, as a form.
    fn new(db: &str, form: Form) -> Self {
        Self { kind: form.kind, name: None, loading: false, form: Some(form), focus: true, ..Self::existing(db, ObjectKind::Procedure, "") }
    }

    /// Its code as the server gave it: as a form when it reads as one.
    pub fn loaded(&mut self, sql: String, mssql: bool) {
        let clean = self.loading || !self.dirty();
        let parsed = form::parse(&sql, mssql);
        if clean {
            self.text = sql.clone();
            self.form = parsed.clone();
            self.sql_mode = parsed.is_none();
        }
        self.original = sql;
        self.original_form = parsed;
        self.loading = false;
    }

    /// Saved: what it is now is the server's.
    pub fn saved(&mut self, dialect: Dialect) {
        self.original = self.sql(dialect);
        self.text = self.original.clone();
        self.original_form = self.form.clone();
        self.saving = false;
    }

    /// The CREATE statement, from the form or as typed.
    fn sql(&self, dialect: Dialect) -> String {
        match &self.form {
            Some(f) if !self.sql_mode => f.sql(dialect),
            _ => self.text.clone(),
        }
    }

    /// Changed since read (a new one: always).
    pub fn dirty(&self) -> bool {
        if self.loading {
            return false;
        }
        self.name.is_none() || if self.sql_mode || self.form.is_none() { self.text != self.original } else { self.form != self.original_form }
    }

    /// The server didn't give its code.
    fn unreadable(&self) -> bool {
        self.name.is_some() && !self.loading && self.original.is_empty()
    }
}

/// A window of these pages.
pub(super) enum ObjectDialog {
    /// A routine run: a value (or NULL) for each of its parameters.
    Run { db: String, routine: Box<Routine>, values: Vec<(String, bool)> },
    /// Changes not saved, before opening another one.
    Discard(Box<CodeEdit>),
}

/// What was done in a list or the editor, applied once drawn.
enum Act {
    Open(ObjectKind, String),
    New(ObjectKind),
    Run(String),
    Drop(ObjectKind, String),
    /// A SQL Server trigger turned on (true) or off, with its table.
    Enable(String, String, bool),
    Save,
    Revert,
    Duplicate,
    ToSql,
    /// The form (false) or the SQL (true).
    Mode(bool),
}

/// The start of a CREATE: what it creates, where the words CREATE [OR REPLACE | OR ALTER] are, and
/// its name (the whole of it, and its parts unquoted).
#[derive(Debug, PartialEq)]
pub(super) struct Head {
    pub kind: ObjectKind,
    pub create: Range<usize>,
    pub name: Range<usize>,
    pub parts: Vec<String>,
}

/// Reads the start of a CREATE PROCEDURE, FUNCTION or TRIGGER (after comments, a DEFINER...).
pub(super) fn head(sql: &str) -> Option<Head> {
    let b = sql.as_bytes();
    let skip = |i: &mut usize| loop {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
        let rest = &sql[*i..];
        if rest.starts_with("--") || rest.starts_with('#') {
            *i = rest.find('\n').map_or(b.len(), |n| *i + n + 1);
        } else if let Some(inside) = rest.strip_prefix("/*") {
            *i = inside.find("*/").map_or(b.len(), |n| *i + n + 4);
        } else {
            break;
        }
    };
    let word = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && (b[*i].is_ascii_alphanumeric() || b[*i] == b'_') {
            *i += 1;
        }
        sql[start..*i].to_ascii_uppercase()
    };
    let mut i = 0;
    skip(&mut i);
    let start = i;
    if word(&mut i) != "CREATE" {
        return None;
    }
    let mut create = start..i;
    let kind = loop {
        skip(&mut i);
        match word(&mut i).as_str() {
            "OR" => {
                skip(&mut i);
                if !matches!(word(&mut i).as_str(), "REPLACE" | "ALTER") {
                    return None;
                }
                create.end = i;
            }
            "DEFINER" => {
                skip(&mut i);
                if b.get(i) != Some(&b'=') {
                    return None;
                }
                i += 1;
                skip(&mut i);
                // `user`@`host`, 'user'@'%', CURRENT_USER...
                let mut quote = None;
                while i < b.len() {
                    match (quote, b[i]) {
                        (Some(q), c) if c == q => quote = None,
                        (Some(_), _) => {}
                        (None, c @ (b'`' | b'\'' | b'"')) => quote = Some(c),
                        (None, c) if c.is_ascii_whitespace() => break,
                        _ => {}
                    }
                    i += 1;
                }
            }
            "SQL" => {
                skip(&mut i);
                word(&mut i);
                skip(&mut i);
                word(&mut i);
            }
            "AGGREGATE" => {}
            "PROCEDURE" | "PROC" => break ObjectKind::Procedure,
            "FUNCTION" => break ObjectKind::Function,
            "TRIGGER" => break ObjectKind::Trigger,
            _ => return None,
        }
    };
    skip(&mut i);
    let back = i;
    if word(&mut i) == "IF" {
        skip(&mut i);
        word(&mut i);
        skip(&mut i);
        word(&mut i);
        skip(&mut i);
    } else {
        i = back;
    }
    let name_start = i;
    let mut parts = Vec::new();
    loop {
        let close = match b.get(i) {
            Some(b'`') => Some('`'),
            Some(b'"') => Some('"'),
            Some(b'[') => Some(']'),
            _ => None,
        };
        match close {
            Some(c) => {
                let end = sql[i + 1..].find(c)? + i + 1;
                parts.push(sql[i + 1..end].to_owned());
                i = end + 1;
            }
            None => {
                let s = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'$' | b'#') || b[i] >= 0x80) {
                    i += 1;
                }
                if i == s {
                    return None;
                }
                parts.push(sql[s..i].to_owned());
            }
        }
        if b.get(i) == Some(&b'.') {
            i += 1;
        } else {
            break;
        }
    }
    Some(Head { kind, create, name: name_start..i, parts })
}

/// The text without its `DELIMITER` lines (a command of the mysql client, unknown to the server)
/// and the delimiter they set at its end.
pub(super) fn without_delimiters(sql: &str) -> String {
    let mut custom: Option<String> = None;
    let mut lines = Vec::new();
    for line in sql.lines() {
        let word = line.trim();
        if word.get(..10).is_some_and(|w| w.eq_ignore_ascii_case("DELIMITER ")) {
            let d = word[10..].trim();
            if d != ";" {
                custom = Some(d.to_owned());
            }
            continue;
        }
        lines.push(line);
    }
    let mut out = lines.join("\n").trim().to_owned();
    if let Some(d) = custom {
        while let Some(s) = out.strip_suffix(d.as_str()) {
            out = s.trim_end().to_owned();
        }
    }
    out.trim_end_matches(';').trim_end().to_owned()
}

/// A routine or trigger's name as listed: MySQL its name alone, SQL Server `schema.name`
/// (`default_schema` when none is written).
fn listed_name(h: &Head, mssql: bool, default_schema: &str) -> String {
    let last = h.parts.last().cloned().unwrap_or_default();
    match (mssql, h.parts.len()) {
        (false, _) => last,
        (true, 1) => format!("{default_schema}.{last}"),
        (true, n) => format!("{}.{last}", h.parts[n - 2]),
    }
}

/// A type of numbers: values given for it aren't quoted.
fn numeric(kind: &str) -> bool {
    let k = kind.trim().to_ascii_lowercase();
    ["int", "tinyint", "smallint", "mediumint", "bigint", "decimal", "numeric", "float", "double", "real", "bit", "money", "smallmoney"].iter().any(|n| k == *n || k.starts_with(&format!("{n}(")) || k.starts_with(&format!("{n} ")))
}

/// A value typed for a parameter, as SQL.
fn value_sql(dialect: Dialect, kind: &str, text: &str, null: bool) -> String {
    if null || (text.trim().is_empty() && numeric(kind)) {
        "NULL".into()
    } else if numeric(kind) && text.trim().parse::<f64>().is_ok() {
        text.trim().to_owned()
    } else {
        dialect.literal(text)
    }
}

/// The SQL that runs `r` with these values (one per parameter, OUT ones' read back after).
pub(super) fn run_sql(dialect: Dialect, db: &str, r: &Routine, values: &[(String, bool)]) -> String {
    let value = |i: usize| {
        let (text, null) = values.get(i).cloned().unwrap_or_default();
        value_sql(dialect, &r.params[i].kind, &text, null)
    };
    let out = |i: usize| r.params[i].mode.contains("OUT");
    if dialect.mssql() {
        let name = dialect.local_table(&r.name);
        if r.function {
            let args: Vec<String> = (0..r.params.len()).map(value).collect();
            return match r.code.as_str() {
                "IF" | "TF" | "FT" => format!("SELECT * FROM {name}({})", args.join(", ")),
                _ => format!("SELECT {name}({}) AS {}", args.join(", "), dialect.ident("result")),
            };
        }
        let mut lines = Vec::new();
        for (i, p) in r.params.iter().enumerate().filter(|(i, _)| out(*i)) {
            lines.push(format!("DECLARE {} {} = {};", p.name, p.kind, value(i)));
        }
        let args: Vec<String> = r.params.iter().enumerate().map(|(i, p)| if out(i) { format!("{0} = {0} OUTPUT", p.name) } else { format!("{} = {}", p.name, value(i)) }).collect();
        lines.push(format!("EXEC {}{}{};", name, if args.is_empty() { "" } else { " " }, args.join(", ")));
        let outs: Vec<String> = r.params.iter().enumerate().filter(|(i, _)| out(*i)).map(|(_, p)| format!("{} AS {}", p.name, dialect.ident(&p.name))).collect();
        if !outs.is_empty() {
            lines.push(format!("SELECT {};", outs.join(", ")));
        }
        return lines.join("\n");
    }
    let name = dialect.table(db, &r.name);
    if r.function {
        let args: Vec<String> = (0..r.params.len()).map(value).collect();
        return format!("SELECT {name}({}) AS {}", args.join(", "), dialect.ident(&r.name));
    }
    let var = |p: &db::Param| format!("@{}", dialect.ident(&p.name));
    let mut lines = Vec::new();
    for (i, p) in r.params.iter().enumerate().filter(|(i, p)| out(*i) && p.mode.starts_with("IN")) {
        lines.push(format!("SET {} = {};", var(p), value(i)));
    }
    let args: Vec<String> = r.params.iter().enumerate().map(|(i, p)| if out(i) { var(p) } else { value(i) }).collect();
    lines.push(format!("CALL {name}({});", args.join(", ")));
    let outs: Vec<String> = r.params.iter().enumerate().filter(|(i, _)| out(*i)).map(|(_, p)| format!("{} AS {}", var(p), dialect.ident(&p.name))).collect();
    if !outs.is_empty() {
        lines.push(format!("SELECT {};", outs.join(", ")));
    }
    lines.join("\n")
}

/// A routine's parameters as written: `(IN id INT, OUT n INT)`, and what a function returns.
fn signature(r: &Routine) -> String {
    let params: Vec<String> = r.params.iter().map(|p| if r.function || p.mode.is_empty() || p.mode == "IN" { format!("{} {}", p.name, p.kind) } else { format!("{} {} {}", p.mode, p.name, p.kind) }).collect();
    let mut s = format!("({})", params.join(", "));
    if r.function && !r.returns.is_empty() {
        s.push_str(&format!(" → {}", r.returns));
    }
    s
}

/// A type typed, or picked in the list beside it.
fn type_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, kind: &mut String, types: &[&str]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add(egui::TextEdit::singleline(kind).desired_width(110.0).font(FontId::monospace(12.5)).margin(Vec2::new(6.0, 4.0)));
        egui::ComboBox::from_id_salt(id).selected_text("").width(22.0).show_ui(ui, |ui| {
            for &k in types {
                if ui.selectable_label(kind.eq_ignore_ascii_case(k), egui::RichText::new(k).monospace()).clicked() {
                    *kind = k.to_owned();
                }
            }
        });
    });
}

/// MySQL type options (UNSIGNED, CHARSET...), typed or picked.
fn option_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, options: &mut String) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add(egui::TextEdit::singleline(options).desired_width(120.0).font(FontId::monospace(12.5)).margin(Vec2::new(6.0, 4.0)));
        egui::ComboBox::from_id_salt(id).selected_text("").width(22.0).show_ui(ui, |ui| {
            for o in form::MYSQL_OPTIONS {
                if ui.selectable_label(*options == o, egui::RichText::new(if o.is_empty() { "—" } else { o }).monospace()).clicked() {
                    *options = o.to_owned();
                }
            }
        });
    });
}

/// A letter in a colored square: P, F or T.
fn paint_badge(painter: &egui::Painter, c: Pos2, kind: ObjectKind, theme: &Theme) {
    badge(painter, c, kind, theme, 18.0);
}

/// The same, small (in the tree).
pub(super) fn paint_mini_badge(painter: &egui::Painter, c: Pos2, kind: ObjectKind, theme: &Theme) {
    badge(painter, c, kind, theme, 14.0);
}

fn badge(painter: &egui::Painter, c: Pos2, kind: ObjectKind, theme: &Theme, size: f32) {
    let (letter, color) = match kind {
        ObjectKind::Procedure => ("P", theme.accent),
        ObjectKind::Function => ("F", theme.ansi[5]),
        ObjectKind::Trigger => ("T", theme.ansi[3]),
    };
    painter.rect_filled(Rect::from_center_size(c, Vec2::splat(size)), size * 0.28, color.gamma_multiply(0.18));
    painter.text(c, Align2::CENTER_CENTER, letter, FontId::monospace(size * 0.6), color);
}

/// One line of a list.
struct Item {
    kind: ObjectKind,
    name: String,
    sub: String,
    /// A SQL Server trigger turned off, and its table.
    off: bool,
    table: String,
}

impl DbView {
    /// Procedures and functions, or triggers (of the table shown, if any).
    pub(super) fn objects_page(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings, group: Group) {
        let Some(d) = self.db.clone() else { return };
        let table = self.table.clone();
        let mssql = self.dialect.mssql();
        let mut act: Option<Act> = None;
        if self.code.as_ref().is_some_and(|c| c.db == d) && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S))) {
            act = Some(Act::Save);
        }
        let items: Option<Vec<Item>> = match group {
            Group::Routines => self.routines.get(&d).map(|rs| {
                rs.iter()
                    .map(|r| Item { kind: if r.function { ObjectKind::Function } else { ObjectKind::Procedure }, name: r.name.clone(), sub: signature(r), off: false, table: String::new() })
                    .collect()
            }),
            Group::Triggers => self.triggers.get(&d).map(|ts| {
                ts.iter()
                    .filter(|x| table.as_ref().is_none_or(|tb| x.table == *tb))
                    .map(|x| {
                        let on = if table.is_some() { String::new() } else { format!("  ·  {}", x.table) };
                        Item { kind: ObjectKind::Trigger, name: x.name.clone(), sub: format!("{} {}{on}", x.timing, x.events), off: !x.enabled, table: x.table.clone() }
                    })
                    .collect()
            }),
        };
        let filter = self.object_filter.to_lowercase();
        // The toolbar: how many, the filter, what can be added.
        ui.horizontal(|ui| {
            let n = items.as_ref().map_or(0, Vec::len);
            let count = match group {
                Group::Routines => t.db_routines_count,
                Group::Triggers => t.db_triggers_count,
            };
            ui.label(egui::RichText::new(count.replace("{n}", &n.to_string())).size(12.5).color(theme.text_muted));
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut self.object_filter).hint_text(t.db_filter).desired_width(180.0).margin(Vec2::new(8.0, 4.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let accent = |label: &str| egui::Button::new(egui::RichText::new(format!("+  {label}")).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0);
                let ready = matches!(self.status, Status::Ready(_));
                match group {
                    Group::Routines => {
                        if ui.add_enabled(ready, accent(t.db_new_function)).clicked() {
                            act = Some(Act::New(ObjectKind::Function));
                        }
                        if ui.add_enabled(ready, accent(t.db_new_procedure)).clicked() {
                            act = Some(Act::New(ObjectKind::Procedure));
                        }
                    }
                    Group::Triggers => {
                        if ui.add_enabled(ready, accent(t.db_new_trigger)).clicked() {
                            act = Some(Act::New(ObjectKind::Trigger));
                        }
                    }
                }
                if ui.button("↻").on_hover_text(t.files_refresh).clicked() {
                    self.send(Request::Routines(d.clone()));
                    self.send(Request::Triggers(d.clone()));
                }
            });
        });
        ui.add_space(8.0);
        let area = ui.available_rect_before_wrap();
        let list_w = (area.width() * 0.3).clamp(210.0, 320.0);
        let left = Rect::from_min_size(area.min, Vec2::new(list_w, area.height()));
        let right = Rect::from_min_max(Pos2::new(left.max.x + 14.0, area.min.y), area.max);
        ui.allocate_rect(area, Sense::hover());

        // The list.
        ui.painter().rect_filled(left, 8.0, theme.chrome_bg);
        ui.painter().rect_stroke(left, 8.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        let open = self.code.as_ref().filter(|c| c.db == d).and_then(|c| c.name.clone().map(|n| (c.kind, n)));
        let mut list = ui.new_child(egui::UiBuilder::new().max_rect(left.shrink(6.0)).layout(egui::Layout::top_down(egui::Align::Min)));
        match &items {
            None => {
                list.add_space(10.0);
                list.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.spinner();
                });
            }
            Some(items) if items.is_empty() => {
                list.add_space(10.0);
                let empty = match group {
                    Group::Routines => t.db_no_routines,
                    Group::Triggers if table.is_some() => t.db_no_table_triggers,
                    Group::Triggers => t.db_no_triggers,
                };
                list.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(egui::RichText::new(empty).size(12.5).color(theme.text_muted)).wrap());
                });
            }
            Some(items) => {
                egui::ScrollArea::vertical().id_salt(("db-objects", group == Group::Routines)).auto_shrink([false, false]).show(&mut list, |ui| {
                    for it in items.iter().filter(|it| filter.is_empty() || it.name.to_lowercase().contains(&filter) || it.table.to_lowercase().contains(&filter)) {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
                        let selected = open.as_ref().is_some_and(|(k, n)| *k == it.kind && *n == it.name);
                        if selected {
                            ui.painter().rect_filled(r, 6.0, theme.tab_active);
                            ui.painter().rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 10.0), Vec2::new(3.0, 22.0)), 1.5, theme.accent);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 6.0, theme.tab_hover.gamma_multiply(0.7));
                        }
                        paint_badge(ui.painter(), Pos2::new(r.min.x + 18.0, r.center().y), it.kind, theme);
                        let dim = if it.off { 0.5 } else { 1.0 };
                        let mut job = egui::text::LayoutJob::simple_singleline(it.name.clone(), FontId::proportional(13.0), theme.text.gamma_multiply(dim));
                        job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 40.0);
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(Pos2::new(r.min.x + 34.0, r.min.y + 5.0), g, theme.text);
                        let sub = if it.off { format!("{}  ·  {}", t.db_trigger_off, it.sub) } else { it.sub.clone() };
                        let mut job = egui::text::LayoutJob::simple_singleline(sub, FontId::monospace(11.0), theme.text_muted.gamma_multiply(dim));
                        job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 40.0);
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(Pos2::new(r.min.x + 34.0, r.min.y + 23.0), g, theme.text_muted);
                        if resp.clicked() {
                            act = Some(Act::Open(it.kind, it.name.clone()));
                        }
                        resp.context_menu(|ui| {
                            if ui.button(t.edit).clicked() {
                                act = Some(Act::Open(it.kind, it.name.clone()));
                                ui.close();
                            }
                            if it.kind != ObjectKind::Trigger && ui.button(format!("▶  {}", t.db_run_routine)).clicked() {
                                act = Some(Act::Run(it.name.clone()));
                                ui.close();
                            }
                            if mssql && it.kind == ObjectKind::Trigger {
                                let label = if it.off { t.db_trigger_enable } else { t.db_trigger_disable };
                                if ui.button(label).clicked() {
                                    act = Some(Act::Enable(it.name.clone(), it.table.clone(), it.off));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button(egui::RichText::new(t.delete).color(theme.ansi[1])).clicked() {
                                act = Some(Act::Drop(it.kind, it.name.clone()));
                                ui.close();
                            }
                        });
                    }
                });
            }
        }

        // The code of the one open.
        let shown = self.code.as_ref().is_some_and(|c| c.db == d && (c.kind == ObjectKind::Trigger) == (group == Group::Triggers));
        let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::top_down(egui::Align::Min)));
        if shown {
            if let Some(a) = self.code_ui(&mut right_ui, theme, t) {
                act = Some(a);
            }
        } else {
            right_ui.add_space(right.height() * 0.3);
            right_ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(t.db_pick_object).size(13.0).color(theme.text_muted));
            });
        }

        match act {
            Some(Act::Open(kind, name)) => {
                if open.as_ref() != Some(&(kind, name.clone())) {
                    self.switch_code(CodeEdit::existing(&d, kind, &name));
                }
            }
            Some(Act::New(kind)) => self.new_object(&d, table, kind),
            Some(Act::Run(name)) => self.run_dialog(&d, &name),
            Some(Act::Drop(kind, name)) => self.drop_object(&d, kind, &name, t),
            Some(Act::Enable(name, table, on)) => {
                let sql = format!("{} TRIGGER {} ON {}", if on { "ENABLE" } else { "DISABLE" }, self.dialect.local_table(&name), self.dialect.local_table(&table));
                self.change(Change { db: Some(d.clone()), ..Change::new(sql, None) });
            }
            Some(Act::Save) => self.save_code(t),
            Some(Act::Revert) => {
                if let Some(c) = &mut self.code {
                    c.text = c.original.clone();
                    c.form = c.original_form.clone();
                }
            }
            Some(Act::Duplicate) => {
                let dialect = self.dialect;
                if let Some(c) = &self.code {
                    let copy = |name: &str| format!("{name}_{}", t.db_copy_suffix);
                    let form = c.form.clone().filter(|_| !c.sql_mode).or_else(|| form::parse(&c.text, dialect.mssql()));
                    match form {
                        Some(mut f) => {
                            f.name = copy(&f.name);
                            self.switch_code(CodeEdit::new(&d, f));
                        }
                        None => {
                            let mut text = c.text.clone();
                            if let Some(h) = head(&text) {
                                let mut parts = h.parts.clone();
                                if let Some(last) = parts.last_mut() {
                                    *last = copy(last);
                                }
                                let name = parts.iter().map(|p| dialect.ident(p)).collect::<Vec<_>>().join(".");
                                text.replace_range(h.name.clone(), &name);
                            }
                            let mut next = CodeEdit::new(&d, Form::new(c.kind, dialect.mssql(), "", ""));
                            (next.text, next.form, next.sql_mode) = (text, None, true);
                            self.switch_code(next);
                        }
                    }
                }
            }
            Some(Act::ToSql) => {
                if let Some(sql) = self.code.as_ref().map(|c| c.sql(self.dialect)) {
                    self.open_sql_tab(sql, None);
                }
            }
            Some(Act::Mode(sql)) => {
                let dialect = self.dialect;
                let mut unreadable = false;
                if let Some(c) = &mut self.code {
                    if sql && !c.sql_mode {
                        // Unchanged: the server's own text.
                        c.text = if c.form == c.original_form && !c.original.is_empty() { c.original.clone() } else { c.sql(dialect) };
                        c.sql_mode = true;
                    } else if !sql && c.sql_mode {
                        let text = if dialect.mssql() { c.text.clone() } else { without_delimiters(&c.text) };
                        match form::parse(&text, dialect.mssql()) {
                            Some(f) => {
                                c.form = if c.text == c.original { c.original_form.clone() } else { Some(f) };
                                c.sql_mode = false;
                            }
                            None => unreadable = true,
                        }
                    }
                }
                if unreadable {
                    self.notify(format!("⚠  {}", t.db_form_unparsed), true);
                }
            }
            None => {}
        }
    }

    /// The editor of the code open: its name and what it is, its buttons, the code.
    fn code_ui(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) -> Option<Act> {
        let mut act = None;
        let ready = matches!(self.status, Status::Ready(_));
        let mssql = self.dialect.mssql();
        let c = self.code.as_ref()?;
        let routine = self.routines.get(&c.db).and_then(|rs| rs.iter().find(|r| Some(&r.name) == c.name.as_ref() && (r.function == (c.kind == ObjectKind::Function)))).cloned();
        let trigger = self.triggers.get(&c.db).and_then(|ts| ts.iter().find(|x| Some(&x.name) == c.name.as_ref() && c.kind == ObjectKind::Trigger)).cloned();
        let (dirty, saving, fresh, unreadable, kind) = (c.dirty(), c.saving, c.name.is_none(), c.unreadable(), c.form.as_ref().filter(|_| !c.sql_mode).map_or(c.kind, |f| f.kind));
        let (sql_mode, can_form) = (c.sql_mode || c.form.is_none(), !c.loading && !unreadable);
        let ready_form = c.sql_mode || c.form.as_ref().is_none_or(Form::ready);
        let typed_name = c.form.as_ref().filter(|_| fresh && !c.sql_mode).map(|f| f.name.trim().to_owned()).filter(|n| !n.is_empty());
        let title = typed_name.or_else(|| c.name.clone()).unwrap_or_else(|| match kind {
            ObjectKind::Procedure => t.db_new_procedure.to_owned(),
            ObjectKind::Function => t.db_new_function.to_owned(),
            ObjectKind::Trigger => t.db_new_trigger.to_owned(),
        });
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            paint_badge(ui.painter(), r.center(), kind, theme);
            ui.label(egui::RichText::new(&title).size(16.0).strong());
            if dirty && !fresh {
                ui.label(egui::RichText::new(format!("●  {}", t.db_code_modified)).size(11.5).color(theme.accent));
            }
            if saving {
                ui.spinner();
            }
            ui.add_space(8.0);
            // Form or SQL.
            if can_form {
                for (label, sql) in [(t.db_mode_form, false), (t.db_mode_sql, true)] {
                    if ui.add(egui::Button::selectable(sql_mode == sql, egui::RichText::new(label).size(12.0)).corner_radius(5.0)).clicked() && sql_mode != sql {
                        act = Some(Act::Mode(sql));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !fresh && ui.button(egui::RichText::new("🗑").color(theme.ansi[1])).on_hover_text(t.delete).clicked() {
                    act = Some(Act::Drop(kind, title.clone()));
                }
                ui.menu_button("…", |ui| {
                    if ui.button(t.db_copy).clicked() {
                        if let Some(c) = &self.code {
                            ui.ctx().copy_text(c.sql(self.dialect));
                        }
                        ui.close();
                    }
                    if ui.button(t.db_open_in_sql).clicked() {
                        act = Some(Act::ToSql);
                        ui.close();
                    }
                    if ui.button(t.db_duplicate).clicked() {
                        act = Some(Act::Duplicate);
                        ui.close();
                    }
                });
                if let Some(x) = trigger.as_ref().filter(|_| mssql) {
                    let label = if x.enabled { t.db_trigger_disable } else { t.db_trigger_enable };
                    if ui.add_enabled(ready, egui::Button::new(label).corner_radius(6.0)).clicked() {
                        act = Some(Act::Enable(x.name.clone(), x.table.clone(), !x.enabled));
                    }
                }
                if let Some(r) = &routine {
                    let run = ui.add_enabled(ready, egui::Button::new(format!("▶  {}", t.db_run_routine)).corner_radius(6.0));
                    let run = if dirty { run.on_hover_text(t.db_run_saved_hint) } else { run };
                    if run.clicked() {
                        act = Some(Act::Run(r.name.clone()));
                    }
                }
                if dirty && !fresh && ui.button(t.db_revert).clicked() {
                    act = Some(Act::Revert);
                }
                let key = if cfg!(target_os = "macos") { "⌘ S" } else { "Ctrl+S" };
                let save = egui::Button::new(egui::RichText::new(format!("{}   {key}", t.save)).size(12.5).color(theme.bg)).fill(theme.accent).corner_radius(6.0);
                if ui.add_enabled(ready && dirty && !saving && !unreadable && ready_form, save).clicked() {
                    act = Some(Act::Save);
                }
            });
        });
        // What it is.
        let mut meta: Vec<String> = Vec::new();
        if let Some(r) = &routine {
            meta.push(if r.function { t.db_function.to_owned() } else { t.db_procedure.to_owned() });
            if r.deterministic {
                meta.push(t.db_deterministic.to_owned());
            }
            for s in [&r.access, &r.security] {
                if !s.is_empty() {
                    meta.push(s.clone());
                }
            }
            if !r.definer.is_empty() {
                meta.push(r.definer.clone());
            }
            if let Some(d) = [&r.modified, &r.created].into_iter().find(|d| !d.is_empty()) {
                meta.push(t.db_modified_on.replace("{d}", d));
            }
            if !r.comment.is_empty() {
                meta.push(format!("“{}”", r.comment));
            }
        }
        if let Some(x) = &trigger {
            meta.push(format!("{} {} ON {}", x.timing, x.events, x.table));
            if !x.enabled {
                meta.push(t.db_trigger_off.to_owned());
            }
            if x.order > 1 {
                meta.push(format!("#{}", x.order));
            }
            if !x.definer.is_empty() {
                meta.push(x.definer.clone());
            }
            if let Some(d) = [&x.modified, &x.created].into_iter().find(|d| !d.is_empty()) {
                meta.push(t.db_modified_on.replace("{d}", d));
            }
        }
        if !meta.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(meta.join("  ·  ")).size(12.0).color(theme.text_muted)).truncate());
        }
        if unreadable {
            ui.label(egui::RichText::new(format!("⚠  {}", t.db_code_unreadable)).size(12.5).color(theme.ansi[3]));
        }
        ui.add_space(6.0);

        // The form.
        if !sql_mode && can_form {
            self.form_ui(ui, theme, t);
            return act;
        }
        // The code.
        let schema_db = self.code.as_ref().map(|c| c.db.clone());
        let c = self.code.as_mut()?;
        let id = egui::Id::new(("db-code", self.id, &c.db, &c.name, c.kind));
        let mut focused = false;
        Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let height = ui.available_height().max(120.0);
            if c.loading {
                ui.set_min_height(height);
                ui.spinner();
                return;
            }
            c.complete.keys(ui, id, &mut c.text);
            egui::ScrollArea::vertical().id_salt(("db-code-scroll", &c.name)).max_height(height).auto_shrink([false, false]).show(ui, |ui| {
                let out = {
                    let mut layouter = crate::app::sql::layouter(theme, FontId::monospace(13.0), &mut c.colors);
                    egui::TextEdit::multiline(&mut c.text)
                        .id(id)
                        .code_editor()
                        .frame(Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .min_size(Vec2::new(0.0, height - 4.0))
                        .font(FontId::monospace(13.0))
                        .interactive(!c.saving && !unreadable)
                        .layouter(&mut layouter)
                        .show(ui)
                };
                if c.focus {
                    out.response.request_focus();
                    c.focus = false;
                }
                let schema = sqlcomplete::Schema {
                    brackets: mssql,
                    db: schema_db.as_deref(),
                    databases: self.databases.iter().map(|d| d.name.as_str()).collect(),
                    tables: &self.tables,
                    columns: schema_db.as_ref().and_then(|d| self.columns.get(d)),
                    table: self.table.as_deref(),
                };
                c.complete.show(ui, id, &out, &mut c.text, &schema, theme, t);
                focused = out.response.has_focus();
            });
        });
        if focused {
            self.want_columns();
        }
        act
    }

    /// The form of the one open: name, type, parameters, what it returns, its body, its options.
    fn form_ui(&mut self, ui: &mut Ui, theme: &Theme, t: &Strings) {
        let mssql = self.dialect.mssql();
        let tables: Vec<String> = self.code.as_ref().and_then(|c| self.tables.get(&c.db)).map(|ts| ts.iter().filter(|x| !x.view || mssql).map(|x| x.name.clone()).collect()).unwrap_or_default();
        let triggers: Vec<String> = self.code.as_ref().and_then(|c| self.triggers.get(&c.db)).map(|ts| ts.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
        let schema_db = self.code.as_ref().map(|c| c.db.clone());
        let id_base = self.id;
        let dialect = self.dialect;
        let Some(c) = self.code.as_mut() else { return };
        let Some(f) = c.form.as_mut() else { return };
        let editable = !c.saving;
        let types: &[&str] = if mssql { &form::MSSQL_TYPES } else { &form::MYSQL_TYPES };
        let mut focused = false;
        let label = |ui: &mut Ui, text: &str| {
            ui.add_sized(Vec2::new(150.0, 24.0), egui::Label::new(egui::RichText::new(text).size(12.5).color(theme.text_muted)));
        };
        let field = |ui: &mut Ui, text: &mut String, width: f32, hint: &str| ui.add(egui::TextEdit::singleline(text).desired_width(width).font(FontId::monospace(12.5)).hint_text(hint).margin(Vec2::new(6.0, 4.0)));
        egui::ScrollArea::vertical().id_salt(("db-form", &c.name)).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                egui::Grid::new("db-form-grid").num_columns(2).spacing([14.0, 10.0]).min_col_width(150.0).show(ui, |ui| {
                    label(ui, t.db_trigger_name);
                    ui.horizontal(|ui| {
                        if mssql {
                            field(ui, &mut f.schema, 70.0, "dbo");
                            ui.label(".");
                        }
                        let name = field(ui, &mut f.name, 260.0, "");
                        if c.focus {
                            name.request_focus();
                            c.focus = false;
                        }
                    });
                    ui.end_row();
                    if f.kind == ObjectKind::Trigger {
                        label(ui, t.db_col_table);
                        egui::ComboBox::from_id_salt("db-form-table").selected_text(f.table.as_str()).width(260.0).show_ui(ui, |ui| {
                            for x in &tables {
                                ui.selectable_value(&mut f.table, x.clone(), x);
                            }
                        });
                        ui.end_row();
                        label(ui, t.db_trigger_timing);
                        ui.horizontal(|ui| {
                            for w in if mssql { ["AFTER", "INSTEAD OF"] } else { ["BEFORE", "AFTER"] } {
                                if ui.add(egui::Button::selectable(f.timing == w, w).corner_radius(5.0)).clicked() {
                                    f.timing = w.to_owned();
                                }
                            }
                        });
                        ui.end_row();
                        label(ui, t.db_trigger_event);
                        ui.horizontal(|ui| {
                            for (k, w) in form::EVENTS.into_iter().enumerate() {
                                // MySQL: one event per trigger.
                                if ui.add(egui::Button::selectable(f.events[k], w).corner_radius(5.0)).clicked() {
                                    if mssql {
                                        f.events[k] = !f.events[k];
                                    } else {
                                        f.events = [false; 3];
                                        f.events[k] = true;
                                    }
                                }
                            }
                        });
                        ui.end_row();
                        if !mssql {
                            label(ui, t.db_form_order);
                            ui.horizontal(|ui| {
                                let shown = if f.order.is_empty() { "—" } else { f.order.as_str() };
                                egui::ComboBox::from_id_salt("db-form-order").selected_text(shown).width(110.0).show_ui(ui, |ui| {
                                    for (value, text) in [("", "—"), ("FOLLOWS", "FOLLOWS"), ("PRECEDES", "PRECEDES")] {
                                        ui.selectable_value(&mut f.order, value.to_owned(), text);
                                    }
                                });
                                if !f.order.is_empty() {
                                    egui::ComboBox::from_id_salt("db-form-order-of").selected_text(f.order_of.as_str()).width(200.0).show_ui(ui, |ui| {
                                        for x in triggers.iter().filter(|x| **x != f.name) {
                                            ui.selectable_value(&mut f.order_of, x.clone(), x);
                                        }
                                    });
                                }
                            });
                            ui.end_row();
                        }
                    } else {
                        label(ui, t.db_form_type);
                        ui.horizontal(|ui| {
                            for (k, w) in [(ObjectKind::Procedure, "PROCEDURE"), (ObjectKind::Function, "FUNCTION")] {
                                if ui.add(egui::Button::selectable(f.kind == k, w).corner_radius(5.0)).clicked() {
                                    f.kind = k;
                                    if k == ObjectKind::Function && f.returns.is_empty() {
                                        f.returns = "INT".into();
                                    }
                                }
                            }
                        });
                        ui.end_row();
                        label(ui, t.db_form_params);
                        ui.vertical(|ui| {
                            let function = f.kind == ObjectKind::Function;
                            let (mut remove, mut up) = (None, None);
                            let count = f.params.len();
                            if count > 0 {
                                egui::Grid::new("db-form-params").num_columns(8).spacing([6.0, 6.0]).show(ui, |ui| {
                                    let heads = [if function { "" } else { t.db_form_mode }, t.db_trigger_name, t.db_col_type, t.db_form_size, if mssql { t.db_col_default } else { t.db_form_options }, "", "", ""];
                                    for h in heads {
                                        ui.label(egui::RichText::new(h).size(11.5).color(theme.text_muted));
                                    }
                                    ui.end_row();
                                    for (i, p) in f.params.iter_mut().enumerate() {
                                        if function {
                                            ui.label("");
                                        } else {
                                            let modes: &[&str] = if mssql { &["IN", "OUT"] } else { &["IN", "OUT", "INOUT"] };
                                            egui::ComboBox::from_id_salt(("db-form-mode", i)).selected_text(p.mode.as_str()).width(64.0).show_ui(ui, |ui| {
                                                for m in modes {
                                                    ui.selectable_value(&mut p.mode, (*m).to_owned(), *m);
                                                }
                                            });
                                        }
                                        field(ui, &mut p.name, 130.0, "");
                                        type_picker(ui, ("db-form-type", i), &mut p.kind, types);
                                        field(ui, &mut p.size, 70.0, "");
                                        if mssql {
                                            field(ui, &mut p.default, 90.0, "");
                                        } else {
                                            option_picker(ui, ("db-form-opt", i), &mut p.options);
                                        }
                                        if ui.add_enabled(i > 0, egui::Button::new("↑").frame_when_inactive(false)).clicked() {
                                            up = Some(i);
                                        }
                                        if ui.add_enabled(i + 1 < count, egui::Button::new("↓").frame_when_inactive(false)).clicked() {
                                            up = Some(i + 1);
                                        }
                                        if ui.add(egui::Button::new(egui::RichText::new("✕").color(theme.ansi[1])).frame_when_inactive(false)).on_hover_text(t.delete).clicked() {
                                            remove = Some(i);
                                        }
                                        ui.end_row();
                                    }
                                });
                            }
                            if let Some(i) = up {
                                f.params.swap(i - 1, i);
                            }
                            if let Some(i) = remove {
                                f.params.remove(i);
                            }
                            if ui.button(format!("+  {}", t.db_form_add_param)).clicked() {
                                let name = if mssql { "@" } else { "" };
                                f.params.push(FormParam { mode: "IN".into(), name: name.into(), kind: "INT".into(), ..Default::default() });
                            }
                        });
                        ui.end_row();
                        if f.kind == ObjectKind::Function {
                            label(ui, t.db_form_returns);
                            ui.horizontal(|ui| {
                                type_picker(ui, "db-form-returns", &mut f.returns, types);
                                field(ui, &mut f.returns_size, 70.0, t.db_form_size);
                                if !mssql {
                                    option_picker(ui, "db-form-returns-opt", &mut f.returns_options);
                                }
                            });
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new(t.db_form_body).size(12.5).color(theme.text_muted));
                ui.add_space(4.0);
                let id = egui::Id::new(("db-form-body", id_base, &c.db, &c.name));
                Frame::NONE.fill(theme.chrome_bg).stroke(Stroke::new(1.0, theme.tab_hover)).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    c.complete.keys(ui, id, &mut f.body);
                    let out = {
                        let mut layouter = crate::app::sql::layouter(theme, FontId::monospace(13.0), &mut c.colors);
                        egui::TextEdit::multiline(&mut f.body).id(id).code_editor().frame(Frame::NONE).desired_width(f32::INFINITY).desired_rows(10).font(FontId::monospace(13.0)).layouter(&mut layouter).show(ui)
                    };
                    let schema = sqlcomplete::Schema {
                        brackets: mssql,
                        db: schema_db.as_deref(),
                        databases: self.databases.iter().map(|d| d.name.as_str()).collect(),
                        tables: &self.tables,
                        columns: schema_db.as_ref().and_then(|d| self.columns.get(d)),
                        table: self.table.as_deref(),
                    };
                    c.complete.show(ui, id, &out, &mut f.body, &schema, theme, t);
                    focused = out.response.has_focus();
                });
                ui.add_space(10.0);
                egui::Grid::new("db-form-options").num_columns(2).spacing([14.0, 10.0]).min_col_width(150.0).show(ui, |ui| {
                    if mssql {
                        label(ui, t.db_form_with);
                        field(ui, &mut f.with, 320.0, "EXECUTE AS OWNER, SCHEMABINDING…");
                        ui.end_row();
                    } else {
                        label(ui, t.db_form_definer);
                        field(ui, &mut f.definer, 260.0, t.db_form_definer_hint);
                        ui.end_row();
                        if f.kind != ObjectKind::Trigger {
                            label(ui, t.db_form_deterministic);
                            ui.checkbox(&mut f.deterministic, "");
                            ui.end_row();
                            label(ui, t.db_form_security);
                            egui::ComboBox::from_id_salt("db-form-security").selected_text(if f.security.is_empty() { "—" } else { f.security.as_str() }).width(140.0).show_ui(ui, |ui| {
                                for (value, text) in [("", "—"), ("DEFINER", "DEFINER"), ("INVOKER", "INVOKER")] {
                                    ui.selectable_value(&mut f.security, value.to_owned(), text);
                                }
                            });
                            ui.end_row();
                            label(ui, t.db_form_access);
                            egui::ComboBox::from_id_salt("db-form-access").selected_text(if f.access.is_empty() { "—" } else { f.access.as_str() }).width(200.0).show_ui(ui, |ui| {
                                for a in form::ACCESS {
                                    ui.selectable_value(&mut f.access, a.to_owned(), if a.is_empty() { "—" } else { a });
                                }
                            });
                            ui.end_row();
                            label(ui, t.db_form_comment);
                            field(ui, &mut f.comment, 360.0, "");
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(10.0);
                egui::CollapsingHeader::new(egui::RichText::new(t.db_form_preview).size(12.5).color(theme.text_muted)).id_salt("db-form-preview").show(ui, |ui| {
                    sql_box(ui, theme, &f.sql(dialect));
                });
                ui.add_space(10.0);
            });
        });
        if focused {
            self.want_columns();
        }
    }

    /// A new procedure, function or trigger (on `table`, or the first one), as an empty form.
    pub(super) fn new_object(&mut self, d: &str, table: Option<String>, kind: ObjectKind) {
        let tables: Vec<String> = self.tables.get(d).map(|ts| ts.iter().filter(|x| !x.view).map(|x| x.name.clone()).collect()).unwrap_or_default();
        let table = if kind == ObjectKind::Trigger { table.or_else(|| tables.first().cloned()).unwrap_or_default() } else { String::new() };
        self.switch_code(CodeEdit::new(d, Form::new(kind, self.dialect.mssql(), "", &table)));
    }

    /// Opens `next` in the editor, once the changes of the one open are given up (asked first).
    fn switch_code(&mut self, next: CodeEdit) {
        if self.code.as_ref().is_some_and(|c| c.dirty() && !c.text.trim().is_empty()) {
            self.object_dialog = Some(ObjectDialog::Discard(Box::new(next)));
        } else {
            self.set_code(next);
        }
    }

    fn set_code(&mut self, next: CodeEdit) {
        if let Some(name) = next.name.clone().filter(|_| next.loading) {
            self.send(Request::Definition { db: next.db.clone(), kind: next.kind, name });
        }
        self.code = Some(next);
    }

    /// Writes the code open to the server.
    fn save_code(&mut self, t: &Strings) {
        let dialect = self.dialect;
        let mssql = dialect.mssql();
        let Some(c) = &self.code else { return };
        if c.saving || !c.dirty() {
            return;
        }
        let text = c.sql(dialect);
        let text = if mssql { text.trim().to_owned() } else { without_delimiters(&text) };
        let Some(h) = head(&text) else {
            self.notify(format!("⚠  {}", t.db_code_head), true);
            return;
        };
        let old_schema = c.name.as_deref().and_then(|n| n.split_once('.')).map_or("dbo", |(s, _)| s).to_owned();
        let name = listed_name(&h, mssql, &old_schema);
        let drop_sql = |kind: ObjectKind, name: &str| if mssql { format!("DROP {} {}", kind.sql(), dialect.local_table(name)) } else { format!("DROP {} IF EXISTS {}", kind.sql(), db::ident(name)) };
        let (check, drop, create, restore) = match &c.name {
            None => (Vec::new(), String::new(), text.clone(), Vec::new()),
            // SQL Server: changed in place (its rights kept), unless renamed.
            Some(old) if mssql && old.eq_ignore_ascii_case(&name) && h.kind == c.kind => {
                let mut alter = text.clone();
                alter.replace_range(h.create.clone(), "ALTER");
                (Vec::new(), String::new(), alter, Vec::new())
            }
            Some(old) => {
                // MySQL: a routine is tried first under another name (a trigger would fire twice).
                let check = if !mssql && h.kind != ObjectKind::Trigger {
                    let base: String = h.parts.last().cloned().unwrap_or_default().chars().take(48).collect();
                    let temp = format!("{base}_ronnie_check");
                    let mut tried = text.clone();
                    tried.replace_range(h.name.clone(), &db::ident(&temp));
                    vec![tried, format!("DROP {} IF EXISTS {}", h.kind.sql(), db::ident(&temp))]
                } else {
                    Vec::new()
                };
                // Put back as it was, else as whoever writes it (its definer may not be allowed).
                let restore = if mssql { vec![c.original.clone()] } else { vec![c.original.clone(), db::strip_definer(&c.original)] };
                (check, drop_sql(c.kind, old), text.clone(), restore)
            }
        };
        let (db_name, fresh) = (c.db.clone(), c.name.is_none());
        self.next_tag += 1;
        let tag = self.next_tag;
        self.expect.insert(tag, Expect::Saved { db: db_name.clone(), kind: h.kind, name, fresh });
        if let Some(id) = self.remember(&create, Some(db_name.clone()), true) {
            self.exec_entries.insert(tag, (id, Instant::now()));
        }
        if let Some(c) = &mut self.code {
            c.saving = true;
        }
        self.send(Request::Replace { db: db_name, check, drop, create, restore, tag });
    }

    /// Asks before dropping a procedure, function or trigger.
    fn drop_object(&mut self, d: &str, kind: ObjectKind, name: &str, t: &Strings) {
        let (sql, db) = if self.dialect.mssql() { (format!("DROP {} {}", kind.sql(), self.dialect.local_table(name)), Some(d.to_owned())) } else { (format!("DROP {} {}", kind.sql(), self.dialect.table(d, name)), None) };
        let reason = t.db_drop_object_reason.replace("{name}", name);
        self.dialog = Some(Dialog::Confirm { sql, db, reason, query: false, expect: Some(Expect::Dropped { db: d.to_owned(), kind, name: name.to_owned() }), fk: None });
    }

    /// The window to run routine `name` of `d`.
    fn run_dialog(&mut self, d: &str, name: &str) {
        let Some(r) = self.routines.get(d).and_then(|rs| rs.iter().find(|r| r.name == name)).cloned() else { return };
        let values = vec![(String::new(), false); r.params.len()];
        self.object_dialog = Some(ObjectDialog::Run { db: d.to_owned(), routine: Box::new(r), values });
    }

    /// A new SQL tab with `sql`, run in `run` (a database) if given.
    pub(super) fn open_sql_tab(&mut self, sql: String, run: Option<String>) {
        let mut tab = SqlTab::new(self.next_sql_tab);
        self.next_sql_tab += 1;
        tab.sql = sql.clone();
        let id = tab.id;
        self.sql_tabs.push(tab);
        self.sql_tab = self.sql_tabs.len() - 1;
        self.page = Page::Sql;
        if let Some(db) = run.filter(|_| !self.sql_running) {
            self.query_for = Some(id);
            self.send_query(Some(db), sql);
        }
    }

    /// The windows of these pages.
    pub(super) fn object_dialog_ui(&mut self, ctx: &egui::Context, theme: &Theme, t: &Strings) {
        let dialect = self.dialect;
        let Some(dialog) = &mut self.object_dialog else { return };
        // Some(true): its action; Some(false): closed. The run window: `to_tab` puts the call in a tab without running it.
        let mut done: Option<bool> = None;
        let mut to_tab = false;
        let frame = Frame::popup(&ctx.global_style()).inner_margin(20.0).fill(theme.chrome_bg);
        let modal = egui::Modal::new(egui::Id::new("db-object-dialog")).frame(frame).show(ctx, |ui| {
            match dialog {
                ObjectDialog::Run { db, routine, values } => {
                    ui.set_width(560.0);
                    ui.label(egui::RichText::new(t.db_run_title.replace("{name}", &routine.name)).size(17.0).strong());
                    ui.add_space(12.0);
                    if routine.params.is_empty() {
                        ui.label(egui::RichText::new(t.db_no_params).size(12.5).color(theme.text_muted));
                    } else {
                        egui::Grid::new("db-run-params").num_columns(4).spacing([10.0, 8.0]).show(ui, |ui| {
                            for (p, (text, null)) in routine.params.iter().zip(values.iter_mut()) {
                                ui.label(egui::RichText::new(&p.name).monospace());
                                ui.label(egui::RichText::new(format!("{} {}", p.mode, p.kind).trim()).size(11.5).monospace().color(theme.text_muted));
                                // OUT only: read back, nothing to give.
                                if p.mode == "OUT" && !dialect.mssql() {
                                    ui.label(egui::RichText::new(t.db_run_output).size(12.0).color(theme.text_muted));
                                    ui.label("");
                                } else {
                                    ui.add_enabled(!*null, egui::TextEdit::singleline(text).desired_width(220.0).font(FontId::monospace(12.5)).margin(Vec2::new(6.0, 4.0)));
                                    ui.checkbox(null, "NULL");
                                }
                                ui.end_row();
                            }
                        });
                    }
                    ui.add_space(12.0);
                    sql_box(ui, theme, &run_sql(dialect, db, routine, values));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let go = egui::Button::new(egui::RichText::new(format!("▶  {}", t.db_run)).color(theme.bg)).fill(theme.accent).corner_radius(6.0);
                        if ui.add(go).clicked() {
                            done = Some(true);
                        }
                        if ui.button(t.db_open_in_sql).clicked() {
                            to_tab = true;
                            done = Some(true);
                        }
                        if ui.button(t.cancel).clicked() {
                            done = Some(false);
                        }
                    });
                }
                ObjectDialog::Discard(_) => {
                    ui.set_width(420.0);
                    ui.label(egui::RichText::new(t.db_discard_title).size(17.0).strong());
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.db_discard_text).size(13.0).color(theme.text_muted));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let go = egui::Button::new(egui::RichText::new(t.db_discard).color(theme.bg)).fill(theme.ansi[1]).corner_radius(6.0);
                        if ui.add(go).clicked() {
                            done = Some(true);
                        }
                        if ui.button(t.cancel).clicked() {
                            done = Some(false);
                        }
                    });
                }
            }
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        let Some(go) = done else { return };
        let Some(dialog) = self.object_dialog.take() else { return };
        if !go {
            return;
        }
        match dialog {
            ObjectDialog::Run { db, routine, values } => {
                let sql = run_sql(dialect, &db, &routine, &values);
                self.open_sql_tab(sql, (!to_tab).then_some(db));
            }
            ObjectDialog::Discard(next) => self.set_code(*next),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Engine;

    #[test]
    fn reads_the_head_of_a_create() {
        let h = head("-- note\nCREATE DEFINER=`root`@`%` PROCEDURE `p x`(IN a INT) BEGIN END").unwrap();
        assert_eq!((h.kind, h.parts.clone()), (ObjectKind::Procedure, vec!["p x".to_owned()]));
        assert_eq!(h.create, 8..14);
        let h = head("create or alter proc [dbo].[go] @a int as select 1").unwrap();
        assert_eq!((h.kind, h.parts), (ObjectKind::Procedure, vec!["dbo".to_owned(), "go".to_owned()]));
        assert_eq!(h.create, 0..15);
        let h = head("CREATE OR REPLACE DEFINER = CURRENT_USER AGGREGATE FUNCTION IF NOT EXISTS db.f() RETURNS INT").unwrap();
        assert_eq!((h.kind, h.parts), (ObjectKind::Function, vec!["db".to_owned(), "f".to_owned()]));
        let text = "/* x */ CREATE TRIGGER t_ins BEFORE INSERT ON t FOR EACH ROW SET NEW.a = 1";
        let h = head(text).unwrap();
        assert_eq!((h.kind, &text[h.name.clone()]), (ObjectKind::Trigger, "t_ins"));
        assert_eq!(head("CREATE TABLE t (a int)"), None);
        assert_eq!(head("SELECT 1"), None);
    }

    #[test]
    fn drops_delimiters() {
        let sql = "DELIMITER $$\nCREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\nEND$$\nDELIMITER ;\n";
        assert_eq!(without_delimiters(sql), "CREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\nEND");
        assert_eq!(without_delimiters("CREATE PROCEDURE p() SELECT 1;\n"), "CREATE PROCEDURE p() SELECT 1");
    }

    #[test]
    fn writes_calls() {
        let param = |mode: &str, name: &str, kind: &str| db::Param { mode: mode.into(), name: name.into(), kind: kind.into() };
        let my = Dialect(Engine::Mysql);
        let p = Routine { name: "p".into(), params: vec![param("IN", "a", "int"), param("INOUT", "b", "varchar(10)"), param("OUT", "c", "int")], ..Default::default() };
        let values = vec![("4".to_owned(), false), ("x'y".to_owned(), false), (String::new(), false)];
        assert_eq!(run_sql(my, "d", &p, &values), "SET @`b` = 'x''y';\nCALL `d`.`p`(4, @`b`, @`c`);\nSELECT @`b` AS `b`, @`c` AS `c`;");
        let f = Routine { name: "f".into(), function: true, params: vec![param("", "x", "int")], ..Default::default() };
        assert_eq!(run_sql(my, "d", &f, &[(String::new(), true)]), "SELECT `d`.`f`(NULL) AS `f`");
        let ms = Dialect(Engine::Sqlserver);
        let p = Routine { name: "dbo.p".into(), code: "P".into(), params: vec![param("IN", "@a", "nvarchar(5)"), param("OUT", "@n", "int")], ..Default::default() };
        let values = vec![("é".to_owned(), false), (String::new(), false)];
        assert_eq!(run_sql(ms, "d", &p, &values), "DECLARE @n int = NULL;\nEXEC [dbo].[p] @a = N'é', @n = @n OUTPUT;\nSELECT @n AS [@n];");
        let f = Routine { name: "s.rows".into(), code: "IF".into(), function: true, params: vec![param("IN", "@id", "int")], ..Default::default() };
        assert_eq!(run_sql(ms, "d", &f, &[("7".to_owned(), false)]), "SELECT * FROM [s].[rows](7)");
    }
}
