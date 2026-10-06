//! The music: a radio of songs drawn at random from the Subsonic server (Navidrome…), of one genre or of all, and
//! its card at the bottom of the sidebar.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{mpsc, Arc, Condvar, Mutex};

use super::*;
use crate::subsonic::{Failure, Genre, Server, Song};

/// Songs asked for at a time; more once only a few are left.
const BATCH: u32 = 40;
/// Downloads failed in a row before the radio gives up.
const MAX_FAILURES: u32 = 4;
const CARD_H: f32 = 86.0;
const IDLE_H: f32 = 32.0;

enum Msg {
    Genres(Result<Vec<Genre>, Failure>),
    /// For the radio of that generation.
    Songs(u64, Result<Vec<Song>, Failure>),
    Track(u64, Box<Song>, Result<Box<Track>, Failure>),
    Liked(Result<Vec<Song>, Failure>),
    /// A like (or its removal) the server refused: the song, and what was asked.
    LikeFailed(String, bool, Failure),
}

/// What the radio plays.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Station {
    /// Songs drawn at random, of a genre (None: any).
    Genre(Option<String>),
    /// The songs liked, shuffled, again and again.
    Liked,
    /// An album, in order (its songs asked for once).
    Album { id: String, name: String },
    /// Songs given (a playlist, a search…), in order, then it ends.
    List(String),
}

impl Station {
    /// Its songs come once (it ends after them), rather than again and again.
    fn finite(&self) -> bool {
        matches!(self, Self::Album { .. } | Self::List(_))
    }
}

/// Songs played before the one playing, for "previous".
const HISTORY: usize = 50;

/// A song's bytes as they arrive (filled by a thread of their own), shared with its reader.
#[derive(Default)]
struct Buffer {
    data: Vec<u8>,
    done: bool,
    /// Skipped or stopped: the reader ends, the download stops.
    cancelled: bool,
}

type Shared = Arc<(Mutex<Buffer>, Condvar)>;

fn cancel(shared: &Shared) {
    shared.0.lock().unwrap().cancelled = true;
    shared.1.notify_all();
}

/// The decoder's side: reads wait for what hasn't arrived yet, so the song plays while it downloads.
struct Growing {
    shared: Shared,
    pos: u64,
    len: Option<u64>,
}

