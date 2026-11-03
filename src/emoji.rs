//! Emojis in colour: Twemoji images (64 px, CC-BY 4.0) drawn by Ronnie, egui's fonts having them in
//! black and white only; each one is decoded the first time it shows (a texture of 16 kB), the whole
//! sheet would take 67 MB. The list comes from emoji-datasource (MIT) with Unicode CLDR's French
//! names and keywords (see assets/emoji/LICENSE.md): the picker, the search (French and English), the
//! `:name:` completion, and finding the emojis in a text to draw them.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The images, one PNG after the other, and where each is ("x y offset length": its cell on the sheet
/// it was cut from, its place in the pack).
const PACK: &[u8] = include_bytes!("../assets/emoji/twemoji-64.bin");
const INDEX: &str = include_str!("../assets/emoji/twemoji-64.idx");
const DATA: &str = include_str!("../assets/emoji/emoji.tsv");
/// The longest emoji, in chars (families, flags of regions…).
const LONGEST: usize = 12;

/// The most used ones: first at equal score in a search, and the "recent" ones before any is used.
pub const POPULAR: &[&str] = &[
    "👍", "❤️", "😂", "🎉", "🙏", "😅", "😊", "😍", "🔥", "👌", "😉", "🤔", "😭", "👀", "💪", "😎", "🙌", "👏", "✅", "❌", "😢", "😡", "🤣", "😘", "🥳", "😬", "🙄", "😴", "🤷", "🤦", "💯", "✨", "☕", "🍺", "🍕", "🚀", "💩", "🤘", "👋", "😱",
];

/// The picker's categories (the last, "Component", holds the skin tones alone: not shown).
pub const CATEGORIES: usize = 9;

/// An emoji and its skin tones.
pub struct Emoji {
    pub chars: String,
    pub category: u8,
    cell: (u16, u16),
    /// Short names (English, as in `:thumbsup:`).
    pub names: Vec<String>,
    /// Its French name, and its French keywords.
    pub name: String,
    pub keywords: Vec<String>,
    /// Tone (1 light – 5 dark), its chars, its cell.
    skins: Vec<(u8, String, (u16, u16))>,
}

impl Emoji {
    /// Itself with this skin tone (0: none), when it has tones.
    pub fn toned(&self, tone: u8) -> (&str, (u16, u16)) {
        self.skins.iter().find(|(t, _, _)| *t == tone).map_or((self.chars.as_str(), self.cell), |(_, c, cell)| (c.as_str(), *cell))
    }

    #[cfg(test)]
    pub fn has_skins(&self) -> bool {
        !self.skins.is_empty()
    }
}

/// Every emoji, in the picker's order.
pub fn all() -> &'static [Emoji] {
    static ALL: OnceLock<Vec<Emoji>> = OnceLock::new();
    ALL.get_or_init(|| DATA.lines().filter_map(parse).collect())
}

fn parse(line: &str) -> Option<Emoji> {
    let mut f = line.split('\t');
    let category = f.next()?.parse().ok()?;
    let chars = f.next()?.to_owned();
    let cell = (f.next()?.parse().ok()?, f.next()?.parse().ok()?);
    let list = |s: &str| s.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let names = list(f.next()?);
    let name = f.next()?.to_owned();
    let keywords = list(f.next()?);
    let skins = f
        .next()
        .unwrap_or("")
        .split('|')
        .filter_map(|s| {
            let mut p = s.split(':');
            Some((p.next()?.parse().ok()?, p.next()?.to_owned(), (p.next()?.parse().ok()?, p.next()?.parse().ok()?)))
        })
        .collect();
    Some(Emoji { chars, category, cell, names, name, keywords, skins })
}

/// Without the variation selector (U+FE0F): "❤️" and "❤" are the same emoji.
fn key(s: &str) -> String {
    s.chars().filter(|c| *c != '\u{fe0f}').collect()
}

/// Every emoji's chars (tones included, without U+FE0F) → its cell.
fn cells() -> &'static HashMap<String, (u16, u16)> {
    static CELLS: OnceLock<HashMap<String, (u16, u16)>> = OnceLock::new();
    CELLS.get_or_init(|| {
        let mut map = HashMap::new();
        for e in all() {
            map.insert(key(&e.chars), e.cell);
            for (_, chars, cell) in &e.skins {
                map.insert(key(chars), *cell);
            }
        }
        map
    })
}

/// An emoji's cell on the sheet, by its chars (tones included).
pub fn cell(chars: &str) -> Option<(u16, u16)> {
    cells().get(&key(chars)).copied()
}

