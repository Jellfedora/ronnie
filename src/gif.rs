//! GIFs: an animated GIF's frames (made smaller, within a budget of pixels: a long GIF loses every
//! other frame rather than eat the memory), and the GIF search through floor (KLIPY's, floor keeping
//! the key and passing the previews on: see floor's src/talk/gifs.js).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

/// The frames of an animation, each with how long it shows (ms).
pub type Frames = Vec<(egui::ColorImage, u32)>;

/// Frames at most, whatever the budget.
const FRAMES_MAX: usize = 600;

/// An animated GIF's frames, no side above `max_side` and all of them `budget` pixels at most (else
/// every other frame goes, its time given to the one kept). None: not a GIF, or a single frame.
pub fn frames(bytes: &[u8], max_side: u32, budget: u64) -> Option<Frames> {
    use image::AnimationDecoder as _;
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?;
    let mut out: Vec<(image::RgbaImage, u32)> = Vec::new();
    for frame in decoder.into_frames().take(FRAMES_MAX) {
        let frame = frame.ok()?;
        let (n, d) = frame.delay().numer_denom_ms();
        let ms = n / d.max(1);
        // As browsers do: a frame of (almost) no time shows 100 ms.
        let ms = if ms < 20 { 100 } else { ms };
        let mut buffer = frame.into_buffer();
        let (w, h) = buffer.dimensions();
        if w.max(h) > max_side {
            let scale = max_side as f32 / w.max(h) as f32;
            let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
            buffer = image::imageops::resize(&buffer, nw, nh, image::imageops::FilterType::Triangle);
        }
        out.push((buffer, ms));
    }
    if out.len() < 2 {
        return None;
    }
    let pixels = |f: &[(image::RgbaImage, u32)]| f.iter().map(|(b, _)| u64::from(b.width()) * u64::from(b.height())).sum::<u64>();
    while pixels(&out) > budget && out.len() > 2 {
        out = out.chunks(2).map(|pair| (pair[0].0.clone(), pair.iter().map(|(_, ms)| ms).sum())).collect();
    }
    Some(out.into_iter().map(|(b, ms)| (egui::ColorImage::from_rgba_unmultiplied([b.width() as usize, b.height() as usize], b.as_raw()), ms)).collect())
}

/// An animation made textures: the frame to show at this time (ms), and how long until the next.
pub struct Animation {
    frames: Vec<(egui::TextureHandle, u32)>,
    total: u32,
}

impl Animation {
    pub fn new(ctx: &egui::Context, name: &str, frames: Frames) -> Self {
        let total = frames.iter().map(|(_, ms)| ms).sum::<u32>().max(1);
        let frames = frames.into_iter().enumerate().map(|(k, (image, ms))| (ctx.load_texture(format!("{name}-{k}"), image, egui::TextureOptions::LINEAR), ms)).collect();
        Self { frames, total }
    }

    pub fn size(&self) -> egui::Vec2 {
        self.frames.first().map_or(egui::Vec2::ZERO, |(t, _)| t.size_vec2())
    }

    pub fn at(&self, now_ms: u64) -> (&egui::TextureHandle, Duration) {
        let mut t = (now_ms % u64::from(self.total)) as u32;
        for (texture, ms) in &self.frames {
            if t < *ms {
                return (texture, Duration::from_millis(u64::from(ms - t)));
            }
            t -= ms;
        }
        let (texture, ms) = &self.frames[0];
        (texture, Duration::from_millis(u64::from(*ms)))
    }
}

