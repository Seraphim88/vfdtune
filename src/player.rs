//! Streaming via librespot 0.8. A wrapper `Sink` taps the PCM for the visualizer (pre-volume)
//! and applies software volume before handing samples to the real audio backend.

use librespot_metadata::{Metadata, Playlist as LsPlaylist, Track as LsTrack};
use librespot_core::{
    authentication::Credentials, config::SessionConfig, session::Session, SpotifyId, SpotifyUri,
};
use librespot_playback::{
    audio_backend::{self, Sink, SinkResult},
    config::{AudioFormat, Bitrate, PlayerConfig},
    convert::Converter,
    decoder::AudioPacket,
    mixer::NoOpVolume,
    player::{Player, PlayerEvent},
};
use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const RING: usize = 8192;

struct TapBuf {
    ring: Vec<f32>,
    pos: usize,
    filled: usize,
    last: Option<Instant>,
}

impl TapBuf {
    fn push(&mut self, stereo: &[f64]) {
        let len = self.ring.len();
        for fr in stereo.chunks(2) {
            let m = if fr.len() == 2 { (fr[0] + fr[1]) * 0.5 } else { fr[0] };
            self.ring[self.pos] = m as f32;
            self.pos = (self.pos + 1) % len;
        }
        self.filled = (self.filled + stereo.len() / 2).min(len);
        self.last = Some(Instant::now());
    }
}

/// Shared between the player thread (writer) and the UI (reader).
pub struct Tap {
    buf: Mutex<TapBuf>,
    gain: AtomicU32,
}

impl Tap {
    pub fn new(volume_pct: u8) -> Arc<Tap> {
        let t = Tap {
            buf: Mutex::new(TapBuf {
                ring: vec![0.0; RING],
                pos: 0,
                filled: 0,
                last: None,
            }),
            gain: AtomicU32::new(0),
        };
        t.set_volume(volume_pct);
        Arc::new(t)
    }

    /// 0..=100 -> amplitude (squared curve: 50% ~ -12 dB)
    pub fn set_volume(&self, pct: u8) {
        let x = pct.min(100) as f32 / 100.0;
        self.gain.store((x * x).to_bits(), Ordering::Relaxed);
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    fn push(&self, stereo: &[f64]) {
        if let Ok(mut b) = self.buf.lock() {
            b.push(stereo);
        }
    }

    /// Latest `n` mono samples, or None if audio isn't flowing (paused / stopped).
    pub fn snapshot(&self, n: usize, max_age: Duration) -> Option<Vec<f32>> {
        let b = self.buf.lock().ok()?;
        match b.last {
            Some(t) if t.elapsed() <= max_age && b.filled >= n => {}
            _ => return None,
        }
        let len = b.ring.len();
        let start = (b.pos + len - n) % len;
        Some((0..n).map(|i| b.ring[(start + i) % len]).collect())
    }
}

struct TapSink {
    inner: Box<dyn Sink>,
    tap: Arc<Tap>,
}

impl Sink for TapSink {
    fn start(&mut self) -> SinkResult<()> {
        self.inner.start()
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.inner.stop()
    }

