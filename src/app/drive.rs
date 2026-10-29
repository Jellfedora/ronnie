//! What Claude asks from a pane (see `crate::drive`), done in the window holding the pane concerned:
//! panes and tabs made or changed in the background, and what would change the view proposed in a
//! notice the user accepts or not.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::{App, Tab, Toast};
use crate::config::{Layout, Profile, TabState};
use crate::drive::{Ask, Asked, NewProfile, View};
use crate::pane::{Axis, Direction, PaneId};
use crate::terminal::Terminal;

/// Where a notice from Claude leads, once the user accepts.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Lead {
    Note(Uuid),
    Pane(PaneId),
    /// A file, in the editor of that pane's tab.
    Doc(PaneId, PathBuf),
    Profile(Uuid),
    Notes,
    Dev,
}

/// The pane `id` in `tabs`: its tab's index.
fn tab_of(tabs: &[Tab], id: PaneId) -> Option<usize> {
    tabs.iter().position(|t| t.layout.leaves().contains(&id))
}

/// A word the shell reads as it is.
fn quoted(text: &str) -> String {
    if cfg!(windows) { format!("'{}'", text.replace('\'', "''")) } else { format!("'{}'", text.replace('\'', "'\\''")) }
}

/// The Node.js versions installed, for Claude.
fn installed_text() -> String {
    let installed = crate::node::installed();
    if installed.is_empty() { "none is: `ronnie node install vX.Y.Z`".into() } else { format!("installed: {}", installed.join(", ")) }
}

/// The panes of a saved layout, in order.
fn layout_panes(layout: &Layout, out: &mut Vec<Option<PathBuf>>) {
    match layout {
        Layout::Pane { cwd, .. } => out.push(cwd.clone()),
        Layout::Split { a, b, .. } => {
            layout_panes(a, out);
            layout_panes(b, out);
        }
    }
}

/// Panes `panes` side by side (or one under the other), each taking the same room.
fn row_of(mut panes: Vec<Layout>, axis: Axis) -> Layout {
    if panes.len() <= 1 {
        return panes.pop().unwrap_or_default();
    }
    let n = panes.len() as f32;
    let first = panes.remove(0);
    Layout::Split { axis, ratio: 1.0 / n, a: Box::new(first), b: Box::new(row_of(panes, axis)) }
}

impl App {
    /// The pane whose RONNIE_PANE is `token`, in this window or another.
    fn pane_by_token(&self, token: &str) -> Option<PaneId> {
        let tabs = self.tabs.iter().chain(self.others.iter().flat_map(|w| &w.tabs));
        tabs.flat_map(|t| &t.panes).find(|(_, term)| term.token() == Some(token)).map(|(id, _)| *id)
    }

    fn pane_anywhere(&self, id: PaneId) -> bool {
        tab_of(&self.tabs, id).is_some() || self.others.iter().any(|w| tab_of(&w.tabs, id).is_some())
    }

    /// Claude's asks for this window (its pane, or the pane they are about, is here), and the lines
    /// waiting to be typed in new panes.
    pub(super) fn claude_asks(&mut self, ctx: &egui::Context) {
        let mine: Vec<Asked> = {
            let Ok(mut queue) = self.claude_asks.lock() else { return };
            let mut mine = Vec::new();
            let mut k = 0;
            while k < queue.len() {
                let (asked, at) = &queue[k];
                let target = match &asked.ask {
                    Ask::Type { pane, .. } | Ask::Cd { pane, .. } | Ask::Output { pane, .. } | Ask::Node { pane, .. } | Ask::Rename { pane, .. } | Ask::Close { pane } => Some(*pane),
                    _ => None,
                };
                let target = target.or_else(|| self.pane_by_token(&asked.pane));
                // Here, or in no window (answered here, with what's wrong), or left too long elsewhere.
                let here = target.is_none_or(|id| tab_of(&self.tabs, id).is_some() || !self.pane_anywhere(id)) || at.elapsed() > Duration::from_secs(10);
                if here {
                    mine.push(queue.remove(k).0);
                } else {
                    k += 1;
                }
            }
            mine
        };
        for asked in mine {
            let result = self.do_ask(ctx, &asked);
            match result {
                Ok(text) => crate::drive::answer(&asked, true, text),
                Err(text) => crate::drive::answer(&asked, false, text),
            }
        }
        self.type_waiting();
    }