/// A GIF found: its preview (small, animated, through floor) and the GIF sent (copied by floor).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Gif {
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub preview: Media,
    pub gif: Media,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Media {
    /// The preview: its path on floor; the GIF: KLIPY's address (floor fetches it).
    pub url: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

/// A preview coming.
enum Preview {
    Loading,
    Ready(Frames),
    /// Made textures by the window (or a still image: its single frame).
    Taken,
    Failed,
}

#[derive(Default)]
struct State {
    query: String,
    results: Vec<Gif>,
    next: Option<String>,
    loading: bool,
    error: Option<String>,
    /// Bumped by each new search: the pages of an older one are dropped.
    generation: u64,
    previews: HashMap<String, Preview>,
}

/// The GIF search: the results so far, the next page, the previews.
#[derive(Default)]
pub struct Search {
    state: Arc<Mutex<State>>,
}

/// floor's address and the session's token.
pub type Key = (String, String);

/// What the window shows: the results, whether a page is coming, why not, whether there are more.
pub struct Shown {
    pub query: String,
    pub results: Vec<Gif>,
    pub loading: bool,
    pub error: Option<String>,
    pub more: bool,
}

impl Search {
    /// A new search (empty: the trending GIFs).
    pub fn search(&self, ctx: &egui::Context, key: &Key, query: &str) {
        let generation = {
            let Ok(mut s) = self.state.lock() else { return };
            s.generation += 1;
            s.query = query.to_owned();
            s.results.clear();
            s.next = None;
            s.error = None;
            s.loading = true;
            s.generation
        };
        self.page(ctx, key, query.to_owned(), None, generation);
    }

    /// The next page, if there's one and none is coming.
    pub fn more(&self, ctx: &egui::Context, key: &Key) {
        let (query, pos, generation) = {
            let Ok(mut s) = self.state.lock() else { return };
            let Some(pos) = s.next.clone().filter(|_| !s.loading) else { return };
            s.loading = true;
            (s.query.clone(), pos, s.generation)
        };
        self.page(ctx, key, query, Some(pos), generation);
    }

    fn page(&self, ctx: &egui::Context, key: &Key, query: String, pos: Option<String>, generation: u64) {
        let (state, ctx, (url, token)) = (self.state.clone(), ctx.clone(), key.clone());
        std::thread::spawn(move || {
            #[derive(Deserialize)]
            struct Page {
                results: Vec<Gif>,
                next: Option<String>,
            }
            let mut request = agent().get(format!("{url}/v1/talk/gifs")).header("User-Agent", USER_AGENT).header("Authorization", format!("Bearer {token}"));
            if !query.is_empty() {
                request = request.query("q", &query);
            }
            if let Some(pos) = &pos {
                request = request.query("pos", pos);
            }
            let got = request.call().map_err(|e| e.to_string()).and_then(|mut r| r.body_mut().read_json::<Page>().map_err(|e| e.to_string()));
            if let Ok(mut s) = state.lock()
                && s.generation == generation
            {
                s.loading = false;
                match got {
                    Ok(page) => {
                        // A page without anything new: the end (KLIPY may give a next one forever).
                        let fresh: Vec<Gif> = page.results.into_iter().filter(|g| !s.results.iter().any(|r| r.id == g.id)).collect();
                        s.next = page.next.filter(|n| !n.is_empty() && !fresh.is_empty());
                        s.results.extend(fresh);
                    }
                    Err(e) => s.error = Some(e),
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn shown(&self) -> Option<Shown> {
        let s = self.state.lock().ok()?;
        Some(Shown { query: s.query.clone(), results: s.results.clone(), loading: s.loading, error: s.error.clone(), more: s.next.is_some() })
    }

    /// A preview's frames, once (the window makes them textures); fetched the first time it's asked.
    pub fn preview(&self, ctx: &egui::Context, key: &Key, gif: &Gif) -> Option<Frames> {
        let mut s = self.state.lock().ok()?;
        match s.previews.get_mut(&gif.id) {
            Some(p @ Preview::Ready(_)) => match std::mem::replace(p, Preview::Taken) {
                Preview::Ready(frames) => Some(frames),
                _ => None,
            },
            Some(_) => None,
            None => {
                s.previews.insert(gif.id.clone(), Preview::Loading);
                let (state, ctx, (url, token), id, path) = (self.state.clone(), ctx.clone(), key.clone(), gif.id.clone(), gif.preview.url.clone());
                std::thread::spawn(move || {
                    let got = agent()
                        .get(format!("{url}/{}", path.trim_start_matches('/')))
                        .header("User-Agent", USER_AGENT)
                        .header("Authorization", format!("Bearer {token}"))
                        .call()
                        .ok()
                        .and_then(|mut r| r.body_mut().with_config().limit(8 * 1024 * 1024).read_to_vec().ok());
                    // Animated: its frames; a still image: one frame.
                    let preview = got
                        .and_then(|bytes| {
                            frames(&bytes, 240, 4_000_000).or_else(|| {
                                let image = image::load_from_memory(&bytes).ok()?.thumbnail(240, 240).to_rgba8();
                                Some(vec![(egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw()), 1000)])
                            })
                        })
                        .map_or(Preview::Failed, Preview::Ready);
                    if let Ok(mut s) = state.lock()
                        && s.previews.contains_key(&id)
                    {
                        s.previews.insert(id, preview);
                    }
                    ctx.request_repaint();
                });
                None
            }
        }
    }

    /// Closed: everything forgotten (the previews' memory freed).
    pub fn clear(&self) {
        if let Ok(mut s) = self.state.lock() {
            *s = State { generation: s.generation + 1, ..Default::default() };
        }
    }
}

const USER_AGENT: &str = concat!("ronnie/", env!("CARGO_PKG_VERSION"));

fn agent() -> ureq::Agent {
    ureq::config::Config::builder().timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(20))).build().new_agent()
}

/// A GIF found, sent to a conversation: floor copies it (an error: floor's word).
pub fn send(key: &Key, room: &str, gif: &Gif, query: &str) -> Result<(), String> {
    let (url, token) = key;
    let room: String = room.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    let body = serde_json::json!({ "url": gif.gif.url, "title": gif.title, "id": gif.id, "q": query });
    ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(40)))
        .http_status_as_error(false)
        .build()
        .new_agent()
        .post(format!("{url}/v1/talk/rooms/{room}/gif"))
        .header("User-Agent", USER_AGENT)
        .header("Authorization", format!("Bearer {token}"))
        .send_json(&body)
        .map_err(|e| e.to_string())
        .and_then(|mut r| {
            if r.status().is_success() {
                return Ok(());
            }
            #[derive(Deserialize)]
            struct Refusal {
                error: String,
            }
            Err(r.body_mut().read_json::<Refusal>().map(|r| r.error).unwrap_or_else(|_| r.status().to_string()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF of `n` frames of `side` px, each of `delay` hundredths of a second.
    fn animated(n: u32, side: u16, delay: u16) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut out);
            encoder.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
            let frames = (0..n).map(|k| {
                let buffer = image::RgbaImage::from_pixel(u32::from(side), u32::from(side), image::Rgba([(k * 40) as u8, 0, 0, 255]));
                image::Frame::from_parts(buffer, 0, 0, image::Delay::from_numer_denom_ms(u32::from(delay) * 10, 1))
            });
            encoder.encode_frames(frames).unwrap();
        }
        out
    }

    #[test]
    fn reads_the_frames_within_the_budget() {
        let f = frames(&animated(4, 50, 10), 480, 1_000_000).unwrap();
        assert_eq!(f.len(), 4);
        assert_eq!(f[0].1, 100);
        assert_eq!(f[0].0.size, [50, 50]);
        // Made smaller.
        let f = frames(&animated(3, 200, 5), 100, 1_000_000).unwrap();
        assert_eq!(f[0].0.size, [100, 100]);
        // Over the budget: every other frame, with its time.
        let f = frames(&animated(8, 100, 10), 480, 4 * 100 * 100).unwrap();
        assert_eq!(f.len(), 4);
        assert_eq!(f[0].1, 200);
        // A single frame: not an animation.
        assert!(frames(&animated(1, 10, 10), 480, 1_000_000).is_none());
        assert!(frames(b"pas un gif", 480, 1_000_000).is_none());
    }

    /// The search the whole way, on a floor running (RONNIE_FLOOR_URL) with a KLIPY key: trending,
    /// found, the next page, an animated preview through floor.
    #[test]
    #[ignore = "needs a floor running, at RONNIE_FLOOR_URL, with KLIPY_KEY"]
    fn searches_through_floor() {
        let url = std::env::var("RONNIE_FLOOR_URL").expect("RONNIE_FLOOR_URL");
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1_000_000;
        let made: serde_json::Value = ureq::post(format!("{url}/v1/accounts")).send_json(serde_json::json!({ "name": format!("Gif{stamp}"), "password": "motdepasse" })).unwrap().body_mut().read_json().unwrap();
        let key: Key = (url, made["token"].as_str().unwrap().to_owned());
        let ctx = egui::Context::default();
        let search = Search::default();
        let wait = |done: &dyn Fn(&Shown) -> bool| {
            let start = std::time::Instant::now();
            loop {
                let shown = search.shown().unwrap();
                if done(&shown) {
                    return shown;
                }
                assert!(start.elapsed() < Duration::from_secs(20), "waiting");
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        search.search(&ctx, &key, "");
        let trending = wait(&|s| !s.loading);
        assert!(trending.error.is_none(), "{:?}", trending.error);
        assert!(trending.results.len() >= 10);
        search.search(&ctx, &key, "chat");
        let found = wait(&|s| !s.loading);
        assert!(found.more);
        let first = found.results[0].clone();
        search.more(&ctx, &key);
        let more = wait(&|s| !s.loading && s.results.len() > found.results.len());
        assert!(more.results.len() > found.results.len());
        // Its preview, animated, through floor.
        assert!(search.preview(&ctx, &key, &first).is_none(), "fetched in the background");
        let start = std::time::Instant::now();
        let frames = loop {
            if let Some(frames) = search.preview(&ctx, &key, &first) {
                break frames;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "waiting for the preview");
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(!frames.is_empty());
        assert!(frames[0].0.size[0] <= 240);
    }

    #[test]
    fn plays_the_frames_in_time() {
        let ctx = egui::Context::default();
        let f = frames(&animated(3, 10, 10), 480, 1_000_000).unwrap();
        let a = Animation::new(&ctx, "test", f);
        let id = |ms| a.at(ms).0.id();
        assert_eq!(id(0), id(99));
        assert_ne!(id(0), id(100));
        assert_eq!(id(0), id(300), "it loops");
        assert_eq!(a.at(150).1, Duration::from_millis(50));
    }
}
