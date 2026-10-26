//! Interaction tests on GPUI's test platform: real keystrokes and input
//! through the real key bindings, checked against the document.

use gpui::{Entity, TestAppContext, VisualTestContext};
use videode_core::{ClipId, Document, NodeKind, demo::demo_project};

use crate::workspace::{FieldTarget, PointerDrag, View, Workspace};

#[cfg(target_os = "macos")]
const MOD: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "ctrl";

fn open(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    let config = std::env::temp_dir().join(format!("videode-test-{}", std::process::id()));
    // SAFETY: every test sets the same value before reading it.
    unsafe { std::env::set_var("VIDEODE_CONFIG_DIR", config) };
    cx.update(crate::init);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.add_window_view(|window, cx| {
        Workspace::with_document(Document::new(demo_project(), None), window, cx)
    })
}

/// The demo clip with a Grade node (dusk_gradient), selected.
fn select_graded(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> ClipId {
    ws.update(cx, |ws, cx| {
        let id = ws
            .doc
            .project
            .sequence
            .clips
            .iter()
            .find(|c| c.first(NodeKind::Grade).is_some() && c.media.is_some())
            .expect("demo has a graded clip")
            .id;
        ws.select(Some(id));
        cx.notify();
        id
    })
}

fn exposure(ws: &Entity<Workspace>, cx: &mut VisualTestContext, clip: ClipId) -> f64 {
    ws.read_with(cx, |ws, _| {
        ws.doc
            .project
            .clip(clip)
            .unwrap()
            .first(NodeKind::Grade)
            .unwrap()
            .number("exposure")
    })
}

#[gpui::test]
fn tab_peeks_and_shift_tab_shows_the_graph(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    select_graded(&ws, cx);
    cx.simulate_keystrokes("tab");
    assert!(
        ws.read_with(cx, |ws, _| ws.peek.is_some()),
        "Tab opens peek"
    );
    cx.simulate_keystrokes("escape");
    assert!(
        ws.read_with(cx, |ws, _| ws.peek.is_none()),
        "Esc closes peek first"
    );
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(ws.read_with(cx, |ws, _| ws.view), View::Graph);
    cx.simulate_keystrokes("escape");
    assert_eq!(ws.read_with(cx, |ws, _| ws.view), View::Timeline);
    assert!(
        ws.read_with(cx, |ws, _| ws.selection.is_some()),
        "Esc from the graph keeps the selection"
    );
}

#[gpui::test]
fn typing_in_the_palette_does_not_trigger_shortcuts(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let clips = ws.read_with(cx, |ws, _| ws.doc.project.sequence.clips.len());
    cx.simulate_keystrokes(&format!("{MOD}-k"));
    assert!(ws.read_with(cx, |ws, _| ws.palette.is_some()));
    // S splits and Space plays in the timeline; here they are just text.
    cx.simulate_input("s ");
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.doc.project.sequence.clips.len()),
        clips
    );
    assert!(ws.read_with(cx, |ws, _| ws.playback.is_none()));
    let query = ws.read_with(cx, |ws, cx| {
        ws.palette
            .as_ref()
            .unwrap()
            .input
            .read(cx)
            .value()
            .to_string()
    });
    assert_eq!(query, "s ");
    cx.simulate_keystrokes("escape");
    assert!(ws.read_with(cx, |ws, _| ws.palette.is_none()));
}

#[gpui::test]
fn the_palette_runs_the_best_match(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    cx.simulate_keystrokes(&format!("{MOD}-k"));
    cx.simulate_input("graph view");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.view), View::Graph);
    assert!(ws.read_with(cx, |ws, _| ws.palette.is_none()));
    assert_eq!(ws.read_with(cx, |ws, _| ws.recent_commands.len()), 1);
}

