//! Retro VFD look: near-black faceplate, phosphor-teal segments, dim "unlit" ghost segments
//! behind every lit one (7-segment clock, level meters, progress bar).

use crate::api::{Item, Kind};
use crate::app::{App, ListView, PState, Tab, Target, VizMode};
use crate::queue::Repeat;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    /// main text / lit segments
    pub lit: Color,
    /// brightest highlight (peak-hold marks)
    pub peak: Color,
    pub dim: Color,
    /// unlit "ghost" segments
    pub ghost: Color,
    /// clock digits, progress bar, volume meter, selection bar
    pub accent: Color,
    /// visualizer: middle of each bar
    pub warm: Color,
    /// visualizer: top of each bar
    pub hot: Color,
}

// Mono themes use the same colour for lit/accent/warm/hot. Edit the RGB values to taste.
pub const THEMES: [Theme; 9] = [
    Theme {
        name: "TEAL",
        bg: Color::Rgb(3, 10, 10),
        lit: Color::Rgb(110, 255, 220),
        peak: Color::Rgb(225, 255, 250),
        dim: Color::Rgb(48, 150, 134),
        ghost: Color::Rgb(13, 50, 46),
        accent: Color::Rgb(110, 255, 220),
        warm: Color::Rgb(110, 255, 220),
        hot: Color::Rgb(110, 255, 220),
    },
    Theme {
        name: "AMBER",
        bg: Color::Rgb(10, 6, 2),
        lit: Color::Rgb(255, 176, 40),
        peak: Color::Rgb(255, 232, 160),
        dim: Color::Rgb(165, 104, 22),
        ghost: Color::Rgb(54, 33, 6),
        accent: Color::Rgb(255, 176, 40),
        warm: Color::Rgb(255, 176, 40),
        hot: Color::Rgb(255, 176, 40),
    },
    Theme {
        name: "GREEN",
        bg: Color::Rgb(2, 10, 4),
        lit: Color::Rgb(100, 255, 130),
        peak: Color::Rgb(210, 255, 220),
        dim: Color::Rgb(42, 150, 66),
        ghost: Color::Rgb(10, 48, 20),
        accent: Color::Rgb(100, 255, 130),
        warm: Color::Rgb(100, 255, 130),
        hot: Color::Rgb(100, 255, 130),
    },
    // 80s hi-fi: cyan text, orange clock/progress/selection, LED bars cyan -> orange -> red
    Theme {
        name: "80S",
        bg: Color::Rgb(8, 5, 14),
        lit: Color::Rgb(90, 235, 255),
        peak: Color::Rgb(255, 236, 214),
        dim: Color::Rgb(70, 140, 165),
        ghost: Color::Rgb(30, 22, 44),
        accent: Color::Rgb(255, 140, 40),
        warm: Color::Rgb(255, 150, 50),
        hot: Color::Rgb(255, 60, 70),
    },
    // orange text, red clock/selection, bars orange -> amber -> hot pink
    Theme {
        name: "SUNSET",
        bg: Color::Rgb(12, 4, 6),
        lit: Color::Rgb(255, 128, 48),
        peak: Color::Rgb(255, 232, 190),
        dim: Color::Rgb(176, 84, 50),
        ghost: Color::Rgb(58, 22, 22),
        accent: Color::Rgb(255, 72, 72),
        warm: Color::Rgb(255, 176, 56),
        hot: Color::Rgb(255, 60, 120),
    },
    // Miami Vice: turquoise text on deep indigo, hot-pink clock/selection, bars turquoise -> violet -> pink
    Theme {
        name: "MIAMI",
        bg: Color::Rgb(10, 6, 24),
        lit: Color::Rgb(0, 229, 208),
        peak: Color::Rgb(255, 240, 250),
        dim: Color::Rgb(88, 150, 176),
        ghost: Color::Rgb(40, 22, 74),
        accent: Color::Rgb(255, 46, 147),
        warm: Color::Rgb(176, 96, 255),
        hot: Color::Rgb(255, 80, 170),
    },
    // South Beach art deco: peach text on deep teal, aqua clock/selection, bars peach -> pink -> sunshine
    Theme {
        name: "DECO",
        bg: Color::Rgb(6, 20, 28),
        lit: Color::Rgb(255, 168, 140),
        peak: Color::Rgb(255, 246, 232),
        dim: Color::Rgb(186, 112, 120),
        ghost: Color::Rgb(22, 50, 62),
        accent: Color::Rgb(0, 232, 200),
        warm: Color::Rgb(255, 108, 170),
        hot: Color::Rgb(255, 226, 110),
    },
    // Cyberpunk: neon-yellow text on near-black, cyan clock/selection, bars yellow -> orange -> neon red
    Theme {
        name: "CYBER",
        bg: Color::Rgb(7, 5, 12),
        lit: Color::Rgb(250, 235, 40),
        peak: Color::Rgb(255, 255, 205),
        dim: Color::Rgb(160, 148, 40),
        ghost: Color::Rgb(38, 20, 44),
        accent: Color::Rgb(0, 240, 255),
        warm: Color::Rgb(255, 110, 40),
        hot: Color::Rgb(255, 20, 110),
    },
    // Autumn: burnt-orange text on aubergine, purple clock/selection, bars orange -> brick red -> violet
    Theme {
        name: "AUTUMN",
        bg: Color::Rgb(14, 8, 16),
        lit: Color::Rgb(205, 95, 35),
        peak: Color::Rgb(255, 214, 160),
        dim: Color::Rgb(150, 82, 72),
        ghost: Color::Rgb(46, 24, 44),
        accent: Color::Rgb(186, 120, 235),
        warm: Color::Rgb(200, 64, 56),
        hot: Color::Rgb(186, 96, 220),
    },
];