impl Read for Growing {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let (lock, arrived) = &*self.shared;
        let mut buffer = lock.lock().unwrap();
        loop {
            if buffer.cancelled {
                return Ok(0);
            }
            let have = buffer.data.len() as u64;
            if have > self.pos {
                let n = out.len().min((have - self.pos) as usize);
                let from = self.pos as usize;
                out[..n].copy_from_slice(&buffer.data[from..from + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            if buffer.done {
                return Ok(0);
            }
            buffer = arrived.wait(buffer).unwrap();
        }
    }
}

impl Seek for Growing {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => {
                // The size unknown: known once all of it is there.
                let len = match self.len {
                    Some(len) => len,
                    None => {
                        let (lock, arrived) = &*self.shared;
                        let buffer = arrived.wait_while(lock.lock().unwrap(), |b| !b.done && !b.cancelled).unwrap();
                        buffer.data.len() as u64
                    }
                };
                len as i64 + d
            }
        };
        self.pos = u64::try_from(pos).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

/// A song ready to play: its decoder, reading from what arrives.
struct Track {
    source: rodio::Decoder<Growing>,
    shared: Shared,
    len: Option<u64>,
}

/// Starts the song's download and opens its decoder as soon as enough has come to know its format.
fn open(server: &Server, song: &Song) -> Result<Track, Failure> {
    let stream = server.stream(song)?;
    let shared: Shared = Arc::default();
    let filler = shared.clone();
    let mut reader = stream.reader;
    std::thread::spawn(move || {
        let mut chunk = vec![0; 64 * 1024];
        loop {
            let read = reader.read(&mut chunk);
            let (lock, arrived) = &*filler;
            let mut buffer = lock.lock().unwrap();
            match read {
                _ if buffer.cancelled => return,
                Ok(0) => buffer.done = true,
                Ok(n) => buffer.data.extend_from_slice(&chunk[..n]),
                Err(e) => {
                    crate::log::error(&format!("music: download: {e}"));
                    buffer.done = true;
                }
            }
            let done = buffer.done;
            drop(buffer);
            arrived.notify_all();
            if done {
                return;
            }
        }
    });
    let growing = Growing { shared: shared.clone(), pos: 0, len: stream.len };
    let mut builder = rodio::Decoder::builder().with_data(growing).with_seekable(true);
    if let Some(len) = stream.len {
        builder = builder.with_byte_len(len);
    }
    match builder.build() {
        Ok(source) => Ok(Track { source, shared, len: stream.len }),
        Err(e) => {
            cancel(&shared);
            Err(Failure::Server(format!("{} : {e}", song.title)))
        }
    }
}

/// The sound device (closed when dropped) and what plays on it.
struct Audio {
    device: rodio::MixerDeviceSink,
    player: rodio::Player,
}

pub(super) struct Music {
    server: Option<Server>,
    genres: Option<Result<Vec<Genre>, String>>,
    genres_loading: bool,
    /// The radio playing; None when stopped.
    station: Option<Station>,
    /// The songs liked, as the server last said (and as changed here since).
    liked: Option<Result<Vec<Song>, String>>,
    liked_loading: bool,
    /// Bumped at each new radio: what arrives for an older one is dropped.
    generation: u64,
    /// Songs to come, then the next one, ready (arriving while the other one plays).
    queue: VecDeque<Song>,
    asking: bool,
    next: Option<(Song, Box<Track>)>,
    /// The download of the song playing (how much is there, to stop it).
    playing_stream: Option<(Shared, Option<u64>)>,
    downloading: bool,
    current: Option<Song>,
    audio: Option<Audio>,
    paused: bool,
    /// Paused for a round of the typing game: played again after it.
    held: bool,
    /// The progress bar dragged: where it is, 0 to 1 (the song moves there when let go of).
    scrub: Option<f32>,
    /// A finite radio's songs have all come.
    all_asked: bool,
    history: Vec<Song>,
    /// The song playing was told to the server as played.
    scrobbled: bool,
    failures: u32,
    error: Option<String>,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
}

impl Default for Music {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { server: None, genres: None, genres_loading: false, station: None, liked: None, liked_loading: false, generation: 0, queue: VecDeque::new(), asking: false, next: None, playing_stream: None, downloading: false, current: None, audio: None, paused: false, held: false, scrub: None, all_asked: false, history: Vec::new(), scrobbled: false, failures: 0, error: None, tx, rx }
    }
}

/// The configured server, with its saved password; None until both are set.
pub(super) fn configured_server(settings: &config::Settings) -> Option<Server> {
    let server = &settings.subsonic;
    if server.url.is_empty() || !server.password_saved {
        return None;
    }
    let password = ssh::load_password(crate::subsonic::PASSWORD_ID)?;
    Some(Server::new(&server.url, &server.user, &password))
}

pub(super) fn describe(failure: &Failure, t: &Strings) -> String {
    match failure {
        Failure::Unreachable(e) => format!("{} : {e}", t.subsonic_unreachable),
        Failure::Refused => t.subsonic_refused.to_owned(),
        Failure::NotSubsonic => t.subsonic_not_subsonic.to_owned(),
        Failure::Server(e) => e.clone(),
        Failure::NoToken => t.subsonic_refused.to_owned(),
    }
}

impl Music {
    pub(super) fn playing(&self) -> bool {
        self.station.is_some()
    }

    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&Server) -> Msg + Send + 'static) {
        let Some(server) = self.server.clone() else { return };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(&server));
            ctx.request_repaint();
        });
    }

    /// A call whose answer doesn't matter.
    fn spawn_quiet(&self, job: impl FnOnce(&Server) + Send + 'static) {
        let Some(server) = self.server.clone() else { return };
        std::thread::spawn(move || job(&server));
    }

    /// The genres, asked for once (again after a failure).
    fn load_genres(&mut self, ctx: &egui::Context, server: Option<Server>) {
        if self.genres_loading || matches!(self.genres, Some(Ok(_))) {
            return;
        }
        if server.is_some() {
            self.server = server;
        }
        self.genres_loading = true;
        self.genres = None;
        self.spawn(ctx, |server| Msg::Genres(server.genres()));
    }

    /// The songs liked, asked for again each time the menu opens.
    fn load_liked(&mut self, ctx: &egui::Context) {
        if self.liked_loading {
            return;
        }
        self.liked_loading = true;
        self.spawn(ctx, |server| Msg::Liked(server.liked()));
    }

    /// A new radio; `first`: the songs it starts with.
    pub(super) fn start(&mut self, server: Option<Server>, station: Station, first: Vec<Song>) {
        self.stop();
        if server.is_some() {
            self.server = server;
        }
        // Given whole: nothing more to ask for.
        self.all_asked = matches!(station, Station::List(_));
        self.station = Some(station);
        self.queue.extend(first);
        self.error = None;
    }

    pub(super) fn current(&self) -> Option<&Song> {
        self.current.as_ref()
    }

    /// Likes the song playing, or no longer.
    fn toggle_like(&mut self, ctx: &egui::Context) -> Option<(String, bool)> {
        let song = self.current.as_ref()?;
        let (id, on) = (song.id.clone(), !song.liked());
        self.like(ctx, song.clone(), on);
        Some((id, on))
    }

    /// Likes `song` (`on`), or no longer: shown at once, then told to the server.
    pub(super) fn like(&mut self, ctx: &egui::Context, mut song: Song, on: bool) {
        song.starred = on.then(|| chrono::Local::now().to_rfc3339());
        for s in self.current.iter_mut().chain(self.queue.iter_mut()).chain(self.next.iter_mut().map(|(s, _)| s)).filter(|s| s.id == song.id) {
            s.starred = song.starred.clone();
        }
        if let Some(Ok(liked)) = &mut self.liked {
            liked.retain(|s| s.id != song.id);
            if on {
                liked.insert(0, song.clone());
            }
        }
        let id = song.id;
        self.spawn(ctx, move |server| match server.like(&id, on) {
            Ok(()) => Msg::Liked(server.liked()),
            Err(e) => Msg::LikeFailed(id, on, e),
        });
    }

    /// The song before (or this one from its start, when well into it).
    fn previous(&mut self, volume: f32) {
        if self.position() > Duration::from_secs(4) || self.history.is_empty() {
            self.seek(Duration::ZERO);
            return;
        }
        let Some(before) = self.history.pop() else { return };
        // The one ahead goes back in line, after the one playing.
        if let Some((song, track)) = self.next.take() {
            cancel(&track.shared);
            self.queue.push_front(song);
        }
        if let Some(song) = self.current.clone() {
            self.queue.push_front(song);
        }
        self.queue.push_front(before);
        self.skip_without_history(volume);
    }

    /// The downloads stopped, and the reader of the one playing let go (it may be waiting for data).
    fn cancel_streams(&mut self) {
        if let Some((_, track)) = self.next.take() {
            cancel(&track.shared);
        }
        if let Some((shared, _)) = self.playing_stream.take() {
            cancel(&shared);
        }
    }

    pub(super) fn stop(&mut self) {
        self.generation += 1;
        self.station = None;
        self.history.clear();
        self.all_asked = false;
        self.queue.clear();
        self.cancel_streams();
        self.current = None;
        self.asking = false;
        self.downloading = false;
        self.paused = false;
        self.held = false;
        self.failures = 0;
        // The device closed too: nothing keeps the sound output busy.
        self.audio = None;
    }

    /// What the media keys (or the system's player) asked.
    pub(super) fn command(&mut self, command: crate::media_keys::Command, volume: f32) {
        use crate::media_keys::Command;
        match command {
            Command::Toggle => self.toggle_pause(),
            Command::Play if self.paused => self.toggle_pause(),
            Command::Pause if !self.paused => self.toggle_pause(),
            Command::Play | Command::Pause => {}
            Command::Next => self.skip(volume),
            Command::Previous => self.previous(volume),
            Command::Stop => self.stop(),
            Command::Seek(secs) => self.seek(Duration::from_secs_f64(secs.max(0.0))),
        }
    }

    /// The song, as shown in the system's player; None between two songs.
    pub(super) fn now_playing(&self) -> Option<crate::media_keys::NowPlaying> {
        let song = self.current.as_ref()?;
        Some(crate::media_keys::NowPlaying { title: song.title.clone(), artist: song.artist.clone(), album: song.album.clone(), duration: song.duration as f64, elapsed: self.position().as_secs_f64(), paused: self.paused })
    }

    fn seek(&mut self, to: Duration) {
        let (Some(audio), Some(song), Some((shared, len))) = (&self.audio, &self.current, &self.playing_stream) else { return };
        // Not past what has arrived (the sound would wait for it, and the window with it).
        let buffer = shared.0.lock().unwrap();
        let ready = match len {
            _ if buffer.done => 1.0,
            Some(len) if *len > 0 => buffer.data.len() as f64 / *len as f64 * 0.97,
            _ => 0.0,
        };
        drop(buffer);
        if ready == 0.0 {
            return;
        }
        let to = to.min(Duration::from_secs_f64(song.duration as f64 * ready));
        {
            if let Err(e) = audio.player.try_seek(to) {
                crate::log::error(&format!("music: seek: {e}"));
            }
        }
    }

    /// A round of the typing game starts (`on`) or ends: the song pauses during it, and plays again
    /// after if it was playing.
    pub(super) fn hold(&mut self, on: bool) {
        if on && !self.held && self.playing() && !self.paused {
            self.toggle_pause();
            self.held = true;
        } else if !on && std::mem::take(&mut self.held) && self.paused {
            self.toggle_pause();
        }
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if let Some(audio) = &self.audio {
            if self.paused { audio.player.pause() } else { audio.player.play() }
        }
    }

    /// The song playing stops; the next one starts as soon as it is there.
    fn skip(&mut self, volume: f32) {
        if let Some(song) = self.current.clone() {
            self.remember(song);
        }
        self.skip_without_history(volume);
    }

    fn remember(&mut self, song: Song) {
        self.history.push(song);
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
    }

    fn skip_without_history(&mut self, volume: f32) {
        if let Some((shared, _)) = self.playing_stream.take() {
            cancel(&shared);
        }
        if let Some(audio) = &mut self.audio {
            audio.player = rodio::Player::connect_new(audio.device.mixer());
            audio.player.set_volume(volume);
        }
        self.current = None;
        self.paused = false;
    }

    /// What arrived, what to ask for, and the next song when this one ends. Each frame.
    pub(super) fn tick(&mut self, ctx: &egui::Context, volume: f32, t: &'static Strings) {
        if let Some(audio) = &self.audio {
            audio.player.set_volume(volume);
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Genres(result) => {
                    self.genres_loading = false;
                    self.genres = Some(result.map_err(|e| describe(&e, t)));
                }
                Msg::Songs(generation, result) if generation == self.generation => {
                    self.asking = false;
                    match result {
                        Ok(songs) if songs.is_empty() && self.queue.is_empty() && self.next.is_none() && self.current.is_none() => {
                            let liked = self.station == Some(Station::Liked);
                            self.stop();
                            self.error = Some(if liked { t.music_no_liked } else { t.music_no_songs }.to_owned());
                        }
                        Ok(mut songs) => {
                            if self.station.as_ref().is_some_and(Station::finite) {
                                self.all_asked = true;
                            }
                            // The liked ones come round again: not twice in a row.
                            if self.station == Some(Station::Liked) {
                                shuffle(&mut songs);
                                let near: Vec<String> = self.queue.iter().chain(self.current.as_ref()).chain(self.next.as_ref().map(|(s, _)| s)).map(|s| s.id.clone()).collect();
                                if songs.len() > near.len() {
                                    songs.retain(|s| !near.contains(&s.id));
                                }
                            }
                            self.queue.extend(songs);
                        }
                        Err(e) => {
                            self.stop();
                            self.error = Some(describe(&e, t));
                        }
                    }
                }
                Msg::Track(generation, song, result) if generation == self.generation => {
                    self.downloading = false;
                    match result {
                        Ok(track) => self.next = Some((*song, track)),
                        Err(e) => self.failed(describe(&e, t)),
                    }
                }
                Msg::Liked(result) => {
                    self.liked_loading = false;
                    self.liked = Some(result.map_err(|e| describe(&e, t)));
                }
                Msg::LikeFailed(id, on, e) => {
                    // Shown as it was.
                    if let Some(song) = self.current.as_mut().filter(|s| s.id == id && s.liked() == on) {
                        song.starred = if on { None } else { Some(String::new()) };
                    }
                    self.error = Some(describe(&e, t));
                }
                // For a radio stopped since.
                Msg::Songs(..) | Msg::Track(..) => {}
            }
        }
        let Some(station) = self.station.clone() else { return };
        if self.queue.len() < 3 && !self.asking && !self.all_asked {
            self.asking = true;
            let generation = self.generation;
            let station = station.clone();
            self.spawn(ctx, move |server| {
                let songs = match &station {
                    Station::Genre(genre) => server.random_songs(genre.as_deref(), BATCH),
                    Station::Liked => server.liked(),
                    Station::Album { id, .. } => server.album(id).map(|(_, songs)| songs),
                    Station::List(_) => Ok(Vec::new()),
                };
                Msg::Songs(generation, songs)
            });
        }
        // Played well enough to count (half of it, or four minutes): told to the server.
        if let Some(song) = &self.current
            && !self.scrobbled
            && self.position().as_secs() >= (song.duration as u64 / 2).clamp(1, 240)
        {
            self.scrobbled = true;
            let id = song.id.clone();
            self.spawn_quiet(move |server| {
                let _ = server.scrobble(&id, true);
            });
        }
        // One song ahead, downloaded while the other plays.
        if self.next.is_none() && !self.downloading && let Some(song) = self.queue.pop_front() {
            self.downloading = true;
            let generation = self.generation;
            self.spawn(ctx, move |server| {
                let track = open(server, &song).map(Box::new);
                Msg::Track(generation, Box::new(song), track)
            });
        }
        let ended = self.audio.as_ref().is_none_or(|a| a.player.empty());
        if ended && !self.paused {
            if let Some(song) = self.current.take() {
                self.remember(song);
            }
            // A finite radio, over.
            if self.all_asked && self.queue.is_empty() && self.next.is_none() && !self.downloading && station.finite() {
                self.stop();
                return;
            }
            if let Some((shared, _)) = self.playing_stream.take() {
                cancel(&shared);
            }
            if let Some((song, track)) = self.next.take() {
                self.play(song, track, t);
            }
        }
        // To see the song end, and the time go by.
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn failed(&mut self, error: String) {
        self.failures += 1;
        crate::log::error(&format!("music: {error}"));
        if self.failures >= MAX_FAILURES {
            self.stop();
            self.error = Some(error);
        }
    }

    #[allow(clippy::boxed_local)] // Boxed as it came in the message (a decoder is big).
    fn play(&mut self, song: Song, track: Box<Track>, t: &Strings) {
        if self.audio.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut device) => {
                    device.log_on_drop(false);
                    let player = rodio::Player::connect_new(device.mixer());
                    self.audio = Some(Audio { device, player });
                }
                Err(e) => {
                    cancel(&track.shared);
                    self.stop();
                    self.error = Some(format!("{} : {e}", t.music_no_device));
                    return;
                }
            }
        }
        if let Some(audio) = &self.audio {
            audio.player.append(track.source);
            audio.player.play();
        }
        self.playing_stream = Some((track.shared, track.len));
        self.failures = 0;
        self.scrobbled = false;
        // "Now playing", for the server's other players.
        let id = song.id.clone();
        self.spawn_quiet(move |server| {
            let _ = server.scrobble(&id, false);
        });
        self.current = Some(song);
    }

    fn position(&self) -> Duration {
        self.audio.as_ref().map_or(Duration::ZERO, |a| a.player.get_pos())
    }
}

