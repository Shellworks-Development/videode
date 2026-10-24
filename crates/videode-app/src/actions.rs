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
    ]
);

pub const CONTEXT: &str = "Workspace";

pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-n", NewProject, ctx),
        KeyBinding::new("secondary-o", OpenProject, ctx),
        KeyBinding::new("secondary-s", SaveProject, ctx),
        KeyBinding::new("secondary-shift-s", SaveProjectAs, ctx),
        KeyBinding::new("secondary-q", Quit, ctx),
        KeyBinding::new("secondary-z", Undo, ctx),
        KeyBinding::new("secondary-shift-z", Redo, ctx),
        KeyBinding::new("secondary-y", Redo, ctx),
        KeyBinding::new("secondary-e", Export, ctx),
        KeyBinding::new("s", Split, ctx),
        KeyBinding::new("delete", DeleteClip, ctx),
        KeyBinding::new("backspace", DeleteClip, ctx),
        KeyBinding::new("shift-delete", RippleDelete, ctx),
        KeyBinding::new("shift-backspace", RippleDelete, ctx),
        KeyBinding::new("escape", Deselect, ctx),
        KeyBinding::new("v", SelectTool, ctx),
        KeyBinding::new("b", BladeTool, ctx),
        KeyBinding::new("n", ToggleSnap, ctx),
        KeyBinding::new("m", ToggleTrack, ctx),
        KeyBinding::new("=", ZoomIn, ctx),
        KeyBinding::new("-", ZoomOut, ctx),
        KeyBinding::new("shift-z", ZoomToFit, ctx),
        KeyBinding::new("space", TogglePlay, ctx),
        KeyBinding::new("l", ShuttleForward, ctx),
        KeyBinding::new("k", ShuttleStop, ctx),
        KeyBinding::new("j", ShuttleBackward, ctx),
        KeyBinding::new("left", StepBack, ctx),
        KeyBinding::new("right", StepForward, ctx),
        KeyBinding::new("shift-left", SecondBack, ctx),
        KeyBinding::new("shift-right", SecondForward, ctx),
        KeyBinding::new("up", PrevEdit, ctx),
        KeyBinding::new("down", NextEdit, ctx),
        KeyBinding::new("home", GoToStart, ctx),
        KeyBinding::new("end", GoToEnd, ctx),
    ]);
}
