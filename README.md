# VFD·TUNE

A retro VFD-style Spotify player for the terminal, written in Rust. Streams audio itself via
[librespot](https://github.com/librespot-org/librespot) (no Spotify app or `spotifyd` needed).
**Spotify Premium required.**

- Phosphor-teal VFD look with ghost "unlit" segments, 7-segment clock, scrolling marquee title
- Real spectrum visualizer (FFT of the actual decoded audio), plus a mirrored variant
- Now playing, queue, library (liked songs + saved albums), playlists, search (tracks/albums/playlists)
- Auto-loads more results as you scroll, gapless preloading, shuffle, repeat, seek, volume
- 9 colour themes (teal / amber / green / 80s / sunset / miami / deco / cyber / autumn): `t` cycles, choice is remembered; colours are plain RGB values at the top of `src/ui.rs`

## Build (Arch Linux)

```sh
sudo pacman -S --needed rust base-devel pkgconf openssl libpulse   # libpulse: PipeWire/Pulse backend
tar xzf vfdtune.tar.gz && cd vfdtune
cargo build --release
./target/release/vfdtune            # or: cargo install --path .
```

Other audio backends: `cargo build --release --no-default-features --features alsa`
(needs `alsa-lib`) or `--features rodio`. Use a terminal with truecolor (kitty, alacritty, foot,
wezterm, ...). `echo $COLORTERM` should print `truecolor`.

If the build fails inside `librespot-core` with `Build: Add is not satisfied`, that's the vergen
version clash this project already pins around (`build.rs`). On an older checkout run:
`cargo update -p vergen --precise 9.0.6`

## One-time setup: your own Spotify app

Spotify throttles the client ID built into librespot (it is shared by every librespot-based app),
which shows up as endless "rate limited" errors. So VFD·TUNE uses **your own** app for browsing and
search, and the built-in ID only for audio streaming.

1. Go to <https://developer.spotify.com/dashboard> and create an app (any name).
2. Redirect URI: `http://127.0.0.1:8898/login` (exactly), then Save. API to use: **Web API**.
3. Copy the app's **Client ID** into `~/.config/vfdtune/config.json` as `"client_id"`
   (or run with `VFDTUNE_CLIENT_ID=...`). Running `vfdtune` once creates that file.

## First run

Two browser logins happen once (browse/search, then streaming). Each prints a URL and opens your
browser; a tiny local listener on `127.0.0.1:8898` catches the redirect. Tokens are cached in
`~/.config/vfdtune/token_web.json` and `token_stream.json` (mode 0600); later runs are silent.
(An old `token.json` from the first version is unused and can be deleted.)

## Keys (press `?` in the app)

| Key | Action |
|---|---|
| `Space` | play / pause |
| `n` / `p` | next / previous (`p` restarts the track after 3 s) |
| `←` / `→` | seek -5 s / +5 s |
| `+` `-` / `m` | volume / mute |
| `/` | search (`Enter` runs, `Esc` cancels) |
| `[` `]` | switch Songs/Albums (library) or Tracks/Albums/Playlists (search) |
| `1`-`4`, `Tab` | queue, library, playlists, search |
| `j` `k` `↑` `↓`, `g` `G`, PgUp/PgDn | move |
| `Enter` | play track (rest of the list is queued) / open album or playlist |
| `Esc` / `Backspace` | back out of an album or playlist |
| `a` | add track to queue |
| `d` / `C` | remove from queue / clear queue |
| `s` / `r` | shuffle / repeat (off, all, one) |
| `v` / `t` | visualizer style / colour theme |
| `<` / `>` | visualizer sensitivity down / up |
| `R` (Shift+r) | reconnect to Spotify |
| `q` | quit |

## Config: `~/.config/vfdtune/config.json`

```json
{ "client_id": null, "audio_backend": null, "bitrate": 320, "search_limit": 10, "volume": 60, "theme": 0 }
```

`client_id` is the Client ID of your own Spotify app (see setup above). Spotify's development-mode
rules apply to it: Premium app owner (you), up to 5 users, search results capped at 10 per request.

## Troubleshooting

Playback errors show in the bottom line. The full log (librespot included) is rewritten on every
start at `~/.cache/vfdtune/vfdtune.log`:
`grep -E "ERROR|WARN" ~/.cache/vfdtune/vfdtune.log | tail -20`

## Notes and known limits

- Playlists you don't own (or Spotify-made ones) 403 on the Web API; VFD·TUNE then reads them
  through the streaming connection (first 100 tracks, slower, experimental).
- Spotify's Feb/Mar 2026 Web API changes are handled (`/playlists/{id}/items`, `item` vs `track`
  rows, search limit 10), but Spotify-owned playlists (Discover Weekly etc.) are not readable
  through the API and show as empty.
- Queue snapshot: pressing Enter queues the tracks *loaded so far* in that list.
- librespot is an unofficial client. Use at your own discretion with respect to Spotify's terms.
- Status/errors show in the bottom line. Logs from librespot are not shown.
