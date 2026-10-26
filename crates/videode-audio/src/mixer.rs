//! Renders the sequence's sound: every audio clip under a stretch of time,
//! with its gain and fades, summed to stereo.

use std::collections::HashMap;
use std::path::PathBuf;

use videode_media::audio::{AudioDecoder, CHANNELS};

/// One audio clip, in seconds of sequence time.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipSound {
    pub path: PathBuf,
    /// Index into [`Mix::tracks`].
    pub track: usize,
    pub start: f64,
    pub end: f64,
    /// Where in the file the clip starts.
    pub source_in: f64,
    /// Linear gain from the clip's nodes.
    pub gain: f32,
    pub fade_in: f64,
    pub fade_out: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackSound {
    pub muted: bool,
}

/// Everything the mixer needs to know about the sequence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mix {
    pub clips: Vec<ClipSound>,
    pub tracks: Vec<TrackSound>,
}

impl ClipSound {
    /// The clip's gain at sequence time `t`, fades included.
    fn gain_at(&self, t: f64) -> f32 {
        let local = t - self.start;
        let mut g = self.gain as f64;
        if self.fade_in > 0. {
            g *= (local / self.fade_in).clamp(0., 1.);
        }
        if self.fade_out > 0. {
            g *= ((self.end - t) / self.fade_out).clamp(0., 1.);
        }
        g as f32
    }
}

struct Voice {
    path: PathBuf,
    decoder: Option<AudioDecoder>,
    /// File time of the next sample the decoder will return.
    next: f64,
}

pub struct Mixer {
    rate: u32,
    mix: Mix,
    /// Open decoders, by clip index; `None` decoder means the file failed.
    voices: HashMap<usize, Voice>,
    scratch: Vec<f32>,
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            mix: Mix::default(),
            voices: HashMap::new(),
            scratch: Vec::new(),
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn set_mix(&mut self, mix: Mix) {
        // Keep decoders whose clip still plays the same file.
        self.voices
            .retain(|&i, v| mix.clips.get(i).is_some_and(|c| c.path == v.path));
        self.mix = mix;
    }

    /// Fills `out` (interleaved stereo) with the sound starting at sequence
    /// time `at` seconds.
    pub fn render(&mut self, at: f64, out: &mut [f32]) {
        out.fill(0.);
        let rate = self.rate as f64;
        let frames = out.len() / CHANNELS;
        let until = at + frames as f64 / rate;
        for (i, clip) in self.mix.clips.iter().enumerate() {
            let muted = self.mix.tracks.get(clip.track).is_none_or(|t| t.muted);
            if muted || clip.end <= at || clip.start >= until || clip.gain == 0. {
                continue;
            }
            let from = at.max(clip.start);
            let o0 = ((from - at) * rate).round() as usize;
            let o1 = (((until.min(clip.end)) - at) * rate).round() as usize;
            let o1 = o1.min(frames);
            if o1 <= o0 {
                continue;
            }
            let src = clip.source_in + (from - clip.start);
            let voice = self.voices.entry(i).or_insert_with(|| Voice {
                path: clip.path.clone(),
                decoder: AudioDecoder::open_at(&clip.path, self.rate).ok(),
                next: 0.,
            });
            let Some(decoder) = &mut voice.decoder else {
                continue;
            };
            if (voice.next - src).abs() > 1.5 / rate {
                if decoder.seek(src).is_err() {
                    voice.decoder = None;
                    continue;
                }
                voice.next = src;
            }
            let n = o1 - o0;
            self.scratch.resize(n * CHANNELS, 0.);
            if decoder.read(&mut self.scratch).is_err() {
                voice.decoder = None;
                continue;
            }
            voice.next += n as f64 / rate;
            for f in 0..n {
                let g = clip.gain_at(from + f as f64 / rate);
                for c in 0..CHANNELS {
                    out[(o0 + f) * CHANNELS + c] += self.scratch[f * CHANNELS + c] * g;
                }
            }
        }
    }
}

