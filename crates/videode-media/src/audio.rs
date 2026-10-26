//! Sound from any file FFmpeg can read, as 48 kHz stereo `f32` samples,
//! plus waveform peaks for drawing.

use std::collections::VecDeque;
use std::path::Path;

use ff::ChannelLayout;
use ff::format::Sample;
use ff::format::sample::Type as SampleType;
use ff::media::Type;
use ff::software::resampling;
use ffmpeg_next as ff;

use crate::{MediaError, init, path_text};

/// The mixer's sample rate.
pub const RATE: u32 = 48_000;
/// Output channels (interleaved left, right).
pub const CHANNELS: usize = 2;

/// Decodes one file's sound as interleaved stereo at a fixed rate,
/// sample-exact after a seek.
pub struct AudioDecoder {
    rate: u32,
    input: ff::format::context::Input,
    stream: usize,
    decoder: ff::decoder::Audio,
    time_base: f64,
    start: f64,
    resampler: Option<(Sample, u64, i32, u32, resampling::Context)>,
    /// Converted samples not yet read, interleaved.
    pending: VecDeque<f32>,
    /// Time (seconds) of the first frame in `pending`.
    pending_at: Option<f64>,
    eof: bool,
}

impl AudioDecoder {
    /// Opens `path` for output at [`RATE`].
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        Self::open_at(path, RATE)
    }

    /// Opens `path` for output at `rate` samples per second.
    pub fn open_at(path: &Path, rate: u32) -> Result<Self, MediaError> {
        init();
        let input = ff::format::input(path).map_err(|_| MediaError::Open(path_text(path)))?;
        let s = input
            .streams()
            .best(Type::Audio)
            .ok_or_else(|| MediaError::NoStreams(path_text(path)))?;
        let stream = s.index();
        let time_base = f64::from(s.time_base());
        let start = if s.start_time() == ff::ffi::AV_NOPTS_VALUE {
            0.
        } else {
            s.start_time() as f64 * time_base
        };
        let decoder = ff::codec::Context::from_parameters(s.parameters())?
            .decoder()
            .audio()?;
        Ok(Self {
            rate,
            input,
            stream,
            decoder,
            time_base,
            start,
            resampler: None,
            pending: VecDeque::new(),
            pending_at: Some(0.),
            eof: false,
        })
    }

    /// Positions the decoder so the next [`Self::read`] starts at `t` seconds.
    pub fn seek(&mut self, t: f64) -> Result<(), MediaError> {
        let t = t.max(0.);
        let ts = ((t + self.start) * ff::ffi::AV_TIME_BASE as f64) as i64;
        self.input.seek(ts, ..ts)?;
        self.decoder.flush();
        self.resampler = None;
        self.pending.clear();
        self.pending_at = None;
        self.eof = false;
        // Decode up to the target and drop what comes before it.
        loop {
            if self.pending_at.is_none() || self.pending.is_empty() {
                self.pending.clear();
                self.pending_at = None;
                if !self.decode_more()? {
                    return Ok(());
                }
            }
            let at = self.pending_at.unwrap_or(t);
            let frames = self.pending.len() / CHANNELS;
            let end = at + frames as f64 / self.rate as f64;
            if end <= t {
                self.pending.clear();
                self.pending_at = None;
                continue;
            }
            let skip = ((t - at) * self.rate as f64).round().max(0.) as usize;
            self.pending
                .drain(..(skip * CHANNELS).min(self.pending.len()));
            self.pending_at = Some(t);
            return Ok(());
        }
    }

    /// Fills `out` (interleaved stereo) with the next samples. Returns how
    /// many frames came from the file; the rest is silence past its end.
    pub fn read(&mut self, out: &mut [f32]) -> Result<usize, MediaError> {
        let want = out.len() / CHANNELS;
        let mut got = 0;
        while got < want {
            if self.pending.is_empty() && !self.decode_more()? {
                break;
            }
            let n = (want - got).min(self.pending.len() / CHANNELS);
            for (i, s) in self.pending.drain(..n * CHANNELS).enumerate() {
                out[got * CHANNELS + i] = s;
            }
            got += n;
            if let Some(at) = &mut self.pending_at {
                *at += n as f64 / self.rate as f64;
            }
        }
        out[got * CHANNELS..].fill(0.);
        Ok(got)
    }

    /// Decodes one more frame into `pending`. False at the end of the file.
    fn decode_more(&mut self) -> Result<bool, MediaError> {
        let mut frame = ff::frame::Audio::empty();
        loop {
            if self.decoder.receive_frame(&mut frame).is_ok() {
                let t = frame
                    .timestamp()
                    .or(frame.pts())
                    .map(|ts| ts as f64 * self.time_base - self.start);
                if self.pending.is_empty() || self.pending_at.is_none() {
                    self.pending_at = t.or(self.pending_at);
                }
                self.convert(&mut frame)?;
                return Ok(true);
            }
            if self.eof {
                return Ok(false);
            }
            let mut sent = false;
            for (s, packet) in self.input.packets() {
                if s.index() == self.stream {
                    self.decoder.send_packet(&packet)?;
                    sent = true;
                    break;
                }
            }
            if !sent {
                self.eof = true;
                self.decoder.send_eof().ok();
            }
        }
    }

    fn convert(&mut self, frame: &mut ff::frame::Audio) -> Result<(), MediaError> {
        if frame.channel_layout().is_empty() {
            let n = frame.channel_layout().channels().max(1);
            frame.set_channel_layout(ChannelLayout::default(n));
        }
        let layout = frame.channel_layout();
        let key = (
            frame.format(),
            layout.bits(),
            layout.channels(),
            frame.rate(),
        );
        let fresh = !matches!(&self.resampler, Some((f, b, c, r, _)) if (*f, *b, *c, *r) == key);
        if fresh {
            let ctx = resampling::Context::get(
                frame.format(),
                layout,
                frame.rate(),
                Sample::F32(SampleType::Packed),
                ChannelLayout::STEREO,
                self.rate,
            )?;
            self.resampler = Some((key.0, key.1, key.2, key.3, ctx));
        }
        let ctx = &mut self.resampler.as_mut().expect("set above").4;
        let capacity = frame.samples() as u64 * self.rate as u64 / frame.rate().max(1) as u64 + 256;
        let mut out = ff::frame::Audio::new(
            Sample::F32(SampleType::Packed),
            capacity as usize,
            ChannelLayout::STEREO,
        );
        ctx.run(frame, &mut out)?;
        let samples = out.samples() * CHANNELS;
        self.pending
            .extend(&out.plane::<f32>(0)[..samples.min(out.plane::<f32>(0).len())]);
        Ok(())
    }
}

