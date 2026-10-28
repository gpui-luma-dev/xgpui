use super::*;

struct QuadView {
    fill: Paint,
    border: Paint,
    widths: Edges<Pixels>,
    radii: Corners<Pixels>,
    native: bool,
}
impl Render for QuadView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let fill = self.fill.clone();
        let border = self.border.clone();
        let widths = self.widths;
        let radii = self.radii;
        let native = self.native;
        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let mut shape = quad(
                    Bounds::new(point(px(10.), px(10.)), size(px(120.), px(80.))),
                    radii,
                    transparent_black(),
                    widths,
                    transparent_black(),
                    BorderStyle::Solid,
                );
                if native {
                    let Paint::Native(background) = fill else {
                        panic!("native reference fill")
                    };
                    let Paint::Native(border) = border else {
                        panic!("native reference border")
                    };
                    // Baseline: the extension's existing two native passes, without
                    // its new guards. A combined fill+border quad has different
                    // rounded-edge blending and is not a regression reference.
                    shape.background = background;
                    window.paint_quad(shape.clone());
                    shape.background = transparent_black().into();
                    shape.border_color = border.as_solid().expect("native reference solid border");
                    window.paint_quad(shape);
                } else {
                    window.paint_quad_with_paints(shape, fill, border);
                }
            },
        )
        .size_full()
    }
}

// Capture real Window recording with a test platform; no additional GPUI test API.
fn recorded(
    fill: Paint,
    border: Paint,
    widths: Edges<Pixels>,
    radii: Corners<Pixels>,
    native: bool,
    probe: Option<&Arc<SolidColor>>,
) -> (Vec<Quad>, f32, usize) {
    let mut cx = TestAppContext::single();
    let window = cx.open_window(size(px(160.), px(120.)), |_, _| QuadView {
        fill,
        border,
        widths,
        radii,
        native,
    });
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.update_window(window.into(), |_, window, _| {
        (
            window.painted_quads(),
            window.scale_factor(),
            probe.map_or(0, Arc::strong_count),
        )
    })
    .unwrap()
}
fn transparent_gradient() -> Paint {
    linear_gradient(
        45.,
        linear_color_stop(rgba(0xff000000), 0.),
        linear_color_stop(rgba(0x0000ff00), 1.),
    )
    .into()
}

#[::core::prelude::v1::test]
fn invisible_native_fills_and_borders_record_no_quads() {
    for radius in [0., 16.] {
        for fill in [Paint::from(transparent_black()), transparent_gradient()] {
            for border in [Paint::from(transparent_black()), transparent_gradient()] {
                for width in [0., 8.] {
                    let (quads, _, _) = recorded(
                        fill.clone(),
                        border.clone(),
                        Edges::all(px(width)),
                        Corners::all(px(radius)),
                        false,
                        None,
                    );
                    assert!(
                        quads.is_empty(),
                        "invisible paints: radius={radius}, width={width}"
                    );
                }
            }
        }
    }
}

#[::core::prelude::v1::test]
fn visible_fill_records_one_quad_without_an_invisible_border() {
    for border in [Paint::from(transparent_black()), transparent_gradient()] {
        for width in [0., 8.] {
            let (quads, _, _) = recorded(
                rgb(0x123456).into(),
                border.clone(),
                Edges::all(px(width)),
                Corners::all(px(16.)),
                false,
                None,
            );
            assert_eq!(quads.len(), 1, "width={width}");
            assert!(!quads[0].background.is_transparent());
        }
    }
}

#[::core::prelude::v1::test]
fn visible_border_does_not_acquire_transparent_fill_strips() {
    for fill in [Paint::from(transparent_black()), transparent_gradient()] {
        let (quads, _, _) = recorded(
            fill,
            white().into(),
            Edges::all(px(8.)),
            Corners::all(px(16.)),
            false,
            None,
        );
        assert_eq!(
            quads.len(),
            4,
            "only the visible border strips should remain"
        );
        assert!(
            quads
                .iter()
                .all(|q| q.background.is_transparent() && !q.border_color.is_transparent())
        );
    }
}

