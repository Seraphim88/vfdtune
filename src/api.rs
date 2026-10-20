//! Minimal Spotify Web API client.
//! Responses are parsed leniently from `serde_json::Value` because Spotify changed several
//! shapes in Feb/Mar 2026 (playlist `/items`, row key `item` vs `track`, search limit max 10).

use crate::auth::Auth;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub duration_ms: u32,
}

#[derive(Clone, Debug)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artists: String,
    pub tracks: u32,
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub tracks: u32,
}

#[derive(Clone, Debug)]
pub enum Item {
    Track(Track),
    Album(Album),
    Playlist(Playlist),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Tracks,
    Albums,
    Playlists,
}

#[derive(Clone, Debug)]
pub enum Source {
    Liked,
    SavedAlbums,
    MyPlaylists,
    Playlist(String),
    Album { id: String, name: String },
    Search { q: String, kind: Kind },
}

pub struct PageData {
    pub items: Vec<Item>,
    pub total: u32,
    /// raw rows consumed (incl. nulls/filtered) -> next offset
    pub raw: u32,
}

#[derive(Debug)]
pub struct ApiErr {
    pub status: u16,
    pub msg: String,
}

impl std::fmt::Display for ApiErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl From<String> for ApiErr {
    fn from(s: String) -> Self {
        ApiErr { status: 0, msg: s }
    }
}

// ---------- lenient JSON helpers ----------

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn n(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0) as u32
}

fn artists(v: &Value) -> String {
    v.get("artists")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn parse_track(v: &Value, album_fallback: &str) -> Option<Track> {
    if !v.is_object() {
        return None;
    }
    if let Some(t) = v.get("type").and_then(Value::as_str) {
        if t != "track" {
            return None; // episodes etc.
        }
    }
    let id = v.get("id").and_then(Value::as_str)?.to_string(); // null for local files
    let album = v
        .get("album")
        .and_then(|a| a.get("name"))
        .and_then(Value::as_str)
        .unwrap_or(album_fallback)
        .to_string();
    Some(Track {
        id,
        name: s(v, "name"),
        artists: artists(v),
        album,
        duration_ms: n(v, "duration_ms"),
    })
}

/// Playlist / liked-songs row: 2026 shape `{"item": {...}}`, legacy `{"track": {...}}`.
fn row_track(row: &Value) -> Option<Track> {
    for key in ["item", "track"] {
        if let Some(v) = row.get(key) {
            if v.is_object() {
                return parse_track(v, "");
            }
        }
    }
    None
}

fn parse_album(v: &Value) -> Option<Album> {
    let id = v.get("id").and_then(Value::as_str)?.to_string();
    Some(Album {
        id,
        name: s(v, "name"),
        artists: artists(v),
        tracks: n(v, "total_tracks"),
    })
}

fn parse_playlist(v: &Value) -> Option<Playlist> {
    let id = v.get("id").and_then(Value::as_str)?.to_string();
    let owner = v
        .get("owner")
        .map(|o| {
            let d = s(o, "display_name");
            if d.is_empty() {
                s(o, "id")
            } else {
                d
            }
        })
        .unwrap_or_default();
    let tracks = ["items", "tracks"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|t| t.get("total")).and_then(Value::as_u64))
        .unwrap_or(0) as u32;
    Some(Playlist {
        id,
        name: s(v, "name"),
        owner,
        tracks,
    })
}

fn page_parts(p: &Value) -> (&[Value], u32) {
    let items: &[Value] = p
        .get("items")
        .and_then(Value::as_array)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    (items, n(p, "total"))
}

fn done(items: Vec<Item>, rows: usize, total: u32) -> Result<PageData, ApiErr> {
    Ok(PageData {
        items,
        total,
        raw: rows as u32,
    })
}

// ---------- client ----------

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    auth: Arc<Auth>,
    search_limit: u32,
}

