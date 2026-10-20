use crate::api::{Api, Item, Kind, PageData, Source, Track};
use crate::auth::Auth;
use crate::config::Config;
use crate::player::{Engine, PlayerMsg, Tap};
use crate::queue::Queue;
use crate::ui::THEMES;
use crate::viz::{Viz, FFT_N};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::UnboundedSender;

pub enum Event {
    Key(KeyEvent),
    Player(PlayerMsg),
    Engine(Result<Engine, String>),
    Page {
        target: Target,
        ver: u64,
        res: Result<PageData, String>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Liked,
    SavedAlbums,
    Playlists,
    SearchTracks,
    SearchAlbums,
    SearchPlaylists,
    Detail,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Queue,
    Library,
    Playlists,
    Search,
}

const TABS: [Tab; 4] = [Tab::Queue, Tab::Library, Tab::Playlists, Tab::Search];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VizMode {
    Spectrum,
    Mirror,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PState {
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Default, Clone, Copy)]
pub struct Cursor {
    pub sel: usize,
    pub top: usize,
}

impl Cursor {
    pub fn move_by(&mut self, len: usize, d: isize) {
        if len == 0 {
            self.sel = 0;
            return;
        }
        let n = (self.sel as isize).saturating_add(d).clamp(0, len as isize - 1);
        self.sel = n as usize;
    }

    /// Scrolls so `sel` is visible and returns the slice of rows to draw.
    pub fn window(&mut self, len: usize, h: usize) -> std::ops::Range<usize> {
        if len == 0 || h == 0 {
            self.sel = 0;
            self.top = 0;
            return 0..0;
        }
        if self.sel >= len {
            self.sel = len - 1;
        }
        if self.sel < self.top {
            self.top = self.sel;
        }
        if self.sel >= self.top + h {
            self.top = self.sel + 1 - h;
        }
        if self.top + h > len {
            self.top = len.saturating_sub(h);
        }
        self.top..(self.top + h).min(len)
    }
}

pub struct ListView {
    pub items: Vec<Item>,
    pub cur: Cursor,
    pub total: u32,
    pub next_offset: u32,
    pub loading: bool,
    pub error: Option<String>,
    pub ver: u64,
    pub source: Option<Source>,
    pub requested: bool,
    pub fallback_tried: bool,
}

impl ListView {
    pub fn new() -> Self {
        ListView {
            items: Vec::new(),
            cur: Cursor::default(),
            total: 0,
            next_offset: 0,
            loading: false,
            error: None,
            ver: 0,
            source: None,
            requested: false,
            fallback_tried: false,
        }
    }

    pub fn reset(&mut self, source: Source, ver: u64) {
        self.items.clear();
        self.cur = Cursor::default();
        self.total = 0;
        self.next_offset = 0;
        self.loading = false;
        self.error = None;
        self.ver = ver;
        self.source = Some(source);
        self.requested = false;
        self.fallback_tried = false;
    }

    pub fn wants_more(&self) -> bool {
        self.source.is_some()
            && !self.loading
            && self.error.is_none()
            && (!self.requested
                || (self.next_offset < self.total && self.cur.sel + 5 >= self.items.len()))
    }

    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.cur.sel)
    }
}

pub struct Detail {
    pub title: String,
    pub view: ListView,
}

pub struct NowPlaying {
    pub track: Option<Track>,
    pub state: PState,
    base_ms: u32,
    since: Instant,
}

impl NowPlaying {
    fn set(&mut self, state: PState, pos: u32) {
        self.state = state;
        self.base_ms = pos;
        self.since = Instant::now();
    }

    pub fn pos_ms(&self) -> u32 {
        let dur = self.track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
        let p = if self.state == PState::Playing {
            self.base_ms
                .saturating_add(self.since.elapsed().as_millis() as u32)
        } else {
            self.base_ms
        };
        if dur > 0 {
            p.min(dur)
        } else {
            p
        }
    }
}

