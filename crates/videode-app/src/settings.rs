//! App-wide settings: loaded at startup, saved on every change, and applied
//! immediately. The animation setting drives GPUI's `reduce_motion` flag,
//! which every animation in the app checks.

use std::path::PathBuf;

use gpui_kit::{App, Global};
use videode_core::Settings;

pub struct AppSettings {
    pub settings: Settings,
    /// Where settings are saved; `None` if no config folder could be found.
    pub path: Option<PathBuf>,
    /// The operating system's reduce-motion preference, if it could be read.
    pub system_reduces_motion: Option<bool>,
    /// Problems from loading or saving, shown in the settings panel.
    pub notes: Vec<String>,
}

impl Global for AppSettings {}

/// `VIDEODE_CONFIG_DIR` overrides the platform config folder (used by the
/// snapshot tool so it never reads or writes real settings).
fn settings_path() -> Option<PathBuf> {
    let dir = match std::env::var_os("VIDEODE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::config_dir()?.join("videode"),
    };
    Some(dir.join("settings.kdl"))
}

fn load() -> (Settings, Vec<String>) {
    let Some(path) = settings_path() else {
        return (
            Settings::default(),
            vec!["No config folder found; settings won't be saved".into()],
        );
    };
    match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), Vec::new()),
        Err(e) => (
            Settings::default(),
            vec![format!("Couldn't read {}: {e}", path.display())],
        ),
        Ok(text) => match Settings::from_kdl(&text) {
            Ok((s, notes)) => (s, notes),
            Err(e) => (
                Settings::default(),
                vec![format!(
                    "{} isn't valid KDL, using defaults: {e}",
                    path.display()
                )],
            ),
        },
    }
}

pub fn init(cx: &mut App) {
    let (settings, notes) = load();
    cx.set_global(AppSettings {
        settings,
        path: settings_path(),
        system_reduces_motion: system_reduces_motion(),
        notes,
    });
    apply_motion(cx);
}

pub fn get(cx: &App) -> &Settings {
    &cx.global::<AppSettings>().settings
}

/// Changes settings, saves them and applies the result.
pub fn update(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    let s = cx.global_mut::<AppSettings>();
    f(&mut s.settings);
    if let Some(path) = s.path.clone() {
        let text = s.settings.to_kdl();
        let saved = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, text));
        s.notes.retain(|n| !n.starts_with("Couldn't save"));
        if let Err(e) = saved {
            s.notes
                .push(format!("Couldn't save {}: {e}", path.display()));
        }
    }
    apply_motion(cx);
}

/// Re-reads the OS preference; called when the settings panel opens so a
/// change made while videode runs is picked up.
pub fn refresh_system(cx: &mut App) {
    cx.global_mut::<AppSettings>().system_reduces_motion = system_reduces_motion();
    apply_motion(cx);
}

fn apply_motion(cx: &mut App) {
    let s = cx.global::<AppSettings>();
    let on = s
        .settings
        .animations
        .enabled(s.system_reduces_motion.unwrap_or(false));
    cx.set_reduce_motion(!on);
}

pub fn motion(cx: &App) -> bool {
    !cx.reduce_motion()
}

/* ---------- OS reduce-motion preference ---------- */

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "macos"))]
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn system_reduces_motion() -> Option<bool> {
    // GNOME and most GTK desktops.
    if let Some(v) = command_output(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
    ) {
        return Some(v == "false");
    }
    // KDE Plasma: an animation speed of 0 means "instant".
    for tool in ["kreadconfig6", "kreadconfig5"] {
        if let Some(v) = command_output(
            tool,
            &[
                "--file",
                "kdeglobals",
                "--group",
                "KDE",
                "--key",
                "AnimationDurationFactor",
            ],
        ) {
            return Some(v.parse::<f64>().is_ok_and(|f| f == 0.0));
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn system_reduces_motion() -> Option<bool> {
    // Missing key means the user never turned it on.
    Some(
        command_output(
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .as_deref()
            == Some("1"),
    )
}

#[cfg(windows)]
fn system_reduces_motion() -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    // "Show animations in Windows" in Settings › Accessibility › Visual effects.
    let mut enabled: i32 = 1;
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0)
    };
    (ok != 0).then_some(enabled == 0)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos",
    windows
)))]
fn system_reduces_motion() -> Option<bool> {
    None
}
