//! Clickable links: URLs printed in the terminal, and OSC 8 hyperlinks.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::Term;

pub struct Link {
    pub url: String,
    /// Grid cells covered by the link, for underlining.
    pub cells: Vec<Point>,
    /// An OSC 8 hyperlink: the program chose its target, which the visible text may not show.
    pub osc8: bool,
}

const SCHEMES: [&str; 3] = ["https://", "http://", "file://"];

/// The link under `point`, if any.
pub fn link_at<L: EventListener>(term: &Term<L>, point: Point) -> Option<Link> {
    let grid = term.grid();
    if point.line < grid.topmost_line() || point.line > grid.bottommost_line() {
        return None;
    }
    let last_col = Column(grid.columns() - 1);
    let wraps = |line: Line| grid[line][last_col].flags.contains(Flags::WRAPLINE);

    // The logical line: rows joined by soft wraps.
    let mut start = point.line;
    while start > grid.topmost_line() && wraps(start - 1) {
        start -= 1;
    }
    let mut end = point.line;
    while end < grid.bottommost_line() && wraps(end) {
        end += 1;
    }

    let mut text = String::new();
    let mut cells = Vec::new();
    for line in start.0..=end.0 {
        let row = &grid[Line(line)];
        for col in 0..grid.columns() {
            let cell = &row[Column(col)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            text.push(cell.c);
            cells.push(Point::new(Line(line), Column(col)));
        }
    }
    let hovered = cells.iter().position(|p| *p == point)?;

    // OSC 8 hyperlink: every cell of the logical line carrying the same link.
    let cell_at = |p: &Point| &grid[p.line][p.column];
    if let Some(link) = cell_at(&cells[hovered]).hyperlink() {
        let covered = cells.iter().copied().filter(|p| cell_at(p).hyperlink().as_ref() == Some(&link)).collect();
        return Some(Link { url: link.uri().to_owned(), cells: covered, osc8: true });
    }

    let (from, to) = find_urls(&text).into_iter().find(|(from, to)| (*from..*to).contains(&hovered))?;
    Some(Link { url: text.chars().skip(from).take(to - from).collect(), cells: cells[from..to].to_vec(), osc8: false })
}

/// URL spans in `text`, as char index ranges.
fn find_urls(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    // Compared on the chars directly: building a string per position cost an allocation per char.
    let starts_with = |i: usize, scheme: &str| scheme.chars().enumerate().all(|(k, c)| chars.get(i + k) == Some(&c));
    while i < chars.len() {
        let Some(scheme) = (chars[i] == 'h' || chars[i] == 'f').then(|| SCHEMES.iter().find(|s| starts_with(i, s))).flatten() else {
            i += 1;
            continue;
        };
        let mut end = i + scheme.len();
        while end < chars.len() && !ends_url(chars[end]) {
            end += 1;
        }
        // Trailing punctuation usually belongs to the sentence, not the URL.
        while end > i + scheme.len() {
            let c = chars[end - 1];
            let unbalanced_paren = c == ')' && !chars[i..end].contains(&'(');
            if matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | ']' | '}') || unbalanced_paren {
                end -= 1;
            } else {
                break;
            }
        }
        if end > i + scheme.len() {
            spans.push((i, end));
        }
        i = end.max(i + 1);
    }
    spans
}

fn ends_url(c: char) -> bool {
    c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '`' | '{' | '}' | '|' | '\\' | '^')
}