/// A piece of text: plain, or an emoji (its chars, its cell on the sheet).
#[derive(Debug, PartialEq)]
pub enum Piece<'a> {
    Text(&'a str),
    Emoji(&'a str, (u16, u16)),
}

/// Whether this char may begin an emoji (digits, # and * only as keycaps: checked by the lookup).
fn may_start(c: char) -> bool {
    !c.is_ascii() || matches!(c, '#' | '*' | '0'..='9')
}

/// The text cut into plain runs and emojis (the longest emoji at each place).
pub fn pieces(text: &str) -> Vec<Piece<'_>> {
    let cells = cells();
    let mut out = Vec::new();
    let mut plain = 0;
    let bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    let mut k = 0;
    while k + 1 < bounds.len() {
        let start = bounds[k];
        let first = text[start..].chars().next().unwrap_or(' ');
        let mut found = None;
        if may_start(first) {
            // The longest match first, so that a family isn't read as its people.
            for n in (1..=LONGEST.min(bounds.len() - 1 - k)).rev() {
                let candidate = &text[start..bounds[k + n]];
                // A lone digit or # isn't an emoji (its keycap is).
                if n == 1 && first.is_ascii() {
                    continue;
                }
                if let Some(cell) = cells.get(&key(candidate)) {
                    found = Some((n, *cell));
                    break;
                }
            }
        }
        match found {
            Some((n, cell)) => {
                if plain < start {
                    out.push(Piece::Text(&text[plain..start]));
                }
                let end = bounds[k + n];
                // Its variation selector, if one follows, goes with it.
                let end = if text[end..].starts_with('\u{fe0f}') { end + '\u{fe0f}'.len_utf8() } else { end };
                out.push(Piece::Emoji(&text[start..end], cell));
                plain = end;
                while k < bounds.len() && bounds[k] < end {
                    k += 1;
                }
            }
            None => k += 1,
        }
    }
    if plain < text.len() {
        out.push(Piece::Text(&text[plain..]));
    }
    out
}

/// Whether the text has an emoji at all (else it is drawn as before, as one label).
pub fn has_emoji(text: &str) -> bool {
    text.chars().any(may_start) && pieces(text).iter().any(|p| matches!(p, Piece::Emoji(..)))
}

/// The number of emojis when the text is only emojis (and spaces), 1 to 3: shown big. None otherwise.
pub fn only_emojis(text: &str) -> Option<usize> {
    let pieces = pieces(text.trim());
    let count = pieces.iter().filter(|p| matches!(p, Piece::Emoji(..))).count();
    let only = pieces.iter().all(|p| match p {
        Piece::Emoji(..) => true,
        Piece::Text(t) => t.trim().is_empty(),
    });
    (only && (1..=3).contains(&count)).then_some(count)
}

/// Lowercase without accents, to compare "cafe" and "Café".
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .flat_map(|c| match c {
            'œ' => "oe".chars().collect::<Vec<_>>(),
            'æ' => "ae".chars().collect(),
            c => vec![c],
        })
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            '’' => '\'',
            c => c,
        })
        .collect()
}

/// The emojis matching a search (French or English), the best first: a name beginning so, then a
/// keyword, then a word of them, then anywhere.
pub fn search(query: &str, limit: usize) -> Vec<&'static Emoji> {
    let q = fold(query.trim().trim_matches(':'));
    if q.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u8, usize, &Emoji)> = all()
        .iter()
        .enumerate()
        .filter(|(_, e)| usize::from(e.category) < CATEGORIES)
        .filter_map(|(i, e)| {
            let name = fold(&e.name);
            let names = e.names.iter().map(|n| fold(n));
            let keywords: Vec<String> = e.keywords.iter().map(|k| fold(k)).collect();
            let score = if names.clone().any(|n| n == q) || name == q {
                0
            } else if names.clone().any(|n| n.starts_with(&q)) || name.starts_with(&q) {
                1
            } else if keywords.iter().any(|k| k.starts_with(&q)) {
                2
            } else if name.split([' ', '-', '_']).chain(keywords.iter().flat_map(|k| k.split(' '))).any(|w| w.starts_with(&q)) || names.clone().any(|n| n.split('_').any(|w| w.starts_with(&q))) {
                3
            } else if name.contains(&q) || keywords.iter().any(|k| k.contains(&q)) {
                4
            } else {
                return None;
            };
            Some((score, i, e))
        })
        .collect();
    // At equal score, the most used first, then the list's order.
    let popular = |e: &Emoji| POPULAR.iter().position(|p| key(p) == key(&e.chars)).unwrap_or(usize::MAX);
    scored.sort_by_key(|(score, i, e)| (*score, popular(e), *i));
    scored.into_iter().take(limit).map(|(_, _, e)| e).collect()
}

/// An emoji by its chars (tones included), with the tone it has.
pub fn find(chars: &str) -> Option<&'static Emoji> {
    let k = key(chars);
    all().iter().find(|e| key(&e.chars) == k || e.skins.iter().any(|(_, c, _)| key(c) == k))
}

/// Each cell's image in the pack (its offset and length).
fn places() -> &'static HashMap<(u16, u16), (usize, usize)> {
    static PLACES: OnceLock<HashMap<(u16, u16), (usize, usize)>> = OnceLock::new();
    PLACES.get_or_init(|| {
        INDEX
            .lines()
            .filter_map(|line| {
                let mut f = line.split(' ').map(str::parse::<usize>);
                let (x, y, offset, length) = (f.next()?.ok()?, f.next()?.ok()?, f.next()?.ok()?, f.next()?.ok()?);
                Some(((x as u16, y as u16), (offset, length)))
            })
            .collect()
    })
}

