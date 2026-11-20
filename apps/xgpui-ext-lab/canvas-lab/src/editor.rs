//! Mixed-shape consumer of the public canvas/curve APIs (#384).
use super::*;
use xgpui_canvas::{AlignmentGuide, Overview, PanGesture};
use xgpui_curve::{
    geom::{FillRule, fill_contains, stroke_contains},
    gpui_paint,
    kurbo::{Affine, BezPath, Shape as _},
};

#[derive(Clone)]
struct Object {
    name: &'static str,
    path: BezPath,
    base: WorldBounds,
    fill: bool,
    color: u32,
    hidden: bool,
    locked: bool,
}
impl Object {
    fn offset(&self, bounds: WorldBounds) -> WorldPoint {
        WorldPoint::new(
            bounds.min.x - self.base.min.x,
            bounds.min.y - self.base.min.y,
        )
    }
    fn contains(&self, bounds: WorldBounds, world: WorldPoint) -> bool {
        let offset = self.offset(bounds);
        let p = WorldPoint::new(world.x - offset.x, world.y - offset.y).into();
        if self.fill {
            fill_contains(&self.path, p, FillRule::NonZero)
        } else {
            stroke_contains(&self.path, p, 8.)
        }
    }
}

struct Move {
    anchor: WorldPoint,
    originals: Vec<(usize, WorldBounds)>,
    group: WorldBounds,
}

struct Editor {
    scene: Scene<Object>,
    camera: Camera2D,
    grid: GridPainter,
    bounds: Bounds<Pixels>,
    focus: FocusHandle,
    pan: PanGesture,
    moving: Option<Move>,
    selected: BTreeSet<usize>,
    guides: Vec<AlignmentGuide>,
    fitted: bool,
    centered_zoom: bool,
    preview_key: Option<(usize, u64, u64)>,
    preview: Vec<(WorldBounds, u32)>,
    overview: Arc<Overview>,
    smoke: Option<PathBuf>,
}

