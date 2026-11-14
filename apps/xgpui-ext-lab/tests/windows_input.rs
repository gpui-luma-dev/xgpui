//! Real Win32 window probe for #284 package 09: synthetic `WM_MOUSEMOVE`
//! suppression on a live window, with the real cursor over it.
//!
//! Windows re-posts `WM_MOUSEMOVE` at the cursor's position when a window
//! repaints under a stationary cursor. The app must not see those, but must
//! see real moves and moves with a button held.
#[cfg(target_os = "windows")]
fn main() {
    use gpui::{prelude::*, *};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{cell::RefCell, rc::Rc, time::Duration};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{GetCursorPos, PostMessageW, SetCursorPos, WM_MOUSEMOVE},
    };

    const MK_LBUTTON: usize = 0x0001;
    type Moves = Rc<RefCell<Vec<(Point<Pixels>, bool)>>>;

    struct Probe {
        moves: Moves,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let moves = self.moves.clone();
            div()
                .id("probe")
                .size_full()
                .bg(rgb(0x224466))
                .on_mouse_move(move |event: &MouseMoveEvent, _, _| {
                    moves
                        .borrow_mut()
                        .push((event.position, event.pressed_button.is_some()));
                })
        }
    }

    fn lparam(x: i32, y: i32) -> LPARAM {
        LPARAM(((y as u16 as isize) << 16) | (x as u16 as isize))
    }

    let moves: Moves = Rc::default();
    gpui_platform::application().run({
        let moves = moves.clone();
        move |cx| {
            let probe_moves = moves.clone();
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(100.), px(100.)),
                            size(px(480.), px(320.)),
                        ))),
                        ..Default::default()
                    },
                    move |_, cx| cx.new(|_| Probe { moves: probe_moves }),
                )
                .expect("open real Windows window");
            cx.activate(true);
            cx.spawn(async move |cx| {
                let executor = cx.background_executor().clone();
                let timer = move |ms| executor.timer(Duration::from_millis(ms));
                timer(500).await;
                let (hwnd, scale) = window
                    .update(cx, |_, window, _| {
                        let RawWindowHandle::Win32(handle) =
                            HasWindowHandle::window_handle(window)
                                .expect("window handle")
                                .as_raw()
                        else {
                            panic!("expected a Win32 window handle");
                        };
                        (HWND(handle.hwnd.get() as *mut _), window.scale_factor())
                    })
                    .expect("read window handle");
                let check = (|| -> Result<String, String> {
                    // Client point P, in physical pixels.
                    let (px_, py_) = ((160.0 * scale) as i32, (120.0 * scale) as i32);
                    let to_screen = |x: i32, y: i32| {
                        let mut p = POINT { x, y };
                        unsafe { ClientToScreen(hwnd, &mut p).ok() }.map_err(|e| e.to_string())?;
                        Ok::<POINT, String>(p)
                    };
                    let screen = to_screen(px_, py_)?;
                    unsafe { SetCursorPos(screen.x, screen.y) }
                        .map_err(|e| format!("SetCursorPos failed (no interactive desktop?): {e}"))?;
                    let mut actual = POINT::default();
                    unsafe { GetCursorPos(&mut actual) }.map_err(|e| e.to_string())?;
                    if (actual.x, actual.y) != (screen.x, screen.y) {
                        return Err(format!(
                            "cursor did not move onto the window: wanted {:?}, at {:?}",
                            (screen.x, screen.y),
                            (actual.x, actual.y)
                        ));
                    }
                    Ok(format!("{px_},{py_}"))
                })();
                let placed = match check {
                    Ok(placed) => placed,
                    Err(error) => report(Err(error)),
                };
                let (x, y): (i32, i32) = {
                    let mut parts = placed.split(',').map(|v| v.parse().unwrap());
                    (parts.next().unwrap(), parts.next().unwrap())
                };
                // 1. The real cursor arrived: Windows delivers one real move at P.
                timer(300).await;
                let post = |x: i32, y: i32, buttons: usize| unsafe {
                    PostMessageW(Some(hwnd), WM_MOUSEMOVE, WPARAM(buttons), lparam(x, y))
                        .expect("post WM_MOUSEMOVE");
                };
                // 2. Synthetic repeats at P with no button: must be dropped.
                post(x, y, 0);
                post(x, y, 0);
                timer(200).await;
                // 3. Same position with the left button held (a drag): must arrive.
                post(x, y, MK_LBUTTON);
                timer(200).await;
                // 4. A real move to P + 10: must arrive.
                let screen = {
                    let mut p = POINT { x: x + 10, y };
                    unsafe { ClientToScreen(hwnd, &mut p).ok() }.expect("client to screen");
                    p
                };
                unsafe { SetCursorPos(screen.x, screen.y) }.expect("move cursor");
                timer(300).await;

                let at = |p: Point<Pixels>, q: Point<Pixels>| {
                    (p.x - q.x).abs() < px(0.75) && (p.y - q.y).abs() < px(0.75)
                };
                let p = point(px(x as f32 / scale), px(y as f32 / scale));
                let p2 = point(px((x + 10) as f32 / scale), px(y as f32 / scale));
                let recorded = moves.borrow().clone();
                let plain_at_p = recorded.iter().filter(|(q, b)| !b && at(*q, p)).count();
                let pressed_at_p = recorded.iter().filter(|(q, b)| *b && at(*q, p)).count();
                let at_p2 = recorded.iter().filter(|(q, _)| at(*q, p2)).count();
                let verdict = if plain_at_p != 1 {
                    Err(format!(
                        "expected exactly one plain move at P (real arrival; synthetic repeats dropped), got {plain_at_p}: {recorded:?}"
                    ))
                } else if pressed_at_p != 1 {
                    Err(format!(
                        "expected the held-button move at P to arrive once, got {pressed_at_p}: {recorded:?}"
                    ))
                } else if at_p2 == 0 {
                    Err(format!("expected the real move to P+10 to arrive: {recorded:?}"))
                } else {
                    Ok(format!(
                        "scale {scale}: real arrival delivered, 2 synthetic repeats dropped, drag at same position delivered, real move delivered"
                    ))
                };
                report(verdict);
            })
            .detach();
        }
    });
    panic!("Windows probe ended without a verdict");
}

/// Print the verdict and exit from inside the app, so the result can't be lost
/// if the platform ends the process when the app quits.
#[cfg(target_os = "windows")]
fn report(result: Result<String, String>) -> ! {
    match result {
        Ok(summary) => {
            println!("Windows synthetic mouse-move probe passed: {summary}");
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("Windows synthetic mouse-move probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("windows_input requires Windows");
}
