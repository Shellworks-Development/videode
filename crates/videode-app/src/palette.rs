//! The command palette (Ctrl/Cmd+K): every action, node, clip and media
//! file, fuzzy-matched. Disabled entries say why instead of disappearing.

use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::prelude::*;
use gpui_kit::{
    Action, AnimationExt as _, AnyElement, Context, Entity, FontWeight, HighlightStyle, Hsla,
    MouseButton, SharedString, StyledText, Subscription, Window, div, hsla, point, px,
};
use videode_core::fuzzy::fuzzy;
use videode_core::{ClipId, MediaId, NodeKind};

use crate::actions::*;
use crate::theme::{self, CategoryExt, c, hex};
use crate::ui::{icon, kbd};
use crate::workspace::{View, Workspace};

const WIDTH: f32 = 580.;
const ROW_H: f32 = 30.;
const MAX_VISIBLE: usize = 12;

pub(crate) struct Palette {
    pub input: Entity<InputState>,
    pub selected: usize,
    pub _subscription: Subscription,
}

#[derive(Clone)]
enum Run {
    Action(SharedString),
    AddNode(NodeKind),
    Clip(ClipId),
    Media(MediaId),
}

struct Item {
    group: &'static str,
    id: String,
    label: String,
    detail: Option<String>,
    color: Option<Hsla>,
    shortcut: Option<String>,
    disabled: Option<&'static str>,
    run: Run,
    positions: Vec<usize>,
    score: i32,
}

/// Every action the palette offers, with when it's unavailable.
/// When an action can't run, the reason shown next to it.
type Unavailable = fn(&Workspace) -> Option<&'static str>;

fn actions() -> Vec<(&'static str, Box<dyn Action>, Unavailable)> {
    fn always(_: &Workspace) -> Option<&'static str> {
        None
    }
    fn needs_clip(ws: &Workspace) -> Option<&'static str> {
        ws.selection.is_none().then_some("select a clip first")
    }
    fn needs_node(ws: &Workspace) -> Option<&'static str> {
        let processing = ws
            .focus_node
            .zip(ws.selection.and_then(|id| ws.project().clip(id)))
            .and_then(|(n, c)| c.node(n))
            .is_some_and(|n| !n.kind.is_generator());
        (!processing).then_some("click a node in the inspector or peek first")
    }
    fn needs_undo(ws: &Workspace) -> Option<&'static str> {
        ws.doc.undo_label().is_none().then_some("nothing to undo")
    }
    fn needs_redo(ws: &Workspace) -> Option<&'static str> {
        ws.doc.redo_label().is_none().then_some("nothing to redo")
    }
    vec![
        ("Split at playhead", Box::new(Split), always),
        ("Delete clip", Box::new(DeleteClip), needs_clip),
        ("Ripple delete clip", Box::new(RippleDelete), needs_clip),
        ("Peek at nodes", Box::new(TogglePeek), needs_clip),
        ("Graph view", Box::new(ToggleGraph), always),
        ("Add node…", Box::new(AddNodeMenu), needs_clip),
        ("Bypass or enable node", Box::new(BypassNode), needs_node),
        ("Undo", Box::new(Undo), needs_undo),
        ("Redo", Box::new(Redo), needs_redo),
        ("Blade tool", Box::new(BladeTool), always),
        ("Select tool", Box::new(SelectTool), always),
        ("Toggle snapping", Box::new(ToggleSnap), always),
        ("Mute or hide track", Box::new(ToggleTrack), needs_clip),
        ("Zoom in", Box::new(ZoomIn), always),
        ("Zoom out", Box::new(ZoomOut), always),
        ("Zoom to fit", Box::new(ZoomToFit), always),
        ("Play / pause", Box::new(TogglePlay), always),
        ("Go to start", Box::new(GoToStart), always),
        ("Go to end", Box::new(GoToEnd), always),
        ("Previous edit point", Box::new(PrevEdit), always),
        ("Next edit point", Box::new(NextEdit), always),
        ("Open Output node (export)", Box::new(OpenOutput), always),
        ("New project", Box::new(NewProject), always),
        ("Open project…", Box::new(OpenProject), always),
        ("Open demo project", Box::new(OpenDemo), always),
        ("Import media…", Box::new(ImportMedia), always),
        ("Save", Box::new(SaveProject), always),
        ("Save as…", Box::new(SaveProjectAs), always),
        ("Settings", Box::new(OpenSettings), always),
        ("About videode", Box::new(About), always),
        ("Quit", Box::new(Quit), always),
    ]
}

