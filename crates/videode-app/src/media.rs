//! Media in the app: importing files, generating the demo footage, bin
//! thumbnails, and the frame the viewer shows. Decoding happens on the
//! `videode_media` pool's threads; results come back as GPUI images.
//!
//! GPUI keeps every image it has drawn in its texture atlas until told to
//! drop it, so the viewer drops each frame it replaces.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{App, Context, Global, PathPromptOptions, RenderImage, Window};
use videode_core::look::look_at;
use videode_core::{Media, MediaKind, Project};
use videode_media::{FramePool, FrameRequest, Kind, MediaError, Picture, demo};
use videode_render::Fx;

use crate::actions::ImportMedia;
use crate::workspace::{BinTab, View, Workspace};

pub const THUMB_W: u32 = 160;
pub const THUMB_H: u32 = 90;
/// Viewer frames are decoded no larger than this.
const VIEWER_MAX: (u32, u32) = (1920, 1080);
const VIEWER_SLOT: u64 = 1;

pub enum Thumb {
    Loading,
    Ready(Arc<RenderImage>),
    Failed,
}

pub struct MediaEngine {
    pub pool: FramePool,
    pub thumbs: HashMap<PathBuf, Thumb>,
    /// Demo footage is being generated.
    pub generating: bool,
}

impl Global for MediaEngine {}

pub fn init(cx: &mut App) {
    cx.set_global(MediaEngine {
        // Three decoders and 384 MB of decoded frames.
        pool: FramePool::new(3, 384),
        thumbs: HashMap::new(),
        generating: false,
    });
}

/// Where the demo project's `media/` folder lives.
pub fn demo_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("videode")
        .join("demo")
}

/// A decoded picture as a GPUI image. Both are BGRA, so no conversion.
pub fn render_image(p: &Picture) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(p.width, p.height, p.bgra.clone())
        .expect("picture buffer matches its size");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// What the viewer wants on screen: a file, a time in it, and a size.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameKey {
    pub path: PathBuf,
    /// Milliseconds into the file.
    pub ms: i64,
    pub w: u32,
    pub h: u32,
}

pub struct ViewerFrame {
    pub key: FrameKey,
    /// The effects baked into `image`.
    pub fx: Fx,
    /// The decoded picture's size before rotation and crop.
    pub base: (u32, u32),
    pub image: Arc<RenderImage>,
}

fn slot_for(path: &Path) -> u64 {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    h.finish() | 1 << 63
}

impl Workspace {
    /// A media file's location on disk: absolute paths as written, relative
    /// ones against the project's folder.
    pub(crate) fn media_path(&self, m: &Media) -> PathBuf {
        if m.path.is_absolute() {
            m.path.clone()
        } else {
            self.media_root.join(&m.path)
        }
    }

    /// Marks missing files offline and reports how many are.
    pub(crate) fn refresh_media(&mut self) -> usize {
        let root = self.media_root.clone();
        self.doc.refresh_offline(|m| {
            let path = if m.path.is_absolute() {
                m.path.clone()
            } else {
                root.join(&m.path)
            };
            !path.exists()
        })
    }

    /// Before saving to `dir`, rewrites relative media paths that wouldn't
    /// resolve from there as absolute ones.
    pub(crate) fn rebase_media(&mut self, dir: &Path) {
        if dir == self.media_root {
            return;
        }
        let root = self.media_root.clone();
        self.doc.rebase_media(|m| {
            if m.path.is_relative() {
                m.path = root.join(&m.path);
            }
        });
        self.media_root = dir.to_path_buf();
    }

