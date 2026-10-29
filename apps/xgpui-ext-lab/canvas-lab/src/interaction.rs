use super::*;
use xgpui_canvas::{AlignmentGuide, Axis};

pub(super) enum Gesture {
    Marquee {
        start: WorldPoint,
        current: WorldPoint,
        prior: BTreeSet<usize>,
    },
    Move {
        anchor: WorldPoint,
        originals: Vec<(usize, WorldBounds)>,
        group: WorldBounds,
    },
}

impl Lab {
    pub(super) fn world_at(&self, position: Point<Pixels>) -> WorldPoint {
        let local = position - self.bounds.origin;
        self.camera.screen_to_world(
            WorldPoint::new(f32::from(local.x) as f64, f32::from(local.y) as f64),
            self.viewport(),
        )
    }

    pub(super) fn start_selection(&mut self, event: &MouseDownEvent) {
        self.cancel_selection();
        let world = self.world_at(event.position);
        if event.modifiers.shift {
            let prior = std::mem::take(&mut self.selected);
            self.gesture = Some(Gesture::Marquee {
                start: world,
                current: world,
                prior,
            });
        } else if let Some(hit) = self.scene.hit_test(world).map(|item| item.data) {
            if !self.selected.contains(&hit) {
                self.selected.clear();
                self.selected.insert(hit);
            }
            let originals: Vec<_> = self
                .selected
                .iter()
                .map(|&index| (index, self.scene.items()[index].bounds))
                .collect();
            let group = originals.iter().map(|(_, b)| *b).reduce(union).unwrap();
            self.gesture = Some(Gesture::Move {
                anchor: world,
                originals,
                group,
            });
        } else {
            self.selected.clear();
        }
    }

    pub(super) fn update_selection(&mut self, position: Point<Pixels>, disable_snap: bool) {
        let world = self.world_at(position);
        match &mut self.gesture {
            Some(Gesture::Marquee { start, current, .. }) => {
                *current = world;
                self.selected = self
                    .scene
                    .query(rectangle(*start, world))
                    .map(|item| item.data)
                    .collect();
            }
            Some(Gesture::Move {
                anchor,
                originals,
                group,
            }) => {
                let mut delta = WorldPoint::new(world.x - anchor.x, world.y - anchor.y);
                self.snap_guides.clear();
                if !disable_snap {
                    let snap = self.scene.snap_translation(
                        *group,
                        delta,
                        6. / self.camera.zoom(),
                        200. / self.camera.zoom(),
                        |id| self.selected.contains(id),
                    );
                    delta = snap.delta;
                    self.snap_guides.extend(snap.x.into_iter().chain(snap.y));
                }
                for &(index, bounds) in originals.iter() {
                    assert!(self.scene.set_bounds(index, bounds.translated(delta)));
                }
            }
            None => (),
        }
    }

    pub(super) fn finish_selection(&mut self, event: &MouseUpEvent) {
        self.update_selection(event.position, event.modifiers.alt);
        let changed = match &self.gesture {
            Some(Gesture::Move { originals, .. }) => originals.iter().any(|(id, old)| {
                let current = self.scene.items()[*id].bounds;
                current.min != old.min || current.max != old.max
            }),
            _ => false,
        };
        if changed {
            self.refresh_overview();
        }
        self.gesture = None;
        self.snap_guides.clear();
        self.drag = None;
    }

    pub(super) fn cancel_selection(&mut self) {
        match self.gesture.take() {
            Some(Gesture::Move { originals, .. }) => {
                for (index, bounds) in originals {
                    assert!(self.scene.set_bounds(index, bounds));
                }
            }
            Some(Gesture::Marquee { prior, .. }) => self.selected = prior,
            None => (),
        }
        self.snap_guides.clear();
    }

    pub(super) fn marquee(&self) -> Option<WorldBounds> {
        match self.gesture {
            Some(Gesture::Marquee { start, current, .. }) => Some(rectangle(start, current)),
            _ => None,
        }
    }
}

