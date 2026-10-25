//! The editor window: owns the document being edited and the view state
//! (playhead, selection, tool, zoom), handles every action, and lays out the
//! panels. Panels live in `panels.rs`, the timeline in `timeline.rs`, and the
//! settings and about dialogs and launch splash in `dialogs.rs`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::prelude::*;
use gpui_kit::{
    Context, FocusHandle, PathPromptOptions, PromptLevel, Render, SharedString, Window, div, px,
    relative,
};
use videode_core::kdl_io::{from_kdl, to_kdl};
use videode_core::{
    ClipId, Document, EditResult, Frame, Project, TrackId, demo::demo_project, time,
};

use crate::actions::*;
use crate::settings;
use crate::theme::{self, c, hex};
use crate::timeline::TimelineState;
use crate::ui::cmd_key;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Blade,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinTab {
    Media,
    Nodes,
}

pub(crate) struct Playback {
    origin: Frame,
    started: Instant,
    pub(crate) speed: f64,
}

pub(crate) struct Status {
    pub text: SharedString,
    pub error: bool,
    /// Bumped on every message so the status line animates in.
    pub seq: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modal {
    Settings,
    About,
}

/// One-shot feedback animations. Each carries a sequence number that keys
/// its animation, so a repeat restarts it.
#[derive(Default)]
pub(crate) struct Fx {
    /// Clips changed by the last undo or redo.
    pub flash: Option<(Vec<ClipId>, u64)>,
    /// Where the last split or blade cut happened.
    pub cut: Option<(Frame, u64)>,
    seq: u64,
}

impl Fx {
    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }
}