impl App {
    /// The radio's card, above `bottom` in the sidebar (nothing without a server). Where it starts.
    pub(super) fn music_card(&mut self, ui: &mut Ui, left: f32, width: f32, bottom: f32) -> f32 {
        let settings = &self.config.settings;
        if settings.subsonic.url.is_empty() || !settings.subsonic.password_saved {
            return bottom;
        }
        let t = self.t();
        let theme = self.theme.clone();
        let playing = self.music.playing();
        let h = if playing { CARD_H } else { IDLE_H };
        let card = Rect::from_min_max(Pos2::new(left, bottom - h - 6.0), Pos2::new(left + width, bottom - 6.0));
        let volume = settings.subsonic.volume;

        if !playing {
            // Two buttons: the music page, and the radio's genres.
            let radio_w = 78.0;
            let page = Rect::from_min_max(card.min, Pos2::new(card.max.x - radio_w - 4.0, card.max.y));
            let radio = Rect::from_min_max(Pos2::new(card.max.x - radio_w, card.min.y), card.max);
            let page_resp = ui.interact(page, ui.id().with("music-page"), Sense::click()).on_hover_text(t.lib_open).on_hover_cursor(egui::CursorIcon::PointingHand);
            let resp = ui.interact(radio, ui.id().with("music-start"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
            for (rect, hot) in [(page, page_resp.hovered() || self.music_page), (radio, resp.hovered() || open)] {
                ui.painter().rect_filled(rect, 8.0, if hot { theme.tab_active } else { Color32::TRANSPARENT });
                ui.painter().rect_stroke(rect, 8.0, Stroke::new(1.0, if hot { theme.accent.gamma_multiply(0.5) } else { theme.tab_hover }), egui::StrokeKind::Inside);
            }
            paint_note(ui.painter(), Pos2::new(page.min.x + 16.0, page.center().y), theme.accent);
            ui.painter().text(Pos2::new(page.min.x + 32.0, page.center().y), Align2::LEFT_CENTER, t.music_nav, FontId::proportional(13.0), theme.text);
            ui.painter().text(Pos2::new(radio.min.x + 12.0, radio.center().y), Align2::LEFT_CENTER, t.music_radio, FontId::proportional(13.0), theme.text);
            paint_chevron(ui.painter(), Pos2::new(radio.max.x - 14.0, radio.center().y), theme.text_muted);
            if let Some(e) = &self.music.error {
                let mark = Pos2::new(page.max.x - 12.0, page.center().y);
                ui.painter().circle_filled(mark, 3.5, theme.ansi[1]);
                page_resp.clone().on_hover_text(e.as_str());
            }
            if page_resp.clicked() {
                self.open_music_page(None);
            }
            if resp.clicked() {
                let server = configured_server(&self.config.settings);
                self.music.load_genres(ui.ctx(), server);
                self.music.load_liked(ui.ctx());
            }
            self.genre_menu(&resp, &theme, t);
            return card.min.y - 6.0;
        }

        let painter = ui.painter().clone();
        painter.rect_filled(card, 8.0, theme.tab_active);
        painter.rect_stroke(card, 8.0, Stroke::new(1.0, theme.tab_hover), egui::StrokeKind::Inside);
        // The wheel over the card: the volume.
        let hover = ui.interact(card, ui.id().with("music-card"), Sense::hover());
        if hover.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.config.settings.subsonic.volume = (volume + scroll / 600.0).clamp(0.0, 1.0);
                self.music_volume_changed = true;
            }
        }
        hover.on_hover_text(t.music_volume.replace("{n}", &format!("{:.0}", self.config.settings.subsonic.volume * 100.0)));
        // The cover, then the title and the artist; clicked: the album, in the music page.
        let cover = Rect::from_min_size(card.min + Vec2::new(8.0, 8.0), Vec2::splat(42.0));
        let text_x = cover.max.x + 9.0;
        let text_w = card.max.x - 32.0 - text_x;
        let line = |text: String, size: f32, color: Color32, y: f32| {
            let mut job = egui::text::LayoutJob::simple_singleline(text, FontId::proportional(size), color);
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
            painter.galley(Pos2::new(text_x, card.min.y + y), painter.layout_job(job), color);
        };
        let song = self.music.current.clone();
        match &song {
            Some(song) => {
                let texture = self.library.cover(&song.cover, 300).cloned();
                match texture {
                    Some(texture) => {
                        egui::Image::from_texture(egui::load::SizedTexture::from_handle(&texture)).corner_radius(5.0).paint_at(ui, cover);
                    }
                    None => {
                        painter.rect_filled(cover, 5.0, theme.tab_hover);
                        paint_note(&painter, cover.center(), theme.text_muted);
                    }
                }
                line(song.title.clone(), 13.0, theme.text, 11.0);
                line(song.artist.clone(), 11.5, theme.text_muted, 30.0);
                let head = Rect::from_min_max(cover.min, Pos2::new(text_x + text_w, cover.max.y));
                let open = ui.interact(head, ui.id().with("music-open-album"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if open.on_hover_text(format!("{}\n{} · {}", song.title, song.artist, song.album)).clicked() && !song.album_id.is_empty() {
                    self.open_music_page(Some(super::library::View::Album(song.album_id.clone())));
                }
                // How far into it, as a thin bar at the bottom: clicked or dragged, the song moves there.
                if song.duration > 0 {
                    let duration = song.duration as f32;
                    let bar = Rect::from_min_max(Pos2::new(card.min.x + 12.0, card.max.y - 6.0), Pos2::new(card.max.x - 12.0, card.max.y - 4.0));
                    let resp = ui.interact(bar.expand2(Vec2::new(4.0, 5.0)), ui.id().with("music-seek"), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    let at = |pos: Pos2| ((pos.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
                    if (resp.dragged() || resp.drag_started()) && let Some(pos) = resp.interact_pointer_pos() {
                        self.music.scrub = Some(at(pos));
                    }
                    let release = if resp.drag_stopped() { self.music.scrub.take() } else if resp.clicked() { resp.interact_pointer_pos().map(at) } else { None };
                    if let Some(k) = release {
                        self.music.scrub = None;
                        self.music.seek(Duration::from_secs_f32(k * duration));
                        self.media_keys.moved();
                    }
                    let k = self.music.scrub.unwrap_or_else(|| (self.music.position().as_secs_f32() / duration).min(1.0));
                    let hot = resp.hovered() || self.music.scrub.is_some();
                    let bar = if hot { bar.expand2(Vec2::new(0.0, 1.0)) } else { bar };
                    painter.rect_filled(bar, 1.5, theme.tab_hover);
                    let x = bar.min.x + bar.width() * k;
                    painter.rect_filled(Rect::from_min_max(bar.min, Pos2::new(x, bar.max.y)), 1.5, theme.accent);
                    if hot {
                        painter.circle_filled(Pos2::new(x, bar.center().y), 4.5, theme.accent);
                        let shown = resp.hover_pos().filter(|_| self.music.scrub.is_none()).map_or(k, at);
                        resp.on_hover_text(format!("{} / {}", clock(shown * duration), clock(duration)));
                    }
                }
            }
            None => {
                painter.rect_filled(cover, 5.0, theme.tab_hover);
                ui.put(Rect::from_center_size(cover.center(), Vec2::splat(14.0)), egui::Spinner::new().size(14.0));
                line(t.music_loading.to_owned(), 12.5, theme.text_muted, 20.0);
            }
        }

        // The genre (to change it), then the buttons.
        let row_y = card.max.y - 22.0;
        let genre = match &self.music.station {
            Some(Station::Genre(Some(genre))) => genre.clone(),
            Some(Station::Liked) => format!("♥ {}", t.music_liked),
            Some(Station::Album { name, .. } | Station::List(name)) => name.clone(),
            _ => t.music_all_genres.to_owned(),
        };
        let mut job = egui::text::LayoutJob::simple_singleline(genre, FontId::proportional(11.5), theme.accent);
        job.wrap = egui::text::TextWrapping::truncate_at_width(card.width() - 136.0);
        let galley = painter.layout_job(job);
        let chip = Rect::from_min_size(Pos2::new(card.min.x + 7.0, row_y - 10.0), Vec2::new(galley.size().x + 24.0, 20.0));
        let chip_resp = ui.interact(chip, ui.id().with("music-genre"), Sense::click()).on_hover_text(t.music_change_genre).on_hover_cursor(egui::CursorIcon::PointingHand);
        if chip_resp.hovered() || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&chip_resp)) {
            painter.rect_filled(chip, 5.0, theme.tab_hover);
        }
        painter.galley(Pos2::new(chip.min.x + 5.0, row_y - galley.size().y / 2.0), galley, theme.accent);
        paint_chevron(&painter, Pos2::new(chip.max.x - 9.0, row_y), theme.text_muted);
        if chip_resp.clicked() {
            let server = configured_server(&self.config.settings);
            self.music.load_genres(ui.ctx(), server);
            self.music.load_liked(ui.ctx());
        }
        self.genre_menu(&chip_resp, &theme, t);

        let button = |x: f32| Rect::from_center_size(Pos2::new(x, row_y), Vec2::splat(22.0));
        // The heart, at the right of the title: the song liked, or not.
        if let Some(liked) = self.music.current.as_ref().map(Song::liked) {
            let rect = Rect::from_center_size(Pos2::new(card.max.x - 18.0, card.min.y + 24.0), Vec2::splat(24.0));
            let resp = ui.interact(rect, ui.id().with("music-like"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                painter.rect_filled(rect, 4.0, theme.tab_hover);
            }
            let color = if liked { theme.accent } else if resp.hovered() { theme.text } else { theme.text_muted };
            paint_heart(&painter, rect.center(), color, liked);
            if resp.on_hover_text(if liked { t.music_unlike } else { t.music_like }).clicked()
                && let Some((id, on)) = self.music.toggle_like(ui.ctx())
            {
                self.library.set_liked(&id, on);
            }
        }
        if icon_button(ui, &painter, button(card.max.x - 88.0), "music-previous", &theme, paint_previous).on_hover_text(t.music_previous).clicked() {
            self.music.previous(self.config.settings.subsonic.volume);
        }
        let paused = self.music.paused;
        let play_tip = if paused { t.music_play } else { t.music_pause };
        if icon_button(ui, &painter, button(card.max.x - 64.0), "music-play", &theme, if paused { paint_play } else { paint_pause }).on_hover_text(play_tip).clicked() {
            self.music.toggle_pause();
        }
        if icon_button(ui, &painter, button(card.max.x - 40.0), "music-next", &theme, paint_next).on_hover_text(t.music_next).clicked() {
            self.music.skip(self.config.settings.subsonic.volume);
        }
        if icon_button(ui, &painter, button(card.max.x - 16.0), "music-stop", &theme, paint_stop).on_hover_text(t.music_stop).clicked() {
            self.music.stop();
        }
        card.min.y - 6.0
    }

    /// The music page shown (in this window), on `view` if given.
    pub(super) fn open_music_page(&mut self, view: Option<super::library::View>) {
        self.library.open(configured_server(&self.config.settings), view);
        self.music_page = true;
        self.notes_page = false;
        self.ctx.memory_mut(|m| m.stop_text_input());
    }

    /// The genres under `resp`, "all" first; picking one starts its radio.
    fn genre_menu(&mut self, resp: &egui::Response, theme: &Theme, t: &Strings) {
        let mut picked: Option<(Station, Option<Song>)> = None;
        let current = self.music.station.clone();
        egui::Popup::menu(resp).width(220.0).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            let item = |ui: &mut Ui, name: &str, count: Option<u32>, selected: bool| {
                let text = match count {
                    Some(n) => format!("{name}  ·  {n}"),
                    None => name.to_owned(),
                };
                ui.add(egui::Button::selectable(selected, egui::RichText::new(text).size(13.0)).min_size(Vec2::new(ui.available_width(), 26.0))).clicked()
            };
            if item(ui, t.music_all_genres, None, current == Some(Station::Genre(None))) {
                picked = Some((Station::Genre(None), None));
            }
            // The songs liked: all of them shuffled, or from one of them.
            let count = self.music.liked.as_ref().and_then(|l| l.as_ref().ok()).map(|l| l.len() as u32);
            let label = match count {
                Some(n) => format!("♥  {}  ·  {n}", t.music_liked),
                None => format!("♥  {}", t.music_liked),
            };
            ui.menu_button(egui::RichText::new(label).size(13.0), |ui| {
                ui.set_min_width(280.0);
                match &self.music.liked {
                    None => super::loading::inline(ui, theme, t.music_loading),
                    Some(Err(e)) => {
                        ui.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                    }
                    Some(Ok(liked)) if liked.is_empty() => {
                        ui.add(egui::Label::new(egui::RichText::new(t.music_no_liked).size(12.0).color(theme.text_muted)).wrap());
                    }
                    Some(Ok(liked)) => {
                        if item(ui, &format!("🔀  {}", t.music_shuffle_liked), None, current == Some(Station::Liked)) {
                            picked = Some((Station::Liked, None));
                        }
                        ui.separator();
                        egui::ScrollArea::vertical().id_salt("music-liked").max_height(360.0).show(ui, |ui| {
                            let playing = self.music.current.as_ref().map(|s| s.id.as_str());
                            for song in liked {
                                let label = format!("{}  —  {}", song.title, song.artist);
                                let button = egui::Button::selectable(playing == Some(song.id.as_str()), egui::RichText::new(label).size(12.5)).truncate().min_size(Vec2::new(ui.available_width(), 24.0));
                                if ui.add(button).on_hover_text(format!("{}\n{} · {}", song.title, song.artist, song.album)).clicked() {
                                    picked = Some((Station::Liked, Some(song.clone())));
                                }
                            }
                        });
                    }
                }
            });
            ui.separator();
            match &self.music.genres {
                None => super::loading::inline(ui, theme, t.music_loading),
                Some(Err(e)) => {
                    ui.add(egui::Label::new(egui::RichText::new(e).size(12.0).color(theme.ansi[1])).wrap());
                }
                Some(Ok(genres)) => {
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for genre in genres {
                            if item(ui, &genre.name, Some(genre.songs), current == Some(Station::Genre(Some(genre.name.clone())))) {
                                picked = Some((Station::Genre(Some(genre.name.clone())), None));
                            }
                        }
                    });
                }
            }
        });
        if let Some((station, first)) = picked {
            let server = configured_server(&self.config.settings);
            self.music.start(server, station, first.into_iter().collect());
            resp.ctx.request_repaint();
        }
    }
}

