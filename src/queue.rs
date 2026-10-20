use crate::api::Track;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Off,
    All,
    One,
}

/// `tracks` keeps insertion order; `order` is the playback order (indices into `tracks`);
/// `pos` is the current position inside `order`. Shuffle only permutes `order`.
pub struct Queue {
    tracks: Vec<Track>,
    order: Vec<usize>,
    pos: Option<usize>,
    pub shuffle: bool,
    pub repeat: Repeat,
    rng: u64,
}

impl Queue {
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
        Queue {
            tracks: Vec::new(),
            order: Vec::new(),
            pos: None,
            shuffle: false,
            repeat: Repeat::Off,
            rng: seed,
        }
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn shuffled(&mut self, mut v: Vec<usize>) -> Vec<usize> {
        for i in (1..v.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
        v
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn pos(&self) -> Option<usize> {
        self.pos
    }

    pub fn get(&self, p: usize) -> Option<&Track> {
        self.order.get(p).and_then(|&i| self.tracks.get(i))
    }

    pub fn current(&self) -> Option<&Track> {
        self.pos.and_then(|p| self.get(p))
    }

    fn reshuffle_around_current(&mut self) {
        let n = self.tracks.len();
        let cur = self.pos.and_then(|p| self.order.get(p).copied());
        let rest: Vec<usize> = (0..n).filter(|&i| Some(i) != cur).collect();
        let rest = self.shuffled(rest);
        self.order = cur.into_iter().chain(rest).collect();
        self.pos = if cur.is_some() { Some(0) } else { None };
    }

    /// Replace the queue; `start` becomes the current track.
    pub fn set(&mut self, tracks: Vec<Track>, start: usize) {
        self.tracks = tracks;
        let n = self.tracks.len();
        self.order = (0..n).collect();
        self.pos = if n == 0 { None } else { Some(start.min(n - 1)) };
        if self.shuffle {
            self.reshuffle_around_current();
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.reshuffle_around_current();
        } else {
            let cur = self.pos.and_then(|p| self.order.get(p).copied());
            self.order = (0..self.tracks.len()).collect();
            self.pos = cur;
        }
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
    }

    pub fn push(&mut self, t: Track) {
        self.tracks.push(t);
        self.order.push(self.tracks.len() - 1);
    }

    /// Remove by playback position. The playing track can't be removed.
    pub fn remove(&mut self, p: usize) -> bool {
        if p >= self.order.len() || Some(p) == self.pos {
            return false;
        }
        let idx = self.order.remove(p);
        self.tracks.remove(idx);
        for o in self.order.iter_mut() {
            if *o > idx {
                *o -= 1;
            }
        }
        if let Some(cp) = self.pos {
            if p < cp {
                self.pos = Some(cp - 1);
            }
        }
        true
    }

    /// Clear everything except the track that is currently playing.
    pub fn clear(&mut self) {
        let cur = self.current().cloned();
        self.tracks.clear();
        self.order.clear();
        self.pos = None;
        if let Some(t) = cur {
            self.tracks.push(t);
            self.order.push(0);
            self.pos = Some(0);
        }
    }

    pub fn jump(&mut self, p: usize) -> Option<Track> {
        if p < self.order.len() {
            self.pos = Some(p);
            self.current().cloned()
        } else {
            None
        }
    }

    /// `auto` = triggered by end-of-track (honours repeat-one).
    pub fn next(&mut self, auto: bool) -> Option<Track> {
        if self.repeat == Repeat::One && auto {
            return self.current().cloned();
        }
        let n = self.order.len();
        if n == 0 {
            return None;
        }
        match self.pos {
            None => self.pos = Some(0),
            Some(p) if p + 1 < n => self.pos = Some(p + 1),
            Some(_) => {
                if self.repeat == Repeat::All {
                    if self.shuffle {
                        let all = self.shuffled((0..n).collect());
                        self.order = all;
                    }
                    self.pos = Some(0);
                } else {
                    return None;
                }
            }
        }
        self.current().cloned()
    }

    pub fn prev(&mut self) -> Option<Track> {
        let n = self.order.len();
        if n == 0 {
            return None;
        }
        self.pos = match self.pos {
            Some(p) if p > 0 => Some(p - 1),
            Some(_) if self.repeat == Repeat::All => Some(n - 1),
            Some(p) => Some(p),
            None => Some(0),
        };
        self.current().cloned()
    }

    /// What will play after the current track (for gapless preloading).
    pub fn peek_next(&self) -> Option<&Track> {
        if self.repeat == Repeat::One {
            return None;
        }
        let n = self.order.len();
        match self.pos {
            Some(p) if p + 1 < n => self.get(p + 1),
            Some(_) if self.repeat == Repeat::All && !self.shuffle => self.get(0),
            _ => None,
        }
    }
}