    fn write(&mut self, mut packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        if let AudioPacket::Samples(ref mut data) = packet {
            self.tap.push(data);
            let g = self.tap.gain() as f64;
            if g < 0.9999 {
                for s in data.iter_mut() {
                    *s *= g;
                }
            }
        }
        self.inner.write(packet, converter)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PlayerMsg {
    Loading,
    Playing(u32),
    Paused(u32),
    Stopped,
    EndOfTrack,
    Unavailable,
    Seeked(u32),
    PreloadNext,
}

pub struct Engine {
    session: Session,
    player: Arc<Player>,
    loaded: Arc<Mutex<Option<SpotifyUri>>>,
}

impl Engine {
    pub async fn start<F>(
        token: String,
        tap: Arc<Tap>,
        backend: Option<String>,
        bitrate: u16,
        emit: F,
    ) -> Result<Engine, String>
    where
        F: Fn(PlayerMsg) + Send + 'static,
    {
        let sink_builder = audio_backend::find(backend).ok_or("unknown audio backend")?;

        let session = Session::new(SessionConfig::default(), None);
        session
            .connect(Credentials::with_access_token(token), false)
            .await
            .map_err(|e| format!("Spotify session: {e}"))?;

        let mut cfg = PlayerConfig::default();
        cfg.bitrate = match bitrate {
            96 => Bitrate::Bitrate96,
            160 => Bitrate::Bitrate160,
            _ => Bitrate::Bitrate320,
        };
        let format = AudioFormat::default();
        let player = Player::new(cfg, session.clone(), Box::new(NoOpVolume), move || {
            let inner = sink_builder(None, format);
            Box::new(TapSink { inner, tap }) as Box<dyn Sink>
        });

        let loaded: Arc<Mutex<Option<SpotifyUri>>> = Arc::new(Mutex::new(None));
        let loaded2 = loaded.clone();
        let mut rx = player.get_player_event_channel();
        tokio::spawn(async move {
            let mut cur: u64 = 0;
            while let Some(ev) = rx.recv().await {
                match ev {
                    PlayerEvent::PlayRequestIdChanged { play_request_id } => cur = play_request_id,
                    PlayerEvent::Loading { play_request_id, .. } if play_request_id == cur => {
                        emit(PlayerMsg::Loading)
                    }
                    PlayerEvent::Playing { play_request_id, position_ms, .. }
                        if play_request_id == cur =>
                    {
                        emit(PlayerMsg::Playing(position_ms))
                    }
                    PlayerEvent::Paused { play_request_id, position_ms, .. }
                        if play_request_id == cur =>
                    {
                        emit(PlayerMsg::Paused(position_ms))
                    }
                    PlayerEvent::Stopped { play_request_id, .. } if play_request_id == cur => {
                        emit(PlayerMsg::Stopped)
                    }
                    PlayerEvent::EndOfTrack { play_request_id, .. } if play_request_id == cur => {
                        emit(PlayerMsg::EndOfTrack)
                    }
                    PlayerEvent::Seeked { play_request_id, position_ms, .. }
                        if play_request_id == cur =>
                    {
                        emit(PlayerMsg::Seeked(position_ms))
                    }
                    PlayerEvent::PositionCorrection { play_request_id, position_ms, .. }
                        if play_request_id == cur =>
                    {
                        emit(PlayerMsg::Seeked(position_ms))
                    }
                    PlayerEvent::TimeToPreloadNextTrack { play_request_id, .. }
                        if play_request_id == cur =>
                    {
                        emit(PlayerMsg::PreloadNext)
                    }
                    PlayerEvent::Unavailable { play_request_id, track_id, .. }
                        if play_request_id == cur =>
                    {
                        // A failed *preload* is reported with the current request id too;
                        // only react when it's the track we actually asked to load.
                        let is_current = loaded2
                            .lock()
                            .map(|l| l.as_ref() == Some(&track_id))
                            .unwrap_or(false);
                        if is_current {
                            emit(PlayerMsg::Unavailable);
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(Engine {
            session,
            player,
            loaded,
        })
    }

    pub fn session(&self) -> Session {
        self.session.clone()
    }

    pub fn alive(&self) -> bool {
        !self.session.is_invalid() && !self.player.is_invalid()
    }

    pub fn load(&self, base62_id: &str) -> Result<(), String> {
        let id = SpotifyId::from_base62(base62_id).map_err(|e| format!("bad track id: {e:?}"))?;
        let uri = SpotifyUri::Track { id };
        if let Ok(mut l) = self.loaded.lock() {
            *l = Some(uri.clone());
        }
        self.player.load(uri, true, 0);
        Ok(())
    }

    pub fn preload(&self, base62_id: &str) {
        if let Ok(id) = SpotifyId::from_base62(base62_id) {
            self.player.preload(SpotifyUri::Track { id });
        }
    }

    pub fn play(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn stop(&self) {
        self.player.stop();
    }

    pub fn seek(&self, ms: u32) {
        self.player.seek(ms);
    }
}

/// Fallback for playlists the Web API refuses (403: not yours / Spotify-made). Reads the track
/// list through the librespot session, then each track's metadata (8 at a time). First `cap` tracks.
pub async fn playlist_fallback(
    session: Session,
    base62_id: String,
    cap: usize,
) -> Result<Vec<crate::api::Track>, String> {
    use crate::api::Track as AppTrack;

    let id = SpotifyId::from_base62(&base62_id).map_err(|e| format!("bad playlist id: {e:?}"))?;
    let uri = SpotifyUri::Playlist { id, user: None };
    let pl = LsPlaylist::get(&session, &uri)
        .await
        .map_err(|e| format!("playlist metadata: {e}"))?;
    let uris: Vec<SpotifyUri> = pl
        .tracks()
        .filter(|u| matches!(u, SpotifyUri::Track { .. }))
        .take(cap)
        .cloned()
        .collect();

    let mut out: Vec<AppTrack> = Vec::with_capacity(uris.len());
    for chunk in uris.chunks(8) {
        let mut set = tokio::task::JoinSet::new();
        for (i, u) in chunk.iter().cloned().enumerate() {
            let s = session.clone();
            set.spawn(async move { (i, LsTrack::get(&s, &u).await) });
        }
        let mut got: Vec<Option<AppTrack>> = vec![None; chunk.len()];
        while let Some(res) = set.join_next().await {
            if let Ok((i, Ok(t))) = res {
                if let SpotifyUri::Track { id } = &t.id {
                    if let Ok(b62) = id.to_base62() {
                        got[i] = Some(AppTrack {
                            id: b62,
                            name: t.name.clone(),
                            artists: t
                                .artists
                                .0
                                .iter()
                                .map(|a| a.name.clone())
                                .collect::<Vec<_>>()
                                .join(", "),
                            album: t.album.name.clone(),
                            duration_ms: t.duration.max(0) as u32,
                        });
                    }
                }
            }
        }
        out.extend(got.into_iter().flatten());
    }
    if out.is_empty() {
        Err("playlist is empty or unreadable".to_string())
    } else {
        Ok(out)
    }
}
