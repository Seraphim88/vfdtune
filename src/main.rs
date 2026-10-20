mod api;
mod app;
mod auth;
mod config;
mod logger;
mod player;
mod queue;
mod ui;
mod viz;

use app::{App, Event};
use ratatui::crossterm::event::{self, Event as CtEvent, KeyEventKind};
use std::time::Duration;
use tokio::sync::mpsc;

const WEB_SCOPES: &str =
    "user-read-private user-library-read playlist-read-private playlist-read-collaborative";

#[tokio::main]
async fn main() {
    logger::init();
    let mut cfg = config::Config::load();

    // Two logins, two client ids:
    //  - Web API (browse/search): YOUR Spotify app. The built-in librespot id is shared by every
    //    librespot user and gets throttled (429, sometimes for 24h), so don't use it for the API.
    //  - Streaming (librespot session): the built-in id, used only for audio.
    let web_id = std::env::var("VFDTUNE_CLIENT_ID")
        .ok()
        .or_else(|| cfg.client_id.clone())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(web_id) = web_id else {
        cfg.save();
        eprintln!(
            "vfdtune needs your own Spotify app Client ID (one-time, ~2 minutes):\n\n\
             1. Go to https://developer.spotify.com/dashboard and create an app\n\
             2. Redirect URI: http://127.0.0.1:8898/login   (exactly, then Save)\n\
             3. API to use: Web API\n\
             4. Copy the app's Client ID into \"client_id\" in\n   {}\n   (or run with VFDTUNE_CLIENT_ID=...)\n",
            config::dir().join("config.json").display()
        );
        std::process::exit(1);
    };

    // Logins happen before the TUI starts (prints a URL, opens your browser).
    let web_auth = match auth::Auth::init("web", "[1/2 browse & search]", web_id, WEB_SCOPES).await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("vfdtune: web login failed: {e}");
            std::process::exit(1);
        }
    };
    let stream_id = librespot_core::config::SessionConfig::default().client_id;
    let stream_auth =
        match auth::Auth::init("stream", "[2/2 streaming]", stream_id, "streaming").await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("vfdtune: streaming login failed: {e}");
                std::process::exit(1);
            }
        };

    let api = api::Api::new(web_auth, cfg.search_limit);
    let tap = player::Tap::new(cfg.volume);
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    let mut app = App::new(cfg.clone(), stream_auth, api, tx.clone(), tap);
    app.connect_engine();

    // Keyboard thread -> channel
    let txk = tx.clone();
    std::thread::spawn(move || loop {
        match event::poll(Duration::from_millis(250)) {
            Ok(true) => match event::read() {
                Ok(CtEvent::Key(k)) if k.kind != KeyEventKind::Release => {
                    if txk.send(Event::Key(k)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            },
            Ok(false) => {
                if txk.is_closed() {
                    break;
                }
            }
            Err(_) => break,
        }
    });

    let mut terminal = ratatui::init();
    let mut ticker = tokio::time::interval(Duration::from_millis(33));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    while !app.quit {
        tokio::select! {
            _ = ticker.tick() => app.on_tick(),
            Some(ev) = rx.recv() => app.on_event(ev),
        }
        while let Ok(ev) = rx.try_recv() {
            app.on_event(ev);
        }
        if terminal.draw(|f| ui::draw(f, &mut app)).is_err() {
            break;
        }
    }

    ratatui::restore();
    cfg.volume = app.volume;
    cfg.theme = app.theme;
    cfg.save();
    // Skip waiting on the audio thread; the OS cleans up.
    std::process::exit(0);
}
