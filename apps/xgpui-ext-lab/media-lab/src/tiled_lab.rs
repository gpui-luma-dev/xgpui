use anyhow::Result;
use gpui::*;
pub fn run() -> Result<()> {
    gpui_platform::application().run(|cx: &mut App| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1360.), px(900.)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(if cfg!(debug_assertions) {
                    "XGPUI Tiled Canvas — DEBUG"
                } else {
                    "XGPUI Tiled Canvas — RELEASE"
                });
                cx.new(|_| crate::tiled_demo::TiledDemo::new())
            },
        )
        .expect("open tiled canvas");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit()
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}
