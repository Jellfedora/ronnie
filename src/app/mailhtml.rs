//! A mail's HTML shown in the window, as a mail app would: paragraphs, headings, bold and italic,
//! links (clickable), lists, quotes, tables read row by row, the inline styles that matter (color,
//! size, alignment, backgrounds of buttons) and the images. What the mail hides (preheaders,
//! `display:none`, the `<head>`) stays hidden.
//!
//! The HTML is read once into blocks (`parse`), drawn each frame (`show`).

use std::ops::Range;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, FontFamily, FontId, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    pub size: Option<f32>,
    pub color: Option<Color32>,
    pub bg: Option<Color32>,
    pub link: Option<String>,
}

#[derive(Debug)]
pub enum Run {
    Text(String, Style),
    Break,
    Image { src: String, alt: String, width: Option<f32>, height: Option<f32>, link: Option<String> },
}

#[derive(Debug)]
pub enum Block {
    Para { runs: Vec<Run>, align: TextAlign, indent: u8, quote: u8 },
    Rule,
}

/// Elements that end the paragraph before them and start a new one.
const BLOCKS: &[&str] = &[
    "p", "div", "section", "article", "header", "footer", "main", "nav", "aside", "center", "address", "figure", "figcaption", "form", "fieldset", "dl", "dt", "dd", "table", "tbody",
    "thead", "tfoot", "tr", "caption", "ul", "ol", "li", "blockquote", "pre", "h1", "h2", "h3", "h4", "h5", "h6", "body", "html", "hgroup", "details", "summary", "legend",
];
/// Elements without a closing tag.
const VOID: &[&str] = &["br", "img", "hr", "meta", "link", "input", "col", "area", "base", "wbr", "source", "track", "embed", "param", "keygen"];
/// Elements whose content is never shown.
const SKIPPED: &[&str] = &["head", "style", "script", "title", "template", "svg", "object", "iframe", "noscript", "xml", "select", "textarea", "video", "audio", "map"];

struct Frame {
    tag: String,
    style: Style,
    hidden: bool,
    align: Option<TextAlign>,
    pre: bool,
    indent: u8,
    quote: u8,
    /// In a list: (ordered, the next item's number).
    list: Option<(bool, u32)>,
}

struct Parser {
    blocks: Vec<Block>,
    runs: Vec<Run>,
    stack: Vec<Frame>,
    /// The last thing written was a space (or nothing yet): the next one is dropped.
    space: bool,
}

impl Parser {
    fn top(&self) -> &Frame {
        self.stack.last().expect("the root frame stays")
    }

    fn flush(&mut self) {
        // Trailing spaces and breaks dropped; a paragraph of nothing isn't one.
        while matches!(self.runs.last(), Some(Run::Break)) {
            self.runs.pop();
        }
        if let Some(Run::Text(text, _)) = self.runs.last_mut() {
            let trimmed = text.trim_end().len();
            text.truncate(trimmed);
        }
        let empty = self.runs.iter().all(|r| match r {
            Run::Text(t, _) => t.trim().is_empty(),
            Run::Break => true,
            Run::Image { .. } => false,
        });
        let runs = std::mem::take(&mut self.runs);
        if !empty {
            let top = self.top();
            let align = self.stack.iter().rev().find_map(|f| f.align).unwrap_or_default();
            self.blocks.push(Block::Para { runs, align, indent: top.indent, quote: top.quote });
        }
        self.space = true;
    }

