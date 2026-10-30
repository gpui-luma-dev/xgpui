mod interaction;
mod mandelbrot;
mod minimap;
mod overview_map;
mod profile;
use interaction::Gesture;

use clap::Parser;
use gpui::{prelude::*, *};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Instant};
use xgpui_canvas::{
    Camera2D, Canvas as SpatialCanvas, GridPainter, GridStyle, LevelOfDetail, Point as WorldPoint,
    Scene, SceneItem, WorldBounds,
};

#[derive(Parser)]
struct Options {
    /// Open the CPU Mandelbrot image example.
    #[arg(long, conflicts_with_all = ["smoke", "benchmark"])]
    mandelbrot: bool,
    /// Save a refined CPU Mandelbrot frame and quit.
    #[arg(long, requires = "mandelbrot")]
    mandelbrot_capture: Option<PathBuf>,
    /// Profile 10k/100k and dense overview redraws, save JSON/PNGs, then quit.
    #[arg(long)]
    benchmark: Option<PathBuf>,
    /// Capture default, zoom limits, distant world, and resized viewport, then quit.
    #[arg(long, conflicts_with = "benchmark")]
    smoke: Option<PathBuf>,
}

struct Lab {
    fractal: mandelbrot::Mandelbrot,
    profile: Option<profile::Profile>,
    smoke: Option<(PathBuf, usize, bool)>,
    grid: GridPainter,
    grid_style: GridStyle,
    scene: Scene<usize>,
    preview: minimap::Preview,
    overview: Arc<xgpui_canvas::Overview>,
    minimap_image: Arc<RenderImage>,
    overview_key: (usize, u64),
    map_thumbnail: bool,
    map_drag: bool,
    whole_scene_map: bool,
    map_bounds: Bounds<Pixels>,
    show_minimap: bool,
    focus: FocusHandle,
    space_down: bool,
    camera: Camera2D,
    bounds: Bounds<Pixels>,
    drag: xgpui_canvas::PanGesture,
    selected: BTreeSet<usize>,
    gesture: Option<Gesture>,
    snap_guides: Vec<xgpui_canvas::AlignmentGuide>,
}

impl Lab {
    fn viewport(&self) -> WorldPoint {
        WorldPoint::new(
            f64::from(f32::from(self.bounds.size.width)),
            f64::from(f32::from(self.bounds.size.height)),
        )
    }
}

impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = Instant::now();
        let smoke_viewport = self.viewport();
        if let Some((directory, stored_tick, pending)) = &mut self.smoke
            && !*pending
            && !self.preview.capturing
        {
            *pending = true;
            let tick = *stored_tick + 1;
            let scenario = (tick - 1) / 8;
            if (tick - 1) % 8 == 0 {
                self.camera = Camera2D::default();
                match scenario {
                    1 => self
                        .camera
                        .zoom_at(WorldPoint::new(137., 247.), smoke_viewport, 1e-10),
                    2 => self
                        .camera
                        .zoom_at(WorldPoint::new(137., 247.), smoke_viewport, 1e10),
                    3 => self.camera.pan_by(WorldPoint::new(1e10 + 13., -1e10 + 7.)),
                    4 => window.resize(size(px(800.), px(600.))),
                    5 => self.camera.zoom_at(
                        WorldPoint::new(smoke_viewport.x / 2., smoke_viewport.y / 2.),
                        smoke_viewport,
                        0.1,
                    ),
                    _ => (),
                }
            }
            let capture = (tick % 8 == 0).then(|| directory.join(format!("{scenario}.png")));
            let preview = (tick % 8 == 3 && (6..=7).contains(&scenario)).then_some(scenario == 6);
            let done = tick >= 64;
            let overview_capture = directory.join("whole-canvas.png");
            let entity = cx.entity();
            window.on_next_frame(move |window, cx| {
                if let Some(marquee) = preview {
                    interaction::smoke_preview(&entity, window, cx, marquee);
                }
                if let Some(path) = capture {
                    smoke_click(&entity, window, cx);
                    if scenario == 0 || scenario == 5 {
                        interaction::smoke_marquee(&entity, window, cx);
                    }
                    window
                        .render_to_image()
                        .expect("capture smoke frame")
                        .save(path)
                        .expect("save smoke frame");
                }
                if done {
                    entity.update(cx, |lab, _| {
                        lab.whole_scene_map = true;
                        lab.refresh_overview();
                    });
                    window.draw(cx).clear(cx);
                    overview_map::smoke_navigation(&entity, window, cx);
                    overview_map::smoke_invalidation_and_modes(&entity, window, cx);
                    window.draw(cx).clear(cx);
                    window
                        .render_to_image()
                        .expect("capture minimap")
                        .save(overview_capture)
                        .expect("save minimap");
                    cx.quit();
                } else {
                    entity.update(cx, |this, cx| {
                        let smoke = this.smoke.as_mut().unwrap();
                        smoke.1 = tick;
                        smoke.2 = false;
                        cx.notify();
                    });
                }
            });
            window.request_animation_frame();
        }
        if let Some(profile) = &mut self.profile
            && !self.preview.capturing
        {
            let count = profile.begin(started);
            if self.scene.len() != count {
                self.scene = make_scene(count);
            }
            if profile.frame == 301 {
                self.camera = Camera2D::default();
                self.camera.zoom_at(
                    WorldPoint::new(smoke_viewport.x / 2., smoke_viewport.y / 2.),
                    smoke_viewport,
                    0.01,
                );
            }
            self.grid_style = if self.scene.len() == 10_000 {
                GridStyle::Lines
            } else {
                GridStyle::Dots
            };
            self.camera.pan_by(WorldPoint::new(1., 0.5));
        }
        let viewport = self.viewport();
        let zoom = self.camera.zoom() as f32;
        let mut cards = Vec::new();
        let mut proxies = Vec::new();
        let mut point_count = 0;
        for entry in self
            .scene
            .visible(self.camera, viewport)
            .filter(|_| !self.fractal.enabled)
        {
            let id = entry.item.data;
            let screen = entry.screen_bounds.min;
            if entry.detail != LevelOfDetail::Full {
                let mut bounds = Bounds::new(
                    point(px(screen.x as f32), px(screen.y as f32)),
                    size(
                        px((entry.screen_bounds.max.x - screen.x) as f32),
                        px((entry.screen_bounds.max.y - screen.y) as f32),
                    ),
                );
                if entry.detail == LevelOfDetail::Point {
                    point_count += 1;
                    // A small filled proxy at the object's center, never a 1px border.
                    let center = bounds.center();
                    bounds.size = size(px(1.), px(1.));
                    bounds.origin = center - point(px(0.5), px(0.5));
                }
                proxies.push((bounds, self.selected.contains(&id)));
                continue;
            }
            cards.push(
                div()
                    .id(id)
                    .absolute()
                    .left(px(screen.x as f32))
                    .top(px(screen.y as f32))
                    .w(px(160. * zoom))
                    .h(px(80. * zoom))
                    .overflow_hidden()
                    .bg(rgb(if self.selected.contains(&id) {
                        0x316eab
                    } else {
                        0x24354b
                    }))
                    .border_1()
                    .border_color(rgb(0x6484a8))
                    .text_size(px(16. * zoom))
                    .text_color(rgb(0xffffff))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .child(format!("Card {id}")),
                    ),
            );
        }
        let full_count = cards.len();
        let proxy_count = proxies.len();
        let count = full_count + proxy_count;
        if let Some(profile) = &mut self.profile
            && !self.preview.capturing
        {
            profile.record_build(started.elapsed().as_secs_f64() * 1000., count, full_count);
            let capture = match profile.frame {
                20 => Some(profile.output.with_extension("10k.png")),
                170 => Some(profile.output.with_extension("100k.png")),
                320 => Some(profile.output.with_extension("overview.png")),
                _ => None,
            };
            let done = profile.frame >= 450;
            let entity = cx.entity();
            window.on_next_frame(move |window, cx| {
                if let Some(path) = capture {
                    window
                        .render_to_image()
                        .expect("capture canvas frame")
                        .save(path)
                        .expect("save canvas screenshot");
                }
                entity.update(cx, |this, cx| {
                    if done {
                        this.profile.as_ref().unwrap().finish(
                            [
                                f32::from(this.bounds.size.width),
                                f32::from(this.bounds.size.height),
                            ],
                            window.scale_factor(),
                        );
                        cx.quit();
                    } else {
                        cx.notify();
                    }
                });
            });
            window.request_animation_frame();
        }
        let entity = cx.entity();
        let marquee = self.marquee();
        let guides = self.snap_guides.clone();
        let camera = self.camera;
        self.schedule_mandelbrot(cx);
        let fractal = self.fractal.enabled.then(|| self.fractal_element());
        if self.fractal.refined
            && let Some(path) = self.fractal.capture.take()
        {
            window.on_next_frame(move |window, cx| {
                window
                    .render_to_image()
                    .expect("capture Mandelbrot")
                    .save(path)
                    .expect("save Mandelbrot");
                cx.quit();
            });
            window.request_animation_frame();
        }
        if self.show_minimap && self.whole_scene_map && self.overview_key != self.scene.cache_key()
        {
            self.refresh_overview();
        }
        minimap::schedule(self, window, cx);
        let minimap = (self.show_minimap && !self.preview.hidden).then(|| {
            if self.whole_scene_map {
                self.overview_map(cx)
            } else {
                self.minimap(cx)
            }
        });
        div().id("canvas-lab").track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "space" { this.space_down = true; cx.stop_propagation(); }
                if event.keystroke.key == "g" {
                    this.grid_style = match this.grid_style { GridStyle::Lines => GridStyle::Dots, GridStyle::Dots => GridStyle::Lines };
                    cx.notify();
                }
                if event.keystroke.key == "f" { this.cancel_interaction(); this.fractal.toggle(); this.show_minimap = !this.fractal.enabled; this.whole_scene_map = !this.fractal.enabled; this.camera = Camera2D::default(); this.invalidate_preview(); cx.notify(); }
                if event.keystroke.key == "t" && !this.fractal.enabled { this.cancel_interaction(); this.whole_scene_map = !this.whole_scene_map; cx.notify(); }
                if event.keystroke.key == "m" { this.cancel_interaction(); this.show_minimap = !this.show_minimap; cx.notify(); }
                if event.keystroke.key == "escape" { this.cancel_interaction(); cx.notify(); }
                if event.keystroke.key == "r" { this.cancel_interaction(); this.camera = Camera2D::default(); cx.notify(); }
                if event.keystroke.key == "o" {
                    this.cancel_interaction();
                    this.camera = Camera2D::default();
                    let viewport = this.viewport();
                    this.camera.zoom_at(WorldPoint::new(viewport.x / 2., viewport.y / 2.), viewport, 0.01);
                    cx.notify();
                }
                if event.keystroke.key == "b" { this.cancel_interaction(); this.scene = make_scene(if this.scene.len() == 10_000 { 100_000 } else { 10_000 }); this.selected.clear(); this.invalidate_preview(); cx.notify(); }
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" { this.space_down = false; this.drag.end(); cx.stop_propagation(); }
            }))
            .relative().size_full().flex().flex_col().bg(rgb(0x101824)).text_color(rgb(0xffffff))
            .child(div().h(px(88.)).flex_none().text_size(px(13.)).flex().flex_col()
                .child(if self.fractal.enabled { "CPU Mandelbrot • Drag / scroll: pan • Pinch: zoom • F: cards • R: reset" } else { "Shift-drag: select • Drag cards: move/snap • Option: free move • Esc: cancel" })
                .child(if self.fractal.enabled { "Cached CPU tiles • Progressive detail • M: preview (adds capture cost)" } else { "Scroll / Space-drag: pan • Pinch: zoom • G: grid • R: reset • O: overview • B: 10k/100k • M: panel • T: map/view • F: Mandelbrot" })
                .child(if self.fractal.enabled { format!("{:.1}% • CPU tiles • {} iterations • {}", zoom * 100., mandelbrot::iterations(2_f64.powf(self.camera.zoom().log2().ceil())), if self.fractal.refined { "refined" } else { "rendering" }) } else { format!("{:.1}% • {count}/{} visible • {full_count} elements • {proxy_count} proxies ({point_count} points) • {} selected", zoom * 100., self.scene.len(), self.selected.len()) }))
            .child(div().id("viewport").relative().flex_1().w_full().overflow_hidden()
                .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                    this.cancel_interaction();
                    let delta = event.delta.pixel_delta(px(24.));
                    this.camera.pan_by(WorldPoint::new(f32::from(delta.x) as f64, f32::from(delta.y) as f64));
                    cx.stop_propagation(); cx.notify();
                }))
                .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| {
                    this.cancel_interaction();
                    let anchor = event.position - this.bounds.origin;
                    this.camera.zoom_at_with_limits(WorldPoint::new(f32::from(anchor.x) as f64, f32::from(anchor.y) as f64), this.viewport(), (1. + f64::from(event.delta)).max(0.01), 0.01, if this.fractal.enabled { 1e10 } else { 64. });
                    cx.stop_propagation(); cx.notify();
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if this.space_down { this.drag.begin(WorldPoint::new(f32::from(event.position.x) as f64, f32::from(event.position.y) as f64)); }
                    else if this.fractal.enabled { this.drag.begin(WorldPoint::new(f32::from(event.position.x) as f64, f32::from(event.position.y) as f64)); }
                    else { this.start_selection(event); cx.notify(); }
                }))
                .on_mouse_down(MouseButton::Middle, cx.listener(|this, event: &MouseDownEvent, _, _| this.drag.begin(WorldPoint::new(f32::from(event.position.x) as f64, f32::from(event.position.y) as f64))))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if this.gesture.is_some() && event.pressed_button == Some(MouseButton::Left) {
                        this.update_selection(event.position, event.modifiers.alt); cx.notify(); return;
                    }
                    if event.pressed_button != Some(MouseButton::Middle) && !((this.space_down || this.fractal.enabled) && event.pressed_button == Some(MouseButton::Left)) { this.drag.end(); }
                    else if let Some(delta) = this.drag.update(WorldPoint::new(f32::from(event.position.x) as f64, f32::from(event.position.y) as f64)) {
                        this.camera.pan_by(delta);
                        cx.notify();
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, event: &MouseUpEvent, _, cx| { this.finish_selection(event); cx.notify(); }))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, event: &MouseUpEvent, _, cx| { this.finish_selection(event); cx.notify(); }))
                .on_mouse_up(MouseButton::Middle, cx.listener(|this, _, _, _| this.drag.end()))
                .on_mouse_up_out(MouseButton::Middle, cx.listener(|this, _, _, _| this.drag.end()))
                .child(SpatialCanvas::new(self.camera, &self.grid, self.grid_style).size_full()
                .child(canvas(move |bounds, _, cx| {
                    entity.update(cx, |this, cx| {
                        if this.bounds != bounds { this.bounds = bounds; cx.notify(); }
                    });
                }, |_, _, _, _| {}).absolute().size_full())
                .child(canvas(|_, _, _| (), move |bounds, _, window, _| {
                    for (mut proxy, selected) in proxies {
                        proxy.origin += bounds.origin;
                        window.paint_quad(fill(proxy, rgb(if selected { 0x59b0ff } else { 0x6484a8 })));
                    }
                }).absolute().size_full())
                .children(cards)
                .children(fractal)
                .child(canvas(|_, _, _| (), move |bounds, _, window, _| interaction::paint_overlay(window, bounds, camera, marquee, &guides)).absolute().size_full())))
            .children(minimap)
    }
}

