//! Sound in the app: the sequence's audio clips as a mix for the output
//! engine, the engine as the playback clock, level meters, and waveforms
//! for audio clips on the timeline.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{App, Context, Global};
use videode_audio::{AudioEngine, ClipSound, Mix, TrackSound, db_to_gain};
use videode_core::{ClipKind, Media, MediaKind, NodeKind, StreamKind};

use crate::workspace::Workspace;

/// Waveform resolution, in peaks per second of audio.
pub const PEAKS_PER_SECOND: u32 = 50;

pub enum Wave {
    Loading,
    Ready(Arc<Vec<f32>>),
    Failed,
}

pub enum Output {
    /// Opened on first play, so launching never waits on the device.
    NotStarted,
    Ready(AudioEngine),
    /// No usable device; playback runs on the wall clock in silence.
    Unavailable,
}

pub struct Sound {
    pub output: Output,
    pub waves: HashMap<PathBuf, Wave>,
}

impl Global for Sound {}

pub fn init(cx: &mut App) {
    cx.set_global(Sound {
        // Tests never touch the real device.
        output: if cfg!(test) {
            Output::Unavailable
        } else {
            Output::NotStarted
        },
        waves: HashMap::new(),
    });
}

impl Sound {
    /// The output engine, opening it the first time.
    pub fn engine(&mut self) -> Option<&mut AudioEngine> {
        if matches!(self.output, Output::NotStarted) {
            self.output = match AudioEngine::start() {
                Ok(e) => Output::Ready(e),
                Err(e) => {
                    eprintln!("videode: no sound: {e}");
                    Output::Unavailable
                }
            };
        }
        match &mut self.output {
            Output::Ready(e) => Some(e),
            _ => None,
        }
    }
}

impl Workspace {
    /// The sequence's sound as the mixer sees it.
    pub(crate) fn build_mix(&self) -> Mix {
        let p = &self.doc.project;
        let fps = p.sequence.fps;
        let audio_tracks: Vec<_> = p
            .sequence
            .tracks
            .iter()
            .filter(|t| t.kind == StreamKind::Audio)
            .collect();
        let tracks = audio_tracks
            .iter()
            .map(|t| TrackSound { muted: !t.enabled })
            .collect();
        let clips = p
            .sequence
            .clips
            .iter()
            .filter(|c| c.kind == ClipKind::Audio)
            .filter_map(|c| {
                let track = audio_tracks.iter().position(|t| t.id == c.track)?;
                let m = p.media(c.media?)?;
                if m.offline || m.kind != MediaKind::Audio {
                    return None;
                }
                let live = c.nodes.iter().filter(|n| !n.bypass);
                let gain = live
                    .clone()
                    .filter(|n| n.kind == NodeKind::Gain)
                    .map(|n| db_to_gain(n.number("gain")))
                    .product();
                let fade = live.clone().find(|n| n.kind == NodeKind::Fade);
                let fade_secs =
                    |key| fade.map_or(0., |n| fps.frames_to_seconds(n.number(key) as i64));
                Some(ClipSound {
                    path: self.media_path(m),
                    track,
                    start: fps.frames_to_seconds(c.start),
                    end: fps.frames_to_seconds(c.end()),
                    source_in: fps.frames_to_seconds(c.source_in),
                    gain,
                    fade_in: fade_secs("in"),
                    fade_out: fade_secs("out"),
                })
            })
            .collect();
        Mix { clips, tracks }
    }

    /// Keeps the output engine in step with playback: starts it when normal
    /// speed playback starts or jumps, sends edits, and stops it after.
    /// Returns the sequence time it is playing, to drive the playhead.
    pub(crate) fn sync_sound(&mut self, cx: &mut Context<Self>) -> Option<f64> {
        let wants = self.playback.as_ref().is_some_and(|pb| pb.speed == 1.);
        if !wants {
            if self.sound_live {
                self.sound_live = false;
                if let Some(e) = cx.global_mut::<Sound>().engine() {
                    e.stop();
                }
            }
            return None;
        }
        let restart = self.playback.as_ref().is_some_and(|pb| !pb.sound_started);
        if restart {
            let at = self.fps().frames_to_seconds(self.playhead);
            let mix = self.build_mix();
            self.sent_mix = Some(mix.clone());
            let engine = cx.global_mut::<Sound>().engine();
            self.sound_live = engine.is_some();
            if let Some(e) = engine {
                e.play(at, mix);
            }
            if let Some(pb) = &mut self.playback {
                pb.sound_started = true;
            }
        } else if self.sound_live {
            let mix = self.build_mix();
            if self.sent_mix.as_ref() != Some(&mix) {
                if let Some(e) = cx.global_mut::<Sound>().engine() {
                    e.update(mix.clone());
                }
                self.sent_mix = Some(mix);
            }
        }
        if !self.sound_live {
            return None;
        }
        cx.global_mut::<Sound>().engine()?.position()
    }

    /// Updates the level meters; true while they still need redrawing.
    pub(crate) fn tick_meters(&mut self, cx: &mut Context<Self>) -> bool {
        let peaks = if self.sound_live {
            cx.global_mut::<Sound>()
                .engine()
                .map(|e| e.take_peaks())
                .unwrap_or_default()
        } else {
            [0.; 2]
        };
        for (level, peak) in self.meters.iter_mut().zip(peaks) {
            // Instant attack, smooth release.
            *level = peak.max(*level * 0.85);
            if *level < 0.001 {
                *level = 0.;
            }
        }
        self.meters.iter().any(|&l| l > 0.)
    }

    /// Waveform peaks for an audio file, decoding them in the background the
    /// first time.
    pub(crate) fn waveform(&self, m: &Media, cx: &mut Context<Self>) -> Option<Arc<Vec<f32>>> {
        if m.offline || m.kind != MediaKind::Audio {
            return None;
        }
        let path = self.media_path(m);
        match cx.global::<Sound>().waves.get(&path) {
            Some(Wave::Ready(w)) => return Some(w.clone()),
            Some(_) => return None,
            None => {}
        }
        cx.global_mut::<Sound>()
            .waves
            .insert(path.clone(), Wave::Loading);
        let task = cx.background_executor().spawn({
            let path = path.clone();
            async move { videode_media::audio::peaks(&path, PEAKS_PER_SECOND) }
        });
        cx.spawn(async move |this, cx| {
            let wave = match task.await {
                Ok(p) => Wave::Ready(Arc::new(p)),
                Err(_) => Wave::Failed,
            };
            this.update(cx, |_, cx| {
                cx.global_mut::<Sound>().waves.insert(path, wave);
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }
}
