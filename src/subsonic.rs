//! The music server: any that speaks the Subsonic API (Navidrome, Gonic, Airsonic, Ampache…). Its address and user in the settings,
//! its password with the others (ssh::save_password), under `PASSWORD_ID`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use md5::{Digest, Md5};
use serde::Deserialize;
use uuid::Uuid;

/// The key of the server's password among the saved passwords (those of hosts and connections are
/// random ids: this one can't meet them).
pub const PASSWORD_ID: Uuid = Uuid::from_u128(0x6e61_7669_6472_6f6d_6500_0000_0000_0001);

/// Why the server didn't let us in.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// No answer, or not over HTTP.
    Unreachable(String),
    /// Wrong user or password.
    Refused,
    /// Something answered, but not a Subsonic server.
    NotSubsonic,
    /// The server's own error message.
    Server(String),
    /// The server doesn't take the salted token (some only take the password).
    NoToken,
}

/// The address as typed, made usable: https:// when no scheme is given, no "/" at the end.
pub fn normalize_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() || url.contains("://") { url.to_owned() } else { format!("https://{url}") }
}

/// Where the server is, and who we are on it.
#[derive(Clone, Debug)]
pub struct Server {
    url: String,
    user: String,
    password: String,
    /// The server refused the token once: the password goes instead (shared by the clones).
    plain: Arc<AtomicBool>,
}

/// A genre of the library, with how many songs it has.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Genre {
    #[serde(rename = "value")]
    pub name: String,
    #[serde(rename = "songCount", default)]
    pub songs: u32,
}

/// A song, as the server lists it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Song {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    /// In seconds.
    #[serde(default)]
    pub duration: u32,
    /// The file's kind ("flac", "mp3"…).
    #[serde(default)]
    pub suffix: String,
    /// Liked (starred): when.
    #[serde(default)]
    pub starred: Option<String>,
    #[serde(rename = "albumId", default)]
    pub album_id: String,
    #[serde(rename = "artistId", default)]
    pub artist_id: String,
    #[serde(rename = "coverArt", default)]
    pub cover: String,
    #[serde(default)]
    pub track: u32,
    #[serde(rename = "discNumber", default)]
    pub disc: u32,
    #[serde(default)]
    pub year: u32,
}

/// An album, as listed.
#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Album {
    pub id: String,
    #[serde(default, alias = "title")]
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(rename = "artistId", default)]
    pub artist_id: String,
    #[serde(rename = "coverArt", default)]
    pub cover: String,
    #[serde(rename = "songCount", default)]
    pub songs: u32,
    /// In seconds.
    #[serde(default)]
    pub duration: u32,
    #[serde(default)]
    pub year: u32,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub starred: Option<String>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Artist {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "albumCount", default)]
    pub albums: u32,
    #[serde(rename = "coverArt", default)]
    pub cover: String,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Playlist {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "songCount", default)]
    pub songs: u32,
    #[serde(default)]
    pub duration: u32,
    #[serde(rename = "coverArt", default)]
    pub cover: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub comment: String,
}

/// Which albums, in which order (getAlbumList2's "type").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlbumOrder {
    Newest,
    Recent,
    Frequent,
    Random,
    ByName,
    ByArtist,
    Starred,
}

impl AlbumOrder {
    fn key(self) -> &'static str {
        match self {
            Self::Newest => "newest",
            Self::Recent => "recent",
            Self::Frequent => "frequent",
            Self::Random => "random",
            Self::ByName => "alphabeticalByName",
            Self::ByArtist => "alphabeticalByArtist",
            Self::Starred => "starred",
        }
    }
}

/// Albums and songs a search asks for at a time.
pub const SEARCH_ALBUMS: usize = 30;
pub const SEARCH_SONGS: usize = 50;

/// What a search found.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

impl Song {
    pub fn liked(&self) -> bool {
        self.starred.is_some()
    }
}

/// The kinds of files played as they are, from their first bytes. The others are converted to MP3 by
/// the server: M4A often keeps its index at the very end (nothing plays until all of it is there), an
/// OGG may hold Opus (not decoded here).
const PLAYABLE: [&str; 3] = ["mp3", "flac", "wav"];

/// A song's audio as it arrives, and its size when the server says it.
pub struct Stream {
    pub reader: Box<dyn std::io::Read + Send>,
    pub len: Option<u64>,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "subsonic-response")]
    response: Response,
}