fn st(c: Color) -> Style {
    Style::new().fg(c)
}

fn sel_style(th: &Theme) -> Style {
    Style::new().fg(th.bg).bg(th.accent).add_modifier(Modifier::BOLD)
}

fn spinner(tick: u64) -> &'static str {
    const F: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    F[(tick / 3) as usize % 10]
}

fn fit(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        format!("{s}{}", " ".repeat(w - n))
    } else if w == 0 {
        String::new()
    } else {
        let mut t: String = s.chars().take(w - 1).collect();
        t.push('…');
        t
    }
}

fn marquee(text: &str, width: usize, tick: u64) -> String {
    let chars: Vec<char> = text.chars().collect();
    if width == 0 || chars.len() <= width {
        return text.to_string();
    }
    let mut doubled = chars.clone();
    doubled.extend("   ◆   ".chars());
    let cycle = doubled.len();
    doubled.extend(chars.iter());
    let start = ((tick / 4) as usize) % cycle;
    doubled[start..start + width].iter().collect()
}

fn mmss(ms: u32) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", (s / 60).min(99), s % 60)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let th = THEMES[app.theme % THEMES.len()];
    let area = f.area();
    f.render_widget(Block::default().style(Style::new().bg(th.bg).fg(th.lit)), area);
    if area.width < 60 || area.height < 20 {
        f.render_widget(
            Paragraph::new(Line::styled("TERMINAL TOO SMALL (need 60x20)", st(th.lit))),
            area,
        );
        return;
    }
    let viz_h = ((area.height as usize) / 3).clamp(6, 12) as u16;
    let [head, now, viz, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(6),
        Constraint::Length(viz_h),
        Constraint::Min(6),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(f, app, head, &th);
    draw_now(f, app, now, &th);
    draw_viz(f, app, viz, &th);
    draw_body(f, app, body, &th);
    draw_footer(f, app, foot, &th);
    if app.show_help {
        draw_help(f, area, &th);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let left = Line::from(vec![
        Span::styled(" VFD·TUNE ", Style::new().fg(th.peak).add_modifier(Modifier::BOLD)),
        Span::styled(format!("[{}]", th.name), st(th.ghost)),
    ]);
    f.render_widget(Paragraph::new(left), area);

    let (rl, rpt_on) = match app.queue.repeat {
        Repeat::Off => ("RPT ", false),
        Repeat::All => ("RPT·ALL ", true),
        Repeat::One => ("RPT·ONE ", true),
    };
    let vol = if app.muted { 0 } else { app.volume };
    let n = (vol as usize + 5) / 10;
    let right = Line::from(vec![
        Span::styled("SHUF ", st(if app.queue.shuffle { th.lit } else { th.ghost })),
        Span::styled(rl, st(if rpt_on { th.lit } else { th.ghost })),
        Span::styled("VOL ", st(th.dim)),
        Span::styled("▮".repeat(n), st(th.accent)),
        Span::styled("▯".repeat(10 - n), st(th.ghost)),
        Span::styled(
            if app.muted {
                " MUTE ".to_string()
            } else {
                format!(" {:>3}% ", app.volume)
            },
            st(th.lit),
        ),
    ]);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
}

// 7-segment masks: bit0=a(top) 1=b 2=c 3=d(bottom) 4=e 5=f 6=g(middle)
const SEG: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];