fn rectangle(a: WorldPoint, b: WorldPoint) -> WorldBounds {
    WorldBounds {
        min: WorldPoint::new(a.x.min(b.x), a.y.min(b.y)),
        max: WorldPoint::new(a.x.max(b.x), a.y.max(b.y)),
    }
}

fn union(a: WorldBounds, b: WorldBounds) -> WorldBounds {
    WorldBounds {
        min: WorldPoint::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
        max: WorldPoint::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
    }
}

pub(super) fn paint_overlay(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    camera: Camera2D,
    marquee: Option<WorldBounds>,
    guides: &[AlignmentGuide],
) {
    let viewport = WorldPoint::new(
        f32::from(bounds.size.width) as f64,
        f32::from(bounds.size.height) as f64,
    );
    let screen = |p| {
        let p = camera.world_to_screen(p, viewport);
        bounds.origin + point(px(p.x as f32), px(p.y as f32))
    };
    if let Some(rect) = marquee {
        let a = screen(rect.min);
        let b = screen(rect.max);
        let rect = Bounds::new(a, size(b.x - a.x, b.y - a.y));
        window.paint_quad(fill(rect, rgba(0x59b0ff22)));
        for (origin, size) in [
            (a, size(rect.size.width, px(1.))),
            (a, size(px(1.), rect.size.height)),
            (point(a.x, b.y - px(1.)), size(rect.size.width, px(1.))),
            (point(b.x - px(1.), a.y), size(px(1.), rect.size.height)),
        ] {
            window.paint_quad(fill(Bounds::new(origin, size), rgb(0x59b0ff)));
        }
    }
    for guide in guides {
        let (a, b) = match guide.axis {
            Axis::X => (
                WorldPoint::new(guide.coordinate, guide.start),
                WorldPoint::new(guide.coordinate, guide.end),
            ),
            Axis::Y => (
                WorldPoint::new(guide.start, guide.coordinate),
                WorldPoint::new(guide.end, guide.coordinate),
            ),
        };
        let a = screen(a);
        let b = screen(b);
        window.paint_quad(fill(
            Bounds::new(a, size((b.x - a.x).max(px(1.)), (b.y - a.y).max(px(1.)))),
            rgb(0xff79db),
        ));
    }
}

