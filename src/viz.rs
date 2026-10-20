//! Spectrum analyzer: Hann window -> radix-2 FFT -> log-spaced bands -> smoothed levels + peak hold.

use std::f32::consts::PI;

pub const FFT_N: usize = 2048;
const SAMPLE_RATE: f32 = 44_100.0;
/// Tweak these two if the bars sit too high/low on your music.
const DB_FLOOR: f32 = -75.0;
const DB_RANGE: f32 = 75.0;
/// >1 pushes mid levels down so bars use their full height range (adjust live with < and >)
const GAMMA: f32 = 1.35;

pub struct Viz {
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    pub levels: Vec<f32>,
    pub peaks: Vec<f32>,
    hold: Vec<u8>,
    /// live sensitivity offset in dB (keys < and >)
    pub sens_db: f32,
}

impl Viz {
    pub fn new() -> Self {
        let window = (0..FFT_N)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * (i as f32) / (FFT_N as f32 - 1.0)).cos())
            .collect();
        Viz {
            window,
            re: vec![0.0; FFT_N],
            im: vec![0.0; FFT_N],
            levels: Vec::new(),
            peaks: Vec::new(),
            hold: Vec::new(),
            sens_db: 0.0,
        }
    }

    /// `samples`: latest mono samples (>= FFT_N) or None when silent/paused (bars fall).
    pub fn update(&mut self, samples: Option<&[f32]>, bands: usize) {
        let bands = bands.max(1);
        if self.levels.len() != bands {
            self.levels = vec![0.0; bands];
            self.peaks = vec![0.0; bands];
            self.hold = vec![0; bands];
        }
        let target = match samples {
            Some(s) if s.len() >= FFT_N => self.analyze(&s[s.len() - FFT_N..], bands),
            _ => vec![0.0; bands],
        };
        for i in 0..bands {
            let t = target[i];
            let l = self.levels[i];
            let mut nl = if t > l { l + (t - l) * 0.65 } else { l * 0.86 + t * 0.14 };
            if nl < 0.004 {
                nl = 0.0;
            }
            self.levels[i] = nl;
            if nl >= self.peaks[i] {
                self.peaks[i] = nl;
                self.hold[i] = 12;
            } else if self.hold[i] > 0 {
                self.hold[i] -= 1;
            } else {
                self.peaks[i] = (self.peaks[i] - 0.025).max(nl);
            }
        }
    }

    fn analyze(&mut self, s: &[f32], bands: usize) -> Vec<f32> {
        for i in 0..FFT_N {
            self.re[i] = s[i] * self.window[i];
            self.im[i] = 0.0;
        }
        fft(&mut self.re, &mut self.im);
        let half = FFT_N / 2;
        let norm = FFT_N as f32 / 4.0; // full-scale sine ~ 1.0 after Hann window
        let mags: Vec<f32> = (0..half)
            .map(|k| (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() / norm)
            .collect();

        let (f_lo, f_hi) = (40.0f32, 16_000.0f32);
        let ratio = (f_hi / f_lo).powf(1.0 / bands as f32);
        let bin_hz = SAMPLE_RATE / FFT_N as f32;
        let mut out = Vec::with_capacity(bands);
        for b in 0..bands {
            let lo_f = f_lo * ratio.powi(b as i32);
            let hi_f = lo_f * ratio;
            let lo = ((lo_f / bin_hz).floor() as usize).clamp(1, half - 1);
            let hi = ((hi_f / bin_hz).ceil() as usize).clamp(lo + 1, half);
            let m = mags[lo..hi].iter().cloned().fold(0.0f32, f32::max);
            let centre = (lo_f * hi_f).sqrt();
            let tilt = 3.0 * (centre / 1000.0).log2(); // +3 dB/octave so highs aren't always flat
            let db = 20.0 * (m + 1e-7).log10() + tilt + self.sens_db;
            out.push(((db - DB_FLOOR) / DB_RANGE).clamp(0.0, 1.0).powf(GAMMA));
        }
        out
    }
}

/// In-place iterative radix-2 FFT. `re.len()` must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wlr, wli) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut wr, mut wi) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let vr = re[b] * wr - im[b] * wi;
                let vi = re[b] * wi + im[b] * wr;
                let (ur, ui) = (re[a], im[a]);
                re[a] = ur + vr;
                im[a] = ui + vi;
                re[b] = ur - vr;
                im[b] = ui - vi;
                let nwr = wr * wlr - wi * wli;
                wi = wr * wli + wi * wlr;
                wr = nwr;
            }
            i += len;
        }
        len <<= 1;
    }
}
