//! Shared native-window capture path; backend selection belongs to gpui-platform.
use gpui::{App, Window};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};

type Completion = Rc<RefCell<Option<anyhow::Result<()>>>>;

pub(super) fn capture(
    window: &mut Window,
    cx: &mut App,
    frames: usize,
    path: Option<PathBuf>,
    quit: bool,
    completion: Completion,
) {
    let start = Instant::now();
    let result = (|| -> anyhow::Result<()> {
        // Render the real controls/overlays and surfaces through the window backend.
        // This measures CPU encode + readback, not GPU execution time.
        window.draw(cx).clear(cx);
        let mut image = window.render_to_image()?;
        for _ in 1..frames {
            window.refresh();
            window.draw(cx).clear(cx);
            image = window.render_to_image()?;
        }
        anyhow::ensure!(
            image.width() >= 960 && image.height() >= 640,
            "incomplete lab viewport"
        );
        if let Some(path) = path {
            image.save(&path)?;
            println!("Clipped sRGB preview: {}", path.display());
        }
        println!(
            "{frames} UI frames in {:?} (encode + readback; not GPU timing)",
            start.elapsed()
        );
        Ok(())
    })();
    if let Err(error) = &result {
        eprintln!("UI capture failed: {error:#}");
    }
    // Some platform event loops terminate the process inside quit(), so preserve
    // a failing command exit status before invoking it.
    if quit && result.is_err() {
        std::process::exit(1);
    }
    *completion.borrow_mut() = Some(result);
    if quit {
        cx.quit();
    }
}
