//! The music page: the Subsonic server's library, as on its own web player. Home (rows of albums),
//! albums, artists, liked songs, playlists and search, with the covers; an album, an artist or a
//! playlist opened in the page, played from any song.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc;

use super::music::Station;
use super::*;
use crate::subsonic::{Album, AlbumOrder, Artist, Failure, Found, Playlist, Server, Song};

/// Covers downloaded at once, at most.
const COVER_WORKERS: usize = 6;
/// The size covers are asked for: the grids', and the headers'.
const COVER: u32 = 300;
const COVER_BIG: u32 = 600;
const CARD_W: f32 = 168.0;
const NAV_W: f32 = 190.0;
/// Albums asked for at a time on the "Albums" page.
const PAGE: u32 = 60;
const HOME_ROWS: [AlbumOrder; 4] = [AlbumOrder::Newest, AlbumOrder::Recent, AlbumOrder::Frequent, AlbumOrder::Random];

/// A place in the library.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) enum View {
    Home,
    Albums(AlbumOrder),
    Artists,
    Liked,
    Playlists,
    Album(String),
    Artist(String),
    Playlist(String),
    Search(String),
}

/// What a place shows, once loaded.
enum Page {
    Home(Vec<(AlbumOrder, Vec<Album>)>),
    /// And whether there may be more.
    Albums(Vec<Album>, bool),
    Artists(Vec<Artist>),
    Liked(Vec<Song>, Vec<Album>),
    Playlists(Vec<Playlist>),
    Album(Album, Vec<Song>),
    Artist(Artist, Vec<Album>),
    Playlist(Playlist, Vec<Song>),
    /// And whether there may be more songs.
    Search(Found, bool),
}

enum Msg {
    Page(View, Result<Page, Failure>),
    /// More albums for the "Albums" page.
    More(AlbumOrder, Result<Vec<Album>, Failure>),
    MoreSongs(String, Result<Vec<Song>, Failure>),
    Cover(String, u32, Option<egui::ColorImage>),
}

enum Cover {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

/// What a click in the page asks the app to do.
enum Action {
    Go(View),
    Back,
    /// Plays these songs, from that one, as a radio of that name.
    Play(Vec<Song>, usize, String),
    Shuffle(Vec<Song>, String),
    PlayAlbum(Album),
    Like(Song, bool),
    LikeAlbum(Album, bool),
    More(AlbumOrder),
    MoreSongs(String),
    Reload,
}

pub(super) struct Library {
    server: Option<Server>,
    view: View,
    back: Vec<View>,
    pages: HashMap<View, Result<Page, String>>,
    loading: HashSet<View>,
    more_loading: bool,
    covers: HashMap<(String, u32), Cover>,
    /// Covers waiting for a worker, and how many work.
    waiting: VecDeque<(String, u32)>,
    workers: usize,
    search: String,
    focus_search: bool,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
}

impl Default for Library {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { server: None, view: View::Home, back: Vec::new(), pages: HashMap::new(), loading: HashSet::new(), more_loading: false, covers: HashMap::new(), waiting: VecDeque::new(), workers: 0, search: String::new(), focus_search: false, tx, rx }
    }
}

impl Library {
    /// The page opened, on `view` if given; the server taken again from the settings (it may have changed).
    pub(super) fn open(&mut self, server: Option<Server>, view: Option<View>) {
        let changed = match (&self.server, &server) {
            (Some(a), Some(b)) => a.address() != b.address(),
            _ => true,
        };
        if changed {
            self.pages.clear();
            self.covers.clear();
            self.back.clear();
            self.view = View::Home;
        }
        self.server = server;
        if let Some(view) = view {
            self.go(view);
        }
    }

    pub(super) fn focus_search(&mut self) {
        self.focus_search = true;
    }

    pub(super) fn needs_server(&self) -> bool {
        self.server.is_none()
    }