pub(super) fn smoke_marquee(entity: &Entity<Lab>, window: &mut Window, cx: &mut App) {
    let (a, b, expected) = {
        let lab = entity.read(cx);
        let a = lab.bounds.origin + point(px(40.), px(60.));
        let b =
            lab.bounds.origin + point(lab.bounds.size.width * 0.6, lab.bounds.size.height * 0.6);
        let expected: BTreeSet<_> = lab
            .scene
            .query(rectangle(lab.world_at(a), lab.world_at(b)))
            .map(|item| item.data)
            .collect();
        (a, b, expected)
    };
    for (start, end) in [(a, b), (b, a)] {
        let modifiers = Modifiers {
            shift: true,
            ..Default::default()
        };
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                position: start,
                button: MouseButton::Left,
                modifiers,
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers,
            }),
            cx,
        );
        assert_eq!(
            entity.read(cx).selected,
            expected,
            "native rectangular selection"
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                position: end,
                button: MouseButton::Left,
                modifiers,
                ..Default::default()
            }),
            cx,
        );
    }
    if expected.is_empty() {
        return;
    }
    let (position, originals, delta) = {
        let lab = entity.read(cx);
        let viewport = lab.viewport();
        let entry = lab
            .scene
            .visible(lab.camera, viewport)
            .find(|entry| expected.contains(&entry.item.data))
            .unwrap();
        let min = entry.screen_bounds.min;
        let max = entry.screen_bounds.max;
        let position = lab.bounds.origin
            + point(
                px(((min.x.max(0.) + max.x.min(viewport.x)) / 2.) as f32),
                px(((min.y.max(0.) + max.y.min(viewport.y)) / 2.) as f32),
            );
        let originals: Vec<_> = expected
            .iter()
            .map(|&id| (id, lab.scene.items()[id].bounds))
            .collect();
        (
            position,
            originals,
            WorldPoint::new(37. / lab.camera.zoom(), 11. / lab.camera.zoom()),
        )
    };
    let modifiers = Modifiers {
        alt: true,
        ..Default::default()
    };
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: position + point(px(37.), px(11.)),
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        cx,
    );
    for &(id, bounds) in &originals {
        let moved = entity.read(cx).scene.items()[id].bounds;
        let expected = bounds.translated(delta);
        assert!(
            (moved.min.x - expected.min.x).abs() < 0.001
                && (moved.min.y - expected.min.y).abs() < 0.001,
            "native group move"
        );
    }
    let snapped = {
        let lab = entity.read(cx);
        let group = originals
            .iter()
            .map(|(_, bounds)| *bounds)
            .reduce(union)
            .unwrap();
        lab.scene.snap_translation(
            group,
            delta,
            6. / lab.camera.zoom(),
            200. / lab.camera.zoom(),
            |id| expected.contains(id),
        )
    };
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: position + point(px(37.), px(11.)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    assert_eq!(
        entity.read(cx).snap_guides.len(),
        usize::from(snapped.x.is_some()) + usize::from(snapped.y.is_some()),
        "native alignment guides"
    );
    for &(id, bounds) in &originals {
        let moved = entity.read(cx).scene.items()[id].bounds;
        let expected = bounds.translated(snapped.delta);
        assert!(
            (moved.min.x - expected.min.x).abs() < 0.001
                && (moved.min.y - expected.min.y).abs() < 0.001,
            "native magnetic move"
        );
    }
    window.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: Keystroke::parse("escape").unwrap(),
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
    for &(id, bounds) in &originals {
        assert_eq!(
            entity.read(cx).scene.items()[id].bounds.min,
            bounds.min,
            "Escape restores scene bounds"
        );
    }
    assert!(entity.read(cx).gesture.is_none());
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position,
            button: MouseButton::Left,
            ..Default::default()
        }),
        cx,
    );
    let cached = entity.read(cx).overview.clone();
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    // Releasing a world drag over the HUD must still commit and clear the gesture.
    assert!(
        matches!(entity.read(cx).gesture, Some(Gesture::Move { .. })),
        "native commit gesture starts"
    );
    let end = entity.read(cx).map_bounds.center();
    let modifiers = Modifiers {
        alt: true,
        ..Default::default()
    };
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: end,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        cx,
    );
    assert!(
        Arc::ptr_eq(&cached, &entity.read(cx).overview),
        "overview stays cached during movement"
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: end,
            button: MouseButton::Left,
            modifiers,
            ..Default::default()
        }),
        cx,
    );
    assert!(
        !Arc::ptr_eq(&cached, &entity.read(cx).overview),
        "committed movement refreshes overview"
    );
    entity.update(cx, |lab, _| {
        for (id, bounds) in originals {
            assert!(lab.scene.set_bounds(id, bounds));
        }
        lab.refresh_overview();
    });
}

pub(super) fn smoke_preview(
    entity: &Entity<Lab>,
    window: &mut Window,
    cx: &mut App,
    marquee: bool,
) {
    if !marquee {
        entity.update(cx, |lab, _| {
            lab.cancel_selection();
            lab.selected.clear();
        });
    }
    let (start, end) = {
        let lab = entity.read(cx);
        if marquee {
            (
                lab.bounds.origin + point(px(40.), px(60.)),
                lab.bounds.origin + point(px(500.), px(300.)),
            )
        } else {
            let p = lab
                .camera
                .world_to_screen(WorldPoint::new(80., 40.), lab.viewport());
            let start = lab.bounds.origin + point(px(p.x as f32), px(p.y as f32));
            (start, start + point(px(37.), px(0.)))
        }
    };
    let modifiers = Modifiers {
        shift: marquee,
        ..Default::default()
    };
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: start,
            button: MouseButton::Left,
            modifiers,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: end,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        cx,
    );
}
