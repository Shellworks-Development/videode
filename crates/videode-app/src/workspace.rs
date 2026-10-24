//! The editor shell: top bar, media bin, viewer, inspector, timeline and
//! status bar, laid out as in `mockup/videode.html`.
//!
//! M0 renders the empty-project state. Editing, playback and the node views
//! arrive in later milestones.

use gpui_kit::component::{Icon, tooltip::Tooltip};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, Render, SharedString, Window, div, px,
    relative,
};

use crate::theme::{self, Category, c, hex};

const TOP_BAR_H: f32 = 38.;
const STATUS_H: f32 = 26.;
const BIN_W: f32 = 264.;
const INSPECTOR_W: f32 = 304.;
const TIMELINE_H: f32 = 340.;
const PANE_HEADER_H: f32 = 32.;
const TRACK_HEADS_W: f32 = 168.;
const RULER_H: f32 = 28.;
/// Ruler scale for the empty timeline: pixels per second.
const PX_PER_SEC: f32 = 24.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BinTab {
    Media,
    Nodes,
}

struct TrackHead {
    id: &'static str,
    name: &'static str,
    audio: bool,
    height: f32,
}

const TRACKS: [TrackHead; 4] = [
    TrackHead {
        id: "V2",
        name: "Titles",
        audio: false,
        height: 52.,
    },
    TrackHead {
        id: "V1",
        name: "Picture",
        audio: false,
        height: 64.,
    },
    TrackHead {
        id: "A1",
        name: "Voice",
        audio: true,
        height: 48.,
    },
    TrackHead {
        id: "A2",
        name: "Music",
        audio: true,
        height: 48.,
    },
];

/// Node types shown in the library, grouped by category.
const NODE_LIBRARY: [(Category, &str, &str); 8] = [
    (
        Category::Transform,
        "Transform",
        "Position, scale, rotation",
    ),
    (Category::Transform, "Crop", "Trim the frame edges"),
    (
        Category::Look,
        "Grade",
        "Exposure, white balance, saturation",
    ),
    (Category::Look, "Blur", "Gaussian blur"),
    (Category::Composite, "Opacity", "Opacity and blend mode"),
    (Category::Composite, "Fade", "Fade in and out over frames"),
    (Category::Audio, "Gain", "Level in decibels"),
    (
        Category::Audio,
        "Duck",
        "Lower this clip under another track",
    ),
];

pub struct Workspace {
    bin_tab: BinTab,
}