pub struct App {
    pub theme: usize,
    pub tab: Tab,
    pub queue: Queue,
    pub qcur: Cursor,
    pub lib_kind: Kind,
    pub liked: ListView,
    pub saved_albums: ListView,
    pub playlists: ListView,
    pub search_input: Option<String>,
    pub search_q: String,
    pub search_kind: Kind,
    pub s_tracks: ListView,
    pub s_albums: ListView,
    pub s_playlists: ListView,
    pub detail: Option<Detail>,
    pub now: NowPlaying,
    pub volume: u8,
    pub muted: bool,
    pub viz: Viz,
    pub viz_mode: VizMode,
    pub viz_bands: usize,
    pub tap: Arc<Tap>,
    pub engine: Option<Engine>,
    pub msg: Option<(String, Instant)>,
    pub conn_note: Option<String>,
    pub show_help: bool,
    pub tick: u64,
    pub quit: bool,
    cfg: Config,
    connecting: bool,
    want_play: bool,
    skips: usize,
    ver_ctr: u64,
    tx: UnboundedSender<Event>,
    api: Api,
    auth: Arc<Auth>,
}

impl App {
    pub fn new(
        cfg: Config,
        auth: Arc<Auth>,
        api: Api,
        tx: UnboundedSender<Event>,
        tap: Arc<Tap>,
    ) -> App {
        let mut app = App {
            theme: cfg.theme % THEMES.len(),
            tab: Tab::Library,
            queue: Queue::new(),
            qcur: Cursor::default(),
            lib_kind: Kind::Tracks,
            liked: ListView::new(),
            saved_albums: ListView::new(),
            playlists: ListView::new(),
            search_input: None,
            search_q: String::new(),
            search_kind: Kind::Tracks,
            s_tracks: ListView::new(),
            s_albums: ListView::new(),
            s_playlists: ListView::new(),
            detail: None,
            now: NowPlaying {
                track: None,
                state: PState::Stopped,
                base_ms: 0,
                since: Instant::now(),
            },
            volume: cfg.volume.min(100),
            muted: false,
            viz: Viz::new(),
            viz_mode: VizMode::Spectrum,
            viz_bands: 32,
            tap,
            engine: None,
            msg: None,
            conn_note: Some("connecting to Spotify...".to_string()),
            show_help: false,
            tick: 0,
            quit: false,
            cfg,
            connecting: false,
            want_play: false,
            skips: 0,
            ver_ctr: 0,
            tx,
            api,
            auth,
        };
        let v = app.next_ver();
        app.liked.reset(Source::Liked, v);
        let v = app.next_ver();
        app.saved_albums.reset(Source::SavedAlbums, v);
        let v = app.next_ver();
        app.playlists.reset(Source::MyPlaylists, v);
        app.apply_volume();
        app
    }

    fn next_ver(&mut self) -> u64 {
        self.ver_ctr += 1;
        self.ver_ctr
    }

    pub fn status(&mut self, s: impl Into<String>) {
        self.msg = Some((s.into(), Instant::now()));
    }

    // ---------- views ----------

    pub fn view(&self, t: Target) -> Option<&ListView> {
        match t {
            Target::Liked => Some(&self.liked),
            Target::SavedAlbums => Some(&self.saved_albums),
            Target::Playlists => Some(&self.playlists),
            Target::SearchTracks => Some(&self.s_tracks),
            Target::SearchAlbums => Some(&self.s_albums),
            Target::SearchPlaylists => Some(&self.s_playlists),
            Target::Detail => self.detail.as_ref().map(|d| &d.view),
        }
    }

    pub fn view_mut(&mut self, t: Target) -> Option<&mut ListView> {
        match t {
            Target::Liked => Some(&mut self.liked),
            Target::SavedAlbums => Some(&mut self.saved_albums),
            Target::Playlists => Some(&mut self.playlists),
            Target::SearchTracks => Some(&mut self.s_tracks),
            Target::SearchAlbums => Some(&mut self.s_albums),
            Target::SearchPlaylists => Some(&mut self.s_playlists),
            Target::Detail => self.detail.as_mut().map(|d| &mut d.view),
        }
    }

    pub fn current_target(&self) -> Option<Target> {
        if self.detail.is_some() {
            return Some(Target::Detail);
        }
        match self.tab {
            Tab::Queue => None,
            Tab::Library => Some(if self.lib_kind == Kind::Albums {
                Target::SavedAlbums
            } else {
                Target::Liked
            }),
            Tab::Playlists => Some(Target::Playlists),
            Tab::Search => Some(match self.search_kind {
                Kind::Tracks => Target::SearchTracks,
                Kind::Albums => Target::SearchAlbums,
                Kind::Playlists => Target::SearchPlaylists,
            }),
        }
    }