#[derive(Deserialize)]
struct Response {
    status: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "serverVersion")]
    server_version: Option<String>,
    version: Option<String>,
    error: Option<ApiError>,
    genres: Option<Genres>,
    #[serde(rename = "randomSongs")]
    random_songs: Option<Songs>,
    starred2: Option<Songs>,
    #[serde(rename = "albumList2")]
    album_list: Option<Albums>,
    album: Option<AlbumWithSongs>,
    artists: Option<ArtistIndex>,
    artist: Option<ArtistWithAlbums>,
    playlists: Option<Playlists>,
    playlist: Option<PlaylistWithSongs>,
    #[serde(rename = "searchResult3")]
    search: Option<SearchResult>,
}

#[derive(Deserialize)]
struct ApiError {
    code: u32,
    message: Option<String>,
}

#[derive(Deserialize)]
struct Genres {
    #[serde(default)]
    genre: Vec<Genre>,
}

#[derive(Deserialize)]
struct Songs {
    #[serde(default)]
    song: Vec<Song>,
}

#[derive(Deserialize)]
struct Albums {
    #[serde(default)]
    album: Vec<Album>,
}

#[derive(Deserialize)]
struct AlbumWithSongs {
    #[serde(flatten)]
    album: Album,
    #[serde(default)]
    song: Vec<Song>,
}

#[derive(Deserialize)]
struct ArtistIndex {
    #[serde(default)]
    index: Vec<ArtistLetter>,
}

#[derive(Deserialize)]
struct ArtistLetter {
    #[serde(default)]
    artist: Vec<Artist>,
}

#[derive(Deserialize)]
struct ArtistWithAlbums {
    #[serde(flatten)]
    artist: Artist,
    #[serde(default)]
    album: Vec<Album>,
}

#[derive(Deserialize)]
struct Playlists {
    #[serde(default)]
    playlist: Vec<Playlist>,
}

#[derive(Deserialize)]
struct PlaylistWithSongs {
    #[serde(flatten)]
    playlist: Playlist,
    #[serde(default)]
    entry: Vec<Song>,
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    artist: Vec<Artist>,
    #[serde(default)]
    album: Vec<Album>,
    #[serde(default)]
    song: Vec<Song>,
}

impl Server {
    /// Where it is and who we are on it (what tells two servers apart).
    pub fn address(&self) -> (&str, &str) {
        (&self.url, &self.user)
    }

    pub fn new(url: &str, user: &str, password: &str) -> Self {
        Self { url: url.to_owned(), user: user.to_owned(), password: password.to_owned(), plain: Arc::default() }
    }

    /// Asks the server whether it answers and accepts the user. Its name and version when it does
    /// ("navidrome 0.53.3").
    pub fn ping(&self) -> Result<String, Failure> {
        let response = self.call("ping", &[])?;
        let name = response.kind.unwrap_or_else(|| "Subsonic".into());
        Ok(match response.server_version.or(response.version) {
            Some(version) => format!("{name} {version}"),
            None => name,
        })
    }

    /// The library's genres, the biggest first.
    pub fn genres(&self) -> Result<Vec<Genre>, Failure> {
        let mut genres = self.call("getGenres", &[])?.genres.map(|g| g.genre).unwrap_or_default();
        genres.retain(|g| g.songs > 0 && !g.name.trim().is_empty());
        genres.sort_by(|a, b| b.songs.cmp(&a.songs).then_with(|| a.name.cmp(&b.name)));
        Ok(genres)
    }

    /// Up to `count` songs drawn at random, of `genre` (any when None).
    pub fn random_songs(&self, genre: Option<&str>, count: u32) -> Result<Vec<Song>, Failure> {
        let count = count.to_string();
        let mut query = vec![("size", count.as_str())];
        if let Some(genre) = genre {
            query.push(("genre", genre));
        }
        Ok(self.call("getRandomSongs", &query)?.random_songs.map(|s| s.song).unwrap_or_default())
    }

    /// The songs liked, the latest first.
    pub fn liked(&self) -> Result<Vec<Song>, Failure> {
        let mut songs = self.call("getStarred2", &[])?.starred2.map(|s| s.song).unwrap_or_default();
        songs.sort_by(|a, b| b.starred.cmp(&a.starred));
        Ok(songs)
    }

