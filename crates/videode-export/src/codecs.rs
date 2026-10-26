//! From the Output node's settings to FFmpeg: which muxer, which encoders
//! to try (hardware first when allowed), and their rate control.

use ffmpeg_next as ff;
use videode_core::OutputSettings;
use videode_core::project::Sequence;

use crate::ExportError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
    Vp9,
    ProRes { hq: bool },
    Png,
}

impl VideoCodec {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "H.264" => Self::H264,
            "H.265" => Self::H265,
            "AV1" => Self::Av1,
            "VP9" => Self::Vp9,
            "ProRes 422" => Self::ProRes { hq: false },
            "ProRes 422 HQ" => Self::ProRes { hq: true },
            "PNG" => Self::Png,
            _ => return None,
        })
    }

    /// FFmpeg encoders for this codec, best first.
    fn encoders(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::H264 => (
                &[
                    "h264_videotoolbox",
                    "h264_nvenc",
                    "h264_qsv",
                    "h264_amf",
                    "h264_mf",
                ],
                &["libx264", "libopenh264"],
            ),
            Self::H265 => (
                &[
                    "hevc_videotoolbox",
                    "hevc_nvenc",
                    "hevc_qsv",
                    "hevc_amf",
                    "hevc_mf",
                ],
                &["libx265"],
            ),
            Self::Av1 => (
                &["av1_nvenc", "av1_qsv", "av1_amf"],
                &["libsvtav1", "libaom-av1"],
            ),
            Self::Vp9 => (&["vp9_qsv"], &["libvpx-vp9"]),
            Self::ProRes { .. } => (&["prores_videotoolbox"], &["prores_ks"]),
            Self::Png => (&[], &["png"]),
        }
    }

    /// Encoders to try in order. Lossless output is software only, since
    /// hardware encoders can't promise it.
    pub fn candidates(self, hardware: bool, quality: Quality) -> Vec<&'static str> {
        let (hw, sw) = self.encoders();
        let mut v = Vec::new();
        if hardware && quality != Quality::Lossless {
            v.extend_from_slice(hw);
        }
        v.extend_from_slice(sw);
        v
    }

    /// Pixel formats in order of preference.
    pub(crate) fn pixel_formats(self) -> &'static [ff::format::Pixel] {
        use ff::format::Pixel;
        match self {
            Self::ProRes { .. } => &[Pixel::YUV422P10LE, Pixel::P210LE],
            Self::Png => &[Pixel::RGB24, Pixel::RGBA],
            _ => &[Pixel::YUV420P, Pixel::NV12],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Draft,
    Standard,
    High,
    Lossless,
}

impl Quality {
    pub fn from_name(name: &str) -> Self {
        match name {
            "Draft" => Self::Draft,
            "Standard" => Self::Standard,
            "Lossless" => Self::Lossless,
            _ => Self::High,
        }
    }
}

/// True for encoders that run on a GPU or media engine.
pub fn is_hardware(encoder: &str) -> bool {
    ["videotoolbox", "nvenc", "_qsv", "_amf", "_mf"]
        .iter()
        .any(|k| encoder.contains(k))
}

/// A short name for an encoder, e.g. `h264_videotoolbox` → `VideoToolbox`.
pub fn encoder_label(encoder: &str) -> &'static str {
    match encoder {
        e if e.contains("videotoolbox") => "VideoToolbox",
        e if e.contains("nvenc") => "NVENC",
        e if e.ends_with("_qsv") => "Quick Sync",
        e if e.ends_with("_amf") => "AMF",
        e if e.ends_with("_mf") => "Media Foundation",
        "libx264" => "x264",
        "libopenh264" => "OpenH264",
        "libx265" => "x265",
        "libsvtav1" => "SVT-AV1",
        "libaom-av1" => "libaom",
        "libvpx-vp9" => "libvpx",
        "prores_ks" => "FFmpeg ProRes",
        "png" => "FFmpeg PNG",
        "aac" => "AAC",
        "libopus" => "Opus",
        e if e.starts_with("pcm_") => "PCM",
        _ => "FFmpeg",
    }
}

/// Everything needed to open the output.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// FFmpeg muxer name.
    pub muxer: &'static str,
    pub video: Option<VideoPlan>,
    /// Audio encoder name, if the output has sound.
    pub audio: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoPlan {
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub quality: Quality,
    pub hardware: bool,
}

