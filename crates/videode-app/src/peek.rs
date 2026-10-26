//! Peek (Tab): the selected clip's node chain, unfolded in place below the
//! clip. Click a node to inspect it, drag to reorder, + to add, Del to remove.

use std::time::Duration;

use gpui_kit::component::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, BoxShadow, Context, CursorStyle, FontWeight,
    MouseButton, MouseDownEvent, PathBuilder, SharedString, Window, canvas, div, hsla, point, px,
};
use videode_core::{Clip, ClipId, Node, NodeId};

use crate::actions::ToggleGraph;
use crate::inspector::add_node_items;
use crate::motion::{ease_out, key, lerp};
use crate::theme::{self, CategoryExt, c, hex};
use crate::timeline;
use crate::ui::{ghost_button, icon_button, kbd, tip};
use crate::workspace::{PointerDrag, Workspace};

pub(crate) const CARD_W: f32 = 136.;
pub(crate) const CARD_H: f32 = 58.;
const GAP: f32 = 40.;
const PAD: f32 = 16.;
const ADD: f32 = 30.;
const PIN_W: f32 = 116.;
const HEADER_H: f32 = 38.;
const BODY_H: f32 = CARD_H + 28.;
const FOOTER_H: f32 = 32.;
const PANEL_H: f32 = HEADER_H + BODY_H + FOOTER_H;
const UNFOLD: Duration = Duration::from_millis(300);
const STAGGER_MS: f32 = 35.;

fn card_x(i: usize) -> f32 {
    PAD + i as f32 * (CARD_W + GAP)
}

fn panel_width(nodes: usize) -> f32 {
    card_x(nodes) + ADD + GAP + PIN_W + PAD
}

/// Where a card dragged by `dx` from slot `from` would land. Slot 0 is the
/// generator's, so processing nodes land at 1 or later.
fn drop_index(from: usize, dx: f32, count: usize) -> usize {
    let centre = card_x(from) + CARD_W / 2. + dx;
    let before = (0..count)
        .filter(|&i| i != from)
        .filter(|&i| {
            // A card dragged exactly onto a neighbour's centre passes it.
            let other = card_x(i) + CARD_W / 2.;
            if dx < 0. {
                other < centre
            } else {
                other <= centre
            }
        })
        .count();
    before.clamp(1, count.saturating_sub(1).max(1))
}

/// One node card, shared by peek and the graph. `z` scales it.
pub(crate) fn node_card(node: &Node, summary: String, focused: bool, z: f32) -> gpui_kit::Div {
    card(
        node.kind.name().into(),
        summary,
        node.kind.category().color(),
        focused,
        node.bypass,
        z,
    )
}

/// A card with a category cap, a title and a one-line mono summary. Bypassed
/// cards are dashed, dimmed and struck through.
pub(crate) fn card(
    title: SharedString,
    summary: String,
    cap: gpui_kit::Hsla,
    focused: bool,
    bypass: bool,
    z: f32,
) -> gpui_kit::Div {
    div()
        .w(px(CARD_W * z))
        .h(px(CARD_H * z))
        .flex()
        .flex_col()
        .rounded(px(6. * z))
        .border_1()
        .border_color(c(if focused { hex::BONE } else { hex::LINE_STRONG }))
        .when(bypass, |d| d.border_dashed().opacity(0.55))
        .bg(c(hex::PLUGE_BLACK))
        .overflow_hidden()
        .child(div().h(px(4. * z)).flex_none().bg(cap))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .justify_center()
                .px(px(9. * z))
                .child(
                    div()
                        .text_size(px(12. * z))
                        .font_weight(FontWeight::SEMIBOLD)
                        .when(bypass, |d| d.line_through())
                        .child(title),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(11. * z))
                        .text_color(c(hex::INK_FAINT))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(if summary.is_empty() {
                            " ".to_string()
                        } else {
                            summary
                        }),
                ),
        )
}

pub(crate) fn port(x: f32, y: f32, d: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(px(x - d / 2.))
        .top(px(y - d / 2.))
        .w(px(d))
        .h(px(d))
        .rounded_full()
        .border(px((d / 6.).max(1.)))
        .border_color(c(hex::INK_DIM))
        .bg(c(hex::PLUGE_RAISED))
}

impl Workspace {
    pub(crate) fn node_summary(&self, clip: &Clip, node: &Node) -> String {
        if node.kind == videode_core::NodeKind::Source {
            clip.media
                .and_then(|m| self.project().media(m))
                .map(|m| m.file_name())
                .unwrap_or_default()
        } else {
            node.summary()
        }
    }

    pub(crate) fn open_peek(&mut self) {
        let seq = self.next_seq();
        self.peek = Some(seq);
    }

    pub(crate) fn node_down(
        &mut self,
        clip: ClipId,
        node: NodeId,
        generator: bool,
        e: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus_node = Some(node);
        if !generator {
            self.pdrag = Some(PointerDrag::Node {
                clip,
                node,
                x0: f32::from(e.position.x),
                dx: 0.,
                moved: false,
            });
        }
        cx.notify();
    }

