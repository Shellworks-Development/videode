//! Renders the main window offscreen and writes a PNG, for visual checks
//! without a display: `cargo run -p videode-app --example snapshot -- out.png 1440 900 [modes]`
//!
//! Modes, comma-separated:
//! - `demo`: the demo project with a clip selected and the playhead inside it
//! - `settings`, `about`: open that dialog
//! - `splash@0.4`: the launch splash frozen 0.4 s in
//! - `media`, `peek`, `graph`, `output`, `focus`, `edit`, `palette=<query>`: see
//!   `Workspace::preview`
//!
//! Animations are off (end states) except in `splash@` mode.
//!
//! Needs GPUI's headless renderer (WGPU on Linux, Metal on macOS).

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AppContext as _, Bounds, HeadlessAppContext, WindowBounds, WindowOptions, point, px, size,
};
use videode_app::workspace::{Modal, Workspace};
use videode_core::{ClipId, Document, demo::demo_project};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "videode.png".into());
    let width: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1440.);
    let height: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(900.);
    let modes = args.next().unwrap_or_default();
    let has = |m: &str| modes.split(',').any(|x| x == m);
    let demo = has("demo");
    let splash: Option<f32> = modes
        .split(',')
        .find_map(|m| m.strip_prefix("splash@").and_then(|s| s.parse().ok()));

    // Never read or write the user's real settings.
    let config = std::env::temp_dir().join("videode-snapshot-config");
    // SAFETY: set before any other thread starts.
    unsafe { std::env::set_var("VIDEODE_CONFIG_DIR", &config) };

    let mut cx = HeadlessAppContext::with_platform(
        gpui_platform::current_platform(true).text_system(),
        Arc::new(videode_app::assets::AppAssets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(videode_app::init);
    cx.update(|cx| cx.set_reduce_motion(splash.is_none()));

    let (handle, _) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(width), px(height)),
                })),
                focus: false,
                show: false,
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut ws = if demo {
                        let project = demo_project();
                        let selected: Option<ClipId> = project
                            .sequence
                            .clips
                            .iter()
                            .find(|c| c.nodes.len() >= 3 && c.media.is_some())
                            .map(|c| c.id);
                        let mut ws =
                            Workspace::with_document(Document::new(project, None), window, cx);
                        ws.set_view(240, selected);
                        ws
                    } else {
                        Workspace::new(window, cx)
                    };
                    if has("settings") {
                        ws.show_modal(Modal::Settings);
                    } else if has("about") {
                        ws.show_modal(Modal::About);
                    }
                    for mode in modes.split(',') {
                        ws.preview(mode, window, cx);
                    }
                    if let Some(t) = splash {
                        ws.preview_splash(Duration::from_secs_f32(t));
                    }
                    ws
                })
            },
        )
    })?;
    // Frames and thumbnails decode on their own threads: draw, give them a
    // moment, and draw again until nothing new arrives.
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })?;
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })?;
    let image = cx.capture_screenshot(handle)?;
    image.save(&out)?;
    println!("wrote {out} ({}x{})", image.width(), image.height());
    Ok(())
}