    fn text(&mut self, text: &str) {
        let top = self.top();
        if top.hidden {
            return;
        }
        let (pre, style) = (top.pre, top.style.clone());
        if pre {
            for (k, line) in text.split('\n').enumerate() {
                if k > 0 {
                    self.runs.push(Run::Break);
                }
                if !line.is_empty() {
                    self.push_text(line.trim_end_matches('\r').to_owned(), &style);
                }
            }
            return;
        }
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if c.is_ascii_whitespace() {
                if !self.space {
                    out.push(' ');
                    self.space = true;
                }
            } else if c != '\u{200c}' && c != '\u{200b}' && c != '\u{ad}' && c != '\u{feff}' && c != '\u{34f}' {
                out.push(c);
                self.space = false;
            }
        }
        if !out.is_empty() {
            self.push_text(out, &style);
        }
    }

    fn push_text(&mut self, text: String, style: &Style) {
        if let Some(Run::Text(last, s)) = self.runs.last_mut()
            && s == style
        {
            last.push_str(&text);
            return;
        }
        self.runs.push(Run::Text(text, style.clone()));
    }

    fn open(&mut self, tag: &str, attrs: &[(String, String)], self_closing: bool) {
        let attr = |name: &str| attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
        // Those a browser closes by itself when the next one opens.
        if matches!(tag, "p" | "li" | "tr" | "td" | "th" | "dt" | "dd" | "option") && self.top().tag == tag {
            self.close(tag);
        }
        let block = BLOCKS.contains(&tag);
        if block {
            self.flush();
        }
        let parent = self.top();
        let hidden = parent.hidden;
        let mut frame = Frame { tag: tag.to_owned(), style: parent.style.clone(), hidden, align: None, pre: parent.pre, indent: parent.indent, quote: parent.quote, list: None };
        let s = &mut frame.style;
        match tag {
            "b" | "strong" | "th" => s.bold = true,
            "i" | "em" | "cite" | "dfn" | "var" => s.italic = true,
            "u" | "ins" => s.underline = true,
            "s" | "strike" | "del" => s.strike = true,
            "code" | "tt" | "kbd" | "samp" => s.mono = true,
            "small" => s.size = Some(s.size.unwrap_or(BASE) * 0.85),
            "big" => s.size = Some(s.size.unwrap_or(BASE) * 1.2),
            "mark" => s.bg = Some(Color32::from_rgb(0xff, 0xf3, 0x8a)),
            "a" => {
                if let Some(href) = attr("href").map(str::trim).filter(|h| !h.is_empty() && !h.starts_with('#') && !h.to_ascii_lowercase().starts_with("javascript:")) {
                    s.link = Some(href.to_owned());
                    s.underline = true;
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                s.bold = true;
                s.size = Some([26.0, 22.0, 18.5, 16.0, 14.5, 13.0][usize::from(tag.as_bytes()[1] - b'1')]);
            }
            "pre" => {
                frame.pre = true;
                s.mono = true;
            }
            "blockquote" => frame.quote += 1,
            "ul" | "ol" | "dd" => frame.indent += 1,
            "center" => frame.align = Some(TextAlign::Center),
            "font" => {
                if let Some(c) = attr("color").and_then(parse_color) {
                    s.color = Some(c);
                }
                if let Some(n) = attr("size").and_then(|v| v.trim().parse::<usize>().ok()) {
                    s.size = Some([10.0, 12.0, BASE, 17.0, 22.0, 28.0, 36.0][n.clamp(1, 7) - 1]);
                }
            }
            _ => {}
        }
        if tag == "ul" || tag == "ol" {
            let start = attr("start").and_then(|v| v.parse().ok()).unwrap_or(1);
            frame.list = Some((tag == "ol", start));
        }
        if let Some(bg) = attr("bgcolor").and_then(parse_color) {
            frame.style.bg = Some(bg);
        }
        // A table's own align places the table, not its text, which starts again from the left (as
        // in the quirks mode mails are mostly read in).
        if tag == "table" {
            frame.align = Some(TextAlign::Left);
        } else if let Some(a) = attr("align").and_then(parse_align) {
            frame.align = Some(a);
        }
        if let Some(css) = attr("style") {
            apply_css(css, &mut frame);
        }
        if SKIPPED.contains(&tag) || attr("hidden").is_some() {
            frame.hidden = true;
        }
        let hidden = frame.hidden;

        // The void ones are done at once.
        match tag {
            "br" if !hidden => {
                self.runs.push(Run::Break);
                self.space = true;
            }
            "hr" if !hidden => {
                self.flush();
                self.blocks.push(Block::Rule);
            }
            "img" if !hidden => {
                let src = attr("src").unwrap_or_default().trim().to_owned();
                let px = |v: Option<&str>| v.and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok()).filter(|v| *v > 0.0);
                let (mut width, mut height) = (px(attr("width")), px(attr("height")));
                if let Some(css) = attr("style") {
                    for (k, v) in css_props(css) {
                        match k.as_str() {
                            "width" if v.ends_with("px") => width = px(Some(&v)).or(width),
                            "height" if v.ends_with("px") => height = px(Some(&v)).or(height),
                            _ => {}
                        }
                    }
                }
                // Tracking pixels and spacers.
                let tiny = width.is_some_and(|w| w <= 2.0) || height.is_some_and(|h| h <= 2.0);
                if !src.is_empty() && !tiny {
                    let link = frame.style.link.clone();
                    self.runs.push(Run::Image { src, alt: attr("alt").unwrap_or_default().trim().to_owned(), width, height, link });
                    self.space = false;
                }
            }
            _ => {}
        }
        if VOID.contains(&tag) || self_closing {
            return;
        }
        // A list item starts with its bullet or number.
        if tag == "li" && !hidden {
            let marker = match self.stack.iter_mut().rev().find_map(|f| f.list.as_mut()) {
                Some((true, n)) => {
                    *n += 1;
                    format!("{}. ", *n - 1)
                }
                _ => "•  ".to_owned(),
            };
            let style = Style { link: None, underline: false, ..frame.style.clone() };
            self.runs.push(Run::Text(marker, style));
            self.space = true;
        }
        // Cells side by side: kept apart.
        if (tag == "td" || tag == "th") && !hidden && !self.space && !self.runs.is_empty() {
            self.push_text("   ".into(), &Style::default());
            self.space = true;
        }
        self.stack.push(frame);
    }

    fn close(&mut self, tag: &str) {
        // Up to the element closed, if it's open (a stray closing tag is ignored).
        let Some(at) = self.stack.iter().rposition(|f| f.tag == tag) else { return };
        if at == 0 {
            return;
        }
        let block = self.stack[at..].iter().any(|f| BLOCKS.contains(&f.tag.as_str()));
        if block {
            self.flush();
        }
        self.stack.truncate(at);
        if block {
            self.space = true;
        }
    }
}

