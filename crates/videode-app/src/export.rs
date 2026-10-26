//! The Export area: presets, the Output node's settings, a summary, and the
//! render queue. Jobs run one at a time, each on its own thread.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, FontWeight, Global, ImageSource, ObjectFit, SharedString, Window,
    div, img, px,
};
use videode_core::{Frame, OutputField};
use videode_export::{ExportError, Job, Plan, Progress, Report, VideoCodec};

use crate::theme::{self, Category, CategoryExt, c, hex};
use crate::ui::{button, icon_button, pane_header, swatch, tip};
use crate::workspace::Workspace;

/// Encoders that open on this machine, per codec, found once in the
/// background.
pub(crate) enum Encoders {
    NotChecked,
    Checking,
    Ready(HashMap<&'static str, Vec<&'static str>>),
}

impl Global for Encoders {}

pub fn init(cx: &mut App) {
    cx.set_global(Encoders::NotChecked);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExportRange {
    #[default]
    Whole,
    Selection,
}

type Outcome = Arc<Mutex<Option<Result<Report, ExportError>>>>;

pub(crate) enum JobState {
    Queued(Box<Job>),
    Running {
        progress: Arc<Progress>,
        started: Instant,
        outcome: Outcome,
    },
    Done {
        report: Report,
        secs: f64,
    },
    Failed(String),
    Cancelled,
}

pub(crate) struct ExportJob {
    id: u64,
    title: String,
    path: PathBuf,
    frames: u64,
    state: JobState,
}

impl ExportJob {
    fn finished(&self) -> bool {
        matches!(
            self.state,
            JobState::Done { .. } | JobState::Failed(_) | JobState::Cancelled
        )
    }
}

#[derive(Default)]
pub(crate) struct Exports {
    jobs: Vec<ExportJob>,
    next_id: u64,
    pub range: ExportRange,
    polling: bool,
}

impl Exports {
    pub fn running(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| matches!(j.state, JobState::Running { .. }))
    }

    pub fn busy(&self) -> bool {
        self.jobs.iter().any(|j| !j.finished())
    }
}

/// Built-in presets: (group, name, container, codec, resolution).
const PRESETS: &[(&str, &str, &str, &str, &str)] = &[
    ("Web", "YouTube 1080p", "MP4", "H.264", "1920×1080"),
    ("Web", "YouTube 4K", "MP4", "H.265", "3840×2160"),
    ("Web", "Vertical 1080×1920", "MP4", "H.264", "1080×1920"),
    (
        "Master",
        "ProRes 422 HQ",
        "MOV",
        "ProRes 422 HQ",
        "Sequence",
    ),
    (
        "Master",
        "Image sequence",
        "PNG sequence",
        "PNG",
        "Sequence",
    ),
    ("Audio", "WAV", "WAV", "PCM", "Sequence"),
];