impl Plan {
    /// Works out the output from the Output node. `sound` says whether the
    /// sequence has any audio clips.
    pub fn new(o: &OutputSettings, seq: &Sequence, sound: bool) -> Result<Self, ExportError> {
        let muxer = match o.container.as_str() {
            "MP4" => "mp4",
            "MOV" => "mov",
            "MKV" => "matroska",
            "WebM" => "webm",
            "PNG sequence" => "image2",
            "WAV" => "wav",
            other => return Err(ExportError::Settings(format!("unknown container {other}"))),
        };
        let video = if o.has_video() {
            let codec = VideoCodec::from_name(&o.codec)
                .ok_or_else(|| ExportError::Settings(format!("unknown codec {}", o.codec)))?;
            if !videode_core::container_codecs(&o.container).contains(&o.codec.as_str()) {
                return Err(ExportError::Settings(format!(
                    "{} can't hold {}",
                    o.container, o.codec
                )));
            }
            let (w, h) = resolution(&o.resolution, seq)?;
            // 4:2:0 and 4:2:2 need even sizes.
            let even = |v: u32| (v.max(2) + 1) & !1;
            let (width, height) = if codec == VideoCodec::Png {
                (w.max(1), h.max(1))
            } else {
                (even(w), even(h))
            };
            Some(VideoPlan {
                codec,
                width,
                height,
                quality: Quality::from_name(&o.quality),
                hardware: o.hardware != "Off",
            })
        } else {
            None
        };
        let audio = match (sound || video.is_none(), muxer) {
            (false, _) | (_, "image2") => None,
            (true, "wav") => Some("pcm_s24le"),
            (true, "webm") => Some("libopus"),
            (true, _)
                if matches!(
                    video,
                    Some(VideoPlan {
                        codec: VideoCodec::ProRes { .. },
                        ..
                    })
                ) =>
            {
                Some("pcm_s24le")
            }
            (true, _) => Some("aac"),
        };
        Ok(Self {
            muxer,
            video,
            audio,
        })
    }
}

/// `Sequence` or `W×H`.
fn resolution(text: &str, seq: &Sequence) -> Result<(u32, u32), ExportError> {
    if text == "Sequence" {
        return Ok((seq.width, seq.height));
    }
    let bad = || ExportError::Settings(format!("unknown resolution {text}"));
    let (w, h) = text.split_once(['×', 'x']).ok_or_else(bad)?;
    let w: u32 = w.trim().parse().map_err(|_| bad())?;
    let h: u32 = h.trim().parse().map_err(|_| bad())?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return Err(bad());
    }
    Ok((w, h))
}

/// Bits per second for encoders that only take a bitrate.
pub fn target_bitrate(plan: &VideoPlan, fps: f64) -> usize {
    let bits_per_pixel = match plan.quality {
        Quality::Draft => 0.06,
        Quality::Standard => 0.12,
        Quality::High | Quality::Lossless => 0.25,
    };
    let efficiency = match plan.codec {
        VideoCodec::H265 | VideoCodec::Av1 | VideoCodec::Vp9 => 0.6,
        _ => 1.,
    };
    (plan.width as f64 * plan.height as f64 * fps * bits_per_pixel * efficiency) as usize
}

/// A rough size in bytes for `secs` of output, for the summary.
pub fn estimate_bytes(plan: &Plan, fps: f64, secs: f64) -> u64 {
    let video = plan.video.as_ref().map_or(0., |v| {
        let pixels = v.width as f64 * v.height as f64 * fps;
        match v.codec {
            VideoCodec::ProRes { hq: true } => pixels * 3.5,
            VideoCodec::ProRes { hq: false } => pixels * 2.4,
            VideoCodec::Png => pixels * 12.,
            _ if v.quality == Quality::Lossless => pixels * 4.,
            _ => target_bitrate(v, fps) as f64,
        }
    });
    let audio = match plan.audio {
        Some("aac") => 320_000.,
        Some("libopus") => 192_000.,
        Some(_) => 48_000. * 2. * 24.,
        None => 0.,
    };
    ((video + audio) * secs / 8.) as u64
}

/// One line on the sound, e.g. `AAC · 48 kHz · stereo · 320 kb/s`.
pub fn audio_text(plan: &Plan) -> String {
    match plan.audio {
        Some("aac") => "AAC · 48 kHz · stereo · 320 kb/s".into(),
        Some("libopus") => "Opus · 48 kHz · stereo · 192 kb/s".into(),
        Some(_) => "PCM · 48 kHz · stereo · 24-bit".into(),
        None => "None".into(),
    }
}