/// The size of the text, in points.
pub const BASE: f32 = 14.5;

fn parse_align(v: &str) -> Option<TextAlign> {
    match v.trim().to_ascii_lowercase().as_str() {
        "center" | "middle" | "-webkit-center" => Some(TextAlign::Center),
        "right" | "end" => Some(TextAlign::Right),
        "left" | "start" | "justify" => Some(TextAlign::Left),
        _ => None,
    }
}

fn css_props(css: &str) -> impl Iterator<Item = (String, String)> + '_ {
    css.split(';').filter_map(|d| {
        let (k, v) = d.split_once(':')?;
        let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
        Some((k.trim().to_ascii_lowercase(), v))
    })
}

fn apply_css(css: &str, frame: &mut Frame) {
    let s = &mut frame.style;
    let mut zero_height = false;
    let mut overflow_hidden = false;
    for (k, v) in css_props(css) {
        match k.as_str() {
            "color" => {
                if let Some(c) = parse_color(&v) {
                    s.color = Some(c);
                }
            }
            "background-color" | "background" => {
                if let Some(c) = v.split_whitespace().find_map(parse_color).or_else(|| parse_color(&v)) {
                    s.bg = Some(c);
                }
            }
            "font-weight" => s.bold = v == "bold" || v == "bolder" || v.parse::<u32>().is_ok_and(|w| w >= 600),
            "font-style" => s.italic = v == "italic" || v == "oblique",
            "font-family" if v.contains("mono") || v.contains("courier") || v.contains("consolas") => s.mono = true,
            "text-decoration" | "text-decoration-line" => {
                s.underline = v.contains("underline");
                s.strike = v.contains("line-through");
            }
            "font-size" => {
                if let Some(size) = css_size(&v, s.size.unwrap_or(BASE)) {
                    s.size = Some(size);
                }
            }
            "text-align" => {
                if let Some(a) = parse_align(&v) {
                    frame.align = Some(a);
                }
            }
            "display" if v == "none" => frame.hidden = true,
            "visibility" if v == "hidden" => frame.hidden = true,
            "mso-hide" if v == "all" => frame.hidden = true,
            "opacity" if v.parse::<f32>().is_ok_and(|o| o == 0.0) => frame.hidden = true,
            "max-height" | "height" if v.trim_end_matches("px").parse::<f32>().is_ok_and(|h| h == 0.0) => zero_height = true,
            "overflow" if v == "hidden" => overflow_hidden = true,
            "white-space" if v.starts_with("pre") => frame.pre = true,
            _ => {}
        }
    }
    // A preheader: in the inbox's preview only.
    if (zero_height && overflow_hidden) || s.size.is_some_and(|z| z < 2.0) {
        frame.hidden = true;
    }
}

