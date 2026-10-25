mod benchmark;
mod camera;
mod gallery;
mod scene;
use anyhow::{Context as _, Result, ensure};
use camera::Camera;
use gpui::{prelude::*, *};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use xgpui_svg::{MIB, RasterImage as Raster, RasterPolicy, SvgDocument, SvgStatus, svg_image};

const ZOOMS: &[f32] = &[0.25, 0.5, 1., 2., 4., 8., 16.];
fn raster(
    renderer: &SvgRenderer,
    parsed: &ParsedSvg,
    pixels: Size<DevicePixels>,
) -> Result<Raster> {
    Raster::render(renderer, parsed, pixels, 256 * MIB)
}
fn raster_size(logical: Size<f32>, zoom: f32, density: f32) -> Size<DevicePixels> {
    RasterPolicy::default()
        .resolution(logical, zoom, density)
        .expect("valid lab size")
        .output
}
fn display_size(parsed: &ParsedSvg) -> Size<f32> {
    let intrinsic = parsed.size();
    if intrinsic.width >= intrinsic.height {
        size(320., 320. * (intrinsic.height / intrinsic.width))
    } else {
        size(320. * (intrinsic.width / intrinsic.height), 320.)
    }
}
struct Imported {
    parsed: SvgDocument,
    fixed: Raster,
    logical: Size<f32>,
    parse_ms: f64,
    path: PathBuf,
}
fn import_svg(renderer: &SvgRenderer, path: PathBuf) -> Result<Imported> {
    use std::io::Read;
    let file =
        std::fs::File::open(&path).with_context(|| format!("Cannot open {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 32 * 1024 * 1024,
        "SVG exceeds the 32 MiB input limit"
    );
    let start = Instant::now();
    let parsed = SvgDocument::parse(renderer, &bytes).context("Cannot parse SVG")?;
    let parse_ms = start.elapsed().as_secs_f64() * 1000.;
    let logical = parsed.fit_size(320.)?;
    let fixed = raster(renderer, &parsed, raster_size(logical, 1., 1.))?;
    Ok(Imported {
        parsed,
        fixed,
        logical,
        parse_ms,
        path,
    })
}
struct Lab {
    camera: Camera,
    panel_bounds: [Bounds<Pixels>; 3],
    drag_position: Option<Point<Pixels>>,
    clipped: bool,
    shadow: bool,
    parsed: SvgDocument,
    parse_ms: f64,
    fixed: Raster,
    policy: RasterPolicy,
    svg_status: Option<SvgStatus>,

    source: Option<PathBuf>,
    loading: bool,
    load_request: usize,
    load_error: Option<String>,
    light_canvas: bool,
}
impl Lab {
    fn new(cx: &App) -> Result<Self> {
        let renderer = cx.svg_renderer();
        let start = Instant::now();
        let parsed = SvgDocument::parse(&renderer, scene::svg(true, true).as_bytes())?;
        let parse_ms = start.elapsed().as_secs_f64() * 1000.;
        Ok(Self {
            fixed: raster(
                &renderer,
                &parsed,
                size(DevicePixels(320), DevicePixels(240)),
            )?,
            parsed,
            parse_ms,
            camera: Camera::default(),
            panel_bounds: [Bounds::default(); 3],
            drag_position: None,
            clipped: true,
            shadow: true,
            policy: RasterPolicy::default(),
            svg_status: None,
            source: None,
            loading: false,
            load_request: 0,
            load_error: None,
            light_canvas: false,
        })
    }
    fn rebuild(&mut self, cx: &App) -> Result<()> {
        let renderer = cx.svg_renderer();
        let start = Instant::now();
        let parsed =
            SvgDocument::parse(&renderer, scene::svg(self.clipped, self.shadow).as_bytes())?;
        let parse_ms = start.elapsed().as_secs_f64() * 1000.;
        let fixed = raster(
            &renderer,
            &parsed,
            size(DevicePixels(320), DevicePixels(240)),
        )?;
        self.parsed = parsed;
        self.parse_ms = parse_ms;
        self.fixed = fixed;
        self.svg_status = None;
        Ok(())
    }
}
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if active { 0x4756b8 } else { 0x253047 }))
        .child(label.into())
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let camera = self.camera;
        let zoom = camera.zoom;
        let mut controls = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child("Zoom");
        for (i, &z) in ZOOMS.iter().enumerate() {
            controls = controls.child(
                button(
                    ElementId::Integer(i as u64),
                    format!("{}%", z * 100.),
                    (z - zoom).abs() < 0.001,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.camera
                        .zoom_at(point(px(0.), px(0.)), z / this.camera.zoom);
                    cx.notify();
                })),
            );
        }
        if self.source.is_none() {
            controls = controls
                .child(
                    button(
                        "clip",
                        if self.clipped { "Clip on" } else { "Clip off" },
                        self.clipped,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clipped = !this.clipped;
                        this.load_error = this.rebuild(cx).err().map(|e| e.to_string());
                        cx.notify();
                    })),
                )
                .child(
                    button(
                        "shadow",
                        if self.shadow {
                            "Shadow on"
                        } else {
                            "Shadow off"
                        },
                        self.shadow,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.shadow = !this.shadow;
                        this.load_error = this.rebuild(cx).err().map(|e| e.to_string());
                        cx.notify();
                    })),
                );
        }
        controls = controls.child(button("reset-camera", "Reset view", false).on_click(
            cx.listener(|this, _, _, cx| {
                this.camera = Camera {
                    scene_size: this.camera.scene_size,
                    ..Camera::default()
                };
                this.drag_position = None;
                cx.notify();
            }),
        ));
        controls = controls.child(
            button(
                "canvas-background",
                if self.light_canvas {
                    "Canvas: light"
                } else {
                    "Canvas: dark"
                },
                false,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.light_canvas = !this.light_canvas;
                cx.notify();
            })),
        );
        for mib in [16, 32] {
            controls = controls.child(
                button(
                    ElementId::Name(format!("budget-{mib}").into()),
                    format!("{mib} MiB"),
                    self.policy.max_output_bytes == mib * MIB,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.policy.max_output_bytes = mib * MIB;
                    cx.notify();
                })),
            );
        }
        let source_controls = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                button(
                    "open-svg",
                    if self.loading {
                        "Loading…"
                    } else {
                        "Open SVG…"
                    },
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.loading {
                        return;
                    }
                    this.loading = true;
                    this.load_request += 1;
                    let request = this.load_request;
                    this.load_error = None;
                    let picker = cx.prompt_for_paths(PathPromptOptions {
                        files: true,
                        directories: false,
                        multiple: false,
                        prompt: Some("Open SVG artwork".into()),
                    });
                    let renderer = cx.svg_renderer();
                    cx.spawn(async move |this, cx| {
                        let result: Result<Option<Imported>> = async {
                            let selected =
                                picker.await.context("File picker closed unexpectedly")??;
                            let Some(path) = selected.and_then(|paths| paths.into_iter().next())
                            else {
                                return Ok(None);
                            };
                            cx.background_spawn(async move { import_svg(&renderer, path) })
                                .await
                                .map(Some)
                        }
                        .await;
                        let _ = this.update(cx, |this, cx| {
                            if request != this.load_request {
                                return;
                            }
                            this.loading = false;
                            match result {
                                Ok(Some(imported)) => {
                                    this.parsed = imported.parsed;
                                    this.parse_ms = imported.parse_ms;
                                    this.fixed = imported.fixed;
                                    this.svg_status = None;
                                    this.camera = Camera {
                                        scene_size: imported.logical,
                                        ..Camera::default()
                                    };
                                    this.source = Some(imported.path);
                                    this.light_canvas = true;
                                    this.drag_position = None;

                                    this.load_error = None;
                                }
                                Ok(None) => (),
                                Err(e) => this.load_error = Some(format!("{e:#}")),
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                    cx.notify();
                })),
            )
            .child(
                button("builtin", "Built-in scene", self.source.is_none()).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.load_request += 1;
                        this.loading = false;
                        match this.rebuild(cx) {
                            Ok(()) => {
                                this.source = None;
                                this.camera = Camera::default();
                                this.drag_position = None;
                                this.load_error = None;
                            }
                            Err(e) => this.load_error = Some(e.to_string()),
                        }
                        cx.notify();
                    },
                )),
            )
            .child(self.source.as_ref().map_or_else(
                || "Built-in ribbon and leaf".to_string(),
                |path| path.display().to_string(),
            ));
        let mut panels = div().flex().gap_3().w_full();
        for mode in 0..3 {
            let entity = cx.entity().downgrade();
            let image = self.fixed.image.clone();
            let builtin = self.source.is_none();
            let clipped = self.clipped;
            let shadow = self.shadow;
            let titles = [
                "01 / Fixed texture",
                "02 / Rerasterized SVG",
                "03 / GPUI paths",
            ];
            let detail = match mode {
                0 => format!(
                    "{} × {} px • {:.2} ms raster",
                    i32::from(self.fixed.image.size(0).width),
                    i32::from(self.fixed.image.size(0).height),
                    self.fixed.ms
                ),
                1 => self
                    .svg_status
                    .as_ref()
                    .and_then(|status| {
                        status.displayed.map(|pixels| {
                            format!(
                                "{} × {} px • {:.2} ms raster",
                                i32::from(pixels.width),
                                i32::from(pixels.height),
                                status.raster_ms.unwrap_or(0.)
                            )
                        })
                    })
                    .unwrap_or_else(|| "Preparing SVG preview".into()),
                _ => if builtin {
                    "Curves tessellated at current zoom"
                } else {
                    "Built-in scene only; imported SVG has no native path equivalent"
                }
                .into(),
            };
            let content = if mode == 1 {
                svg_image("progressive-svg", self.parsed.clone())
                    .logical_size(camera.scene_size)
                    .zoom(zoom)
                    .pan(camera.pan)
                    .policy(self.policy)
                    .on_status_change(cx.listener(|this, status: &SvgStatus, _, cx| {
                        this.svg_status = Some(status.clone());
                        cx.notify();
                    }))
                    .into_any_element()
            } else {
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            if mode == 2 && builtin {
                                scene::native(camera.origin(bounds), zoom, clipped, shadow, window);
                            } else if mode == 0 {
                                let image_size = size(
                                    px(camera.scene_size.width * zoom),
                                    px(camera.scene_size.height * zoom),
                                );
                                let image_bounds = Bounds::new(camera.origin(bounds), image_size);
                                if let Err(error) = window.paint_image(
                                    image_bounds,
                                    image_bounds,
                                    Corners::default(),
                                    image.clone(),
                                    0,
                                    false,
                                ) {
                                    eprintln!("Image paint: {error}");
                                }
                            }
                        });
                    },
                )
                .size_full()
                .into_any_element()
            };
            panels = panels.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_size(px(18.)).child(titles[mode]))
                    .child(detail)
                    .child(
                        div()
                            .id(ElementId::Name(format!("viewport-{mode}").into()))
                            .cursor_crosshair()
                            .on_scroll_wheel(cx.listener(
                                move |this, event: &ScrollWheelEvent, _, cx| {
                                    let delta = f32::from(event.delta.pixel_delta(px(24.)).y);
                                    if delta != 0. {
                                        let anchor =
                                            event.position - this.panel_bounds[mode].center();
                                        this.camera
                                            .zoom_at(anchor, (delta * 0.002).clamp(-2., 2.).exp());
                                        this.drag_position = None;
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    this.drag_position = Some(event.position);
                                    cx.stop_propagation();
                                }),
                            )
                            .h(px(450.))
                            .w_full()
                            .overflow_hidden()
                            .bg(rgb(if self.light_canvas { 0xf8fafc } else { 0x172238 }))
                            .text_color(rgb(if self.light_canvas { 0x172238 } else { 0xe0e9fb }))
                            .relative()
                            .when(mode == 2 && !builtin, |view| view.child(
                                div().absolute().top_4().left_4().right_4()
                                    .child("Native paths are available for the built-in scene. Imported SVGs are compared in the two raster views.")
                            ))
                            .child(canvas(move |bounds,_,cx| {
                                let _=entity.update(cx,|this,_|this.panel_bounds[mode]=bounds);
                            },|_,_,_,_| ()).absolute().top_0().left_0().size_full())
                            .child(content),
                    )
                    .child(if mode == 2 {
                        if builtin { "Layered shadow approximation; gradients use GPUI path bounds." } else { "Return to the built-in scene for the native path comparison." }
                    } else {
                        if builtin {
                            "Full-color SVG; clipPath and Gaussian shadow toggles above."
                        } else {
                            "Imported artwork preserves its own colors, clipping and filters."
                        }
                    }),
            );
        }
        div().id("svg-lab")
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    this.drag_position = None;
                } else if let Some(previous) = this.drag_position.replace(event.position) {
                    this.camera.pan += event.position - previous;
                    cx.notify();
                } else {
                    this.drag_position = None;
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.drag_position = None))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.drag_position = None))
            .size_full().overflow_y_scroll().bg(rgb(0x0c1424)).text_color(rgb(0xe0e9fb)).text_size(px(13.)).p_4().flex().flex_col().gap_4()
            .child(div().text_size(px(28.)).child("SVG / scale laboratory"))
            .child(if self.source.is_none() { "Shared cubic curves • two gradients • rectangular clipping • fine vein strokes • leaf shadow" } else { "Imported SVG artwork • full-color texture scaling versus zoom-aware rerasterization" })
            .child(source_controls)
            .children(self.load_error.clone().map(|e|format!("Load error: {e}")))
            .child(controls)
            .child(format!("{:.1}% • Wheel to zoom at pointer • Drag any panel to pan all three • Reset view to recenter", zoom * 100.))
            .child(self.svg_status.as_ref().map_or_else(||"Awaiting SVG element layout".into(),|status| {
                let dimensions=|pixels: Option<Size<DevicePixels>>|pixels.map_or_else(||"—".into(),|p|format!("{} × {}",i32::from(p.width),i32::from(p.height)));
                format!("Requested {} px • Budget target {} px • Displayed {} px • {:?}{}",
                    dimensions(status.resolution.map(|r|r.requested)), dimensions(status.resolution.map(|r|r.output)), dimensions(status.displayed),status.phase,
                    if status.resolution.is_some_and(|r|r.limited()) {" • Resolution limited by output budget"} else {""})
            }))
            .child(panels)
            .child(format!("Display {:.1}×  |  Parse {:.2} ms  |  SVG element builds {}",window.scale_factor(),self.parse_ms,self.svg_status.as_ref().map_or(0,|status|status.completed_rasters)))
            .child("Shared camera across all panels; zoom presets anchor at panel center. 100% fits the artwork’s longest edge to 320 logical pixels; fixed texture uses 1 source pixel per logical pixel. A preview stays visible while zoom changes; detail waits 120 ms after the latest resolution request. Output budget excludes filter intermediates and GPU memory.")
            .child("CPU raster timing includes pixel conversion; excludes GPU upload/presentation. GPUI paths rebuild each paint. Native shadow and gradient output are approximate, not pixel parity.")
            .children(self.svg_status.as_ref().and_then(|s|s.error.clone()).map(|e|format!("Render error: {e}")))
    }
}
fn main() -> Result<()> {
    let options = <benchmark::Options as clap::Parser>::parse();
    if options.gallery.is_some() {
        return gallery::run(options);
    }
    if options.benchmark.is_some() {
        return benchmark::run(options);
    }
    gpui_platform::application().run(|cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1440.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("XGPUI SVG Lab");
                cx.new(|cx| Lab::new(cx).expect("valid bundled SVG fixture"))
            },
        )
        .expect("open SVG lab");
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn loads_portrait_file_without_distortion_and_rejects_invalid_input() {
        let renderer = SvgRenderer::new(Arc::new(()));
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/portrait.svg");
        let imported = import_svg(&renderer, path).unwrap();
        assert_eq!(imported.parsed.size(), size(160., 640.));
        assert_eq!(imported.logical, size(80., 320.));
        assert_eq!(
            imported.fixed.image.size(0),
            size(DevicePixels(80), DevicePixels(320))
        );
        assert!(
            imported
                .fixed
                .image
                .as_bytes(0)
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 0)
        );
        let invalid = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(import_svg(&renderer, invalid).is_err());
        assert!(import_svg(&renderer, PathBuf::from("/missing/svg-lab-fixture.svg")).is_err());
    }
    #[::core::prelude::v1::test]
    fn bundled_artwork_renders_without_network_resources() {
        let renderer = SvgRenderer::new(Arc::new(()));
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples");
        for name in [
            "AJ_Digital_Camera",
            "rg1024_green_grapes",
            "rg1024_metal_effect",
            "dragon",
            "ribbon-leaf",
        ] {
            let bytes = std::fs::read(directory.join(format!("{name}.svg"))).unwrap();
            let document = SvgDocument::parse(&renderer, &bytes).unwrap();
            let logical = document.fit_size(128.).unwrap();
            let image = document
                .rasterize(&renderer, raster_size(logical, 1., 1.), MIB)
                .unwrap();
            assert!(
                image
                    .image
                    .as_bytes(0)
                    .unwrap()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 0),
                "empty bundled sample: {name}"
            );
        }
    }
    #[::core::prelude::v1::test]
    fn raster_budget_caps_both_axes_and_keeps_aspect_ratio() {
        for logical in [size(80., 320.), size(320., 80.), size(320., 320.)] {
            let pixels = raster_size(logical, 16., 4.);
            let w = i32::from(pixels.width);
            let h = i32::from(pixels.height);
            assert!(w > 0 && w <= 8192 && h > 0 && h <= 8192);
            assert!((w as f32 / h as f32 - logical.width / logical.height).abs() < 0.001);
        }
    }

    #[::core::prelude::v1::test]
    fn fixture_preserves_color_clip_and_shadow_at_multiple_resolutions() {
        let renderer = SvgRenderer::new(Arc::new(()));
        for width in [80, 320, 640] {
            let parsed = renderer
                .parse_svg(scene::svg(true, true).as_bytes())
                .unwrap();
            let rendered = raster(
                &renderer,
                &parsed,
                size(DevicePixels(width), DevicePixels(width * 3 / 4)),
            )
            .unwrap();
            assert_eq!(
                rendered.image.size(0),
                size(DevicePixels(width), DevicePixels(width * 3 / 4))
            );
            let data = rendered.image.as_bytes(0).unwrap();
            let w = width as usize;
            for (index, pixel) in data.as_chunks::<4>().0.iter().enumerate() {
                let x = (index % w) as f32 * 320. / width as f32;
                let y = (index / w) as f32 * 320. / width as f32;
                if x < 28. || x > 292. || y < 28. || y > 212. {
                    assert_eq!(pixel[3], 0, "clip leak at {x},{y}");
                }
            }
            assert!(
                data.as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 200 && p[0].abs_diff(p[1]) > 30),
                "full color survives"
            );
        }
        let render = |clip, shadow| {
            let parsed = renderer
                .parse_svg(scene::svg(clip, shadow).as_bytes())
                .unwrap();
            raster(
                &renderer,
                &parsed,
                size(DevicePixels(320), DevicePixels(240)),
            )
            .unwrap()
            .image
        };
        let clipped = render(true, true);
        let unclipped = render(false, true);
        let unshadowed = render(true, false);
        assert_ne!(
            clipped.as_bytes(0),
            unshadowed.as_bytes(0),
            "SVG filter has a visible effect"
        );
        let count = |image: &RenderImage| {
            image
                .as_bytes(0)
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 0)
                .count()
        };
        assert!(
            count(&unclipped) > count(&clipped),
            "clip removes visible geometry"
        );
    }
}