    /// Albums, `count` from `offset`, in `order`.
    pub fn albums(&self, order: AlbumOrder, count: u32, offset: u32) -> Result<Vec<Album>, Failure> {
        let (count, offset) = (count.to_string(), offset.to_string());
        Ok(self.call("getAlbumList2", &[("type", order.key()), ("size", &count), ("offset", &offset)])?.album_list.map(|a| a.album).unwrap_or_default())
    }

    /// An album and its songs, in order.
    pub fn album(&self, id: &str) -> Result<(Album, Vec<Song>), Failure> {
        let album = self.call("getAlbum", &[("id", id)])?.album.ok_or(Failure::NotSubsonic)?;
        let mut songs = album.song;
        songs.sort_by_key(|s| (s.disc, s.track));
        Ok((album.album, songs))
    }

    /// All the artists, by name.
    pub fn artists(&self) -> Result<Vec<Artist>, Failure> {
        let index = self.call("getArtists", &[])?.artists.map(|a| a.index).unwrap_or_default();
        Ok(index.into_iter().flat_map(|letter| letter.artist).collect())
    }

    /// An artist and their albums, the latest first.
    pub fn artist(&self, id: &str) -> Result<(Artist, Vec<Album>), Failure> {
        let artist = self.call("getArtist", &[("id", id)])?.artist.ok_or(Failure::NotSubsonic)?;
        let mut albums = artist.album;
        albums.sort_by(|a, b| b.year.cmp(&a.year).then_with(|| a.name.cmp(&b.name)));
        Ok((artist.artist, albums))
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>, Failure> {
        Ok(self.call("getPlaylists", &[])?.playlists.map(|p| p.playlist).unwrap_or_default())
    }

    /// A playlist and its songs, in its order.
    pub fn playlist(&self, id: &str) -> Result<(Playlist, Vec<Song>), Failure> {
        let playlist = self.call("getPlaylist", &[("id", id)])?.playlist.ok_or(Failure::NotSubsonic)?;
        Ok((playlist.playlist, playlist.entry))
    }

    /// Artists, albums and songs matching `query`.
    pub fn search(&self, query: &str) -> Result<Found, Failure> {
        let (albums, songs) = (SEARCH_ALBUMS.to_string(), SEARCH_SONGS.to_string());
        let found = self.call("search3", &[("query", query), ("artistCount", "12"), ("albumCount", &albums), ("songCount", &songs)])?.search;
        Ok(found.map(|f| Found { artists: f.artist, albums: f.album, songs: f.song }).unwrap_or_default())
    }

    /// The songs matching `query` after the first `offset` (the next page of a search).
    pub fn search_songs(&self, query: &str, offset: usize) -> Result<Vec<Song>, Failure> {
        let (count, offset) = (SEARCH_SONGS.to_string(), offset.to_string());
        let found = self.call("search3", &[("query", query), ("artistCount", "0"), ("albumCount", "0"), ("songCount", &count), ("songOffset", &offset)])?.search;
        Ok(found.map(|f| f.song).unwrap_or_default())
    }

    /// A cover, as an image file (JPEG, PNG…), about `size` pixels wide.
    pub fn cover(&self, id: &str, size: u32) -> Result<Vec<u8>, Failure> {
        let size = size.to_string();
        let mut answer = self.request("getCoverArt", Some(Duration::from_secs(30))).query("id", id).query("size", &size).call().map_err(|e| Failure::Unreachable(e.to_string()))?;
        let image = answer.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("image/"));
        if !image || !answer.status().is_success() {
            return Err(Failure::NotSubsonic);
        }
        answer.body_mut().with_config().limit(20 * 1024 * 1024).read_to_vec().map_err(|e| Failure::Unreachable(e.to_string()))
    }

    /// Tells the server a song is being played (`done` false), or was (it counts in "most played"
    /// and "recently played").
    pub fn scrobble(&self, id: &str, done: bool) -> Result<(), Failure> {
        self.call("scrobble", &[("id", id), ("submission", if done { "true" } else { "false" })]).map(|_| ())
    }

    /// Likes the album (`on`), or no longer.
    pub fn like_album(&self, id: &str, on: bool) -> Result<(), Failure> {
        self.call(if on { "star" } else { "unstar" }, &[("albumId", id)]).map(|_| ())
    }

    /// Likes the song (`on`), or no longer.
    pub fn like(&self, id: &str, on: bool) -> Result<(), Failure> {
        self.call(if on { "star" } else { "unstar" }, &[("id", id)]).map(|_| ())
    }

