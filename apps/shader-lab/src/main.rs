use anyhow::{Result, ensure};
use clap::{Parser, ValueEnum};
use gpui::{
    App, Bounds, ContentMask, Corners, CustomDraw, DevicePixels, IntoElement, Pixels, Render,
    Scene, Window, WindowBounds, WindowOptions, div, point, prelude::*, px, rgb, size,
};
use std::{sync::Arc, time::Instant};
use xgpui_shader::{MeshGradient, SolidColor, shader};
mod animation_clock;
mod demo;
use animation_clock::AnimationClock;
mod mesh_checks;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Effect {
    Mesh,
    Solid,
}

#[derive(Parser, Clone)]
#[command(about = "Metal custom-content correctness and CPU/readback performance lab")]
struct Options {
    #[arg(long)]
    headless: bool,
    #[arg(long, value_enum, default_value_t=Effect::Mesh)]
    effect: Effect,
    #[arg(long, default_value_t = 3)]
    grid: u32,
    /// Number of samples per patch edge (2..=64), matching the source's subdivisions parameter.
    #[arg(long, default_value_t = 18)]
    subdivisions: u32,
    /// Measure CPU encode/submit without per-frame readback; final readback drains GPU work.
    #[arg(long, requires = "headless")]
    submit_only: bool,
    #[arg(long)]
    baseline: bool,
    #[arg(long)]
    animate: bool,
    /// Run animation at one-tenth speed, like a slow-animation debugging mode.
    #[arg(long)]
    slow_animations: bool,
    /// Logical window width (physical pixels in headless mode).
    #[arg(long, default_value_t = 1280)]
    width: u32,
    #[arg(long, default_value_t = 960)]
    height: u32,
    #[arg(long, default_value_t = 1)]
    count: usize,
    #[arg(long, default_value_t = 60)]
    frames: usize,
    #[arg(long, default_value_t = 5)]
    warmup: usize,
    #[arg(long, requires = "headless")]
    png: Option<std::path::PathBuf>,
    #[arg(long, requires = "headless")]
    json: Option<std::path::PathBuf>,
}
fn producer(color: [f32; 4]) -> Arc<SolidColor> {
    Arc::new(SolidColor::new(color).unwrap())
}
fn draw(
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    opacity: f32,
    content: Arc<dyn gpui::CustomContent>,
) -> CustomDraw {
    CustomDraw {
        order: 0,
        bounds: bounds.scale(1.),
        content_mask: ContentMask {
            bounds: mask.scale(1.),
        },
        corner_radii: Corners::all(px(12.)).scale(1.),
        opacity,
        content,
        coverage: None,
    }
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

fn quad(bounds: Bounds<Pixels>, color: u32) -> gpui::Quad {
    gpui::Quad {
        bounds: bounds.scale(1.),
        content_mask: ContentMask {
            bounds: bounds.scale(1.),
        },
        background: rgb(color).into(),
        ..Default::default()
    }
}
fn verify(renderer: &mut dyn gpui::PlatformHeadlessRenderer) -> Result<()> {
    let bounds = rect(0., 0., 128., 96.);
    let red = producer([1., 0., 0., 1.]);
    let mut scene = Scene::default();
    scene.insert_primitive(quad(bounds, 0x000000));
    scene.insert_primitive(draw(
        rect(16., 16., 80., 64.),
        rect(32., 16., 64., 64.),
        0.5,
        red.clone(),
    ));
    scene.insert_primitive(quad(rect(60., 40., 12., 12.), 0xffffff));
    let length = scene.len();
    scene.finish();
    let extent = size(DevicePixels(128), DevicePixels(96));
    let image = renderer.render_scene_to_image(&scene, extent)?;
    let sample = |x, y| image.get_pixel(x, y).0;
    ensure!(sample(20, 40) == [0, 0, 0, 255], "inherited clip failed");
    ensure!(sample(94, 17) == [0, 0, 0, 255], "rounded corner failed");
    ensure!(
        sample(50, 40)[0].abs_diff(128) <= 2 && sample(50, 40)[1] == 0,
        "opacity/color failed: {:?}",
        sample(50, 40)
    );
    ensure!(
        sample(64, 44) == [255, 255, 255, 255],
        "overlay ordering failed"
    );
    let mut replay = Scene::default();
    replay.replay(0..length, &scene);
    replay.finish();
    let replayed = renderer.render_scene_to_image(&replay, extent)?;
    ensure!(image == replayed, "scene replay changed output");
    ensure!(
        red.preparations() == 1,
        "static producer did not reuse its texture"
    );
    drop(scene);
    drop(replay);
    let mut empty = Scene::default();
    empty.insert_primitive(quad(bounds, 0x000000));
    empty.finish();
    ensure!(
        renderer
            .render_scene_to_image(&empty, extent)?
            .get_pixel(50, 40)
            .0
            == [0, 0, 0, 255],
        "removed content persisted"
    );
    // Two first-use records share an unsubmitted producer command. The host
    // must submit it exactly once, and both outputs must be initialized.
    let shared = producer([0., 1., 0., 0.5]);
    let mut multiple = Scene::default();
    multiple.insert_primitive(quad(bounds, 0x000000));
    multiple.insert_primitive(draw(rect(0., 0., 48., 48.), bounds, 1., shared.clone()));
    multiple.insert_primitive(draw(rect(64., 0., 48., 48.), bounds, 1., shared.clone()));
    multiple.finish();
    let image = renderer.render_scene_to_image(&multiple, extent)?;
    ensure!(
        image.get_pixel(24, 24).0[1].abs_diff(128) <= 2
            && image.get_pixel(88, 24).0[1].abs_diff(128) <= 2,
        "shared first-use/straight alpha failed"
    );
    ensure!(shared.preparations() == 1, "shared output was regenerated");
    // Resize the producer and destination; this creates a new immutable output.
    let mut resized = Scene::default();
    resized.insert_primitive(quad(bounds, 0x000000));
    resized.insert_primitive(draw(rect(0., 0., 65., 33.), bounds, 1., shared.clone()));
    resized.finish();
    ensure!(
        renderer
            .render_scene_to_image(&resized, extent)?
            .get_pixel(30, 16)
            .0[1]
            .abs_diff(128)
            <= 2,
        "resize output failed"
    );
    ensure!(
        shared.preparations() == 2,
        "resize did not regenerate output"
    );
    // A later bad producer aborts preparation before any commands are submitted.
    // Retrying the earlier cached pending output must still initialize it.
    let pending = producer([0., 0., 1., 1.]);
    let mut failed = Scene::default();
    failed.insert_primitive(draw(rect(0., 0., 48., 48.), bounds, 1., pending.clone()));
    failed.insert_primitive(draw(
        rect(64., 0., 48., 48.),
        bounds,
        1.,
        Arc::new(InvalidOutput),
    ));
    failed.finish();
    ensure!(
        renderer.render_scene_to_image(&failed, extent).is_err(),
        "invalid output was accepted"
    );
    let mut retry = Scene::default();
    retry.insert_primitive(draw(rect(0., 0., 48., 48.), bounds, 1., pending.clone()));
    retry.finish();
    ensure!(
        renderer
            .render_scene_to_image(&retry, extent)?
            .get_pixel(24, 24)
            .0[2]
            == 255,
        "aborted preparation poisoned cache"
    );
    ensure!(
        pending.preparations() == 1,
        "pending output was unnecessarily rebuilt"
    );
    let mut clipped = Scene::default();
    clipped.insert_primitive(draw(
        rect(200., 200., 48., 48.),
        bounds,
        1.,
        pending.clone(),
    ));
    ensure!(
        clipped.custom_draws.is_empty(),
        "fully clipped draw was retained"
    );
    Ok(())
}
#[derive(Debug)]
struct InvalidOutput;
impl gpui::CustomContent for InvalidOutput {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: gpui::Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        Ok(Box::new(()))
    }
}
fn distribution(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    serde_json::json!({"median": values[values.len()/2], "p95": values[((values.len()-1) as f64 * 0.95).ceil() as usize], "max": values[values.len()-1]})
}
fn headless(options: &Options) -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?
        .ok_or_else(|| anyhow::anyhow!("no headless renderer"))?;
    verify(renderer.as_mut())?;
    mesh_checks::verify_mesh(renderer.as_mut())?;
    let extent = size(
        DevicePixels(options.width as i32),
        DevicePixels(options.height as i32),
    );
    let bounds = rect(0., 0., options.width as f32, options.height as f32);
    let solid = producer([0.2, 0.6, 0.9, 1.]);
    let mesh = Arc::new(MeshGradient::new(
        demo::morph(options.grid, 0.)?,
        options.subdivisions,
    )?);
    let mut last_solid = solid.clone();
    let mut last_mesh = mesh.clone();
    let mut generations = 0;
    let mut cpu = Vec::new();
    let mut rendering = Vec::new();
    for frame in 0..options.frames + options.warmup {
        let start = Instant::now();
        let t = if options.animate {
            frame as f32 / 60. * if options.slow_animations { 0.1 } else { 1. }
        } else {
            0.
        };
        let rgba = if options.animate {
            [0.2, 0.4 + 0.2 * t.sin(), 0.9, 1.]
        } else {
            [0.2, 0.6, 0.9, 1.]
        };
        last_solid = if options.animate {
            producer(rgba)
        } else {
            solid.clone()
        };
        last_mesh = if options.animate && options.effect == Effect::Mesh && !options.baseline {
            Arc::new(mesh.with_grid(demo::grid(options.grid, 0, 0., t)?)?)
        } else {
            mesh.clone()
        };
        let content: Arc<dyn gpui::CustomContent> = match options.effect {
            Effect::Mesh => last_mesh.clone(),
            Effect::Solid => last_solid.clone(),
        };
        let before = match options.effect {
            Effect::Mesh => last_mesh.preparations(),
            Effect::Solid => last_solid.preparations(),
        };
        let mut scene = Scene::default();
        scene.insert_primitive(quad(bounds, 0x181818));
        let columns = (options.count as f32).sqrt().ceil();
        for i in 0..options.count {
            let cell = rect(
                (i as f32 % columns) * f32::from(bounds.size.width) / columns,
                (i as f32 / columns).floor() * f32::from(bounds.size.height) / columns,
                f32::from(bounds.size.width) / columns,
                f32::from(bounds.size.height) / columns,
            );
            if options.baseline {
                let mut native = quad(cell, 0x3399e6);
                native.corner_radii = Corners::all(px(12.)).scale(1.);
                native.background = gpui::Rgba {
                    r: rgba[0],
                    g: rgba[1],
                    b: rgba[2],
                    a: rgba[3],
                }
                .into();
                scene.insert_primitive(native);
            } else {
                scene.insert_primitive(draw(cell, bounds, 1., content.clone()));
            }
        }
        scene.finish();
        let recorded = start.elapsed().as_secs_f64() * 1000.;
        let start = Instant::now();
        let image = if options.submit_only {
            renderer.render_scene(&scene, extent)?;
            None
        } else {
            Some(renderer.render_scene_to_image(&scene, extent)?)
        };
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        generations += match options.effect {
            Effect::Mesh => last_mesh.preparations(),
            Effect::Solid => last_solid.preparations(),
        } - before;
        if frame >= options.warmup {
            cpu.push(recorded);
            rendering.push(elapsed);
        }
        if frame + 1 == options.frames + options.warmup
            && let Some(path) = &options.png
        {
            if let Some(image) = &image {
                image.save(path)?;
            } else {
                renderer.render_scene_to_image(&scene, extent)?.save(path)?;
            }
        }
    }
    if options.submit_only {
        let mut drain = Scene::default();
        drain.insert_primitive(quad(bounds, 0));
        drain.finish();
        renderer.render_scene_to_image(&drain, extent)?;
    }
    let info = match options.effect {
        Effect::Mesh => last_mesh.cached_texture_info(),
        Effect::Solid => last_solid.cached_texture_info(),
    };
    let stats = last_mesh.stats();
    let report = serde_json::json!({"backend":"Metal", "effect":format!("{:?}",options.effect),"producer_device":info.as_ref().map(|i| &i.0),
        "current_cached_texture_bytes":info.as_ref().map(|i| i.1),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"optimized":!cfg!(debug_assertions),
        "width":options.width,"height":options.height,"scale":1,"count":options.count,"baseline":options.baseline,
        "baseline_description":"native solid quad; not a mesh-equivalent rendering", "animated":options.animate,"animation_speed":if options.slow_animations {0.1} else {1.0},"frames":options.frames,"warmup":options.warmup,
        "grid":options.grid,"subdivisions":options.subdivisions,"triangles_per_output":if options.effect==Effect::Mesh {stats.triangles} else {0},
        "mesh_geometry_bytes_per_generation":if options.effect==Effect::Mesh {stats.buffer_bytes} else {0},
        "texture_generations_including_warmup":generations,"pipeline_compilations":last_mesh.pipeline_compilations(),
        "scene_record_ms":distribution(cpu),"render_cpu_wall_ms":distribution(rendering),"submit_only":options.submit_only,
        "timing_scope":if options.submit_only {"CPU encode/submit; may include queue backpressure; final drain excluded; not GPU time or FPS"} else {"CPU wall time including GPU wait/readback; not GPU-only time or presentation cadence"},
        "correctness":"solid integration and independent mesh Hermite reference, coverage, snapshot, resize, replay and pipeline-cache checks passed"});
    println!("{}", serde_json::to_string_pretty(&report)?);
    if let Some(path) = &options.json {
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
    }
    Ok(())
}
struct Lab {
    mesh: Arc<MeshGradient>,
    clock: AnimationClock,
    animate: bool,
    slow_animations: bool,
    progress: f32,
    grid: u32,
    slider_bounds: Bounds<Pixels>,
    dragging: bool,
}
impl Lab {
    fn advance(&mut self) {
        let elapsed = self
            .clock
            .advance(Instant::now(), self.animate, self.slow_animations);
        self.progress = (elapsed / 4.).clamp(0., 1.);
        if self.progress >= 1. {
            self.animate = false;
        }
    }
    fn scrub(&mut self, x: Pixels) {
        self.progress =
            ((x - self.slider_bounds.origin.x) / self.slider_bounds.size.width).clamp(0., 1.);
        self.clock.seek(self.progress * 4., Instant::now());
        self.animate = false;
    }
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        self.advance();
        if self.animate {
            window.request_animation_frame();
        }
        let grid = demo::morph(self.grid, self.progress).expect("valid keyframes");
        if self.mesh.grid().points() != grid.points() {
            self.mesh = Arc::new(self.mesh.with_grid(grid).expect("bounded mesh"));
        }
        let points = self.mesh.grid().points().to_vec();
        let width = self.grid;
        let overlay = gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let positions: Vec<_> = points
                    .iter()
                    .map(|p| {
                        point(
                            bounds.origin.x + bounds.size.width * p.position[0],
                            bounds.origin.y + bounds.size.height * p.position[1],
                        )
                    })
                    .collect();
                let mut path = gpui::PathBuilder::stroke(px(1.));
                for y in 0..width {
                    for x in 0..width {
                        let i = (y * width + x) as usize;
                        if x + 1 < width {
                            path.move_to(positions[i]);
                            path.line_to(positions[i + 1]);
                        }
                        if y + 1 < width {
                            path.move_to(positions[i]);
                            path.line_to(positions[i + width as usize]);
                        }
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, gpui::rgba(0xffffff80));
                }
                for position in positions {
                    for (radius, color) in [
                        (7., rgb(0x181818)),
                        (5., rgb(0xffffff)),
                        (2., rgb(0x181818)),
                    ] {
                        let mut quad = gpui::fill(
                            Bounds::new(
                                position - point(px(radius), px(radius)),
                                size(px(radius * 2.), px(radius * 2.)),
                            ),
                            color,
                        );
                        quad.corner_radii = Corners::all(px(radius));
                        window.paint_quad(quad);
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let entity = cx.entity().downgrade();
        let slider_measure = gpui::canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, _| this.slider_bounds = bounds);
            },
            |_, _, _, _| (),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let button = |id, label: String| {
            div()
                .id(id)
                .px_3()
                .py_2()
                .rounded_md()
                .bg(rgb(0x343434))
                .cursor_pointer()
                .child(label)
        };
        div().size_full().flex().flex_col().gap_4().p_6().bg(rgb(0x181818)).text_color(rgb(0xffffff))
            .child(div().text_xl().child("Mesh morph"))
            .child("Warm / upper-left → Cool / lower-right · circles are the mesh control points")
            .child(div().flex().items_center().gap_4()
                .child(button("play", if self.animate { "Pause" } else if self.progress >= 1. { "Replay" } else { "Play" }.into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advance();
                        if this.progress >= 1. { this.progress = 0.; this.clock.seek(0., Instant::now()); }
                        this.animate = !this.animate;
                        cx.notify();
                    })))
                .child(div().child("A"))
                .child(div().id("progress").relative().w(px(480.)).h(px(32.)).cursor_pointer()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| { this.dragging = true; this.scrub(event.position.x); cx.notify(); }))
                    .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| { if this.dragging { this.scrub(event.position.x); cx.notify(); } }))
                    .on_mouse_up(gpui::MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = false))
                    .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = false))
                    .child(div().absolute().top(px(14.)).h(px(4.)).w_full().rounded_full().bg(rgb(0x505050)))
                    .child(div().absolute().top(px(14.)).h(px(4.)).w(px(480. * self.progress)).rounded_full().bg(rgb(0x89c5ff)))
                    .child(div().absolute().left(px(472. * self.progress)).top(px(8.)).size(px(16.)).rounded_full().bg(rgb(0xffffff)))
                    .child(slider_measure))
                .child(format!("B   {:3.0}%", self.progress * 100.))
                .child(button("slow", format!("Slow animations: {}", if self.slow_animations { "on · 40s" } else { "off · 4s" }))
                    .on_click(cx.listener(|this, _, _, cx| { this.advance(); this.slow_animations = !this.slow_animations; cx.notify(); }))))
            .child(div().id("preview-scroll").flex_1().overflow_y_scroll().p_3()
                .child(div().relative().w(px(960.)).h(px(640.))
                    .child(shader(self.mesh.clone()).size_full())
                    .child(overlay)))
            .child(div().text_sm().text_color(rgb(0xaaaaaa)).child("Play runs A → B once. Drag the slider to pause and inspect any point in the transition."))
    }
}
fn main() -> Result<()> {
    let options = Options::parse();
    ensure!(
        (64..=4096).contains(&options.width) && (64..=4096).contains(&options.height),
        "extent must be 64..4096"
    );
    ensure!(
        (1..=256).contains(&options.count)
            && (1..=10000).contains(&options.frames)
            && options.warmup <= 1000,
        "invalid workload size"
    );
    ensure!((2..=8).contains(&options.grid), "lab grid must be 2..=8");
    let mesh = Arc::new(MeshGradient::new(
        demo::morph(options.grid, 0.)?,
        options.subdivisions,
    )?);
    if options.headless {
        return headless(&options);
    }
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(
            None,
            size(px(options.width as f32), px(options.height as f32)),
            cx,
        );
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("XGPUI Shader Lab");
                cx.new(|_| Lab {
                    mesh,
                    grid: options.grid,
                    animate: options.animate,
                    slow_animations: options.slow_animations,
                    progress: 0.,
                    slider_bounds: Bounds::default(),
                    dragging: false,
                    clock: AnimationClock::new(Instant::now()),
                })
            },
        )
        .expect("open shader-lab");
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metal_pixels_replay_cache_and_failures() -> Result<()> {
        let mut renderer = gpui_platform::current_headless_renderer()?.expect("Metal renderer");
        verify(renderer.as_mut())
    }
    #[test]
    fn metal_mesh_matches_cpu_and_preserves_snapshots() -> Result<()> {
        let mut renderer = gpui_platform::current_headless_renderer()?.expect("Metal renderer");
        mesh_checks::verify_mesh(renderer.as_mut())
    }
}
