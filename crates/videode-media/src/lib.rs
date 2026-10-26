//! Media I/O on the system FFmpeg: probing files, decoding exact frames for
//! the viewer and thumbnails, and a small pool of decoder threads with a
//! frame cache. Pictures come out as BGRA, the order GPUI draws.

pub mod audio;
pub mod demo;
mod pool;

use std::path::Path;
use std::sync::Once;

use ff::format::Pixel;
use ff::media::Type;
use ff::software::scaling;
use ffmpeg_next as ff;

pub use pool::{FramePool, FrameRequest};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum MediaError {
    #[error("couldn't open {0}")]
    Open(String),
    #[error("{0} has no picture")]
    NoVideo(String),
    #[error("{0} has no picture or sound")]
    NoStreams(String),
    #[error("decoding failed: {0}")]
    Decode(String),
    /// A newer request for the same slot replaced this one.
    #[error("superseded by a newer request")]
    Superseded,
}

impl From<ff::Error> for MediaError {
    fn from(e: ff::Error) -> Self {
        MediaError::Decode(e.to_string())
    }
}

pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        ff::init().ok();
        ff::util::log::set_level(ff::util::log::Level::Error);
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
    /// A single picture (PNG, JPEG, …) that can be any length.
    Image,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioInfo {
    pub codec: String,
    pub channels: u16,
    pub sample_rate: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub kind: Kind,
    /// Length in seconds; `None` for stills.
    pub duration: Option<f64>,
    /// Picture codec, or the audio codec for sound-only files.
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub audio: Option<AudioInfo>,
}

impl Probe {
    /// One-line format text, e.g. `1920×1080 · 24 fps` or `Stereo · 48 kHz`.
    pub fn format_text(&self) -> String {
        match (self.width, self.height, &self.audio) {
            (Some(w), Some(h), _) => match self.fps.filter(|_| self.kind == Kind::Video) {
                Some(f) => format!("{w}×{h} · {} fps", trim_fps(f)),
                None => format!("{w}×{h}"),
            },
            (_, _, Some(a)) => format!(
                "{} · {} kHz",
                match a.channels {
                    1 => "Mono".to_string(),
                    2 => "Stereo".to_string(),
                    n => format!("{n} ch"),
                },
                trim_fps(a.sample_rate as f64 / 1000.)
            ),
            _ => String::new(),
        }
    }
}

fn trim_fps(f: f64) -> String {
    if (f - f.round()).abs() < 0.005 {
        format!("{}", f.round() as i64)
    } else {
        format!("{f:.2}")
    }
}

