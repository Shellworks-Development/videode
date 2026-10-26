//! The panels around the timeline: top bar with menus, media bin and node
//! library, viewer and transport, inspector, and status bar.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnimationExt as _, AnyElement, Context, FocusHandle, FontWeight, ImageSource, SharedString,
    canvas, div, img, px,
};
use videode_core::look::look_at;
use videode_core::{Clip, MediaId, MediaKind, NodeKind, time};

use crate::actions::*;
use crate::brand;
use crate::media::MediaEngine;
use crate::motion::{self, anim, hover_fx, key, lerp};
use crate::theme::{self, Category, CategoryExt, c, hex};
use crate::ui::{button, cmd_key, icon, icon_button, kbd, pane_header, swatch, tip};
use crate::workspace::{Area, BinTab, View, Workspace};

const TOP_BAR_H: f32 = 38.;
const STATUS_H: f32 = 26.;
const BIN_W: f32 = 264.;

type MenuBuilder = fn(PopupMenu, &Workspace) -> PopupMenu;

fn file_menu(m: PopupMenu, _: &Workspace) -> PopupMenu {
    m.menu("New project", Box::new(NewProject))
        .menu("Open…", Box::new(OpenProject))
        .menu("Open demo project", Box::new(OpenDemo))
        .separator()
        .menu("Import media…", Box::new(ImportMedia))
        .separator()
        .menu("Save", Box::new(SaveProject))
        .menu("Save as…", Box::new(SaveProjectAs))
        .separator()
        .menu("Export…", Box::new(Export))
        .separator()
        .menu("Settings…", Box::new(OpenSettings))
        .separator()
        .menu("Quit", Box::new(Quit))
}

fn edit_menu(m: PopupMenu, ws: &Workspace) -> PopupMenu {
    let undo = ws.doc.undo_label().map(|l| format!("Undo {l}"));
    let redo = ws.doc.redo_label().map(|l| format!("Redo {l}"));
    m.menu_with_disabled(
        undo.clone().unwrap_or("Undo".into()),
        Box::new(Undo),
        undo.is_none(),
    )
    .menu_with_disabled(
        redo.clone().unwrap_or("Redo".into()),
        Box::new(Redo),
        redo.is_none(),
    )
    .separator()
    .menu("Deselect", Box::new(Deselect))
}

fn clip_menu(m: PopupMenu, ws: &Workspace) -> PopupMenu {
    let none = ws.selection.is_none();
    m.menu("Split at playhead", Box::new(Split))
        .menu_with_disabled("Peek at nodes", Box::new(TogglePeek), none)
        .menu_with_disabled("Add node…", Box::new(AddNodeMenu), none)
        .menu_with_disabled(
            "Bypass or enable node",
            Box::new(BypassNode),
            ws.focused_node().is_none(),
        )
        .separator()
        .menu_with_disabled("Delete", Box::new(DeleteClip), none)
        .menu_with_disabled("Ripple delete", Box::new(RippleDelete), none)
        .separator()
        .menu_with_disabled("Mute or hide track", Box::new(ToggleTrack), none)
}

fn view_menu(m: PopupMenu, ws: &Workspace) -> PopupMenu {
    m.menu_with_check(
        "Select tool",
        ws.tool == crate::workspace::Tool::Select,
        Box::new(SelectTool),
    )
    .menu_with_check(
        "Blade tool",
        ws.tool == crate::workspace::Tool::Blade,
        Box::new(BladeTool),
    )
    .menu_with_check("Snapping", ws.snap, Box::new(ToggleSnap))
    .separator()
    .menu_with_check("Edit", ws.area == Area::Edit, Box::new(ShowEdit))
    .menu_with_check("Export", ws.area == Area::Export, Box::new(Export))
    .separator()
    .menu_with_check("Graph view", ws.view == View::Graph, Box::new(ToggleGraph))
    .menu("Command palette", Box::new(CommandPalette))
    .separator()
    .menu("Zoom in", Box::new(ZoomIn))
    .menu("Zoom out", Box::new(ZoomOut))
    .menu("Zoom to fit", Box::new(ZoomToFit))
}