    /// Opens the demo, generating its footage first if it's missing.
    pub(crate) fn prepare_demo_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.media_root.join("media");
        let missing = demo::missing_demo_media(&dir);
        if missing.is_empty() {
            let offline = self.refresh_media();
            self.say(format!("Opened Signal Study · {offline} file offline"));
            return;
        }
        self.refresh_media();
        if !demo::ffmpeg_cli_available() {
            self.say_error("The demo needs the ffmpeg command-line tool");
            return;
        }
        if cx.global::<MediaEngine>().generating {
            return;
        }
        cx.global_mut::<MediaEngine>().generating = true;
        self.say(format!(
            "Generating demo footage ({} files)…",
            missing.len()
        ));
        let job = cx
            .background_executor()
            .spawn(async move { demo::generate_demo_media(&dir, |_, _| {}) });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            this.update(cx, |ws, cx| {
                cx.global_mut::<MediaEngine>().generating = false;
                let offline = ws.refresh_media();
                match result {
                    Ok(()) => ws.say(format!("Demo footage ready · {offline} file offline")),
                    Err(e) => ws.say_error(format!("Couldn't make the demo footage: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /* ---------- import ---------- */

    pub(crate) fn import_media(&mut self, _: &ImportMedia, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            this.update(cx, |ws, cx| ws.import_paths(paths, cx)).ok();
        })
        .detach();
    }

    /// Probes files in the background, then adds the readable ones to the
    /// bin as one undoable step.
    pub(crate) fn import_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.say(format!(
            "Importing {} file{}…",
            paths.len(),
            if paths.len() == 1 { "" } else { "s" }
        ));
        cx.notify();
        let probes = cx.background_executor().spawn(async move {
            paths
                .into_iter()
                .map(|p| {
                    let r = videode_media::probe(&p);
                    (p, r)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let results = probes.await;
            this.update(cx, |ws, cx| ws.finish_import(results, cx)).ok();
        })
        .detach();
    }

    fn finish_import(
        &mut self,
        results: Vec<(
            PathBuf,
            Result<videode_media::Probe, videode_media::MediaError>,
        )>,
        cx: &mut Context<Self>,
    ) {
        let fps = self.fps();
        let mut failed = Vec::new();
        let mut ok = Vec::new();
        for (path, r) in results {
            match r {
                Ok(p) => ok.push((path, p)),
                Err(e) => failed.push(format!(
                    "{}: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )),
            }
        }
        let existing: Vec<PathBuf> = self
            .project()
            .media
            .iter()
            .map(|m| self.media_path(m))
            .collect();
        ok.retain(|(path, _)| !existing.contains(path));
        let added = ok.len();
        if added > 0 {
            self.apply("import media", cx, |p: &mut Project| {
                for (path, probe) in &ok {
                    let kind = match probe.kind {
                        Kind::Video => MediaKind::Video,
                        Kind::Audio => MediaKind::Audio,
                        Kind::Image => MediaKind::Image,
                    };
                    let duration = probe.duration.map(|d| fps.seconds_to_frames(d).max(1));
                    p.add_media(
                        path.clone(),
                        kind,
                        duration,
                        probe.codec.clone(),
                        probe.format_text(),
                    );
                }
                Ok(())
            });
            self.bin_tab = BinTab::Media;
        }
        match (added, failed.len()) {
            (0, 0) => self.say("Already in the bin"),
            (n, 0) => self.say(format!(
                "Imported {n} file{}",
                if n == 1 { "" } else { "s" }
            )),
            (0, _) => self.say_error(format!("Couldn't import {}", failed.join(" · "))),
            (n, _) => self.say_error(format!(
                "Imported {n}; couldn't import {}",
                failed.join(" · ")
            )),
        }
        cx.notify();
    }

    /* ---------- thumbnails ---------- */

    /// The bin thumbnail for `m`, starting a decode the first time.
    pub(crate) fn thumbnail(&self, m: &Media, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        if m.offline || m.kind == MediaKind::Audio {
            return None;
        }
        let path = self.media_path(m);
        match cx.global::<MediaEngine>().thumbs.get(&path) {
            Some(Thumb::Ready(img)) => return Some(img.clone()),
            Some(_) => return None,
            None => {}
        }
        let secs = m
            .duration
            .map_or(0., |d| (self.fps().frames_to_seconds(d) * 0.1).min(2.));
        let engine = cx.global_mut::<MediaEngine>();
        engine.thumbs.insert(path.clone(), Thumb::Loading);
        let rx = engine.pool.request(FrameRequest {
            path: path.clone(),
            time: secs,
            max_w: THUMB_W,
            max_h: THUMB_H,
            slot: slot_for(&path),
        });
        cx.spawn(async move |this, cx| {
            let thumb = match rx.await {
                Ok(Ok(p)) => Thumb::Ready(render_image(&p)),
                _ => Thumb::Failed,
            };
            this.update(cx, |_, cx| {
                cx.global_mut::<MediaEngine>().thumbs.insert(path, thumb);
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /* ---------- viewer ---------- */

    /// The frame the viewer should show now, if any: the topmost enabled
    /// media clip on a video track at the playhead.
    pub(crate) fn wanted_frame(&self, window: &Window) -> Option<FrameKey> {
        let clip = self.picture_clip()?;
        let p = self.project();
        let m = clip.media.and_then(|id| p.media(id))?;
        if m.offline {
            return None;
        }
        let secs = match m.kind {
            MediaKind::Image => 0.,
            _ => self
                .fps()
                .frames_to_seconds(clip.source_in + (self.playhead - clip.start)),
        };
        let b = self.viewer_bounds.get();
        let scale = window.scale_factor();
        let w = ((f32::from(b.size.width) * scale) as u32).clamp(64, VIEWER_MAX.0);
        let h = ((f32::from(b.size.height) * scale) as u32).clamp(36, VIEWER_MAX.1);
        Some(FrameKey {
            path: self.media_path(m),
            ms: (secs * 1000.).round() as i64,
            w,
            h,
        })
    }

    /// Keeps the viewer's picture in step with the playhead: shows a cached
    /// frame at once, otherwise asks the pool and keeps the old frame up
    /// until the new one arrives.
    pub(crate) fn sync_viewer_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view == View::Graph {
            // The viewer isn't on screen; catch up when it is.
            return;
        }
        let wanted = self.wanted_frame(window);
        let Some(key) = wanted else {
            if let Some(old) = self.frame.take() {
                window.drop_image(old.image).ok();
            }
            self.frame_source = None;
            self.frame_pending = None;
            self.frame_generation += 1;
            return;
        };
        let decoded = self.frame_source.as_ref().is_some_and(|(k, _)| *k == key);
        if decoded {
            self.present_frame(window, cx);
            return;
        }
        if self.frame_pending.as_ref() == Some(&key) || self.frame_failed.as_ref() == Some(&key) {
            return;
        }
        let req = FrameRequest {
            path: key.path.clone(),
            time: key.ms as f64 / 1000.,
            max_w: key.w,
            max_h: key.h,
            slot: VIEWER_SLOT,
        };
        // Replies to anything asked for before this are stale: requests to
        // different files run on different workers and can finish out of order.
        self.frame_generation += 1;
        let generation = self.frame_generation;
        let pool = &cx.global::<MediaEngine>().pool;
        if let Some(p) = pool.cached(&req) {
            self.frame_source = Some((key, p));
            self.present_frame(window, cx);
            return;
        }
        let rx = pool.request(req);
        self.frame_pending = Some(key.clone());
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            this.update_in(cx, |ws, window, cx| {
                if ws.frame_generation != generation {
                    return;
                }
                ws.frame_pending = None;
                match result {
                    Ok(Ok(p)) => {
                        ws.frame_source = Some((key, p));
                        ws.present_frame(window, cx);
                    }
                    // A newer frame was asked for; it will arrive instead.
                    Ok(Err(MediaError::Superseded)) => {}
                    Ok(Err(e)) => {
                        ws.frame_failed = Some(key);
                        ws.say_error(format!("Couldn't decode this frame: {e}"));
                    }
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The effects the picture clip needs now, in the decoded picture's
    /// pixel units.
    fn wanted_fx(&self, picture_width: u32) -> Fx {
        let Some(clip) = self.picture_clip() else {
            return Fx::default();
        };
        let l = look_at(clip, self.playhead);
        let seq_w = self.project().sequence.width.max(1) as f64;
        Fx {
            grades: l.grades,
            // Blur is set in sequence pixels; the picture is decoded at
            // roughly the frame's width on screen.
            blur: l.blur * picture_width as f64 / seq_w,
            rotation: l.rotation,
            crop: if l.rotation != 0. { l.crop } else { [0.; 4] },
        }
    }

    /// Puts the decoded frame on screen with the clip's effects, working
    /// them out in the background when there are any. The previous picture
    /// stays up meanwhile.
    fn present_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((key, source)) = self.frame_source.clone() else {
            return;
        };
        let fx = self.wanted_fx(source.width);
        let current = |f: &ViewerFrame| f.key == key && f.fx == fx;
        if self.frame.as_ref().is_some_and(current) {
            return;
        }
        if fx.is_identity() {
            self.fx_pending = None;
            self.show_frame(key, fx, &source, window);
            return;
        }
        if self.fx_pending.as_ref() == Some(&(key.clone(), fx.clone())) {
            return;
        }
        self.fx_pending = Some((key.clone(), fx.clone()));
        let task = cx.background_executor().spawn({
            let fx = fx.clone();
            async move { videode_render::apply(&source, &fx) }
        });
        cx.spawn_in(window, async move |this, cx| {
            let picture = task.await;
            this.update_in(cx, |ws, window, cx| {
                // Only the latest request is shown; older ones are dropped.
                if ws.fx_pending.as_ref() != Some(&(key.clone(), fx.clone())) {
                    return;
                }
                ws.fx_pending = None;
                let base = ws
                    .frame_source
                    .as_ref()
                    .map_or((picture.width, picture.height), |(_, s)| {
                        (s.width, s.height)
                    });
                ws.show_picture(key, fx, base, &picture, window);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn show_frame(&mut self, key: FrameKey, fx: Fx, p: &Picture, window: &mut Window) {
        self.show_picture(key, fx, (p.width, p.height), p, window);
    }

    fn show_picture(
        &mut self,
        key: FrameKey,
        fx: Fx,
        base: (u32, u32),
        p: &Picture,
        window: &mut Window,
    ) {
        if let Some(old) = self.frame.take() {
            window.drop_image(old.image).ok();
        }
        self.frame = Some(ViewerFrame {
            key,
            fx,
            base,
            image: render_image(p),
        });
    }
}