    fn go(&mut self, view: View) {
        if view != self.view {
            self.back.push(std::mem::replace(&mut self.view, view));
        }
    }

    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&Server) -> Msg + Send + 'static) {
        let Some(server) = self.server.clone() else { return };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(&server));
            ctx.request_repaint();
        });
    }

    fn load(&mut self, ctx: &egui::Context, view: View) {
        if self.loading.contains(&view) {
            return;
        }
        self.loading.insert(view.clone());
        self.spawn(ctx, move |server| {
            let page = match &view {
                // The rows asked for together.
                View::Home => std::thread::scope(|scope| {
                    let rows: Vec<_> = HOME_ROWS.iter().map(|order| scope.spawn(move || server.albums(*order, 18, 0).map(|albums| (*order, albums)))).collect();
                    rows.into_iter().map(|row| row.join().unwrap_or(Err(Failure::NotSubsonic))).collect::<Result<Vec<_>, _>>().map(Page::Home)
                }),
                View::Albums(order) => server.albums(*order, PAGE, 0).map(|albums| {
                    let more = albums.len() as u32 == PAGE;
                    Page::Albums(albums, more)
                }),
                View::Artists => server.artists().map(Page::Artists),
                View::Liked => server.liked().and_then(|songs| Ok(Page::Liked(songs, server.albums(AlbumOrder::Starred, 100, 0)?))),
                View::Playlists => server.playlists().map(Page::Playlists),
                View::Album(id) => server.album(id).map(|(album, songs)| Page::Album(album, songs)),
                View::Artist(id) => server.artist(id).map(|(artist, albums)| Page::Artist(artist, albums)),
                View::Playlist(id) => server.playlist(id).map(|(playlist, songs)| Page::Playlist(playlist, songs)),
                View::Search(query) => server.search(query).map(|found| {
                    let more = found.songs.len() == crate::subsonic::SEARCH_SONGS;
                    Page::Search(found, more)
                }),
            };
            Msg::Page(view, page)
        });
    }

    /// What arrived, and the covers waiting given to the workers. Each frame.
    pub(super) fn poll(&mut self, ctx: &egui::Context, t: &Strings) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Page(view, page) => {
                    self.loading.remove(&view);
                    self.pages.insert(view, page.map_err(|e| super::music::describe(&e, t)));
                }
                Msg::More(order, albums) => {
                    self.more_loading = false;
                    if let Some(Ok(Page::Albums(albums_shown, may_have_more))) = self.pages.get_mut(&View::Albums(order)) {
                        match albums {
                            Ok(more) => {
                                *may_have_more = more.len() as u32 == PAGE;
                                albums_shown.extend(more);
                            }
                            // Not asked again and again: the end, until reloaded.
                            Err(e) => {
                                *may_have_more = false;
                                crate::log::error(&format!("music: more albums: {}", super::music::describe(&e, t)));
                            }
                        }
                    }
                }
                Msg::MoreSongs(query, songs) => {
                    self.more_loading = false;
                    if let Some(Ok(Page::Search(found, may_have_more))) = self.pages.get_mut(&View::Search(query)) {
                        match songs {
                            Ok(more) => {
                                *may_have_more = more.len() == crate::subsonic::SEARCH_SONGS;
                                // Not twice the same (the results may have moved meanwhile).
                                let known: HashSet<String> = found.songs.iter().map(|s| s.id.clone()).collect();
                                found.songs.extend(more.into_iter().filter(|s| !known.contains(&s.id)));
                            }
                            Err(e) => {
                                *may_have_more = false;
                                crate::log::error(&format!("music: more songs: {}", super::music::describe(&e, t)));
                            }
                        }
                    }
                }
                Msg::Cover(id, size, image) => {
                    self.workers -= 1;
                    let cover = match image {
                        Some(image) => Cover::Ready(ctx.load_texture(format!("cover-{id}-{size}"), image, egui::TextureOptions::LINEAR)),
                        None => Cover::Failed,
                    };
                    self.covers.insert((id, size), cover);
                }
            }
        }
        // The covers waiting, as workers free up.
        while self.workers < COVER_WORKERS {
            let Some((id, size)) = self.waiting.pop_front() else { break };
            self.workers += 1;
            self.spawn(ctx, move |server| {
                let image = server.cover(&id, size).ok().and_then(|bytes| image::load_from_memory(&bytes).ok()).map(|image| {
                    let image = image.to_rgba8();
                    egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw())
                });
                Msg::Cover(id, size, image)
            });
        }
    }

    /// A cover, asked for the first time it is wanted; None until it is there.
    pub(super) fn cover(&mut self, id: &str, size: u32) -> Option<&egui::TextureHandle> {
        if id.is_empty() {
            return None;
        }
        let key = (id.to_owned(), size);
        if !self.covers.contains_key(&key) {
            self.covers.insert(key.clone(), Cover::Loading);
            self.waiting.push_back(key.clone());
        }
        match self.covers.get(&key) {
            Some(Cover::Ready(texture)) => Some(texture),
            _ => None,
        }
    }

    /// A song liked or no longer, everywhere it shows.
    pub(super) fn set_liked(&mut self, id: &str, on: bool) {
        let starred = on.then(|| chrono::Local::now().to_rfc3339());
        for page in self.pages.values_mut().flatten() {
            let songs = match page {
                Page::Album(_, songs) | Page::Playlist(_, songs) => songs,
                Page::Search(found, _) => &mut found.songs,
                Page::Liked(songs, _) => {
                    // Gone from the liked ones, or back (at its place once reloaded).
                    if !on {
                        songs.retain(|s| s.id != id);
                    }
                    songs
                }
                _ => continue,
            };
            for song in songs.iter_mut().filter(|s| s.id == id) {
                song.starred = starred.clone();
            }
        }
        if on {
            self.pages.remove(&View::Liked);
        }
    }

    fn set_album_liked(&mut self, id: &str, on: bool) {
        let starred = on.then(|| chrono::Local::now().to_rfc3339());
        for page in self.pages.values_mut().flatten() {
            let albums: Vec<&mut Album> = match page {
                Page::Home(rows) => rows.iter_mut().flat_map(|(_, albums)| albums.iter_mut()).collect(),
                Page::Albums(albums, _) | Page::Artist(_, albums) | Page::Liked(_, albums) => albums.iter_mut().collect(),
                Page::Search(found, _) => found.albums.iter_mut().collect(),
                Page::Album(album, _) => vec![album],
                _ => continue,
            };
            for album in albums.into_iter().filter(|a| a.id == id) {
                album.starred = starred.clone();
            }
        }
        self.pages.remove(&View::Liked);
        self.pages.remove(&View::Albums(AlbumOrder::Starred));
    }
}