#[::core::prelude::v1::test]
fn zero_width_borders_skip_native_and_custom_sources_without_losing_fill() {
    let custom = Arc::new(SolidColor::new([0., 0., 0., 0.]).unwrap());
    for border in [
        Paint::from(white()),
        Paint::from(linear_gradient(
            45.,
            linear_color_stop(red(), 0.),
            linear_color_stop(blue(), 1.),
        )),
        custom.clone().into(),
    ] {
        let (quads, _, retained) = recorded(
            rgb(0x123456).into(),
            border,
            Edges::default(),
            Corners::all(px(16.)),
            false,
            Some(&custom),
        );
        assert_eq!(quads.len(), 1);
        // At most the test's Arc and the view's material; no retained custom scene record.
        assert!(retained <= 2, "zero-width border retained a producer");
    }
    for width in [0., 8.] {
        let (quads, _, retained) = recorded(
            custom.clone().into(),
            transparent_black().into(),
            Edges::all(px(width)),
            Corners::all(px(16.)),
            false,
            Some(&custom),
        );
        assert!(
            quads.is_empty(),
            "custom fill acquired invisible native border strips"
        );
        assert_eq!(
            retained, 3,
            "custom fill must remain recorded even with zero alpha"
        );
    }
    let (_, _, retained) = recorded(
        transparent_black().into(),
        custom.clone().into(),
        Edges::all(px(8.)),
        Corners::default(),
        false,
        Some(&custom),
    );
    assert_eq!(
        retained, 3,
        "custom border alpha is unknown; do not skip it"
    );
}

fn visible_border_pixels(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let widths = [
        Edges::all(px(8.)),
        Edges {
            top: px(3.),
            right: px(9.),
            bottom: px(5.),
            left: px(12.),
        },
        Edges {
            top: px(0.),
            right: px(0.2),
            bottom: px(0.),
            left: px(0.),
        },
    ];
    for radii in [Corners::default(), Corners::all(px(16.))] {
        for widths in widths {
            for fill in [Paint::from(transparent_black()), Paint::from(rgb(0x123456))] {
                for border in [white(), white().opacity(0.35)] {
                    let (actual, scale, _) =
                        recorded(fill.clone(), border.into(), widths, radii, false, None);
                    let (reference, _, _) =
                        recorded(fill.clone(), border.into(), widths, radii, true, None);
                    assert!(!actual.is_empty());
                    let scene = |quads: Vec<Quad>| {
                        let mut scene = Scene::default();
                        for quad in quads {
                            scene.insert_primitive(quad);
                        }
                        scene.finish();
                        scene
                    };
                    let extent = size(
                        DevicePixels((160. * scale) as i32),
                        DevicePixels((120. * scale) as i32),
                    );
                    let expected = renderer.render_scene_to_image(&scene(reference), extent)?;
                    let actual = renderer.render_scene_to_image(&scene(actual), extent)?;
                    // Straight edges, asymmetric widths, corners and a subpixel border.
                    for (a, b) in actual.pixels().zip(expected.pixels()) {
                        ensure!(
                            a.0.iter().zip(b.0).all(|(a, b)| a.abs_diff(b) <= 3),
                            "visible border pixel changed: {a:?} vs {b:?}; widths={widths:?}, radii={radii:?}"
                        );
                    }
                    ensure!(
                        expected.pixels().any(|p| p.0[..3].iter().any(|c| *c > 0)),
                        "empty reference image"
                    );
                }
            }
        }
    }
    Ok(())
}

#[::core::prelude::v1::test]
fn native_visible_border_pixels_match_unmodified_quad_path() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    visible_border_pixels(renderer.as_mut())
}
#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_visible_border_pixels_match_unmodified_quad_path() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    visible_border_pixels(&mut renderer)
}
