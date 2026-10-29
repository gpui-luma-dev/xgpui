use super::*;

impl Lab {
    pub(super) fn refresh_overview(&mut self) {
        self.overview = Arc::new(xgpui_canvas::Overview::new(&self.scene));
    }

    pub(super) fn navigate_map(&mut self, position: Point<Pixels>) {
        let local = position - self.map_bounds.origin;
        let size = self.map_bounds.size;
        if let Some(world) = self.overview.map_to_world(
            WorldPoint::new(f32::from(local.x) as f64, f32::from(local.y) as f64),
            WorldPoint::new(f32::from(size.width) as f64, f32::from(size.height) as f64),
        ) {
            self.camera.center_on(world);
        }
    }

    pub(super) fn minimap(&self, cx: &mut Context<Self>) -> AnyElement {
        let overview = self.overview.clone();
        let visible = self.camera.visible_world_bounds(self.viewport());
        let entity = cx.entity();
        div()
            .id("minimap")
            .absolute()
            .right(px(12.))
            .bottom(px(12.))
            .w(px(230.))
            .h(px(170.))
            .bg(rgb(0x172238))
            .border_1()
            .border_color(rgb(0x6484a8))
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.cancel_selection();
                    this.drag = None;
                    if this.map_bounds.contains(&event.position) {
                        this.map_drag = true;
                        this.navigate_map(event.position);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.map_drag && event.pressed_button == Some(MouseButton::Left) {
                    this.navigate_map(event.position);
                    cx.notify();
                } else if this.gesture.is_some() && event.pressed_button == Some(MouseButton::Left)
                {
                    this.update_selection(event.position, event.modifiers.alt);
                    cx.notify();
                } else if event.pressed_button != Some(MouseButton::Left) {
                    this.map_drag = false;
                }
                cx.stop_propagation();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    if this.map_drag {
                        this.navigate_map(event.position);
                        this.map_drag = false;
                        cx.notify();
                    } else if this.gesture.is_some() {
                        this.finish_selection(event);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    if this.map_drag {
                        this.navigate_map(event.position);
                        this.map_drag = false;
                        cx.notify();
                        cx.stop_propagation();
                    }
                }),
            )
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_pinch(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(px(6.))
                    .text_size(px(12.))
                    .child("Minimap • click/drag • M to hide"),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |this, cx| {
                            if this.map_bounds != bounds {
                                this.map_bounds = bounds;
                                cx.notify();
                            }
                        });
                    },
                    move |bounds, _, window, _| {
                        paint(window, bounds, &overview, visible);
                    },
                )
                .absolute()
                .left(px(6.))
                .right(px(6.))
                .top(px(24.))
                .bottom(px(6.)),
            )
            .into_any_element()
    }
}

fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    overview: &xgpui_canvas::Overview,
    visible: WorldBounds,
) {
    let size = WorldPoint::new(
        f32::from(bounds.size.width) as f64,
        f32::from(bounds.size.height) as f64,
    );
    let screen = |world| {
        let p = overview.world_to_map(world, size);
        bounds.origin + point(px(p.x as f32), px(p.y as f32))
    };
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        window.paint_quad(fill(bounds, rgb(0x101824)));
        let max = overview.cells().map(|cell| cell.count).max().unwrap_or(1) as f32;
        for cell in overview.cells() {
            let a = screen(cell.bounds.min);
            let b = screen(cell.bounds.max);
            let intensity = (cell.count as f32).ln_1p() / max.ln_1p();
            // Match canvas/card values rather than showing a bright inverse heatmap.
            // Opaque cells also avoid bright seams where rounded cell edges overlap.
            let color = (((27. + 9. * intensity) as u32) << 16)
                | (((42. + 11. * intensity) as u32) << 8)
                | (60. + 15. * intensity) as u32;
            window.paint_quad(fill(
                Bounds::new(
                    a,
                    gpui::size((b.x - a.x).max(px(0.5)), (b.y - a.y).max(px(0.5))),
                ),
                rgb(color),
            ));
        }
        let a = screen(visible.min);
        let b = screen(visible.max);
        // Keep tiny high-zoom viewports visible without changing navigation math.
        let width = (b.x - a.x).max(px(2.));
        let height = (b.y - a.y).max(px(2.));
        let rect = Bounds::new(a, gpui::size(width, height));
        window.paint_quad(fill(rect, rgba(0xffd47922)));
        for (origin, size) in [
            (a, gpui::size(width, px(1.))),
            (a, gpui::size(px(1.), height)),
            (
                a + point(px(0.), height - px(1.)),
                gpui::size(width, px(1.)),
            ),
            (
                a + point(width - px(1.), px(0.)),
                gpui::size(px(1.), height),
            ),
        ] {
            window.paint_quad(fill(Bounds::new(origin, size), rgb(0xffd479)));
        }
    });
}

pub(super) fn smoke_navigation(entity: &Entity<Lab>, window: &mut Window, cx: &mut App) {
    let (a, b, selected, zoom, bounds) = {
        let lab = entity.read(cx);
        let a = lab.map_bounds.origin
            + point(
                lab.map_bounds.size.width * 0.75,
                lab.map_bounds.size.height * 0.65,
            );
        let b = lab.map_bounds.origin
            + point(
                lab.map_bounds.size.width * 0.25,
                lab.map_bounds.size.height * 0.25,
            );
        (
            a,
            b,
            lab.selected.clone(),
            lab.camera.zoom(),
            lab.scene.bounds().unwrap(),
        )
    };
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: a,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    assert!(entity.read(cx).map_drag, "minimap owns pointer gesture");
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: b,
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        }),
        cx,
    );
    {
        let lab = entity.read(cx);
        let local = b - lab.map_bounds.origin;
        let size = lab.map_bounds.size;
        let expected = lab
            .overview
            .map_to_world(
                WorldPoint::new(f32::from(local.x) as f64, f32::from(local.y) as f64),
                WorldPoint::new(f32::from(size.width) as f64, f32::from(size.height) as f64),
            )
            .unwrap();
        let viewport = lab.viewport();
        let center = lab
            .camera
            .screen_to_world(WorldPoint::new(viewport.x / 2., viewport.y / 2.), viewport);
        assert!(
            (center.x - expected.x).abs() < 1e-5 && (center.y - expected.y).abs() < 1e-5,
            "minimap recentering"
        );
        assert_eq!(lab.camera.zoom(), zoom);
        assert_eq!(
            lab.selected, selected,
            "minimap must not select world content"
        );
        assert_eq!(lab.scene.bounds().unwrap().min, bounds.min);
        assert!(lab.gesture.is_none());
    }
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: b,
            button: MouseButton::Left,
            ..Default::default()
        }),
        cx,
    );
    assert!(
        !entity.read(cx).map_drag,
        "minimap release clears drag state"
    );
}
