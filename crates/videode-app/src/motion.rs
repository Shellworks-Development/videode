//! Motion: one easing curve, the standard durations, and the hover layer.
//!
//! Everything here animates through GPUI's `with_animation`, which renders
//! the end state immediately when `reduce_motion` is set, so the Animations
//! setting switches all of it at once. See DESIGN.md › Motion.

use std::cell::RefCell;
use std::collections::HashSet;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, Div, ElementId, Hsla, Pixels, SharedString, Stateful, div, px,
};

use crate::theme::{c, hex};

pub const HOVER_IN: Duration = Duration::from_millis(120);
pub const HOVER_OUT: Duration = Duration::from_millis(90);
pub const STATUS: Duration = Duration::from_millis(180);
pub const ZOOM: Duration = Duration::from_millis(180);
pub const CUT: Duration = Duration::from_millis(380);
pub const FLASH: Duration = Duration::from_millis(520);
pub const DIALOG: Duration = Duration::from_millis(160);
pub const SWITCH: Duration = Duration::from_millis(140);

/// Ease-out quint, the stand-in for DESIGN.md's `cubic-bezier(.16,1,.3,1)`.
pub fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(5)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn anim(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(ease_out)
}

/// A key that changes whenever `state` does, so `with_animation` restarts.
pub fn key(name: impl Into<SharedString>, state: u64) -> ElementId {
    ElementId::NamedInteger(name.into(), state)
}

#[derive(Default)]
struct HoverState {
    current: Option<SharedString>,
    /// Elements that have ever been hovered or toggled. Others render
    /// statically, so nothing animates just because it appeared.
    touched: HashSet<SharedString>,
}

thread_local! {
    static HOVER: RefCell<HoverState> = RefCell::default();
}

fn touched(id: &SharedString) -> bool {
    HOVER.with(|h| h.borrow().touched.contains(id))
}

fn touch(id: &SharedString) {
    HOVER.with(|h| h.borrow_mut().touched.insert(id.clone()));
}

/// Adds an animated hover background to `el`. Call before adding children so
/// the layer paints underneath them. `el` becomes `relative`.
pub fn hover_fx(
    el: Stateful<Div>,
    id: impl Into<SharedString>,
    hot: Hsla,
    radius: Pixels,
) -> Stateful<Div> {
    let id = id.into();
    let hovered = HOVER.with(|h| h.borrow().current.as_ref() == Some(&id));
    let layer = touched(&id).then(|| {
        let (from, to, duration) = if hovered {
            (0., 1., HOVER_IN)
        } else {
            (1., 0., HOVER_OUT)
        };
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(radius)
            .bg(hot)
            .with_animation(
                key(format!("hover-{id}"), hovered as u64),
                anim(duration),
                move |d, t| d.opacity(lerp(from, to, t)),
            )
    });
    el.relative()
        .children(layer)
        .on_hover(move |&now, window, _| {
            HOVER.with(|h| {
                let mut h = h.borrow_mut();
                if now {
                    h.current = Some(id.clone());
                    h.touched.insert(id.clone());
                } else if h.current.as_ref() == Some(&id) {
                    h.current = None;
                }
            });
            window.refresh();
        })
}

/// An on/off switch whose knob slides when it changes.
pub fn switch(id: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let id: SharedString = id.into();
    let knob_x = |on: bool| if on { 14. } else { 2. };
    let knob = div()
        .absolute()
        .top(px(2.))
        .w(px(12.))
        .h(px(12.))
        .rounded_full()
        .bg(c(if on { hex::PLUGE_SUB } else { hex::INK_DIM }));
    let knob = if touched(&id) {
        let (from, to) = (knob_x(!on), knob_x(on));
        knob.with_animation(
            key(format!("switch-{id}"), on as u64),
            anim(SWITCH),
            move |d, t| d.left(px(lerp(from, to, t))),
        )
        .into_any_element()
    } else {
        knob.left(px(knob_x(on))).into_any_element()
    };
    let touch_id = id.clone();
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(30.))
        .h(px(18.))
        .rounded_full()
        .border_1()
        .cursor_pointer()
        .border_color(c(if on { hex::BONE } else { hex::LINE_STRONG }))
        .bg(c(if on { hex::BONE } else { hex::PLUGE_CONTROL }))
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, _| touch(&touch_id))
        .child(knob)
}
