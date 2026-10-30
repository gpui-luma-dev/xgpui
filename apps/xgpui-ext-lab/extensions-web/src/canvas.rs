//! Browser companions to the native card and CPU Mandelbrot labs.
use gpui::{prelude::*, *};
use std::{collections::BTreeSet, sync::Arc};
use web_time::Instant;
use xgpui_canvas::{
    Camera2D, Canvas, GridOptions, GridPainter, GridStyle, LevelOfDetail, Overview, PanGesture,
    Point as WorldPoint, Scene, SceneItem, WorldBounds,
};
#[path = "../../shared/mandelbrot.rs"]
mod fractal_math;

pub struct CanvasDemo {
    fractal: bool,
    camera: Camera2D,
    bounds: Bounds<Pixels>,
    grid: GridPainter,
    options: GridOptions,
    scene: Scene<usize>,
    selected: BTreeSet<usize>,
    gesture: Option<Selection>,
    pan: PanGesture,
    focus: FocusHandle,
    space: bool,
    overview: Overview,
    overview_key: (usize, u64),
    map_bounds: Bounds<Pixels>,
    map_drag: bool,
    raster: Raster,
}

enum Selection {
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

struct Work {
    camera: Camera2D,
    viewport: WorldPoint,
    bounds: WorldBounds,
    pixels: image::RgbaImage,
    next: usize,
    width: u32,
    height: u32,
    limit: u32,
}
#[derive(Default)]
struct Raster {
    signature: Option<[u64; 5]>,
    work: Option<Work>,
    image: Option<(Arc<RenderImage>, WorldBounds)>,
    fine: bool,
    complete: bool,
}

impl Raster {
    fn advance(&mut self, camera: Camera2D, viewport: WorldPoint) -> bool {
        if viewport.x <= 0. || viewport.y <= 0. {
            return false;
        }
        let bounds = camera.visible_world_bounds(viewport);
        let signature = [
            bounds.min.x.to_bits(),
            bounds.min.y.to_bits(),
            bounds.max.x.to_bits(),
            bounds.max.y.to_bits(),
            camera.zoom().to_bits(),
        ];
        if self.signature != Some(signature) {
            self.signature = Some(signature);
            self.fine = false;
            self.complete = false;
            self.work = None; // Cancels stale work before computing another pixel.
        }
        if self.complete {
            return false;
        }
        if self.work.is_none() {
            let width = if self.fine { 480 } else { 120 };
            let height = (width as f64 * viewport.y / viewport.x)
                .round()
                .clamp(1., 360.) as u32;
            self.work = Some(Work {
                camera,
                viewport,
                bounds,
                pixels: image::RgbaImage::new(width, height),
                next: 0,
                width,
                height,
                limit: fractal_math::iterations(camera.zoom()),
            });
        }
        let work = self.work.as_mut().unwrap();
        let started = Instant::now();
        let total = (work.width * work.height) as usize;
        // Bound each slice by both elapsed time and work count. No workers/shared memory.
        let stop = (work.next + 2048).min(total);
        while work.next < stop {
            let x = work.next as u32 % work.width;
            let y = work.next as u32 / work.width;
            let world = work.camera.screen_to_world(
                WorldPoint::new(
                    (x as f64 + 0.5) * work.viewport.x / work.width as f64,
                    (y as f64 + 0.5) * work.viewport.y / work.height as f64,
                ),
                work.viewport,
            );
            work.pixels.put_pixel(
                x,
                y,
                image::Rgba(fractal_math::bgra(
                    world.x / 300. - 0.5,
                    world.y / 300.,
                    work.limit,
                )),
            );
            work.next += 1;
            if work.next % 32 == 0 && started.elapsed().as_secs_f64() >= 0.004 {
                break;
            }
        }
        if work.next == total {
            let work = self.work.take().unwrap();
            self.image = Some((
                Arc::new(RenderImage::new(vec![image::Frame::new(work.pixels)])),
                work.bounds,
            ));
            if self.fine {
                self.complete = true;
            } else {
                self.fine = true;
            }
        }
        !self.complete
    }
}

impl CanvasDemo {
    pub fn new(fractal: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scene = make_scene(10_000);
        let overview = Overview::new(&scene);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        cx.on_blur(&focus, window, |this, _, cx| {
            this.cancel();
            this.space = false;
            cx.notify();
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.cancel();
                this.space = false;
                cx.notify();
            }
        })
        .detach();
        Self {
            fractal,
            camera: Camera2D::default(),
            bounds: Bounds::default(),
            grid: GridPainter::new().expect("canvas grid shader"),
            options: GridOptions::default(),
            overview,
            overview_key: scene.cache_key(),
            scene,
            selected: BTreeSet::new(),
            gesture: None,
            pan: PanGesture::default(),
            focus,
            space: false,
            map_bounds: Bounds::default(),
            map_drag: false,
            raster: Raster::default(),
        }
    }
    fn viewport(&self) -> WorldPoint {
        wp(self.bounds.size.width, self.bounds.size.height)
    }
    fn world(&self, p: Point<Pixels>) -> WorldPoint {
        let p = p - self.bounds.origin;
        self.camera.screen_to_world(wp(p.x, p.y), self.viewport())
    }
    fn cancel(&mut self) {
        if let Some(gesture) = self.gesture.take() {
            match gesture {
                Selection::Move { originals, .. } => {
                    for (id, bounds) in originals {
                        self.scene.set_bounds(id, bounds);
                    }
                }
                Selection::Marquee { prior, .. } => self.selected = prior,
            }
        }
        self.pan.end();
        self.map_drag = false;
    }
    fn update_selection(&mut self, p: Point<Pixels>, free: bool) {
        let world = self.world(p);
        match self.gesture.as_mut() {
            Some(Selection::Marquee { start, current, .. }) => {
                *current = world;
                self.selected = self
                    .scene
                    .query(rect(*start, world))
                    .map(|i| i.data)
                    .collect();
            }
            Some(Selection::Move {
                anchor,
                originals,
                group,
            }) => {
                let mut delta = WorldPoint::new(world.x - anchor.x, world.y - anchor.y);
                if !free {
                    delta = self
                        .scene
                        .snap_translation(
                            *group,
                            delta,
                            6. / self.camera.zoom(),
                            200. / self.camera.zoom(),
                            |id| self.selected.contains(id),
                        )
                        .delta;
                }
                for &(id, bounds) in originals.iter() {
                    self.scene.set_bounds(id, bounds.translated(delta));
                }
            }
            None => (),
        }
    }
    fn navigate(&mut self, p: Point<Pixels>) {
        let local = p - self.map_bounds.origin;
        if let Some(world) = self.overview.map_to_world(
            wp(local.x, local.y),
            wp(self.map_bounds.size.width, self.map_bounds.size.height),
        ) {
            self.camera.center_on(world);
        }
    }
}
fn wp(x: Pixels, y: Pixels) -> WorldPoint {
    WorldPoint::new(f32::from(x) as f64, f32::from(y) as f64)
}
fn rect(a: WorldPoint, b: WorldPoint) -> WorldBounds {
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
fn make_scene(count: usize) -> Scene<usize> {
    Scene::new((0..count).map(|i| {
        let x = (i % 100) as f64 * 200.;
        let y = (i / 100) as f64 * 120.;
        SceneItem {
            bounds: WorldBounds {
                min: WorldPoint::new(x, y),
                max: WorldPoint::new(x + 160., y + 80.),
            },
            data: i,
        }
    }))
}
fn button(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x263b53))
        .hover(|s| s.bg(rgb(0x365777)))
        .cursor_pointer()
        .child(label.into())
}