/// Waveform peaks: the loudest absolute sample (0–1, both channels) in each
/// `1 / per_second` slice of the file.
pub fn peaks(path: &Path, per_second: u32) -> Result<Vec<f32>, MediaError> {
    let mut d = AudioDecoder::open(path)?;
    let bucket = (RATE / per_second.max(1)) as usize;
    let mut buf = vec![0.; bucket * CHANNELS];
    let mut out = Vec::new();
    loop {
        let got = d.read(&mut buf)?;
        if got == 0 {
            break;
        }
        let peak = buf[..got * CHANNELS]
            .iter()
            .fold(0f32, |m, s| m.max(s.abs()));
        out.push(peak.min(1.));
        if got < bucket {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    /// 1 s of 44.1 kHz mono: silence for the first half, then a full-scale
    /// square wave, so resampling and seeking both show up in the samples.
    fn fixture() -> Option<PathBuf> {
        static FIXTURE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let dir =
                    std::env::temp_dir().join(format!("videode-audio-test-{}", std::process::id()));
                std::fs::create_dir_all(&dir).ok()?;
                let path = dir.join("half.wav");
                let ok = Command::new("ffmpeg")
                    .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                    .arg("aevalsrc='if(lt(t,0.5),0,if(lt(mod(t*100,1),0.5),0.9,-0.9))':s=44100:d=1")
                    .args(["-c:a", "pcm_s16le"])
                    .arg(&path)
                    .status()
                    .ok()?
                    .success();
                ok.then_some(path)
            })
            .clone()
    }

    fn loud(samples: &[f32]) -> bool {
        samples.iter().any(|s| s.abs() > 0.5)
    }

    #[test]
    fn reads_resampled_stereo_to_the_end() {
        let Some(path) = fixture() else {
            eprintln!("skipped: ffmpeg CLI not available");
            return;
        };
        let mut d = AudioDecoder::open(&path).unwrap();
        let mut all = Vec::new();
        let mut buf = vec![0.; 4096];
        loop {
            let got = d.read(&mut buf).unwrap();
            all.extend_from_slice(&buf[..got * CHANNELS]);
            if got == 0 {
                break;
            }
        }
        let frames = all.len() / CHANNELS;
        assert!((frames as i64 - RATE as i64).abs() < 64, "{frames} frames");
        let half = RATE as usize / 2 * CHANNELS;
        assert!(!loud(&all[..half - 200]), "silent first half");
        assert!(loud(&all[half + 200..]), "loud second half");
        assert_eq!(all[half + 400], all[half + 401], "both channels match");
    }

    #[test]
    fn seeks_to_the_sample() {
        let Some(path) = fixture() else {
            return;
        };
        let mut d = AudioDecoder::open(&path).unwrap();
        let mut buf = vec![0.; 480 * CHANNELS];
        d.seek(0.49).unwrap();
        d.read(&mut buf).unwrap();
        assert!(
            !loud(&buf[..400 * CHANNELS]),
            "10 ms before the edge is silent"
        );
        d.seek(0.51).unwrap();
        d.read(&mut buf).unwrap();
        assert!(loud(&buf), "just after the edge is loud");
        d.seek(0.2).unwrap();
        d.read(&mut buf).unwrap();
        assert!(!loud(&buf), "seeking backwards works");
    }

    #[test]
    fn peaks_follow_loudness() {
        let Some(path) = fixture() else {
            return;
        };
        let p = peaks(&path, 10).unwrap();
        assert_eq!(p.len(), 10);
        assert!(p[..4].iter().all(|&v| v < 0.05), "{p:?}");
        assert!(p[6..].iter().all(|&v| v > 0.8), "{p:?}");
    }
}
