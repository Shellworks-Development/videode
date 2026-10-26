//! Footage for the "Signal Study" demo project: synthetic test patterns made
//! with the `ffmpeg` command-line tool's built-in sources, matching the clips
//! in the HTML mockup. `bars_tone.mov` is deliberately never made, so the
//! demo shows what offline media looks like.

use std::path::Path;
use std::process::Command;

/// `(file name, ffmpeg arguments after the output-independent setup)`.
fn jobs() -> Vec<(&'static str, Vec<String>)> {
    let video = |filter: &str| -> Vec<String> {
        [
            "-f", "lavfi", "-i", filter, "-t", "16", "-c:v", "libx264", "-preset", "veryfast",
            "-crf", "26", "-g", "24", "-pix_fmt", "yuv420p",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let audio = |expr: &str, seconds: &str| -> Vec<String> {
        [
            "-f",
            "lavfi",
            "-i",
            expr,
            "-t",
            seconds,
            "-c:a",
            "pcm_s16le",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    vec![
        (
            "dusk_gradient.mov",
            video(
                "gradients=s=1280x720:r=24:speed=0.01:c0=0x2b1a4a:c1=0xff7a3d:c2=0x1d3b6e:nb_colors=3",
            ),
        ),
        (
            "mandel_dive.mp4",
            video("mandelbrot=s=1280x720:r=24:maxiter=256:end_scale=0.002:end_pts=384"),
        ),
        (
            "colony_sim.mov",
            video(
                "life=s=320x180:r=24:mold=10:ratio=0.08:life_color=#5bc85b:death_color=#b02040,scale=1280:720:flags=neighbor",
            ),
        ),
        (
            "zoneplate_ref.mov",
            video("zoneplate=s=1280x720:r=24:ku=256:kv=256:kt2=8"),
        ),
        (
            "rule110.mov",
            video(
                "cellauto=s=320x180:r=24:rule=110:scroll=1:random_fill_ratio=0.5,scale=1280:720:flags=neighbor",
            ),
        ),
        (
            "voiceover_t3.wav",
            audio(
                "aevalsrc=0.4*sin(2*PI*(170+30*sin(2*PI*0.7*t))*t)*(0.5+0.5*sin(2*PI*2.3*t)):s=48000",
                "40",
            ),
        ),
        (
            "pad_drone.wav",
            audio(
                "aevalsrc=0.2*sin(2*PI*110*t)+0.15*sin(2*PI*165*t)|0.2*sin(2*PI*110.5*t)+0.15*sin(2*PI*220*t):s=48000",
                "60",
            ),
        ),
    ]
}

/// Whether the `ffmpeg` command-line tool can be run.
pub fn ffmpeg_cli_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Files the demo needs that aren't in `dir` yet.
pub fn missing_demo_media(dir: &Path) -> Vec<&'static str> {
    jobs()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !dir.join(name).exists())
        .collect()
}

/// Writes any missing demo files into `dir`, calling `progress(done, total)`
/// after each. Each file is written under a temporary name and renamed, so
/// an interrupted run never leaves a half-written clip behind.
pub fn generate_demo_media(
    dir: &Path,
    mut progress: impl FnMut(usize, usize),
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    let todo: Vec<_> = jobs()
        .into_iter()
        .filter(|(name, _)| !dir.join(name).exists())
        .collect();
    let total = todo.len();
    for (i, (name, args)) in todo.into_iter().enumerate() {
        let out = dir.join(name);
        let tmp = dir.join(format!(".partial-{name}"));
        let status = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-nostdin"])
            .args(&args)
            .arg("-f")
            .arg(if name.ends_with(".wav") {
                "wav"
            } else if name.ends_with(".mp4") {
                "mp4"
            } else {
                "mov"
            })
            .arg(&tmp)
            .status()
            .map_err(|e| format!("couldn't run ffmpeg: {e}"))?;
        if !status.success() {
            std::fs::remove_file(&tmp).ok();
            return Err(format!("ffmpeg failed making {name}"));
        }
        std::fs::rename(&tmp, &out)
            .map_err(|e| format!("couldn't write {}: {e}", out.display()))?;
        progress(i + 1, total);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slow (about ten seconds): `cargo test -p videode-media -- --ignored`.
    #[test]
    #[ignore]
    fn generates_every_demo_file() {
        let dir = std::env::temp_dir().join("videode-demo-media-test");
        std::fs::remove_dir_all(&dir).ok();
        let mut calls = 0;
        generate_demo_media(&dir, |_, _| calls += 1).unwrap();
        assert_eq!(calls, jobs().len());
        assert!(missing_demo_media(&dir).is_empty());
        for (name, _) in jobs() {
            let p = crate::probe(&dir.join(name)).unwrap();
            let expected = match name {
                "voiceover_t3.wav" => 40.,
                "pad_drone.wav" => 60.,
                _ => 16.,
            };
            assert!(
                (p.duration.unwrap() - expected).abs() < 0.1,
                "{name}: {p:?}"
            );
        }
    }
}