impl Render for CanvasDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = self.viewport();
        if self.fractal && self.raster.advance(self.camera, viewport) {
            window.request_animation_frame();
            cx.notify();
        }
        if self.overview_key != self.scene.cache_key() {
            self.overview = Overview::new(&self.scene);
            self.overview_key = self.scene.cache_key();
        }
        let mut cards = Vec::new();
        let mut proxies = Vec::new();
        let mut visible = 0;
        if !self.fractal {
            for entry in self.scene.visible(self.camera, viewport) {
                visible += 1;
                let selected = self.selected.contains(&entry.item.data);
                let b = entry.screen_bounds;
                let origin = point(px(b.min.x as f32), px(b.min.y as f32));
                let extent = size(
                    px((b.max.x - b.min.x) as f32),
                    px((b.max.y - b.min.y) as f32),
                );
                if entry.detail == LevelOfDetail::Full {
                    cards.push(
                        div()
                            .absolute()
                            .left(origin.x)
                            .top(origin.y)
                            .w(extent.width)
                            .h(extent.height)
                            .bg(rgb(if selected { 0x285985 } else { 0x24354b }))
                            .border_1()
                            .border_color(rgb(if selected { 0x72c7ff } else { 0x6484a8 }))
                            .text_size(px((14. * self.camera.zoom()) as f32))
                            .child(format!("Card {}", entry.item.data)),
                    );
                } else {
                    proxies.push((
                        Bounds::new(
                            origin,
                            if entry.detail == LevelOfDetail::Point {
                                size(px(1.), px(1.))
                            } else {
                                extent
                            },
                        ),
                        selected,
                    ));
                }
            }
        }
        let entity = cx.entity();
        let camera = self.camera;
        let image = self.raster.image.clone();
        let marquee = match self.gesture {
            Some(Selection::Marquee { start, current, .. }) => Some(rect(start, current)),
            _ => None,
        };
        let overview_cells: Vec<_> = self.overview.cells().collect();
        let domain = self.overview.domain();
        let world_view = camera.visible_world_bounds(viewport);
        let map_entity = cx.entity();
        let header = if self.fractal {
            format!(
                "CPU Mandelbrot · {:.1}× · {}",
                camera.zoom(),
                if self.raster.complete {
                    "refined"
                } else {
                    "refining…"
                }
            )
        } else {
            format!(
                "Spatial canvas · {visible}/{} visible · {} selected · {:.1}%",
                self.scene.len(),
                self.selected.len(),
                camera.zoom() * 100.
            )
        };
        div().id("spatial-demo").track_focus(&self.focus).size_full().flex().flex_col().bg(rgb(0x101824)).text_color(rgb(0xe6efff)).font_family("IBM Plex Sans")
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| { match e.keystroke.key.as_str() { "space" => this.space = true, "escape" => this.cancel(), "r" => { this.cancel(); this.camera = Camera2D::default(); }, _ => () } cx.notify(); }))
            .on_key_up(cx.listener(|this,e: &KeyUpEvent,_,_| { if e.keystroke.key == "space" { this.space=false; this.pan.end(); } }))
            .child(div().flex_none().p_3().flex().flex_col().gap_2().child(header)
                .child(div().text_sm().child(if self.fractal { "Drag: pan · Wheel or pinch: zoom · R: reset · f64 CPU, coarse then fine, no workers" } else { "Scroll: pan · Ctrl/Cmd+wheel or pinch: zoom · Shift-drag: select · Drag cards: move/snap · Alt: free move · Esc: cancel" }))
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("reset","Reset").on_click(cx.listener(|this,_,_,cx|{this.cancel();this.camera=Camera2D::default();cx.notify();})))
                    .child(button("zoom-out","−").on_click(cx.listener(|this,_,_,cx|{this.cancel();let v=this.viewport();this.camera.zoom_at_with_limits(WorldPoint::new(v.x/2.,v.y/2.),v,0.5,0.01,if this.fractal {1e8}else{64.});cx.notify();})))
                    .child(button("zoom-in","+").on_click(cx.listener(|this,_,_,cx|{this.cancel();let v=this.viewport();this.camera.zoom_at_with_limits(WorldPoint::new(v.x/2.,v.y/2.),v,2.,0.01,if this.fractal {1e8}else{64.});cx.notify();})))
                    .when(!self.fractal, |row| row
                        .child(button("scene-count",if self.scene.len()==10_000 {"10k → 100k"}else{"100k → 10k"}).on_click(cx.listener(|this,_,_,cx|{this.cancel();this.scene=make_scene(if this.scene.len()==10_000 {100_000}else{10_000});this.selected.clear();cx.notify();})))
                        .child(button("grid-style","Lines / dots").on_click(cx.listener(|this,_,_,cx|{this.options.style=match this.options.style {GridStyle::Lines=>GridStyle::Dots,GridStyle::Dots=>GridStyle::Lines};cx.notify();})))
                        .child(button("overview","Overview").on_click(cx.listener(|this,_,_,cx|{this.cancel();this.camera=Camera2D::default();let v=this.viewport();this.camera.zoom_at(WorldPoint::new(v.x/2.,v.y/2.),v,0.01);cx.notify();}))))))
            .child(div().id("canvas-viewport").relative().flex_1().w_full().overflow_hidden()
                .on_scroll_wheel(cx.listener(|this,e: &ScrollWheelEvent,_,cx| {this.cancel();let delta=e.delta.pixel_delta(px(24.));if this.fractal || e.modifiers.control || e.modifiers.platform {let p=e.position-this.bounds.origin;this.camera.zoom_at_with_limits(wp(p.x,p.y),this.viewport(),(-f32::from(delta.y) as f64*0.003).exp().clamp(0.2,5.),0.01,if this.fractal {1e8}else{64.});}else{this.camera.pan_by(wp(delta.x,delta.y));}cx.stop_propagation();cx.notify();}))
                .on_pinch(cx.listener(|this,e: &PinchEvent,_,cx|{this.cancel();let p=e.position-this.bounds.origin;this.camera.zoom_at_with_limits(wp(p.x,p.y),this.viewport(),(1.+f64::from(e.delta)).max(0.01),0.01,if this.fractal {1e8}else{64.});cx.stop_propagation();cx.notify();}))
                .on_mouse_down(MouseButton::Middle,cx.listener(|this,e: &MouseDownEvent,window,cx|{this.focus.focus(window,cx);this.cancel();this.pan.begin(wp(e.position.x,e.position.y));}))
                .on_mouse_down(MouseButton::Left,cx.listener(|this,e: &MouseDownEvent,window,cx|{this.focus.focus(window,cx);this.cancel();if this.fractal || this.space {this.pan.begin(wp(e.position.x,e.position.y));}else{let world=this.world(e.position);if e.modifiers.shift {let prior=std::mem::take(&mut this.selected);this.gesture=Some(Selection::Marquee{start:world,current:world,prior});}else if let Some(id)=this.scene.hit_test(world).map(|i|i.data){if !this.selected.contains(&id){this.selected.clear();this.selected.insert(id);}let originals:Vec<_>=this.selected.iter().map(|&id|(id,this.scene.items()[id].bounds)).collect();let group=originals.iter().map(|(_,b)|*b).reduce(union).unwrap();this.gesture=Some(Selection::Move{anchor:world,originals,group});}else{this.selected.clear();}}cx.notify();}))
                .on_mouse_move(cx.listener(|this,e: &MouseMoveEvent,_,cx|{if this.map_drag {this.navigate(e.position);}else if e.pressed_button==Some(MouseButton::Left) && this.gesture.is_some(){this.update_selection(e.position,e.modifiers.alt);}else if e.pressed_button.is_some(){if let Some(delta)=this.pan.update(wp(e.position.x,e.position.y)){this.camera.pan_by(delta);}}else{this.pan.end();}cx.notify();}))
                .on_mouse_up(MouseButton::Left,cx.listener(|this,e: &MouseUpEvent,_,cx|{this.update_selection(e.position,e.modifiers.alt);this.gesture=None;this.pan.end();this.map_drag=false;cx.notify();}))
                .on_mouse_up_out(MouseButton::Left,cx.listener(|this,e: &MouseUpEvent,_,cx|{this.update_selection(e.position,e.modifiers.alt);this.gesture=None;this.pan.end();this.map_drag=false;cx.notify();}))
                .on_mouse_up(MouseButton::Middle,cx.listener(|this,_,_,_|this.pan.end()))
                .on_mouse_up_out(MouseButton::Middle,cx.listener(|this,_,_,_|this.pan.end()))
                .child(Canvas::with_grid_options(camera,&self.grid,self.options).size_full()
                    .child(canvas(move |bounds,_,cx|{entity.update(cx,|this,cx|{if this.bounds!=bounds{this.bounds=bounds;cx.notify();}});},move |bounds,_,window,_|{
                        if let Some((image,world))=image.clone(){let a=camera.world_to_screen(world.min,viewport);let b=camera.world_to_screen(world.max,viewport);let image_bounds=Bounds::new(bounds.origin+point(px(a.x as f32),px(a.y as f32)),size(px((b.x-a.x) as f32),px((b.y-a.y) as f32)));let _=window.paint_image(image_bounds,image_bounds,Corners::default(),image,0,false);}
                        for (mut b,selected) in proxies {b.origin+=bounds.origin;window.paint_quad(fill(b,rgb(if selected {0x72c7ff}else{0x6484a8})));}
                        if let Some(m)=marquee{let a=camera.world_to_screen(m.min,viewport);let b=camera.world_to_screen(m.max,viewport);window.paint_quad(fill(Bounds::new(bounds.origin+point(px(a.x as f32),px(a.y as f32)),size(px((b.x-a.x) as f32),px((b.y-a.y) as f32))),rgba(0x59b0ff55)));}
                    }).absolute().size_full())
                    .children(cards))
                .when(!self.fractal,|view|view.child(div().id("canvas-map").absolute().right(px(12.)).bottom(px(12.)).w(px(180.)).h(px(120.)).bg(rgb(0x172238)).border_1().border_color(rgb(0x6484a8))
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,e: &MouseDownEvent,_,cx|{this.cancel();this.map_drag=true;this.navigate(e.position);cx.stop_propagation();cx.notify();}))
                    .on_mouse_move(cx.listener(|this,e: &MouseMoveEvent,_,cx|{if this.map_drag && e.pressed_button==Some(MouseButton::Left){this.navigate(e.position);cx.notify();}else if this.gesture.is_some() && e.pressed_button==Some(MouseButton::Left){this.update_selection(e.position,e.modifiers.alt);cx.notify();}cx.stop_propagation();}))
                    .on_mouse_up(MouseButton::Left,cx.listener(|this,e: &MouseUpEvent,_,cx|{this.update_selection(e.position,e.modifiers.alt);this.gesture=None;this.map_drag=false;this.pan.end();cx.stop_propagation();cx.notify();}))
                    .on_scroll_wheel(|_,_,cx|cx.stop_propagation()).on_pinch(|_,_,cx|cx.stop_propagation())
                    .child(canvas(move |bounds,_,cx|{map_entity.update(cx,|this,_|this.map_bounds=bounds);},move |bounds,_,window,_|{
                        let project=|p:WorldPoint|{let scale=(f32::from(bounds.size.width) as f64/(domain.max.x-domain.min.x)).min(f32::from(bounds.size.height) as f64/(domain.max.y-domain.min.y));let x=(f32::from(bounds.size.width) as f64-(domain.max.x-domain.min.x)*scale)/2.;let y=(f32::from(bounds.size.height) as f64-(domain.max.y-domain.min.y)*scale)/2.;bounds.origin+point(px((x+(p.x-domain.min.x)*scale) as f32),px((y+(p.y-domain.min.y)*scale) as f32))};
                        window.with_content_mask(Some(ContentMask{bounds}),|window|{for cell in overview_cells{let a=project(cell.bounds.min);let b=project(cell.bounds.max);window.paint_quad(fill(Bounds::new(a,size((b.x-a.x).max(px(1.)),(b.y-a.y).max(px(1.)))),rgb(0x46688a)));}let a=project(world_view.min);let b=project(world_view.max);let w=(b.x-a.x).max(px(2.));let h=(b.y-a.y).max(px(2.));for (origin,extent) in [(a,size(w,px(1.))),(a,size(px(1.),h)),(a+point(px(0.),h),size(w,px(1.))),(a+point(w,px(0.)),size(px(1.),h))]{window.paint_quad(fill(Bounds::new(origin,extent),rgb(0xffd479)));}});
                    }).size_full()))))
    }
}
