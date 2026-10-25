//! The videode desktop application: window, theme, actions and the editor.

pub mod actions;
pub mod assets;
mod brand;
mod dialogs;
mod graph_view;
mod inspector;
mod motion;
mod palette;
mod panels;
mod peek;
pub mod settings;
#[cfg(test)]
mod tests;
pub mod theme;
mod timeline;
mod ui;
pub mod workspace;

use std::borrow::Cow;

use gpui_kit::{
    App, AppContext as _, Bounds, Result, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};

use crate::workspace::Workspace;

const FONTS: [&[u8]; 6] = [
    include_bytes!("../../../assets/fonts/Geist-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Geist-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../../../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../../../assets/fonts/GeistMono-SemiBold.ttf"),
];

/// Matches the Linux .desktop file and icon names in packaging/linux.
pub const APP_ID: &str = "dev.videode.videode";

/// The window icon (used on X11; other platforms take it from the bundle).
fn window_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    let png = include_bytes!("../../../assets/brand/png/videode-256.png");
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()
        .map(|i| std::sync::Arc::new(i.into_rgba8()))
}

/// Registers bundled fonts, GPUI Kit components and the videode theme.
/// Must run before any window opens.
pub fn init(cx: &mut App) {
    let fonts = FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("videode: failed to load bundled fonts: {error:#}");
    }
    gpui_kit::init(cx);
    theme::apply(cx);
    settings::init(cx);
    actions::bind_keys(cx);
}

pub fn main_window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1440.), px(900.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("videode".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(1024.), px(640.))),
        app_id: Some(APP_ID.into()),
        icon: window_icon(),
        ..Default::default()
    }
}

pub fn open_main_window(cx: &mut App) -> Result<()> {
    gpui_kit::open_window(main_window_options(cx), cx, |window, cx| {
        cx.new(|cx| {
            let mut ws = Workspace::new(window, cx);
            ws.start_splash(cx);
            ws
        })
    })?;
    Ok(())
}
