//! End to end: export small sequences, then probe and decode what was
//! written. Needs the `ffmpeg` CLI for fixtures; skips without it.

use std::process::Command;
use std::sync::OnceLock;

use videode_audio::{ClipSound, TrackSound};
use videode_core::{MediaKind, NodeKind, Value};
use videode_media::{Kind, VideoDecoder, probe};

use super::*;

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("videode-export-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ffmpeg(args: &[&str], out: &Path) -> Option<PathBuf> {
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(args)
        .arg(out)
        .status()
        .ok()?
        .success();
    ok.then(|| out.to_path_buf())
}

/// 2 s of flat red at 24 fps, 320×180.
fn red() -> Option<PathBuf> {
    static F: OnceLock<Option<PathBuf>> = OnceLock::new();
    F.get_or_init(|| {
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=0xd02020:s=320x180:r=24:d=2",
                "-c:v",
                "libx264",
                "-qp",
                "0",
                "-pix_fmt",
                "yuv444p",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-colorspace",
                "bt709",
            ],
            &dir().join("red.mp4"),
        )
    })
    .clone()
}

/// 2 s of a constant 0.25 at 48 kHz stereo.
fn tone() -> Option<PathBuf> {
    static F: OnceLock<Option<PathBuf>> = OnceLock::new();
    F.get_or_init(|| {
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=0.25|0.25:s=48000:d=2",
                "-c:a",
                "pcm_f32le",
            ],
            &dir().join("tone.wav"),
        )
    })
    .clone()
}

/// A 320×180 sequence: red on V1 for 24 frames, a title on V2, and the
/// tone on A1.
fn job(container: &str, codec: &str, name: &str) -> Option<Job> {
    let (red, tone) = (red()?, tone()?);
    let mut p = Project::new("t");
    p.sequence.width = 320;
    p.sequence.height = 180;
    let v = p.add_media(&red, MediaKind::Video, Some(48), "H.264", "");
    let a = p.add_media(&tone, MediaKind::Audio, Some(48), "PCM", "");
    let v1 = p.track_by_label("V1").unwrap().id;
    let v2 = p.track_by_label("V2").unwrap().id;
    let vc = p.add_media_clip(v, Some(v1), 0).unwrap();
    p.add_media_clip(a, None, 0).unwrap();
    let t = p.add_text_clip(v2, 0, 48, "Hi").unwrap();
    let tn = p.clip(t).unwrap().nodes[0].id;
    p.set_param(t, tn, "size", Value::Number(40.)).unwrap();
    // The title sits low, so the middle of the frame stays red.
    let tr = p.add_node(t, NodeKind::Transform).unwrap();
    p.set_param(t, tr, "y", Value::Number(60.)).unwrap();
    p.set_output(videode_core::OutputField::Container, container)
        .unwrap();
    p.set_output(videode_core::OutputField::Codec, codec)
        .unwrap();
    p.set_output(videode_core::OutputField::Hardware, "Off")
        .unwrap();
    let a1 = p.track_by_label("A1").unwrap().id;
    let mix = Mix {
        clips: p
            .sequence
            .clips
            .iter()
            .filter(|c| c.track == a1)
            .map(|c| ClipSound {
                path: tone.clone(),
                track: 0,
                start: 0.,
                end: c.duration as f64 / 24.,
                source_in: 0.,
                gain: 1.,
                fade_in: 0.,
                fade_out: 0.,
            })
            .collect(),
        tracks: vec![TrackSound { muted: false }],
    };
    let paths = [(v, red), (a, tone)].into_iter().collect();
    let range = 0..p.sequence.clips.iter().find(|c| c.id == vc).unwrap().end();
    Some(Job {
        project: p,
        paths,
        mix,
        range,
        path: dir().join(name),
    })
}

fn centre(pic: &videode_media::Picture) -> [u8; 3] {
    let o = (((pic.height / 2) * pic.width + pic.width / 2) * 4) as usize;
    [pic.bgra[o], pic.bgra[o + 1], pic.bgra[o + 2]]
}

