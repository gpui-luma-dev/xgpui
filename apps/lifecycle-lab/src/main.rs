use gpui::App;
use xgpui_lifecycle::{configure, open_window};

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let session = configure(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        if let Err(error) = open_window(&session, cx) {
            eprintln!("Could not open the lifecycle exercise: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