/// Occurrences of `query` (case-insensitive) in the whole grid, scrollback included, top to bottom, as
/// the cells they cover. At most `limit` of them (the most recent).
pub fn find_text<L: EventListener>(term: &Term<L>, query: &str, limit: usize) -> Vec<Vec<Point>> {
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let grid = term.grid();
    let last_col = Column(grid.columns() - 1);
    let (top, bottom) = (grid.topmost_line(), grid.bottommost_line());
    let mut found = Vec::new();
    let mut chars: Vec<char> = Vec::new();
    let mut cells: Vec<Point> = Vec::new();
    for line in top.0..=bottom.0 {
        let row = &grid[Line(line)];
        for col in 0..grid.columns() {
            let cell = &row[Column(col)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            // One lowercase char per cell keeps chars and cells aligned.
            chars.push(cell.c.to_lowercase().next().unwrap_or(cell.c));
            cells.push(Point::new(Line(line), Column(col)));
        }
        // A soft-wrapped row continues on the next one.
        if row[last_col].flags.contains(Flags::WRAPLINE) && line < bottom.0 {
            continue;
        }
        let mut i = 0;
        while i + needle.len() <= chars.len() {
            if chars[i..i + needle.len()] == needle[..] {
                found.push(cells[i..i + needle.len()].to_vec());
                i += needle.len();
            } else {
                i += 1;
            }
        }
        chars.clear();
        cells.clear();
    }
    let excess = found.len().saturating_sub(limit);
    found.drain(..excess);
    found
}

/// A server on this machine announced in the output (`http://localhost:3002/`...).
#[derive(Clone, Debug, PartialEq)]
pub struct LocalUrl {
    pub port: u16,
    pub url: String,
}

/// Local server URLs in the last `max_rows` rows of the grid (scrollback included), oldest first.
pub fn local_urls<L: EventListener>(term: &Term<L>, max_rows: usize) -> Vec<LocalUrl> {
    let grid = term.grid();
    let last_col = Column(grid.columns() - 1);
    let bottom = grid.bottommost_line();
    let top = grid.topmost_line().max(bottom - max_rows as i32);
    let mut found = Vec::new();
    let mut text = String::new();
    for line in top.0..=bottom.0 {
        let row = &grid[Line(line)];
        text.extend((0..grid.columns()).map(|c| &row[Column(c)]).filter(|c| !c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)).map(|c| c.c));
        // A soft-wrapped row continues on the next one.
        if row[last_col].flags.contains(Flags::WRAPLINE) && line < bottom.0 {
            continue;
        }
        // Most lines hold no URL at all: skip them cheaply.
        if !text.contains("://") {
            text.clear();
            continue;
        }
        let chars: Vec<char> = text.chars().collect();
        for (from, to) in find_urls(&text) {
            let url: String = chars[from..to].iter().collect();
            if let Some(local) = as_local(&url) {
                found.retain(|u: &LocalUrl| u.port != local.port);
                found.push(local);
            }
        }
        text.clear();
    }
    found
}

/// `url` if it points to this machine with an explicit port; `0.0.0.0` becomes `localhost`.
fn as_local(url: &str) -> Option<LocalUrl> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split('/').next()?;
    let (host, port) = authority.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    if !matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]" | "[::]") {
        return None;
    }
    let url = if matches!(host, "0.0.0.0" | "[::]") { url.replacen(host, "localhost", 1) } else { url.to_owned() };
    Some(LocalUrl { port, url })
}

/// Whether `url` can be opened on a click without asking: a web address. Anything else (file://,
/// app URL schemes, paths) could launch a program, so it is confirmed first.
pub fn is_safe(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && !url.chars().any(char::is_control)
}

/// Opens a URL with the system's default handler. Never through a shell: on Windows `cmd /C start`
/// would run whatever follows a `&` in the URL.
pub fn open(url: &str) {
    // An argument starting with "-" would be read as an option by `open` / `xdg-open`.
    if url.starts_with('-') || url.chars().any(char::is_control) {
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let wide = |s: &str| std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect::<Vec<u16>>();
        let (operation, target) = (wide("open"), wide(url));
        // SAFETY: both strings are NUL-terminated UTF-16 and outlive the call.
        unsafe { ShellExecuteW(std::ptr::null_mut(), operation.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    }
}

#[cfg(test)]
mod tests {
    use super::{as_local, find_urls, is_safe};

    fn urls(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        find_urls(text).into_iter().map(|(a, b)| chars[a..b].iter().collect()).collect()
    }

    #[test]
    fn finds_urls() {
        assert_eq!(urls("  ➜  Local:   http://localhost:3004/"), ["http://localhost:3004/"]);
        assert_eq!(urls("voir https://example.com/a?b=1, puis"), ["https://example.com/a?b=1"]);
        assert_eq!(urls("(https://example.com/x)."), ["https://example.com/x"]);
        assert_eq!(urls("https://en.wikipedia.org/wiki/Rust_(langage)"), ["https://en.wikipedia.org/wiki/Rust_(langage)"]);
        assert_eq!(urls("a http:// b"), Vec::<String>::new());
    }

    #[test]
    fn only_web_links_open_directly() {
        assert!(is_safe("https://example.com/a?b=1&c=2"));
        assert!(is_safe("HTTP://localhost:3000/"));
        assert!(!is_safe("file:///Users/me/Downloads/Evil.app"));
        assert!(!is_safe("vscode://file/etc/passwd"));
        assert!(!is_safe("/tmp/x.command"));
        assert!(!is_safe("https://x.io/\nrm"));
    }

    #[test]
    fn keeps_local_servers() {
        assert_eq!(as_local("http://localhost:3002/").map(|u| u.port), Some(3002));
        assert_eq!(as_local("http://0.0.0.0:8082/api-docs").unwrap().url, "http://localhost:8082/api-docs");
        assert_eq!(as_local("http://127.0.0.1:5173").map(|u| u.port), Some(5173));
        assert_eq!(as_local("https://example.com:8443/"), None);
        assert_eq!(as_local("http://localhost/"), None);
    }
}
