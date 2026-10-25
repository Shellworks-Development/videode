//! The graph view (Shift+Tab): everything in the sequence as one node graph.
//! Drag to pan, scroll to zoom, double-click a clip to cut it in the timeline.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent, PathBuilder,
    ScrollWheelEvent, SharedString, Window, canvas, div, hsla, point, px,
};
use videode_core::graph::{GraphRef, NODE_H, NODE_W, layout};

use crate::actions::ToggleGraph;
use crate::peek::{card, node_card, port};
use crate::theme::{self, ALL_CATEGORIES, CategoryExt, c, hex};
use crate::ui::{ghost_button, icon_button, tip};
use crate::workspace::{PointerDrag, View, Workspace};

const MIN_ZOOM: f32 = 0.3;
const MAX_ZOOM: f32 = 2.;

impl Workspace {
    pub(crate) fn open_graph(&mut self) {
        self.peek = None;
        self.view = View::Graph;
        self.graph.fit_pending = true;
        self.say("Graph view · everything in the sequence is a node · Esc returns to the timeline");
    }

    pub(crate) fn close_graph(&mut self) {
        self.view = View::Timeline;
        self.say("Timeline");
    }

    fn fit_graph(&mut self) {
        let b = self.graph.bounds.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        if w <= 0. {
            return;
        }
        let l = layout(self.project());
        let (aw, ah) = (w - 48., h - 112.);
        let z = (aw / l.width).min(ah / l.height).clamp(0.45, 1.15);
        self.graph.zoom = z;
        self.graph.x = 24. + ((aw - l.width * z) / 2.).max(0.);
        self.graph.y = 56. + ((ah - l.height * z) / 2.).max(0.);
        self.graph.fit_pending = false;
    }

    fn graph_wheel(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let b = self.graph.bounds.get();
        let delta = e.delta.pixel_delta(px(20.));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if e.modifiers.shift {
            self.graph.x += if dx != 0. { dx } else { dy };
        } else {
            let (ax, ay) = (
                f32::from(e.position.x - b.origin.x),
                f32::from(e.position.y - b.origin.y),
            );
            let z0 = self.graph.zoom;
            let z1 = (z0 * (dy * 0.0025).exp()).clamp(MIN_ZOOM, MAX_ZOOM);
            self.graph.x = ax - (ax - self.graph.x) * z1 / z0;
            self.graph.y = ay - (ay - self.graph.y) * z1 / z0;
            self.graph.zoom = z1;
        }
        cx.notify();
    }

    fn graph_node_click(&mut self, r: GraphRef, cx: &mut Context<Self>) {
        match r {
            GraphRef::ClipNode { clip, node } => {
                self.select(Some(clip));
                self.focus_node = Some(node);
            }
            GraphRef::Output => {
                self.inspect_output = true;
            }
            GraphRef::Sequence => {
                self.select(None);
                self.inspect_output = false;
            }
            GraphRef::Track(t) => {
                let label = self
                    .project()
                    .track(t)
                    .map(|t| t.label.clone())
                    .unwrap_or_default();
                self.say(format!(
                    "Track {label} · mute, hide and lock it from the timeline header"
                ));
            }
            GraphRef::Composite => {
                self.say("Composite stacks the video tracks, top track over bottom")
            }
            GraphRef::Mix => self.say("Mix sums the audio tracks"),
        }
        cx.notify();
    }

    /// Back to the timeline with this clip selected, scrolled into view and peeked.
    fn graph_open_clip(&mut self, clip: videode_core::ClipId, cx: &mut Context<Self>) {
        self.view = View::Timeline;
        self.select(Some(clip));
        self.reveal_clip(clip);
        self.open_peek();
        self.say("Back in the timeline · peeking at the clip you opened");
        cx.notify();
    }