    /// A song's audio, as it comes: the file itself when it can be played as it is (nothing to wait
    /// for), else converted to MP3 by the server.
    pub fn stream(&self, song: &Song) -> Result<Stream, Failure> {
        self.again_without_token(|| self.stream_once(song))
    }

    fn stream_once(&self, song: &Song) -> Result<Stream, Failure> {
        let raw = PLAYABLE.contains(&song.suffix.to_lowercase().as_str());
        let request = self.request("stream", None).query("id", &song.id);
        let request = if raw { request.query("format", "raw") } else { request.query("format", "mp3").query("maxBitRate", "320") };
        let mut answer = request.call().map_err(|e| Failure::Unreachable(e.to_string()))?;
        // An error comes as JSON (or XML) instead of the audio.
        let text = answer.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.contains("json") || t.contains("xml") || t.contains("text/"));
        if text || !answer.status().is_success() {
            let bytes = answer.body_mut().read_to_vec().unwrap_or_default();
            return Err(match serde_json::from_slice::<Envelope>(&bytes) {
                Ok(envelope) => failure(envelope.response.error),
                Err(_) => Failure::Server(format!("HTTP {}", answer.status())),
            });
        }
        let len = answer.body().content_length();
        Ok(Stream { reader: Box::new(answer.into_body().into_with_config().limit(u64::MAX).reader()), len })
    }

    /// One API call, answered in JSON.
    fn call(&self, method: &str, query: &[(&str, &str)]) -> Result<Response, Failure> {
        self.again_without_token(|| self.call_once(method, query))
    }

    /// Tried with the token, then with the password if the server only takes that.
    fn again_without_token<T>(&self, attempt: impl Fn() -> Result<T, Failure>) -> Result<T, Failure> {
        match attempt() {
            Err(Failure::NoToken) if !self.plain.swap(true, Ordering::Relaxed) => attempt(),
            other => other,
        }
    }

    fn call_once(&self, method: &str, query: &[(&str, &str)]) -> Result<Response, Failure> {
        let mut request = self.request(method, Some(Duration::from_secs(20)));
        for (key, value) in query {
            request = request.query(*key, *value);
        }
        let mut answer = request.call().map_err(|e| Failure::Unreachable(e.to_string()))?;
        let envelope: Envelope = answer.body_mut().read_json().map_err(|_| Failure::NotSubsonic)?;
        let response = envelope.response;
        if response.status == "ok" {
            return Ok(response);
        }
        Err(failure(response.error))
    }

    /// A request to the API, authenticated with a salted token (the password itself never goes over
    /// the wire), or with the password, hex-encoded, for the servers that only take that.
    /// `total`: None for a song, which takes as long as it takes (but must start answering soon).
    fn request(&self, method: &str, total: Option<Duration>) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
        let agent = ureq::config::Config::builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(total)
            .timeout_recv_response(Some(Duration::from_secs(30)))
            // A wrong password is a 200 with an error inside; anything else is read as an answer too.
            .http_status_as_error(false)
            .build()
            .new_agent();
        let request = agent.get(format!("{}/rest/{method}", normalize_url(&self.url))).query("u", &self.user);
        let request = if self.plain.load(Ordering::Relaxed) {
            request.query("p", format!("enc:{}", self.password.bytes().map(|b| format!("{b:02x}")).collect::<String>()))
        } else {
            let salt = Uuid::new_v4().simple().to_string();
            let token: String = Md5::digest(format!("{}{salt}", self.password)).iter().map(|b| format!("{b:02x}")).collect();
            request.query("t", token).query("s", salt)
        };
        request
            .query("v", "1.16.1")
            .query("c", "ronnie")
            .query("f", "json")
            .header("User-Agent", concat!("ronnie/", env!("CARGO_PKG_VERSION")))
    }
}

fn failure(error: Option<ApiError>) -> Failure {
    match error {
        // 40: wrong user or password; 41: tokens not taken.
        Some(ApiError { code: 40, .. }) => Failure::Refused,
        Some(ApiError { code: 41, .. }) => Failure::NoToken,
        Some(ApiError { message: Some(message), .. }) => Failure::Server(message),
        _ => Failure::NotSubsonic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(normalize_url(" music.example.com/ "), "https://music.example.com");
        assert_eq!(normalize_url("http://192.168.1.10:4533/"), "http://192.168.1.10:4533");
        assert_eq!(normalize_url(""), "");
    }
}