/// "47 min", "1 h 12".
fn total(secs: u32) -> String {
    let minutes = (secs + 30) / 60;
    if minutes < 60 { format!("{minutes} min") } else { format!("{} h {:02}", minutes / 60, minutes % 60) }
}

fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn order_label(order: AlbumOrder, t: &Strings) -> &'static str {
    match order {
        AlbumOrder::Newest => t.lib_newest,
        AlbumOrder::Recent => t.lib_recent,
        AlbumOrder::Frequent => t.lib_frequent,
        AlbumOrder::Random => t.lib_random,
        AlbumOrder::ByName => t.lib_by_name,
        AlbumOrder::ByArtist => t.lib_by_artist,
        AlbumOrder::Starred => t.music_liked,
    }
}

/// Text cut with "…" to fit `width`.
fn galley(ui: &Ui, text: &str, size: f32, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(size), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(10.0));
    ui.painter().layout_job(job)
}

/// A cover in `rect`, or its place (a note) until it is there.
fn paint_cover(ui: &Ui, library: &mut Library, id: &str, size: u32, rect: Rect, radius: f32, theme: &Theme) {
    let texture = if ui.is_rect_visible(rect) { library.cover(id, size).cloned() } else { None };
    match texture {
        Some(texture) => {
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(&texture)).corner_radius(radius).paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, radius, theme.tab_hover);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "♫", FontId::proportional(rect.height() * 0.3), theme.text_muted.gamma_multiply(0.6));
        }
    }
}

