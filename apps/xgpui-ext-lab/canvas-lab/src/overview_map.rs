use super::*;

impl Lab {
    pub(super) fn refresh_overview(&mut self) {
        self.overview = Arc::new(xgpui_canvas::Overview::new(&self.scene));
        self.minimap_image = thumbnail(&self.scene, &self.overview);
        self.overview_key = self.scene.cache_key();
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

    pub(super) fn overview_map(&self, cx: &mut Context<Self>) -> AnyElement {
        let overview = self.overview.clone();
        let thumbnail = self.map_thumbnail.then(|| self.minimap_image.clone());
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
                    this.drag.end();
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
                    .child("Whole canvas • click/drag • T: view"),
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
                        paint(window, bounds, &overview, thumbnail.clone(), visible);
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

/// Rasterize actual card bounds in paint order; no grid, density bins, or labels.
/// The 2x image is retained and rebuilt only when scene geometry changes.
pub(super) fn thumbnail(
    scene: &Scene<usize>,
    overview: &xgpui_canvas::Overview,
) -> Arc<RenderImage> {
    const WIDTH: u32 = 432;
    const HEIGHT: u32 = 276;
    let size = WorldPoint::new(WIDTH as f64, HEIGHT as f64);
    let mut pixels = image::RgbaImage::from_pixel(WIDTH, HEIGHT, image::Rgba([16, 24, 36, 255]));
    for item in scene.items() {
        let a = overview.world_to_map(item.bounds.min, size);
        let b = overview.world_to_map(item.bounds.max, size);
        let left = a.x.max(0.).min(WIDTH as f64);
        let top = a.y.max(0.).min(HEIGHT as f64);
        let right = b.x.max(0.).min(WIDTH as f64);
        let bottom = b.y.max(0.).min(HEIGHT as f64);
        for y in top.floor() as u32..bottom.ceil() as u32 {
            for x in left.floor() as u32..right.ceil() as u32 {
                let coverage = ((right.min(x as f64 + 1.) - left.max(x as f64)).max(0.)
                    * (bottom.min(y as f64 + 1.) - top.max(y as f64)).max(0.))
                    as f32;
                let pixel = pixels.get_pixel_mut(x, y);
                for (channel, color) in [36., 53., 75.].into_iter().enumerate() {
                    pixel[channel] =
                        (pixel[channel] as f32 * (1. - coverage) + color * coverage).round() as u8;
                }
            }
        }
    }
    // GPUI RenderImage expects BGRA channel order.
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
}

fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    overview: &xgpui_canvas::Overview,
    thumbnail: Option<Arc<RenderImage>>,
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
        if let Some(thumbnail) = thumbnail {
            if let Err(error) =
                window.paint_image(bounds, bounds, Corners::default(), thumbnail, 0, false)
            {
                eprintln!("minimap thumbnail: {error}");
            }
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

/// Native event coverage for cache refresh and HUD ownership transitions.
pub(super) fn smoke_invalidation_and_modes(
    entity: &Entity<Lab>,
    window: &mut Window,
    cx: &mut App,
) {
    entity.update(cx, |lab, _| {
        lab.cancel_interaction();
        lab.show_minimap = true;
        lab.whole_scene_map = true;
        let original = lab.scene.items()[0].bounds;
        lab.gesture = Some(Gesture::Move {
            anchor: original.min,
            originals: vec![(0, original)],
            group: original,
        });
        lab.scene
            .set_bounds(0, original.translated(WorldPoint::new(-500., -500.)));
    });
    window.draw(cx).clear(cx);
    assert_eq!(
        entity.read(cx).overview_key,
        entity.read(cx).scene.cache_key(),
        "map refreshes during geometry edits"
    );
    assert_eq!(
        entity.read(cx).overview.domain().min,
        entity.read(cx).scene.bounds().unwrap().min
    );
    let image = entity.read(cx).minimap_image.clone();
    window.draw(cx).clear(cx);
    assert!(
        Arc::ptr_eq(&image, &entity.read(cx).minimap_image),
        "unchanged redraw retains the thumbnail"
    );
    entity.update(cx, |lab, _| lab.cancel_interaction());
    window.draw(cx).clear(cx);
    assert_eq!(
        entity.read(cx).overview.domain().min,
        entity.read(cx).scene.bounds().unwrap().min,
        "cancellation restores minimap geometry"
    );
    assert!(!Arc::ptr_eq(&image, &entity.read(cx).minimap_image));
    let prior = entity.read(cx).overview_key;
    entity.update(cx, |lab, _| {
        lab.scene = make_scene(10_000);
    });
    window.draw(cx).clear(cx);
    assert_ne!(
        entity.read(cx).overview_key,
        prior,
        "replacement scene invalidates revision-zero cache"
    );
    for key in ["m", "m", "t", "t", "r", "o", "f", "f", "escape"] {
        entity.update(cx, |lab, _| {
            lab.map_drag = true;
            lab.drag.begin(WorldPoint::default());
        });
        window.dispatch_event(
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap(),
                is_held: false,
                prefer_character_input: false,
            }),
            cx,
        );
        let lab = entity.read(cx);
        assert!(
            !lab.map_drag && !lab.drag.is_active() && lab.gesture.is_none(),
            "mode change cancels pointer ownership: {key}"
        );
        window.draw(cx).clear(cx);
    }
    entity.update(cx, |lab, _| {
        lab.map_drag = true;
        lab.space_down = true;
    });
    window.blur(cx);
    window.draw(cx).clear(cx);
    assert!(
        !entity.read(cx).map_drag && !entity.read(cx).space_down,
        "focus loss clears map and modifier state"
    );
    entity.update(cx, |lab, cx| {
        lab.focus.focus(window, cx);
    });
}
