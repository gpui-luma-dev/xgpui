//! Shared native-window capture path; backend selection belongs to gpui-platform.
use gpui::{
    App, Window,
    content::{ContentError, ContentErrorKind},
};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
type Completion = Rc<RefCell<Option<anyhow::Result<()>>>>;
pub(super) fn capture(
    window: &mut Window,
    cx: &mut App,
    frames: usize,
    path: Option<PathBuf>,
    quit: bool,
    completion: Completion,
) {
    let handle = window.window_handle();
    let start = Instant::now();
    cx.spawn(async move |cx| {
        let result = async {
            let mut captured = 0;
            let mut image = None;
            while captured < frames {
                anyhow::ensure!(
                    start.elapsed() < Duration::from_secs(30),
                    "timed out awaiting requested portable content for capture"
                );
                let snapshot = handle.update(cx, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                    window.render_to_image()
                })?;
                match snapshot {
                    Ok(snapshot) => {
                        image = Some(snapshot);
                        captured += 1;
                    }
                    Err(error)
                        if error
                            .downcast_ref::<ContentError>()
                            .is_some_and(|e| e.kind == ContentErrorKind::Pending) =>
                    {
                        cx.background_executor()
                            .timer(Duration::from_millis(5))
                            .await;
                    }
                    Err(error) => return Err(error),
                }
            }
            let image = image.expect("validated positive capture count");
            anyhow::ensure!(
                image.width() >= 960 && image.height() >= 640,
                "incomplete lab viewport"
            );
            if let Some(path) = path {
                image.save(&path)?;
                println!("Clipped sRGB preview: {}", path.display());
            }
            println!(
                "{frames} ready UI frames in {:?} (encode + readback; not GPU timing)",
                start.elapsed()
            );
            Ok(())
        }
        .await;
        if let Err(error) = &result {
            eprintln!("UI capture failed: {error:#}");
        }
        if quit && result.is_err() {
            std::process::exit(1);
        }
        *completion.borrow_mut() = Some(result);
        if quit {
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
}