fn union(a: WorldBounds, b: WorldBounds) -> WorldBounds {
    WorldBounds {
        min: WorldPoint::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
        max: WorldPoint::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
    }
}
fn object(name: &'static str, path: BezPath, fill: bool, color: u32) -> SceneItem<Object> {
    let rect = path
        .bounding_box()
        .inflate(if fill { 0. } else { 4. }, if fill { 0. } else { 4. });
    let bounds: WorldBounds = rect.into();
    SceneItem {
        bounds,
        data: Object {
            name,
            path,
            base: bounds,
            fill,
            color,
            hidden: false,
            locked: false,
        },
    }
}
fn rectangle(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x, y));
    p.line_to((x + w, y));
    p.line_to((x + w, y + h));
    p.line_to((x, y + h));
    p.close_path();
    p
}
fn make_scene() -> Scene<Object> {
    let mut ring = rectangle(150., 90., 240., 200.);
    // Opposite winding makes a real transparent hole under nonzero fill.
    ring.move_to((210., 140.));
    ring.line_to((210., 240.));
    ring.line_to((330., 240.));
    ring.line_to((330., 140.));
    ring.close_path();
    let mut curve = BezPath::new();
    curve.move_to((70., 320.));
    curve.curve_to((150., 160.), (340., 440.), (450., 270.));
    let mut concave = BezPath::new();
    concave.move_to((470., 70.));
    concave.line_to((650., 70.));
    concave.line_to((650., 130.));
    concave.line_to((530., 130.));
    concave.line_to((530., 250.));
    concave.line_to((470., 250.));
    concave.close_path();
    Scene::new([
        object(
            "Background rectangle",
            rectangle(80., 100., 340., 200.),
            true,
            0x3675b5,
        ),
        object("Holed path", ring, true, 0xd09c45),
        object("Open curve", curve, false, 0xf279b1),
        object("Concave path", concave, true, 0x6abf91),
        object(
            "Overlapping rectangle",
            rectangle(380., 330., 210., 110.),
            true,
            0x8b79da,
        ),
    ])
}
impl Editor {
    fn viewport(&self) -> WorldPoint {
        WorldPoint::new(
            f32::from(self.bounds.size.width) as f64,
            f32::from(self.bounds.size.height) as f64,
        )
    }
    fn world(&self, p: Point<Pixels>) -> WorldPoint {
        let local = p - self.bounds.origin;
        self.camera.screen_to_world(
            WorldPoint::new(f32::from(local.x) as f64, f32::from(local.y) as f64),
            self.viewport(),
        )
    }
    fn pick(&self, p: WorldPoint) -> Option<usize> {
        self.scene
            .hits_at(p)
            .find(|(_, item)| {
                !item.data.hidden && !item.data.locked && item.data.contains(item.bounds, p)
            })
            .map(|(i, _)| i)
    }
    fn cancel(&mut self) {
        if let Some(moving) = self.moving.take() {
            assert!(self.scene.set_bounds_many(moving.originals));
        }
        self.pan.end();
        self.guides.clear();
    }
    fn recenter(&mut self) {
        self.cancel();
        if let Some(bounds) = self
            .scene
            .items()
            .iter()
            .map(|item| item.bounds)
            .reduce(union)
        {
            let delta = WorldPoint::new(
                -(bounds.min.x / 2. + bounds.max.x / 2.),
                -(bounds.min.y / 2. + bounds.max.y / 2.),
            );
            // Translate every shape together, preserving relative placement and identity.
            let updates: Vec<_> = self
                .scene
                .items()
                .iter()
                .enumerate()
                .map(|(i, item)| (i, item.bounds.translated(delta)))
                .collect();
            assert!(self.scene.set_bounds_many(updates));
        }
        self.camera.center_on(WorldPoint::new(0., 0.));
        self.centered_zoom = true;
    }
    fn zoom(&mut self, position: Point<Pixels>, factor: f64) {
        self.cancel();
        let viewport = self.viewport();
        let anchor = if self.centered_zoom {
            WorldPoint::new(viewport.x / 2., viewport.y / 2.)
        } else {
            let p = position - self.bounds.origin;
            WorldPoint::new(f32::from(p.x) as f64, f32::from(p.y) as f64)
        };
        self.camera
            .zoom_at_with_limits(anchor, viewport, factor, 1e-6, 64.);
    }
    fn fit(&mut self, selection: bool) {
        self.cancel();
        self.centered_zoom = false;
        let bounds = self
            .scene
            .items()
            .iter()
            .enumerate()
            .filter(|(i, item)| !item.data.hidden && (!selection || self.selected.contains(i)))
            .map(|(_, item)| item.bounds)
            .reduce(union);
        if let Some(bounds) = bounds {
            self.camera
                .fit_bounds_with_limits(bounds, self.viewport(), 40., 1e-6, 64.);
        }
    }
    fn down(&mut self, event: &MouseDownEvent) {
        self.cancel();
        let world = self.world(event.position);
        if let Some(index) = self.pick(world) {
            if event.modifiers.shift {
                if !self.selected.remove(&index) {
                    self.selected.insert(index);
                }
                return;
            }
            if !self.selected.contains(&index) {
                self.selected.clear();
                self.selected.insert(index);
            }
            let originals: Vec<_> = self
                .selected
                .iter()
                .copied()
                .filter(|i| {
                    !self.scene.items()[*i].data.locked && !self.scene.items()[*i].data.hidden
                })
                .map(|i| (i, self.scene.items()[i].bounds))
                .collect();
            if let Some(group) = originals.iter().map(|(_, b)| *b).reduce(union) {
                self.moving = Some(Move {
                    anchor: world,
                    originals,
                    group,
                });
            }
        } else {
            if !event.modifiers.shift {
                self.selected.clear();
            }
            self.centered_zoom = false;
            self.pan.begin(WorldPoint::new(
                f32::from(event.position.x) as f64,
                f32::from(event.position.y) as f64,
            ));
        }
    }
    fn movement(&mut self, p: Point<Pixels>, free: bool) {
        if let Some(moving) = &self.moving {
            let world = self.world(p);
            let mut delta = WorldPoint::new(world.x - moving.anchor.x, world.y - moving.anchor.y);
            self.guides.clear();
            if !free {
                let snap = self.scene.snap_translation(
                    moving.group,
                    delta,
                    6. / self.camera.zoom(),
                    200. / self.camera.zoom(),
                    |object| {
                        self.selected
                            .iter()
                            .any(|i| std::ptr::eq(object, &self.scene.items()[*i].data))
                            || object.hidden
                            || object.locked
                    },
                );
                delta = snap.delta;
                self.guides.extend(snap.x.into_iter().chain(snap.y));
            }
            if !self.scene.set_bounds_many(
                moving
                    .originals
                    .iter()
                    .map(|(i, b)| (*i, b.translated(delta))),
            ) {
                self.guides.clear();
            }
        } else if let Some(delta) = self.pan.update(WorldPoint::new(
            f32::from(p.x) as f64,
            f32::from(p.y) as f64,
        )) {
            self.camera.pan_by(delta);
        }
    }
    fn edit(&mut self, key: &str) {
        self.cancel();
        if key == "v" {
            for i in 0..self.scene.len() {
                if self.scene.items()[i].data.hidden {
                    self.scene.data_mut(i).unwrap().hidden = false;
                }
            }
            return;
        }
        for &i in &self.selected {
            let object = self.scene.data_mut(i).unwrap();
            match key {
                "c" => {
                    object.color = if object.color == 0x58c9dc {
                        0xd09c45
                    } else {
                        0x58c9dc
                    }
                }
                "l" => object.locked = !object.locked,
                "h" => object.hidden = !object.hidden,
                _ => (),
            }
        }
    }
    fn refresh_preview(&mut self) {
        let key = self.scene.content_cache_key();
        if self.preview_key == Some(key) {
            return;
        }
        self.preview = self
            .scene
            .items()
            .iter()
            .filter(|i| !i.data.hidden)
            .map(|i| (i.bounds, i.data.color))
            .collect();
        self.overview = Arc::new(Overview::new(&self.scene));
        self.preview_key = Some(key);
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.fitted && self.viewport().x > 0. && self.viewport().y > 0. {
            self.fit(false);
            self.fitted = true;
        }
        self.refresh_preview();
        let viewport = self.viewport();
        let camera = self.camera;
        let objects: Vec<_> = self
            .scene
            .visible_indexed(camera, viewport)
            .filter(|(_, v)| !v.item.data.hidden)
            .map(|(i, v)| (i, v.item.bounds, v.item.data.clone()))
            .collect();
        let selected = self.selected.clone();
        let guides = self.guides.clone();
        let preview = self.preview.clone();
        let overview = self.overview.clone();
        let entity = cx.entity();
        if self.fitted
            && window.is_window_active()
            && let Some(directory) = self.smoke.take()
        {
            let entity = entity.clone();
            window.on_next_frame(move |window, cx| smoke(&entity, window, cx, directory));
            window.request_animation_frame();
        }
        let names = self
            .selected
            .iter()
            .map(|i| self.scene.items()[*i].data.name)
            .collect::<Vec<_>>()
            .join(", ");
        div().track_focus(&self.focus).size_full().flex().flex_col().bg(rgb(0x101824)).text_color(rgb(0xe9eff7))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                match e.keystroke.key.as_str() { "escape" => this.cancel(), "f" => this.fit(false), "s" => this.fit(true), "0" => this.recenter(),
                    "c"|"l"|"h"|"v" => this.edit(e.keystroke.key.as_str()), "r" => { this.cancel(); this.scene.rebuild_index(); }, _=>() } cx.notify();
            }))
            .child(div().p_3().flex_none().flex().flex_col().gap_1()
                .child("Mixed-shape editor · rectangles, holes, concave fills and open curves")
                .child("Click: select · Shift-click: group · Drag: move/snap · Option: free move · Background drag: pan · Wheel/pinch: zoom")
                .child("F: fit scene · S: fit selection · C: color · L: lock/unlock selected · H: hide selected · V: show all · R: rebuild index · Esc: cancel")
                .child(div().id("recenter").cursor_pointer().child("Re-center (0): drawing + grid origin")
                    .on_click(cx.listener(|this, _, window, cx| { this.recenter(); this.focus.focus(window, cx); cx.notify(); })))
                .child(format!("{:.1}% · {} selected · {names}",camera.zoom()*100.,self.selected.len())))
            .child(div().relative().flex_1().w_full().overflow_hidden()
                .on_mouse_down(MouseButton::Left,cx.listener(|this,e:&MouseDownEvent,window,cx|{this.focus.focus(window,cx);this.down(e);cx.notify();}))
                .on_mouse_move(cx.listener(|this,e:&MouseMoveEvent,_,cx|{if e.pressed_button==Some(MouseButton::Left){this.movement(e.position,e.modifiers.alt);cx.notify();}else{this.pan.end();}}))
                .on_mouse_up(MouseButton::Left,cx.listener(|this,e:&MouseUpEvent,_,cx|{if this.moving.is_some(){this.movement(e.position,e.modifiers.alt);}this.moving=None;this.pan.end();this.guides.clear();cx.notify();}))
                .on_mouse_up_out(MouseButton::Left,cx.listener(|this,e:&MouseUpEvent,_,cx|{if this.moving.is_some(){this.movement(e.position,e.modifiers.alt);}this.moving=None;this.pan.end();this.guides.clear();cx.notify();}))
                .on_scroll(cx.listener(|this,e:&ScrollEvent,_,cx|{let d=e.delta.pixel_delta(px(24.));this.zoom(e.position,(f32::from(d.y) as f64*0.005).clamp(-1.,1.).exp());cx.stop_propagation();cx.notify();}))
                .on_pinch(cx.listener(|this,e:&PinchEvent,_,cx|{this.zoom(e.position,(1.+f64::from(e.delta)).max(0.01));cx.stop_propagation();cx.notify();}))
                .child(SpatialCanvas::new(camera,&self.grid,GridStyle::Lines).size_full()
                    .child(canvas(move|bounds,_,cx|{entity.update(cx,|this,cx|{if this.bounds!=bounds{this.bounds=bounds;cx.notify();}});},move|bounds,_,window,_|{
                        if let Some(matrix)=camera.world_to_screen_affine(viewport) {
                            let base=Affine::translate((f32::from(bounds.origin.x) as f64,f32::from(bounds.origin.y) as f64))*Affine::new(matrix);
                            for (i,world,object) in &objects {
                                let offset=object.offset(*world);let t=base*Affine::translate((offset.x,offset.y));
                                let path=if object.fill {gpui_paint::fill_path(&object.path,t)}else{gpui_paint::stroke_path(&object.path,t,(8.*camera.zoom()) as f32)};
                                if let Some(path)=path {window.with_element_opacity(Some(if object.locked {0.4}else{1.}),|w|w.paint_path(path,rgb(object.color)));}
                                if selected.contains(i) {let b=camera.world_bounds_to_screen(*world,viewport);let a=bounds.origin+point(px(b.min.x as f32),px(b.min.y as f32));let size=gpui::size(px((b.max.x-b.min.x) as f32),px((b.max.y-b.min.y) as f32));window.paint_quad(outline(Bounds::new(a,size),rgb(0xffd479),BorderStyle::default()));}
                            }
                        }
                        interaction::paint_overlay(window,bounds,camera,None,&guides);
                    }).size_full())))
            .child(div().h(px(130.)).flex_none().relative().bg(rgb(0x172238))
                .child(div().absolute().left(px(8.)).top(px(4.)).child("Bounds overview · updates with geometry and appearance"))
                .child(canvas(|_,_,_|(),move|bounds,_,window,_|{let size=WorldPoint::new(f32::from(bounds.size.width) as f64,f32::from(bounds.size.height) as f64);for (world,color) in &preview {let a=overview.world_to_map(world.min,size);let b=overview.world_to_map(world.max,size);window.paint_quad(fill(Bounds::from_corners(bounds.origin+point(px(a.x as f32),px(a.y as f32)),bounds.origin+point(px(b.x as f32),px(b.y as f32))),rgb(*color)));}}).absolute().top(px(30.)).bottom(px(6.)).left(px(8.)).right(px(8.))))
    }
}