impl App {
    /// The music page, filling `rect`.
    pub(super) fn library_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.t();
        let theme = self.theme.clone();
        let ctx = ui.ctx().clone();
        if self.library.server.is_none() {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, t.lib_no_server, FontId::proportional(15.0), theme.text_muted);
            return;
        }
        let view = self.library.view.clone();
        if !self.library.pages.contains_key(&view) {
            self.library.load(&ctx, view.clone());
        }
        let mut action: Option<Action> = None;

        // The navigation, on the left.
        let nav = Rect::from_min_size(rect.min, Vec2::new(NAV_W, rect.height()));
        ui.painter().rect_filled(nav, 0.0, theme.chrome_bg);
        ui.painter().vline(nav.max.x, nav.y_range(), Stroke::new(1.0, theme.tab_hover));
        ui.scope_builder(egui::UiBuilder::new().max_rect(nav.shrink2(Vec2::new(10.0, 16.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            ui.label(egui::RichText::new(t.music_nav).size(18.0).strong().color(theme.text));
            ui.add_space(14.0);
            let section = match &view {
                View::Albums(_) | View::Album(_) => 1,
                View::Artists | View::Artist(_) => 2,
                View::Liked => 3,
                View::Playlists | View::Playlist(_) => 4,
                View::Search(_) => 5,
                View::Home => 0,
            };
            let entries = [(View::Home, "⌂", t.lib_home), (View::Albums(AlbumOrder::Newest), "◫", t.lib_albums), (View::Artists, "☺", t.lib_artists), (View::Liked, "♥", t.music_liked), (View::Playlists, "☰", t.lib_playlists)];
            for (k, (target, icon, label)) in entries.into_iter().enumerate() {
                let selected = section == k;
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
                let fill = if selected { theme.tab_active } else if resp.hovered() { theme.tab_hover } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(row, 7.0, fill);
                ui.painter().text(Pos2::new(row.min.x + 18.0, row.center().y), Align2::CENTER_CENTER, icon, FontId::proportional(14.0), if selected { theme.accent } else { theme.text_muted });
                ui.painter().text(Pos2::new(row.min.x + 36.0, row.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.5), if selected { theme.text } else { theme.text_muted });
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    action = Some(Action::Go(target));
                }
            }
        });

        // The bar on top: back, search, reload.
        let main = Rect::from_min_max(Pos2::new(nav.max.x, rect.min.y), rect.max);
        let bar = Rect::from_min_size(main.min, Vec2::new(main.width(), 52.0));
        ui.painter().hline(bar.x_range(), bar.max.y, Stroke::new(1.0, theme.tab_hover));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(16.0, 10.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let back = egui::Button::new(egui::RichText::new("‹").size(18.0)).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::new(30.0, 30.0));
            if ui.add_enabled(!self.library.back.is_empty(), back).on_hover_text(t.lib_back).clicked() {
                action = Some(Action::Back);
            }
            ui.add_space(6.0);
            let edit = ui.add(egui::TextEdit::singleline(&mut self.library.search).hint_text(t.lib_search_hint.replace("{key}", &self.config.settings.shortcuts.find_text.label())).desired_width((ui.available_width() - 50.0).min(420.0)).margin(Vec2::new(10.0, 6.0)));
            if std::mem::take(&mut self.library.focus_search) {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let query = self.library.search.trim().to_owned();
                if !query.is_empty() {
                    // Searched again: asked again.
                    self.library.pages.remove(&View::Search(query.clone()));
                    action = Some(Action::Go(View::Search(query)));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let reload = egui::Button::new(egui::RichText::new("⟳").size(15.0)).frame_when_inactive(false).corner_radius(6.0).min_size(Vec2::new(30.0, 30.0));
                if ui.add(reload).on_hover_text(t.lib_reload).clicked() {
                    action = Some(Action::Reload);
                }
            });
        });

        // The page.
        let body = Rect::from_min_max(Pos2::new(main.min.x, bar.max.y + 1.0), main.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            ui.set_clip_rect(body);
            egui::ScrollArea::vertical().id_salt(("library", format!("{view:?}"))).auto_shrink(false).show(ui, |ui| {
                egui::Frame::NONE.inner_margin(egui::Margin { left: 28, right: 28, top: 22, bottom: 28 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.library_page(ui, &view, &theme, t, &mut action);
                });
            });
        });

        if let Some(action) = action {
            self.library_action(&ctx, action);
        }
    }

    fn library_page(&mut self, ui: &mut Ui, view: &View, theme: &Theme, t: &Strings, action: &mut Option<Action>) {
        let playing = self.music.current().map(|s| s.id.clone());
        // Taken out while drawn (the page draws with the library's covers).
        let Some(page) = self.library.pages.remove(view) else {
            ui.add_space(40.0);
            super::loading::inline(ui, theme, t.music_loading);
            return;
        };
        let lib = &mut self.library;
        match &page {
            Err(e) => {
                ui.add(egui::Label::new(egui::RichText::new(e).size(13.0).color(theme.ansi[1])).wrap());
            }
            Ok(Page::Home(rows)) => {
                for (order, albums) in rows {
                    if albums.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(order_label(*order, t)).size(17.0).strong().color(theme.text));
                        if ui.add(egui::Button::new(egui::RichText::new(t.lib_see_all).size(12.0).color(theme.accent)).frame_when_inactive(false)).clicked() {
                            *action = Some(Action::Go(View::Albums(*order)));
                        }
                    });
                    ui.add_space(8.0);
                    egui::ScrollArea::horizontal().id_salt(("home-row", *order as u8)).auto_shrink([false, true]).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 16.0;
                            for album in albums {
                                album_card(ui, lib, album, theme, t, action);
                            }
                        });
                    });
                    ui.add_space(24.0);
                }
            }
            Ok(Page::Albums(albums, more)) => {
                let View::Albums(current) = view else { unreachable!() };
                ui.horizontal_wrapped(|ui| {
                    for order in [AlbumOrder::Newest, AlbumOrder::Recent, AlbumOrder::Frequent, AlbumOrder::Random, AlbumOrder::ByName, AlbumOrder::ByArtist, AlbumOrder::Starred] {
                        if ui.add(egui::Button::selectable(*current == order, egui::RichText::new(order_label(order, t)).size(12.5)).corner_radius(14.0).min_size(Vec2::new(0.0, 28.0))).clicked() {
                            *action = Some(Action::Go(View::Albums(order)));
                        }
                    }
                });
                ui.add_space(18.0);
                album_grid(ui, lib, albums, theme, t, action);
                if *more && near_end(ui, lib, theme, t) {
                    *action = Some(Action::More(*current));
                }
            }
            Ok(Page::Artists(artists)) => {
                if artists.is_empty() {
                    ui.label(egui::RichText::new(t.lib_empty).color(theme.text_muted));
                }
                let per_row = ((ui.available_width() + 16.0) / (140.0 + 16.0)).floor().max(1.0) as usize;
                for row in artists.chunks(per_row) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        for artist in row {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(140.0, 186.0), Sense::click());
                            let picture = Rect::from_min_size(rect.min, Vec2::splat(140.0));
                            paint_cover(ui, lib, &artist.cover, COVER, picture, 70.0, theme);
                            if resp.hovered() {
                                ui.painter().circle_stroke(picture.center(), 70.0, Stroke::new(2.0, theme.accent));
                            }
                            let name = galley(ui, &artist.name, 13.5, theme.text, 140.0);
                            ui.painter().galley(Pos2::new(rect.center().x - name.size().x / 2.0, picture.max.y + 8.0), name, theme.text);
                            let count = galley(ui, &t.lib_albums_count.replace("{n}", &artist.albums.to_string()), 11.5, theme.text_muted, 140.0);
                            ui.painter().galley(Pos2::new(rect.center().x - count.size().x / 2.0, picture.max.y + 26.0), count, theme.text_muted);
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                *action = Some(Action::Go(View::Artist(artist.id.clone())));
                            }
                        }
                    });
                    ui.add_space(14.0);
                }
            }
            Ok(Page::Liked(songs, albums)) => {
                header(ui, lib, None, t.music_liked, &t.lib_songs_count.replace("{n}", &songs.len().to_string()), None, theme);
                list_buttons(ui, songs, t.music_liked, None, theme, t, action);
                ui.add_space(14.0);
                if songs.is_empty() {
                    ui.label(egui::RichText::new(t.music_no_liked).color(theme.text_muted));
                }
                song_table(ui, lib, songs, t.music_liked, true, playing.as_deref(), theme, t, action);
                if !albums.is_empty() {
                    ui.add_space(26.0);
                    ui.label(egui::RichText::new(t.lib_albums).size(17.0).strong().color(theme.text));
                    ui.add_space(10.0);
                    album_grid(ui, lib, albums, theme, t, action);
                }
            }
            Ok(Page::Playlists(playlists)) => {
                if playlists.is_empty() {
                    ui.label(egui::RichText::new(t.lib_empty).color(theme.text_muted));
                }
                let per_row = ((ui.available_width() + 16.0) / (CARD_W + 16.0)).floor().max(1.0) as usize;
                for row in playlists.chunks(per_row) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        for playlist in row {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(CARD_W, CARD_W + 46.0), Sense::click());
                            let picture = Rect::from_min_size(rect.min, Vec2::splat(CARD_W));
                            paint_cover(ui, lib, &playlist.cover, COVER, picture, 8.0, theme);
                            if resp.hovered() {
                                ui.painter().rect_stroke(picture, 8.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
                            }
                            ui.painter().galley(Pos2::new(rect.min.x, picture.max.y + 8.0), galley(ui, &playlist.name, 13.5, theme.text, CARD_W), theme.text);
                            let what = format!("{} · {}", t.lib_songs_count.replace("{n}", &playlist.songs.to_string()), total(playlist.duration));
                            ui.painter().galley(Pos2::new(rect.min.x, picture.max.y + 26.0), galley(ui, &what, 11.5, theme.text_muted, CARD_W), theme.text_muted);
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                *action = Some(Action::Go(View::Playlist(playlist.id.clone())));
                            }
                        }
                    });
                    ui.add_space(14.0);
                }
            }
            Ok(Page::Album(album, songs)) => {
                let mut what = vec![];
                if album.year > 0 {
                    what.push(album.year.to_string());
                }
                if !album.genre.is_empty() {
                    what.push(album.genre.clone());
                }
                what.push(t.lib_songs_count.replace("{n}", &songs.len().to_string()));
                what.push(total(album.duration));
                let artist = (!album.artist_id.is_empty()).then_some((album.artist.as_str(), album.artist_id.as_str()));
                if let Some(id) = header(ui, lib, Some(&album.cover), &album.name, &what.join("  ·  "), artist, theme) {
                    *action = Some(Action::Go(View::Artist(id)));
                }
                list_buttons(ui, songs, &album.name, Some(album), theme, t, action);
                ui.add_space(14.0);
                song_table(ui, lib, songs, &album.name, false, playing.as_deref(), theme, t, action);
            }
            Ok(Page::Artist(artist, albums)) => {
                header(ui, lib, Some(&artist.cover), &artist.name, &t.lib_albums_count.replace("{n}", &albums.len().to_string()), None, theme);
                album_grid(ui, lib, albums, theme, t, action);
            }
            Ok(Page::Playlist(playlist, songs)) => {
                let what = format!("{}  ·  {}", t.lib_songs_count.replace("{n}", &songs.len().to_string()), total(playlist.duration));
                header(ui, lib, Some(&playlist.cover), &playlist.name, &what, None, theme);
                if !playlist.comment.is_empty() {
                    ui.label(egui::RichText::new(&playlist.comment).size(12.5).color(theme.text_muted));
                    ui.add_space(8.0);
                }
                list_buttons(ui, songs, &playlist.name, None, theme, t, action);
                ui.add_space(14.0);
                song_table(ui, lib, songs, &playlist.name, true, playing.as_deref(), theme, t, action);
            }
            Ok(Page::Search(found, more)) => {
                if found.artists.is_empty() && found.albums.is_empty() && found.songs.is_empty() {
                    ui.label(egui::RichText::new(t.lib_no_results).size(14.0).color(theme.text_muted));
                }
                if !found.artists.is_empty() {
                    ui.label(egui::RichText::new(t.lib_artists).size(17.0).strong().color(theme.text));
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        for artist in &found.artists {
                            if ui.add(egui::Button::new(egui::RichText::new(&artist.name).size(13.0)).corner_radius(14.0).min_size(Vec2::new(0.0, 28.0))).clicked() {
                                *action = Some(Action::Go(View::Artist(artist.id.clone())));
                            }
                        }
                    });
                    ui.add_space(22.0);
                }
                if !found.albums.is_empty() {
                    ui.label(egui::RichText::new(t.lib_albums).size(17.0).strong().color(theme.text));
                    ui.add_space(10.0);
                    album_grid(ui, lib, &found.albums, theme, t, action);
                    ui.add_space(14.0);
                }
                if !found.songs.is_empty() {
                    ui.label(egui::RichText::new(t.lib_songs).size(17.0).strong().color(theme.text));
                    ui.add_space(8.0);
                    let View::Search(query) = view else { unreachable!() };
                    song_table(ui, lib, &found.songs, query, true, playing.as_deref(), theme, t, action);
                    if *more && near_end(ui, lib, theme, t) {
                        *action = Some(Action::MoreSongs(query.clone()));
                    }
                }
            }
        }
        self.library.pages.insert(view.clone(), page);
    }

    fn library_action(&mut self, ctx: &egui::Context, action: Action) {
        let server = || super::music::configured_server(&self.config.settings);
        match action {
            Action::Go(view) => self.library.go(view),
            Action::Back => {
                if let Some(view) = self.library.back.pop() {
                    self.library.view = view;
                }
            }
            Action::Play(songs, from, name) => {
                let songs = songs.into_iter().skip(from).collect();
                self.music.start(server(), Station::List(name), songs);
            }
            Action::Shuffle(mut songs, name) => {
                super::music::shuffle(&mut songs);
                self.music.start(server(), Station::List(name), songs);
            }
            Action::PlayAlbum(album) => self.music.start(server(), Station::Album { id: album.id, name: album.name }, Vec::new()),
            Action::Like(song, on) => {
                self.library.set_liked(&song.id, on);
                self.music.like(ctx, song, on);
            }
            Action::LikeAlbum(album, on) => {
                self.library.set_album_liked(&album.id, on);
                if let Some(server) = self.library.server.clone() {
                    std::thread::spawn(move || {
                        if let Err(e) = server.like_album(&album.id, on) {
                            crate::log::error(&format!("music: like album: {e:?}"));
                        }
                    });
                }
            }
            Action::More(order) => {
                let offset = match self.library.pages.get(&View::Albums(order)) {
                    Some(Ok(Page::Albums(albums, _))) => albums.len() as u32,
                    _ => return,
                };
                self.library.more_loading = true;
                self.library.spawn(ctx, move |server| Msg::More(order, server.albums(order, PAGE, offset)));
            }
            Action::MoreSongs(query) => {
                let offset = match self.library.pages.get(&View::Search(query.clone())) {
                    Some(Ok(Page::Search(found, _))) => found.songs.len(),
                    _ => return,
                };
                self.library.more_loading = true;
                self.library.spawn(ctx, move |server| {
                    let songs = server.search_songs(&query, offset);
                    Msg::MoreSongs(query, songs)
                });
            }
            Action::Reload => {
                let view = self.library.view.clone();
                self.library.pages.remove(&view);
            }
        }
    }
}

