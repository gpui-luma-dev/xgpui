use super::{AxisLab, Controls, SLIDER_IDS, SPECIMEN};
use crate::samples;
use gpui::{
    Entity, Modifiers, MouseButton, PlatformTextSystem, TestAppContext, TestDispatcher,
    VisualTestContext, div, point, prelude::*, px, size,
};
use std::{borrow::Cow, sync::Arc};

fn parley() -> Arc<dyn PlatformTextSystem> {
    let mut fonts = parley::FontContext::new();
    for bytes in [samples::PLEX, samples::FLEX] {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(bytes.to_vec())), None);
    }
    Arc::new(xgpui_parley::ParleyTextSystem::new(fonts, "IBM Plex Sans").unwrap())
}

fn draw(visual: &mut VisualTestContext, view: &Entity<AxisLab>) {
    visual.draw(
        point(px(100.0), px(100.0)),
        size(px(620.0), px(640.0)),
        |_, _| {
            div()
                .w(px(620.0))
                .font_family("IBM Plex Sans")
                .text_size(px(13.0))
                .child(view.clone())
        },
    );
}

fn interaction_checks(backend: Arc<dyn PlatformTextSystem>) {
    backend
        .add_fonts(vec![
            Cow::Borrowed(samples::PLEX),
            Cow::Borrowed(samples::FLEX),
        ])
        .unwrap();
    let controls = Controls::for_backend(&*backend)
        .unwrap()
        .expect("axis support required");
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
    let window = cx.open_window(size(px(620.0), px(640.0)), |_, cx| {
        AxisLab::new("Test".into(), controls, cx)
    });
    let view = window.root(&mut cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    draw(&mut visual, &view);
    let width_before = visual.update(|window, cx| {
        let font = view.read(cx).controls.font.clone();
        window
            .text_system()
            .layout_line(
                SPECIMEN,
                px(32.0),
                &[gpui::TextRun {
                    len: SPECIMEN.len(),
                    font,
                    ..Default::default()
                }],
                None,
            )
            .width
    });
    // Every slider reaches its upper endpoint through real pointer events.
    for (index, id) in SLIDER_IDS.iter().enumerate() {
        let bounds = visual.debug_bounds(id).unwrap();
        let right = point(bounds.right() - px(8.0), bounds.center().y);
        visual.simulate_click(right, Modifiers::default());
        visual.update(|_, cx| {
            let state = view.read(cx);
            assert_eq!(
                state.controls.values[index],
                state.controls.axes[index].info.max_value
            );
            assert!(state.controls.error.is_none());
        });
        draw(&mut visual, &view);
    }
    // Keyboard changes use the same admission/rendering path.
    visual.simulate_keystrokes("home right");
    visual.update(|_, cx| assert_eq!(view.read(cx).controls.values[3], 9.0));
    draw(&mut visual, &view);
    // Drag outside the track, then release outside the panel. A later move must
    // not continue changing the slider, and thumb/value must remain in range.
    let bounds = visual.debug_bounds("axis-width").unwrap();
    visual.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(
        point(bounds.right() + px(30.0), px(620.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.simulate_mouse_up(
        point(bounds.right() + px(30.0), px(620.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.update(|_, cx| {
        let state = view.read(cx);
        assert_eq!(
            state.controls.values[1],
            state.controls.axes[1].info.max_value
        );
        assert!(state.dragging.is_none());
    });
    visual.simulate_mouse_move(
        point(bounds.left(), px(620.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.update(|_, cx| {
        assert_eq!(
            view.read(cx).controls.values[1],
            view.read(cx).controls.axes[1].info.max_value
        )
    });
    // Presets are identical between backends and reset revisits an admitted state.
    draw(&mut visual, &view);
    let reset = visual.debug_bounds("axis-default").unwrap();
    visual.simulate_click(reset.center(), Modifiers::default());
    draw(&mut visual, &view);
    visual.update(|window, cx| {
        let state = view.read(cx);
        assert_eq!(state.controls.values, vec![400.0, 100.0, 0.0, 14.0]);
        let font = state.controls.font.clone();
        let width = window
            .text_system()
            .layout_line(
                SPECIMEN,
                px(32.0),
                &[gpui::TextRun {
                    len: SPECIMEN.len(),
                    font,
                    ..Default::default()
                }],
                None,
            )
            .width;
        assert_eq!(width, width_before);
    });
    let wide = visual.debug_bounds("axis-wide").unwrap();
    visual.simulate_click(wide.center(), Modifiers::default());
    draw(&mut visual, &view);
    visual.update(|window, cx| {
        let state = view.read(cx);
        assert_eq!(state.controls.values, vec![700.0, 150.0, -5.0, 72.0]);
        let font = state.controls.font.clone();
        let width = window
            .text_system()
            .layout_line(
                SPECIMEN,
                px(32.0),
                &[gpui::TextRun {
                    len: SPECIMEN.len(),
                    font,
                    ..Default::default()
                }],
                None,
            )
            .width;
        assert!(
            width > width_before,
            "changing width must affect actual backend layout"
        );
    });
}

#[test]
fn live_axis_controls_on_parley() {
    interaction_checks(parley());
}

#[cfg(target_os = "macos")]
#[test]
fn live_axis_controls_on_native() {
    interaction_checks(Arc::new(gpui_apple::AppleTextSystem::new()));
}

#[cfg(target_os = "windows")]
#[test]
fn live_axis_controls_on_native_windows() {
    // A headless Windows platform deliberately supplies NoopTextSystem. Keep a
    // window-capable platform alive for COM and native DirectWrite resources;
    // interaction_checks itself still draws into GPUI's simulated test window.
    let platform = gpui_platform::current_platform(false);
    interaction_checks(platform.text_system());
}

#[test]
fn preview_limit_holds_previous_font_and_reset_still_works() {
    let backend = parley();
    let controls = Controls::for_backend(&*backend).unwrap().unwrap();
    let metadata = controls.axes.iter().map(|axis| axis.info.clone()).collect();
    let mut controls = Controls::new(metadata, 2).unwrap();
    controls.set(0, 700.0);
    let previous = controls.font.clone();
    let values = controls.values.clone();
    controls.set(1, 50.0);
    assert!(controls.error.is_some());
    assert_eq!(controls.font, previous);
    assert_eq!(controls.values, values);
    assert_eq!(controls.preview.admitted_instances(), 2);
    controls.preset(0);
    assert_eq!(controls.values, vec![400.0, 100.0, 0.0, 14.0]);
    assert!(controls.error.is_none());
}