/// The emojis' images, each made a texture the first time it is drawn (kept: a few hundred at most
/// are ever shown). Cloned freely: the clones share them.
#[derive(Clone)]
pub struct Faces {
    ctx: egui::Context,
    loaded: std::sync::Arc<std::sync::Mutex<Loaded>>,
}

/// Each cell's texture once made (None: its image couldn't be read).
type Loaded = HashMap<(u16, u16), Option<egui::TextureHandle>>;

impl Faces {
    /// This cell's texture (None: not in the pack, or not readable).
    pub fn get(&self, cell: (u16, u16)) -> Option<egui::TextureHandle> {
        let mut loaded = self.loaded.lock().ok()?;
        loaded
            .entry(cell)
            .or_insert_with(|| {
                let (offset, length) = *places().get(&cell)?;
                let image = image::load_from_memory(PACK.get(offset..offset + length)?).ok()?.to_rgba8();
                let picture = egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
                Some(self.ctx.load_texture(format!("emoji-{}-{}", cell.0, cell.1), picture, egui::TextureOptions::LINEAR))
            })
            .clone()
    }

    /// An emoji drawn in `rect`.
    pub fn paint(&self, painter: &egui::Painter, cell: (u16, u16), rect: egui::Rect) {
        self.paint_tinted(painter, cell, rect, egui::Color32::WHITE);
    }

    pub fn paint_tinted(&self, painter: &egui::Painter, cell: (u16, u16), rect: egui::Rect, tint: egui::Color32) {
        if let Some(texture) = self.get(cell) {
            painter.image(texture.id(), rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), tint);
        }
    }

    /// An emoji as a widget of this size (empty, the same size, when it can't be read).
    pub fn image(&self, cell: (u16, u16), size: egui::Vec2) -> egui::Image<'static> {
        match self.get(cell) {
            Some(texture) => egui::Image::new((texture.id(), size)),
            None => egui::Image::new((egui::TextureId::default(), size)).tint(egui::Color32::TRANSPARENT),
        }
    }
}

/// The faces, made once for the app's context.
#[derive(Default)]
pub struct Sheet {
    faces: Option<Faces>,
}

impl Sheet {
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<&Faces> {
        Some(self.faces.get_or_insert_with(|| Faces { ctx: ctx.clone(), loaded: Default::default() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emoji_has_its_image() {
        let places = places();
        for e in all() {
            assert!(places.contains_key(&e.cell), "{} has no image", e.chars);
            for (_, chars, cell) in &e.skins {
                assert!(places.contains_key(cell), "{chars} has no image");
            }
        }
        // They are PNGs of 64 px.
        let (offset, length) = places[&find("👍").unwrap().cell];
        let image = image::load_from_memory(&PACK[offset..offset + length]).unwrap();
        assert_eq!((image.width(), image.height()), (64, 64));
    }

    #[test]
    fn reads_the_list() {
        assert!(all().len() > 1800);
        let thumbs = find("👍").unwrap();
        assert!(thumbs.names.contains(&"thumbsup".to_owned()));
        assert_eq!(thumbs.name, "pouce vers le haut");
        assert!(thumbs.has_skins());
        assert_eq!(thumbs.toned(5).0, "👍🏿");
        assert_eq!(thumbs.toned(0).0, "👍");
    }

    #[test]
    fn finds_the_emojis_in_a_text() {
        let p = pieces("salut 👍🏽 ça va ? ❤️ 👨‍👩‍👧 #1 1️⃣");
        let emojis: Vec<&str> = p.iter().filter_map(|p| if let Piece::Emoji(c, _) = p { Some(*c) } else { None }).collect();
        assert_eq!(emojis, ["👍🏽", "❤️", "👨‍👩‍👧", "1️⃣"]);
        assert_eq!(p[0], Piece::Text("salut "));
        assert!(!has_emoji("rien ici, #1 et 2*3"));
        assert_eq!(pieces("abc"), [Piece::Text("abc")]);
        // A heart without its selector is the same emoji.
        assert!(has_emoji("❤"));
    }

    #[test]
    fn only_emojis_are_shown_big() {
        assert_eq!(only_emojis(" 😂 "), Some(1));
        assert_eq!(only_emojis("🎉🎉🎉"), Some(3));
        assert_eq!(only_emojis("🎉🎉🎉🎉"), None);
        assert_eq!(only_emojis("ok 👍"), None);
        assert_eq!(only_emojis(""), None);
    }

    #[test]
    fn searches_in_french_and_english() {
        let first = |q: &str| search(q, 5).first().map(|e| e.chars.clone());
        assert_eq!(first("thumbsup").as_deref(), Some("👍"));
        assert_eq!(first("pouce").as_deref(), Some("👍"));
        assert!(search("mdr", 10).iter().any(|e| e.chars == "😂"));
        assert!(search("coeur", 10).iter().any(|e| key(&e.chars) == "❤"));
        assert!(search("café", 10).iter().any(|e| e.chars == "☕"));
        assert!(search(":tada", 3).iter().any(|e| e.chars == "🎉"));
        assert!(search("zzzzqq", 5).is_empty());
    }
}