/// In a random order (no need for a good one).
pub(super) fn shuffle<T>(items: &mut [T]) {
    let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
    for i in (1..items.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        items.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}

/// "3:07".
fn clock(secs: f32) -> String {
    let secs = secs.max(0.0) as u32;
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub(super) fn paint_heart(painter: &egui::Painter, c: Pos2, color: Color32, filled: bool) {
    // Two lobes and a point, as a polygon.
    let points: Vec<Pos2> = (0..32)
        .map(|k| {
            let a = k as f32 / 32.0 * std::f32::consts::TAU;
            let (s, co) = a.sin_cos();
            let x = 16.0 * s.powi(3);
            let y = 13.0 * co - 5.0 * (2.0 * a).cos() - 2.0 * (3.0 * a).cos() - (4.0 * a).cos();
            c + Vec2::new(x, -y - 1.5) * 0.36
        })
        .collect();
    if filled {
        // Not convex: drawn as a fan from its middle.
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(c, color);
        for p in &points {
            mesh.colored_vertex(*p, color);
        }
        for k in 0..points.len() as u32 {
            mesh.add_triangle(0, 1 + k, 1 + (k + 1) % points.len() as u32);
        }
        painter.add(egui::Shape::mesh(mesh));
    } else {
        painter.add(egui::Shape::closed_line(points, Stroke::new(1.4, color)));
    }
}

fn paint_note(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.circle_filled(c + Vec2::new(-3.0, 4.0), 2.8, color);
    painter.line_segment([c + Vec2::new(-0.5, 4.0), c + Vec2::new(-0.5, -5.5)], Stroke::new(1.4, color));
    painter.line_segment([c + Vec2::new(-0.5, -5.5), c + Vec2::new(4.0, -3.0)], Stroke::new(1.4, color));
}

fn paint_chevron(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.add(egui::Shape::line(vec![c + Vec2::new(-3.5, -1.5), c + Vec2::new(0.0, 2.0), c + Vec2::new(3.5, -1.5)], Stroke::new(1.4, color)));
}

fn paint_play(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-3.5, -5.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(-3.5, 5.0)], color, Stroke::NONE));
}

fn paint_pause(painter: &egui::Painter, c: Pos2, color: Color32) {
    for dx in [-2.5, 2.5] {
        painter.rect_filled(Rect::from_center_size(c + Vec2::new(dx, 0.0), Vec2::new(2.5, 10.0)), 0.5, color);
    }
}

fn paint_previous(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(4.5, -5.0), c + Vec2::new(4.5, 5.0), c + Vec2::new(-2.5, 0.0)], color, Stroke::NONE));
    painter.rect_filled(Rect::from_center_size(c + Vec2::new(-4.0, 0.0), Vec2::new(2.0, 10.0)), 0.5, color);
}

fn paint_next(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-4.5, -5.0), c + Vec2::new(2.5, 0.0), c + Vec2::new(-4.5, 5.0)], color, Stroke::NONE));
    painter.rect_filled(Rect::from_center_size(c + Vec2::new(4.0, 0.0), Vec2::new(2.0, 10.0)), 0.5, color);
}

fn paint_stop(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.rect_filled(Rect::from_center_size(c, Vec2::splat(8.5)), 1.5, color);
}
