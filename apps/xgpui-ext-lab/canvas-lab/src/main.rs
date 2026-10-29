mod profile;

use clap::Parser;
use gpui::{prelude::*, *};
use std::{path::PathBuf, time::Instant};
use xgpui_canvas::{
    Camera2D, Canvas as SpatialCanvas, GridPainter, GridStyle, Point as WorldPoint, WorldBounds,
};

#[derive(Parser)]
struct Options {
    /// Profile 10k/100k windowed redraws, save JSON and two PNGs, then quit.
    #[arg(long)]
    benchmark: Option<PathBuf>,
    /// Capture default, zoom limits, distant world, and resized viewport, then quit.
    #[arg(long, conflicts_with = "benchmark")]
    smoke: Option<PathBuf>,
}

struct Lab {
    profile: Option<profile::Profile>,
    smoke: Option<(PathBuf, usize)>,
    grid: GridPainter,
    grid_style: GridStyle,
    scene_count: usize,
    focus: FocusHandle,
    space_down: bool,
    camera: Camera2D,
    bounds: Bounds<Pixels>,
    drag: Option<Point<Pixels>>,
    selected: Option<usize>,
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
        if let Some((directory, tick)) = &mut self.smoke {
            *tick += 1;
            let scenario = (*tick - 1) / 8;
            if (*tick - 1) % 8 == 0 {
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
                    _ => (),
                }
            }
            let capture = (*tick % 8 == 0).then(|| directory.join(format!("{scenario}.png")));
            let done = *tick >= 40;
            let entity = cx.entity();
            window.on_next_frame(move |window, cx| {
                if let Some(path) = capture {
                    window
                        .render_to_image()
                        .expect("capture smoke frame")
                        .save(path)
                        .expect("save smoke frame");
                }
                if done {
                    cx.quit();
                } else {
                    entity.update(cx, |_, cx| cx.notify());
                }
            });
            window.request_animation_frame();
        }
        if let Some(profile) = &mut self.profile {
            self.scene_count = profile.begin(started);
            self.grid_style = if self.scene_count == 10_000 {
                GridStyle::Lines
            } else {
                GridStyle::Dots
            };
            self.camera.pan_by(WorldPoint::new(1., 0.5));
        }
        let viewport = self.viewport();
        let visible = self.camera.visible_world_bounds(viewport);
        let zoom = self.camera.zoom() as f32;
        let mut cards = Vec::new();
        // Cull using lightweight world bounds before constructing GPUI children.
        for id in 0..self.scene_count {
            let min = WorldPoint::new(
                (id % 100) as f64 * 200. - 10000.,
                (id / 100) as f64 * 120. - 6000.,
            );
            let bounds = WorldBounds {
                min,
                max: WorldPoint::new(min.x + 160., min.y + 80.),
            };
            if !bounds.intersects(visible) {
                continue;
            }
            let screen = self.camera.world_to_screen(min, viewport);
            cards.push(
                div()
                    .id(id)
                    .absolute()
                    .left(px(screen.x as f32))
                    .top(px(screen.y as f32))
                    .w(px(160. * zoom))
                    .h(px(80. * zoom))
                    .overflow_hidden()
                    .bg(rgb(if self.selected == Some(id) {
                        0x316eab
                    } else {
                        0x24354b
                    }))
                    .border_1()
                    .border_color(rgb(0x6484a8))
                    .text_size(px(16. * zoom))
                    .text_color(rgb(0xffffff))
                    .child(format!("Card {id}"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.focus.focus(window, cx);
                            if this.space_down {
                                this.drag = Some(event.position);
                                cx.stop_propagation();
                                return;
                            }
                            let local = event.position - this.bounds.origin;
                            let world = this.camera.screen_to_world(
                                WorldPoint::new(
                                    f32::from(local.x) as f64,
                                    f32::from(local.y) as f64,
                                ),
                                this.viewport(),
                            );
                            if world.x >= bounds.min.x
                                && world.x <= bounds.max.x
                                && world.y >= bounds.min.y
                                && world.y <= bounds.max.y
                            {
                                this.selected = Some(id);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        let count = cards.len();
        if let Some(profile) = &mut self.profile {
            profile.record_build(started.elapsed().as_secs_f64() * 1000., count);
            let capture = match profile.frame {
                20 => Some(profile.output.with_extension("10k.png")),
                170 => Some(profile.output.with_extension("100k.png")),
                _ => None,
            };
            let done = profile.frame >= 300;
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
        div().id("canvas-lab").track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "space" { this.space_down = true; cx.stop_propagation(); }
                if event.keystroke.key == "g" {
                    this.grid_style = match this.grid_style { GridStyle::Lines => GridStyle::Dots, GridStyle::Dots => GridStyle::Lines };
                    cx.notify();
                }
                if event.keystroke.key == "r" { this.camera = Camera2D::default(); cx.notify(); }
                if event.keystroke.key == "b" { this.scene_count = if this.scene_count == 10_000 { 100_000 } else { 10_000 }; cx.notify(); }
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" { this.space_down = false; this.drag = None; cx.stop_propagation(); }
            }))
            .size_full().flex().flex_col().bg(rgb(0x101824)).text_color(rgb(0xffffff))
            .child(div().h(px(48.)).child(format!("Canvas prototype • Two-finger scroll / Space-drag: pan • Pinch: zoom • Click card: select • {:.1}% • G: grid • R: reset • B: 10k/100k • {count}/{} visible • selected {:?}", zoom * 100., self.scene_count, self.selected)))
            .child(div().id("viewport").relative().flex_1().w_full().overflow_hidden()
                .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                    let delta = event.delta.pixel_delta(px(24.));
                    this.camera.pan_by(WorldPoint::new(f32::from(delta.x) as f64, f32::from(delta.y) as f64));
                    cx.stop_propagation(); cx.notify();
                }))
                .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| {
                    let anchor = event.position - this.bounds.origin;
                    this.camera.zoom_at(WorldPoint::new(f32::from(anchor.x) as f64, f32::from(anchor.y) as f64), this.viewport(), (1. + f64::from(event.delta)).max(0.01));
                    cx.stop_propagation(); cx.notify();
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if this.space_down { this.drag = Some(event.position); }
                }))
                .on_mouse_down(MouseButton::Middle, cx.listener(|this, event: &MouseDownEvent, _, _| this.drag = Some(event.position)))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if event.pressed_button != Some(MouseButton::Middle) && !(this.space_down && event.pressed_button == Some(MouseButton::Left)) { this.drag = None; }
                    else if let Some(previous) = this.drag.replace(event.position) {
                        let delta = event.position - previous;
                        this.camera.pan_by(WorldPoint::new(f32::from(delta.x) as f64, f32::from(delta.y) as f64));
                        cx.notify();
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.drag = None))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.drag = None))
                .on_mouse_up(MouseButton::Middle, cx.listener(|this, _, _, _| this.drag = None))
                .on_mouse_up_out(MouseButton::Middle, cx.listener(|this, _, _, _| this.drag = None))
                .child(SpatialCanvas::new(self.camera, &self.grid, self.grid_style).size_full()
                .child(canvas(move |bounds, _, cx| {
                    entity.update(cx, |this, cx| {
                        if this.bounds != bounds { this.bounds = bounds; cx.notify(); }
                    });
                }, |_, _, _, _| {}).absolute().size_full())
                .children(cards)))
    }
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
                    cx.on_blur(&focus, window, |this: &mut Lab, _, _| {
                        this.space_down = false;
                        this.drag = None;
                    })
                    .detach();
                    cx.observe_window_activation(window, |this: &mut Lab, window, _| {
                        if !window.is_window_active() {
                            this.space_down = false;
                            this.drag = None;
                        }
                    })
                    .detach();
                    Lab {
                        profile: options.benchmark.map(profile::Profile::new),
                        smoke: options.smoke.map(|directory| (directory, 0)),
                        grid: GridPainter::new().expect("compile canvas grid"),
                        grid_style: GridStyle::Lines,
                        scene_count: 10_000,
                        focus,
                        space_down: false,
                        camera: Camera2D::default(),
                        bounds: Bounds::default(),
                        drag: None,
                        selected: None,
                    }
                })
            },
        )
        .expect("open canvas lab");
        cx.activate(true);
    });
}