fn css_size(v: &str, parent: f32) -> Option<f32> {
    let num = |s: &str| s.trim().parse::<f32>().ok();
    let size = if let Some(px) = v.strip_suffix("px") {
        num(px)?
    } else if let Some(pt) = v.strip_suffix("pt") {
        num(pt)? * 4.0 / 3.0
    } else if let Some(em) = v.strip_suffix("rem") {
        num(em)? * 16.0
    } else if let Some(em) = v.strip_suffix("em") {
        num(em)? * parent
    } else if let Some(pc) = v.strip_suffix('%') {
        num(pc)? / 100.0 * parent
    } else {
        match v {
            "xx-small" => 9.0,
            "x-small" => 10.0,
            "small" => 13.0,
            "medium" => 16.0,
            "large" => 18.0,
            "x-large" => 24.0,
            "xx-large" => 32.0,
            _ => num(v)?,
        }
    };
    // Mails are written for pixels a bit smaller than the window's points.
    Some(if size < 2.0 { size } else { (size * 0.95).clamp(9.0, 40.0) })
}

pub fn parse_color(v: &str) -> Option<Color32> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        let d = |s: &str| u8::from_str_radix(s, 16).ok();
        return match hex.len() {
            3 | 4 => {
                let c: Vec<u8> = hex.chars().take(3).map(|c| d(&c.to_string()).map(|x| x * 17)).collect::<Option<_>>()?;
                Some(Color32::from_rgb(c[0], c[1], c[2]))
            }
            6 | 8 => Some(Color32::from_rgb(d(&hex[0..2])?, d(&hex[2..4])?, d(&hex[4..6])?)),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let parts: Vec<&str> = args.trim_end_matches(')').split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
        let c = |k: usize| parts.get(k)?.trim().trim_end_matches('%').parse::<f32>().ok().map(|x| x.clamp(0.0, 255.0) as u8);
        if parts.get(3).and_then(|a| a.parse::<f32>().ok()).is_some_and(|a| a < 0.1) {
            return None;
        }
        return Some(Color32::from_rgb(c(0)?, c(1)?, c(2)?));
    }
    let rgb = match v.as_str() {
        "black" => 0x000000,
        "white" => 0xffffff,
        "red" => 0xff0000,
        "green" => 0x008000,
        "blue" => 0x0000ff,
        "navy" => 0x000080,
        "gray" | "grey" => 0x808080,
        "silver" => 0xc0c0c0,
        "lightgray" | "lightgrey" => 0xd3d3d3,
        "darkgray" | "darkgrey" => 0xa9a9a9,
        "orange" => 0xffa500,
        "yellow" => 0xffff00,
        "purple" => 0x800080,
        "teal" => 0x008080,
        "maroon" => 0x800000,
        "olive" => 0x808000,
        "lime" => 0x00ff00,
        "aqua" | "cyan" => 0x00ffff,
        "fuchsia" | "magenta" => 0xff00ff,
        "darkblue" => 0x00008b,
        "darkred" => 0x8b0000,
        "darkgreen" => 0x006400,
        "whitesmoke" => 0xf5f5f5,
        "gainsboro" => 0xdcdcdc,
        "dimgray" | "dimgrey" => 0x696969,
        "royalblue" => 0x4169e1,
        "steelblue" => 0x4682b4,
        "crimson" => 0xdc143c,
        "tomato" => 0xff6347,
        "gold" => 0xffd700,
        _ => return None,
    };
    Some(Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
}