#[gpui::test]
fn palette_adds_nodes_to_the_selection(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let clip = select_graded(&ws, cx);
    let before = ws.read_with(cx, |ws, _| ws.doc.project.clip(clip).unwrap().nodes.len());
    cx.simulate_keystrokes("a");
    cx.simulate_input("blur");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let nodes = ws.read_with(cx, |ws, _| {
        ws.doc
            .project
            .clip(clip)
            .unwrap()
            .nodes
            .iter()
            .map(|n| n.kind)
            .collect::<Vec<_>>()
    });
    assert_eq!(nodes.len(), before + 1);
    assert_eq!(nodes.last(), Some(&NodeKind::Blur));
}

#[gpui::test]
fn typed_values_commit_on_enter_and_cancel_on_escape(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let clip = select_graded(&ws, cx);
    let target = ws.read_with(cx, |ws, _| {
        let node = ws
            .doc
            .project
            .clip(clip)
            .unwrap()
            .first(NodeKind::Grade)
            .unwrap()
            .id;
        FieldTarget::Param {
            clip,
            node,
            key: "exposure",
        }
    });
    let start = |cx: &mut VisualTestContext| {
        ws.update_in(cx, |ws, window, cx| {
            ws.begin_field(target, "0".into(), window, cx)
        });
    };

    start(cx);
    cx.simulate_input("1.5");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(exposure(&ws, cx, clip), 1.5);
    assert!(ws.read_with(cx, |ws, _| ws.field.is_none()));

    start(cx);
    cx.simulate_input("3");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(exposure(&ws, cx, clip), 1.5, "Esc discards the typed value");

    start(cx);
    cx.simulate_input("bright");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(exposure(&ws, cx, clip), 1.5);
    assert!(
        ws.read_with(cx, |ws, _| ws.status.error),
        "invalid input is reported"
    );
}

/// Drags the selected clip's exposure label from x = 100 to `to`.
fn scrub_exposure(ws: &Entity<Workspace>, cx: &mut VisualTestContext, clip: ClipId, to: f32) {
    let v0 = exposure(ws, cx, clip);
    ws.update(cx, |ws, cx| {
        let node = ws
            .doc
            .project
            .clip(clip)
            .unwrap()
            .first(NodeKind::Grade)
            .unwrap()
            .id;
        ws.pdrag = Some(PointerDrag::Scrub {
            clip,
            node,
            key: "exposure",
            x0: 100.,
            v0,
        });
        for x in [110., (110. + to) / 2., to] {
            ws.pointer_move(
                gpui::point(gpui::px(x), gpui::px(0.)),
                Default::default(),
                cx,
            );
        }
        ws.pointer_up(cx);
    });
}

#[gpui::test]
fn scrubbing_is_one_undo_step(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let clip = select_graded(&ws, cx);
    let before = exposure(&ws, cx, clip);
    scrub_exposure(&ws, cx, clip, 160.);
    let after = exposure(&ws, cx, clip);
    assert!(after > before, "dragging right raises the value");
    cx.simulate_keystrokes(&format!("{MOD}-z"));
    assert_eq!(exposure(&ws, cx, clip), before, "one undo restores it");
}

#[gpui::test]
fn each_scrub_is_its_own_undo_step(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let clip = select_graded(&ws, cx);
    scrub_exposure(&ws, cx, clip, 160.);
    let first = exposure(&ws, cx, clip);
    scrub_exposure(&ws, cx, clip, 160.);
    assert!(exposure(&ws, cx, clip) > first);
    cx.simulate_keystrokes(&format!("{MOD}-z"));
    assert_eq!(
        exposure(&ws, cx, clip),
        first,
        "undo only takes back the second drag, though both started at the same x"
    );
}