/// Decibels to a linear gain; −96 dB and below is silence.
pub fn db_to_gain(db: f64) -> f32 {
    if db <= -96. {
        0.
    } else {
        10f64.powf(db / 20.) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// 2 s of a constant 0.25 on both channels at 48 kHz.
    fn level() -> Option<PathBuf> {
        static FIXTURE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let dir =
                    std::env::temp_dir().join(format!("videode-mixer-test-{}", std::process::id()));
                std::fs::create_dir_all(&dir).ok()?;
                let path = dir.join("level.wav");
                let ok = Command::new("ffmpeg")
                    .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                    .arg("aevalsrc=0.25|0.25:s=48000:d=2")
                    .args(["-c:a", "pcm_f32le"])
                    .arg(&path)
                    .status()
                    .ok()?
                    .success();
                ok.then_some(path)
            })
            .clone()
    }

    fn clip(path: &std::path::Path, track: usize, start: f64, end: f64) -> ClipSound {
        ClipSound {
            path: path.to_path_buf(),
            track,
            start,
            end,
            source_in: 0.,
            gain: 1.,
            fade_in: 0.,
            fade_out: 0.,
        }
    }

    fn tracks(n: usize) -> Vec<TrackSound> {
        vec![TrackSound { muted: false }; n]
    }

    fn render(m: &mut Mixer, at: f64, frames: usize) -> Vec<f32> {
        let mut out = vec![0.; frames * CHANNELS];
        m.render(at, &mut out);
        out
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn places_clips_in_time_and_sums_them() {
        let Some(p) = level() else {
            eprintln!("skipped: ffmpeg CLI not available");
            return;
        };
        let mut m = Mixer::new(48_000);
        m.set_mix(Mix {
            clips: vec![clip(&p, 0, 0.5, 1.5), clip(&p, 1, 1.0, 2.0)],
            tracks: tracks(2),
        });
        let out = render(&mut m, 0., 48_000 * 2);
        let at = |s: f64| out[(s * 48_000.) as usize * CHANNELS];
        assert_eq!(at(0.25), 0., "nothing before the first clip");
        assert!(close(at(0.75), 0.25));
        assert!(close(at(1.25), 0.5), "two clips sum");
        assert!(close(at(1.75), 0.25));
    }

    #[test]
    fn applies_gain_fades_and_mute() {
        let Some(p) = level() else {
            return;
        };
        let mut faded = clip(&p, 0, 0., 1.);
        faded.fade_in = 0.5;
        let mut quiet = clip(&p, 1, 0., 1.);
        quiet.gain = db_to_gain(-6.0206);
        let mut m = Mixer::new(48_000);
        m.set_mix(Mix {
            clips: vec![faded, quiet],
            tracks: vec![TrackSound { muted: false }, TrackSound { muted: true }],
        });
        let out = render(&mut m, 0., 48_000);
        assert!(
            close(out[0], 0.),
            "fade starts silent; muted track adds nothing"
        );
        assert!(close(out[12_000 * CHANNELS], 0.125), "halfway up the fade");
        assert!(close(out[36_000 * CHANNELS], 0.25));

        let mut m = Mixer::new(48_000);
        let mut q = clip(&p, 0, 0., 1.);
        q.gain = db_to_gain(-6.0206);
        m.set_mix(Mix {
            clips: vec![q],
            tracks: tracks(1),
        });
        assert!(close(render(&mut m, 0.5, 16)[0], 0.125), "-6 dB halves it");
    }

    #[test]
    fn consecutive_blocks_read_on_without_gaps() {
        let Some(p) = level() else {
            return;
        };
        let mut m = Mixer::new(44_100);
        m.set_mix(Mix {
            clips: vec![clip(&p, 0, 0., 2.)],
            tracks: tracks(1),
        });
        for block in 0..20 {
            let out = render(&mut m, block as f64 * 512. / 44_100., 512);
            // The resampler's filter needs a few samples to settle after the
            // first seek; after that every block continues seamlessly.
            let settled = if block == 0 { 64 * CHANNELS } else { 0 };
            assert!(
                out[settled..].iter().all(|&s| (s - 0.25).abs() < 0.01),
                "block {block}"
            );
        }
    }
}
