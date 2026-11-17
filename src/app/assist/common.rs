//! What the systems' parts (`mac`, `linux`) and the panel share: what is in front, Teams's message box,
//! the debug lines.

use egui::Rect;

/// The app in front, as last looked at: `A` names Teams's app or window, `E` is an element of its page.
#[derive(Clone, PartialEq)]
pub(super) enum Front<A, E> {
    Other,
    /// Ours (the button just pressed, the panel).
    Ours,
    /// Teams, with its message box when found.
    Teams { app: A, field: Option<Field<E>> },
}

// Not derived: that would ask a default of `A` and `E` too.
#[allow(clippy::derivable_impls)]
impl<A, E> Default for Front<A, E> {
    fn default() -> Self {
        Front::Other
    }
}

impl<A, E> Front<A, E> {
    /// For `RONNIE_TEAMS_DEBUG`.
    pub(super) fn describe(&self) -> String {
        match self {
            Front::Other => "another app in front".into(),
            Front::Ours => "ours in front".into(),
            Front::Teams { field: None, .. } => "Teams in front, its message box not found".into(),
            Front::Teams { field: Some(f), .. } => format!("Teams's message box at {:?}, first tool at {:?}", f.rect, f.tool),
        }
    }
}

/// Teams's message box: the element, where (on the screens), and the first of the tools under it (the
/// formatting one), the button put just left of it.
#[derive(Clone, PartialEq)]
pub(super) struct Field<E> {
    pub(super) el: E,
    pub(super) rect: Rect,
    pub(super) tool: Option<Rect>,
}

/// The conversation's last messages given to Claude, and each one's length at most.
pub(super) const MESSAGES: usize = 15;
pub(super) const MESSAGE_CHARS: usize = 600;

/// A button of the row under the box (`under`): its size, and just under the box.
pub(super) fn is_tool(r: Rect, under: Rect) -> bool {
    (20.0..=48.0).contains(&r.width()) && (20.0..=48.0).contains(&r.height()) && r.min.y >= under.max.y - 4.0 && r.min.y <= under.max.y + 60.0
}

/// The leftmost of `found`.
pub(super) fn leftmost(found: Vec<Rect>) -> Option<Rect> {
    found.into_iter().min_by(|a, b| a.min.x.total_cmp(&b.min.x))
}

/// The messages kept: the last `MESSAGES`.
pub(super) fn last_messages(mut all: Vec<(Option<String>, String)>) -> Vec<(Option<String>, String)> {
    let skip = all.len().saturating_sub(MESSAGES);
    all.split_off(skip)
}

/// A line on the standard error with `RONNIE_TEAMS_DEBUG`.
pub(super) fn debug(what: &str) {
    if std::env::var_os("RONNIE_TEAMS_DEBUG").is_some() {
        eprintln!("{} teams claude: {what}", chrono::Local::now().format("%H:%M:%S"));
    }
}