impl Api {
    pub fn new(auth: Arc<Auth>, search_limit: u32) -> Api {
        Api {
            http: reqwest::Client::builder()
                .user_agent("vfdtune/0.1")
                .build()
                .expect("http client"),
            auth,
            search_limit: search_limit.clamp(1, 50),
        }
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, ApiErr> {
        let url = format!("https://api.spotify.com/v1{path}");
        let mut token = self.auth.access_token().await?;
        for attempt in 0..2 {
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&token)
                .query(params)
                .send()
                .await
                .map_err(|e| ApiErr {
                    status: 0,
                    msg: format!("network error: {e}"),
                })?;
            let status = resp.status().as_u16();
            if status == 401 && attempt == 0 {
                token = self.auth.force_refresh().await?;
                continue;
            }
            if status == 429 {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|x| x.parse::<u64>().ok())
                    .unwrap_or(5);
                return Err(ApiErr {
                    status,
                    msg: if wait > 3600 {
                        format!("rate limited by Spotify for {}h: this client id is throttled", wait / 3600)
                    } else {
                        format!("rate limited by Spotify, retry in {wait}s")
                    },
                });
            }
            let body = resp.text().await.map_err(|e| ApiErr {
                status,
                msg: e.to_string(),
            })?;
            if (200..300).contains(&status) {
                return serde_json::from_str(&body).map_err(|e| ApiErr {
                    status,
                    msg: format!("bad JSON from Spotify: {e}"),
                });
            }
            let detail = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default();
            let hint = if status == 403 {
                " (endpoint or playlist not available to this app/account)"
            } else {
                ""
            };
            return Err(ApiErr {
                status,
                msg: format!("Spotify {status}: {detail}{hint}"),
            });
        }
        Err(ApiErr {
            status: 401,
            msg: "unauthorized".into(),
        })
    }

    pub async fn fetch(&self, src: &Source, offset: u32) -> Result<PageData, ApiErr> {
        let off = ("offset", offset.to_string());
        match src {
            Source::Liked => {
                let v = self
                    .get("/me/tracks", &[("limit", "50".to_string()), off])
                    .await?;
                let (rows, total) = page_parts(&v);
                let items = rows.iter().filter_map(row_track).map(Item::Track).collect();
                done(items, rows.len(), total)
            }
            Source::SavedAlbums => {
                let v = self
                    .get("/me/albums", &[("limit", "50".to_string()), off])
                    .await?;
                let (rows, total) = page_parts(&v);
                let items = rows
                    .iter()
                    .filter_map(|r| r.get("album").and_then(parse_album))
                    .map(Item::Album)
                    .collect();
                done(items, rows.len(), total)
            }
            Source::MyPlaylists => {
                let v = self
                    .get("/me/playlists", &[("limit", "50".to_string()), off])
                    .await?;
                let (rows, total) = page_parts(&v);
                let items = rows.iter().filter_map(parse_playlist).map(Item::Playlist).collect();
                done(items, rows.len(), total)
            }
            Source::Playlist(id) => {
                let params = [("limit", "50".to_string()), off];
                let v = match self.get(&format!("/playlists/{id}/items"), &params).await {
                    Ok(v) => v,
                    Err(e) if e.status == 404 => {
                        self.get(&format!("/playlists/{id}/tracks"), &params).await?
                    }
                    Err(e) => return Err(e),
                };
                let (rows, total) = page_parts(&v);
                let items = rows.iter().filter_map(row_track).map(Item::Track).collect();
                done(items, rows.len(), total)
            }
            Source::Album { id, name } => {
                let v = self
                    .get(
                        &format!("/albums/{id}/tracks"),
                        &[
                            ("limit", "50".to_string()),
                            ("market", "from_token".to_string()),
                            off,
                        ],
                    )
                    .await?;
                let (rows, total) = page_parts(&v);
                let items = rows
                    .iter()
                    .filter_map(|t| parse_track(t, name))
                    .map(Item::Track)
                    .collect();
                done(items, rows.len(), total)
            }
            Source::Search { q, kind } => {
                let ty = match kind {
                    Kind::Tracks => "track",
                    Kind::Albums => "album",
                    Kind::Playlists => "playlist",
                };
                let v = self
                    .get(
                        "/search",
                        &[
                            ("q", q.clone()),
                            ("type", ty.to_string()),
                            ("limit", self.search_limit.to_string()),
                            off,
                        ],
                    )
                    .await?;
                let key = format!("{ty}s");
                let page = v.get(key.as_str()).cloned().unwrap_or(Value::Null);
                let (rows, total) = page_parts(&page);
                let items: Vec<Item> = match kind {
                    Kind::Tracks => rows
                        .iter()
                        .filter_map(|t| parse_track(t, ""))
                        .map(Item::Track)
                        .collect(),
                    Kind::Albums => rows.iter().filter_map(parse_album).map(Item::Album).collect(),
                    Kind::Playlists => rows
                        .iter()
                        .filter_map(parse_playlist)
                        .map(Item::Playlist)
                        .collect(),
                };
                done(items, rows.len(), total)
            }
        }
    }
}
