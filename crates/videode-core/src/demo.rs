//! "Signal Study", the demo project from the HTML mockup. Its media are
//! synthetic test patterns that the app generates on first open (see
//! `videode_media::demo`); `bars_tone.mov` is never made, to show offline
//! media.

use crate::kdl_io::from_kdl;
use crate::project::Project;

pub const DEMO_KDL: &str = r#"
videode version=1
project "Signal Study"
media {
    file id=1 path="media/dusk_gradient.mov" kind=video duration=384 codec=H.264 format="1280×720 · 24 fps"
    file id=2 path="media/mandel_dive.mp4" kind=video duration=384 codec=H.264 format="1280×720 · 24 fps"
    file id=3 path="media/colony_sim.mov" kind=video duration=384 codec=H.264 format="1280×720 · 24 fps"
    file id=4 path="media/zoneplate_ref.mov" kind=video duration=384 codec=H.264 format="1280×720 · 24 fps"
    file id=5 path="media/rule110.mov" kind=video duration=384 codec=H.264 format="1280×720 · 24 fps"
    file id=6 path="media/bars_tone.mov" kind=video duration=384 codec="ProRes 422" format="1920×1080" status=offline
    file id=7 path="media/voiceover_t3.wav" kind=audio duration=960 codec=PCM format="Mono · 48 kHz"
    file id=8 path="media/pad_drone.wav" kind=audio duration=1440 codec=PCM format="Stereo · 48 kHz"
}
sequence "Main" fps=24 fps-den=1 width=1920 height=1080 {
    track V2 kind=video name=Titles {
        clip start=24 duration=108 {
            text text="Signal Study" size=120 weight="600"
            transform y=-40
            fade in=12 out=18
        }
        clip start=600 duration=96 {
            text text="part two — emergence" size=64 weight="500"
            transform y=360
            fade in=8 out=8
        }
    }
    track V1 kind=video name=Picture {
        clip start=0 duration=168 media=1 in=0 {
            source
            grade exposure=-0.2 temperature=5200
            fade in=12 out=0
        }
        clip start=168 duration=192 media=2 in=48 {
            source
            transform scale=108 y=-42
            grade exposure=0.35 temperature=5600 saturation=112
        }
        clip start=360 duration=240 media=3 in=0 {
            source
            grade saturation=80 contrast=112
            blur radius=1.5
        }
        clip start=600 duration=144 media=4 in=24 {
            source
            crop top=6 bottom=6
            opacity opacity=85
        }
        clip start=744 duration=168 media=5 in=0 {
            source
            grade exposure=0.1 temperature=7200
        }
        clip start=912 duration=120 media=6 in=0 {
            source
        }
    }
    track A1 kind=audio name=Voice {
        clip start=24 duration=840 media=7 in=0 {
            source
            gain gain=-3
        }
    }
    track A2 kind=audio name=Music {
        clip start=0 duration=1032 media=8 in=96 {
            source
            gain gain=-6
            duck under=A1 amount=-12
            fade in=24 out=48
        }
    }
}
output container=MP4 codec=H.264 resolution="1920×1080" quality=High path="signal_study.mp4"
"#;

pub fn demo_project() -> Project {
    from_kdl(DEMO_KDL).expect("the bundled demo project is valid")
}
