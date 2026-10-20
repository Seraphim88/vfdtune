# vfdtune: project handoff

Terminal Spotify player in Rust for Arch Linux (Spotify Premium). Retro VFD look, FFT visualizer,
search, library, playlists, queue, keyboard controls, streams audio itself via librespot 0.8.0.
ratatui 0.29 UI. Read README.md for keys/config. Build: `cargo build --release`.

## Current bug (start here)
Every track fails to load: the app showed "nothing playable in the queue" (now: "PLAYBACK FAILED:
<reason>" in the footer after 3 consecutive failures). Cause unknown. Next step:
`grep -E "ERROR|WARN" ~/.cache/vfdtune/vfdtune.log | tail -30` (log is rewritten each start).
Likely suspects: librespot "Unable to load audio item" (spclient 403/500/429), audio-key errors,
or the streaming token/session. Login for streaming uses librespot's built-in client id with
scope "streaming" (same as librespot's examples/play.rs).

## Architecture (src/)
- main.rs: two logins, event loop (tokio select: 33ms tick + channel), keyboard thread.
- auth.rs: OAuth PKCE with local listener on 127.0.0.1:8898/login. Two instances:
  "web" (user's OWN Spotify app client id from ~/.config/vfdtune/config.json) for the Web API,
  "stream" (librespot built-in id, scope "streaming") for the librespot session.
  Why: the built-in id is shared by all librespot users and gets 429-throttled on the Web API.
- api.rs: Web API client parsing serde_json::Value leniently (Spotify's Feb/Mar 2026 changes:
  /playlists/{id}/items, row key `item` vs `track`, search limit max 10). 403 on Spotify-made playlists.
- player.rs: librespot Session + Player. TapSink wraps the real audio sink: copies PCM for the
  visualizer (pre-volume) and applies software volume. Events mapped to PlayerMsg; stale
  play_request_ids ignored; Unavailable only honoured for the track we loaded (preload failures
  also fire Unavailable with the current request id).
- queue.rs: queue with shuffle (permutation of indices) and repeat. viz.rs: Hann + radix-2 FFT.
- app.rs: state, key handling, lazy paged loading. ui.rs: VFD rendering (7-seg clock w/ ghost segments).
- logger.rs: file logger + last warning/error for the UI.

## Notes
- Cargo.toml pins build-dep `vergen = "=9.0.6"` (+ empty build.rs): librespot-core 0.8.0 fails to
  build with vergen 9.1.0 otherwise.
- librespot 0.8 API used: Session::new(SessionConfig, None) + connect(Credentials::with_access_token, false);
  Player::new(cfg, session, Box::new(NoOpVolume), sink_builder); player.load(SpotifyUri::Track{id}, true, 0).
- Audio backend feature `pulse` (PipeWire via pipewire-pulse) is the default.
- Secrets live in ~/.config/vfdtune/token_*.json (0600). Never commit them.

## Added later
- Web API 403 on non-owned playlists -> app.rs try_playlist_fallback() -> player.rs playlist_fallback()
  (librespot_metadata Playlist/Track, first 100). Untested; check ~/.cache/vfdtune/vfdtune.log.
- Visualizer: GAMMA/DB_FLOOR/DB_RANGE in viz.rs, live sensitivity with < and >.
- Themes in ui.rs (9). accent/warm/hot give the multi-colour looks.