/// The end of a list that goes on: a spinner while more comes. True when it is close to being seen
/// (more should be asked for).
fn near_end(ui: &mut Ui, lib: &Library, theme: &Theme, t: &Strings) -> bool {
    ui.add_space(12.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
    if lib.more_loading {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| super::loading::inline(ui, theme, t.music_loading));
        return false;
    }
    // Asked for a screen ahead, so that it is there by the time the end is reached.
    let ahead = rect.translate(Vec2::new(0.0, -ui.clip_rect().height()));
    ui.is_rect_visible(rect) || ui.is_rect_visible(ahead)
}

/// An album as a card: its cover, its name and artist; a play button on the cover when hovered.
fn album_card(ui: &mut Ui, lib: &mut Library, album: &Album, theme: &Theme, t: &Strings, action: &mut Option<Action>) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(CARD_W, CARD_W + 46.0), Sense::click());
    let picture = Rect::from_min_size(rect.min, Vec2::splat(CARD_W));
    paint_cover(ui, lib, &album.cover, COVER, picture, 8.0, theme);
    // The pointer in the card (also over its play button, which takes the hover).
    let inside = resp.contains_pointer();
    let mut play_clicked = false;
    if inside {
        ui.painter().rect_stroke(picture, 8.0, Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
        let button = Rect::from_center_size(picture.max - Vec2::splat(26.0), Vec2::splat(38.0));
        let play = ui.interact(button, ui.id().with(("album-play", &album.id)), Sense::click());
        ui.painter().circle_filled(button.center(), 19.0, if play.hovered() { theme.text } else { theme.accent });
        paint_play_big(ui.painter(), button.center(), theme.bg);
        play_clicked = play.on_hover_text(t.lib_play).clicked();
    }
    ui.painter().galley(Pos2::new(rect.min.x, picture.max.y + 8.0), galley(ui, &album.name, 13.5, theme.text, CARD_W), theme.text);
    let by = if album.year > 0 { format!("{} · {}", album.artist, album.year) } else { album.artist.clone() };
    ui.painter().galley(Pos2::new(rect.min.x, picture.max.y + 26.0), galley(ui, &by, 11.5, theme.text_muted, CARD_W), theme.text_muted);
    if play_clicked {
        *action = Some(Action::PlayAlbum(album.clone()));
    } else if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *action = Some(Action::Go(View::Album(album.id.clone())));
    }
}

