//! The timeline: ruler, track headers, clips with chain strips, playhead,
//! and the pointer interactions (select, move, trim, blade, scrub, zoom).
//!
//! Hit-testing is done here from the project data rather than per element,
//! so drags keep working while clips re-render under the pointer.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, BoxShadow, Context, CursorStyle, DispatchPhase, FontWeight, Hsla,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, Window, canvas, div, point, px,
};
use videode_core::{
    Clip, ClipId, ClipKind, Frame, NodeKind, StreamKind, Track, TrackId, snap, time,
};

use crate::actions::{Split, ZoomIn, ZoomOut, ZoomToFit};
use crate::theme::{self, CategoryExt, c, hex};
use crate::ui::{self, ghost_button, icon_button, tip};
use crate::workspace::{Tool, Workspace};

const TOOLBAR_H: f32 = 32.;
const RULER_H: f32 = 28.;
const HEADS_W: f32 = 168.;
const EDGE_PX: f32 = 6.;
const SNAP_PX: f32 = 8.;
const MIN_PPF: f32 = 0.05;
const MAX_PPF: f32 = 40.;

pub(crate) fn track_height(t: &Track) -> f32 {
    match t.kind {
        StreamKind::Video => 60.,
        StreamKind::Audio => 48.,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Drag {
    Move {
        clip: ClipId,
        grab: Frame,
        from_track: TrackId,
    },
    TrimStart {
        clip: ClipId,
    },
    TrimEnd {
        clip: ClipId,
    },
    Scrub,
}

pub(crate) struct TimelineState {
    /// Pixels per frame.
    pub ppf: f32,
    pub scroll_x: f32,
    pub fit_pending: bool,
    pub drag: Option<Drag>,
    pub snap_line: Option<Frame>,
    pub hover_frame: Option<Frame>,
    /// Lanes area in window coordinates, captured each frame.
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            ppf: 1.2,
            scroll_x: 0.,
            fit_pending: true,
            drag: None,
            snap_line: None,
            hover_frame: None,
            bounds: Rc::default(),
        }
    }
}

enum Hit {
    Clip { id: ClipId, edge: Option<bool> },
    Empty,
}

impl Workspace {
    fn lanes_width(&self) -> f32 {
        f32::from(self.tl.bounds.get().size.width)
    }

    fn frame_at(&self, window_x: Pixels) -> Frame {
        let local = f32::from(window_x - self.tl.bounds.get().origin.x);
        ((local + self.tl.scroll_x) / self.tl.ppf).round() as Frame
    }

    fn frame_x(&self, frame: Frame) -> f32 {
        frame as f32 * self.tl.ppf - self.tl.scroll_x
    }

    fn track_at(&self, window_y: Pixels) -> Option<TrackId> {
        let mut y = f32::from(window_y - self.tl.bounds.get().origin.y);
        for t in &self.project().sequence.tracks {
            let h = track_height(t);
            if y >= 0. && y < h {
                return Some(t.id);
            }
            y -= h;
        }
        None
    }

    fn hit(&self, position: Point<Pixels>) -> Hit {
        let track = self.track_at(position.y);
        let local = f32::from(position.x - self.tl.bounds.get().origin.x);
        if let Some(track) = track {
            for clip in self.project().clips_on(track) {
                let x0 = self.frame_x(clip.start);
                let x1 = self.frame_x(clip.end());
                if local >= x0 && local <= x1 {
                    let zone = EDGE_PX.min((x1 - x0) / 3.);
                    let edge = if local - x0 <= zone {
                        Some(false)
                    } else if x1 - local <= zone {
                        Some(true)
                    } else {
                        None
                    };
                    return Hit::Clip { id: clip.id, edge };
                }
            }
        }
        Hit::Empty
    }

    fn snap_tolerance(&self) -> Frame {
        ((SNAP_PX / self.tl.ppf).ceil() as Frame).max(1)
    }

    /// Snaps to clip edges and the playhead, then to whole seconds when the
    /// grid is visible. Returns the snapped frame and whether it snapped.
    fn snapped(&self, frame: Frame, exclude: Option<ClipId>) -> (Frame, bool) {
        if !self.snap {
            return (frame, false);
        }
        let points = self.project().snap_points(exclude, &[self.playhead]);
        let tol = self.snap_tolerance();
        if let Some(f) = snap(frame, &points, tol) {
            return (f, true);
        }
        let sec = self.fps().nominal();
        if self.tl.ppf * sec as f32 >= 12. {
            let grid = ((frame as f64 / sec as f64).round() as Frame) * sec;
            if (grid - frame).abs() <= tol / 2 {
                return (grid, true);
            }
        }
        (frame, false)
    }