fn playback_menu(m: PopupMenu, ws: &Workspace) -> PopupMenu {
    m.menu(
        if ws.is_playing() { "Pause" } else { "Play" },
        Box::new(TogglePlay),
    )
    .menu("Play backward / faster", Box::new(ShuttleBackward))
    .menu("Stop", Box::new(ShuttleStop))
    .menu("Play forward / faster", Box::new(ShuttleForward))
    .separator()
    .menu("Previous frame", Box::new(StepBack))
    .menu("Next frame", Box::new(StepForward))
    .menu("Back one second", Box::new(SecondBack))
    .menu("Forward one second", Box::new(SecondForward))
    .separator()
    .menu("Previous edit point", Box::new(PrevEdit))
    .menu("Next edit point", Box::new(NextEdit))
    .menu("Go to start", Box::new(GoToStart))
    .menu("Go to end", Box::new(GoToEnd))
}

fn help_menu(m: PopupMenu, _: &Workspace) -> PopupMenu {
    m.menu("Settings…", Box::new(OpenSettings))
        .separator()
        .menu("About videode", Box::new(About))
}

impl Workspace {
    fn menu(
        &self,
        name: &'static str,
        build: MenuBuilder,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        let focus: FocusHandle = self.focus.clone();
        Button::new(SharedString::from(format!("menu-{name}")))
            .label(name)
            .ghost()
            .small()
            .dropdown_menu(move |menu, _, cx| {
                let menu = menu.action_context(focus.clone()).min_w(px(220.));
                match this.upgrade() {
                    Some(ws) => build(menu, ws.read(cx)),
                    None => menu,
                }
            })
    }

