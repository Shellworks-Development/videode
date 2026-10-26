//! The inspector: the selected clip's node chain with every value editable,
//! the Output node, or the sequence. Values can be typed (click the value),
//! scrubbed (drag the label; Shift ×10, Alt ÷10; double-click resets) or
//! stepped with the arrow keys while typing.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent, SharedString,
    WeakEntity, Window, div, px,
};
use videode_core::{
    Clip, ClipId, ClipKind, Node, NodeId, NodeKind, OutputField, ParamKind, ParamSpec, Value, time,
};

use crate::actions::OpenOutput;
use crate::theme::{self, Category, CategoryExt, c, hex};
use crate::ui::{button, field_row, icon_button, pane_header, swatch, tip};
use crate::workspace::{FieldEdit, FieldTarget, PointerDrag, Workspace};

const INSPECTOR_W: f32 = 304.;
const LABEL_W: f32 = 96.;

/// Menu items adding each node that fits `kind` to `clip`.
pub(crate) fn add_node_items(
    mut menu: PopupMenu,
    clip: ClipId,
    kind: ClipKind,
    ws: WeakEntity<Workspace>,
) -> PopupMenu {
    let mut last = None;
    for k in NodeKind::ALL.into_iter().filter(|k| k.applies_to(kind)) {
        let cat = k.category();
        if last.is_some_and(|l| l != cat) {
            menu = menu.separator();
        }
        last = Some(cat);
        let ws = ws.clone();
        menu = menu.item(
            PopupMenuItem::new(k.name())
                .icon(
                    gpui_kit::component::Icon::empty()
                        .path("icons/square.svg")
                        .text_color(cat.color()),
                )
                .on_click(move |_, _, cx| {
                    ws.update(cx, |ws, cx| ws.add_node(clip, k, cx)).ok();
                }),
        );
    }
    menu
}