pub struct Workspace {
    pub(crate) doc: Document,
    pub(crate) focus: FocusHandle,
    pub(crate) playhead: Frame,
    pub(crate) selection: Option<ClipId>,
    pub(crate) tool: Tool,
    pub(crate) snap: bool,
    pub(crate) bin_tab: BinTab,
    pub(crate) tl: TimelineState,
    pub(crate) status: Status,
    pub(crate) playback: Option<Playback>,
    pub(crate) modal: Option<Modal>,
    pub(crate) modal_opened: Instant,
    /// When the launch splash started; `None` once it's gone.
    pub(crate) splash: Option<Instant>,
    pub(crate) fx: Fx,
    title: String,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_document(Document::new(Project::default(), None), window, cx)
    }

    pub fn with_document(doc: Document, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let snap = !cx.has_global::<settings::AppSettings>() || settings::get(cx).snap;
        Self {
            doc,
            focus,
            playhead: 0,
            selection: None,
            tool: Tool::Select,
            snap,
            bin_tab: BinTab::Media,
            tl: TimelineState::default(),
            status: Status {
                text: "New project · open the demo to try editing, or open a .vde project".into(),
                error: false,
                seq: 0,
            },
            playback: None,
            modal: None,
            modal_opened: Instant::now(),
            splash: None,
            fx: Fx::default(),
            title: String::new(),
        }
    }

    /// For snapshots: open a dialog directly.
    pub fn show_modal(&mut self, modal: Modal) {
        self.modal = Some(modal);
        self.modal_opened = Instant::now();
    }

    /// For snapshots: show the launch splash as it looks `elapsed` in.
    pub fn preview_splash(&mut self, elapsed: std::time::Duration) {
        self.splash = Instant::now().checked_sub(elapsed);
    }

    /// For tests and snapshots: set the playhead and selection directly.
    pub fn set_view(&mut self, playhead: Frame, selection: Option<ClipId>) {
        self.playhead = playhead;
        self.selection = selection;
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub(crate) fn project(&self) -> &Project {
        &self.doc.project
    }

    pub(crate) fn fps(&self) -> videode_core::Fps {
        self.doc.project.sequence.fps
    }

    pub(crate) fn tc(&self, f: Frame) -> String {
        time::timecode(f, self.fps())
    }

    pub(crate) fn say(&mut self, text: impl Into<SharedString>) {
        self.set_status(text.into(), false);
    }

    pub(crate) fn say_error(&mut self, text: impl Into<SharedString>) {
        self.set_status(text.into(), true);
    }

    fn set_status(&mut self, text: SharedString, error: bool) {
        if self.status.text != text || self.status.error != error {
            self.status = Status {
                text,
                error,
                seq: self.status.seq + 1,
            };
        }
    }

    /// Plays the launch splash, if enabled and motion is allowed.
    pub fn start_splash(&mut self, cx: &mut Context<Self>) {
        if settings::get(cx).splash && settings::motion(cx) {
            self.splash = Some(Instant::now());
            cx.notify();
        }
    }

    pub(crate) fn flash_cut(&mut self, at: Frame) {
        let seq = self.fx.next();
        self.fx.cut = Some((at, seq));
    }

    /// Runs one undoable edit, reporting failures in the status bar.
    pub(crate) fn apply<R>(
        &mut self,
        label: &str,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Project) -> EditResult<R>,
    ) -> Option<R> {
        let r = match self.doc.edit(label, f) {
            Ok(r) => Some(r),
            Err(e) => {
                self.say_error(capitalize(&e.to_string()));
                None
            }
        };
        cx.notify();
        r
    }

    /// Drops a selection that no longer exists and flashes the clips that
    /// changed, so it's clear what undo or redo touched.
    fn after_history_change(&mut self, before: &Project) {
        let changed: Vec<ClipId> = self
            .doc
            .project
            .sequence
            .clips
            .iter()
            .filter(|c| before.clip(c.id) != Some(*c))
            .map(|c| c.id)
            .collect();
        if !changed.is_empty() {
            let seq = self.fx.next();
            self.fx.flash = Some((changed, seq));
        }
        if let Some(id) = self.selection
            && self.doc.project.clip(id).is_none()
        {
            self.selection = None;
        }
    }

    pub(crate) fn seek(&mut self, frame: Frame) {
        self.playhead = frame.max(0);
        if let Some(pb) = &mut self.playback {
            pb.origin = self.playhead;
            pb.started = Instant::now();
        }
    }

    fn replace_document(&mut self, doc: Document) {
        self.doc = doc;
        self.selection = None;
        self.playback = None;
        self.playhead = 0;
        self.tl.scroll_x = 0.;
        self.tl.fit_pending = true;
    }

    /* ---------- file actions ---------- */

    /// Runs `then` now, or after the user agrees to discard unsaved changes.
    fn guard_unsaved(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.doc.is_dirty() {
            then(self, window, cx);
            return;
        }
        let message = format!("Discard unsaved changes to “{}”?", self.doc.project.name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your edits since the last save will be lost."),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |this, window, cx| then(this, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn new_project(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_unsaved(window, cx, |this, _, cx| {
            this.replace_document(Document::new(Project::default(), None));
            this.say("New project");
            cx.notify();
        });
    }

    fn open_demo(&mut self, _: &OpenDemo, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_unsaved(window, cx, |this, _, cx| {
            this.replace_document(Document::new(demo_project(), None));
            this.say("Opened the Signal Study demo · picture previews arrive with media import");
            cx.notify();
        });
    }

    fn open_project(&mut self, _: &OpenProject, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_unsaved(window, cx, |_, window, cx| {
            let paths = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Open".into()),
            });
            cx.spawn_in(window, async move |this, cx| {
                let Ok(Ok(Some(paths))) = paths.await else {
                    return;
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                this.update(cx, |this, cx| {
                    this.load(&path);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        });
    }

    pub fn load(&mut self, path: &Path) {
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| from_kdl(&text).map_err(|e| e.to_string()));
        match result {
            Ok(project) => {
                self.replace_document(Document::new(project, Some(path.to_path_buf())));
                self.say(format!("Opened {}", path.display()));
            }
            Err(e) => self.say_error(format!("Couldn't open {}: {e}", path.display())),
        }
    }

    fn write(&mut self, path: PathBuf) {
        match std::fs::write(&path, to_kdl(&self.doc.project)) {
            Ok(()) => {
                self.say(format!("Saved {}", path.display()));
                self.doc.mark_saved(path);
            }
            Err(e) => self.say_error(format!("Couldn't save {}: {e}", path.display())),
        }
    }

    fn save(&mut self, _: &SaveProject, window: &mut Window, cx: &mut Context<Self>) {
        match self.doc.path.clone() {
            Some(path) => {
                self.write(path);
                cx.notify();
            }
            None => self.save_as(&SaveProjectAs, window, cx),
        }
    }

    fn save_as(&mut self, _: &SaveProjectAs, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self
            .doc
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        let name = format!("{}.vde", self.doc.project.name);
        let path = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut path))) = path.await else {
                return;
            };
            if path.extension().is_none() {
                path.set_extension("vde");
            }
            this.update(cx, |this, cx| {
                this.write(path);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_unsaved(window, cx, |_, _, cx| cx.quit());
    }

    fn export(&mut self, _: &Export, _: &mut Window, cx: &mut Context<Self>) {
        let o = &self.doc.project.output;
        let msg = format!(
            "Export isn't built yet · the Output node is set to {} · {} · {}",
            o.container, o.codec, o.resolution
        );
        self.say(msg);
        cx.notify();
    }

    /* ---------- edit actions ---------- */

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let before = self.doc.project.clone();
        match self.doc.undo() {
            Some(label) => self.say(format!("Undid {label}")),
            None => self.say("Nothing to undo"),
        }
        self.after_history_change(&before);
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let before = self.doc.project.clone();
        match self.doc.redo() {
            Some(label) => self.say(format!("Redid {label}")),
            None => self.say("Nothing to redo"),
        }
        self.after_history_change(&before);
        cx.notify();
    }

    fn split(&mut self, _: &Split, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.playhead;
        let p = &self.doc.project;
        let targets: Vec<ClipId> = match self.selection.and_then(|id| p.clip(id)) {
            Some(c) if t > c.start && t < c.end() => vec![c.id],
            _ => p
                .sequence
                .clips
                .iter()
                .filter(|c| t > c.start && t < c.end())
                .filter(|c| p.track(c.track).is_some_and(|tr| !tr.locked))
                .map(|c| c.id)
                .collect(),
        };
        if targets.is_empty() {
            self.say("No clip under the playhead to split");
            cx.notify();
            return;
        }
        let n = targets.len();
        let done = self.apply("split", cx, |p| {
            targets
                .iter()
                .try_for_each(|id| p.split(*id, t).map(|_| ()))
        });
        if done.is_some() {
            self.flash_cut(t);
            let what = if n == 1 {
                "1 clip".to_string()
            } else {
                format!("{n} clips")
            };
            self.say(format!(
                "Split {what} at {} · {} to undo",
                self.tc(t),
                cmd_key("Z")
            ));
        }
    }

    fn delete(&mut self, ripple: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selection else {
            self.say("Select a clip to delete");
            cx.notify();
            return;
        };
        let p = &self.doc.project;
        let name = p.clip(id).map(|c| p.clip_name(c)).unwrap_or_default();
        let label = if ripple { "ripple delete" } else { "delete" };
        let done = self.apply(label, cx, |p| {
            if ripple {
                p.ripple_delete(id)
            } else {
                p.delete_clip(id)
            }
        });
        if done.is_some() {
            self.selection = None;
            let verb = if ripple { "Ripple deleted" } else { "Deleted" };
            self.say(format!("{verb} {name} · {} to undo", cmd_key("Z")));
        }
    }

    fn delete_clip(&mut self, _: &DeleteClip, _: &mut Window, cx: &mut Context<Self>) {
        self.delete(false, cx);
    }

    fn ripple_delete(&mut self, _: &RippleDelete, _: &mut Window, cx: &mut Context<Self>) {
        self.delete(true, cx);
    }

    fn deselect(&mut self, _: &Deselect, _: &mut Window, cx: &mut Context<Self>) {
        if self.modal.is_some() {
            self.modal = None;
        } else if self.tl.drag.is_some() {
            self.cancel_drag();
        } else if self.tool == Tool::Blade {
            self.tool = Tool::Select;
        } else {
            self.selection = None;
        }
        cx.notify();
    }

    fn select_tool(&mut self, _: &SelectTool, _: &mut Window, cx: &mut Context<Self>) {
        self.set_tool(Tool::Select, cx);
    }

    fn blade_tool(&mut self, _: &BladeTool, _: &mut Window, cx: &mut Context<Self>) {
        self.set_tool(Tool::Blade, cx);
    }

    pub(crate) fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.tool = tool;
        self.say(match tool {
            Tool::Select => "Select tool",
            Tool::Blade => "Blade · click a clip to cut it · V or Esc to go back",
        });
        cx.notify();
    }

    pub(crate) fn toggle_snap(&mut self, _: &ToggleSnap, _: &mut Window, cx: &mut Context<Self>) {
        self.snap = !self.snap;
        self.say(if self.snap {
            "Snapping on · 1 s grid, clip edges, playhead"
        } else {
            "Snapping off"
        });
        cx.notify();
    }

    fn toggle_track(&mut self, _: &ToggleTrack, _: &mut Window, cx: &mut Context<Self>) {
        let track = self
            .selection
            .and_then(|id| self.doc.project.clip(id))
            .map(|c| c.track);
        match track {
            Some(track) => self.toggle_track_enabled(track, cx),
            None => {
                self.say("Select a clip to mute or hide its track");
                cx.notify();
            }
        }
    }

    pub(crate) fn toggle_track_enabled(&mut self, track: TrackId, cx: &mut Context<Self>) {
        let Some(t) = self.doc.project.track(track) else {
            return;
        };
        let (enabled, label) = (!t.enabled, t.label.clone());
        let edit = if enabled {
            "enable track"
        } else {
            "disable track"
        };
        if self
            .apply(edit, cx, |p| p.set_track_enabled(track, enabled))
            .is_some()
        {
            self.say(format!("{label} {}", if enabled { "on" } else { "off" }));
        }
    }

    pub(crate) fn toggle_track_locked(&mut self, track: TrackId, cx: &mut Context<Self>) {
        let Some(t) = self.doc.project.track(track) else {
            return;
        };
        let (locked, label) = (!t.locked, t.label.clone());
        let edit = if locked { "lock track" } else { "unlock track" };
        if self
            .apply(edit, cx, |p| p.set_track_locked(track, locked))
            .is_some()
        {
            self.say(format!(
                "{label} {}",
                if locked { "locked" } else { "unlocked" }
            ));
        }
    }

    /* ---------- view actions ---------- */

    pub(crate) fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        let step = settings::get(cx).zoom_step as f32;
        self.zoom_around_playhead(step, settings::motion(cx));
        cx.notify();
    }

    pub(crate) fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        let step = settings::get(cx).zoom_step as f32;
        self.zoom_around_playhead(1. / step, settings::motion(cx));
        cx.notify();
    }

    pub(crate) fn zoom_fit(&mut self, _: &ZoomToFit, _: &mut Window, cx: &mut Context<Self>) {
        self.fit(settings::motion(cx));
        cx.notify();
    }

    /* ---------- dialogs ---------- */

    pub(crate) fn open_settings(
        &mut self,
        _: &OpenSettings,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        settings::refresh_system(cx);
        self.modal = Some(Modal::Settings);
        self.modal_opened = Instant::now();
        cx.notify();
    }

    pub(crate) fn about(&mut self, _: &About, _: &mut Window, cx: &mut Context<Self>) {
        self.modal = Some(Modal::About);
        self.modal_opened = Instant::now();
        cx.notify();
    }

    /* ---------- transport ---------- */

    pub(crate) fn is_playing(&self) -> bool {
        self.playback.is_some()
    }

    fn start_playback(&mut self, speed: f64) {
        let end = self.doc.project.end();
        if speed > 0. && self.playhead >= end {
            self.playhead = 0;
        }
        self.playback = Some(Playback {
            origin: self.playhead,
            started: Instant::now(),
            speed,
        });
    }

    pub(crate) fn toggle_play(&mut self, _: &TogglePlay, _: &mut Window, cx: &mut Context<Self>) {
        if self.playback.take().is_none() {
            self.start_playback(1.);
        }
        cx.notify();
    }

    fn shuttle(&mut self, dir: f64) {
        let speed = match &self.playback {
            Some(pb) if pb.speed.signum() == dir => (pb.speed * 2.).clamp(-8., 8.),
            _ => dir,
        };
        self.start_playback(speed);
    }

    fn shuttle_forward(&mut self, _: &ShuttleForward, _: &mut Window, cx: &mut Context<Self>) {
        self.shuttle(1.);
        cx.notify();
    }

    fn shuttle_backward(&mut self, _: &ShuttleBackward, _: &mut Window, cx: &mut Context<Self>) {
        self.shuttle(-1.);
        cx.notify();
    }

    fn shuttle_stop(&mut self, _: &ShuttleStop, _: &mut Window, cx: &mut Context<Self>) {
        self.playback = None;
        cx.notify();
    }

    fn nudge(&mut self, frames: Frame, cx: &mut Context<Self>) {
        self.playback = None;
        self.seek(self.playhead + frames);
        self.follow_playhead();
        cx.notify();
    }

    pub(crate) fn step_back(&mut self, _: &StepBack, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge(-1, cx);
    }

    pub(crate) fn step_forward(&mut self, _: &StepForward, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge(1, cx);
    }

    fn second_back(&mut self, _: &SecondBack, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.fps().nominal();
        self.nudge(-n, cx);
    }

    fn second_forward(&mut self, _: &SecondForward, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.fps().nominal();
        self.nudge(n, cx);
    }

    fn prev_edit(&mut self, _: &PrevEdit, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.playhead;
        let points = self.doc.project.edit_points();
        let to = points.into_iter().rev().find(|&f| f < t).unwrap_or(0);
        self.nudge(to - t, cx);
    }

    fn next_edit(&mut self, _: &NextEdit, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.playhead;
        let end = self.doc.project.end();
        let points = self.doc.project.edit_points();
        let to = points.into_iter().find(|&f| f > t).unwrap_or(end);
        self.nudge(to - t, cx);
    }

    pub(crate) fn go_start(&mut self, _: &GoToStart, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.playhead;
        self.nudge(-t, cx);
    }

    pub(crate) fn go_end(&mut self, _: &GoToEnd, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.playhead;
        let end = self.doc.project.end();
        self.nudge(end - t, cx);
    }

    /// Advances the playhead from the wall clock while playing.
    fn tick_playback(&mut self, window: &mut Window) {
        let Some(pb) = &self.playback else {
            return;
        };
        let elapsed = pb.started.elapsed().as_secs_f64();
        let frame = pb.origin + (elapsed * self.fps().as_f64() * pb.speed).floor() as Frame;
        let end = self.doc.project.end();
        if frame >= end && pb.speed > 0. {
            self.playhead = end;
            self.playback = None;
        } else if frame <= 0 && pb.speed < 0. {
            self.playhead = 0;
            self.playback = None;
        } else {
            self.playhead = frame;
            window.request_animation_frame();
        }
        self.follow_playhead();
    }

    fn update_title(&mut self, window: &mut Window) {
        let dirty = if self.doc.is_dirty() { " •" } else { "" };
        let title = format!("{}{dirty} — videode", self.doc.project.name);
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick_playback(window);
        self.update_title(window);
        let editing = self.modal.is_none();

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_SUB))
            .text_color(c(hex::INK))
            .font_family(theme::FONT)
            .text_size(theme::TEXT)
            .line_height(relative(1.45))
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::open_demo))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::export))
            .on_action(cx.listener(Self::deselect))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::about))
            // Editing and transport are detached while a dialog is open.
            .when(editing, |d| {
                d.on_action(cx.listener(Self::undo))
                    .on_action(cx.listener(Self::redo))
                    .on_action(cx.listener(Self::split))
                    .on_action(cx.listener(Self::delete_clip))
                    .on_action(cx.listener(Self::ripple_delete))
                    .on_action(cx.listener(Self::select_tool))
                    .on_action(cx.listener(Self::blade_tool))
                    .on_action(cx.listener(Self::toggle_snap))
                    .on_action(cx.listener(Self::toggle_track))
                    .on_action(cx.listener(Self::zoom_in))
                    .on_action(cx.listener(Self::zoom_out))
                    .on_action(cx.listener(Self::zoom_fit))
                    .on_action(cx.listener(Self::toggle_play))
                    .on_action(cx.listener(Self::shuttle_forward))
                    .on_action(cx.listener(Self::shuttle_backward))
                    .on_action(cx.listener(Self::shuttle_stop))
                    .on_action(cx.listener(Self::step_back))
                    .on_action(cx.listener(Self::step_forward))
                    .on_action(cx.listener(Self::second_back))
                    .on_action(cx.listener(Self::second_forward))
                    .on_action(cx.listener(Self::prev_edit))
                    .on_action(cx.listener(Self::next_edit))
                    .on_action(cx.listener(Self::go_start))
                    .on_action(cx.listener(Self::go_end))
            })
            // Any key skips the splash without doing anything else.
            .when(self.splash.is_some(), |d| {
                d.capture_key_down(cx.listener(|this, _, _, cx| {
                    this.splash = None;
                    cx.stop_propagation();
                    cx.notify();
                }))
            })
            .child(self.render_top_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_bin(cx))
                    .child(self.render_viewer(cx))
                    .child(self.render_inspector()),
            )
            .child(
                div()
                    .h(px(5.))
                    .flex_none()
                    .bg(c(hex::PLUGE_SUB))
                    .border_t_1()
                    .border_color(c(hex::LINE)),
            )
            .child(self.render_timeline(window, cx))
            .child(self.render_status_bar())
            .children(self.modal.map(|m| self.render_modal(m, cx)))
            .children(self.render_splash(window, cx))
    }
}