    /// Edit and Export, as one segmented control.
    fn area_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tab = |id: &'static str,
                   label: &'static str,
                   cat: Category,
                   key: &'static str,
                   area: Area,
                   this: &Self,
                   cx: &mut Context<Self>| {
            let on = this.area == area;
            div()
                .id(id)
                .h(px(22.))
                .px(px(9.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(4.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(if on { hex::INK } else { hex::INK_DIM }))
                .when(on, |d| d.bg(c(hex::PLUGE_CONTROL)))
                .when(!on, |d| {
                    hover_fx(d, id, c(hex::PLUGE_RAISED), px(4.))
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(hex::INK)))
                })
                .tooltip(tip(label, Some(cmd_key(key))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.area = area;
                    cx.notify();
                }))
                .child(div().w(px(6.)).h(px(6.)).rounded(px(2.)).bg(cat.color()))
                .child(label)
        };
        div()
            .ml(px(10.))
            .flex()
            .flex_none()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(6.))
            .border_1()
            .border_color(c(hex::LINE))
            .bg(c(hex::PLUGE_SUB))
            .child(tab(
                "area-edit",
                "Edit",
                Category::Source,
                "1",
                Area::Edit,
                self,
                cx,
            ))
            .child(tab(
                "area-export",
                "Export",
                Category::Output,
                "4",
                Area::Export,
                self,
                cx,
            ))
    }

    pub(crate) fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let logo = div()
            .id("logo")
            .mr(px(2.))
            .cursor_pointer()
            .tooltip(tip("About videode", None))
            .on_click(|_, w, cx| w.dispatch_action(Box::new(About), cx))
            .child(brand::mark(20., 1.));

        let p = self.project();
        let crumbs = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .ml(px(14.))
            .flex_shrink_0()
            .whitespace_nowrap()
            .text_color(c(hex::INK_DIM))
            .child(SharedString::from(p.name.clone()))
            .when(self.doc.is_dirty(), |d| {
                d.child(
                    div()
                        .id("dirty")
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(c(hex::INK_DIM))
                        .tooltip(tip("Unsaved changes", Some(cmd_key("S")))),
                )
            })
            .child(icon("chevron-right", px(12.), c(hex::INK_FAINT)))
            .child(
                div()
                    .text_color(c(hex::INK))
                    .font_weight(FontWeight::MEDIUM)
                    .child(SharedString::from(format!("{} Sequence", p.sequence.name))),
            )
            .map(|d| {
                let tail = match (
                    self.view,
                    self.peek,
                    self.selection.and_then(|id| p.clip(id)),
                ) {
                    (View::Graph, ..) => Some(("Graph".to_string(), None)),
                    (View::Timeline, Some(_), Some(clip)) => {
                        Some((p.clip_name(clip), Some("nodes")))
                    }
                    _ => None,
                };
                match tail {
                    Some((name, suffix)) => d
                        .child(icon("chevron-right", px(12.), c(hex::INK_FAINT)))
                        .child(
                            div()
                                .text_color(c(hex::INK))
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                        )
                        .when_some(suffix, |d, s| {
                            d.child(div().text_color(c(hex::INK_FAINT)).child(s))
                        }),
                    None => d,
                }
            });

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
            .cursor_pointer()
            .map(|d| hover_fx(d, "command-palette", c(hex::PLUGE_RAISED), px(5.)))
            .hover(|s| s.text_color(c(hex::INK)))
            .tooltip(tip("Search", Some(cmd_key("K"))))
            .on_click(|_, w, cx| w.dispatch_action(Box::new(CommandPalette), cx))
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

        let undo = self.doc.undo_label().map(|l| format!("Undo {l}"));
        let redo = self.doc.redo_label().map(|l| format!("Redo {l}"));

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
            .child(
                div()
                    .flex()
                    .child(self.menu("File", file_menu, cx))
                    .child(self.menu("Edit", edit_menu, cx))
                    .child(self.menu("Clip", clip_menu, cx))
                    .child(self.menu("View", view_menu, cx))
                    .child(self.menu("Playback", playback_menu, cx))
                    .child(self.menu("Help", help_menu, cx)),
            )
            .child(self.area_tabs(cx))
            .child(crumbs)
            .child(
                div()
                    .ml_auto()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(command)
                    .child(
                        icon_button("undo", "undo", false, undo.is_none())
                            .tooltip(tip(
                                undo.unwrap_or("Nothing to undo".into()),
                                Some(cmd_key("Z")),
                            ))
                            .on_click(|_, w, cx| w.dispatch_action(Box::new(Undo), cx)),
                    )
                    .child(
                        icon_button("redo", "redo", false, redo.is_none())
                            .tooltip(tip(
                                redo.unwrap_or("Nothing to redo".into()),
                                Some(cmd_key("⇧ Z")),
                            ))
                            .on_click(|_, w, cx| w.dispatch_action(Box::new(Redo), cx)),
                    ),
            )
    }

    /* ---------- bin ---------- */

    pub(crate) fn add_media_at_playhead(&mut self, media: MediaId, cx: &mut Context<Self>) {
        let at = self.playhead;
        if let Some(id) = self.apply("add clip", cx, |p| p.add_media_clip(media, None, at)) {
            self.select(Some(id));
            let p = self.project();
            if let Some(clip) = p.clip(id) {
                let track = p
                    .track(clip.track)
                    .map(|t| t.label.clone())
                    .unwrap_or_default();
                let msg = format!(
                    "Added {} to {track} at {}",
                    p.clip_name(clip),
                    self.tc(clip.start)
                );
                self.say(msg);
            }
        }
    }

    fn add_node_to_selection(&mut self, kind: NodeKind, cx: &mut Context<Self>) {
        let Some(clip) = self.selection else {
            self.say("Select a clip first");
            cx.notify();
            return;
        };
        if self
            .apply("add node", cx, |p| p.add_node(clip, kind))
            .is_some()
        {
            self.say(format!("Added {}", kind.name()));
        }
    }

    pub(crate) fn render_bin(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
                .cursor_pointer()
                .text_color(c(if selected { hex::INK } else { hex::INK_DIM }))
                .when(selected, |d| d.bg(c(hex::PLUGE_CONTROL)))
                .map(|d| hover_fx(d, id, c(hex::PLUGE_CONTROL), px(5.)))
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
            .child(
                icon_button("import", "import", false, false)
                    .tooltip(tip("Import media", Some(cmd_key("I"))))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(ImportMedia), cx)),
            );

        let (body, footer): (AnyElement, String) = match self.bin_tab {
            BinTab::Media => (self.render_media_list(cx), self.media_footer()),
            BinTab::Nodes => (self.render_node_library(cx), self.nodes_footer()),
        };

        div()
            .w(px(BIN_W))
            .flex_none()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_BLACK))
            .border_r_1()
            .border_color(c(hex::LINE))
            .child(header)
            .child(
                div()
                    .id("bin-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(
                div()
                    .h(px(26.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .border_t_1()
                    .border_color(c(hex::LINE))
                    .text_color(c(hex::INK_FAINT))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(footer),
            )
    }

    fn media_footer(&self) -> String {
        match self.project().media.len() {
            1 => "1 item · double-click to add at the playhead".into(),
            n => format!("{n} items"),
        }
    }

    fn nodes_footer(&self) -> String {
        match self.selection.and_then(|id| self.project().clip(id)) {
            Some(clip) => format!("Adds to {}", self.project().clip_name(clip)),
            None => "Select a clip to add nodes".into(),
        }
    }

    fn render_media_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.project();
        if p.media.is_empty() {
            return div()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(40.))
                .text_color(c(hex::INK_DIM))
                .child(icon("film", px(24.), c(hex::INK_FAINT)))
                .child(
                    div()
                        .text_color(c(hex::INK))
                        .font_weight(FontWeight::MEDIUM)
                        .child("No media yet"),
                )
                .child(div().text_center().child("Drop files here"))
                .child(
                    button("bin-import", Some("import"), "Import media", false)
                        .on_click(|_, w, cx| w.dispatch_action(Box::new(ImportMedia), cx)),
                )
                .child(
                    div()
                        .id("bin-demo")
                        .cursor_pointer()
                        .text_color(c(hex::INK_FAINT))
                        .hover(|s| s.text_color(c(hex::INK)))
                        .on_click(|_, w, cx| w.dispatch_action(Box::new(OpenDemo), cx))
                        .child("or open the demo project"),
                )
                .into_any_element();
        }
        let in_use = |id: MediaId| {
            p.sequence
                .clips
                .iter()
                .filter(|c| c.media == Some(id))
                .count()
        };
        let fps = p.sequence.fps;
        div()
            .flex()
            .flex_col()
            .p(px(6.))
            .gap(px(1.))
            .children(p.media.iter().map(|m| {
                let id = m.id;
                let kind_icon = match m.kind {
                    MediaKind::Audio => "volume-2",
                    MediaKind::Video | MediaKind::Image => "film",
                };
                let length = m
                    .duration
                    .map(|d| time::timecode_short(d, fps))
                    .unwrap_or("still".into());
                let uses = in_use(id);
                let thumb = self.thumbnail(m, cx);
                let shown_path = self.media_path(m);
                div()
                    .id(SharedString::from(format!("media-{}", id.0)))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .map(|d| hover_fx(d, format!("media-{}", id.0), c(hex::PLUGE_RAISED), px(5.)))
                    .tooltip(tip(
                        if m.offline {
                            format!("{} · file not found", shown_path.display())
                        } else {
                            format!(
                                "{} · double-click to add at the playhead",
                                shown_path.display()
                            )
                        },
                        None,
                    ))
                    .on_click(cx.listener(move |this, e: &gpui_kit::ClickEvent, _, cx| {
                        if e.click_count() >= 2 {
                            this.add_media_at_playhead(id, cx);
                        }
                    }))
                    .child(
                        div()
                            .w(px(64.))
                            .h(px(36.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(c(if m.offline { hex::WARN } else { hex::LINE }))
                            .when(m.offline, |d| d.border_dashed())
                            .bg(c(hex::PLUGE_SUB))
                            .map(|d| match thumb {
                                Some(image) => d.child(img(ImageSource::Render(image)).size_full()),
                                None => d.child(icon(kind_icon, px(14.), c(hex::INK_FAINT))),
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(m.offline, |d| d.text_color(c(hex::WARN)))
                                    .child(m.file_name()),
                            )
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(c(hex::INK_FAINT))
                                    .child(if m.offline {
                                        "offline".to_string()
                                    } else {
                                        format!("{} · {}", m.codec, m.format)
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_end()
                            .font_family(theme::MONO)
                            .text_color(c(hex::INK_DIM))
                            .child(length)
                            .child(div().text_color(c(hex::INK_FAINT)).child(if uses == 0 {
                                String::new()
                            } else {
                                format!("×{uses}")
                            })),
                    )
            }))
            .into_any_element()
    }

    fn render_node_library(&self, cx: &mut Context<Self>) -> AnyElement {
        let target = self
            .selection
            .and_then(|id| self.project().clip(id))
            .map(|c| c.kind);
        let mut list = div().flex().flex_col().px(px(8.)).pb(px(8.));
        let mut last: Option<Category> = None;
        for kind in NodeKind::ALL.into_iter().filter(|k| !k.is_generator()) {
            let cat = kind.category();
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
            let unusable = target.is_some_and(|k| !kind.applies_to(k));
            list = list.child(
                div()
                    .id(SharedString::from(format!("node-{}", kind.ident())))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(5.))
                    .when(unusable, |d| d.opacity(0.4))
                    .when(!unusable, |d| {
                        hover_fx(
                            d.cursor_pointer(),
                            format!("node-{}", kind.ident()),
                            c(hex::PLUGE_RAISED),
                            px(5.),
                        )
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.add_node_to_selection(kind, cx)),
                    )
                    .child(swatch(cat))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).child(kind.name()))
                            .child(
                                div()
                                    .text_color(c(hex::INK_FAINT))
                                    .child(kind.description()),
                            ),
                    ),
            );
        }
        list.into_any_element()
    }

    /* ---------- viewer ---------- */

    /// The topmost enabled video clip under the playhead.
    /// The topmost enabled media clip on a video track at the playhead.
    pub(crate) fn picture_clip(&self) -> Option<&Clip> {
        let p = self.project();
        p.sequence
            .tracks
            .iter()
            .filter(|t| t.enabled && t.kind == videode_core::StreamKind::Video)
            .filter_map(|t| p.clip_at(t.id, self.playhead))
            .find(|c| c.media.is_some())
    }

    /// The picture, then text clips above it, laid out on a screen of
    /// `(w, h)` pixels showing the sequence frame scaled to fit.
    fn screen_layers(&self, w: f32, h: f32, generating: bool) -> Vec<AnyElement> {
        let p = self.project();
        let seq = &p.sequence;
        let k = w / seq.width.max(1) as f32;
        let mut layers: Vec<AnyElement> = Vec::new();
        let picture = self.picture_clip();

        if let (Some(clip), Some(frame)) = (picture, &self.frame) {
            let size = frame.image.size(0);
            let (pw, ph) = (
                (i32::from(size.width) as f32).max(1.),
                (i32::from(size.height) as f32).max(1.),
            );
            // Contain the unturned picture in the frame, then apply the
            // clip's look.
            let (bw, bh) = (frame.base.0.max(1) as f32, frame.base.1.max(1) as f32);
            let fit = (w / bw).min(h / bh);
            let l = look_at(clip, self.playhead);
            let (cx, cy) = (w / 2. + l.x as f32 * k, h / 2. + l.y as f32 * k);
            if frame.fx.rotation != 0. {
                // Crop and rotation are baked in; the image is the turned
                // picture's bounding box at the same pixel scale.
                let s = fit * l.scale as f32;
                let (iw, ih) = (pw * s, ph * s);
                layers.push(
                    img(ImageSource::Render(frame.image.clone()))
                        .absolute()
                        .left(px(cx - iw / 2.))
                        .top(px(cy - ih / 2.))
                        .w(px(iw))
                        .h(px(ih))
                        .opacity(l.opacity as f32)
                        .into_any_element(),
                );
            }
            let (iw, ih) = (pw * fit * l.scale as f32, ph * fit * l.scale as f32);
            let [cl, cr, ct, cb] = l.crop.map(|v| v as f32);
            let (left, top) = (cx - iw / 2., cy - ih / 2.);
            if frame.fx.rotation == 0. {
                layers.push(
                    div()
                        .absolute()
                        .left(px(left + iw * cl))
                        .top(px(top + ih * ct))
                        .w(px(iw * (1. - cl - cr)))
                        .h(px(ih * (1. - ct - cb)))
                        .overflow_hidden()
                        .opacity(l.opacity as f32)
                        .child(
                            img(ImageSource::Render(frame.image.clone()))
                                .absolute()
                                .left(px(-iw * cl))
                                .top(px(-ih * ct))
                                .w(px(iw))
                                .h(px(ih)),
                        )
                        .into_any_element(),
                );
            }
        } else if let Some(clip) = picture {
            let offline = clip
                .media
                .and_then(|m| p.media(m))
                .is_some_and(|m| m.offline);
            let note = if offline && generating {
                Some(("Making the demo footage…", hex::INK_DIM))
            } else if offline {
                Some(("Media offline", hex::WARN))
            } else {
                None
            };
            if let Some((text, color)) = note {
                layers.push(centered(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(theme::TITLE)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c(color))
                                .child(p.clip_name(clip)),
                        )
                        .child(div().text_color(c(hex::INK_FAINT)).child(text)),
                ));
            }
        }

        // Text clips, bottom track first so higher tracks draw over.
        let mut texts = 0;
        for t in seq
            .tracks
            .iter()
            .rev()
            .filter(|t| t.enabled && t.kind == videode_core::StreamKind::Video)
        {
            let Some(clip) = p.clip_at(t.id, self.playhead) else {
                continue;
            };
            let Some(text) = clip.first(NodeKind::Text) else {
                continue;
            };
            texts += 1;
            let l = look_at(clip, self.playhead);
            let size = (text.number("size") * l.scale) as f32 * k;
            let weight = text.text("weight").parse::<f32>().unwrap_or(500.);
            layers.push(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(l.opacity as f32)
                    .child(
                        div()
                            .relative()
                            .left(px(l.x as f32 * k))
                            .top(px(l.y as f32 * k))
                            .text_size(px(size.max(1.)))
                            .line_height(gpui_kit::relative(1.1))
                            .font_weight(FontWeight(weight))
                            .text_color(gpui_kit::white())
                            .whitespace_nowrap()
                            .child(text.text("text").to_string()),
                    )
                    .into_any_element(),
            );
        }
        if picture.is_none() && texts == 0 {
            layers.push(centered(
                div()
                    .text_color(c(hex::INK_FAINT))
                    .child("No picture at playhead"),
            ));
        }
        layers
    }

    pub(crate) fn render_viewer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = self.project();
        let seq = &p.sequence;
        let b = self.viewer_bounds.get();
        let generating = cx.global::<MediaEngine>().generating;
        let layers = self.screen_layers(
            f32::from(b.size.width),
            f32::from(b.size.height),
            generating,
        );
        let bounds = self.viewer_bounds.clone();

        let screen = div()
            .relative()
            .h_full()
            .max_w_full()
            .aspect_ratio(seq.width as f32 / seq.height.max(1) as f32)
            .overflow_hidden()
            .bg(gpui_kit::black())
            .border_1()
            .border_color(c(hex::LINE))
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(layers);

        let playing = self.is_playing();
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
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .font_family(theme::MONO)
                    .text_size(theme::TITLE)
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.tc(self.playhead))
                    .when_some(
                        self.playback.as_ref().filter(|pb| pb.speed != 1.),
                        |d, pb| {
                            d.child(
                                div()
                                    .text_size(theme::TEXT)
                                    .text_color(c(hex::INK_DIM))
                                    .child(format!("{}×", pb.speed)),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .child(
                        icon_button("to-start", "skip-back", false, false)
                            .tooltip(tip("Go to start", Some("Home".into())))
                            .on_click(
                                cx.listener(|this, _, w, cx| this.go_start(&GoToStart, w, cx)),
                            ),
                    )
                    .child(
                        icon_button("step-back", "step-back", false, false)
                            .tooltip(tip("Previous frame", Some("←".into())))
                            .on_click(
                                cx.listener(|this, _, w, cx| this.step_back(&StepBack, w, cx)),
                            ),
                    )
                    .child(
                        icon_button("play", if playing { "pause" } else { "play" }, true, false)
                            .bg(c(hex::PLUGE_CONTROL))
                            .tooltip(tip(
                                if playing { "Pause" } else { "Play" },
                                Some("Space".into()),
                            ))
                            .on_click(
                                cx.listener(|this, _, w, cx| this.toggle_play(&TogglePlay, w, cx)),
                            ),
                    )
                    .child(
                        icon_button("step-fwd", "step-forward", false, false)
                            .tooltip(tip("Next frame", Some("→".into())))
                            .on_click(
                                cx.listener(|this, _, w, cx| {
                                    this.step_forward(&StepForward, w, cx)
                                }),
                            ),
                    )
                    .child(
                        icon_button("to-end", "skip-forward", false, false)
                            .tooltip(tip("Go to end", Some("End".into())))
                            .on_click(cx.listener(|this, _, w, cx| this.go_end(&GoToEnd, w, cx))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .font_family(theme::MONO)
                    .text_color(c(hex::INK_DIM))
                    .child(self.tc(p.end())),
            );

        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(c(hex::PLUGE_SUB))
            .child(
                div()
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .gap(px(8.))
                    .text_color(c(hex::INK_DIM))
                    .child(SharedString::from(format!("{} Sequence", seq.name)))
                    .child(div().flex_1())
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .child(format!("{}×{} · {}", seq.width, seq.height, seq.fps)),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(20.))
                    .pt(px(4.))
                    .pb(px(8.))
                    .child(screen)
                    .child(meters(self.meters)),
            )
            .child(transport)
    }

    /* ---------- status bar ---------- */

    pub(crate) fn render_status_bar(&self) -> impl IntoElement + use<> {
        let item = |icon_name: &'static str, label: String| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .child(icon(icon_name, px(14.), c(hex::INK_DIM)))
                .child(label)
        };
        let p = self.project();
        let seq = &p.sequence;
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
                div().flex_1().min_w_0().overflow_hidden().child(
                    div()
                        .relative()
                        .w_full()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .when(self.status.error, |d| d.text_color(c(hex::ERROR)))
                        .child(self.status.text.clone())
                        .with_animation(
                            key("status", self.status.seq),
                            anim(motion::STATUS),
                            |d, t| d.opacity(t).top(px(lerp(6., 0., t))),
                        ),
                ),
            )
            .child(item(
                "magnet",
                if self.snap {
                    "Snap · 1 s grid".into()
                } else {
                    "Snap off".into()
                },
            ))
            .child(item("workflow", format!("{} nodes", p.node_count())))
            .child(
                div()
                    .flex_none()
                    .font_family(theme::MONO)
                    .child(format!("{}×{} · {}", seq.width, seq.height, seq.fps)),
            )
    }
}

