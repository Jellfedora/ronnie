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
        return Some(Link { url: link.uri().to_owned(), cells: covered });
    }

    let (from, to) = find_urls(&text).into_iter().find(|(from, to)| (*from..*to).contains(&hovered))?;
    Some(Link { url: text.chars().skip(from).take(to - from).collect(), cells: cells[from..to].to_vec() })
}

/// URL spans in `text`, as char index ranges.
fn find_urls(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..chars.len().min(i + 8)].iter().collect();
        let Some(scheme) = SCHEMES.iter().find(|s| rest.starts_with(**s)) else {
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

/// Opens a URL with the system's default handler.
pub fn open(url: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    let _ = cmd.arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::find_urls;

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
}