    fn request_page(&mut self, t: Target) {
        let api = self.api.clone();
        let tx = self.tx.clone();
        let Some(v) = self.view_mut(t) else { return };
        let Some(src) = v.source.clone() else { return };
        if v.loading {
            return;
        }
        v.loading = true;
        v.requested = true;
        let (offset, ver) = (v.next_offset, v.ver);
        tokio::spawn(async move {
            let res = api.fetch(&src, offset).await.map_err(|e| e.to_string());
            let _ = tx.send(Event::Page { target: t, ver, res });
        });
    }

    /// The Web API 403s on playlists you don't own / Spotify-made ones. Read them through the
    /// streaming connection instead (first 100 tracks).
    fn try_playlist_fallback(&mut self, ver: u64) -> bool {
        let session = match self.engine.as_ref() {
            Some(e) if e.alive() => e.session(),
            _ => return false,
        };
        let tx = self.tx.clone();
        let Some(d) = self.detail.as_mut() else { return false };
        if d.view.ver != ver || d.view.fallback_tried {
            return false;
        }
        let Some(Source::Playlist(id)) = d.view.source.clone() else { return false };
        d.view.fallback_tried = true;
        d.view.loading = true;
        tokio::spawn(async move {
            let res = crate::player::playlist_fallback(session, id, 100)
                .await
                .map(|tracks| {
                    let n = tracks.len() as u32;
                    PageData {
                        items: tracks.into_iter().map(Item::Track).collect(),
                        total: n,
                        raw: n,
                    }
                });
            let _ = tx.send(Event::Page { target: Target::Detail, ver, res });
        });
        self.status("reading playlist via streaming connection (first 100 tracks)");
        true
    }

    fn on_page(&mut self, t: Target, ver: u64, res: Result<PageData, String>) {
        if t == Target::Detail {
            if let Err(e) = &res {
                if e.contains("403") && self.try_playlist_fallback(ver) {
                    return;
                }
            }
        }
        let Some(v) = self.view_mut(t) else { return };
        if v.ver != ver {
            return; // stale response (new search / detail opened since)
        }
        v.loading = false;
        match res {
            Ok(p) => {
                v.total = p.total;
                v.next_offset += p.raw;
                if p.raw == 0 {
                    v.total = v.next_offset;
                }
                v.items.extend(p.items);
            }
            Err(e) => v.error = Some(e),
        }
    }

    // ---------- connection ----------

    pub fn connect_engine(&mut self) {
        if self.connecting {
            return;
        }
        self.connecting = true;
        self.conn_note = Some("connecting to Spotify...".to_string());
        let (tx, auth, tap) = (self.tx.clone(), self.auth.clone(), self.tap.clone());
        let (backend, bitrate) = (self.cfg.audio_backend.clone(), self.cfg.bitrate);
        tokio::spawn(async move {
            let res = async {
                let token = auth.access_token().await?;
                let tx2 = tx.clone();
                Engine::start(token, tap, backend, bitrate, move |m| {
                    let _ = tx2.send(Event::Player(m));
                })
                .await
            }
            .await;
            let _ = tx.send(Event::Engine(res));
        });
    }

    fn on_engine(&mut self, r: Result<Engine, String>) {
        self.connecting = false;
        match r {
            Ok(e) => {
                self.engine = Some(e);
                self.conn_note = None;
                if self.want_play {
                    self.want_play = false;
                    self.play_current();
                } else {
                    self.status("CONNECTED");
                }
            }
            Err(e) => {
                self.want_play = false;
                self.conn_note = Some(format!("NOT CONNECTED: {e}  (Shift+R to retry)"));
            }
        }
    }