#[gpui::test]
fn delete_in_peek_removes_the_focused_node_not_the_clip(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let clip = select_graded(&ws, cx);
    let clips = ws.read_with(cx, |ws, _| ws.doc.project.sequence.clips.len());
    ws.update(cx, |ws, cx| {
        ws.focus_node = ws
            .doc
            .project
            .clip(clip)
            .unwrap()
            .first(NodeKind::Grade)
            .map(|n| n.id);
        ws.open_peek();
        cx.notify();
    });
    cx.simulate_keystrokes("delete");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.doc.project.sequence.clips.len(), clips);
        assert!(
            ws.doc
                .project
                .clip(clip)
                .unwrap()
                .first(NodeKind::Grade)
                .is_none()
        );
    });
    cx.simulate_keystrokes("shift-b");
    assert!(
        ws.read_with(cx, |ws, _| ws.status.text.contains("Select a node")),
        "nothing focused to bypass"
    );
}

#[gpui::test]
fn bypass_and_reorder_from_peek(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let clip = select_graded(&ws, cx);
    let (grade, fade) = ws.read_with(cx, |ws, _| {
        let c = ws.doc.project.clip(clip).unwrap();
        (
            c.first(NodeKind::Grade).unwrap().id,
            c.first(NodeKind::Fade).unwrap().id,
        )
    });
    ws.update(cx, |ws, cx| {
        ws.focus_node = Some(grade);
        cx.notify();
    });
    cx.simulate_keystrokes("shift-b");
    assert!(ws.read_with(cx, |ws, _| {
        ws.doc
            .project
            .clip(clip)
            .unwrap()
            .node(grade)
            .unwrap()
            .bypass
    }));
    // Drag Fade (slot 2) one slot left.
    ws.update(cx, |ws, cx| {
        ws.drop_node(clip, fade, -(crate::peek::CARD_W + 40.), cx)
    });
    let order = ws.read_with(cx, |ws, _| {
        ws.doc
            .project
            .clip(clip)
            .unwrap()
            .nodes
            .iter()
            .map(|n| n.kind)
            .collect::<Vec<_>>()
    });
    assert_eq!(order, [NodeKind::Source, NodeKind::Fade, NodeKind::Grade]);
}

#[gpui::test]
fn relinking_one_file_finds_its_neighbours(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let dir = std::env::temp_dir().join(format!("videode-relink-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["dusk_gradient.mov", "mandel_dive.mp4", "voiceover_t3.wav"] {
        std::fs::write(dir.join(name), b"").unwrap();
    }
    let offline = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, _| {
            ws.doc.project.media.iter().filter(|m| m.offline).count()
        })
    };
    let before = offline(&ws, cx);
    ws.update(cx, |ws, cx| {
        let dusk = ws
            .doc
            .project
            .media
            .iter()
            .find(|m| m.file_name() == "dusk_gradient.mov")
            .unwrap()
            .id;
        ws.relink_to(dusk, dir.join("dusk_gradient.mov"), cx);
    });
    assert_eq!(
        offline(&ws, cx),
        before - 3,
        "the two neighbours came along"
    );
    cx.simulate_keystrokes(&format!("{MOD}-z"));
    assert_eq!(offline(&ws, cx), before, "one undo puts all three back");
}

#[gpui::test]
fn export_area_and_presets(cx: &mut TestAppContext) {
    use crate::workspace::Area;
    let (ws, cx) = open(cx);
    cx.simulate_keystrokes(&format!("{MOD}-4"));
    assert_eq!(ws.read_with(cx, |ws, _| ws.area), Area::Export);
    ws.update(cx, |ws, cx| ws.apply_preset(3, cx));
    let output = |cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, _| {
            let o = &ws.doc.project.output;
            (o.container.clone(), o.codec.clone(), o.path.clone())
        })
    };
    assert_eq!(
        output(cx),
        (
            "MOV".into(),
            "ProRes 422 HQ".into(),
            "signal_study.mov".into()
        )
    );
    // A preset is one undo step.
    cx.simulate_keystrokes(&format!("{MOD}-z"));
    assert_eq!(
        output(cx),
        ("MP4".into(), "H.264".into(), "signal_study.mp4".into())
    );
    cx.simulate_keystrokes(&format!("{MOD}-1"));
    assert_eq!(ws.read_with(cx, |ws, _| ws.area), Area::Edit);
}