pub(super) fn run(smoke: Option<PathBuf>) {
    gpui_platform::application().run(move |cx: &mut App| {
        if smoke.is_some() {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                eprintln!("editor smoke timed out; requires an active native window");
                std::process::exit(1);
            })
            .detach();
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.open_lab_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(820.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_lab_title("Canvas mixed-shape editor");
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    focus.focus(window, cx);
                    cx.on_blur(&focus, window, |this: &mut Editor, _, cx| {
                        this.cancel();
                        cx.notify();
                    })
                    .detach();
                    cx.observe_window_activation(window, |this: &mut Editor, window, cx| {
                        if !window.is_window_active() {
                            this.cancel();
                            cx.notify();
                        }
                    })
                    .detach();
                    let scene = make_scene();
                    let overview = Arc::new(Overview::new(&scene));
                    Editor {
                        scene,
                        camera: Camera2D::default(),
                        grid: GridPainter::new().expect("editor grid"),
                        bounds: Bounds::default(),
                        focus,
                        pan: PanGesture::default(),
                        moving: None,
                        selected: BTreeSet::new(),
                        guides: Vec::new(),
                        fitted: false,
                        centered_zoom: false,
                        preview_key: None,
                        preview: Vec::new(),
                        overview,
                        smoke,
                    }
                })
            },
        )
        .expect("open editor window");
        cx.activate(true);
    });
}