    // ---------- events ----------

    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) => self.on_key(k),
            Event::Player(m) => self.on_player(m),
            Event::Engine(r) => self.on_engine(r),
            Event::Page { target, ver, res } => self.on_page(target, ver, res),
        }
    }

    pub fn on_tick(&mut self) {
        self.tick += 1;
        if let Some((_, t)) = &self.msg {
            if t.elapsed() > Duration::from_secs(4) {
                self.msg = None;
            }
        }
        let samples = if self.now.state == PState::Playing {
            self.tap.snapshot(FFT_N, Duration::from_millis(160))
        } else {
            None
        };
        self.viz.update(samples.as_deref(), self.viz_bands);

        if self.tick % 90 == 0 {
            let dead = self.engine.as_ref().map_or(false, |e| !e.alive());
            if dead {
                self.engine = None;
                self.status("connection lost, reconnecting...");
                self.connect_engine();
            }
        }

        if let Some(t) = self.current_target() {
            if self.view(t).map_or(false, |v| v.wants_more()) {
                self.request_page(t);
            }
        }
    }

    fn on_player(&mut self, m: PlayerMsg) {
        match m {
            PlayerMsg::Loading => self.now.set(PState::Loading, 0),
            PlayerMsg::Playing(p) => {
                self.skips = 0;
                self.conn_note = None;
                self.now.set(PState::Playing, p);
            }
            PlayerMsg::Paused(p) => self.now.set(PState::Paused, p),
            PlayerMsg::Stopped => self.now.set(PState::Stopped, 0),
            PlayerMsg::EndOfTrack => {
                if self.now.state != PState::Stopped {
                    self.advance(true);
                }
            }
            PlayerMsg::Unavailable => {
                self.skips += 1;
                let why = crate::logger::recent_problem()
                    .unwrap_or_else(|| "no details, see the log".to_string());
                if self.skips >= 3 {
                    // Stop instead of hammering Spotify by skipping through the whole queue.
                    self.now.set(PState::Stopped, 0);
                    self.conn_note = Some(format!("PLAYBACK FAILED: {why}"));
                } else {
                    self.status(format!("track unavailable, skipping: {why}"));
                    self.advance(true);
                }
            }
            PlayerMsg::Seeked(p) => {
                let st = self.now.state;
                self.now.set(st, p);
            }
            PlayerMsg::PreloadNext => {
                if let (Some(e), Some(t)) = (self.engine.as_ref(), self.queue.peek_next()) {
                    e.preload(&t.id);
                }
            }
        }
    }

    // ---------- playback ----------

    fn start_track(&mut self, t: Track) {
        self.now.track = Some(t.clone());
        self.now.set(PState::Loading, 0);
        let res = self
            .engine
            .as_ref()
            .filter(|e| e.alive())
            .map(|e| e.load(&t.id));
        match res {
            Some(Ok(())) => {}
            Some(Err(m)) => self.status(m),
            None => {
                self.want_play = true;
                self.engine = None;
                self.connect_engine();
                self.status("connecting to Spotify...");
            }
        }
    }

    fn play_current(&mut self) {
        if let Some(t) = self.queue.current().cloned() {
            self.start_track(t);
        }
    }

    fn advance(&mut self, auto: bool) {
        match self.queue.next(auto) {
            Some(t) => self.start_track(t),
            None => {
                self.now.set(PState::Stopped, 0);
                if let Some(e) = &self.engine {
                    e.stop();
                }
                self.status("END OF QUEUE");
            }
        }
    }

    fn toggle_pause(&mut self) {
        match self.now.state {
            PState::Playing => {
                let p = self.now.pos_ms();
                self.now.set(PState::Paused, p);
                if let Some(e) = &self.engine {
                    e.pause();
                }
            }
            PState::Paused => {
                let p = self.now.pos_ms();
                self.now.set(PState::Playing, p);
                if let Some(e) = &self.engine {
                    e.play();
                }
            }
            PState::Loading => {}
            PState::Stopped => {
                if self.queue.current().is_some() {
                    self.play_current();
                } else {
                    self.status("QUEUE EMPTY: pick something with ENTER");
                }
            }
        }
    }

    fn skip_prev(&mut self) {
        if self.now.state != PState::Stopped && self.now.pos_ms() > 3000 {
            if let Some(e) = &self.engine {
                e.seek(0);
            }
            let st = self.now.state;
            self.now.set(st, 0);
            return;
        }
        if let Some(t) = self.queue.prev() {
            self.start_track(t);
        }
    }

    fn seek_rel(&mut self, delta: i64) {
        let dur = match &self.now.track {
            Some(t) => t.duration_ms as i64,
            None => return,
        };
        if !matches!(self.now.state, PState::Playing | PState::Paused) {
            return;
        }
        let target = (self.now.pos_ms() as i64 + delta).clamp(0, (dur - 500).max(0)) as u32;
        if let Some(e) = &self.engine {
            e.seek(target);
        }
        let st = self.now.state;
        self.now.set(st, target);
    }

    fn apply_volume(&self) {
        self.tap.set_volume(if self.muted { 0 } else { self.volume });
    }

    fn change_volume(&mut self, d: i32) {
        self.muted = false;
        self.volume = (self.volume as i32 + d).clamp(0, 100) as u8;
        self.apply_volume();
        self.status(format!("VOLUME {}%", self.volume));
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.apply_volume();
        self.status(if self.muted { "MUTE" } else { "UNMUTE" });
    }

    fn change_sens(&mut self, d: f32) {
        self.viz.sens_db = (self.viz.sens_db + d).clamp(-30.0, 24.0);
        self.status(format!("VISUALIZER SENSITIVITY {:+.0} dB", self.viz.sens_db));
    }

    // ---------- navigation ----------

    fn set_tab(&mut self, t: Tab) {
        self.tab = t;
        self.detail = None;
        if t == Tab::Queue {
            self.qcur.sel = self.queue.pos().unwrap_or(0);
        }
        if let Some(tg) = self.current_target() {
            if let Some(v) = self.view_mut(tg) {
                if v.error.is_some() {
                    v.error = None;
                    v.requested = false; // retry on visit
                }
            }
        }
    }

    fn cycle_tab(&mut self, d: isize) {
        let i = TABS.iter().position(|t| *t == self.tab).unwrap_or(0) as isize;
        self.set_tab(TABS[(i + d).rem_euclid(4) as usize]);
    }

    fn cycle_kind(&mut self, d: i32) {
        if self.detail.is_some() {
            return;
        }
        match self.tab {
            Tab::Library => {
                self.lib_kind = if self.lib_kind == Kind::Tracks {
                    Kind::Albums
                } else {
                    Kind::Tracks
                }
            }
            Tab::Search => {
                let ks = [Kind::Tracks, Kind::Albums, Kind::Playlists];
                let i = ks.iter().position(|k| *k == self.search_kind).unwrap_or(0) as i32;
                self.search_kind = ks[(i + d).rem_euclid(3) as usize];
            }
            _ => {}
        }
    }

    fn move_sel(&mut self, d: isize) {
        if self.tab == Tab::Queue && self.detail.is_none() {
            let len = self.queue.len();
            self.qcur.move_by(len, d);
            return;
        }
        if let Some(t) = self.current_target() {
            if let Some(v) = self.view_mut(t) {
                let len = v.items.len();
                v.cur.move_by(len, d);
            }
        }
    }

    fn open_detail(&mut self, title: String, src: Source) {
        let ver = self.next_ver();
        let mut view = ListView::new();
        view.reset(src, ver);
        self.detail = Some(Detail { title, view });
    }

    fn activate(&mut self) {
        if self.tab == Tab::Queue && self.detail.is_none() {
            let p = self.qcur.sel;
            if let Some(t) = self.queue.jump(p) {
                self.start_track(t);
            }
            return;
        }
        let Some(target) = self.current_target() else { return };
        let Some(v) = self.view(target) else { return };
        let Some(item) = v.selected().cloned() else { return };
        match item {
            Item::Track(_) => {
                let sel = v.cur.sel;
                let tracks: Vec<Track> = v
                    .items
                    .iter()
                    .filter_map(|i| if let Item::Track(t) = i { Some(t.clone()) } else { None })
                    .collect();
                let start = v.items[..sel]
                    .iter()
                    .filter(|i| matches!(i, Item::Track(_)))
                    .count();
                self.queue.set(tracks, start);
                self.qcur.sel = self.queue.pos().unwrap_or(0);
                self.play_current();
            }
            Item::Album(a) => self.open_detail(
                a.name.clone(),
                Source::Album {
                    id: a.id,
                    name: a.name,
                },
            ),
            Item::Playlist(p) => self.open_detail(p.name.clone(), Source::Playlist(p.id)),
        }
    }

    fn add_to_queue(&mut self) {
        let Some(t) = self.current_target() else { return };
        let picked = self.view(t).and_then(|v| v.selected()).cloned();
        match picked {
            Some(Item::Track(tr)) => {
                let name = tr.name.clone();
                self.queue.push(tr);
                self.status(format!("QUEUED: {name}"));
            }
            _ => self.status("select a track to queue it"),
        }
    }

    fn run_search(&mut self, q: String) {
        self.search_q = q.clone();
        self.detail = None;
        let (v1, v2, v3) = (self.next_ver(), self.next_ver(), self.next_ver());
        self.s_tracks.reset(Source::Search { q: q.clone(), kind: Kind::Tracks }, v1);
        self.s_albums.reset(Source::Search { q: q.clone(), kind: Kind::Albums }, v2);
        self.s_playlists.reset(Source::Search { q, kind: Kind::Playlists }, v3);
        self.request_page(Target::SearchTracks);
        self.request_page(Target::SearchAlbums);
        self.request_page(Target::SearchPlaylists);
    }

    // ---------- keys ----------

    fn on_key_input(&mut self, k: KeyEvent) {
        let Some(buf) = self.search_input.as_mut() else { return };
        match k.code {
            KeyCode::Esc => self.search_input = None,
            KeyCode::Enter => {
                let q = buf.trim().to_string();
                self.search_input = None;
                if !q.is_empty() {
                    self.run_search(q);
                }
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => buf.clear(),
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => buf.push(c),
            _ => {}
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.search_input.is_some() {
            self.on_key_input(k);
            return;
        }
        if self.show_help {
            self.show_help = false;
            return;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) {
            return;
        }
        const FAR: isize = 1_000_000_000;
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char(' ') => self.toggle_pause(),
            KeyCode::Char('n') => {
                if let Some(t) = self.queue.next(false) {
                    self.start_track(t);
                } else {
                    self.status("END OF QUEUE");
                }
            }
            KeyCode::Char('p') => self.skip_prev(),
            KeyCode::Char('+') | KeyCode::Char('=') => self.change_volume(5),
            KeyCode::Char('-') | KeyCode::Char('_') => self.change_volume(-5),
            KeyCode::Char('m') => self.toggle_mute(),
            KeyCode::Left => self.seek_rel(-5000),
            KeyCode::Right => self.seek_rel(5000),
            KeyCode::Char('1') => self.set_tab(Tab::Queue),
            KeyCode::Char('2') => self.set_tab(Tab::Library),
            KeyCode::Char('3') => self.set_tab(Tab::Playlists),
            KeyCode::Char('4') => self.set_tab(Tab::Search),
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('/') => {
                self.set_tab(Tab::Search);
                self.search_input = Some(String::new());
            }
            KeyCode::Char('s') => {
                self.queue.toggle_shuffle();
                let on = self.queue.shuffle;
                self.status(if on { "SHUFFLE ON" } else { "SHUFFLE OFF" });
            }
            KeyCode::Char('r') => {
                self.queue.cycle_repeat();
                use crate::queue::Repeat;
                let s = match self.queue.repeat {
                    Repeat::Off => "REPEAT OFF",
                    Repeat::All => "REPEAT ALL",
                    Repeat::One => "REPEAT ONE",
                };
                self.status(s);
            }
            KeyCode::Char('t') => self.theme = (self.theme + 1) % THEMES.len(),
            KeyCode::Char('<') => self.change_sens(-3.0),
            KeyCode::Char('>') => self.change_sens(3.0),
            KeyCode::Char('v') => {
                self.viz_mode = if self.viz_mode == VizMode::Spectrum {
                    VizMode::Mirror
                } else {
                    VizMode::Spectrum
                }
            }
            KeyCode::Char('[') => self.cycle_kind(-1),
            KeyCode::Char(']') => self.cycle_kind(1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::Home | KeyCode::Char('g') => self.move_sel(-FAR),
            KeyCode::End | KeyCode::Char('G') => self.move_sel(FAR),
            KeyCode::Enter => self.activate(),
            KeyCode::Esc | KeyCode::Backspace => {
                if self.detail.is_some() {
                    self.detail = None;
                }
            }
            KeyCode::Char('a') => self.add_to_queue(),
            KeyCode::Char('d') | KeyCode::Delete => {
                if self.tab == Tab::Queue && self.detail.is_none() {
                    if !self.queue.remove(self.qcur.sel) {
                        self.status("can't remove the playing track");
                    }
                }
            }
            KeyCode::Char('C') => {
                self.queue.clear();
                self.qcur = Cursor::default();
                self.status("QUEUE CLEARED");
            }
            KeyCode::Char('R') => {
                self.engine = None;
                self.connect_engine();
            }
            KeyCode::Char('?') => self.show_help = true,
            _ => {}
        }
    }
}
