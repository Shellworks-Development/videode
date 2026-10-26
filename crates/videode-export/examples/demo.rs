//! Exports the demo project with its generated footage, for timing:
//! `cargo run -p videode-export --example demo -- <demo media folder> out.mp4`

use std::time::Instant;

use videode_audio::{ClipSound, Mix, TrackSound};
use videode_core::{ClipKind, MediaKind, StreamKind, demo::demo_project};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = std::path::PathBuf::from(args.next().expect("demo media folder"));
    let out = std::path::PathBuf::from(args.next().unwrap_or_else(|| "/tmp/demo.mp4".into()));
    let mut project = demo_project();
    project.output.hardware = "Off".into();
    let paths: std::collections::HashMap<_, _> = project
        .media
        .iter_mut()
        .filter_map(|m| {
            let path = root.join(&m.path);
            m.offline = !path.exists();
            (!m.offline).then_some((m.id, path))
        })
        .collect();
    let fps = project.sequence.fps;
    let audio: Vec<_> = project
        .sequence
        .tracks
        .iter()
        .filter(|t| t.kind == StreamKind::Audio)
        .collect();
    let clips = project
        .sequence
        .clips
        .iter()
        .filter(|c| c.kind == ClipKind::Audio)
        .filter_map(|c| {
            let m = project.media(c.media?)?;
            (m.kind == MediaKind::Audio).then_some(())?;
            Some(ClipSound {
                path: paths.get(&m.id)?.clone(),
                track: audio.iter().position(|t| t.id == c.track)?,
                start: fps.frames_to_seconds(c.start),
                end: fps.frames_to_seconds(c.end()),
                source_in: fps.frames_to_seconds(c.source_in),
                gain: 1.,
                fade_in: 0.,
                fade_out: 0.,
            })
        })
        .collect();
    let mix = Mix {
        clips,
        tracks: audio
            .iter()
            .map(|t| TrackSound { muted: !t.enabled })
            .collect(),
    };
    let range = 0..project.end();
    let frames = range.end - range.start;
    let job = videode_export::Job {
        project,
        paths,
        mix,
        range,
        path: out.clone(),
    };
    let t = Instant::now();
    let report = videode_export::run(&job, &videode_export::Progress::default()).unwrap();
    let secs = t.elapsed().as_secs_f64();
    println!(
        "{} frames in {secs:.1} s ({:.0} fps) · {} bytes · {:?} → {}",
        frames,
        frames as f64 / secs,
        report.bytes,
        report.encoder,
        out.display()
    );
}
