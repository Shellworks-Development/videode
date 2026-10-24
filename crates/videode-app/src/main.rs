fn main() {
    gpui_kit::application()
        .with_assets(videode_app::assets::AppAssets)
        .run(|cx| {
            videode_app::init(cx);
            videode_app::open_main_window(cx).expect("failed to open the videode window");
            cx.activate(true);
        });
}
