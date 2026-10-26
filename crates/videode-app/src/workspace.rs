//! The editor window: owns the document being edited and the view state
//! (playhead, selection, tool, zoom), handles every action, and lays out the
//! panels. Panels live in `panels.rs`, the timeline in `timeline.rs`, and the
//! settings and about dialogs and launch splash in `dialogs.rs`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::component::input::InputState;
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, FocusHandle, PathPromptOptions, PromptLevel, Render, SharedString, Window, div, px,
    relative,
};
use gpui_kit::{Entity, Subscription};
use videode_core::kdl_io::{from_kdl, to_kdl};
use videode_core::{
    ClipId, Document, EditResult, Frame, NodeId, OutputField, Project, TrackId, demo::demo_project,
    time,
};

use crate::actions::*;
use crate::media::{FrameKey, ViewerFrame};
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Timeline,
    /// Shift+Tab: the whole sequence as one node graph.
    Graph,
}

/// A value being typed into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FieldTarget {
    Param {
        clip: ClipId,
        node: NodeId,
        key: &'static str,
    },
    Output(OutputField),
}

pub(crate) struct FieldEdit {
    pub target: FieldTarget,
    pub input: Entity<InputState>,
    pub _subscription: Subscription,
}

/// Pointer drags outside the timeline. Each follows the pointer anywhere in
/// the window until release.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PointerDrag {
    /// Dragging a param label left/right to change its value.
    Scrub {
        clip: ClipId,
        node: NodeId,
        key: &'static str,
        x0: f32,
        v0: f64,
    },
    /// Dragging a node card in the peek panel to reorder it.
    Node {
        clip: ClipId,
        node: NodeId,
        x0: f32,
        dx: f32,
        moved: bool,
    },
    /// Panning the graph canvas.
    Pan { x0: f32, y0: f32, gx: f32, gy: f32 },
}