fn centered(content: impl IntoElement) -> AnyElement {
    div()
        .absolute()
        .left_0()
        .top_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(content)
        .into_any_element()
}

/// Peak meters, left and right, on a −60 to 0 dB scale: green, then yellow
/// above −12 dB, red above −3 dB.
fn meters(levels: [f32; 2]) -> impl IntoElement {
    let pos = |db: f32| ((db + 60.) / 60.).clamp(0., 1.);
    let bar = |level: f32| {
        let db = if level > 0. {
            20. * level.log10()
        } else {
            -96.
        };
        let top = pos(db);
        let band = |from: f32, to: f32, color: u32| {
            let (lo, hi) = (pos(from), pos(to).min(top));
            (hi > lo).then(|| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(gpui_kit::relative(lo))
                    .h(gpui_kit::relative(hi - lo))
                    .bg(c(color))
            })
        };
        div()
            .relative()
            .w(px(4.))
            .h_full()
            .rounded(px(1.))
            .overflow_hidden()
            .bg(c(hex::PLUGE_RAISED))
            .children(band(-60., -12., 0x45c45a))
            .children(band(-12., -3., 0xc9c43c))
            .children(band(-3., 0., 0xe05a4f))
    };
    div()
        .absolute()
        .right(px(6.))
        .top(px(4.))
        .bottom(px(8.))
        .flex()
        .gap(px(2.))
        .child(bar(levels[0]))
        .child(bar(levels[1]))
}