impl Workspace {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            bin_tab: BinTab::Media,
        }
    }

    fn top_bar(&self) -> impl IntoElement {
        let logo = div().flex().gap(px(2.)).mr(px(6.)).children(
            Category::ALL.map(|cat| div().w(px(3.)).h(px(15.)).rounded(px(1.)).bg(cat.color())),
        );
        let menus = ["File", "Edit", "Clip", "View", "Playback", "Help"].map(|name| {
            div()
                .id(SharedString::from(format!("menu-{name}")))
                .h(px(26.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .text_color(c(hex::INK_DIM))
                .hover(|s| s.bg(c(hex::PLUGE_CONTROL)).text_color(c(hex::INK)))
                .child(name)
        });
        let crumbs = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .ml(px(14.))
            .flex_shrink_0()
            .whitespace_nowrap()
            .text_color(c(hex::INK_DIM))
            .child("Untitled project")
            .child(icon("chevron-right", px(12.), c(hex::INK_FAINT)))
            .child(
                div()
                    .text_color(c(hex::INK))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Main Sequence"),
            );

        let command = div()
            .id("command-palette")
            .w(px(340.))
            .min_w(px(150.))
            .flex_shrink(1.)
            .h(px(26.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(8.))
            .pr(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(c(hex::LINE_STRONG))
            .bg(c(hex::PLUGE_SUB))
            .text_color(c(hex::INK_DIM))
            .hover(|s| s.text_color(c(hex::INK)))
            .tooltip(|window, cx| Tooltip::new("Every action, node and clip").build(window, cx))
            .child(icon("search", px(16.), c(hex::INK_DIM)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child("Search actions, nodes, clips…"),
            )
            .child(kbd(cmd_key("K")));

        div()
            .h(px(TOP_BAR_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .pl(px(12.))
            .pr(px(10.))
            .bg(c(hex::PLUGE_BLACK))
            .border_b_1()
            .border_color(c(hex::LINE))
            .child(logo)
            .child(
                div()
                    .mr(px(8.))
                    .text_size(theme::TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("videode"),
            )
            .child(div().flex().children(menus))
            .child(crumbs)
            .child(
                div()
                    .ml_auto()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(command)
                    .child(icon_button("undo", "undo", "Nothing to undo", true))
                    .child(icon_button("redo", "redo", "Nothing to redo", true))
                    .child(button("export", Some("upload"), "Export", true).tooltip(
                        |window, cx| Tooltip::new("Open the Output node").build(window, cx),
                    )),
            )
    }

    fn bin(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str,
                   label: &'static str,
                   which: BinTab,
                   this: &Self,
                   cx: &mut Context<Self>| {
            let selected = this.bin_tab == which;
            div()
                .id(id)
                .h(px(24.))
                .px(px(9.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(if selected { hex::INK } else { hex::INK_DIM }))
                .when(selected, |d| d.bg(c(hex::PLUGE_CONTROL)))
                .hover(|s| s.text_color(c(hex::INK)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.bin_tab = which;
                    cx.notify();
                }))
                .child(label)
        };

        let header = pane_header()
            .child(tab("bin-media", "Media", BinTab::Media, self, cx))
            .child(tab("bin-nodes", "Nodes", BinTab::Nodes, self, cx))
            .child(div().flex_1())
            .child(icon_button(
                "import",
                "import",
                &format!("Import media  {}", cmd_key("I")),
                false,
            ));

        let filter = div()
            .m(px(8.))
            .h(px(26.))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(c(hex::LINE))
            .bg(c(hex::PLUGE_SUB))
            .text_color(c(hex::INK_FAINT))
            .child(icon("search", px(16.), c(hex::INK_FAINT)))
            .child(div().flex_1().child(match self.bin_tab {
                BinTab::Media => "Filter media",
                BinTab::Nodes => "Filter nodes",
            }))
            .child(kbd("/".into()));

        let body: AnyElement = match self.bin_tab {
            BinTab::Media => div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .px(px(16.))
                .text_color(c(hex::INK_DIM))
                .child(icon("film", px(24.), c(hex::INK_FAINT)))
                .child(
                    div()
                        .text_color(c(hex::INK))
                        .font_weight(FontWeight::MEDIUM)
                        .child("No media yet"),
                )
                .child(
                    div()
                        .text_center()
                        .child("Drop files anywhere on the window, or import them."),
                )
                .child(button("bin-import", Some("import"), "Import media", false))
                .into_any_element(),
            BinTab::Nodes => {
                let mut list = div().flex_1().flex().flex_col().px(px(8.));
                let mut last: Option<Category> = None;
                for (cat, name, desc) in NODE_LIBRARY {
                    if last != Some(cat) {
                        list = list.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .pt(px(10.))
                                .pb(px(4.))
                                .px(px(2.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c(hex::INK_DIM))
                                .child(swatch(cat))
                                .child(cat.label()),
                        );
                        last = Some(cat);
                    }
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("node-{name}")))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(5.))
                            .hover(|s| s.bg(c(hex::PLUGE_RAISED)))
                            .child(swatch(cat))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().font_weight(FontWeight::MEDIUM).child(name))
                                    .child(div().text_color(c(hex::INK_FAINT)).child(desc)),
                            ),
                    );
                }
                list.into_any_element()
            }
        };

        let footer = div()
            .h(px(26.))
            .flex_none()
            .flex()
            .items_center()
            .px(px(10.))
            .border_t_1()
            .border_color(c(hex::LINE))
            .text_color(c(hex::INK_FAINT))
            .child(match self.bin_tab {
                BinTab::Media => "0 items",
                BinTab::Nodes => "Select a clip, then click a node to add it",
            });

        div()
            .w(px(BIN_W))
            .flex_none()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_BLACK))
            .border_r_1()
            .border_color(c(hex::LINE))
            .child(header)
            .child(filter)
            .child(body)
            .child(footer)
    }

    fn viewer(&self) -> impl IntoElement {
        let screen = div()
            .h_full()
            .max_w_full()
            .aspect_ratio(16. / 9.)
            .bg(gpui_kit::black())
            .border_1()
            .border_color(c(hex::LINE))
            .flex()
            .items_center()
            .justify_center()
            .text_color(c(hex::INK_FAINT))
            .child("No picture at playhead");

        let transport = div()
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .px(px(14.))
            .bg(c(hex::PLUGE_BLACK))
            .border_t_1()
            .border_color(c(hex::LINE))
            .child(
                div()
                    .flex_1()
                    .font_family(theme::MONO)
                    .text_size(theme::TITLE)
                    .font_weight(FontWeight::MEDIUM)
                    .child("00:00:00:00"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .child(icon_button(
                        "to-start",
                        "skip-back",
                        "Go to start  Home",
                        false,
                    ))
                    .child(icon_button(
                        "step-back",
                        "step-back",
                        "Previous frame  ←",
                        false,
                    ))
                    .child(
                        icon_button("play", "play", "Play / pause  Space", false)
                            .bg(c(hex::PLUGE_CONTROL))
                            .text_color(c(hex::INK)),
                    )
                    .child(icon_button(
                        "step-fwd",
                        "step-forward",
                        "Next frame  →",
                        false,
                    ))
                    .child(icon_button(
                        "to-end",
                        "skip-forward",
                        "Go to end  End",
                        false,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .font_family(theme::MONO)
                    .text_color(c(hex::INK_DIM))
                    .child("00:00:00:00"),
            );

        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_SUB))
            .child(
                div()
                    .h(px(PANE_HEADER_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .gap(px(8.))
                    .text_color(c(hex::INK_DIM))
                    .child("Main Sequence")
                    .child(div().flex_1())
                    .child(div().font_family(theme::MONO).child("1920×1080 · 24 fps")),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(16.))
                    .pt(px(4.))
                    .pb(px(8.))
                    .child(screen),
            )
            .child(transport)
    }

    fn inspector(&self) -> impl IntoElement {
        let row = |label: &'static str, value: &'static str| {
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
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(theme::MONO)
                        .text_color(c(hex::INK_DIM))
                        .text_ellipsis()
                        .child(value),
                )
        };
        let section_head = |cat: Category, title: &'static str| {
            div()
                .h(px(34.))
                .flex()
                .items_center()
                .gap(px(7.))
                .px(px(12.))
                .child(swatch(cat))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        };
        let hint = |text: &'static str, key: SharedString| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(text)
                .child(kbd(key))
        };

        div()
            .w(px(INSPECTOR_W))
            .flex_none()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_BLACK))
            .border_l_1()
            .border_color(c(hex::LINE))
            .child(
                pane_header().child(
                    div()
                        .h(px(24.))
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .bg(c(hex::PLUGE_CONTROL))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Inspector"),
                ),
            )
            .child(
                div()
                    .px(px(12.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .border_b_1()
                    .border_color(c(hex::LINE))
                    .child(
                        div()
                            .text_size(theme::TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Main Sequence"),
                    )
                    .child(
                        div()
                            .text_color(c(hex::INK_DIM))
                            .child("Sequence node · empty"),
                    ),
            )
            .child(
                div()
                    .border_b_1()
                    .border_color(c(hex::LINE))
                    .child(section_head(Category::Composite, "Sequence"))
                    .child(
                        div()
                            .px(px(12.))
                            .pb(px(10.))
                            .child(row("Resolution", "1920 × 1080"))
                            .child(row("Frame rate", "24 fps"))
                            .child(row("Duration", "00:00:00:00"))
                            .child(row("Color space", "Rec.709 · gamma 2.4")),
                    ),
            )
            .child(
                div()
                    .border_b_1()
                    .border_color(c(hex::LINE))
                    .child(section_head(Category::Output, "Output"))
                    .child(
                        div()
                            .px(px(12.))
                            .pb(px(10.))
                            .text_color(c(hex::INK_DIM))
                            .child("MP4 · H.264 · 1920×1080"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(14.))
                    .text_color(c(hex::INK_DIM))
                    .child(hint("Select a clip to see its nodes", "click".into()))
                    .child(hint("Peek inside the selected clip", "Tab".into()))
                    .child(hint("See the whole sequence as a graph", "⇧ Tab".into()))
                    .child(hint("Find any action", cmd_key("K"))),
            )
    }

    fn timeline(&self) -> impl IntoElement {
        let toolbar = div()
            .h(px(PANE_HEADER_H))
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
                        icon_button("tool-select", "mouse-pointer", "Select and move  V", false)
                            .bg(c(hex::PLUGE_HOVER)),
                    )
                    .child(icon_button(
                        "tool-blade",
                        "scissors",
                        "Blade: click a clip to cut  B",
                        false,
                    )),
            )
            .child(
                icon_button(
                    "snap",
                    "magnet",
                    "Snap to grid, edges and playhead  N",
                    false,
                )
                .bg(c(hex::PLUGE_HOVER))
                .text_color(c(hex::INK)),
            )
            .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(c(hex::LINE)))
            .child(ghost_button("split", "scissors", "Split", Some("S")))
            .child(ghost_button("peek", "layers", "Peek nodes", Some("Tab")))
            .child(ghost_button("graph", "workflow", "Graph", Some("⇧ Tab")))
            .child(div().flex_1())
            .child(icon_button("zoom-out", "minus", "Zoom out  −", false))
            .child(icon_button("zoom-in", "plus", "Zoom in  =", false))
            .child(icon_button(
                "zoom-fit",
                "maximize",
                "Fit sequence  ⇧ Z",
                false,
            ));

        let heads = div()
            .w(px(TRACK_HEADS_W))
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
                    .child("00:00:00:00"),
            )
            .children(TRACKS.iter().map(|t| {
                div()
                    .h(px(t.height))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .border_b_1()
                    .border_color(c(hex::LINE))
                    .child(
                        div()
                            .w(px(22.))
                            .font_family(theme::MONO)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t.id),
                    )
                    .child(div().flex_1().text_ellipsis().child(t.name))
                    .child(icon_button(
                        &format!("{}-enable", t.id),
                        if t.audio { "volume-2" } else { "eye" },
                        if t.audio {
                            "Mute track  M"
                        } else {
                            "Hide track  M"
                        },
                        false,
                    ))
                    .child(
                        icon_button(&format!("{}-lock", t.id), "lock-open", "Lock track", false)
                            .text_color(c(hex::INK_FAINT)),
                    )
            }));

        let ticks = (0..=180).filter(|s| s % 5 == 0).map(|sec| {
            let x = sec as f32 * PX_PER_SEC;
            div()
                .absolute()
                .left(px(x))
                .top_0()
                .h_full()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .w(px(1.))
                        .h(px(14.))
                        .bg(c(0x5a5a62)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .top(px(3.))
                        .font_family(theme::MONO)
                        .text_color(c(hex::INK_DIM))
                        .whitespace_nowrap()
                        .child(format!("{}:{:02}", sec / 60, sec % 60)),
                )
        });
        let minor = (0..=180).filter(|s| s % 5 != 0).map(|sec| {
            div()
                .absolute()
                .left(px(sec as f32 * PX_PER_SEC))
                .bottom_0()
                .w(px(1.))
                .h(px(7.))
                .bg(c(0x34343a))
        });
        let ruler = div()
            .h(px(RULER_H))
            .flex_none()
            .relative()
            .overflow_hidden()
            .border_b_1()
            .border_color(c(hex::LINE))
            .children(minor)
            .children(ticks)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .bg(c(hex::PLUGE_BLACK))
                    .border_r_1()
                    .border_color(c(hex::LINE))
                    .font_family(theme::MONO)
                    .font_weight(FontWeight::MEDIUM)
                    .child("00:00:00:00"),
            );

        let lanes = div()
            .flex_1()
            .relative()
            .children(TRACKS.iter().map(|t| {
                div()
                    .h(px(t.height))
                    .flex_none()
                    .border_b_1()
                    .border_color(c(hex::LINE))
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
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
                    .child(format!(
                        "Drag media here from the bin, or press {} to import",
                        cmd_key("I")
                    )),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(px(1.))
                    .bg(c(hex::BONE)),
            );

        div()
            .h(px(TIMELINE_H))
            .flex_none()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_BLACK))
            .child(toolbar)
            .child(
                div().flex_1().min_h_0().flex().child(heads).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(ruler)
                        .child(lanes),
                ),
            )
    }

    fn status_bar(&self) -> impl IntoElement {
        let item = |icon_name: &'static str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon(icon_name, px(14.), c(hex::INK_DIM)))
                .child(label)
        };
        div()
            .h(px(STATUS_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(10.))
            .bg(c(hex::PLUGE_BLACK))
            .border_t_1()
            .border_color(c(hex::LINE))
            .text_color(c(hex::INK_DIM))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child("New project · import media to start")
                    .child(kbd(cmd_key("I")))
                    .child("· clips are made of nodes, press")
                    .child(kbd("Tab".into()))
                    .child("on a clip to peek inside"),
            )
            .child(item("magnet", "Snap · 1 s grid"))
            .child(item("workflow", "2 nodes"))
            .child(div().font_family(theme::MONO).child("1920×1080 · 24 fps"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(icon("keyboard", px(14.), c(hex::INK_DIM)))
                    .child("Shortcuts")
                    .child(kbd("?".into())),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_SUB))
            .text_color(c(hex::INK))
            .font_family(theme::FONT)
            .text_size(theme::TEXT)
            .line_height(relative(1.45))
            .child(self.top_bar())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.bin(cx))
                    .child(self.viewer())
                    .child(self.inspector()),
            )
            .child(
                div()
                    .h(px(5.))
                    .flex_none()
                    .bg(c(hex::PLUGE_SUB))
                    .border_t_1()
                    .border_color(c(hex::LINE)),
            )
            .child(self.timeline())
            .child(self.status_bar())
    }
}

