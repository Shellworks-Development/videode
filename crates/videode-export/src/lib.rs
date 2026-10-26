//! Renders a sequence to a file, as the Output node describes: pictures
//! from the compositor, sound from the mixer, encoded with FFmpeg.

pub mod codecs;
mod compose;
mod encode;
mod text;

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use videode_audio::{Mix, Mixer};
use videode_core::{Frame, MediaId, Project};
use videode_media::MediaError;
use videode_media::audio::{CHANNELS, RATE};

pub use codecs::{
    Plan, Quality, VideoCodec, audio_text, encoder_label, estimate_bytes, is_hardware,
};
pub use compose::Compositor;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ExportError {
    #[error("cancelled")]
    Cancelled,
    #[error("nothing to export")]
    Empty,
    #[error("{0}")]
    Settings(String),
    #[error("no encoder: {0}")]
    NoEncoder(String),
    #[error("{0}")]
    Ffmpeg(String),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Media(#[from] MediaError),
}

/// One export: a snapshot of the project and where its files are.
#[derive(Clone)]
pub struct Job {
    pub project: Project,
    /// Every online media file.
    pub paths: HashMap<MediaId, PathBuf>,
    pub mix: Mix,
    /// Sequence frames to export.
    pub range: Range<Frame>,
    /// The file to write. For image sequences, frame numbers are added to
    /// its name.
    pub path: PathBuf,
}

/// Shared with the UI while a job runs.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicU64,
    cancel: AtomicBool,
}

impl Progress {
    /// Frames finished so far.
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// The video encoder used, if any.
    pub encoder: Option<&'static str>,
    /// Bytes written (all frames, for image sequences).
    pub bytes: u64,
}

/// Where an image sequence's frames go: `name_000001.png` next to `path`.
pub fn sequence_pattern(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frame".into());
    path.with_file_name(format!("{stem}_%06d.png"))
}

fn sequence_frame(pattern: &Path, n: u64) -> PathBuf {
    let name = pattern
        .file_name()
        .map(|s| s.to_string_lossy().replace("%06d", &format!("{n:06}")))
        .unwrap_or_default();
    pattern.with_file_name(name)
}

/// Renders `job` on the calling thread. Writes to a temporary file and
/// moves it into place at the end, so a cancelled or failed export never
/// leaves a broken file or replaces a good one.
pub fn run(job: &Job, progress: &Progress) -> Result<Report, ExportError> {
    videode_media::init();
    let p = &job.project;
    let plan = Plan::new(&p.output, &p.sequence, !job.mix.clips.is_empty())?;
    if job.range.is_empty() {
        return Err(ExportError::Empty);
    }
    if let Some(dir) = job.path.parent()
        && !dir.as_os_str().is_empty()
        && !dir.is_dir()
    {
        return Err(ExportError::Io(format!("{} doesn't exist", dir.display())));
    }
    let images = plan.muxer == "image2";
    let target = if images {
        sequence_pattern(&job.path)
    } else {
        temp_path(&job.path)
    };
    let result = render(job, &plan, &target, progress);
    match result {
        Ok(encoder) if images => {
            let frames = (job.range.end - job.range.start) as u64;
            let bytes = (1..=frames)
                .filter_map(|n| std::fs::metadata(sequence_frame(&target, n)).ok())
                .map(|m| m.len())
                .sum();
            Ok(Report { encoder, bytes })
        }
        Ok(encoder) => {
            std::fs::rename(&target, &job.path).map_err(|e| {
                std::fs::remove_file(&target).ok();
                ExportError::Io(format!("couldn't write {}: {e}", job.path.display()))
            })?;
            let bytes = std::fs::metadata(&job.path).map(|m| m.len()).unwrap_or(0);
            Ok(Report { encoder, bytes })
        }
        Err(e) => {
            if images {
                for n in 1..=progress.done() + 1 {
                    std::fs::remove_file(sequence_frame(&target, n)).ok();
                }
            } else {
                std::fs::remove_file(&target).ok();
            }
            Err(e)
        }
    }
}

/// `.name.part` in the same folder, so the final move is a rename.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export".into());
    path.with_file_name(format!(".{name}.part"))
}

fn render(
    job: &Job,
    plan: &Plan,
    target: &Path,
    progress: &Progress,
) -> Result<Option<&'static str>, ExportError> {
    let p = &job.project;
    let fps = p.sequence.fps;
    let mut writer = encode::Writer::open(target, plan, fps)?;
    let mut compositor = plan
        .video
        .as_ref()
        .map(|v| Compositor::new(p, &job.paths, v.width, v.height));
    let mut mixer = writer.has_audio().then(|| {
        let mut m = Mixer::new(RATE);
        m.set_mix(job.mix.clone());
        m
    });
    let start_secs = fps.frames_to_seconds(job.range.start);
    // Sound up to the end of frame `n`, in samples, without drift.
    let samples_until =
        |n: i64| (n as i128 * fps.den as i128 * RATE as i128 / fps.num as i128) as i64;
    let mut buf = Vec::new();
    for (i, at) in job.range.clone().enumerate() {
        if progress.cancelled() {
            return Err(ExportError::Cancelled);
        }
        let i = i as i64;
        if let Some(c) = &mut compositor {
            let picture = c.frame(at)?;
            writer.video(&picture, i)?;
        }
        if let Some(m) = &mut mixer {
            let from = samples_until(i);
            let n = (samples_until(i + 1) - from) as usize;
            buf.resize(n * CHANNELS, 0.);
            m.render(start_secs + from as f64 / RATE as f64, &mut buf);
            writer.audio(&buf)?;
        }
        progress.done.store(i as u64 + 1, Ordering::Relaxed);
    }
    let encoder = writer.encoder;
    writer.finish()?;
    Ok(encoder)
}

/// The encoders that work on this machine for `codec`, best first, found
/// by opening each one on a small picture. Takes a moment; run it off the
/// UI thread.
pub fn available_encoders(codec: VideoCodec) -> Vec<&'static str> {
    videode_media::init();
    let plan = codecs::VideoPlan {
        codec,
        width: 640,
        height: 360,
        quality: Quality::High,
        hardware: true,
    };
    codec
        .candidates(true, Quality::High)
        .into_iter()
        .filter(|name| encode::try_video(name, &plan, videode_core::Fps::FPS_24, false).is_ok())
        .collect()
}

/// Every encoder found, keyed by codec name as the Output node says it.
pub fn detect_all() -> HashMap<&'static str, Vec<&'static str>> {
    let names = ["H.264", "H.265", "AV1", "VP9", "ProRes 422", "PNG"];
    // Encoders that aren't there complain as they fail to open.
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Quiet);
    let found = names
        .into_iter()
        .filter_map(|n| Some((n, available_encoders(VideoCodec::from_name(n)?))))
        .collect();
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);
    found
}

#[cfg(test)]
mod tests;
