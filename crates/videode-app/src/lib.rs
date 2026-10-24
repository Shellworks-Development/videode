//! The videode desktop application: window, theme and the editor shell.

pub mod assets;
pub mod theme;
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

/// Registers bundled fonts, GPUI Kit components and the videode theme.
/// Must run before any window opens.
pub fn init(cx: &mut App) {
    let fonts = FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("videode: failed to load bundled fonts: {error:#}");
    }
    gpui_kit::init(cx);
    theme::apply(cx);
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
        ..Default::default()
    }
}

pub fn open_main_window(cx: &mut App) -> Result<()> {
    gpui_kit::open_window(main_window_options(cx), cx, |window, cx| {
        cx.new(|cx| Workspace::new(window, cx))
    })?;
    Ok(())
}