/// The graph canvas transform: screen = origin + layout × zoom.
pub(crate) struct GraphView {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
    pub fit_pending: bool,
    pub bounds: std::rc::Rc<std::cell::Cell<gpui_kit::Bounds<gpui_kit::Pixels>>>,
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
    pub(crate) view: View,
    /// The peek panel is open on the selected clip; the number keys its
    /// unfold animation.
    pub(crate) peek: Option<u64>,
    /// The node highlighted in peek, graph and inspector.
    pub(crate) focus_node: Option<NodeId>,
    /// The inspector shows the Output node instead of the selection.
    pub(crate) inspect_output: bool,
    pub(crate) field: Option<FieldEdit>,
    pub(crate) pdrag: Option<PointerDrag>,
    pub(crate) graph: GraphView,
    pub(crate) palette: Option<crate::palette::Palette>,
    /// Palette entries run most recently, newest first.
    pub(crate) recent_commands: Vec<String>,
    /// Folder that relative media paths are resolved against: the project
    /// file's folder, or the demo's.
    pub(crate) media_root: PathBuf,
    /// The picture in the viewer, and the frame being decoded to replace it.
    pub(crate) frame: Option<ViewerFrame>,
    pub(crate) frame_pending: Option<FrameKey>,
    /// A frame that failed to decode, so it isn't retried every render.
    pub(crate) frame_failed: Option<FrameKey>,
    /// The viewer's screen in window coordinates, captured each frame.
    pub(crate) viewer_bounds: std::rc::Rc<std::cell::Cell<gpui_kit::Bounds<gpui_kit::Pixels>>>,
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
        let mut ws = Self {
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
            view: View::Timeline,
            peek: None,
            focus_node: None,
            inspect_output: false,
            field: None,
            pdrag: None,
            graph: GraphView {
                x: 0.,
                y: 0.,
                zoom: 1.,
                fit_pending: true,
                bounds: Default::default(),
            },
            palette: None,
            recent_commands: Vec::new(),
            media_root: std::env::current_dir().unwrap_or_default(),
            frame: None,
            frame_pending: None,
            frame_failed: None,
            viewer_bounds: Default::default(),
            title: String::new(),
        };
        ws.refresh_media();
        ws
    }

    /// For snapshots: open a dialog directly.
    pub fn show_modal(&mut self, modal: Modal) {
        self.modal = Some(modal);
        self.modal_opened = Instant::now();
    }

    /// For snapshots: put the window in a state by name: `peek`, `graph`,
    /// `output`, `media` (use the generated demo footage), `focus` (second
    /// node of the selection), `edit` (typing into its first value) or
    /// `palette=<query>`.
    pub fn preview(&mut self, mode: &str, window: &mut Window, cx: &mut Context<Self>) {
        match mode {
            "peek" => self.open_peek(),
            "graph" => self.open_graph(),
            "output" => self.inspect_output = true,
            "media" => {
                self.media_root = crate::media::demo_root();
                self.refresh_media();
            }
            "focus" | "edit" => {
                let Some(clip) = self.selection.and_then(|id| self.project().clip(id)) else {
                    return;
                };
                let Some(node) = clip.nodes.get(1) else {
                    return;
                };
                let (clip_id, node_id) = (clip.id, node.id);
                let spec = node.kind.params().first();
                let text = spec
                    .and_then(|spec| {
                        node.get(spec.key)
                            .map(|v| crate::inspector::number_text(spec, v))
                    })
                    .unwrap_or_default();
                self.focus_node = Some(node_id);
                if mode == "edit"
                    && let Some(spec) = spec
                {
                    let target = FieldTarget::Param {
                        clip: clip_id,
                        node: node_id,
                        key: spec.key,
                    };
                    self.begin_field(target, text, window, cx);
                }
            }
            m => {
                if let Some(q) = m.strip_prefix("palette=") {
                    self.open_palette(q, window, cx);
                }
            }
        }
    }

    /// For snapshots: show the launch splash as it looks `elapsed` in.
    pub fn preview_splash(&mut self, elapsed: std::time::Duration) {
        self.splash = Instant::now().checked_sub(elapsed);
    }

    /// For tests and snapshots: set the playhead and selection directly.
    pub fn set_view(&mut self, playhead: Frame, selection: Option<ClipId>) {
        self.playhead = playhead;
        self.select(selection);
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

    pub(crate) fn next_seq(&mut self) -> u64 {
        self.fx.next()
    }

    /// Selects a clip (or nothing), resetting what hangs off the old one.
    pub(crate) fn select(&mut self, clip: Option<ClipId>) {
        if self.selection != clip {
            self.focus_node = None;
            if clip.is_none() {
                self.peek = None;
            }
        }
        self.selection = clip;
        if clip.is_some() {
            self.inspect_output = false;
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
            self.select(None);
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
        self.select(None);
        self.view = View::Timeline;
        self.inspect_output = false;
        self.field = None;
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
        self.guard_unsaved(window, cx, |this, window, cx| {
            this.replace_document(Document::new(demo_project(), None));
            this.media_root = crate::media::demo_root();
            this.prepare_demo_media(window, cx);
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
                self.media_root = path.parent().map(Path::to_path_buf).unwrap_or_default();
                match self.refresh_media() {
                    0 => self.say(format!("Opened {}", path.display())),
                    n => self.say_error(format!(
                        "Opened {} · {n} media file{} can't be found",
                        path.display(),
                        if n == 1 { "" } else { "s" }
                    )),
                }
            }
            Err(e) => self.say_error(format!("Couldn't open {}: {e}", path.display())),
        }
    }

    fn write(&mut self, path: PathBuf) {
        // Relative media paths must still resolve from the new location.
        if let Some(dir) = path.parent() {
            self.rebase_media(dir);
        }
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

    fn export(&mut self, _: &Export, window: &mut Window, cx: &mut Context<Self>) {
        self.open_output(&OpenOutput, window, cx);
    }

    /* ---------- node views ---------- */

    fn open_output(&mut self, _: &OpenOutput, _: &mut Window, cx: &mut Context<Self>) {
        self.inspect_output = true;
        self.say("Export is a node: the Output node is open in the inspector");
        cx.notify();
    }

    fn toggle_peek(&mut self, _: &TogglePeek, _: &mut Window, cx: &mut Context<Self>) {
        if self.peek.is_some() {
            self.peek = None;
        } else if let Some(clip) = self.selection {
            if self.view == View::Graph {
                self.view = View::Timeline;
            }
            self.reveal_clip(clip);
            self.open_peek();
            self.say("Peeking at the clip's nodes · click one to inspect it, drag to reorder");
        } else {
            self.say("Select a clip, then press Tab to peek at its nodes");
        }
        cx.notify();
    }

    fn toggle_graph(&mut self, _: &ToggleGraph, _: &mut Window, cx: &mut Context<Self>) {
        if self.view == View::Graph {
            self.close_graph();
        } else {
            self.open_graph();
        }
        cx.notify();
    }

    fn command_palette(&mut self, _: &CommandPalette, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        } else {
            self.open_palette("", window, cx);
        }
    }

    fn add_node_menu(&mut self, _: &AddNodeMenu, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_none() {
            self.say("Select a clip first, then add a node");
            cx.notify();
            return;
        }
        self.open_palette("Add ", window, cx);
    }

    /// The focused node, if it belongs to the selected clip.
    pub(crate) fn focused_node(&self) -> Option<(ClipId, NodeId)> {
        let clip = self.selection?;
        let node = self.focus_node?;
        self.project().clip(clip)?.node(node)?;
        Some((clip, node))
    }

    fn bypass_node(&mut self, _: &BypassNode, _: &mut Window, cx: &mut Context<Self>) {
        match self.focused_node() {
            Some((clip, node)) => self.toggle_bypass(clip, node, cx),
            None => {
                self.say("Click a node in the inspector or peek first");
                cx.notify();
            }
        }
    }

    /// Scrolls the timeline so `clip` is on screen.
    pub(crate) fn reveal_clip(&mut self, clip: ClipId) {
        let Some(c) = self.project().clip(clip) else {
            return;
        };
        let (x0, x1) = (self.frame_x(c.start), self.frame_x(c.end()));
        let w = self.lanes_width();
        if w > 0. && (x1 < 0. || x0 > w) {
            self.tl.scroll_x = (c.start as f32 * self.tl.ppf - 80.).max(0.);
        }
    }

    /* ---------- window-wide pointer drags ---------- */

    pub(crate) fn pointer_move(
        &mut self,
        pos: gpui_kit::Point<gpui_kit::Pixels>,
        m: gpui_kit::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let (x, y) = (f32::from(pos.x), f32::from(pos.y));
        match &mut self.pdrag {
            Some(PointerDrag::Scrub { .. }) => self.scrub_to(x, m.shift, m.alt, cx),
            Some(PointerDrag::Node { x0, dx, moved, .. }) => {
                *dx = x - *x0;
                *moved |= dx.abs() > 4.;
                cx.notify();
            }
            Some(PointerDrag::Pan { x0, y0, gx, gy }) => {
                self.graph.x = *gx + x - *x0;
                self.graph.y = *gy + y - *y0;
                cx.notify();
            }
            None => {}
        }
    }

    pub(crate) fn pointer_up(&mut self, cx: &mut Context<Self>) {
        if let Some(PointerDrag::Node {
            clip,
            node,
            dx,
            moved: true,
            ..
        }) = self.pdrag.take()
        {
            self.drop_node(clip, node, dx, cx);
        }
        cx.notify();
    }

    fn pointer_listeners(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        gpui_kit::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let on_move = this.clone();
                window.on_mouse_event(move |e: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                    if phase == gpui_kit::DispatchPhase::Bubble {
                        on_move
                            .update(cx, |ws, cx| ws.pointer_move(e.position, e.modifiers, cx))
                            .ok();
                    }
                });
                let on_up = this.clone();
                window.on_mouse_event(move |_: &gpui_kit::MouseUpEvent, phase, _, cx| {
                    if phase == gpui_kit::DispatchPhase::Bubble {
                        on_up.update(cx, |ws, cx| ws.pointer_up(cx)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
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
        // With the peek open, Delete removes the focused node instead.
        if !ripple
            && self.peek.is_some()
            && let Some((clip, node)) = self.focused_node()
            && !self
                .project()
                .clip(clip)
                .and_then(|c| c.node(node))
                .is_some_and(|n| n.kind.is_generator())
        {
            self.remove_node(clip, node, cx);
            return;
        }
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
            self.select(None);
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
        } else if self.pdrag.take().is_some() {
        } else if self.peek.is_some() {
            self.peek = None;
        } else if self.view == View::Graph {
            self.close_graph();
        } else if self.inspect_output {
            self.inspect_output = false;
        } else if self.tl.drag.is_some() {
            self.cancel_drag();
        } else if self.tool == Tool::Blade {
            self.tool = Tool::Select;
        } else {
            self.select(None);
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
        self.sync_viewer_frame(window, cx);
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
            .on_action(cx.listener(Self::command_palette))
            .on_action(cx.listener(Self::open_output))
            .on_action(cx.listener(Self::import_media))
            // Files dropped anywhere on the window are imported.
            .on_drop(cx.listener(|this, paths: &gpui_kit::ExternalPaths, _, cx| {
                this.import_paths(paths.paths().to_vec(), cx);
            }))
            // Editing and transport are detached while a dialog is open.
            .when(editing, |d| {
                d.on_action(cx.listener(Self::toggle_peek))
                    .on_action(cx.listener(Self::toggle_graph))
                    .on_action(cx.listener(Self::add_node_menu))
                    .on_action(cx.listener(Self::bypass_node))
                    .on_action(cx.listener(Self::undo))
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
            .map(|d| match self.view {
                View::Graph => d.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(self.render_graph(window, cx))
                        .child(self.render_inspector(cx)),
                ),
                View::Timeline => d
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .child(self.render_bin(cx))
                            .child(self.render_viewer(cx))
                            .child(self.render_inspector(cx)),
                    )
                    .child(
                        div()
                            .h(px(5.))
                            .flex_none()
                            .bg(c(hex::PLUGE_SUB))
                            .border_t_1()
                            .border_color(c(hex::LINE)),
                    )
                    .child(self.render_timeline(window, cx)),
            })
            .child(self.render_status_bar())
            .children(
                (self.view == View::Timeline)
                    .then(|| self.render_peek(window, cx))
                    .flatten(),
            )
            .when(self.pdrag.is_some(), |d| {
                d.child(self.pointer_listeners(cx))
            })
            .children(self.render_palette(window, cx))
            .children(self.modal.map(|m| self.render_modal(m, cx)))
            .children(self.render_splash(window, cx))
    }
}