fn album_grid(ui: &mut Ui, lib: &mut Library, albums: &[Album], theme: &Theme, t: &Strings, action: &mut Option<Action>) {
    if albums.is_empty() {
        ui.label(egui::RichText::new(t.lib_empty).color(theme.text_muted));
        return;
    }
    let per_row = ((ui.available_width() + 16.0) / (CARD_W + 16.0)).floor().max(1.0) as usize;
    for row in albums.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for album in row {
                album_card(ui, lib, album, theme, t, action);
            }
        });
        ui.add_space(14.0);
    }
}

/// An album's, an artist's or a playlist's head: the big cover, the name, what it holds, and the
/// artist as a link. The artist's id when it was clicked.
fn header(ui: &mut Ui, lib: &mut Library, cover: Option<&str>, title: &str, what: &str, artist: Option<(&str, &str)>, theme: &Theme) -> Option<String> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        if let Some(cover) = cover {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(200.0), Sense::hover());
            paint_cover(ui, lib, cover, COVER_BIG, rect, 10.0, theme);
            ui.add_space(22.0);
        }
        ui.vertical(|ui| {
            ui.add_space(if cover.is_some() { 70.0 } else { 0.0 });
            ui.add(egui::Label::new(egui::RichText::new(title).size(28.0).strong().color(theme.text)).truncate());
            ui.add_space(6.0);
            if let Some((name, id)) = artist {
                let link = ui.add(egui::Label::new(egui::RichText::new(name).size(15.0).color(theme.accent)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                if link.clicked() {
                    clicked = Some(id.to_owned());
                }
                ui.add_space(4.0);
            }
            ui.label(egui::RichText::new(what).size(12.5).color(theme.text_muted));
        });
    });
    ui.add_space(18.0);
    clicked
}