fn smoke(entity: &Entity<Editor>, window: &mut Window, cx: &mut App, directory: PathBuf) {
    assert!(
        window.is_window_active(),
        "focus acceptance requires an active native window"
    );
    std::fs::create_dir_all(&directory).expect("create editor smoke directory");
    let report = directory.join("report.json");
    if report.exists() {
        std::fs::remove_file(report).expect("remove previous smoke report");
    }
    let position = |world: WorldPoint, cx: &App| {
        let editor = entity.read(cx);
        let p = editor.camera.world_to_screen(world, editor.viewport());
        editor.bounds.origin + point(px(p.x as f32), px(p.y as f32))
    };
    let key = |name: &'static str, window: &mut Window, cx: &mut App| {
        window.dispatch_event(
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: Keystroke::parse(name).unwrap(),
                is_held: false,
                prefer_character_input: false,
            }),
            cx,
        );
        window.draw(cx).clear(cx);
    };
    let down = |p: Point<Pixels>, shift: bool, window: &mut Window, cx: &mut App| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                position: p,
                button: MouseButton::Left,
                modifiers: Modifiers {
                    shift,
                    ..Default::default()
                },
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
    };
    let up = |p: Point<Pixels>, window: &mut Window, cx: &mut App| {
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                position: p,
                button: MouseButton::Left,
                ..Default::default()
            }),
            cx,
        );
    };
    let motion = |p: Point<Pixels>, alt: bool, window: &mut Window, cx: &mut App| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: p,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers {
                    alt,
                    ..Default::default()
                },
            }),
            cx,
        );
    };
    window
        .render_to_image()
        .unwrap()
        .save(directory.join("startup.png"))
        .unwrap();
    let hole = position(WorldPoint::new(270., 190.), cx);
    down(hole, false, window, cx);
    up(hole, window, cx);
    assert_eq!(
        entity.read(cx).selected,
        BTreeSet::from([0]),
        "hole picks the rectangle underneath"
    );
    let ring = position(WorldPoint::new(170., 110.), cx);
    down(ring, true, window, cx);
    up(ring, window, cx);
    assert_eq!(entity.read(cx).selected, BTreeSet::from([0, 1]));
    let original: Vec<_> = entity
        .read(cx)
        .selected
        .iter()
        .map(|&i| (i, entity.read(cx).scene.items()[i].bounds))
        .collect();
    down(ring, false, window, cx);
    motion(ring + point(px(23.), px(17.)), true, window, cx);
    let zoom = entity.read(cx).camera.zoom();
    for &(i, b) in &original {
        let moved = entity.read(cx).scene.items()[i].bounds;
        assert!(
            (moved.min.x - b.min.x - 23. / zoom).abs() < 1e-3
                && (moved.min.y - b.min.y - 17. / zoom).abs() < 1e-3
        );
    }
    key("escape", window, cx);
    for &(i, b) in &original {
        assert_eq!(entity.read(cx).scene.items()[i].bounds.min, b.min);
    }
    assert!(entity.read(cx).moving.is_none());
    let target = position(WorldPoint::new(480., 380.), cx);
    down(target, false, window, cx);
    assert_eq!(entity.read(cx).selected, BTreeSet::from([4]));
    let moving = entity.read(cx).moving.as_ref().unwrap().group;
    let delta = WorldPoint::new(15. / zoom, 0.);
    let expected =
        entity
            .read(cx)
            .scene
            .snap_translation(moving, delta, 6. / zoom, 200. / zoom, |o| {
                o.name == "Overlapping rectangle"
            });
    motion(target + point(px(15.), px(0.)), false, window, cx);
    assert_eq!(
        entity.read(cx).guides.len(),
        usize::from(expected.x.is_some()) + usize::from(expected.y.is_some())
    );
    let moved = entity.read(cx).scene.items()[4].bounds;
    assert!((moved.min.x - moving.min.x - expected.delta.x).abs() < 1e-3);
    key("escape", window, cx);
    let geometry = entity.read(cx).scene.cache_key();
    let content = entity.read(cx).scene.content_cache_key();
    let color = entity.read(cx).scene.items()[4].data.color;
    key("c", window, cx);
    assert_eq!(entity.read(cx).scene.cache_key(), geometry);
    assert_ne!(entity.read(cx).scene.content_cache_key(), content);
    assert_ne!(entity.read(cx).scene.items()[4].data.color, color);
    assert_eq!(
        entity.read(cx).preview_key,
        Some(entity.read(cx).scene.content_cache_key())
    );
    assert_eq!(
        entity.read(cx).preview[4].1,
        entity.read(cx).scene.items()[4].data.color
    );
    let content = entity.read(cx).scene.content_cache_key();
    key("r", window, cx);
    assert_eq!(entity.read(cx).scene.content_cache_key(), content);
    key("l", window, cx);
    assert!(entity.read(cx).pick(WorldPoint::new(480., 380.)).is_none());
    key("l", window, cx);
    assert_eq!(entity.read(cx).pick(WorldPoint::new(480., 380.)), Some(4));
    key("h", window, cx);
    assert!(entity.read(cx).pick(WorldPoint::new(480., 380.)).is_none());
    key("v", window, cx);
    assert_eq!(entity.read(cx).pick(WorldPoint::new(480., 380.)), Some(4));
    key("s", window, cx);
    let editor = entity.read(cx);
    let selection = editor
        .camera
        .world_bounds_to_screen(editor.scene.items()[4].bounds, editor.viewport());
    assert!(selection.min.x >= 40. - 1e-4 && selection.min.y >= 40. - 1e-4);
    window
        .render_to_image()
        .unwrap()
        .save(directory.join("fit-selection.png"))
        .unwrap();
    let zoom = entity.read(cx).camera.zoom();
    let original_bounds: Vec<_> = entity
        .read(cx)
        .scene
        .items()
        .iter()
        .enumerate()
        .map(|(i, item)| (i, item.bounds))
        .collect();
    key("0", window, cx);
    let editor = entity.read(cx);
    let drawing = editor
        .scene
        .items()
        .iter()
        .map(|item| item.bounds)
        .reduce(union)
        .unwrap();
    assert!(
        (drawing.min.x + drawing.max.x).abs() < 1e-9
            && (drawing.min.y + drawing.max.y).abs() < 1e-9,
        "re-center must place the drawing center over grid origin"
    );
    assert_eq!(editor.camera.center(), WorldPoint::new(0., 0.));
    assert_eq!(editor.camera.zoom(), zoom);
    let origin = editor
        .camera
        .world_to_screen(WorldPoint::new(0., 0.), editor.viewport());
    assert_eq!(
        origin,
        WorldPoint::new(editor.viewport().x / 2., editor.viewport().y / 2.)
    );
    window.draw(cx).clear(cx);
    window
        .render_to_image()
        .unwrap()
        .save(directory.join("recentered.png"))
        .unwrap();
    let off_center = entity.read(cx).bounds.origin + point(px(80.), px(60.));
    for delta in [0.5, -0.25] {
        let previous_zoom = entity.read(cx).camera.zoom();
        window.dispatch_event(
            PlatformInput::Pinch(PinchEvent {
                position: off_center,
                delta,
                ..Default::default()
            }),
            cx,
        );
        let editor = entity.read(cx);
        assert_ne!(editor.camera.zoom(), previous_zoom);
        assert_eq!(
            editor.camera.center(),
            WorldPoint::new(0., 0.),
            "re-centered pinch must keep grid origin centered"
        );
    }
    for delta in [30., -30.] {
        let previous_zoom = entity.read(cx).camera.zoom();
        window.dispatch_event(
            PlatformInput::Scroll(ScrollEvent {
                position: off_center,
                delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
                ..Default::default()
            }),
            cx,
        );
        let editor = entity.read(cx);
        assert_ne!(editor.camera.zoom(), previous_zoom);
        assert_eq!(
            editor.camera.center(),
            WorldPoint::new(0., 0.),
            "re-centered wheel must keep grid origin centered"
        );
    }
    entity.update(cx, |this, _| {
        assert!(this.scene.set_bounds_many(original_bounds));
    });
    key("f", window, cx);
    let anchor = position(WorldPoint::new(270., 190.), cx);
    let before = entity.read(cx).world(anchor);
    let old_zoom = entity.read(cx).camera.zoom();
    window.dispatch_event(
        PlatformInput::Pinch(PinchEvent {
            position: anchor,
            delta: 0.2,
            ..Default::default()
        }),
        cx,
    );
    assert!(
        entity.read(cx).camera.zoom() > old_zoom * 1.1,
        "pinch must actually change zoom"
    );
    let after = entity.read(cx).world(anchor);
    assert!((before.x - after.x).abs() < 1e-6 && (before.y - after.y).abs() < 1e-6);
    let before = entity.read(cx).world(anchor);
    let old_zoom = entity.read(cx).camera.zoom();
    window.dispatch_event(
        PlatformInput::Scroll(ScrollEvent {
            position: anchor,
            delta: ScrollDelta::Pixels(point(px(0.), px(40.))),
            ..Default::default()
        }),
        cx,
    );
    assert!(entity.read(cx).camera.zoom() > old_zoom * 1.1);
    let after = entity.read(cx).world(anchor);
    assert!((before.x - after.x).abs() < 1e-6 && (before.y - after.y).abs() < 1e-6);
    key("f", window, cx);
    let start = position(WorldPoint::new(50., 50.), cx);
    let center = entity.read(cx).camera.center();
    let zoom = entity.read(cx).camera.zoom();
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: start,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: start + point(px(12.), px(9.)),
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: start + point(px(12.), px(9.)),
            button: MouseButton::Left,
            ..Default::default()
        }),
        cx,
    );
    let moved = entity.read(cx).camera.center();
    assert!(
        (moved.x - center.x + 12. / zoom).abs() < 1e-6
            && (moved.y - center.y + 9. / zoom).abs() < 1e-6
    );
    key("f", window, cx);
    let target = position(WorldPoint::new(480., 380.), cx);
    let original = entity.read(cx).scene.items()[4].bounds;
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: target,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: target + point(px(10.), px(0.)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers {
                alt: true,
                ..Default::default()
            },
        }),
        cx,
    );
    assert_ne!(entity.read(cx).scene.items()[4].bounds.min, original.min);
    window.blur(cx);
    // Flush focus transitions through a draw, then inspect after queued effects.
    window.draw(cx).clear(cx);
    let handle = window.window_handle();
    let entity = entity.clone();
    cx.defer(move |cx| { handle.update(cx, |_, window, cx| {
        assert_eq!(entity.read(cx).scene.items()[4].bounds.min, original.min);
        assert!(entity.read(cx).moving.is_none() && !entity.read(cx).pan.is_active());
        entity.update(cx, |this, cx| this.focus.focus(window, cx));
        window.draw(cx).clear(cx);
        window.render_to_image().unwrap().save(directory.join("edited.png")).unwrap();
        let report = serde_json::json!({"passed":true,"checks":["hole picking","group move","free move","snap guides","Escape restore","appearance invalidation","rebuild","lock/hide","fit scene/selection","centered zoom in/out","pinch anchor","wheel anchor","background pan","focus-loss restore"]});
        std::fs::write(directory.join("report.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        cx.quit();
    }).expect("editor focus verification window"); });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pick(scene: &Scene<Object>, point: WorldPoint) -> Option<usize> {
        scene
            .hits_at(point)
            .find(|(_, o)| !o.data.hidden && !o.data.locked && o.data.contains(o.bounds, point))
            .map(|(i, _)| i)
    }
    #[::core::prelude::v1::test]
    fn shape_picking_rejects_holes_notches_and_empty_curve_bounds() {
        let scene = make_scene();
        assert_eq!(pick(&scene, WorldPoint::new(270., 190.)), Some(0));
        assert_eq!(pick(&scene, WorldPoint::new(170., 110.)), Some(1));
        assert_eq!(pick(&scene, WorldPoint::new(600., 200.)), None);
        assert_eq!(pick(&scene, WorldPoint::new(70., 320.)), Some(2));
        assert!(
            scene
                .hits_at(WorldPoint::new(200., 320.))
                .any(|(i, _)| i == 2)
        );
        assert_ne!(pick(&scene, WorldPoint::new(200., 320.)), Some(2));
    }
    #[::core::prelude::v1::test]
    fn moved_paths_use_the_same_offset_for_bounds_and_precise_hits() {
        let mut scene = make_scene();
        let old = scene.items()[1].bounds;
        assert!(scene.set_bounds_many([(1, old.translated(WorldPoint::new(800., 0.)))]));
        assert_eq!(pick(&scene, WorldPoint::new(970., 110.)), Some(1));
        assert_eq!(pick(&scene, WorldPoint::new(1070., 190.)), None);
        scene.data_mut(1).unwrap().locked = true;
        assert_eq!(pick(&scene, WorldPoint::new(970., 110.)), None);
        scene.data_mut(1).unwrap().locked = false;
        scene.data_mut(1).unwrap().hidden = true;
        assert_eq!(pick(&scene, WorldPoint::new(970., 110.)), None);
    }
}
