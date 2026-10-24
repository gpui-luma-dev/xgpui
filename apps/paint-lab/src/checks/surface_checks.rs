use super::*;

struct SurfaceView {
    background: Arc<SolidColor>,
    border: Arc<SolidColor>,
    children: bool,
    opacity: f32,
}
impl Render for SurfaceView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut surface = painted()
            .w(px(120.))
            .h(px(80.))
            .rounded(px(16.))
            .border_8()
            .border_paint(self.border.clone())
            .bg_paint(self.background.clone())
            .opacity(self.opacity)
            .overflow_hidden();
        if self.children {
            surface = surface.child(
                canvas(
                    |_, _, _| (),
                    |bounds, (), window, _| {
                        // Deliberately paint outside the child's layout rectangle: the
                        // extension must clip content inside the visible border ring.
                        window.paint_quad(fill(
                            Bounds::new(point(px(0.), px(0.)), size(px(160.), px(120.))),
                            rgb(0x00ff00),
                        ));
                        assert!(bounds.size.width > px(0.));
                    },
                )
                .size_full(),
            );
        }
        div().size_full().bg(black()).child(surface)
    }
}
#[::core::prelude::v1::test]
fn extension_container_keeps_native_layout_clipping_and_opacity() {
    let mut cx = TestAppContext::single();
    let background = Arc::new(SolidColor::new([1., 0., 0., 0.5]).unwrap());
    let border = Arc::new(SolidColor::new([0., 0., 1., 1.]).unwrap());
    let window = cx.open_window(size(px(160.), px(120.)), |_, _| SurfaceView {
        background: background.clone(),
        border: border.clone(),
        children: true,
        opacity: 0.5,
    });
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.update_window(window.into(), |_, window, _| {
        let quads = window.painted_quads();
        let child = quads
            .iter()
            .find(|q| {
                q.background
                    .as_solid()
                    .is_some_and(|c| (c.h - 1. / 3.).abs() < 0.01 && c.s > 0.9)
            })
            .expect("child painted");
        assert_eq!(
            child.content_mask.bounds,
            Bounds::new(point(px(8.), px(8.)), size(px(104.), px(64.)))
                .scale(window.scale_factor()),
            "content must clip inside the visible border"
        );
        assert_eq!(
            child.background.as_solid().unwrap().a,
            0.5,
            "local opacity must apply once"
        );
    })
    .unwrap();
    // Scene retention is the public producer lifetime contract. Both decoration
    // callbacks must retain more than the test and view owners, rather than losing
    // zero-size canvases.
    assert!(Arc::strong_count(&background) > 2);
    assert!(Arc::strong_count(&border) > 2);
}

#[::core::prelude::v1::test]
fn native_style_api_and_serialization_have_no_extension_paint_fields() -> Result<()> {
    // Exhaustive matching intentionally fails to compile if custom paint enters Fill again.
    let Fill::Color(color) = Fill::from(white());
    ensure!(!color.is_transparent(), "native fill conversion changed");
    let style = StyleRefinement::default().border_color(white());
    ensure!(
        serde_json::to_value(style)?.get("border_paint").is_none(),
        "native style gained an extension field"
    );
    Ok(())
}