// Dispatch real GPUI input against the rendered viewport in native smoke mode.
fn smoke_click(entity: &Entity<Lab>, window: &mut Window, cx: &mut App) {
    let target = {
        let lab = entity.read(cx);
        let viewport = lab.viewport();
        lab.scene.visible(lab.camera, viewport).next().map(|entry| {
            let rect = entry.screen_bounds;
            let local = WorldPoint::new(
                (rect.min.x.max(0.) + rect.max.x.min(viewport.x)) / 2.,
                (rect.min.y.max(0.) + rect.max.y.min(viewport.y)) / 2.,
            );
            let expected = lab
                .scene
                .hit_test(lab.camera.screen_to_world(local, viewport))
                .unwrap()
                .data;
            (
                lab.bounds.origin + point(px(local.x as f32), px(local.y as f32)),
                expected,
            )
        })
    };
    if let Some((position, expected)) = target {
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
        assert_eq!(
            entity.read(cx).selected.contains(&expected),
            true,
            "native viewport selection"
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                position,
                button: MouseButton::Left,
                ..Default::default()
            }),
            cx,
        );
    }
}

fn make_scene(count: usize) -> Scene<usize> {
    Scene::new((0..count).map(|id| {
        let min = WorldPoint::new(
            (id % 100) as f64 * 200. - 10000.,
            (id / 100) as f64 * 120. - 6000.,
        );
        SceneItem {
            bounds: WorldBounds {
                min,
                max: WorldPoint::new(min.x + 160., min.y + 80.),
            },
            data: id,
        }
    }))
}