    /// Lines for new panes, typed once their shell shows its prompt (given up after a minute).
    fn type_waiting(&mut self) {
        let mut k = 0;
        while k < self.claude_typing.len() {
            let (id, ref text, since) = self.claude_typing[k];
            let Some(i) = tab_of(&self.tabs, id) else {
                k += 1;
                continue;
            };
            let local = self.tabs[i].ssh.is_none();
            let typed = match self.tabs[i].panes.get_mut(&id) {
                Some(term) if term.ready_for_commands(since.elapsed(), local) => {
                    term.type_text(text);
                    true
                }
                _ => since.elapsed() > Duration::from_secs(60),
            };
            if typed {
                self.claude_typing.remove(k);
            } else {
                self.ctx.request_repaint_after(Duration::from_millis(100));
                k += 1;
            }
        }
    }

    /// Does `f` without changing what the window shows (a tab opened stays behind).
    fn in_background(&mut self, f: impl FnOnce(&mut Self)) {
        let (active, home, notes, focus) = (self.active, self.home.take(), self.notes_page, self.focus_terminal);
        f(self);
        self.active = active.min(self.tabs.len().saturating_sub(1));
        self.home = home;
        self.notes_page = notes;
        self.focus_terminal = focus;
    }

    /// A notice from Claude: what it did or proposes, and where it leads once accepted.
    fn claude_notice(&mut self, title: &str, body: String, lead: Lead) {
        self.toasts.push(Toast { ok: true, title: title.to_owned(), body, tab: self.active, at: Instant::now(), mail: None, lead: Some(lead) });
        self.ctx.request_repaint();
    }

    /// Goes where a notice from Claude leads.
    pub(super) fn follow(&mut self, lead: Lead) {
        match lead {
            Lead::Note(id) => {
                self.open_notes();
                self.notes.show(id);
            }
            Lead::Pane(id) => self.show_pane(id),
            Lead::Doc(id, path) => {
                self.show_pane(id);
                if let Some(i) = tab_of(&self.tabs, id) {
                    self.ws_open(i, &path);
                }
            }
            Lead::Profile(id) => self.open_profile(id),
            Lead::Notes => self.open_notes(),
            Lead::Dev => {
                self.home = None;
                self.notes_page = false;
                self.open_dev_page();
            }
        }
    }

    /// Pane `id` on screen: its tab shown (its window in front), the pane focused.
    fn show_pane(&mut self, id: PaneId) {
        if let Some(i) = tab_of(&self.tabs, id) {
            self.select(i);
            self.tabs[i].focused = id;
            self.tabs[i].show_files = false;
            self.tabs[i].doc = None;
            self.tabs[i].show_diff = false;
            return;
        }
        for w in &mut self.others {
            if let Some(i) = tab_of(&w.tabs, id) {
                w.active = i;
                w.tabs[i].focused = id;
                w.focus_terminal = true;
                self.ctx.send_viewport_cmd_to(w.viewport, egui::ViewportCommand::Focus);
                return;
            }
        }
    }

    /// The tab of pane `id` in this window, and its terminal if started: an error for Claude else.
    fn local_pane(&mut self, id: PaneId) -> Result<(usize, Option<&mut Terminal>), String> {
        let i = tab_of(&self.tabs, id).ok_or_else(|| format!("no pane {id} (see `ronnie panes`)"))?;
        Ok((i, self.tabs[i].panes.get_mut(&id)))
    }