/// A human name for an FFmpeg codec id, e.g. `h264` → `H.264`.
pub fn codec_name(id: &str) -> String {
    match id {
        "h264" => "H.264".into(),
        "hevc" => "H.265".into(),
        "av1" => "AV1".into(),
        "vp8" => "VP8".into(),
        "vp9" => "VP9".into(),
        "prores" => "ProRes".into(),
        "dnxhd" => "DNxHD/HR".into(),
        "mjpeg" => "JPEG".into(),
        "png" => "PNG".into(),
        "webp" => "WebP".into(),
        "aac" => "AAC".into(),
        "mp3" | "mp3float" => "MP3".into(),
        "opus" => "Opus".into(),
        "vorbis" => "Vorbis".into(),
        "flac" => "FLAC".into(),
        "alac" => "ALAC".into(),
        id if id.starts_with("pcm_") => "PCM".into(),
        other => other.to_uppercase(),
    }
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

const STILL_CODECS: [&str; 8] = [
    "mjpeg", "png", "webp", "bmp", "tiff", "gif", "jpegxl", "avif",
];

pub fn probe(path: &Path) -> Result<Probe, MediaError> {
    init();
    let input = ff::format::input(path).map_err(|_| MediaError::Open(path_text(path)))?;
    let duration =
        (input.duration() > 0).then(|| input.duration() as f64 / ff::ffi::AV_TIME_BASE as f64);

    let audio = input.streams().best(Type::Audio).and_then(|s| {
        let ctx = ff::codec::Context::from_parameters(s.parameters()).ok()?;
        let a = ctx.decoder().audio().ok()?;
        Some(AudioInfo {
            codec: codec_name(s.parameters().id().name()),
            channels: a.channel_layout().channels() as u16,
            sample_rate: a.rate(),
        })
    });

    if let Some(s) = input.streams().best(Type::Video) {
        let id = s.parameters().id().name().to_string();
        let ctx = ff::codec::Context::from_parameters(s.parameters())?;
        let v = ctx.decoder().video()?;
        let rate = s.avg_frame_rate();
        let fps = (rate.denominator() != 0 && rate.numerator() > 0).then(|| f64::from(rate));
        let still = STILL_CODECS.contains(&id.as_str()) && s.frames() <= 1 && audio.is_none();
        return Ok(Probe {
            kind: if still { Kind::Image } else { Kind::Video },
            duration: if still { None } else { duration },
            codec: codec_name(&id),
            width: Some(v.width()),
            height: Some(v.height()),
            fps,
            audio,
        });
    }
    match audio {
        Some(a) => Ok(Probe {
            kind: Kind::Audio,
            duration,
            codec: a.codec.clone(),
            width: None,
            height: None,
            fps: None,
            audio: Some(a),
        }),
        None => Err(MediaError::NoStreams(path_text(path))),
    }
}

/// A decoded picture in BGRA, rows packed with no padding.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    /// Presentation time of the frame, in seconds from the start.
    pub time: f64,
}

/// The largest size with the same aspect that fits in `max_w × max_h`
/// (never upscaled; even dimensions for the scaler).
pub fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let s = (max_w as f64 / w as f64)
        .min(max_h as f64 / h as f64)
        .min(1.);
    let even = |v: f64| ((v.round() as u32).max(2) + 1) & !1;
    (even(w as f64 * s), even(h as f64 * s))
}

/// Decodes exact frames from one file. Requests that move forward a little
/// keep decoding from where the last one stopped; anything else seeks to the
/// keyframe before the target and decodes forward.
pub struct VideoDecoder {
    input: ff::format::context::Input,
    stream: usize,
    decoder: ff::decoder::Video,
    time_base: f64,
    start: f64,
    frame_dur: f64,
    /// The frame last returned, and its time.
    last: Option<(f64, ff::frame::Video)>,
    /// A frame decoded past the last target, kept for the next request.
    ahead: Option<(f64, ff::frame::Video)>,
    eof: bool,
    scaler: Option<(u32, u32, Pixel, u32, u32, scaling::Context)>,
}

/// How far ahead (seconds) a request may be and still decode forward
/// instead of seeking.
const FORWARD_WINDOW: f64 = 2.;