impl Workspace {
    pub(crate) fn open_palette(
        &mut self,
        prefill: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_field(window, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search actions, nodes, clips, media…")
        });
        input.update(cx, |s, cx| {
            // set_value leaves the caret at the end, ready to keep typing.
            s.set_value(prefill.to_string(), window, cx);
            s.focus(window, cx);
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if let Some(p) = &mut this.palette {
                        p.selected = 0;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.run_palette(None, window, cx),
                _ => {}
            },
        );
        self.palette = Some(Palette {
            input,
            selected: 0,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn palette_items(&self, query: &str, window: &Window) -> Vec<Item> {
        let p = self.project();
        let clip_kind = self.selection.and_then(|id| p.clip(id)).map(|c| c.kind);
        let mut items = Vec::new();
        for (label, action, unavailable) in actions() {
            let shortcut = window
                .highest_precedence_binding_for_action(&*action)
                .map(|b| shortcut_text(&b));
            items.push(Item {
                group: "Actions",
                id: format!("action:{}", action.name()),
                label: label.into(),
                detail: None,
                color: None,
                shortcut,
                disabled: unavailable(self),
                run: Run::Action(action.name().into()),
                positions: Vec::new(),
                score: 0,
            });
        }
        for kind in NodeKind::ALL.into_iter().filter(|k| !k.is_generator()) {
            let disabled = match clip_kind {
                None => Some("select a clip first"),
                Some(k) if !kind.applies_to(k) => Some("doesn't work on this kind of clip"),
                _ => None,
            };
            items.push(Item {
                group: "Add node",
                id: format!("node:{}", kind.ident()),
                label: format!("Add {}", kind.name()),
                detail: Some(kind.description().into()),
                color: Some(kind.category().color()),
                shortcut: None,
                disabled,
                run: Run::AddNode(kind),
                positions: Vec::new(),
                score: 0,
            });
        }
        for clip in &p.sequence.clips {
            let track = p
                .track(clip.track)
                .map(|t| t.label.clone())
                .unwrap_or_default();
            items.push(Item {
                group: "Clips",
                id: format!("clip:{}", clip.id.0),
                label: p.clip_name(clip),
                detail: Some(format!("{track} · {}", self.tc(clip.start))),
                color: clip.nodes.first().map(|n| n.kind.category().color()),
                shortcut: None,
                disabled: None,
                run: Run::Clip(clip.id),
                positions: Vec::new(),
                score: 0,
            });
        }
        for m in &p.media {
            items.push(Item {
                group: "Media",
                id: format!("media:{}", m.id.0),
                label: m.file_name(),
                detail: Some(if m.offline {
                    "offline".into()
                } else {
                    "add at the playhead".into()
                }),
                color: None,
                shortcut: None,
                disabled: None,
                run: Run::Media(m.id),
                positions: Vec::new(),
                score: 0,
            });
        }

        if query.trim().is_empty() {
            // Recent first, then everything in its group order.
            let mut recent: Vec<Item> = Vec::new();
            for id in &self.recent_commands {
                if let Some(i) = items.iter().position(|it| &it.id == id) {
                    let mut it = items.remove(i);
                    it.group = "Recent";
                    recent.push(it);
                }
            }
            recent.extend(items);
            return recent;
        }
        let mut matched: Vec<Item> = items
            .into_iter()
            .filter_map(|mut it| {
                let m = fuzzy(query, &it.label)?;
                it.positions = m.positions;
                it.score = m.score - if it.disabled.is_some() { 20 } else { 0 };
                Some(it)
            })
            .collect();
        matched.sort_by_key(|it| std::cmp::Reverse(it.score));
        // Keep groups together, ordered by their best match.
        let mut order: Vec<&'static str> = Vec::new();
        for it in &matched {
            if !order.contains(&it.group) {
                order.push(it.group);
            }
        }
        let mut grouped = Vec::with_capacity(matched.len());
        for g in order {
            let (these, rest): (Vec<Item>, Vec<Item>) =
                matched.into_iter().partition(|it| it.group == g);
            grouped.extend(these);
            matched = rest;
        }
        grouped
    }

    /// Runs the selected entry, or entry `index` when clicked.
    fn run_palette(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &self.palette else {
            return;
        };
        let query = p.input.read(cx).value().to_string();
        let items = self.palette_items(&query, window);
        let Some(item) = items.into_iter().nth(index.unwrap_or(p.selected)) else {
            return;
        };
        if let Some(why) = item.disabled {
            self.say(format!("{} · {why}", item.label));
            cx.notify();
            return;
        }
        self.recent_commands.retain(|id| *id != item.id);
        self.recent_commands.insert(0, item.id.clone());
        self.recent_commands.truncate(3);
        self.close_palette(window, cx);
        match item.run {
            Run::Action(name) => {
                if let Some(action) = actions()
                    .into_iter()
                    .find(|(_, a, _)| a.name() == name.as_ref())
                    .map(|(_, a, _)| a)
                {
                    window.dispatch_action(action, cx);
                }
            }
            Run::AddNode(kind) => {
                if let Some(clip) = self.selection {
                    self.add_node(clip, kind, cx);
                }
            }
            Run::Clip(clip) => {
                self.view = View::Timeline;
                self.select(Some(clip));
                self.reveal_clip(clip);
                let name = self
                    .project()
                    .clip(clip)
                    .map(|c| self.project().clip_name(c))
                    .unwrap_or_default();
                self.say(format!("Selected {name} · Tab to peek at its nodes"));
            }
            Run::Media(media) => self.add_media_at_playhead(media, cx),
        }
        cx.notify();
    }

    fn move_palette(&mut self, delta: isize, window: &Window, cx: &mut Context<Self>) {
        let Some(p) = &self.palette else {
            return;
        };
        let query = p.input.read(cx).value().to_string();
        let count = self.palette_items(&query, window).len();
        if count == 0 {
            return;
        }
        if let Some(p) = &mut self.palette {
            p.selected = (p.selected as isize + delta).rem_euclid(count as isize) as usize;
        }
        cx.notify();
    }

    pub(crate) fn render_palette(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let p = self.palette.as_ref()?;
        let query = p.input.read(cx).value().to_string();
        let items = self.palette_items(&query, window);
        let selected = p.selected.min(items.len().saturating_sub(1));

        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_group = "";
        for (i, it) in items.iter().enumerate() {
            if it.group != last_group {
                last_group = it.group;
                rows.push(
                    div()
                        .px(px(14.))
                        .pt(px(8.))
                        .pb(px(3.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(hex::INK_FAINT))
                        .child(it.group)
                        .into_any_element(),
                );
            }
            rows.push(self.palette_row(i, it, i == selected, cx));
        }
        if items.is_empty() {
            rows.push(
                div()
                    .px(px(14.))
                    .py(px(16.))
                    .text_color(c(hex::INK_DIM))
                    .child(format!("Nothing matches “{query}”"))
                    .into_any_element(),
            );
        }

        let panel = div()
            .id("palette")
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(c(hex::LINE_STRONG))
            .bg(c(hex::PLUGE_RAISED))
            .shadow(vec![gpui_kit::BoxShadow {
                color: hsla(0., 0., 0., 0.6),
                offset: point(px(0.), px(18.)),
                blur_radius: px(48.),
                spread_radius: px(0.),
                inset: false,
            }])
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                cx.stop_propagation();
                this.move_palette(-1, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                cx.stop_propagation();
                this.move_palette(1, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                cx.stop_propagation();
                this.close_palette(window, cx);
            }))
            .child(
                div()
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(c(hex::LINE))
                    .text_size(theme::TITLE)
                    .child(icon("search", px(16.), c(hex::INK_DIM)))
                    .child(div().flex_1().child(Input::new(&p.input).appearance(false)))
                    .child(kbd("Esc")),
            )
            .child(
                div()
                    .id("palette-list")
                    .max_h(px(ROW_H * MAX_VISIBLE as f32 + 60.))
                    .overflow_y_scroll()
                    .py(px(4.))
                    .children(rows),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .px(px(14.))
                    .border_t_1()
                    .border_color(c(hex::LINE))
                    .text_color(c(hex::INK_FAINT))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .items_center()
                            .child(kbd("↑ ↓"))
                            .child("move"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .items_center()
                            .child(kbd("Enter"))
                            .child("run"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .items_center()
                            .child(kbd("Esc"))
                            .child("close"),
                    ),
            )
            .with_animation(
                crate::motion::key("palette", 0),
                crate::motion::anim(crate::motion::DIALOG),
                |d, t| d.opacity(t).mt(px(crate::motion::lerp(-6., 0., t))),
            );

        Some(
            div()
                .id("palette-scrim")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(72.))
                .occlude()
                .bg(hsla(0., 0., 0., 0.35))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    fn palette_row(
        &self,
        i: usize,
        it: &Item,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Matched characters in bone and medium weight.
        let label = it.label.clone();
        let highlights: Vec<_> = label
            .char_indices()
            .enumerate()
            .filter(|(ci, _)| it.positions.contains(ci))
            .map(|(_, (b, ch))| {
                (
                    b..b + ch.len_utf8(),
                    HighlightStyle {
                        color: Some(c(hex::BONE)),
                        font_weight: Some(FontWeight::SEMIBOLD),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let text = StyledText::new(label).with_highlights(highlights);
        div()
            .id(SharedString::from(format!("pal-{i}")))
            .h(px(ROW_H))
            .mx(px(4.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(selected, |d| d.bg(c(hex::PLUGE_HOVER)))
            .when(it.disabled.is_some(), |d| d.text_color(c(hex::INK_FAINT)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered
                    && let Some(p) = &mut this.palette
                    && p.selected != i
                {
                    p.selected = i;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.run_palette(Some(i), window, cx)))
            .child(
                div()
                    .w(px(8.))
                    .h(px(8.))
                    .flex_none()
                    .rounded(px(2.))
                    .when_some(it.color, |d, col| d.bg(col))
                    .when(it.color.is_none(), |d| {
                        d.border_1().border_color(c(hex::LINE_STRONG))
                    }),
            )
            .child(div().flex_none().child(text))
            .when_some(it.detail.clone(), |d, detail| {
                d.child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(c(hex::INK_FAINT))
                        .child(detail),
                )
            })
            .child(div().flex_1())
            .when_some(it.disabled, |d, why| {
                d.child(div().flex_none().text_color(c(hex::INK_FAINT)).child(why))
            })
            .when(it.disabled.is_none(), |d| {
                d.when_some(it.shortcut.clone(), |d, k| d.child(kbd(k)))
            })
            .into_any_element()
    }
}