/// &amp; &eacute; &#233; &#xE9; → the characters.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|e| e + 1).unwrap_or(rest.len());
        let name = &rest[1..end];
        let decoded = if let Some(num) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            u32::from_str_radix(num, 16).ok().and_then(char::from_u32).map(String::from)
        } else if let Some(num) = name.strip_prefix('#') {
            num.parse::<u32>().ok().and_then(char::from_u32).map(String::from)
        } else {
            named_entity(name).map(str::to_owned)
        };
        match decoded {
            Some(d) if end > 1 => {
                out.push_str(&d);
                rest = &rest[end..];
                if rest.starts_with(';') {
                    rest = &rest[1..];
                }
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" | "AMP" => "&",
        "lt" | "LT" => "<",
        "gt" | "GT" => ">",
        "quot" | "QUOT" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "ensp" | "emsp" | "thinsp" => " ",
        "zwnj" | "zwj" | "shy" | "lrm" | "rlm" => "",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "laquo" => "«",
        "raquo" => "»",
        "lsquo" => "‘",
        "rsquo" => "’",
        "sbquo" => "‚",
        "ldquo" => "“",
        "rdquo" => "”",
        "bdquo" => "„",
        "bull" => "•",
        "middot" => "·",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "deg" => "°",
        "times" => "×",
        "divide" => "÷",
        "plusmn" => "±",
        "sect" => "§",
        "para" => "¶",
        "larr" => "←",
        "rarr" => "→",
        "uarr" => "↑",
        "darr" => "↓",
        "check" => "✓",
        "hearts" => "♥",
        "star" => "☆",
        "iexcl" => "¡",
        "iquest" => "¿",
        "szlig" => "ß",
        "aacute" => "á",
        "agrave" => "à",
        "acirc" => "â",
        "auml" => "ä",
        "atilde" => "ã",
        "aring" => "å",
        "aelig" => "æ",
        "ccedil" => "ç",
        "eacute" => "é",
        "egrave" => "è",
        "ecirc" => "ê",
        "euml" => "ë",
        "iacute" => "í",
        "igrave" => "ì",
        "icirc" => "î",
        "iuml" => "ï",
        "ntilde" => "ñ",
        "oacute" => "ó",
        "ograve" => "ò",
        "ocirc" => "ô",
        "ouml" => "ö",
        "otilde" => "õ",
        "oslash" => "ø",
        "oelig" => "œ",
        "uacute" => "ú",
        "ugrave" => "ù",
        "ucirc" => "û",
        "uuml" => "ü",
        "yacute" => "ý",
        "yuml" => "ÿ",
        "Aacute" => "Á",
        "Agrave" => "À",
        "Acirc" => "Â",
        "Auml" => "Ä",
        "Ccedil" => "Ç",
        "Eacute" => "É",
        "Egrave" => "È",
        "Ecirc" => "Ê",
        "Euml" => "Ë",
        "Icirc" => "Î",
        "Iuml" => "Ï",
        "Ocirc" => "Ô",
        "Ouml" => "Ö",
        "OElig" => "Œ",
        "Ucirc" => "Û",
        "Ugrave" => "Ù",
        "Uuml" => "Ü",
        "Ntilde" => "Ñ",
        _ => return None,
    })
}

/// The tag at the start of `rest` (just after its '<'): (its name lowercased, closing, its
/// attributes, self-closing, the bytes it takes).
type Tag = (String, bool, Vec<(String, String)>, bool, usize);

fn read_tag(rest: &str) -> Option<Tag> {
    let bytes = rest.as_bytes();
    let mut i = 0;
    let closing = bytes.first() == Some(&b'/');
    if closing {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' && bytes[i] != b'/' {
        i += 1;
    }
    let name = rest[start..i].to_ascii_lowercase();
    if name.is_empty() || !name.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            None => return Some((name, closing, attrs, self_closing, i)),
            Some(b'>') => return Some((name, closing, attrs, self_closing, i + 1)),
            Some(b'/') => {
                self_closing = true;
                i += 1;
                continue;
            }
            _ => {}
        }
        self_closing = false;
        let a = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'=' | b'>') && !(bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'>')) {
            i += 1;
        }
        let key = rest[a..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            match bytes.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let v = i + 1;
                    let end = rest[v..].find(q as char).map_or(rest.len(), |e| v + e);
                    value = decode_entities(&rest[v..end]);
                    i = (end + 1).min(rest.len());
                }
                _ => {
                    let v = i;
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                        i += 1;
                    }
                    value = decode_entities(&rest[v..i]);
                }
            }
        }
        if key.is_empty() {
            i += 1;
        } else {
            attrs.push((key, value));
        }
    }
}