fn mb(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.;
    if mb >= 1000. {
        format!("{:.1} GB", mb / 1000.)
    } else if mb >= 10. {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

impl Workspace {
    /// Where the Output node's file goes: as typed if absolute, otherwise
    /// next to the project file, or in the Movies/Videos folder.
    pub(crate) fn output_path(&self) -> PathBuf {
        let typed = PathBuf::from(&self.project().output.path);
        if typed.is_absolute() {
            return typed;
        }
        let base = self
            .doc
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(dirs::video_dir)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(typed)
    }

    fn export_range(&self) -> Option<Range<Frame>> {
        match self.exports.range {
            ExportRange::Whole => Some(0..self.project().end()),
            ExportRange::Selection => {
                let clip = self.selection.and_then(|id| self.project().clip(id))?;
                Some(clip.start..clip.end())
            }
        }
    }

    fn export_plan(&self) -> Result<Plan, ExportError> {
        let p = self.project();
        let sound = !self.build_mix().clips.is_empty();
        Plan::new(&p.output, &p.sequence, sound)
    }

    /// Offline files used by enabled tracks in the export range.
    fn offline_in_range(&self) -> Vec<String> {
        let p = self.project();
        let Some(range) = self.export_range() else {
            return Vec::new();
        };
        let mut names: Vec<String> = p
            .sequence
            .clips
            .iter()
            .filter(|c| c.start < range.end && c.end() > range.start)
            .filter(|c| p.track(c.track).is_some_and(|t| t.enabled))
            .filter_map(|c| p.media(c.media?))
            .filter(|m| m.offline)
            .map(|m| {
                m.path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Adds the export the Output node describes to the queue.
    pub(crate) fn queue_export(&mut self, cx: &mut Context<Self>) {
        let plan = match self.export_plan() {
            Ok(p) => p,
            Err(e) => {
                self.say_error(format!("Can't export: {e}"));
                return;
            }
        };
        let Some(range) = self.export_range().filter(|r| !r.is_empty()) else {
            self.say_error(match self.exports.range {
                ExportRange::Whole => "The sequence is empty",
                ExportRange::Selection => "Select a clip to export",
            });
            return;
        };
        let p = self.project();
        let paths = p
            .media
            .iter()
            .filter(|m| !m.offline)
            .map(|m| (m.id, self.media_path(m)))
            .collect();
        let path = self.output_path();
        let job = Job {
            project: p.clone(),
            paths,
            mix: self.build_mix(),
            range: range.clone(),
            path: path.clone(),
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kind = if plan.video.is_some() {
            p.output.codec.clone()
        } else {
            "WAV".into()
        };
        self.exports.next_id += 1;
        self.exports.jobs.push(ExportJob {
            id: self.exports.next_id,
            title: format!("{name} · {kind}"),
            path,
            frames: (range.end - range.start) as u64,
            state: JobState::Queued(Box::new(job)),
        });
        if self.exports.running() {
            self.say(format!("Queued {name}"));
        }
        self.run_next_export(cx);
    }

    /// Starts the oldest queued job if nothing is running.
    fn run_next_export(&mut self, cx: &mut Context<Self>) {
        if self.exports.running() {
            return;
        }
        let Some(job) = self
            .exports
            .jobs
            .iter_mut()
            .find(|j| matches!(j.state, JobState::Queued(_)))
        else {
            return;
        };
        let progress = Arc::new(Progress::default());
        let outcome: Outcome = Arc::default();
        let JobState::Queued(spec) = std::mem::replace(
            &mut job.state,
            JobState::Running {
                progress: progress.clone(),
                started: Instant::now(),
                outcome: outcome.clone(),
            },
        ) else {
            unreachable!("found queued above");
        };
        let name = job.title.clone();
        let spawned = std::thread::Builder::new()
            .name("videode-export".into())
            .spawn(move || {
                let result = videode_export::run(&spec, &progress);
                *outcome.lock().expect("export outcome lock") = Some(result);
            });
        if let Err(e) = spawned {
            job.state = JobState::Failed(e.to_string());
            return;
        }
        self.say(format!("Rendering {name}"));
        if !self.exports.polling {
            self.exports.polling = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    let more = this
                        .update(cx, |ws, cx| ws.poll_exports(cx))
                        .unwrap_or(false);
                    if !more {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    /// Picks up finished jobs; false once nothing is left to watch.
    fn poll_exports(&mut self, cx: &mut Context<Self>) -> bool {
        let mut message = None;
        for job in &mut self.exports.jobs {
            let JobState::Running {
                started, outcome, ..
            } = &job.state
            else {
                continue;
            };
            let Some(result) = outcome.lock().expect("export outcome lock").take() else {
                continue;
            };
            let secs = started.elapsed().as_secs_f64();
            let name = job
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            job.state = match result {
                Ok(report) => {
                    message = Some((format!("Exported {name} · {}", mb(report.bytes)), false));
                    JobState::Done { report, secs }
                }
                Err(ExportError::Cancelled) => {
                    message = Some((format!("Cancelled {name}"), false));
                    JobState::Cancelled
                }
                Err(e) => {
                    message = Some((format!("Export failed: {e}"), true));
                    JobState::Failed(e.to_string())
                }
            };
        }
        match message {
            Some((m, true)) => self.say_error(m),
            Some((m, false)) => self.say(m),
            None => {}
        }
        self.run_next_export(cx);
        cx.notify();
        self.exports.polling = self.exports.busy();
        self.exports.polling
    }

    fn cancel_export(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(job) = self.exports.jobs.iter_mut().find(|j| j.id == id) {
            match &job.state {
                JobState::Queued(_) => job.state = JobState::Cancelled,
                JobState::Running { progress, .. } => progress.cancel(),
                _ => {}
            }
        }
        cx.notify();
    }

    /// Finds the working encoders once, in the background.
    pub(crate) fn ensure_encoders(&mut self, cx: &mut Context<Self>) {
        if !matches!(cx.global::<Encoders>(), Encoders::NotChecked) {
            return;
        }
        cx.set_global(Encoders::Checking);
        let task = cx
            .background_executor()
            .spawn(async move { videode_export::detect_all() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            this.update(cx, |_, cx| {
                cx.set_global(Encoders::Ready(found));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The encoder the export would use, e.g. `VideoToolbox · hardware`.
    fn encoder_text(&self, plan: &Plan, cx: &App) -> (String, bool) {
        let Some(v) = &plan.video else {
            return (videode_export::audio_text(plan), false);
        };
        let key = match v.codec {
            VideoCodec::ProRes { .. } => "ProRes 422",
            _ => self.project().output.codec.as_str(),
        };
        match cx.global::<Encoders>() {
            Encoders::Ready(found) => {
                let ok = found.get(key).cloned().unwrap_or_default();
                match v
                    .codec
                    .candidates(v.hardware, v.quality)
                    .into_iter()
                    .find(|e| ok.contains(e))
                {
                    Some(e) => {
                        let kind = if videode_export::is_hardware(e) {
                            "hardware"
                        } else {
                            "software"
                        };
                        (
                            format!("{} · {kind}", videode_export::encoder_label(e)),
                            false,
                        )
                    }
                    None => ("None found".into(), true),
                }
            }
            _ => ("Checking…".into(), false),
        }
    }

    pub(crate) fn apply_preset(&mut self, preset: usize, cx: &mut Context<Self>) {
        let (_, name, container, codec, resolution) = PRESETS[preset];
        self.apply("apply export preset", cx, |p| {
            p.set_output(OutputField::Container, container)?;
            p.set_output(OutputField::Codec, codec)?;
            p.set_output(OutputField::Resolution, resolution)
        });
        self.say(format!("{name} preset"));
        cx.notify();
    }

    fn browse_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.output_path();
        let dir = current
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let name = current
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        let chosen = cx.prompt_for_new_path(&dir, name.as_deref());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            this.update(cx, |ws, cx| {
                let text = path.to_string_lossy().into_owned();
                ws.apply("set output file", cx, |p| {
                    p.set_output(OutputField::Path, &text)
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /* ---------- rendering ---------- */

    pub(crate) fn render_export_area(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.render_presets(cx))
            .child(self.render_export_form(cx))
            .child(self.render_export_summary(cx))
    }

    fn render_presets(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let o = &self.project().output;
        let mut list = div().flex().flex_col().py(px(4.));
        let mut group = "";
        for (i, &(g, name, container, codec, resolution)) in PRESETS.iter().enumerate() {
            if g != group {
                group = g;
                list = list.child(
                    div()
                        .px(px(12.))
                        .pt(px(10.))
                        .pb(px(2.))
                        .text_color(c(hex::INK_FAINT))
                        .child(g),
                );
            }
            let selected = o.container == container
                && o.codec == codec
                && (o.resolution == resolution || container == "WAV");
            let detail = match container {
                "WAV" => "48 kHz · 24-bit".to_string(),
                "PNG sequence" => "PNG · one file per frame".to_string(),
                _ if resolution == "Sequence" => format!("{codec} · {container}"),
                _ => format!("{codec} · {container} · {resolution}"),
            };
            let id = SharedString::from(format!("preset-{i}"));
            list = list.child(
                div()
                    .id(id.clone())
                    .mx(px(6.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(c(if selected {
                        hex::LINE_STRONG
                    } else {
                        hex::PLUGE_BLACK
                    }))
                    .when(selected, |d| d.bg(c(hex::PLUGE_CONTROL)))
                    .cursor_pointer()
                    .map(|d| crate::motion::hover_fx(d, id, c(hex::PLUGE_RAISED), px(5.)))
                    .on_click(cx.listener(move |this, _, _, cx| this.apply_preset(i, cx)))
                    .child(div().font_weight(FontWeight::MEDIUM).child(name))
                    .child(div().text_color(c(hex::INK_FAINT)).child(detail)),
            );
        }
        div()
            .w(px(220.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_BLACK))
            .border_r_1()
            .border_color(c(hex::LINE))
            .child(pane_header().child(pane_title("Presets")))
            .child(
                div()
                    .id("presets")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
    }

    fn render_export_form(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = self.project();
        let o = &p.output;
        let plan = self.export_plan();
        let has_video = o.has_video();
        let selection = self.selection.is_some();
        let range = self.exports.range;

        let row = |label: &'static str, control: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(30.))
                .child(
                    div()
                        .w(px(96.))
                        .flex_none()
                        .text_color(c(hex::INK_DIM))
                        .child(label),
                )
                .child(div().flex_1().min_w_0().flex().child(control))
        };
        let plain = |text: String| {
            div()
                .text_color(c(hex::INK_DIM))
                .child(text)
                .into_any_element()
        };
        let heading = |title: &'static str, cat: Option<Category>| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .mt(px(14.))
                .mb(px(2.))
                .font_weight(FontWeight::SEMIBOLD)
                .when_some(cat, |d, cat| d.child(swatch(cat)))
                .child(title)
        };

        let file = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .child(self.output_control(OutputField::Path, cx)),
            )
            .child(
                button("browse-output", None, "Browse…", false)
                    .h(px(24.))
                    .on_click(cx.listener(|this, _, window, cx| this.browse_output(window, cx))),
            )
            .into_any_element();

        let seg = |id: &'static str, label: &'static str, on: bool, enabled: bool| {
            div()
                .id(id)
                .h(px(22.))
                .px(px(9.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_color(c(if on { hex::INK } else { hex::INK_DIM }))
                .when(on, |d| d.bg(c(hex::PLUGE_HOVER)))
                .when(!enabled, |d| d.opacity(0.4))
                .when(enabled && !on, |d| {
                    d.cursor_pointer().hover(|s| s.text_color(c(hex::INK)))
                })
                .child(label)
        };
        let range_control = div()
            .flex()
            .p(px(1.))
            .rounded(px(5.))
            .border_1()
            .border_color(c(hex::LINE_STRONG))
            .bg(c(hex::PLUGE_SUB))
            .child(
                seg(
                    "range-whole",
                    "Whole sequence",
                    range == ExportRange::Whole,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.exports.range = ExportRange::Whole;
                    cx.notify();
                })),
            )
            .child(
                seg(
                    "range-clip",
                    "Selected clip",
                    range == ExportRange::Selection,
                    selection,
                )
                .when(selection, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| {
                        this.exports.range = ExportRange::Selection;
                        cx.notify();
                    }))
                }),
            )
            .into_any_element();

        let saves_to = self
            .output_path()
            .parent()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        let home = dirs::home_dir().map(|h| h.display().to_string());
        let saves_to = match home {
            Some(h) if saves_to.starts_with(&h) => format!("~{}", &saves_to[h.len()..]),
            _ => saves_to,
        };

        let mut form = div()
            .max_w(px(560.))
            .flex()
            .flex_col()
            .child(heading("Output", Some(Category::Output)))
            .child(row("File", file))
            .child(row("Folder", plain(saves_to)))
            .child(row(
                "Format",
                self.output_control(OutputField::Container, cx),
            ))
            .child(row("Range", range_control));
        if has_video {
            let fps = p.sequence.fps;
            form = form
                .child(heading("Video", None))
                .child(row("Codec", self.output_control(OutputField::Codec, cx)))
                .child(row(
                    "Resolution",
                    self.output_control(OutputField::Resolution, cx),
                ))
                .child(row("Frame rate", plain(format!("{fps} · sequence"))))
                .child(row(
                    "Quality",
                    self.output_control(OutputField::Quality, cx),
                ))
                .child(row(
                    "Hardware",
                    self.output_control(OutputField::Hardware, cx),
                ));
        }
        form = form.child(heading("Audio", None)).child(row(
            "Sound",
            plain(match &plan {
                Ok(plan) => videode_export::audio_text(plan),
                Err(_) => "None".into(),
            }),
        ));
        if let Err(e) = &plan {
            form = form.child(
                div()
                    .mt(px(10.))
                    .text_color(c(hex::ERROR))
                    .child(format!("Can't export: {e}")),
            );
        }
        div()
            .id("export-form")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .px(px(20.))
            .pb(px(20.))
            .bg(c(hex::PLUGE_SUB))
            .child(form)
    }

    fn render_export_summary(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = self.project();
        let fps = p.sequence.fps;
        let plan = self.export_plan();
        let range = self.export_range().unwrap_or(0..0);
        let frames = range.end - range.start;
        let secs = fps.frames_to_seconds(frames);
        let offline = self.offline_in_range();

        let thumb = div()
            .w_full()
            .aspect_ratio(16. / 9.)
            .flex_none()
            .rounded(px(5.))
            .overflow_hidden()
            .border_1()
            .border_color(c(hex::LINE))
            .bg(gpui_kit::black())
            .when_some(self.frame.as_ref(), |d, f| {
                d.child(
                    img(ImageSource::Render(f.image.clone()))
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                )
            });

        let kv = |k: &'static str, v: String, color: u32| {
            div()
                .flex()
                .gap(px(8.))
                .min_h(px(20.))
                .child(
                    div()
                        .w(px(96.))
                        .flex_none()
                        .text_color(c(hex::INK_FAINT))
                        .child(k),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(c(color))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(v),
                )
        };
        let mut facts = div()
            .flex()
            .flex_col()
            .child(kv("Duration", self.tc(frames), hex::INK));
        match &plan {
            Ok(plan) => {
                if let Some(v) = &plan.video {
                    facts = facts.child(kv(
                        "Video",
                        format!("{} · {}×{} · {}", p.output.codec, v.width, v.height, fps),
                        hex::INK,
                    ));
                }
                let (encoder, missing) = self.encoder_text(plan, cx);
                if plan.video.is_some() {
                    facts = facts.child(kv(
                        "Encoder",
                        encoder,
                        if missing { hex::ERROR } else { hex::INK },
                    ));
                }
                facts = facts
                    .child(kv("Audio", videode_export::audio_text(plan), hex::INK))
                    .child(kv(
                        "Size",
                        format!(
                            "≈ {}",
                            mb(videode_export::estimate_bytes(plan, fps.as_f64(), secs))
                        ),
                        hex::INK,
                    ));
            }
            Err(e) => facts = facts.child(kv("Settings", e.to_string(), hex::ERROR)),
        }
        if !offline.is_empty() {
            facts = facts.child(kv("Offline", offline.join(", "), hex::WARN));
        }

        let busy = self.exports.running();
        let render = button(
            "export-render",
            Some("upload"),
            if busy { "Add to queue" } else { "Render" },
            true,
        )
        .justify_center()
        .tooltip(tip(
            if busy {
                "Render after the current job"
            } else {
                "Render now"
            },
            None,
        ))
        .on_click(cx.listener(|this, _, _, cx| {
            this.queue_export(cx);
            cx.notify();
        }));

        let finished = self.exports.jobs.iter().any(ExportJob::finished);
        let count = self.exports.jobs.len();
        let queue_header = pane_header()
            .border_t_1()
            .child(pane_title("Queue"))
            .child(
                div()
                    .ml_auto()
                    .mr(px(4.))
                    .text_color(c(hex::INK_FAINT))
                    .child(match count {
                        0 => "Empty".to_string(),
                        1 => "1 job".to_string(),
                        n => format!("{n} jobs"),
                    }),
            )
            .when(finished, |d| {
                d.child(
                    button("clear-finished", None, "Clear", false)
                        .h(px(22.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.exports.jobs.retain(|j| !j.finished());
                            cx.notify();
                        })),
                )
            });

        let jobs: Vec<AnyElement> = self
            .exports
            .jobs
            .iter()
            .rev()
            .map(|j| self.render_job(j, cx))
            .collect();

        div()
            .w(px(320.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_BLACK))
            .border_l_1()
            .border_color(c(hex::LINE))
            .child(pane_header().child(pane_title("Summary")))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(12.))
                    .child(thumb)
                    .child(facts)
                    .child(render),
            )
            .child(queue_header)
            .child(
                div()
                    .id("export-queue")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(jobs),
            )
    }

    fn render_job(&self, j: &ExportJob, cx: &mut Context<Self>) -> AnyElement {
        let (state, detail, fraction, color) = match &j.state {
            JobState::Queued(_) => ("Queued".to_string(), String::new(), 0., hex::INK_FAINT),
            JobState::Running {
                progress, started, ..
            } => {
                let done = progress.done();
                let f = done as f32 / j.frames.max(1) as f32;
                let elapsed = started.elapsed().as_secs_f64();
                let left = if done > 0 {
                    format!(
                        " · {} left",
                        clock(elapsed * (j.frames - done.min(j.frames)) as f64 / done as f64)
                    )
                } else {
                    String::new()
                };
                (
                    format!("{:.0}%", f * 100.),
                    format!("{}{left}", clock(elapsed)),
                    f,
                    hex::INK,
                )
            }
            JobState::Done { report, secs } => (
                "Done".into(),
                format!(
                    "{} · {}{}",
                    mb(report.bytes),
                    clock(*secs),
                    report
                        .encoder
                        .map(|e| format!(" · {}", videode_export::encoder_label(e)))
                        .unwrap_or_default()
                ),
                1.,
                hex::INK_DIM,
            ),
            JobState::Failed(e) => ("Failed".into(), e.clone(), 0., hex::ERROR),
            JobState::Cancelled => ("Cancelled".into(), String::new(), 0., hex::INK_FAINT),
        };
        let id = j.id;
        let bar_color = match j.state {
            JobState::Done { .. } => Category::Composite.color(),
            _ => c(hex::BONE),
        };
        let action: Option<AnyElement> = match &j.state {
            JobState::Queued(_) | JobState::Running { .. } => Some(
                icon_button(
                    SharedString::from(format!("cancel-{id}")),
                    "close",
                    false,
                    false,
                )
                .tooltip(tip("Cancel", None))
                .on_click(cx.listener(move |this, _, _, cx| this.cancel_export(id, cx)))
                .into_any_element(),
            ),
            JobState::Done { .. } => {
                let path = j.path.clone();
                Some(
                    button(
                        SharedString::from(format!("show-{id}")),
                        None,
                        "Show",
                        false,
                    )
                    .h(px(22.))
                    .on_click(move |_, _, cx| cx.reveal_path(&path))
                    .into_any_element(),
                )
            }
            _ => None,
        };
        div()
            .px(px(12.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .border_b_1()
            .border_color(c(hex::LINE))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::MEDIUM)
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(j.title.clone()),
                    )
                    .child(div().text_color(c(color)).child(state))
                    .children(action),
            )
            .when(
                matches!(j.state, JobState::Running { .. } | JobState::Done { .. }),
                |d| {
                    d.child(
                        div()
                            .h(px(3.))
                            .w_full()
                            .rounded(px(2.))
                            .bg(c(hex::PLUGE_CONTROL))
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(2.))
                                    .bg(bar_color)
                                    .w(gpui_kit::relative(fraction.clamp(0., 1.))),
                            ),
                    )
                },
            )
            .when(!detail.is_empty(), |d| {
                d.child(
                    div()
                        .font_family(theme::MONO)
                        .text_color(c(if matches!(j.state, JobState::Failed(_)) {
                            hex::ERROR
                        } else {
                            hex::INK_FAINT
                        }))
                        .child(detail),
                )
            })
            .into_any_element()
    }
}

fn pane_title(text: &'static str) -> impl IntoElement {
    div()
        .h(px(24.))
        .px(px(9.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .bg(c(hex::PLUGE_CONTROL))
        .font_weight(FontWeight::MEDIUM)
        .child(text)
}
