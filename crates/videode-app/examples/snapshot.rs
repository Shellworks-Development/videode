//! Renders the main window offscreen and writes a PNG, for visual checks
//! without a display: `cargo run -p videode-app --example snapshot -- out.png 1440 900`
//!
//! Needs GPUI's headless renderer (WGPU on Linux, Metal on macOS).

use std::sync::Arc;

use gpui::{
    AppContext as _, Bounds, HeadlessAppContext, WindowBounds, WindowOptions, point, px, size,
};
use videode_app::workspace::Workspace;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "videode.png".into());
    let width: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1440.);
    let height: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(900.);

    let mut cx = HeadlessAppContext::with_platform(
        gpui_platform::current_platform(true).text_system(),
        Arc::new(videode_app::assets::AppAssets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(videode_app::init);

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
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
    })?;
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