/// Reads a mail's HTML into blocks.
pub fn parse(html: &str) -> Vec<Block> {
    let root = Frame { tag: String::new(), style: Style::default(), hidden: false, align: None, pre: false, indent: 0, quote: 0, list: None };
    let mut p = Parser { blocks: Vec::new(), runs: Vec::new(), stack: vec![root], space: true };
    let mut rest = html;
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            p.text(&decode_entities(rest));
            break;
        };
        if at > 0 {
            p.text(&decode_entities(&rest[..at]));
        }
        rest = &rest[at + 1..];
        if let Some(comment) = rest.strip_prefix("!--") {
            rest = comment.find("-->").map_or("", |e| &comment[e + 3..]);
            continue;
        }
        if rest.starts_with('!') || rest.starts_with('?') {
            rest = rest.find('>').map_or("", |e| &rest[e + 1..]);
            continue;
        }
        let Some((name, closing, attrs, self_closing, len)) = read_tag(rest) else {
            // A lone "<": text.
            p.text("<");
            continue;
        };
        rest = &rest[len.min(rest.len())..];
        if closing {
            p.close(&name);
            continue;
        }
        // Raw text: up to its closing tag, unread.
        if matches!(name.as_str(), "style" | "script" | "title" | "textarea" | "xml") && !self_closing {
            let close = format!("</{name}");
            let lower = rest.to_ascii_lowercase();
            rest = lower.find(&close).map_or("", |e| {
                let after = &rest[e..];
                after.find('>').map_or("", |g| &after[g + 1..])
            });
            continue;
        }
        p.open(&name, &attrs, self_closing);
    }
    p.flush();
    p.blocks
}

/// A mail's plain text into blocks: its paragraphs, its lines kept, its links clickable.
pub fn from_text(text: &str) -> Vec<Block> {
    let text = text.replace("\r\n", "\n");
    let mut blocks = Vec::new();
    for para in text.split("\n\n").filter(|p| !p.trim().is_empty()) {
        let quote = u8::from(para.lines().all(|l| l.starts_with('>')));
        let mut runs = Vec::new();
        for (k, line) in para.trim_matches('\n').lines().enumerate() {
            if k > 0 {
                runs.push(Run::Break);
            }
            let line = if quote > 0 { line.trim_start_matches('>').trim_start() } else { line };
            let mut plain = String::new();
            for (j, word) in line.split(' ').enumerate() {
                if j > 0 {
                    plain.push(' ');
                }
                let url = word.trim_end_matches(['.', ',', ')', ';', '>', '"']).trim_start_matches(['(', '<', '"']);
                if url.starts_with("http://") || url.starts_with("https://") {
                    let at = word.find(url).unwrap_or(0);
                    plain.push_str(&word[..at]);
                    runs.push(Run::Text(std::mem::take(&mut plain), Style::default()));
                    runs.push(Run::Text(url.to_owned(), Style { link: Some(url.to_owned()), underline: true, ..Default::default() }));
                    plain.push_str(&word[at + url.len()..]);
                } else {
                    plain.push_str(word);
                }
            }
            if !plain.is_empty() {
                runs.push(Run::Text(plain, Style::default()));
            }
        }
        blocks.push(Block::Para { runs, align: TextAlign::Left, indent: 0, quote });
    }
    blocks
}

/// Light (for a text) or dark (for a background) enough to need the other.
fn luminance(c: Color32) -> f32 {
    (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0
}

/// An image as the page draws it: its texture and its natural size, in points.
pub enum Picture {
    Ready(egui::TextureId, Vec2),
    Loading,
    Failed,
}

/// The paper's colors: mails are written for a white page.
pub const PAPER: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const INK: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
const LINK: Color32 = Color32::from_rgb(0x1a, 0x5f, 0xd0);

fn font(style: &Style) -> FontId {
    let size = style.size.unwrap_or(BASE);
    if style.mono {
        return FontId::new(size * 0.92, FontFamily::Monospace);
    }
    let face = match (style.bold, style.italic) {
        (true, true) => "note-bold-italic",
        (true, false) => "note-bold",
        (false, true) => "note-italic",
        (false, false) => "note",
    };
    FontId::new(size, FontFamily::Name(face.into()))
}

/// Draws the blocks on a white page; `picture` gives the images (asked for as they come).
pub fn show(ui: &mut Ui, blocks: &[Block], picture: &mut dyn FnMut(&str) -> Picture) {
    for block in blocks {
        match block {
            Block::Rule => {
                ui.add_space(6.0);
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, Color32::from_gray(0xdd));
                ui.add_space(6.0);
            }
            Block::Para { runs, align, indent, quote } => {
                let pad = *indent as f32 * 22.0 + *quote as f32 * 14.0;
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        if *quote > 0 {
                            let top = ui.cursor().min;
                            paragraph(ui, runs, *align, picture);
                            let bottom = ui.min_rect().max.y;
                            ui.painter().rect_filled(egui::Rect::from_min_max(top - Vec2::new(10.0, 0.0), egui::pos2(top.x - 7.0, bottom)), 1.0, Color32::from_gray(0xcc));
                        } else {
                            paragraph(ui, runs, *align, picture);
                        }
                    });
                });
                ui.add_space(8.0);
            }
        }
    }
}