impl VideoDecoder {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        init();
        let input = ff::format::input(path).map_err(|_| MediaError::Open(path_text(path)))?;
        let s = input
            .streams()
            .best(Type::Video)
            .ok_or_else(|| MediaError::NoVideo(path_text(path)))?;
        let stream = s.index();
        let tb = s.time_base();
        let time_base = f64::from(tb);
        let start = if s.start_time() == ff::ffi::AV_NOPTS_VALUE {
            0.
        } else {
            s.start_time() as f64 * time_base
        };
        let rate = s.avg_frame_rate();
        let fps = if rate.numerator() > 0 && rate.denominator() > 0 {
            f64::from(rate)
        } else {
            25.
        };
        let mut ctx = ff::codec::Context::from_parameters(s.parameters())?;
        // Use a few threads per decoder; the pool runs several decoders.
        ctx.set_threading(ff::threading::Config {
            kind: ff::threading::Type::Frame,
            count: 2,
        });
        let decoder = ctx.decoder().video()?;
        Ok(Self {
            input,
            stream,
            decoder,
            time_base,
            start,
            frame_dur: 1. / fps,
            last: None,
            ahead: None,
            eof: false,
            scaler: None,
        })
    }

    fn frame_time(&self, f: &ff::frame::Video) -> Option<f64> {
        // Best-effort timestamps survive streams with missing pts.
        f.timestamp()
            .or(f.pts())
            .map(|ts| ts as f64 * self.time_base - self.start)
    }

    fn seek(&mut self, t: f64) -> Result<(), MediaError> {
        let ts = ((t + self.start).max(0.) * ff::ffi::AV_TIME_BASE as f64) as i64;
        // The range end is the latest keyframe allowed (avformat max_ts).
        self.input.seek(ts, ..ts)?;
        self.decoder.flush();
        self.last = None;
        self.ahead = None;
        self.eof = false;
        Ok(())
    }

    /// Decodes the next frame and its time, or `None` at the end.
    fn next_frame(&mut self) -> Result<Option<(f64, ff::frame::Video)>, MediaError> {
        if let Some(f) = self.ahead.take() {
            return Ok(Some(f));
        }
        let mut frame = ff::frame::Video::empty();
        loop {
            if self.decoder.receive_frame(&mut frame).is_ok() {
                let t = self.frame_time(&frame).unwrap_or(0.);
                return Ok(Some((t, frame)));
            }
            if self.eof {
                return Ok(None);
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

    /// The frame showing at `t` seconds: the last frame starting at or before
    /// `t`, with half a frame of slack to absorb rounding between rates.
    /// Past the end, the last frame.
    pub fn frame_at(&mut self, t: f64, max_w: u32, max_h: u32) -> Result<Picture, MediaError> {
        let target = t.max(0.) + self.frame_dur / 2.;
        let covers = |ft: f64, dur: f64| target >= ft && target < ft + dur;
        let reuse = matches!(&self.last, Some((ft, _)) if covers(*ft, self.frame_dur));
        if !reuse {
            let forward =
                matches!(&self.last, Some((ft, _)) if target > *ft && target - ft < FORWARD_WINDOW);
            if !forward {
                self.seek(t.max(0.))?;
            }
            while let Some((ft, f)) = self.next_frame()? {
                if ft > target && self.last.is_some() {
                    // Overshot: the previous frame is the one; keep this for later.
                    self.ahead = Some((ft, f));
                    break;
                }
                self.last = Some((ft, f));
                if covers(ft, self.frame_dur) {
                    break;
                }
            }
        }
        let (time, frame) = self
            .last
            .take()
            .ok_or_else(|| MediaError::Decode("no frames".into()))?;
        let picture = self.scale(&frame, time, max_w, max_h);
        self.last = Some((time, frame));
        picture
    }

    fn scale(
        &mut self,
        f: &ff::frame::Video,
        time: f64,
        max_w: u32,
        max_h: u32,
    ) -> Result<Picture, MediaError> {
        let (w, h) = fit(f.width(), f.height(), max_w, max_h);
        let key = (f.width(), f.height(), f.format(), w, h);
        let fresh =
            !matches!(&self.scaler, Some((a, b, c, d, e, _)) if (*a, *b, *c, *d, *e) == key);
        if fresh {
            let ctx = scaling::Context::get(
                f.format(),
                f.width(),
                f.height(),
                Pixel::BGRA,
                w,
                h,
                scaling::Flags::BILINEAR,
            )?;
            self.scaler = Some((key.0, key.1, key.2, key.3, key.4, ctx));
        }
        let scaler = &mut self.scaler.as_mut().expect("set above").5;
        let mut out = ff::frame::Video::empty();
        scaler.run(f, &mut out)?;
        let stride = out.stride(0);
        let row = w as usize * 4;
        let data = out.data(0);
        let mut bgra = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            bgra.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        Ok(Picture {
            width: w,
            height: h,
            bgra,
            time,
        })
    }
}

/// A small picture for the bin: a tenth of the way in, or the only frame of
/// a still.
pub fn thumbnail(path: &Path, max_w: u32, max_h: u32) -> Result<Picture, MediaError> {
    let p = probe(path)?;
    if p.kind == Kind::Audio {
        return Err(MediaError::NoVideo(path_text(path)));
    }
    let at = p.duration.map_or(0., |d| (d * 0.1).min(2.));
    VideoDecoder::open(path)?.frame_at(at, max_w, max_h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    /// 2 s at 24 fps, 64×48, each frame a flat grey that encodes its index,
    /// with a keyframe every 12 frames so seeks land mid-GOP.
    fn fixture() -> Option<PathBuf> {
        // Generated once; tests run in parallel and must not race on it.
        static FIXTURE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let dir =
                    std::env::temp_dir().join(format!("videode-media-test-{}", std::process::id()));
                std::fs::create_dir_all(&dir).ok()?;
                let path = dir.join("frames.mp4");
                let ok = Command::new("ffmpeg")
                    .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
                    .arg("nullsrc=s=64x48:r=24:d=2,format=gray,geq=lum='16+N*4'")
                    .args([
                        "-c:v", "libx264", "-qp", "0", "-g", "12", "-pix_fmt", "yuv420p",
                    ])
                    .arg(&path)
                    .status()
                    .ok()?
                    .success();
                ok.then_some(path)
            })
            .clone()
    }

    #[test]
    fn probes_video() {
        let Some(path) = fixture() else {
            eprintln!("skipped: ffmpeg CLI not available");
            return;
        };
        let p = probe(&path).unwrap();
        assert_eq!(p.kind, Kind::Video);
        assert_eq!((p.width, p.height), (Some(64), Some(48)));
        assert_eq!(p.codec, "H.264");
        assert!((p.fps.unwrap() - 24.).abs() < 0.01);
        assert!((p.duration.unwrap() - 2.).abs() < 0.1);
        assert_eq!(p.format_text(), "64×48 · 24 fps");
    }

    #[test]
    fn decodes_exact_frames_in_any_order() {
        let Some(path) = fixture() else {
            eprintln!("skipped: ffmpeg CLI not available");
            return;
        };
        // Ground truth: every frame, decoded front to back from a fresh open.
        let mut d = VideoDecoder::open(&path).unwrap();
        let truth: Vec<Picture> = (0..48)
            .map(|n| d.frame_at(n as f64 / 24., 64, 48).unwrap())
            .collect();
        let greys: Vec<u8> = truth
            .iter()
            .map(|p| p.bgra[(24 * 64 + 32) * 4 + 1])
            .collect();
        assert!(
            greys.windows(2).all(|w| w[1] > w[0]),
            "each frame is distinct: {greys:?}"
        );

        // Random access, backwards, keyframes and mid-GOP, then a forward run.
        let mut d = VideoDecoder::open(&path).unwrap();
        let order: Vec<usize> = [30, 5, 47, 13, 12, 0, 23, 24]
            .into_iter()
            .chain(31..40)
            .collect();
        for n in order {
            let pic = d.frame_at(n as f64 / 24., 64, 48).unwrap();
            assert_eq!(
                pic.bgra, truth[n].bgra,
                "frame {n} differs from sequential decode"
            );
            assert!(
                (pic.time - n as f64 / 24.).abs() < 1e-3,
                "frame {n} has time {}",
                pic.time
            );
        }
        // Past the end clamps to the last frame; between frames rounds down.
        assert_eq!(d.frame_at(10., 64, 48).unwrap().bgra, truth[47].bgra);
        assert_eq!(d.frame_at(10.4 / 24., 64, 48).unwrap().bgra, truth[10].bgra);
    }

    #[test]
    fn scales_to_fit_without_upscaling() {
        assert_eq!(fit(1920, 1080, 160, 90), (160, 90));
        assert_eq!(fit(1920, 1080, 160, 160), (160, 90));
        assert_eq!(fit(64, 48, 640, 480), (64, 48));
        let (w, h) = fit(1001, 777, 300, 300);
        assert!(w % 2 == 0 && h % 2 == 0 && w <= 300);
    }

    #[test]
    fn missing_files_fail_cleanly() {
        assert!(matches!(
            probe(Path::new("/nope/missing.mov")),
            Err(MediaError::Open(_))
        ));
    }
}