fn main() {
    let options = Options::parse();
    if let Some(directory) = &options.smoke {
        std::fs::create_dir_all(directory).expect("create smoke directory");
    }
    gpui_platform::application().run(move |cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(750.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("XGPUI Canvas Prototype");
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    focus.focus(window, cx);
                    cx.on_blur(&focus, window, |this: &mut Lab, _, cx| {
                        this.cancel_interaction();
                        this.space_down = false;

                        cx.notify();
                    })
                    .detach();
                    cx.observe_window_activation(window, |this: &mut Lab, window, cx| {
                        if !window.is_window_active() {
                            this.cancel_interaction();
                            this.space_down = false;

                            cx.notify();
                        }
                    })
                    .detach();
                    let scene = make_scene(10_000);
                    let overview = Arc::new(xgpui_canvas::Overview::new(&scene));
                    let minimap_image = overview_map::thumbnail(&scene, &overview);
                    Lab {
                        fractal: mandelbrot::Mandelbrot::new(
                            options.mandelbrot,
                            options.mandelbrot_capture,
                        ),
                        preview: minimap::Preview::default(),
                        overview,
                        minimap_image,
                        overview_key: scene.cache_key(),
                        map_thumbnail: true,
                        map_drag: false,
                        whole_scene_map: !options.mandelbrot && options.smoke.is_none(),
                        map_bounds: Bounds::default(),
                        show_minimap: !options.mandelbrot,
                        profile: options.benchmark.map(profile::Profile::new),
                        smoke: options.smoke.map(|directory| (directory, 0, false)),
                        grid: GridPainter::new().expect("compile canvas grid"),
                        grid_style: GridStyle::Lines,
                        scene,
                        focus,
                        space_down: false,
                        camera: Camera2D::default(),
                        bounds: Bounds::default(),
                        drag: xgpui_canvas::PanGesture::default(),
                        selected: BTreeSet::new(),
                        gesture: None,
                        snap_guides: Vec::new(),
                    }
                })
            },
        )
        .expect("open canvas lab");
        cx.activate(true);
    });
}