fn close(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (*x as i32 - y as i32).abs() <= tol)
}

#[test]
fn exports_h264_with_sound_and_colour_intact() {
    let Some(job) = job("MP4", "H.264", "out.mp4") else {
        eprintln!("skipped: ffmpeg CLI not available");
        return;
    };
    let progress = Progress::default();
    let report = run(&job, &progress).unwrap();
    assert_eq!(report.encoder, Some("libx264"));
    assert_eq!(progress.done(), 48);
    assert!(report.bytes > 0);
    assert!(!temp_path(&job.path).exists(), "temp file moved into place");

    let info = probe(&job.path).unwrap();
    assert_eq!(info.kind, Kind::Video);
    assert_eq!((info.width, info.height), (Some(320), Some(180)));
    assert_eq!(info.codec, "H.264");
    assert!((info.duration.unwrap() - 2.).abs() < 0.1, "{info:?}");
    assert_eq!(info.audio.as_ref().map(|a| a.codec.as_str()), Some("AAC"));

    let mut d = VideoDecoder::open(&job.path).unwrap();
    let source = VideoDecoder::open(&red().unwrap())
        .unwrap()
        .frame_at(0.5, 320, 180)
        .unwrap();
    let out = d.frame_at(0.5, 320, 180).unwrap();
    assert!(
        close(centre(&out), centre(&source), 6),
        "{:?} vs {:?}",
        centre(&out),
        centre(&source)
    );
    // The title is white, low in the frame.
    let lit = out.bgra[(out.width * 4 * 120) as usize..]
        .chunks(4)
        .filter(|px| px.iter().take(3).all(|&c| c > 220))
        .count();
    assert!(lit > 50, "{lit} white pixels for the title");
}

#[test]
fn exports_prores_wav_and_png_sequences() {
    let Some(job) = job("MOV", "ProRes 422 HQ", "out.mov") else {
        return;
    };
    let report = run(&job, &Progress::default()).unwrap();
    assert_eq!(report.encoder, Some("prores_ks"));
    let info = probe(&job.path).unwrap();
    assert_eq!(info.codec, "ProRes");
    assert_eq!(info.audio.unwrap().codec, "PCM");

    let job = self::job("WAV", "PCM", "out.wav").unwrap();
    run(&job, &Progress::default()).unwrap();
    let info = probe(&job.path).unwrap();
    assert_eq!(info.kind, Kind::Audio);
    assert!((info.duration.unwrap() - 2.).abs() < 0.05, "{info:?}");
    let mut sound = videode_media::audio::AudioDecoder::open(&job.path).unwrap();
    let mut buf = vec![0.; 2048];
    sound.read(&mut buf).unwrap();
    assert!((buf[1000] - 0.25).abs() < 0.001, "{}", buf[1000]);

    let mut job = self::job("PNG sequence", "PNG", "frames.png").unwrap();
    job.range = 10..13;
    let report = run(&job, &Progress::default()).unwrap();
    let pattern = sequence_pattern(&job.path);
    let frames: Vec<_> = (1..=3).map(|n| sequence_frame(&pattern, n)).collect();
    assert!(frames.iter().all(|f| f.exists()), "{frames:?}");
    assert!(!sequence_frame(&pattern, 4).exists());
    assert!(report.bytes > 0);
}

#[test]
fn cancelling_leaves_nothing_behind() {
    let Some(job) = job("MP4", "H.264", "cancelled.mp4") else {
        return;
    };
    let progress = Progress::default();
    progress.cancel();
    assert_eq!(run(&job, &progress), Err(ExportError::Cancelled));
    assert!(!job.path.exists());
    assert!(!temp_path(&job.path).exists());

    let mut empty = job.clone();
    empty.range = 5..5;
    assert_eq!(run(&empty, &Progress::default()), Err(ExportError::Empty));
}