fn paragraph(ui: &mut Ui, runs: &[Run], align: TextAlign, picture: &mut dyn FnMut(&str) -> Picture) {
    let mut start = 0;
    for (k, run) in runs.iter().enumerate() {
        if let Run::Image { src, alt, width, height, link } = run {
            text_runs(ui, &runs[start..k], align);
            image(ui, src, alt, *width, *height, link.as_deref(), align, picture);
            start = k + 1;
        }
    }
    text_runs(ui, &runs[start..], align);
}

fn text_runs(ui: &mut Ui, runs: &[Run], align: TextAlign) {
    if runs.iter().all(|r| matches!(r, Run::Break) || matches!(r, Run::Text(t, _) if t.trim().is_empty())) {
        return;
    }
    let width = ui.available_width();
    let mut job = LayoutJob { halign: match align { TextAlign::Left => Align::LEFT, TextAlign::Center => Align::Center, TextAlign::Right => Align::RIGHT }, ..Default::default() };
    job.wrap.max_width = width;
    let mut links: Vec<(Range<usize>, String)> = Vec::new();
    let mut chars = 0;
    let mut first = true;
    for run in runs {
        let (text, style) = match run {
            Run::Text(t, s) => (if first { t.trim_start() } else { t.as_str() }, s.clone()),
            Run::Break => ("\n", Style::default()),
            Run::Image { .. } => continue,
        };
        first = false;
        if text.is_empty() {
            continue;
        }
        // Mails written for white paper: a light text keeps its dark background (a button),
        // otherwise backgrounds are left out (they would stripe the text).
        let mut color = style.color.unwrap_or(if style.link.is_some() { LINK } else { INK });
        let bg = style.bg.filter(|bg| luminance(*bg) < 0.75 || luminance(color) > 0.7);
        if bg.is_none() && luminance(color) > 0.8 {
            color = INK;
        }
        let mut format = TextFormat { font_id: font(&style), color, background: bg.unwrap_or(Color32::TRANSPARENT), italics: false, ..Default::default() };
        if style.underline {
            format.underline = Stroke::new(1.0, color);
        }
        if style.strike {
            format.strikethrough = Stroke::new(1.0, color);
        }
        format.line_height = Some(format.font_id.size * 1.45);
        // A button: a bit of its color around its text.
        let padded;
        let text = if bg.is_some() && style.link.is_some() {
            padded = format!("\u{2002}{}\u{2002}", text.trim());
            padded.as_str()
        } else {
            text
        };
        let n = text.chars().count();
        if let Some(link) = &style.link {
            links.push((chars..chars + n, link.clone()));
        }
        chars += n;
        job.append(text, 0.0, format);
    }
    let galley = ui.painter().layout_job(job);
    let size = galley.size();
    let x = match align {
        TextAlign::Left => 0.0,
        TextAlign::Center => (width - size.x).max(0.0) / 2.0,
        TextAlign::Right => (width - size.x).max(0.0),
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, size.y), Sense::click());
    // A centered galley is laid out around its x: drawn from its left edge.
    let origin = egui::pos2(rect.min.x + x - galley.rect.min.x, rect.min.y);
    let link_at = |pos: egui::Pos2| {
        let cursor = galley.cursor_from_pos(pos - origin);
        links.iter().find(|(r, _)| r.contains(&cursor.index.0)).map(|(_, l)| l.clone())
    };
    let resp = match resp.hover_pos().and_then(link_at) {
        Some(link) => resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(link),
        None => resp,
    };
    if resp.clicked()
        && let Some(link) = resp.interact_pointer_pos().and_then(link_at)
    {
        open_link(&link);
    }
    ui.painter().galley(origin, galley, INK);
}

/// A mail's link in the browser (egui's `open_url` does nothing here: eframe is built without it).
/// Web and mail addresses only: a `file:` or an app's scheme in a mail is not followed.
fn open_link(link: &str) {
    let lower = link.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        crate::terminal::open_url(link);
    }
}