/// "Play", "Shuffle", and the album's heart.
fn list_buttons(ui: &mut Ui, songs: &[Song], name: &str, album: Option<&Album>, theme: &Theme, t: &Strings, action: &mut Option<Action>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let play = egui::Button::new(egui::RichText::new(format!("▶  {}", t.lib_play)).size(13.5).color(theme.bg)).fill(theme.accent).corner_radius(18.0).min_size(Vec2::new(110.0, 36.0));
        if ui.add_enabled(!songs.is_empty(), play).clicked() {
            *action = Some(Action::Play(songs.to_vec(), 0, name.to_owned()));
        }
        let shuffle = egui::Button::new(egui::RichText::new(format!("🔀  {}", t.lib_shuffle)).size(13.5)).corner_radius(18.0).min_size(Vec2::new(110.0, 36.0));
        if ui.add_enabled(songs.len() > 1, shuffle).clicked() {
            *action = Some(Action::Shuffle(songs.to_vec(), name.to_owned()));
        }
        if let Some(album) = album {
            let liked = album.starred.is_some();
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
            if resp.hovered() {
                ui.painter().circle_filled(rect.center(), 18.0, theme.tab_hover);
            }
            super::music::paint_heart(ui.painter(), rect.center(), if liked { theme.accent } else { theme.text_muted }, liked);
            if resp.on_hover_text(if liked { t.music_unlike } else { t.music_like }).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *action = Some(Action::LikeAlbum(album.clone(), !liked));
            }
        }
    });
}