fn field_box() -> gpui_kit::Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(24.))
        .px(px(7.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded(px(5.))
        .border_1()
        .border_color(c(hex::LINE))
        .bg(c(hex::PLUGE_SUB))
        .whitespace_nowrap()
        .overflow_hidden()
}

impl Workspace {
    /* ---------- edits ---------- */

    pub(crate) fn add_node(&mut self, clip: ClipId, kind: NodeKind, cx: &mut Context<Self>) {
        if let Some(id) = self.apply("add node", cx, |p| p.add_node(clip, kind)) {
            self.focus_node = Some(id);
            self.say(format!("Added {}", kind.name()));
        }
    }

    pub(crate) fn remove_node(&mut self, clip: ClipId, node: NodeId, cx: &mut Context<Self>) {
        let name = self.node_name(clip, node);
        if self
            .apply("remove node", cx, |p| p.remove_node(clip, node))
            .is_some()
        {
            if self.focus_node == Some(node) {
                self.focus_node = None;
            }
            self.say(format!("Removed {name}"));
        }
    }

    pub(crate) fn toggle_bypass(&mut self, clip: ClipId, node: NodeId, cx: &mut Context<Self>) {
        let Some(bypass) = self
            .project()
            .clip(clip)
            .and_then(|c| c.node(node))
            .map(|n| !n.bypass)
        else {
            return;
        };
        let name = self.node_name(clip, node);
        let label = if bypass { "bypass node" } else { "enable node" };
        if self
            .apply(label, cx, |p| p.set_bypass(clip, node, bypass))
            .is_some()
        {
            self.say(format!(
                "{name} {}",
                if bypass { "bypassed" } else { "enabled" }
            ));
        }
    }

    fn node_name(&self, clip: ClipId, node: NodeId) -> &'static str {
        self.project()
            .clip(clip)
            .and_then(|c| c.node(node))
            .map_or("Node", |n| n.kind.name())
    }

    /// Sets one param. With `coalesce`, repeated calls with the same key
    /// (one scrub) make a single undo step.
    pub(crate) fn set_param(
        &mut self,
        clip: ClipId,
        node: NodeId,
        key: &'static str,
        value: Value,
        coalesce: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let label = self
            .project()
            .clip(clip)
            .and_then(|c| c.node(node))
            .and_then(|n| n.kind.param(key))
            .map_or_else(
                || "edit value".to_string(),
                |s| format!("edit {}", s.label.to_lowercase()),
            );
        let result = match &coalesce {
            Some(k) => self
                .doc
                .edit_coalesced(&label, k, |p| p.set_param(clip, node, key, value)),
            None => self
                .doc
                .edit(&label, |p| p.set_param(clip, node, key, value)),
        };
        if let Err(e) = result {
            self.say_error(e.to_string());
        }
        cx.notify();
    }

    fn param_spec(
        &self,
        clip: ClipId,
        node: NodeId,
        key: &str,
    ) -> Option<(&'static ParamSpec, Value)> {
        let n = self.project().clip(clip)?.node(node)?;
        let spec = n.kind.param(key)?;
        Some((
            spec,
            n.get(key).cloned().unwrap_or_else(|| spec.default_value()),
        ))
    }

    /* ---------- typing ---------- */

    pub(crate) fn begin_field(
        &mut self,
        target: FieldTarget,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_field(window, cx);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
        input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.commit_field(window, cx);
                }
            });
        self.field = Some(FieldEdit {
            target,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(crate) fn commit_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.field.take() else {
            return;
        };
        let text = f.input.read(cx).value().to_string();
        match f.target {
            FieldTarget::Param { clip, node, key } => {
                if let Some((spec, current)) = self.param_spec(clip, node, key) {
                    match spec.parse(&text) {
                        Some(v) if v != current => self.set_param(clip, node, key, v, None, cx),
                        Some(_) => {}
                        None => self.say_error(format!(
                            "“{}” isn't a valid {}",
                            text.trim(),
                            spec.label.to_lowercase()
                        )),
                    }
                }
            }
            FieldTarget::Output(field) => {
                if self.project().output.get(field) != text.trim() {
                    let label = format!("set output {}", field.label().to_lowercase());
                    self.apply(&label, cx, |p| p.set_output(field, &text));
                }
            }
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn cancel_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.field = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Arrow keys while typing a number: step the value and keep typing.
    fn step_field(&mut self, dir: f64, big: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = &self.field else {
            return;
        };
        let FieldTarget::Param { clip, node, key } = f.target else {
            return;
        };
        let Some((spec, current)) = self.param_spec(clip, node, key) else {
            return;
        };
        let typed = spec.parse(&f.input.read(cx).value()).unwrap_or(current);
        let Some(next) = spec.nudge(&typed, dir * if big { 10. } else { 1. }) else {
            return;
        };
        let text = number_text(spec, &next);
        let input = f.input.clone();
        self.set_param(
            clip,
            node,
            key,
            next,
            Some(format!("step-{}-{key}", node.0)),
            cx,
        );
        input.update(cx, |s, cx| {
            s.set_value(text, window, cx);
            s.select_all(window, cx);
        });
    }

    /* ---------- scrubbing ---------- */

    pub(crate) fn scrub_to(&mut self, x: f32, shift: bool, alt: bool, cx: &mut Context<Self>) {
        let Some(PointerDrag::Scrub {
            clip,
            node,
            key,
            x0,
            v0,
        }) = self.pdrag
        else {
            return;
        };
        let Some((spec, _)) = self.param_spec(clip, node, key) else {
            return;
        };
        let rate = if shift {
            10.
        } else if alt {
            0.1
        } else {
            1.
        };
        let steps = (((x - x0) / 2.) * rate).round() as f64;
        if let Some(v) = spec.nudge(&Value::Number(v0), steps) {
            let key_id = format!("scrub-{}-{key}-{x0}", node.0);
            self.set_param(clip, node, key, v, Some(key_id), cx);
        }
    }

    fn label_down(
        &mut self,
        clip: ClipId,
        node: NodeId,
        key: &'static str,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((spec, value)) = self.param_spec(clip, node, key) else {
            return;
        };
        self.commit_field(window, cx);
        if e.click_count >= 2 {
            self.pdrag = None;
            self.set_param(clip, node, key, spec.default_value(), None, cx);
            self.say(format!("Reset {}", spec.label.to_lowercase()));
            return;
        }
        if let Value::Number(v0) = value {
            self.pdrag = Some(PointerDrag::Scrub {
                clip,
                node,
                key,
                x0: f32::from(e.position.x),
                v0,
            });
        }
        cx.notify();
    }

    /* ---------- rendering ---------- */

    pub(crate) fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let body = if self.inspect_output {
            self.output_inspector(cx)
        } else {
            match self.selection.and_then(|id| self.project().clip(id)) {
                Some(clip) => self.clip_inspector(clip, cx),
                None => self.sequence_inspector(),
            }
        };
        div()
            .w(px(INSPECTOR_W))
            .h_full()
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
                    .id("inspector-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
    }

    fn sequence_inspector(&self) -> AnyElement {
        let p = self.project();
        let seq = &p.sequence;
        let o = &p.output;
        let clips = seq.clips.len();
        div()
            .child(title_block(
                format!("{} Sequence", seq.name),
                if clips == 0 {
                    "Empty".into()
                } else {
                    format!("{clips} clips · {} nodes", p.node_count())
                },
                None,
            ))
            .child(section_shell(
                Category::Composite,
                "Sequence",
                false,
                false,
                div(),
                vec![
                    field_row(
                        "Resolution",
                        format!("{} × {}", seq.width, seq.height),
                        true,
                    ),
                    field_row("Frame rate", seq.fps.to_string(), true),
                    field_row("Duration", self.tc(p.end()), true),
                    field_row("Color space", "Rec.709 · gamma 2.4", false),
                ]
                .into_iter()
                .map(IntoElement::into_any_element)
                .collect(),
            ))
            .child(section_shell(
                Category::Output,
                "Output",
                false,
                false,
                button("open-output", None, "Open", false)
                    .h(px(22.))
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(OpenOutput), cx)),
                vec![
                    div()
                        .text_color(c(hex::INK_DIM))
                        .child(format!("{} · {} · {}", o.container, o.codec, o.resolution))
                        .into_any_element(),
                ],
            ))
            .into_any_element()
    }

    fn clip_inspector(&self, clip: &Clip, cx: &mut Context<Self>) -> AnyElement {
        let p = self.project();
        let fps = p.sequence.fps;
        let track = p
            .track(clip.track)
            .map(|t| format!("{} {}", t.label, t.name))
            .unwrap_or_default();
        let strip = div()
            .mt(px(8.))
            .h(px(4.))
            .flex()
            .gap(px(1.))
            .children(clip.nodes.iter().map(|n| {
                div()
                    .flex_1()
                    .h_full()
                    .bg(n.kind.category().color())
                    .when(n.bypass, |d| d.opacity(0.25))
            }));
        let mut root = div().child(title_block(
            p.clip_name(clip),
            format!(
                "{track} · {}–{} · {} nodes",
                time::timecode_short(clip.start, fps),
                time::timecode_short(clip.end(), fps),
                clip.nodes.len()
            ),
            Some(strip.into_any_element()),
        ));
        for node in &clip.nodes {
            root = root.child(self.node_section(clip, node, cx));
        }
        let ws = cx.entity().downgrade();
        let (id, kind) = (clip.id, clip.kind);
        root.child(
            div().p(px(12.)).child(
                Button::new("add-node")
                    .label("Add node")
                    .icon(gpui_kit::component::IconName::Plus)
                    .outline()
                    .small()
                    .w_full()
                    .dropdown_menu(move |menu, _, _| {
                        add_node_items(menu.min_w(px(200.)), id, kind, ws.clone())
                    }),
            ),
        )
        .into_any_element()
    }

    fn node_section(&self, clip: &Clip, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        let (clip_id, node_id) = (clip.id, node.id);
        let focused = self.focus_node == Some(node.id);
        let fixed = node.kind.is_generator();
        let actions = div().flex().when(!fixed, |d| {
            d.child(
                icon_button(
                    SharedString::from(format!("bypass-{}", node_id.0)),
                    "power",
                    node.bypass,
                    false,
                )
                .tooltip(tip(
                    if node.bypass {
                        "Enable node"
                    } else {
                        "Bypass node"
                    },
                    Some("⇧ B".into()),
                ))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.toggle_bypass(clip_id, node_id, cx)),
                ),
            )
            .child(
                icon_button(
                    SharedString::from(format!("remove-{}", node_id.0)),
                    "trash",
                    false,
                    false,
                )
                .tooltip(tip("Remove node", Some("Del".into())))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.remove_node(clip_id, node_id, cx)),
                ),
            )
        });

        let mut rows: Vec<AnyElement> = Vec::new();
        if node.kind == NodeKind::Source {
            let p = self.project();
            let fps = p.sequence.fps;
            if let Some(m) = clip.media.and_then(|m| p.media(m)) {
                rows.push(field_row("File", m.file_name(), true).into_any_element());
                rows.push(
                    field_row("Format", format!("{} · {}", m.codec, m.format), true)
                        .into_any_element(),
                );
                rows.push(
                    field_row("In", time::timecode(clip.source_in, fps), true).into_any_element(),
                );
                rows.push(
                    field_row(
                        "Out",
                        time::timecode(clip.source_in + clip.duration, fps),
                        true,
                    )
                    .into_any_element(),
                );
                if m.offline {
                    let media_id = m.id;
                    rows.push(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(div().text_color(c(hex::WARN)).child("File not found"))
                            .child(button("relink", Some("link"), "Relink…", false).on_click(
                                cx.listener(move |this, _, _, cx| this.relink_media(media_id, cx)),
                            ))
                            .into_any_element(),
                    );
                }
            }
        }
        for spec in node.kind.params() {
            rows.push(self.param_row(clip_id, node, spec, cx));
        }

        let header_name = div()
            .id(SharedString::from(format!("node-name-{}", node_id.0)))
            .cursor_pointer()
            .font_weight(FontWeight::SEMIBOLD)
            .when(node.bypass, |d| d.line_through())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.focus_node = Some(node_id);
                cx.notify();
            }))
            .child(node.kind.name());
        section_shell(
            node.kind.category(),
            "",
            node.bypass,
            focused,
            actions,
            rows,
        )
        .child_header_name(header_name)
    }

    fn param_row(
        &self,
        clip: ClipId,
        node: &Node,
        spec: &'static ParamSpec,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let node_id = node.id;
        let key = spec.key;
        let value = node
            .get(key)
            .cloned()
            .unwrap_or_else(|| spec.default_value());
        let changed = !spec.is_default(&value);
        let target = FieldTarget::Param {
            clip,
            node: node_id,
            key,
        };
        let is_number = matches!(spec.kind, ParamKind::Number { .. });

        let label = div()
            .id(SharedString::from(format!("lbl-{}-{key}", node_id.0)))
            .w(px(LABEL_W))
            .flex_none()
            .text_color(c(hex::INK_DIM))
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
            .when(is_number, |d| {
                d.cursor(CursorStyle::ResizeLeftRight)
                    .hover(|s| s.text_color(c(hex::INK)))
                    .tooltip(tip("Drag to scrub · double-click to reset", None))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            this.label_down(clip, node_id, key, e, window, cx)
                        }),
                    )
            })
            .child(spec.label);

        let field: AnyElement = match spec.kind {
            ParamKind::Choice { options, .. } => {
                let current = value.as_text().unwrap_or_default().to_string();
                let ws = cx.entity().downgrade();
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        choice_button(
                            SharedString::from(format!("choice-{}-{key}", node_id.0)),
                            current.clone(),
                            changed,
                            cx,
                        )
                        .dropdown_menu(move |mut menu, _, _| {
                            for &o in options {
                                let ws = ws.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(o).checked(o == current).on_click(
                                        move |_, _, cx| {
                                            ws.update(cx, |ws, cx| {
                                                ws.set_param(
                                                    clip,
                                                    node_id,
                                                    key,
                                                    Value::Text(o.into()),
                                                    None,
                                                    cx,
                                                )
                                            })
                                            .ok();
                                        },
                                    ),
                                );
                            }
                            menu
                        }),
                    )
                    .into_any_element()
            }
            _ => self.text_field(
                target,
                FieldLook {
                    shown: spec_display(spec, &value),
                    edit: number_text(spec, &value),
                    unit: spec.unit,
                    changed,
                    number: is_number,
                },
                cx,
            ),
        };

        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(28.))
            .child(label)
            .child(field)
            .into_any_element()
    }

    /// A value shown as text; click to type. While typing, Enter commits,
    /// Esc cancels and (for numbers) ↑/↓ step.
    fn text_field(
        &self,
        target: FieldTarget,
        look: FieldLook,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let FieldLook {
            shown,
            edit: edit_text,
            unit,
            changed,
            number,
        } = look;
        if let Some(f) = self.field.as_ref().filter(|f| f.target == target) {
            return field_box()
                .border_color(c(hex::BONE))
                .key_context("ParamField")
                .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                    cx.stop_propagation();
                    this.cancel_field(window, cx);
                }))
                .when(number, |d| {
                    d.capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                        cx.stop_propagation();
                        let big = window.modifiers().shift;
                        this.step_field(1., big, window, cx);
                    }))
                    .capture_action(cx.listener(
                        |this, _: &MoveDown, window, cx| {
                            cx.stop_propagation();
                            let big = window.modifiers().shift;
                            this.step_field(-1., big, window, cx);
                        },
                    ))
                })
                .when(number, |d| d.font_family(theme::MONO))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&f.input).appearance(false).small()),
                )
                .when(!unit.is_empty(), |d| {
                    d.child(div().flex_none().text_color(c(hex::INK_FAINT)).child(unit))
                })
                .into_any_element();
        }
        let id = match target {
            FieldTarget::Param { node, key, .. } => format!("val-{}-{key}", node.0),
            FieldTarget::Output(f) => format!("out-{f:?}"),
        };
        div()
            .id(SharedString::from(id))
            .flex_1()
            .min_w_0()
            .cursor(CursorStyle::IBeam)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.begin_field(target, edit_text.clone(), window, cx)
            }))
            .child(
                field_box()
                    .hover(|s| s.border_color(c(hex::LINE_STRONG)))
                    .when(number, |d| d.font_family(theme::MONO))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(c(if changed { hex::INK } else { hex::INK_DIM }))
                            .child(shown),
                    )
                    .when(!unit.is_empty(), |d| {
                        d.child(div().flex_none().text_color(c(hex::INK_FAINT)).child(unit))
                    }),
            )
            .into_any_element()
    }

    /// The control for one Output setting: a menu of its choices, or a
    /// text box for the file.
    pub(crate) fn output_control(&self, field: OutputField, cx: &mut Context<Self>) -> AnyElement {
        let o = &self.project().output;
        let value = o.get(field).to_string();
        let choices = o.choices(field);
        if choices.is_empty() {
            return self.text_field(
                FieldTarget::Output(field),
                FieldLook {
                    shown: value.clone(),
                    edit: value,
                    unit: "",
                    changed: true,
                    number: false,
                },
                cx,
            );
        }
        let ws = cx.entity().downgrade();
        let current = value.clone();
        div()
            .flex_1()
            .min_w_0()
            .child(
                choice_button(
                    SharedString::from(format!("out-{field:?}")),
                    value,
                    true,
                    cx,
                )
                .dropdown_menu(move |mut menu, _, _| {
                    for &opt in choices {
                        let ws = ws.clone();
                        menu = menu.item(PopupMenuItem::new(opt).checked(opt == current).on_click(
                            move |_, _, cx| {
                                ws.update(cx, |ws, cx| {
                                    let label =
                                        format!("set output {}", field.label().to_lowercase());
                                    ws.apply(&label, cx, |p| p.set_output(field, opt));
                                })
                                .ok();
                            },
                        ));
                    }
                    menu
                }),
            )
            .into_any_element()
    }

    fn output_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        for field in OutputField::ALL {
            let label = div()
                .w(px(LABEL_W))
                .flex_none()
                .text_color(c(hex::INK_DIM))
                .child(field.label());
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_h(px(28.))
                    .child(label)
                    .child(self.output_control(field, cx))
                    .into_any_element(),
            );
        }
        let close = icon_button("close-output", "close", false, false)
            .tooltip(tip("Back to the selection", Some("Esc".into())))
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspect_output = false;
                cx.notify();
            }));
        div()
            .child(title_block(
                "Output".into(),
                format!("{} Sequence", self.project().sequence.name),
                None,
            ))
            .child(section_shell(
                Category::Output,
                "Export",
                false,
                false,
                close,
                rows,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .p(px(12.))
                    .text_color(c(hex::INK_DIM))
                    .child(format!(
                        "Whole sequence · {}",
                        self.tc(self.project().end())
                    ))
                    .child(
                        button("render", Some("upload"), "Render", true)
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.queue_export(cx);
                                this.area = crate::workspace::Area::Export;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

/// How a typed field looks when it isn't being edited, and what typing
/// starts from.
struct FieldLook {
    shown: String,
    edit: String,
    unit: &'static str,
    changed: bool,
    number: bool,
}

/// The display text of a value, without its unit (shown separately).
fn spec_display(spec: &ParamSpec, value: &Value) -> String {
    let s = spec.format(value);
    s.strip_suffix(spec.unit)
        .map(|t| t.trim_end().to_string())
        .unwrap_or(s)
}

/// The text placed in the box when typing starts: plain ASCII digits.
pub(crate) fn number_text(spec: &ParamSpec, value: &Value) -> String {
    match (spec.kind, value) {
        (ParamKind::Number { decimals, .. }, Value::Number(n)) => {
            format!("{:.*}", decimals as usize, n)
        }
        (_, Value::Text(t)) => t.clone(),
        (_, Value::Number(n)) => n.to_string(),
    }
}

fn choice_button(id: SharedString, label: String, changed: bool, cx: &mut gpui_kit::App) -> Button {
    let variant = ButtonCustomVariant::new(cx)
        .color(c(hex::PLUGE_SUB))
        .foreground(c(if changed { hex::INK } else { hex::INK_DIM }))
        .hover(c(hex::PLUGE_RAISED))
        .active(c(hex::PLUGE_CONTROL));
    Button::new(id)
        .custom(variant)
        .label(label)
        .dropdown_caret(true)
        .small()
        .border_1()
        .border_color(c(hex::LINE))
        .w_full()
        .justify_between()
}

fn title_block(title: String, subtitle: String, extra: Option<AnyElement>) -> impl IntoElement {
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
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(title),
        )
        .child(div().text_color(c(hex::INK_DIM)).child(subtitle))
        .children(extra)
}

/// A node section: swatch, name, actions on the right, then rows. Bypassed
/// sections are dashed and dimmed; the focused one is raised.
struct Section {
    cat: Category,
    title: &'static str,
    name: Option<AnyElement>,
    bypass: bool,
    focused: bool,
    actions: AnyElement,
    rows: Vec<AnyElement>,
}

fn section_shell(
    cat: Category,
    title: &'static str,
    bypass: bool,
    focused: bool,
    actions: impl IntoElement,
    rows: Vec<AnyElement>,
) -> Section {
    Section {
        cat,
        title,
        name: None,
        bypass,
        focused,
        actions: actions.into_any_element(),
        rows,
    }
}

impl Section {
    fn child_header_name(mut self, name: impl IntoElement) -> AnyElement {
        self.name = Some(name.into_any_element());
        self.into_any_element()
    }
}

impl IntoElement for Section {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        let name = self.name.unwrap_or_else(|| {
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(self.title)
                .into_any_element()
        });
        div()
            .border_b_1()
            .border_color(c(hex::LINE))
            .when(self.focused, |d| d.bg(c(hex::PLUGE_RAISED)))
            .child(
                div()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .pl(px(12.))
                    .pr(px(6.))
                    .child(swatch(self.cat).when(self.bypass, |d| d.opacity(0.3)))
                    .child(name)
                    .when(self.bypass, |d| {
                        d.child(div().text_color(c(hex::INK_FAINT)).child("bypassed"))
                    })
                    .child(div().flex_1())
                    .child(self.actions),
            )
            .when(!self.rows.is_empty(), |d| {
                d.child(
                    div()
                        .px(px(12.))
                        .pb(px(10.))
                        .when(self.bypass, |d| d.opacity(0.55))
                        .children(self.rows),
                )
            })
            .into_any_element()
    }
}