    fn do_ask(&mut self, ctx: &egui::Context, asked: &Asked) -> Result<String, String> {
        let me = self.pane_by_token(&asked.pane);
        let t = self.t();
        match &asked.ask {
            Ask::Panes => Ok(self.panes_text(me)),
            Ask::Open { path } => {
                if !path.is_file() {
                    return Err(format!("{}: not a file", path.display()));
                }
                let pane = me.ok_or("the asking pane isn't in Ronnie anymore")?;
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                self.claude_notice(t.claude_suggests, name, Lead::Doc(pane, path.clone()));
                Ok("Proposed to the user (a notice to accept): the file opens beside the terminals if they do.".into())
            }
            Ask::Run { command, cwd, down, name } => {
                let me = me.ok_or("the asking pane isn't in Ronnie anymore")?;
                let i = tab_of(&self.tabs, me).ok_or("the asking pane isn't in this window")?;
                let before = self.tabs[i].focused;
                self.tabs[i].focused = me;
                let count = self.tabs[i].layout.leaves().len();
                self.split(ctx, i, if *down { Direction::Down } else { Direction::Right });
                let tab = &mut self.tabs[i];
                let new = tab.focused;
                tab.focused = if before == me { me } else { before };
                if tab.layout.leaves().len() == count {
                    return Err("the pane couldn't be made".into());
                }
                if let Some(name) = name {
                    tab.names.insert(new, name.clone());
                }
                let mut text = String::new();
                if let Some(cwd) = cwd {
                    text.push_str(&format!("cd {}\r", quoted(&cwd.display().to_string())));
                }
                text.push_str(&format!("{command}\r"));
                self.claude_typing.push((new, text, Instant::now()));
                Ok(format!("Pane {new} made beside yours, running: {command}\nIts output: `ronnie output {new}`."))
            }
            Ask::Tab { cwd, command, name } => {
                let state = TabState { name: name.clone(), layout: Layout::Pane { cwd: cwd.clone(), history: None, name: None, startup: None, node: None }, ..Default::default() };
                let id = self.open_in_background(ctx, &state, None);
                if let Some(command) = command {
                    self.claude_typing.push((id, format!("{command}\r"), Instant::now()));
                }
                let label = name.clone().or_else(|| cwd.as_ref().map(|c| c.display().to_string())).unwrap_or_default();
                self.claude_notice(t.claude_opened_tab, label, Lead::Pane(id));
                Ok(format!("Tab opened in the background, its pane: {id}. The user is told (and can go to it)."))
            }
            Ask::Type { pane, text } => {
                if me == Some(*pane) {
                    return Err("that's your own pane".into());
                }
                match self.local_pane(*pane)? {
                    (_, Some(term)) => term.type_text(&format!("{text}\r")),
                    (_, None) => self.claude_typing.push((*pane, format!("{text}\r"), Instant::now())),
                }
                Ok(format!("Typed in pane {pane}."))
            }
            Ask::Cd { pane, dir } => {
                if me == Some(*pane) {
                    return Err("that's your own pane".into());
                }
                let (i, term) = self.local_pane(*pane)?;
                let line = format!("cd {}\r", quoted(&dir.display().to_string()));
                match term {
                    Some(term) => term.type_text(&line),
                    None => self.claude_typing.push((*pane, line, Instant::now())),
                }
                if self.tabs[i].ssh.is_none() && !dir.is_dir() {
                    return Err(format!("{}: no such folder (typed anyway)", dir.display()));
                }
                Ok(format!("Pane {pane} goes to {}.", dir.display()))
            }
            Ask::Output { pane, lines } => match self.local_pane(*pane)? {
                (_, Some(term)) => term.dump((*lines).clamp(1, 5000)).map(|b| crate::drive::plain(&b)).ok_or_else(|| "a full-screen program (vim, htop…) has that pane's screen".into()),
                (_, None) => Err("that pane's shell hasn't started yet (its tab was never shown)".into()),
            },
            Ask::Node { pane, version } => {
                let (i, _) = self.local_pane(*pane)?;
                if self.tabs[i].ssh.is_some() {
                    return Err("an SSH pane: Ronnie's Node.js versions are for local panes".into());
                }
                let version = version.as_ref().map(|v| if v.starts_with('v') { v.clone() } else { format!("v{v}") });
                if let Some(v) = &version
                    && !crate::node::is_installed(v)
                {
                    return Err(format!("{v} isn't installed ({})", installed_text()));
                }
                self.set_pane_node(i, *pane, version.clone());
                Ok(format!("Pane {pane} uses {} from its next command.", version.as_deref().unwrap_or("the system's Node.js")))
            }
            Ask::NodeInstall { version } => {
                let v = if version.starts_with('v') { version.clone() } else { format!("v{version}") };
                if v.split('.').count() != 3 {
                    return Err("an exact version: vX.Y.Z".into());
                }
                if crate::node::is_installed(&v) {
                    return Ok(format!("{v} is installed already."));
                }
                self.dev.install(ctx, v.clone());
                Ok(format!("Installing {v} (about a minute): `ronnie node` lists it once done."))
            }
            Ask::Rename { pane, name } => {
                let (i, _) = self.local_pane(*pane)?;
                match name {
                    Some(name) => self.tabs[i].names.insert(*pane, name.clone()),
                    None => self.tabs[i].names.remove(pane),
                };
                Ok(format!("Pane {pane} renamed."))
            }
            Ask::Close { pane } => {
                if me == Some(*pane) {
                    return Err("that's your own pane".into());
                }
                let (i, _) = self.local_pane(*pane)?;
                self.close_pane(i, *pane);
                Ok(format!("Pane {pane} closed."))
            }
            Ask::Show { what } => {
                let (body, lead) = match what {
                    View::Pane(id) => {
                        let i = tab_of(&self.tabs, *id).ok_or_else(|| format!("no pane {id}"))?;
                        (self.tabs[i].title().to_owned(), Lead::Pane(*id))
                    }
                    View::Notes => (t.notes.to_owned(), Lead::Notes),
                    View::Note(prefix) => {
                        let (id, title) = self.notes.find(prefix).ok_or_else(|| format!("no note {prefix}"))?;
                        (title, Lead::Note(id))
                    }
                    View::Mails | View::Mail(_) => {
                        let id = if let View::Mail(id) = what { id.clone() } else { String::new() };
                        self.toasts.push(Toast { ok: true, title: t.claude_suggests.to_owned(), body: t.mail_title.to_owned(), tab: self.active, at: Instant::now(), mail: Some(id), lead: None });
                        return Ok("Proposed to the user (a notice to accept).".into());
                    }
                    View::Dev => (t.cat_dev.to_owned(), Lead::Dev),
                };
                self.claude_notice(t.claude_suggests, body, lead);
                Ok("Proposed to the user (a notice to accept).".into())
            }
            Ask::Profile(profile) => self.make_profile(ctx, profile),
            Ask::OpenProfile { name } => {
                let profile = self.config.profiles.iter().find(|p| p.tab.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name))).ok_or_else(|| format!("no profile {name}"))?;
                let (id, state) = (profile.id, profile.tab.clone());
                let open = self.tabs.iter().chain(self.others.iter().flat_map(|w| &w.tabs)).find(|t| t.profile == Some(id)).map(|t| t.layout.first_leaf());
                if let Some(pane) = open {
                    return Ok(format!("Already open: its first pane is {pane}."));
                }
                let pane = self.open_in_background(ctx, &state, Some(id));
                self.claude_notice(t.claude_opened_tab, name.clone(), Lead::Pane(pane));
                Ok(format!("Opened in the background, its first pane: {pane}. The user is told."))
            }
        }
    }

    /// A tab opened behind the one shown, its shells started: its first pane.
    fn open_in_background(&mut self, ctx: &egui::Context, state: &TabState, profile: Option<Uuid>) -> PaneId {
        let mut index = 0;
        self.in_background(|app| {
            app.open_tab(state, profile);
            index = app.tabs.len() - 1;
        });
        self.start_pending(ctx, index);
        self.tabs[index].layout.first_leaf()
    }

    fn make_profile(&mut self, ctx: &egui::Context, new: &NewProfile) -> Result<String, String> {
        let color = match &new.color {
            Some(hex) => {
                let digits = hex.trim_start_matches('#');
                let v = u32::from_str_radix(digits, 16).ok().filter(|_| digits.len() == 6).ok_or_else(|| format!("{hex}: a color is #rrggbb"))?;
                Some(egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
            }
            None => None,
        };
        if self.config.profiles.iter().any(|p| p.tab.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&new.name))) {
            return Err(format!("a profile named {} exists already", new.name));
        }
        for pane in &new.panes {
            if !pane.cwd.is_dir() {
                return Err(format!("{}: no such folder", pane.cwd.display()));
            }
            if let Some(v) = &pane.node
                && !crate::node::is_installed(v)
            {
                return Err(format!("Node.js {v} isn't installed ({})", installed_text()));
            }
        }
        let panes = new.panes.iter().map(|p| Layout::Pane { cwd: Some(p.cwd.clone()), history: None, name: p.name.clone(), startup: p.startup.clone(), node: p.node.clone() }).collect();
        let state = TabState { name: Some(new.name.clone()), color, layout: row_of(panes, if new.down { Axis::Vertical } else { Axis::Horizontal }), ..Default::default() };
        let id = Uuid::new_v4();
        self.config.profiles.push(Profile { id, tab: state.clone(), commands: Vec::new(), extra: Default::default() });
        let t = self.t();
        if new.open {
            let pane = self.open_in_background(ctx, &state, Some(id));
            self.claude_notice(t.claude_made_profile, new.name.clone(), Lead::Pane(pane));
            return Ok(format!("Profile {} saved and opened in the background, its first pane: {pane}. The user is told.", new.name));
        }
        self.claude_notice(t.claude_made_profile, new.name.clone(), Lead::Profile(id));
        Ok(format!("Profile {} saved (in the left column). The user is told.", new.name))
    }

    /// What Claude is told of the windows, tabs, panes and profiles.
    fn panes_text(&self, me: Option<PaneId>) -> String {
        let mut out = String::new();
        let windows = std::iter::once((&self.tabs, self.active)).chain(self.others.iter().map(|w| (&w.tabs, w.active)));
        let mut open_profiles = Vec::new();
        for (w, (tabs, active)) in windows.enumerate() {
            out.push_str(&format!("Window {}{}\n", w + 1, if w == 0 { " (yours)" } else { "" }));
            for (i, tab) in tabs.iter().enumerate() {
                let mut line = format!("  Tab \"{}\"", tab.title());
                if let Some(p) = tab.profile.and_then(|id| self.config.profiles.iter().find(|p| p.id == id)) {
                    open_profiles.push(p.id);
                    line.push_str(&format!(", profile \"{}\"", p.name("?")));
                }
                if let Some(host) = tab.ssh.and_then(|id| self.config.ssh.iter().find(|h| h.id == id)) {
                    line.push_str(&format!(", SSH {}", host.name));
                }
                if tab.db.is_some() {
                    line.push_str(", database");
                }
                if i == active {
                    line.push_str(" [shown]");
                }
                out.push_str(&line);
                out.push('\n');
                for id in tab.layout.leaves() {
                    out.push_str(&self.pane_line(tab, id, me));
                    out.push('\n');
                }
            }
        }
        let saved: Vec<String> = self.config.profiles.iter().map(|p| {
            let mut dirs = Vec::new();
            layout_panes(&p.tab.layout, &mut dirs);
            let dirs: Vec<String> = dirs.iter().map(|d| d.as_deref().map_or_else(|| "~".into(), |d| d.display().to_string())).collect();
            format!("  \"{}\"{}: {}", p.name("?"), if open_profiles.contains(&p.id) { " (open)" } else { "" }, dirs.join(", "))
        }).collect();
        if !saved.is_empty() {
            out.push_str(&format!("Saved profiles:\n{}\n", saved.join("\n")));
        }
        let node = crate::node::installed();
        out.push_str(&format!("Node.js installed in Ronnie: {}", if node.is_empty() { "none".into() } else { node.join(", ") }));
        out
    }

    fn pane_line(&self, tab: &Tab, id: PaneId, me: Option<PaneId>) -> String {
        let mut line = format!("    pane {id}");
        if let Some(name) = tab.names.get(&id) {
            line.push_str(&format!(" \"{name}\""));
        }
        let term = tab.panes.get(&id);
        let cwd = match tab.ssh {
            Some(_) => term.and_then(Terminal::reported_cwd).map(PathBuf::from),
            None => term.and_then(Terminal::cwd).or_else(|| tab.cwds.get(&id).cloned()),
        };
        if let Some(cwd) = cwd {
            line.push_str(&format!(": {}", cwd.display()));
        }
        match term {
            None => line.push_str(" (not started: its tab was never shown)"),
            Some(term) if term.has_exited() => line.push_str(" (ended)"),
            Some(term) => {
                if let Some(program) = term.foreground() {
                    line.push_str(&format!(", running {program}"));
                }
            }
        }
        if let Some(node) = tab.nodes.get(&id) {
            line.push_str(&format!(", Node.js {node}"));
        }
        if tab.focused == id {
            line.push_str(" [focused]");
        }
        if me == Some(id) {
            line.push_str("  <- you");
        }
        line
    }
}