    pub(crate) fn drop_node(
        &mut self,
        clip: ClipId,
        node: NodeId,
        dx: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(c) = self.project().clip(clip) else {
            return;
        };
        let Some(from) = c.node_index(node) else {
            return;
        };
        let to = drop_index(from, dx, c.nodes.len());
        if to != from {
            let name = c.nodes[from].kind.name();
            if self
                .apply("reorder node", cx, |p| p.move_node(clip, node, to))
                .is_some()
            {
                self.say(format!("Moved {name}"));
            }
        }
        cx.notify();
    }

    pub(crate) fn render_peek(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let seq = self.peek?;
        let clip = self.selection.and_then(|id| self.project().clip(id))?;
        let track = self.project().track(clip.track)?;

        // Anchor to the clip on screen: below it when there's room, else above.
        let lanes = self.tl.bounds.get();
        let viewport = window.viewport_size();
        let (win_w, win_h) = (f32::from(viewport.width), f32::from(viewport.height));
        let track_top: f32 = self
            .project()
            .sequence
            .tracks
            .iter()
            .take_while(|t| t.id != clip.track)
            .map(timeline::track_height)
            .sum();
        let clip_top = f32::from(lanes.origin.y) + track_top;
        let clip_bottom = clip_top + timeline::track_height(track);
        let lanes_left = f32::from(lanes.origin.x);
        let lanes_right = lanes_left + f32::from(lanes.size.width);
        let x0 = (lanes_left + self.frame_x(clip.start)).max(lanes_left);
        let x1 = (lanes_left + self.frame_x(clip.end())).min(lanes_right);
        let w = panel_width(clip.nodes.len()).min(win_w - 24.);
        let left = ((x0 + x1) / 2. - w / 2.).clamp(12., (win_w - w - 12.).max(12.));
        let floor = win_h - 26. - 8.;
        let mut top = clip_bottom + 8.;
        if top + PANEL_H > floor {
            top = clip_top - PANEL_H - 8.;
            let ceiling = f32::from(lanes.origin.y) - 60.;
            if top < ceiling {
                top = (floor - PANEL_H).max(ceiling);
            }
        }

        let dragging = match self.pdrag {
            Some(PointerDrag::Node {
                node,
                dx,
                moved: true,
                ..
            }) => Some((node, dx)),
            _ => None,
        };
        let n = clip.nodes.len();
        let mid = 14. + CARD_H / 2.;

        // Wires run card to card, then past the + to the track pin.
        let wires = {
            let bypassed: Vec<bool> = clip.nodes.iter().map(|n| n.bypass).collect();
            canvas(
                |_, _, _| (),
                move |b, _, window, _| {
                    let at = |x: f32, y: f32| point(b.origin.x + px(x), b.origin.y + px(y));
                    let mut seg = |x1: f32, x2: f32, dashed: bool| {
                        let mut path = PathBuilder::stroke(px(1.5));
                        if dashed {
                            path = path.dash_array(&[px(4.), px(3.)]);
                        }
                        path.move_to(at(x1, mid));
                        path.line_to(at(x2, mid));
                        if let Ok(p) = path.build() {
                            window.paint_path(p, c(hex::INK_FAINT));
                        }
                    };
                    for i in 0..n.saturating_sub(1) {
                        seg(card_x(i) + CARD_W, card_x(i + 1), bypassed[i + 1]);
                    }
                    seg(card_x(n - 1) + CARD_W, card_x(n) + ADD + GAP, false);
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };

        let total_ms = UNFOLD.as_millis() as f32 + STAGGER_MS * (n + 2) as f32;
        let stagger = move |i: usize| {
            let delay = 60. + i as f32 * STAGGER_MS;
            Animation::new(Duration::from_millis(total_ms as u64))
                .with_easing(move |t| ease_out(((t * total_ms - delay) / 240.).clamp(0., 1.)))
        };

        let mut body: Vec<AnyElement> = vec![wires.into_any_element()];
        let ws = cx.entity().downgrade();
        let drop_at = dragging
            .and_then(|(node, dx)| clip.node_index(node).map(|from| drop_index(from, dx, n)));
        for (i, node) in clip.nodes.iter().enumerate() {
            let (clip_id, node_id, generator) = (clip.id, node.id, node.kind.is_generator());
            let dx = dragging.filter(|(id, _)| *id == node.id).map(|(_, dx)| dx);
            let x = card_x(i) + dx.unwrap_or(0.);
            let card = node_card(
                node,
                self.node_summary(clip, node),
                self.focus_node == Some(node.id),
                1.,
            )
            .id(SharedString::from(format!("peek-node-{}", node.id.0)))
            .absolute()
            .left(px(x))
            .top(px(14.))
            .when(!generator, |d| d.cursor(CursorStyle::OpenHand))
            .when(dx.is_some(), |d| {
                d.cursor(CursorStyle::ClosedHand).shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.5),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(20.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.node_down(clip_id, node_id, generator, e, cx)
                }),
            )
            .tooltip(tip(
                if generator {
                    "Source"
                } else {
                    "Drag to reorder"
                },
                None,
            ));
            let card = if dx.is_some() {
                card.into_any_element()
            } else {
                card.with_animation(key("peek-node", seq * 64 + i as u64), stagger(i), |d, t| {
                    d.opacity(t).mt(px(lerp(6., 0., t)))
                })
                .into_any_element()
            };
            if !generator {
                body.push(port(card_x(i), mid, 9.).into_any_element());
            }
            body.push(port(card_x(i) + CARD_W, mid, 9.).into_any_element());
            body.push(card);
        }
        if let Some(to) = drop_at {
            let x = if to == 0 {
                card_x(0)
            } else {
                card_x(to) - GAP / 2.
            };
            body.push(
                div()
                    .absolute()
                    .left(px(x - 1.))
                    .top(px(10.))
                    .w(px(2.))
                    .h(px(CARD_H + 8.))
                    .bg(c(hex::BONE))
                    .into_any_element(),
            );
        }
        let (clip_id, clip_kind) = (clip.id, clip.kind);
        body.push(
            div()
                .absolute()
                .left(px(card_x(n)))
                .top(px(mid - ADD / 2.))
                .child(
                    Button::new("peek-add")
                        .icon(IconName::Plus)
                        .outline()
                        .small()
                        .tooltip("Add node  A")
                        .dropdown_menu(move |menu, _, _| {
                            add_node_items(menu.min_w(px(200.)), clip_id, clip_kind, ws.clone())
                        }),
                )
                .into_any_element(),
        );
        body.push(
            div()
                .absolute()
                .left(px(card_x(n) + ADD + GAP))
                .top(px(mid - 15.))
                .h(px(30.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(5.))
                .rounded_full()
                .border_1()
                .border_color(c(hex::LINE_STRONG))
                .text_color(c(hex::INK_DIM))
                .child("to")
                .child(
                    div()
                        .font_family(theme::MONO)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(hex::INK))
                        .child(SharedString::from(track.label.clone())),
                )
                .child(SharedString::from(track.name.clone()))
                .into_any_element(),
        );
        body.push(port(card_x(n) + ADD + GAP, mid, 9.).into_any_element());

        let header = div()
            .h(px(HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(6.))
            .border_b_1()
            .border_color(c(hex::LINE))
            .child(crate::ui::icon("layers", px(16.), c(hex::INK_DIM)))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.project().clip_name(clip)),
            )
            .child(div().text_color(c(hex::INK_FAINT)).child(format!(
                "{n} node{} · drag to reorder",
                if n == 1 { "" } else { "s" }
            )))
            .child(div().flex_1())
            .child(
                ghost_button("peek-graph", "workflow", "Whole graph", Some("⇧ Tab"))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(ToggleGraph), cx)),
            )
            .child(
                icon_button("peek-close", "close", false, false)
                    .tooltip(tip("Close", Some("Esc".into())))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.peek = None;
                        cx.notify();
                    })),
            );
        let hint = |k: &'static str, text: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(kbd(k))
                .child(text)
        };
        let footer = div()
            .h(px(FOOTER_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(14.))
            .text_color(c(hex::INK_FAINT))
            .child(hint("click", "inspect node"))
            .child(hint("Del", "remove focused node"))
            .child(hint("⇧ B", "bypass"))
            .child(hint("Esc", "close"));

        Some(
            div()
                .id("peek")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(w))
                .h(px(PANEL_H))
                .flex()
                .flex_col()
                .overflow_hidden()
                .occlude()
                .rounded(px(10.))
                .border_1()
                .border_color(c(hex::LINE_STRONG))
                .bg(c(hex::PLUGE_RAISED))
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.6),
                    offset: point(px(0.), px(18.)),
                    blur_radius: px(48.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(header)
                .child(
                    div()
                        .id("peek-body")
                        .relative()
                        .h(px(BODY_H))
                        .flex_none()
                        .overflow_x_scroll()
                        .children(body),
                )
                .child(footer)
                .with_animation(
                    key("peek", seq),
                    Animation::new(UNFOLD).with_easing(ease_out),
                    |d, t| d.h(px(lerp(12., PANEL_H, t))).opacity(lerp(0.3, 1., t)),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_index_respects_generator_slot() {
        // Three nodes; drag the last one far left: it lands at 1, not 0.
        assert_eq!(drop_index(2, -1000., 3), 1);
        // A small wiggle leaves it in place.
        assert_eq!(drop_index(1, 10., 3), 1);
        // Past the next card's centre swaps them.
        assert_eq!(drop_index(1, CARD_W + GAP, 3), 2);
        // And the same exact distance back to the left.
        assert_eq!(drop_index(2, -(CARD_W + GAP), 3), 1);
    }
}