    pub(crate) fn render_graph(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if self.graph.fit_pending {
            if f32::from(self.graph.bounds.get().size.width) > 0. {
                self.fit_graph();
            } else {
                window.request_animation_frame();
            }
        }
        let l = layout(self.project());
        let (gx, gy, z) = (self.graph.x, self.graph.y, self.graph.zoom);
        let p = self.project();

        // Canvas: dot grid, then wires; it also records the view's bounds.
        let bounds_cell = self.graph.bounds.clone();
        let wires: Vec<_> = l.wires().collect();
        let backdrop = canvas(
            move |b, _, _| bounds_cell.set(b),
            move |b, _, window, _| {
                let at = |x: f32, y: f32| {
                    point(b.origin.x + px(gx + x * z), b.origin.y + px(gy + y * z))
                };
                let step = 24. * z;
                if step >= 8. {
                    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
                    let mut y = gy.rem_euclid(step);
                    while y < h {
                        let mut x = gx.rem_euclid(step);
                        while x < w {
                            window.paint_quad(gpui_kit::fill(
                                gpui_kit::Bounds::new(
                                    point(b.origin.x + px(x), b.origin.y + px(y)),
                                    gpui_kit::size(px(1.5), px(1.5)),
                                ),
                                hsla(0., 0., 1., 0.06),
                            ));
                            x += step;
                        }
                        y += step;
                    }
                }
                for ((x1, y1), (x2, y2), bypass) in &wires {
                    let dx = ((x2 - x1) / 2.).max(30.);
                    let mut path = PathBuilder::stroke(px((1.5 * z).max(1.)));
                    if *bypass {
                        path = path.dash_array(&[px(4.), px(3.)]);
                    }
                    path.move_to(at(*x1, *y1));
                    path.cubic_bezier_to(at(*x2, *y2), at(x1 + dx, *y1), at(x2 - dx, *y2));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, hsla(0., 0., 0.85, 0.45));
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let mut world: Vec<AnyElement> = Vec::new();
        let sx = |x: f32| gx + x * z;
        let sy = |y: f32| gy + y * z;
        for g in &l.groups {
            let Some(clip) = p.clip(g.clip) else { continue };
            let offline = clip
                .media
                .and_then(|m| p.media(m))
                .is_some_and(|m| m.offline);
            let selected = self.selection == Some(clip.id);
            let id = clip.id;
            world.push(
                div()
                    .id(SharedString::from(format!("group-{}", id.0)))
                    .absolute()
                    .left(px(sx(g.x)))
                    .top(px(sy(g.y)))
                    .w(px(g.w * z))
                    .h(px(g.h * z))
                    .rounded(px(8. * z))
                    .border_1()
                    .border_color(c(if selected {
                        hex::BONE
                    } else if offline {
                        hex::WARN
                    } else {
                        hex::LINE
                    }))
                    .when(offline, |d| d.border_dashed())
                    .bg(hsla(0., 0., 0.07, 0.7))
                    .px(px(8. * z))
                    .pt(px(4. * z))
                    .flex()
                    .gap(px(8. * z))
                    .text_size(px(11. * z))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, e: &gpui_kit::ClickEvent, _, cx| {
                        if e.click_count() >= 2 {
                            this.graph_open_clip(id, cx);
                        } else {
                            this.select(Some(id));
                            cx.notify();
                        }
                    }))
                    .tooltip(tip(
                        "Click to select · double-click to open in the timeline",
                        None,
                    ))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(p.clip_name(clip)),
                    )
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_color(c(hex::INK_FAINT))
                            .child(self.tc(clip.start)),
                    )
                    .into_any_element(),
            );
        }
        for (i, n) in l.graph.nodes.iter().enumerate() {
            let (x, y) = l.pos[i];
            let r = n.id;
            let focused = match r {
                GraphRef::ClipNode { node, .. } => self.focus_node == Some(node),
                GraphRef::Output => self.inspect_output,
                _ => false,
            };
            let (card, generator) = match r {
                GraphRef::ClipNode { clip, node } => {
                    let Some(c) = p.clip(clip) else { continue };
                    let Some(node) = c.node(node) else { continue };
                    (
                        node_card(node, self.node_summary(c, node), focused, z),
                        node.kind.is_generator(),
                    )
                }
                _ => (
                    card(
                        n.label.clone().into(),
                        n.summary.clone(),
                        n.category.color(),
                        focused,
                        n.bypass,
                        z,
                    ),
                    false,
                ),
            };
            if !generator {
                world.push(port(sx(x), sy(y + NODE_H / 2.), 8. * z).into_any_element());
            }
            if r != GraphRef::Output {
                world.push(port(sx(x + NODE_W), sy(y + NODE_H / 2.), 8. * z).into_any_element());
            }
            world.push(
                card.id(SharedString::from(format!("gnode-{i}")))
                    .absolute()
                    .left(px(sx(x)))
                    .top(px(sy(y)))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| this.graph_node_click(r, cx)))
                    .into_any_element(),
            );
        }

        let hud = div()
            .absolute()
            .top(px(12.))
            .left(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                ghost_button("graph-back", "chevron-left", "Timeline", Some("Esc"))
                    .border_1()
                    .border_color(c(hex::LINE_STRONG))
                    .bg(c(hex::PLUGE_BLACK))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(ToggleGraph), cx)),
            )
            .child(
                icon_button("graph-fit", "maximize", false, false)
                    .tooltip(tip("Fit the graph", None))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.fit_graph();
                        cx.notify();
                    })),
            )
            .child(div().text_color(c(hex::INK_FAINT)).child(
                "Drag to pan · scroll to zoom · double-click a clip to open it in the timeline",
            ));
        let legend = div()
            .absolute()
            .bottom(px(12.))
            .left(px(12.))
            .flex()
            .gap(px(12.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(8.))
            .border_1()
            .border_color(c(hex::LINE))
            .bg(c(hex::PLUGE_BLACK))
            .text_color(c(hex::INK_DIM))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(ALL_CATEGORIES.map(|cat| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(crate::ui::swatch(cat))
                    .child(cat.label())
            }));
        let zoom = div()
            .absolute()
            .bottom(px(14.))
            .right(px(14.))
            .font_family(theme::MONO)
            .text_color(c(hex::INK_FAINT))
            .child(format!("{:.0}%", z * 100.));

        div()
            .id("graph")
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(c(hex::PLUGE_SUB))
            .cursor(if matches!(self.pdrag, Some(PointerDrag::Pan { .. })) {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::Arrow
            })
            .on_scroll_wheel(
                cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.graph_wheel(e, cx)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.pdrag = Some(PointerDrag::Pan {
                        x0: f32::from(e.position.x),
                        y0: f32::from(e.position.y),
                        gx: this.graph.x,
                        gy: this.graph.y,
                    });
                    cx.notify();
                }),
            )
            .child(backdrop)
            .children(world)
            .child(hud)
            .child(legend)
            .child(zoom)
    }
}