fn seg(on: bool, ch: &'static str, th: &Theme) -> Span<'static> {
    Span::styled(ch, st(if on { th.accent } else { th.ghost }))
}

fn digit(d: u32, th: &Theme) -> [Vec<Span<'static>>; 3] {
    let m = SEG[(d % 10) as usize];
    let on = |bit: u8| m & (1 << bit) != 0;
    [
        vec![Span::raw(" "), seg(on(0), "_", th), Span::raw(" ")],
        vec![seg(on(5), "|", th), seg(on(6), "_", th), seg(on(1), "|", th)],
        vec![seg(on(4), "|", th), seg(on(3), "_", th), seg(on(2), "|", th)],
    ]
}

fn draw_now(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let block = Block::bordered()
        .border_style(st(th.dim))
        .title(Span::styled(" NOW PLAYING ", st(th.lit)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 4 || inner.width < 30 {
        return;
    }
    let [left, right] =
        Layout::horizontal([Constraint::Length(21), Constraint::Min(8)]).areas(inner);

    let pos = app.now.pos_ms();
    let dur = app.now.track.as_ref().map(|t| t.duration_ms).unwrap_or(0);

    // big 7-segment clock with ghost segments
    let (m, s) = ((pos / 60_000).min(99), (pos / 1000) % 60);
    let digs = [m / 10, m % 10, s / 10, s % 10];
    let colon_lit = app.now.state != PState::Playing || (app.tick / 15) % 2 == 0;
    let cst = st(if colon_lit { th.accent } else { th.ghost });
    let mut rows: [Vec<Span<'static>>; 3] = [vec![], vec![], vec![]];
    for (i, d) in digs.iter().enumerate() {
        if i > 0 {
            for r in rows.iter_mut() {
                r.push(Span::raw(" "));
            }
        }
        if i == 2 {
            rows[0].push(Span::raw(" "));
            rows[1].push(Span::styled("·", cst));
            rows[2].push(Span::styled("·", cst));
            for r in rows.iter_mut() {
                r.push(Span::raw(" "));
            }
        }
        for (r, spans) in rows.iter_mut().zip(digit(*d, th)) {
            r.extend(spans);
        }
    }
    let mut lines: Vec<Line> = rows.into_iter().map(Line::from).collect();
    let state_txt = match app.now.state {
        PState::Playing => "▶ PLAY",
        PState::Paused => "❚❚ PAUSE",
        PState::Stopped => "■ STOP",
        PState::Loading => "◌ LOAD",
    };
    lines.push(Line::from(Span::styled(format!(" {state_txt}"), st(th.lit))));
    f.render_widget(Paragraph::new(lines), left);

    // title / artist / album / progress
    let w = right.width as usize;
    let (title, artist, album) = match &app.now.track {
        Some(t) => (t.name.clone(), t.artists.clone(), t.album.clone()),
        None => (
            "— NO TRACK —".to_string(),
            "press / to search, 2 for your library, ? for help".to_string(),
            String::new(),
        ),
    };
    let l1 = Line::from(Span::styled(
        marquee(&title, w, app.tick),
        Style::new().fg(th.peak).add_modifier(Modifier::BOLD),
    ));
    let l2 = Line::from(Span::styled(marquee(&artist, w, app.tick + 7), st(th.lit)));
    let l3 = Line::from(Span::styled(marquee(&album, w, app.tick + 13), st(th.dim)));

    let total = format!(" {}", mmss(dur));
    let bar_w = w.saturating_sub(total.chars().count());
    let mut filled = 0usize;
    if dur > 0 && bar_w > 0 {
        filled = ((pos as u64 * bar_w as u64) / dur as u64) as usize;
    }
    filled = filled.min(bar_w);
    let mut spans = vec![Span::styled("━".repeat(filled), st(th.accent))];
    if bar_w > filled {
        spans.push(Span::styled("●", st(th.peak)));
        spans.push(Span::styled("─".repeat(bar_w - filled - 1), st(th.ghost)));
    }
    spans.push(Span::styled(total, st(th.dim)));
    f.render_widget(Paragraph::new(vec![l1, l2, l3, Line::from(spans)]), right);
}

/// (segment index from the base/centre, segments per half) for text row `r`.
fn seg_pos(mirror: bool, r: usize, h: usize) -> Option<(usize, usize)> {
    if !mirror {
        return Some((h - 1 - r, h));
    }
    let half = h / 2;
    if half == 0 {
        None
    } else if r < half {
        Some((half - 1 - r, half))
    } else if r < half * 2 {
        Some((r - half, half))
    } else {
        None
    }
}

fn draw_viz(f: &mut Frame, app: &mut App, area: Rect, th: &Theme) {
    let mirror = app.viz_mode == VizMode::Mirror;
    let block = Block::bordered().border_style(st(th.dim)).title(Span::styled(
        if mirror { " MIRROR " } else { " SPECTRUM " },
        st(th.lit),
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let (w, h) = (inner.width as usize, inner.height as usize);
    if w < 3 || h == 0 {
        return;
    }
    let (bar_w, gap) = (2usize, 1usize);
    let bands = ((w + gap) / (bar_w + gap)).max(1);
    app.viz_bands = bands;
    let used = bands * (bar_w + gap) - gap;
    let pad = (w - used) / 2;

    let mut lines: Vec<Line> = Vec::with_capacity(h);
    for r in 0..h {
        let Some((k, cells)) = seg_pos(mirror, r, h) else {
            lines.push(Line::from(""));
            continue;
        };
        let mut spans = vec![Span::raw(" ".repeat(pad))];
        for b in 0..bands {
            let lv = app.viz.levels.get(b).copied().unwrap_or(0.0);
            let pk = app.viz.peaks.get(b).copied().unwrap_or(0.0);
            let filled = (lv * cells as f32).round() as usize;
            let peak_k = (pk * cells as f32).ceil() as usize;
            let frac = (k + 1) as f32 / cells as f32;
            let color = if k < filled {
                if frac > 0.82 {
                    th.hot
                } else if frac > 0.55 {
                    th.warm
                } else {
                    th.lit
                }
            } else if peak_k > 0 && k + 1 == peak_k {
                th.peak
            } else {
                th.ghost
            };
            spans.push(Span::styled("━━ ", st(color)));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn view_status(v: &ListView, tick: u64) -> String {
    if let Some(e) = &v.error {
        return format!("ERR {e}");
    }
    if v.loading {
        return format!("{} LOADING", spinner(tick));
    }
    format!("{}/{}", v.items.len(), v.total.max(v.items.len() as u32))
}

fn kind_spans(labels: &[(&str, bool)], th: &Theme) -> Vec<Span<'static>> {
    labels
        .iter()
        .map(|(l, on)| {
            Span::styled(
                format!(" {l} "),
                if *on { sel_style(th) } else { st(th.dim) },
            )
        })
        .collect()
}

fn sub_line(app: &App, t: Target, th: &Theme) -> (Vec<Span<'static>>, String) {
    let status = app
        .view(t)
        .map(|v| view_status(v, app.tick))
        .unwrap_or_default();
    let left = if let Some(d) = &app.detail {
        vec![
            Span::styled(" ◂ ESC ", st(th.dim)),
            Span::styled(d.title.clone(), st(th.lit)),
        ]
    } else {
        match app.tab {
            Tab::Library => kind_spans(
                &[
                    ("SONGS", app.lib_kind == Kind::Tracks),
                    ("ALBUMS", app.lib_kind == Kind::Albums),
                ],
                th,
            ),
            Tab::Playlists => vec![Span::styled(" YOUR PLAYLISTS", st(th.dim))],
            Tab::Search => {
                if let Some(q) = &app.search_input {
                    let blink = if (app.tick / 10) % 2 == 0 { "█" } else { " " };
                    vec![Span::styled(format!(" / {q}{blink}"), st(th.peak))]
                } else {
                    let mut s = kind_spans(
                        &[
                            ("TRACKS", app.search_kind == Kind::Tracks),
                            ("ALBUMS", app.search_kind == Kind::Albums),
                            ("PLAYLISTS", app.search_kind == Kind::Playlists),
                        ],
                        th,
                    );
                    let hint = if app.search_q.is_empty() {
                        "  press / to search".to_string()
                    } else {
                        format!("  “{}”", app.search_q)
                    };
                    s.push(Span::styled(hint, st(th.dim)));
                    s
                }
            }
            Tab::Queue => vec![],
        }
    };
    (left, status)
}

fn empty_msg(app: &App, t: Target) -> &'static str {
    match t {
        Target::Liked => "no liked songs",
        Target::SavedAlbums => "no saved albums",
        Target::Playlists => "no playlists",
        Target::Detail => "empty (Spotify-made playlists can't be read by the API)",
        _ => {
            if app.search_q.is_empty() {
                "press / and type something, ENTER to search"
            } else {
                "no results"
            }
        }
    }
}

fn row_line(
    idx: usize,
    name: &str,
    sub: &str,
    tail: &str,
    playing: bool,
    selected: bool,
    w: usize,
    th: &Theme,
) -> Line<'static> {
    let flex = w.saturating_sub(15);
    let wn = flex * 3 / 5;
    let ws = flex - wn;
    let marker = if playing { "♪ " } else { "  " };
    let (s1, s2) = if selected {
        (sel_style(th), sel_style(th))
    } else {
        (st(th.lit), st(th.dim))
    };
    Line::from(vec![
        Span::styled(format!("{marker}{idx:>4} {}", fit(name, wn)), s1),
        Span::styled(format!(" {}", fit(sub, ws)), s2),
        Span::styled(format!(" {tail:>6}"), s2),
    ])
}

fn item_row(i: usize, it: &Item, selected: bool, now_id: Option<&str>, w: usize, th: &Theme) -> Line<'static> {
    match it {
        Item::Track(t) => row_line(
            i + 1,
            &t.name,
            &t.artists,
            &mmss(t.duration_ms),
            now_id == Some(t.id.as_str()),
            selected,
            w,
            th,
        ),
        Item::Album(a) => row_line(i + 1, &a.name, &a.artists, &format!("{} tr", a.tracks), false, selected, w, th),
        Item::Playlist(p) => row_line(i + 1, &p.name, &p.owner, &format!("{} tr", p.tracks), false, selected, w, th),
    }
}

fn draw_list(
    f: &mut Frame,
    area: Rect,
    v: &mut ListView,
    now_id: Option<&str>,
    th: &Theme,
    tick: u64,
    empty: &str,
) {
    if v.items.is_empty() {
        let msg = if let Some(e) = &v.error {
            format!(" ! {e}")
        } else if v.loading || !v.requested {
            format!(" {} loading...", spinner(tick))
        } else {
            format!(" {empty}")
        };
        f.render_widget(Paragraph::new(Line::styled(msg, st(th.dim))), area);
        return;
    }
    let (h, w) = (area.height as usize, area.width as usize);
    let range = v.cur.window(v.items.len(), h);
    let sel = v.cur.sel;
    let mut lines = Vec::with_capacity(h);
    for i in range {
        lines.push(item_row(i, &v.items[i], i == sel, now_id, w, th));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_queue(f: &mut Frame, app: &mut App, sub: Rect, list: Rect, th: &Theme) {
    let n = app.queue.len();
    f.render_widget(
        Paragraph::new(Line::styled(format!(" {n} TRACKS"), st(th.dim))),
        sub,
    );
    f.render_widget(
        Paragraph::new(Line::styled("ENTER play · D remove · C clear ", st(th.dim)))
            .alignment(Alignment::Right),
        sub,
    );
    if n == 0 {
        f.render_widget(
            Paragraph::new(Line::styled(
                " queue is empty: ENTER on a track plays it and queues the rest; A adds one",
                st(th.dim),
            )),
            list,
        );
        return;
    }
    let (h, w) = (list.height as usize, list.width as usize);
    let mut cur = app.qcur;
    let range = cur.window(n, h);
    app.qcur = cur;
    let playing = app.queue.pos();
    let mut lines = Vec::with_capacity(h);
    for p in range {
        if let Some(t) = app.queue.get(p) {
            lines.push(row_line(
                p + 1,
                &t.name,
                &t.artists,
                &mmss(t.duration_ms),
                playing == Some(p),
                p == cur.sel,
                w,
                th,
            ));
        }
    }
    f.render_widget(Paragraph::new(lines), list);
}

fn draw_body(f: &mut Frame, app: &mut App, area: Rect, th: &Theme) {
    let title = if let Some(d) = &app.detail {
        format!(" {} ", d.title.to_uppercase())
    } else {
        match app.tab {
            Tab::Queue => " QUEUE ".to_string(),
            Tab::Library => " LIBRARY ".to_string(),
            Tab::Playlists => " PLAYLISTS ".to_string(),
            Tab::Search => " SEARCH ".to_string(),
        }
    };
    let block = Block::bordered()
        .border_style(st(th.dim))
        .title(Span::styled(title, st(th.lit)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 3 {
        return;
    }
    let [tabs_a, sub_a, list_a] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    let mut tabs = Vec::new();
    for (i, (tab, name)) in [
        (Tab::Queue, "QUEUE"),
        (Tab::Library, "LIBRARY"),
        (Tab::Playlists, "PLAYLISTS"),
        (Tab::Search, "SEARCH"),
    ]
    .iter()
    .enumerate()
    {
        let style = if *tab == app.tab && app.detail.is_none() {
            sel_style(th)
        } else if *tab == app.tab {
            st(th.lit)
        } else {
            st(th.dim)
        };
        tabs.push(Span::styled(format!(" {} {} ", i + 1, name), style));
        tabs.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(tabs)), tabs_a);

    let tick = app.tick;
    let now_id = app.now.track.as_ref().map(|t| t.id.clone());
    match app.current_target() {
        None => draw_queue(f, app, sub_a, list_a, th),
        Some(t) => {
            let (left, right) = sub_line(app, t, th);
            f.render_widget(Paragraph::new(Line::from(left)), sub_a);
            f.render_widget(
                Paragraph::new(Line::styled(format!("{right} "), st(th.dim)))
                    .alignment(Alignment::Right),
                sub_a,
            );
            let empty = empty_msg(app, t);
            if let Some(v) = app.view_mut(t) {
                draw_list(f, list_a, v, now_id.as_deref(), th, tick, empty);
            }
        }
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let (text, color) = if let Some((m, _)) = &app.msg {
        (format!(" {m}"), th.peak)
    } else if let Some(n) = &app.conn_note {
        (format!(" {n}"), th.peak)
    } else {
        (
            " SPACE play/pause  N/P skip  ←/→ seek  +/- vol  / search  1-4 tabs  ENTER play  A queue  ? help  Q quit"
                .to_string(),
            th.dim,
        )
    };
    f.render_widget(Paragraph::new(Line::styled(text, st(color))), area);
}

fn draw_help(f: &mut Frame, area: Rect, th: &Theme) {
    let rows = [
        ("SPACE", "play / pause"),
        ("N / P", "next / previous (P restarts the track after 3s)"),
        ("← / →", "seek -5s / +5s"),
        ("+ / -  M", "volume up / down, mute"),
        ("/", "search (ENTER runs it, ESC cancels)"),
        ("[ / ]", "switch songs/albums, or tracks/albums/playlists"),
        ("1-4 · TAB", "queue · library · playlists · search"),
        ("↑↓ J K", "move   G/g bottom/top   PgUp/PgDn"),
        ("ENTER", "play a track (queues the rest) / open album or playlist"),
        ("ESC · ⌫", "back out of an album or playlist"),
        ("A", "add selected track to queue"),
        ("D · C", "remove from queue · clear queue (C = Shift+C)"),
        ("S · R", "shuffle · repeat off/all/one"),
        ("V · T", "visualizer style · colour theme"),
        ("< / >", "visualizer sensitivity down / up"),
        ("Shift+R", "reconnect to Spotify"),
        ("Q", "quit"),
    ];
    let w = 70u16.min(area.width.saturating_sub(2));
    let h = (rows.len() as u16 + 4).min(area.height);
    let r = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .border_style(st(th.lit))
        .title(Span::styled(" KEYS ", st(th.peak)))
        .style(Style::new().bg(th.bg));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!(" {k:<10}"), st(th.lit)),
                Span::styled(d.to_string(), st(th.dim)),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::styled(" press any key to close", st(th.ghost)));
    f.render_widget(Paragraph::new(lines), inner);
}