#[allow(clippy::too_many_arguments)]
fn image(ui: &mut Ui, src: &str, alt: &str, width: Option<f32>, height: Option<f32>, link: Option<&str>, align: TextAlign, picture: &mut dyn FnMut(&str) -> Picture) {
    let avail = ui.available_width();
    let (texture, natural) = match picture(src) {
        Picture::Ready(t, size) => (Some(t), size),
        Picture::Loading => (None, Vec2::new(width.unwrap_or(120.0), height.unwrap_or(60.0))),
        Picture::Failed => {
            if alt.is_empty() {
                return;
            }
            (None, Vec2::ZERO)
        }
    };
    // The size the mail asks for (one side given: the other follows), never wider than the page.
    let mut size = match (width, height) {
        (Some(w), Some(h)) => Vec2::new(w, h),
        (Some(w), None) if natural.x > 0.0 => Vec2::new(w, w * natural.y / natural.x),
        (None, Some(h)) if natural.y > 0.0 => Vec2::new(h * natural.x / natural.y, h),
        _ => natural,
    };
    if size.x > avail {
        size *= avail / size.x;
    }
    let layout = match align {
        TextAlign::Left => egui::Layout::left_to_right(Align::Min),
        TextAlign::Center => egui::Layout::top_down(Align::Center),
        TextAlign::Right => egui::Layout::right_to_left(Align::Min),
    };
    ui.allocate_ui_with_layout(Vec2::new(avail, 0.0), layout, |ui| {
        let resp = match texture {
            Some(t) => ui.add(egui::Image::new((t, size)).sense(if link.is_some() { Sense::click() } else { Sense::hover() })),
            None if size == Vec2::ZERO => ui.add(egui::Label::new(egui::RichText::new(format!("🖼 {alt}")).color(Color32::from_gray(0x77)).size(12.5)).sense(Sense::click())),
            None => {
                let (r, resp) = ui.allocate_exact_size(size, Sense::click());
                ui.painter().rect_filled(r, 4.0, Color32::from_gray(0xee));
                if !alt.is_empty() {
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, alt, FontId::proportional(12.0), Color32::from_gray(0x88));
                }
                resp
            }
        };
        if let Some(link) = link {
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(link);
            if resp.clicked() {
                open_link(link);
            }
        }
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(blocks: &[Block]) -> Vec<String> {
        blocks
            .iter()
            .filter_map(|b| match b {
                Block::Para { runs, .. } => Some(
                    runs.iter()
                        .map(|r| match r {
                            Run::Text(t, _) => t.clone(),
                            Run::Break => "\n".into(),
                            Run::Image { alt, .. } => format!("[{alt}]"),
                        })
                        .collect(),
                ),
                Block::Rule => None,
            })
            .collect()
    }

    #[test]
    fn reads_a_mail() {
        let html = r#"<!DOCTYPE html><html><head><title>T</title><style>p { color: red; }</style></head>
            <body><div style="display:none;max-height:0;overflow:hidden">Preheader&zwnj;&nbsp;</div>
            <h1>Bienvenue&nbsp;!</h1><p>Bonjour <b>Julien</b>,<br>voici   ton <a href="https://x.test/a?b=1&amp;c=2">lien</a>.</p>
            <table><tr><td>Total</td><td>12&euro;</td></tr><tr><td>TVA</td><td>2 &#8364;</td></tr></table>
            <ul><li>un<li>deux</ul><ol><li>a</li><li>b</li></ol><img src="cid:logo" alt="Logo" width="1" height="1"><img src="cid:x" alt="X">
            <!--[if mso]><p>Outlook</p><![endif]--></body></html>"#;
        let blocks = parse(html);
        assert_eq!(texts(&blocks), ["Bienvenue\u{a0}!", "Bonjour Julien,\nvoici ton lien.", "Total   12€", "TVA   2 €", "•  un", "•  deux", "1. a", "2. b", "[X]"]);
        let Block::Para { runs, .. } = &blocks[1] else { panic!() };
        assert!(runs.iter().any(|r| matches!(r, Run::Text(t, s) if t == "lien" && s.link.as_deref() == Some("https://x.test/a?b=1&c=2"))));
        assert!(runs.iter().any(|r| matches!(r, Run::Text(t, s) if t == "Julien" && s.bold)));
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#fff"), Some(Color32::WHITE));
        assert_eq!(parse_color("#1A5FD0"), Some(Color32::from_rgb(0x1a, 0x5f, 0xd0)));
        assert_eq!(parse_color("rgb(10, 20, 30)"), Some(Color32::from_rgb(10, 20, 30)));
        assert_eq!(parse_color("transparent"), None);
    }
}

