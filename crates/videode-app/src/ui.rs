//! Small shared pieces of the Test Card UI: icons, keycaps, buttons, swatches.

use gpui_kit::component::{Icon, tooltip::Tooltip};
use gpui_kit::prelude::*;
use gpui_kit::{Div, FontWeight, Hsla, Pixels, SharedString, Stateful, div, px};

use crate::motion::hover_fx;
use crate::theme::{self, Category, CategoryExt, c, hex};

pub fn cmd_key(key: &str) -> SharedString {
    if cfg!(target_os = "macos") {
        format!("⌘ {key}").into()
    } else {
        format!("Ctrl {key}").into()
    }
}

pub fn icon(name: &str, size: Pixels, color: Hsla) -> Icon {
    Icon::empty()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(size)
        .text_color(color)
}

pub fn swatch(cat: Category) -> Div {
    div()
        .w(px(8.))
        .h(px(8.))
        .flex_none()
        .rounded(px(2.))
        .bg(cat.color())
}

pub fn kbd(label: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_b(px(2.))
        .border_color(c(hex::LINE_STRONG))
        .bg(c(hex::PLUGE_BLACK))
        .font_family(theme::MONO)
        .text_size(theme::KEYCAP)
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(hex::INK_DIM))
        .child(label.into())
}

pub fn pane_header() -> Div {
    div()
        .h(px(32.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(6.))
        .border_b_1()
        .border_color(c(hex::LINE))
}

/// A tooltip that names the action and, when given, its shortcut.
pub fn tip(
    text: impl Into<SharedString>,
    key: Option<SharedString>,
) -> impl Fn(&mut gpui_kit::Window, &mut gpui_kit::App) -> gpui_kit::AnyView + 'static {
    let text = text.into();
    move |window, cx| {
        let label: SharedString = match &key {
            Some(k) => format!("{text}   {k}").into(),
            None => text.clone(),
        };
        Tooltip::new(label).build(window, cx)
    }
}

pub fn icon_button(
    id: impl Into<SharedString>,
    icon_name: &str,
    pressed: bool,
    disabled: bool,
) -> Stateful<Div> {
    let id = id.into();
    let el = div()
        .id(id.clone())
        .w(px(28.))
        .h(px(26.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(c(if pressed { hex::INK } else { hex::INK_DIM }))
        .when(pressed, |d| d.bg(c(hex::PLUGE_HOVER)))
        .when(disabled, |d| d.opacity(0.35));
    let el = if disabled {
        el
    } else {
        let hot = c(if pressed {
            hex::PLUGE_HOVER
        } else {
            hex::PLUGE_CONTROL
        });
        hover_fx(el, id, hot, px(5.))
            .cursor_pointer()
            .hover(|s| s.text_color(c(hex::INK)))
            .active(|s| s.opacity(0.75))
    };
    el.child(
        Icon::empty()
            .path(SharedString::from(format!("icons/{icon_name}.svg")))
            .size(px(16.)),
    )
}

pub fn button(
    id: impl Into<SharedString>,
    icon_name: Option<&str>,
    label: impl Into<SharedString>,
    primary: bool,
) -> Stateful<Div> {
    let (bg, fg, border) = if primary {
        (hex::BONE, hex::PLUGE_SUB, hex::BONE)
    } else {
        (hex::PLUGE_RAISED, hex::INK, hex::LINE_STRONG)
    };
    let id = id.into();
    let hot = c(if primary {
        0xfffaf0
    } else {
        hex::PLUGE_CONTROL
    });
    let el = div()
        .id(id.clone())
        .h(px(26.))
        .px(px(10.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(5.))
        .border_1()
        .border_color(c(border))
        .bg(c(bg))
        .text_color(c(fg))
        .cursor_pointer()
        .active(|s| s.opacity(0.75))
        .when(primary, |d| d.font_weight(FontWeight::MEDIUM));
    hover_fx(el, id, hot, px(4.))
        .when_some(icon_name, |d, name| d.child(icon(name, px(16.), c(fg))))
        .child(label.into())
}

pub fn ghost_button(
    id: impl Into<SharedString>,
    icon_name: &str,
    label: &'static str,
    key: Option<&'static str>,
) -> Stateful<Div> {
    let id = id.into();
    let el = div()
        .id(id.clone())
        .h(px(26.))
        .px(px(8.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(5.))
        .text_color(c(hex::INK_DIM))
        .cursor_pointer()
        .hover(|s| s.text_color(c(hex::INK)))
        .active(|s| s.opacity(0.75));
    hover_fx(el, id, c(hex::PLUGE_CONTROL), px(5.))
        .child(icon(icon_name, px(16.), c(hex::INK_DIM)))
        .child(label)
        .when_some(key, |d, k| d.child(kbd(k)))
}

/// Label/value row used in the inspector.
pub fn field_row(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    mono: bool,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(28.))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_color(c(hex::INK_DIM))
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(24.))
                .px(px(7.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .border_1()
                .border_color(c(hex::LINE))
                .bg(c(hex::PLUGE_SUB))
                .when(mono, |d| d.font_family(theme::MONO))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(value.into()),
        )
}