/* ---------- small shared pieces ---------- */

fn cmd_key(key: &str) -> SharedString {
    if cfg!(target_os = "macos") {
        format!("⌘ {key}").into()
    } else {
        format!("Ctrl {key}").into()
    }
}

fn icon(name: &str, size: gpui_kit::Pixels, color: Hsla) -> Icon {
    Icon::empty()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(size)
        .text_color(color)
}

fn swatch(cat: Category) -> impl IntoElement {
    div()
        .w(px(8.))
        .h(px(8.))
        .flex_none()
        .rounded(px(2.))
        .bg(cat.color())
}

fn kbd(label: SharedString) -> impl IntoElement {
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
        .child(label)
}

fn pane_header() -> gpui_kit::Div {
    div()
        .h(px(PANE_HEADER_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(6.))
        .border_b_1()
        .border_color(c(hex::LINE))
}

fn icon_button(
    id: &str,
    icon_name: &str,
    tip: &str,
    disabled: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let tip = SharedString::from(tip.to_string());
    div()
        .id(SharedString::from(id.to_string()))
        .w(px(28.))
        .h(px(26.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(c(hex::INK_DIM))
        .when(disabled, |d| d.opacity(0.35))
        .when(!disabled, |d| {
            d.hover(|s| s.bg(c(hex::PLUGE_CONTROL)).text_color(c(hex::INK)))
        })
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(
            Icon::empty()
                .path(SharedString::from(format!("icons/{icon_name}.svg")))
                .size(px(16.)),
        )
}

fn button(
    id: &str,
    icon_name: Option<&str>,
    label: &'static str,
    primary: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let (bg, fg, border) = if primary {
        (hex::BONE, hex::PLUGE_SUB, hex::BONE)
    } else {
        (hex::PLUGE_RAISED, hex::INK, hex::LINE_STRONG)
    };
    div()
        .id(SharedString::from(id.to_string()))
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(5.))
        .border_1()
        .border_color(c(border))
        .bg(c(bg))
        .text_color(c(fg))
        .when(primary, |d| d.font_weight(FontWeight::MEDIUM))
        .hover(move |s| {
            s.bg(c(if primary {
                0xfffaf0
            } else {
                hex::PLUGE_CONTROL
            }))
        })
        .when_some(icon_name, |d, name| d.child(icon(name, px(16.), c(fg))))
        .child(label)
}

fn ghost_button(
    id: &str,
    icon_name: &str,
    label: &'static str,
    key: Option<&'static str>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(id.to_string()))
        .h(px(26.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(5.))
        .text_color(c(hex::INK_DIM))
        .hover(|s| s.bg(c(hex::PLUGE_CONTROL)).text_color(c(hex::INK)))
        .child(icon(icon_name, px(16.), c(hex::INK_DIM)))
        .child(label)
        .when_some(key, |d, k| d.child(kbd(k.into())))
}
