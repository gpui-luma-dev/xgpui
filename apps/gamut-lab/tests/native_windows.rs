//! Native platform windows on the process main thread, not a mocked platform.
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};
fn main() {
    let done = Rc::new(Cell::new(false));
    let check = done.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        println!("native checks: creating default windows");
        let a = cx
            .open_window(
                WindowOptions {
                    show: false,
                    ..Default::default()
                },
                |_, cx| cx.new(|_| EmptyView),
            )
            .unwrap();
        let b = cx
            .open_window(
                WindowOptions {
                    show: false,
                    ..Default::default()
                },
                |_, cx| cx.new(|_| EmptyView),
            )
            .unwrap();
        a.update(cx, |_, w, _| {
            assert_eq!(w.color_status().requested(), WindowColorMode::Default)
        })
        .unwrap();
        println!("native checks: enabling window B");
        b.update(cx, |_, w, _| {
            let status = w.set_color_mode(WindowColorMode::WideGamutSdr).unwrap();
            assert!(status.supports_wide_gamut_sdr());
            assert_eq!(status.effective(), WindowColorMode::WideGamutSdr);
        })
        .unwrap();
        a.update(cx, |_, w, _| {
            assert_eq!(w.color_status().requested(), WindowColorMode::Default);
            w.set_color_mode(WindowColorMode::WideGamutSdr).unwrap();
        })
        .unwrap();
        b.update(cx, |_, w, _| {
            w.set_color_mode(WindowColorMode::Default).unwrap()
        })
        .unwrap();
        a.update(cx, |_, w, _| {
            assert_eq!(w.color_status().effective(), WindowColorMode::WideGamutSdr)
        })
        .unwrap();
        b.update(cx, |_, w, _| {
            assert_eq!(w.color_status().effective(), WindowColorMode::Default)
        })
        .unwrap();
        // The old infallible background setter must preserve a valid wide window.
        a.update(cx, |_, w, _| {
            w.set_background_appearance(WindowBackgroundAppearance::Transparent);
            assert_eq!(w.color_status().effective(), WindowColorMode::WideGamutSdr);
            w.set_color_mode(WindowColorMode::Default).unwrap();
        })
        .unwrap();
        let transparent = cx
            .open_window(
                WindowOptions {
                    show: false,
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                |_, cx| cx.new(|_| EmptyView),
            )
            .unwrap();
        transparent
            .update(cx, |_, w, _| {
                assert!(w.set_color_mode(WindowColorMode::WideGamutSdr).is_err());
                assert_eq!(w.color_status().requested(), WindowColorMode::Default);
            })
            .unwrap();
        println!("native checks completed");
        check.set(true);
        cx.quit();
    });
    assert!(done.get(), "native window checks did not complete");
    println!("Native per-window presentation isolation and transparency checks passed");
}