/// Encoder options for one encoder: rate control and speed.
pub(crate) fn options(
    encoder: &str,
    plan: &VideoPlan,
    fps: f64,
) -> (Vec<(&'static str, String)>, Option<usize>) {
    let q = plan.quality;
    let crf = |draft: u32, standard: u32, high: u32| {
        match q {
            Quality::Draft => draft,
            Quality::Standard => standard,
            _ => high,
        }
        .to_string()
    };
    let speed = if q == Quality::Draft {
        "veryfast"
    } else {
        "medium"
    };
    let mut o: Vec<(&'static str, String)> = Vec::new();
    let mut bitrate = None;
    match encoder {
        "libx264" => {
            o.push(("preset", speed.into()));
            if q == Quality::Lossless {
                o.push(("qp", "0".into()));
            } else {
                o.push(("crf", crf(26, 22, 18)));
            }
        }
        "libx265" => {
            o.push(("preset", speed.into()));
            if q == Quality::Lossless {
                o.push(("x265-params", "lossless=1:log-level=error".into()));
            } else {
                o.push(("crf", crf(30, 26, 21)));
                o.push(("x265-params", "log-level=error".into()));
            }
        }
        "libsvtav1" => {
            o.push((
                "preset",
                if q == Quality::Draft { "10" } else { "7" }.into(),
            ));
            o.push((
                "crf",
                if q == Quality::Lossless {
                    "1".into()
                } else {
                    crf(40, 33, 26)
                },
            ));
        }
        "libaom-av1" => {
            o.push(("cpu-used", "6".into()));
            o.push(("row-mt", "1".into()));
            o.push((
                "crf",
                if q == Quality::Lossless {
                    "0".into()
                } else {
                    crf(40, 33, 26)
                },
            ));
            bitrate = Some(0);
        }
        "libvpx-vp9" => {
            o.push(("deadline", "good".into()));
            o.push((
                "cpu-used",
                if q == Quality::Draft { "8" } else { "4" }.into(),
            ));
            o.push(("row-mt", "1".into()));
            if q == Quality::Lossless {
                o.push(("lossless", "1".into()));
            } else {
                o.push(("crf", crf(40, 34, 28)));
            }
            bitrate = Some(0);
        }
        "prores_ks" => {
            o.push((
                "profile",
                if matches!(plan.codec, VideoCodec::ProRes { hq: true }) {
                    "3"
                } else {
                    "2"
                }
                .into(),
            ));
            o.push(("vendor", "apl0".into()));
        }
        "prores_videotoolbox" => {
            o.push((
                "profile",
                if matches!(plan.codec, VideoCodec::ProRes { hq: true }) {
                    "hq"
                } else {
                    "standard"
                }
                .into(),
            ));
        }
        "png" | "libopenh264" => {}
        hw => {
            bitrate = Some(target_bitrate(plan, fps));
            if hw.contains("nvenc") {
                o.push(("preset", "p5".into()));
                o.push(("rc", "vbr".into()));
            } else if hw.ends_with("_mf") {
                o.push(("hw_encoding", "1".into()));
            }
        }
    }
    if encoder == "libopenh264" {
        bitrate = Some(target_bitrate(plan, fps));
    }
    (o, bitrate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use videode_core::Project;

    #[test]
    fn plans_follow_the_output_node() {
        let mut p = Project::new("t");
        let seq = p.sequence.clone();
        let plan = Plan::new(&p.output, &seq, true).unwrap();
        assert_eq!(plan.muxer, "mp4");
        assert_eq!(plan.audio, Some("aac"));
        let v = plan.video.unwrap();
        assert_eq!((v.width, v.height), (1920, 1080));
        assert!(v.hardware);

        p.output.container = "MOV".into();
        p.output.codec = "ProRes 422 HQ".into();
        p.output.resolution = "1281×721".into();
        let plan = Plan::new(&p.output, &seq, true).unwrap();
        assert_eq!(plan.audio, Some("pcm_s24le"));
        let v = plan.video.unwrap();
        assert_eq!(v.codec, VideoCodec::ProRes { hq: true });
        assert_eq!((v.width, v.height), (1282, 722), "even sizes");

        p.output.container = "WAV".into();
        p.output.codec = "PCM".into();
        let plan = Plan::new(&p.output, &seq, false).unwrap();
        assert!(plan.video.is_none());
        assert_eq!(plan.audio, Some("pcm_s24le"), "WAV always has sound");

        p.output.container = "WebM".into();
        p.output.codec = "H.264".into();
        assert!(Plan::new(&p.output, &seq, false).is_err());
    }

    #[test]
    fn lossless_skips_hardware() {
        let c = VideoCodec::H264;
        assert_eq!(c.candidates(true, Quality::High)[0], "h264_videotoolbox");
        assert_eq!(c.candidates(true, Quality::Lossless)[0], "libx264");
        assert_eq!(c.candidates(false, Quality::High)[0], "libx264");
        assert!(is_hardware("hevc_nvenc") && !is_hardware("libx265"));
    }
}