/// The songs, a row each: number (or the one playing), title and artist, album, length, heart. A
/// click plays from that song. `covers`: each with its album's cover (lists of songs from anywhere).
#[allow(clippy::too_many_arguments)]
fn song_table(ui: &mut Ui, lib: &mut Library, songs: &[Song], name: &str, covers: bool, playing: Option<&str>, theme: &Theme, t: &Strings, action: &mut Option<Action>) {
    let width = ui.available_width();
    let row_h = if covers { 48.0 } else { 40.0 };
    let album_w = if covers && width > 640.0 { (width * 0.28).min(280.0) } else { 0.0 };
    for (k, song) in songs.iter().enumerate() {
        let (row, resp) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click());
        if !ui.is_rect_visible(row) {
            continue;
        }
        let current = playing == Some(song.id.as_str());
        // The pointer on the row (also over its heart, which takes the hover).
        let inside = resp.contains_pointer();
        if inside || current {
            ui.painter().rect_filled(row, 6.0, if current { theme.tab_active } else { theme.tab_hover.gamma_multiply(0.7) });
        }
        let color = if current { theme.accent } else { theme.text };
        // The number, or ▶ on the row hovered, or the one playing.
        let number = if current { "♪".to_owned() } else if inside { "▶".to_owned() } else if covers { (k + 1).to_string() } else { song.track.max(k as u32 + 1).to_string() };
        ui.painter().text(Pos2::new(row.min.x + 20.0, row.center().y), Align2::CENTER_CENTER, number, FontId::proportional(12.5), if current || inside { theme.accent } else { theme.text_muted });
        let mut x = row.min.x + 44.0;
        if covers {
            let picture = Rect::from_min_size(Pos2::new(x, row.center().y - 18.0), Vec2::splat(36.0));
            paint_cover(ui, lib, &song.cover, COVER, picture, 4.0, theme);
            x += 48.0;
        }
        let right = row.max.x - 110.0 - album_w;
        let title_w = right - x - 12.0;
        let title = galley(ui, &song.title, 13.5, color, title_w);
        let by = galley(ui, &song.artist, 11.5, theme.text_muted, title_w);
        let top = row.center().y - (title.size().y + by.size().y + 2.0) / 2.0;
        ui.painter().galley(Pos2::new(x, top), title.clone(), color);
        ui.painter().galley(Pos2::new(x, top + title.size().y + 2.0), by, theme.text_muted);
        if album_w > 0.0 {
            let album = galley(ui, &song.album, 12.5, theme.text_muted, album_w - 12.0);
            ui.painter().galley(Pos2::new(right, row.center().y - album.size().y / 2.0), album, theme.text_muted);
        }
        ui.painter().text(Pos2::new(row.max.x - 16.0, row.center().y), Align2::RIGHT_CENTER, clock(song.duration), FontId::proportional(12.5), theme.text_muted);
        // The heart: always when liked, else on the row hovered.
        let heart = Rect::from_center_size(Pos2::new(row.max.x - 82.0, row.center().y), Vec2::splat(26.0));
        let liked = song.liked();
        let mut heart_clicked = false;
        if liked || inside {
            let hit = ui.interact(heart, ui.id().with(("song-like", &song.id, k)), Sense::click());
            super::music::paint_heart(ui.painter(), heart.center(), if liked { theme.accent } else if hit.hovered() { theme.text } else { theme.text_muted }, liked);
            heart_clicked = hit.on_hover_text(if liked { t.music_unlike } else { t.music_like }).clicked();
        }
        if heart_clicked {
            *action = Some(Action::Like(song.clone(), !liked));
        } else if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *action = Some(Action::Play(songs.to_vec(), k, name.to_owned()));
        }
    }
}

fn paint_play_big(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(-5.0, -8.0), c + Vec2::new(9.0, 0.0), c + Vec2::new(-5.0, 8.0)], color, Stroke::NONE));
}