    pub(crate) fn follow_playhead(&mut self) {
        let w = self.lanes_width();
        if w <= 0. {
            return;
        }
        let x = self.frame_x(self.playhead);
        if x > w - 40. {
            self.tl.scroll_x = (self.playhead as f32 * self.tl.ppf - 60.).max(0.);
        } else if x < 0. {
            self.tl.scroll_x = (self.playhead as f32 * self.tl.ppf - w + 60.).max(0.);
        }
    }

    fn clamp_scroll(&mut self) {
        let w = self.lanes_width();
        let tail = 20 * self.fps().nominal();
        let content = (self.project().end() + tail) as f32 * self.tl.ppf;
        self.tl.scroll_x = self.tl.scroll_x.clamp(0., (content - w).max(0.));
    }

    fn zoom_at(&mut self, factor: f32, anchor_x: f32) {
        let frame = (anchor_x + self.tl.scroll_x) / self.tl.ppf;
        self.tl.ppf = (self.tl.ppf * factor).clamp(MIN_PPF, MAX_PPF);
        self.tl.scroll_x = frame * self.tl.ppf - anchor_x;
        self.clamp_scroll();
    }

    pub(crate) fn zoom_around_playhead(&mut self, factor: f32) {
        let x = self
            .frame_x(self.playhead)
            .clamp(0., self.lanes_width().max(0.));
        self.zoom_at(factor, x);
    }

    fn fit(&mut self) {
        let w = self.lanes_width();
        let end = self.project().end().max(30 * self.fps().nominal());
        self.tl.ppf = ((w - 40.) / end as f32).clamp(MIN_PPF, MAX_PPF);
        self.tl.scroll_x = 0.;
    }

    pub(crate) fn cancel_drag(&mut self) {
        if matches!(
            self.tl.drag,
            Some(Drag::Move { .. } | Drag::TrimStart { .. } | Drag::TrimEnd { .. })
        ) {
            self.doc.cancel_gesture();
        }
        self.tl.drag = None;
        self.tl.snap_line = None;
    }

    /* ---------- pointer ---------- */

