//! Every user command is a GPUI action, so menus, shortcuts and (later) the
//! command palette all go through one path.

use gpui_kit::{App, KeyBinding, actions};

actions!(
    videode,
    [
        NewProject,
        OpenProject,
        OpenDemo,
        SaveProject,
        SaveProjectAs,
        Quit,
        Undo,
        Redo,
        Split,
        DeleteClip,
        RippleDelete,
        Deselect,
        SelectTool,
        BladeTool,
        ToggleSnap,
        ToggleTrack,
        ZoomIn,
        ZoomOut,
        ZoomToFit,
        TogglePlay,
        ShuttleForward,
        ShuttleStop,
        ShuttleBackward,
        StepBack,
        StepForward,
        SecondBack,
        SecondForward,
        PrevEdit,
        NextEdit,
        GoToStart,
        GoToEnd,
        Export,
        OpenSettings,
        About,
        TogglePeek,
        ToggleGraph,
        CommandPalette,
        OpenOutput,
        BypassNode,
        AddNodeMenu,
    ]
);

pub const CONTEXT: &str = "Workspace";
/// Single-key shortcuts must not fire while typing in a text field.
pub const NOT_TYPING: &str = "Workspace && !Input";

pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    let keys = Some(NOT_TYPING);
    cx.bind_keys([
        KeyBinding::new("secondary-n", NewProject, ctx),
        KeyBinding::new("secondary-o", OpenProject, ctx),
        KeyBinding::new("secondary-s", SaveProject, ctx),
        KeyBinding::new("secondary-shift-s", SaveProjectAs, ctx),
        KeyBinding::new("secondary-q", Quit, ctx),
        KeyBinding::new("secondary-z", Undo, keys),
        KeyBinding::new("secondary-shift-z", Redo, keys),
        KeyBinding::new("secondary-y", Redo, keys),
        KeyBinding::new("secondary-e", Export, ctx),
        KeyBinding::new("secondary-,", OpenSettings, ctx),
        KeyBinding::new("secondary-k", CommandPalette, ctx),
        KeyBinding::new("escape", Deselect, keys),
        KeyBinding::new("tab", TogglePeek, keys),
        KeyBinding::new("shift-tab", ToggleGraph, keys),
        KeyBinding::new("s", Split, keys),
        KeyBinding::new("delete", DeleteClip, keys),
        KeyBinding::new("backspace", DeleteClip, keys),
        KeyBinding::new("shift-delete", RippleDelete, keys),
        KeyBinding::new("shift-backspace", RippleDelete, keys),
        KeyBinding::new("v", SelectTool, keys),
        KeyBinding::new("b", BladeTool, keys),
        KeyBinding::new("shift-b", BypassNode, keys),
        KeyBinding::new("a", AddNodeMenu, keys),
        KeyBinding::new("n", ToggleSnap, keys),
        KeyBinding::new("m", ToggleTrack, keys),
        KeyBinding::new("=", ZoomIn, keys),
        KeyBinding::new("-", ZoomOut, keys),
        KeyBinding::new("shift-z", ZoomToFit, keys),
        KeyBinding::new("space", TogglePlay, keys),
        KeyBinding::new("l", ShuttleForward, keys),
        KeyBinding::new("k", ShuttleStop, keys),
        KeyBinding::new("j", ShuttleBackward, keys),
        KeyBinding::new("left", StepBack, keys),
        KeyBinding::new("right", StepForward, keys),
        KeyBinding::new("shift-left", SecondBack, keys),
        KeyBinding::new("shift-right", SecondForward, keys),
        KeyBinding::new("up", PrevEdit, keys),
        KeyBinding::new("down", NextEdit, keys),
        KeyBinding::new("home", GoToStart, keys),
        KeyBinding::new("end", GoToEnd, keys),
    ]);
}

/// A binding as keycap text in the Test Card style, e.g. `Ctrl K`, `⇧ Tab`.
pub fn shortcut_text(binding: &KeyBinding) -> String {
    binding
        .keystrokes()
        .iter()
        .map(|k| {
            let text = k.to_string();
            if text == "-" {
                return "−".to_string();
            }
            text.split('-')
                .map(|part| match part {
                    "ctrl" => "Ctrl".to_string(),
                    "alt" => {
                        if cfg!(target_os = "macos") {
                            "⌥".into()
                        } else {
                            "Alt".into()
                        }
                    }
                    "shift" => "⇧".into(),
                    "cmd" | "super" | "win" => {
                        if cfg!(target_os = "macos") {
                            "⌘".into()
                        } else {
                            "Super".into()
                        }
                    }
                    "space" => "Space".into(),
                    "tab" => "Tab".into(),
                    "escape" => "Esc".into(),
                    "delete" => "Del".into(),
                    "backspace" => "⌫".into(),
                    "left" => "←".into(),
                    "right" => "→".into(),
                    "up" => "↑".into(),
                    "down" => "↓".into(),
                    "home" => "Home".into(),
                    "end" => "End".into(),
                    "" => "-".into(),
                    other => other.to_uppercase(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(", ")
}
