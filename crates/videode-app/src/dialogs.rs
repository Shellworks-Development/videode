//! Dialogs over the editor (Settings, About) and the launch splash.

use std::time::Instant;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnimationExt as _, AnyElement, Context, FontWeight, MouseButton, SharedString, Window, div,
    hsla, px,
};
use videode_core::Animations;
use videode_core::settings::ZOOM_STEPS;

use crate::brand::{self, SPLASH_FADE, SPLASH_MARK, SPLASH_WORD};
use crate::motion::{self, anim, ease_out, hover_fx, key, lerp, switch};
use crate::settings;
use crate::theme::{self, c, hex};
use crate::ui::{button, icon_button, kbd, tip};
use crate::workspace::{Modal, Workspace};

/// How long the About mark takes to build.
const ABOUT_BUILD: f32 = 0.7;

/// Brand surfaces (splash, About) may use this display size; the shell keeps
/// to the two-size rule.
const WORDMARK: f32 = 24.;

fn segmented<T: Copy + PartialEq + 'static>(
    id: &str,
    options: &[(T, &str)],
    current: T,
    on_pick: impl Fn(T, &mut gpui_kit::App) + Clone + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .p(px(1.))
        .rounded(px(6.))
        .border_1()
        .border_color(c(hex::LINE))
        .bg(c(hex::PLUGE_SUB))
        .children(options.iter().enumerate().map(|(i, &(value, label))| {
            let selected = value == current;
            let item_id = SharedString::from(format!("{id}-{i}"));
            let on_pick = on_pick.clone();
            div()
                .id(item_id.clone())
                .h(px(24.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .cursor_pointer()
                .text_color(c(if selected { hex::INK } else { hex::INK_DIM }))
                .when(selected, |d| d.bg(c(hex::PLUGE_CONTROL)))
                .map(|d| hover_fx(d, item_id, c(hex::PLUGE_RAISED), px(5.)))
                .hover(|s| s.text_color(c(hex::INK)))
                .on_click(move |_, _, cx| on_pick(value, cx))
                .child(label.to_string())
        }))
}

fn setting_row(title: &'static str, control: impl IntoElement) -> impl IntoElement {
    div()
        .h(px(34.))
        .flex()
        .items_center()
        .gap(px(12.))
        .child(div().flex_1().min_w_0().child(title))
        .child(div().flex_none().child(control))
}

fn section_title(text: &'static str) -> impl IntoElement {
    div()
        .pt(px(10.))
        .pb(px(2.))
        .text_color(c(hex::INK_FAINT))
        .child(text)
}

impl Workspace {
    pub(crate) fn close_modal(&mut self, cx: &mut Context<Self>) {
        self.modal = None;
        cx.notify();
    }

    pub(crate) fn render_modal(&self, modal: Modal, cx: &mut Context<Self>) -> AnyElement {
        let (panel, width) = match modal {
            Modal::Settings => (self.settings_panel(cx), 420.),
            Modal::About => (self.about_panel(cx), 340.),
        };
        let panel = div()
            .id("modal-panel")
            .w(px(width))
            .max_h(gpui_kit::relative(0.85))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(c(hex::LINE_STRONG))
            .bg(c(hex::PLUGE_RAISED))
            .shadow(vec![gpui_kit::BoxShadow {
                color: hsla(0., 0., 0., 0.6),
                offset: gpui_kit::point(px(0.), px(18.)),
                blur_radius: px(48.),
                spread_radius: px(0.),
                inset: false,
            }])
            // Clicks inside the panel don't reach the backdrop.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(panel)
            .with_animation(key("modal", modal as u64), anim(motion::DIALOG), |d, t| {
                d.opacity(t).mt(px(lerp(10., 0., t)))
            });

        div()
            .id("modal-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .bg(hsla(0., 0., 0., 0.5))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_modal(cx)),
            )
            .child(panel)
            .with_animation(
                key("backdrop", modal as u64),
                anim(motion::DIALOG),
                |d, t| d.opacity(t),
            )
            .into_any_element()
    }

    fn dialog_header(&self, title: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(8.))
            .border_b_1()
            .border_color(c(hex::LINE))
            .child(
                div()
                    .flex_1()
                    .text_size(theme::TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(kbd("Esc"))
            .child(
                icon_button("modal-close", "close", false, false)
                    .tooltip(tip("Close", Some("Esc".into())))
                    .on_click(cx.listener(|this, _, _, cx| this.close_modal(cx))),
            )
    }

    fn settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let app = cx.global::<settings::AppSettings>();
        let s = app.settings.clone();
        let notes = app.notes.clone();
        let refresh = cx.entity().downgrade();
        let changed = move |cx: &mut gpui_kit::App| {
            refresh.update(cx, |_, cx| cx.notify()).ok();
        };

        let animations = {
            let changed = changed.clone();
            segmented(
                "set-anim",
                &[
                    (Animations::System, "Follow system"),
                    (Animations::On, "On"),
                    (Animations::Off, "Off"),
                ],
                s.animations,
                move |a, cx| {
                    settings::update(cx, |s| s.animations = a);
                    changed(cx);
                },
            )
        };
        let splash = {
            let changed = changed.clone();
            switch("set-splash", s.splash).on_click(move |_, _, cx| {
                settings::update(cx, |s| s.splash = !s.splash);
                changed(cx);
            })
        };
        let snap = {
            let changed = changed.clone();
            switch("set-snap", s.snap).on_click(move |_, _, cx| {
                settings::update(cx, |s| s.snap = !s.snap);
                changed(cx);
            })
        };
        let zoom = segmented(
            "set-zoom",
            &[
                (ZOOM_STEPS[0], "1.25×"),
                (ZOOM_STEPS[1], "1.4×"),
                (ZOOM_STEPS[2], "2×"),
            ],
            s.zoom_step,
            move |z, cx| {
                settings::update(cx, |s| s.zoom_step = z);
                changed(cx);
            },
        );

        div()
            .flex()
            .flex_col()
            .min_h_0()
            .child(self.dialog_header("Settings", cx))
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(14.))
                    .pb(px(8.))
                    .child(section_title("Appearance"))
                    .child(setting_row("Animations", animations))
                    .child(setting_row("Splash on launch", splash))
                    .child(section_title("Timeline"))
                    .child(setting_row("Snap by default", snap))
                    .child(setting_row("Zoom step", zoom))
                    .children(
                        notes
                            .into_iter()
                            .map(|n| div().pt(px(8.)).text_color(c(hex::WARN)).child(n)),
                    ),
            )
            .into_any_element()
    }

    fn about_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let progress = self.timed(self.modal_opened, ABOUT_BUILD, cx);
        div()
            .flex()
            .flex_col()
            .child(self.dialog_header("About", cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(18.))
                    .py(px(16.))
                    .child(brand::mark(56., progress))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(WORDMARK))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_height(px(30.))
                                    .child("videode"),
                            )
                            .child(
                                div()
                                    .text_color(c(hex::INK_DIM))
                                    .child("by Shellworks Development"),
                            )
                            .child(
                                div()
                                    .pt(px(2.))
                                    .font_family(theme::MONO)
                                    .text_color(c(hex::INK_FAINT))
                                    .child(format!(
                                        "{} · {} {}",
                                        env!("CARGO_PKG_VERSION"),
                                        std::env::consts::OS,
                                        std::env::consts::ARCH
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(18.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(c(hex::LINE))
                    .text_color(c(hex::INK_FAINT))
                    .child(div().flex_1().child("Special thanks to FFmpeg"))
                    .child(
                        button("about-done", None, "Done", false)
                            .on_click(cx.listener(|this, _, _, cx| this.close_modal(cx))),
                    ),
            )
            .into_any_element()
    }

    /// 0→1 over `seconds` since `since`, or 1 straight away without motion.
    /// Keeps frames coming while it runs.
    fn timed(&self, since: Instant, seconds: f32, cx: &mut Context<Self>) -> f32 {
        if !settings::motion(cx) {
            return 1.;
        }
        let t = since.elapsed().as_secs_f32() / seconds;
        if t < 1. {
            cx.notify();
        }
        t.min(1.)
    }

    pub(crate) fn render_splash(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let started = self.splash?;
        let t = started.elapsed().as_secs_f32();
        if t >= SPLASH_FADE.1 || !settings::motion(cx) {
            self.splash = None;
            return None;
        }
        window.request_animation_frame();
        let phase = |(a, b): (f32, f32)| ease_out(((t - a) / (b - a)).clamp(0., 1.));
        let word = phase(SPLASH_WORD);
        let fade = phase(SPLASH_FADE);
        Some(
            div()
                .id("splash")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .bg(c(hex::PLUGE_SUB))
                .opacity(1. - fade)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.splash = None;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(brand::mark(88., (t / SPLASH_MARK).min(1.)))
                        .child(
                            div()
                                .ml(px(lerp(-8., 0., word)))
                                .opacity(word)
                                .text_size(px(WORDMARK * 1.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("videode"),
                        ),
                )
                .into_any_element(),
        )
    }
}