    fn lanes_mouse_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        match self.hit(e.position) {
            Hit::Empty => {
                self.selection = None;
                self.seek(self.frame_at(e.position.x));
                self.tl.drag = Some(Drag::Scrub);
            }
            Hit::Clip { id, edge } => {
                if self.tool == Tool::Blade {
                    let (at, _) = self.snapped(self.frame_at(e.position.x), None);
                    if self.apply("split", cx, |p| p.split(id, at)).is_some() {
                        let tc = self.tc(at);
                        self.say(format!("Cut at {tc} · {} to undo", ui::cmd_key("Z")));
                    }
                    return;
                }
                self.selection = Some(id);
                let clip = self.project().clip(id).expect("hit-tested");
                let locked = self.project().track(clip.track).is_some_and(|t| t.locked);
                if locked {
                    let label = self.project().track(clip.track).map(|t| t.label.clone());
                    self.say(format!(
                        "{} is locked · unlock it in the track header to edit",
                        label.unwrap_or_default()
                    ));
                } else {
                    let grab = self.frame_at(e.position.x) - clip.start;
                    let from_track = clip.track;
                    self.doc.begin_gesture();
                    self.tl.drag = Some(match edge {
                        Some(false) => Drag::TrimStart { clip: id },
                        Some(true) => Drag::TrimEnd { clip: id },
                        None => Drag::Move {
                            clip: id,
                            grab,
                            from_track,
                        },
                    });
                }
            }
        }
        cx.notify();
    }

    fn drag_move(&mut self, position: Point<Pixels>, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(drag) = self.tl.drag else {
            return;
        };
        let frame = self.frame_at(position.x);
        self.tl.snap_line = None;
        match drag {
            Drag::Scrub => {
                let f = if modifiers.shift {
                    self.snapped(frame, None).0
                } else {
                    frame
                };
                self.seek(f);
            }
            Drag::Move {
                clip,
                grab,
                from_track,
            } => {
                let Some(c) = self.doc.project.clip(clip) else {
                    return;
                };
                let (kind, duration) = (c.kind, c.duration);
                let desired = frame - grab;
                let (s, s_hit) = self.snapped(desired, Some(clip));
                let (e, e_hit) = self.snapped(desired + duration, Some(clip));
                let start = match (s_hit, e_hit) {
                    (true, true) if (e - desired - duration).abs() < (s - desired).abs() => {
                        e - duration
                    }
                    (true, _) => s,
                    (false, true) => e - duration,
                    _ => desired,
                };
                if s_hit || e_hit {
                    self.tl.snap_line = Some(if start == s { s } else { e });
                }
                let target = self
                    .track_at(position.y)
                    .filter(|t| {
                        self.project()
                            .track(*t)
                            .is_some_and(|t| t.kind == kind.stream() && !t.locked)
                    })
                    .unwrap_or(from_track);
                let moved = self.doc.gesture(|p| p.move_clip(clip, target, start));
                if moved.is_err() {
                    self.doc
                        .gesture(|p| p.move_clip(clip, from_track, start))
                        .ok();
                }
            }
            Drag::TrimStart { clip } => {
                let (f, hit) = self.snapped(frame, Some(clip));
                if hit {
                    self.tl.snap_line = Some(f);
                }
                self.doc.gesture(|p| p.trim_start(clip, f)).ok();
            }
            Drag::TrimEnd { clip } => {
                let (f, hit) = self.snapped(frame, Some(clip));
                if hit {
                    self.tl.snap_line = Some(f);
                }
                self.doc.gesture(|p| p.trim_end(clip, f)).ok();
            }
        }
        cx.notify();
    }

    fn drag_end(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.tl.drag.take() else {
            return;
        };
        self.tl.snap_line = None;
        let (label, clip) = match drag {
            Drag::Scrub => {
                cx.notify();
                return;
            }
            Drag::Move { clip, .. } => ("move", clip),
            Drag::TrimStart { clip } | Drag::TrimEnd { clip } => ("trim", clip),
        };
        if self.doc.end_gesture(label)
            && let Some(c) = self.project().clip(clip)
        {
            let msg = format!(
                "{} {} · {} – {}",
                if label == "move" { "Moved" } else { "Trimmed" },
                self.project().clip_name(c),
                self.tc(c.start),
                self.tc(c.end())
            );
            self.say(msg);
        }
        cx.notify();
    }

    fn lanes_hover(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.tool == Tool::Blade && self.tl.drag.is_none() {
            let (f, _) = self.snapped(self.frame_at(e.position.x), None);
            if self.tl.hover_frame != Some(f) {
                self.tl.hover_frame = Some(f);
                cx.notify();
            }
        } else if self.tl.hover_frame.take().is_some() {
            cx.notify();
        }
    }

    fn wheel(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = e.delta.pixel_delta(px(20.));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if e.modifiers.secondary() {
            let anchor = f32::from(e.position.x - self.tl.bounds.get().origin.x);
            self.zoom_at((dy * 0.0025).exp(), anchor);
        } else {
            self.tl.scroll_x -= if dx.abs() > dy.abs() { dx } else { dy };
            self.clamp_scroll();
        }
        cx.notify();
    }

    /* ---------- rendering ---------- */

    pub(crate) fn render_timeline(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if self.tl.fit_pending {
            if self.lanes_width() > 0. {
                self.fit();
                self.tl.fit_pending = false;
            } else {
                window.request_animation_frame();
            }
        }

        let tracks: Vec<Track> = self.project().sequence.tracks.clone();
        let lanes_h: f32 = tracks.iter().map(track_height).sum();

        div()
            .h(px(340.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_BLACK))
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_heads(&tracks, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .on_scroll_wheel(
                                cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.wheel(e, cx)),
                            )
                            .child(self.render_ruler(cx))
                            .child(self.render_lanes(&tracks, lanes_h, cx)),
                    ),
            )
            .when(self.tl.drag.is_some(), |d| d.child(self.drag_listeners(cx)))
    }

    /// While dragging, follow the pointer anywhere in the window.
    fn drag_listeners(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let move_target = this.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        move_target
                            .update(cx, |ws, cx| ws.drag_move(e.position, e.modifiers, cx))
                            .ok();
                    }
                });
                let up_target = this.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        up_target.update(cx, |ws, cx| ws.drag_end(cx)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let snap_on = self.snap;
        div()
            .h(px(TOOLBAR_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .border_b_1()
            .border_color(c(hex::LINE))
            .child(
                div()
                    .flex()
                    .p(px(1.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c(hex::LINE))
                    .bg(c(hex::PLUGE_SUB))
                    .child(
                        icon_button(
                            "tool-select",
                            "mouse-pointer",
                            self.tool == Tool::Select,
                            false,
                        )
                        .tooltip(tip("Select and move", Some("V".into())))
                        .on_click(cx.listener(|this, _, _, cx| this.set_tool(Tool::Select, cx))),
                    )
                    .child(
                        icon_button("tool-blade", "scissors", self.tool == Tool::Blade, false)
                            .tooltip(tip("Blade: click a clip to cut it", Some("B".into())))
                            .on_click(cx.listener(|this, _, _, cx| this.set_tool(Tool::Blade, cx))),
                    ),
            )
            .child(
                icon_button("snap", "magnet", snap_on, false)
                    .tooltip(tip("Snap to grid, edges and playhead", Some("N".into())))
                    .on_click(cx.listener(|this, _, w, cx| {
                        this.toggle_snap(&crate::actions::ToggleSnap, w, cx)
                    })),
            )
            .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(c(hex::LINE)))
            .child(
                ghost_button("split", "scissors", "Split", Some("S"))
                    .tooltip(tip("Split at the playhead", None))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(Split), cx)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .font_family(theme::MONO)
                    .text_color(c(hex::INK_FAINT))
                    .mr(px(6.))
                    .child(format!("{:.0}%", self.tl.ppf / 1.2 * 100.)),
            )
            .child(
                icon_button("zoom-out", "minus", false, false)
                    .tooltip(tip("Zoom out", Some("−".into())))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(ZoomOut), cx)),
            )
            .child(
                icon_button("zoom-in", "plus", false, false)
                    .tooltip(tip("Zoom in", Some("=".into())))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(ZoomIn), cx)),
            )
            .child(
                icon_button("zoom-fit", "maximize", false, false)
                    .tooltip(tip("Fit sequence", Some("⇧ Z".into())))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(ZoomToFit), cx)),
            )
    }

    fn render_heads(&self, tracks: &[Track], cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected_track = self
            .selection
            .and_then(|id| self.project().clip(id))
            .map(|c| c.track);
        div()
            .w(px(HEADS_W))
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(c(hex::LINE))
            .child(
                div()
                    .h(px(RULER_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .border_b_1()
                    .border_color(c(hex::LINE))
                    .font_family(theme::MONO)
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.tc(self.playhead)),
            )
            .children(tracks.iter().map(|t| {
                let id = t.id;
                let enable_icon = match (t.kind, t.enabled) {
                    (StreamKind::Video, true) => "eye",
                    (StreamKind::Video, false) => "eye-off",
                    (StreamKind::Audio, true) => "volume-2",
                    (StreamKind::Audio, false) => "volume-x",
                };
                let enable_tip = match (t.kind, t.enabled) {
                    (StreamKind::Video, true) => "Hide track",
                    (StreamKind::Video, false) => "Show track",
                    (StreamKind::Audio, true) => "Mute track",
                    (StreamKind::Audio, false) => "Unmute track",
                };
                div()
                    .h(px(track_height(t)))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .border_b_1()
                    .border_color(c(hex::LINE))
                    .when(selected_track == Some(id), |d| d.bg(c(hex::PLUGE_RAISED)))
                    .child(
                        div()
                            .w(px(22.))
                            .font_family(theme::MONO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(t.label.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .when(!t.enabled, |d| d.line_through().text_color(c(hex::INK_DIM)))
                            .child(SharedString::from(t.name.clone())),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!("{}-enable", t.label)),
                            enable_icon,
                            false,
                            false,
                        )
                        .when(!t.enabled, |d| d.text_color(c(hex::INK_FAINT)))
                        .tooltip(tip(enable_tip, Some("M".into())))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_track_enabled(id, cx)),
                        ),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!("{}-lock", t.label)),
                            if t.locked { "lock" } else { "lock-open" },
                            false,
                            false,
                        )
                        .text_color(c(if t.locked { hex::INK } else { hex::INK_FAINT }))
                        .tooltip(tip(
                            if t.locked {
                                "Unlock track"
                            } else {
                                "Lock track"
                            },
                            None,
                        ))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_track_locked(id, cx)),
                        ),
                    )
            }))
    }

    fn render_ruler(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let fps = self.fps();
        let w = self.lanes_width();
        let ppf = self.tl.ppf;
        let step = [1, 2, 6, 12, 24, 48, 120, 240, 480, 720, 1440, 2880, 7200]
            .into_iter()
            .map(|s| s * fps.nominal() / 24)
            .find(|s| *s as f32 * ppf >= 86.)
            .unwrap_or(7200)
            .max(1);
        let minor = if step % 6 == 0 {
            step / 6
        } else {
            (step / 2).max(1)
        };
        let f0 = (self.tl.scroll_x / ppf) as Frame / minor * minor;
        let f1 = ((self.tl.scroll_x + w) / ppf) as Frame + minor;

        let mut ticks: Vec<AnyElement> = Vec::new();
        let mut f = f0;
        while f <= f1 {
            let x = self.frame_x(f);
            let major = f % step == 0;
            ticks.push(
                div()
                    .absolute()
                    .left(px(x))
                    .bottom_0()
                    .w(px(1.))
                    .h(px(if major { 14. } else { 7. }))
                    .bg(c(if major { 0x5a5a62 } else { 0x34343a }))
                    .into_any_element(),
            );
            if major {
                ticks.push(
                    div()
                        .absolute()
                        .left(px(x + 4.))
                        .top(px(3.))
                        .font_family(theme::MONO)
                        .text_color(c(hex::INK_DIM))
                        .whitespace_nowrap()
                        .child(time::timecode_short(f, fps))
                        .into_any_element(),
                );
            }
            f += minor;
        }

        let guide = |text: String| {
            div()
                .absolute()
                .top_0()
                .h_full()
                .flex()
                .items_center()
                .px(px(6.))
                .bg(c(hex::PLUGE_BLACK))
                .border_color(c(hex::LINE))
                .font_family(theme::MONO)
                .font_weight(FontWeight::MEDIUM)
                .child(text)
        };
        let first = (self.tl.scroll_x / ppf).floor() as Frame;
        let last = ((self.tl.scroll_x + w) / ppf).floor() as Frame;
        let ph = self.frame_x(self.playhead);

        div()
            .id("ruler")
            .h(px(RULER_H))
            .flex_none()
            .relative()
            .overflow_hidden()
            .border_b_1()
            .border_color(c(hex::LINE))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.seek(this.frame_at(e.position.x));
                    this.tl.drag = Some(Drag::Scrub);
                    cx.notify();
                }),
            )
            .children(ticks)
            .child(guide(self.tc(first)).left_0().border_r_1())
            .child(guide(self.tc(last)).right_0().border_l_1())
            .when(ph >= -6. && ph <= w + 6., |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(ph - 5.))
                        .top_0()
                        .w(px(11.))
                        .h(px(9.))
                        .rounded_b(px(3.))
                        .bg(c(hex::BONE)),
                )
            })
    }

    fn render_lanes(
        &self,
        tracks: &[Track],
        lanes_h: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let w = self.lanes_width();
        let fps = self.fps();
        let mut children: Vec<AnyElement> = Vec::new();

        // Visible one-second grid while snapping.
        let sec_px = fps.nominal() as f32 * self.tl.ppf;
        if self.snap && sec_px >= 12. {
            let s0 = (self.tl.scroll_x / sec_px).floor() as i64;
            let s1 = ((self.tl.scroll_x + w) / sec_px).ceil() as i64;
            for s in s0..=s1 {
                children.push(
                    div()
                        .absolute()
                        .left(px(s as f32 * sec_px - self.tl.scroll_x))
                        .top_0()
                        .h(px(lanes_h))
                        .w(px(1.))
                        .bg(gpui_kit::hsla(0., 0., 1., 0.035))
                        .into_any_element(),
                );
            }
        }

        let mut y = 0.;
        for t in tracks {
            let h = track_height(t);
            children.push(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(y + h - 1.))
                    .h(px(1.))
                    .bg(c(hex::LINE))
                    .into_any_element(),
            );
            for clip in self.project().clips_on(t.id) {
                let x = self.frame_x(clip.start);
                let cw = clip.duration as f32 * self.tl.ppf;
                if x + cw < 0. || x > w {
                    continue;
                }
                children.push(self.render_clip(clip, t, x, y, cw, h).into_any_element());
            }
            y += h;
        }

        if let Some(f) = self.tl.snap_line {
            children.push(vline(self.frame_x(f), lanes_h, c(0xc9c43c)).into_any_element());
        }
        if self.tool == Tool::Blade
            && let Some(f) = self.tl.hover_frame
        {
            children.push(vline(self.frame_x(f), lanes_h, c(0xd0463f)).into_any_element());
        }
        let ph = self.frame_x(self.playhead);
        if ph >= 0. && ph <= w {
            children.push(vline(ph, lanes_h, c(hex::BONE)).into_any_element());
        }

        let empty = self.project().sequence.clips.is_empty();
        let bounds = self.tl.bounds.clone();

        div()
            .id("lanes")
            .flex_1()
            .relative()
            .overflow_hidden()
            .when(self.tool == Tool::Blade, |d| {
                d.cursor(CursorStyle::Crosshair)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.lanes_mouse_down(e, window, cx)
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.lanes_hover(e, cx)))
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(children)
            .when(empty, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(lanes_h))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .text_color(c(hex::INK_DIM))
                        .child(
                            div()
                                .text_color(c(hex::INK))
                                .font_weight(FontWeight::MEDIUM)
                                .child("The timeline is empty"),
                        )
                        .child("Open the demo project from the bin, or open a .vde project"),
                )
            })
    }

    fn render_clip(
        &self,
        clip: &Clip,
        track: &Track,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> impl IntoElement + use<> {
        let p = self.project();
        let selected = self.selection == Some(clip.id);
        let media = clip.media.and_then(|m| p.media(m));
        let offline = media.is_some_and(|m| m.offline);
        let muted = !track.enabled;
        let name = p.clip_name(clip);
        let detail: String = match (clip.kind, media) {
            (ClipKind::Text, _) => {
                let t = clip
                    .first(NodeKind::Text)
                    .expect("text clips start with Text");
                format!("{} px · {}", t.number("size"), t.text("weight"))
            }
            (_, Some(m)) => format!(
                "{} · in {}",
                m.file_name(),
                time::timecode_short(clip.source_in, p.sequence.fps)
            ),
            _ => String::new(),
        };

        let border = if selected {
            c(hex::BONE)
        } else if offline {
            c(hex::WARN)
        } else {
            c(hex::LINE_STRONG)
        };
        let body_bg = match clip.kind {
            ClipKind::Audio => c(0x221a24),
            ClipKind::Text => c(0x241b1b),
            ClipKind::Video => c(hex::PLUGE_CONTROL),
        };

        div()
            .absolute()
            .left(px(x))
            .top(px(y + 3.))
            .w(px(w.max(2.)))
            .h(px(h - 7.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .when(offline, |d| d.border_dashed())
            .bg(if muted || offline {
                c(hex::PLUGE_RAISED)
            } else {
                body_bg
            })
            .when(selected, |d| {
                d.shadow(vec![BoxShadow {
                    color: c(hex::BONE),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                }])
            })
            .when(track.locked, |d| d.opacity(0.7))
            .child(
                div()
                    .h(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(6.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_weight(FontWeight::MEDIUM)
                    .when(track.locked, |d| {
                        d.child(ui::icon("lock", px(12.), c(hex::INK_DIM)))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .when(offline, |d| d.text_color(c(hex::WARN)))
                            .when(muted, |d| d.line_through().text_color(c(hex::INK_DIM)))
                            .child(name),
                    )
                    .when(offline, |d| {
                        d.child(
                            div()
                                .font_family(theme::MONO)
                                .text_color(c(hex::INK_DIM))
                                .child("offline"),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(6.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(c(hex::INK_FAINT))
                    .child(detail),
            )
            .child(
                div()
                    .h(px(5.))
                    .flex_none()
                    .flex()
                    .gap(px(1.))
                    .children(clip.nodes.iter().map(|n| {
                        div()
                            .flex_1()
                            .h_full()
                            .bg(n.kind.category().color())
                            .when(n.bypass, |d| d.opacity(0.25))
                    })),
            )
            .child(edge_handle(true))
            .child(edge_handle(false))
    }
}

fn vline(x: f32, h: f32, color: Hsla) -> impl IntoElement {
    div()
        .absolute()
        .left(px(x))
        .top_0()
        .w(px(1.))
        .h(px(h))
        .bg(color)
}

fn edge_handle(left: bool) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(EDGE_PX))
        .when(left, |d| d.left_0())
        .when(!left, |d| d.right_0())
        .cursor(CursorStyle::ResizeLeftRight)
}
