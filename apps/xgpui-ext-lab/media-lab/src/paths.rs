use crate::{model::Lab, ui::button};
use anyhow::{Result, ensure};
use gpui::{prelude::*, *};
use std::{path::Path as FilePath, time::Instant};
use xgpui_svg::path_effects::{Contour, MeasuredPath, PathEffect};

pub fn samples() -> Result<Vec<MeasuredPath>> {
    let p = |x, y| point(px(x), px(y));
    [
        vec![
            Contour::new(p(20., 100.))
                .cubic_to(p(60., -30.), p(140., 210.), p(190., 85.))
                .quadratic_to(p(250., -20.), p(300., 100.)),
        ],
        vec![
            Contour::new(p(160., 150.))
                .cubic_to(p(120., 115.), p(20., 60.), p(70., 30.))
                .cubic_to(p(105., 5.), p(145., 20.), p(160., 50.))
                .cubic_to(p(175., 20.), p(215., 5.), p(250., 30.))
                .cubic_to(p(300., 60.), p(200., 115.), p(160., 150.))
                .close(),
        ],
        vec![
            Contour::new(p(20., 45.))
                .line_to(p(110., 45.))
                .line_to(p(110., 120.))
                .line_to(p(20., 120.))
                .close(),
            Contour::new(p(170., 120.))
                .quadratic_to(p(170., 5.), p(230., 45.))
                .cubic_to(p(300., -5.), p(320., 120.), p(250., 135.)),
        ],
    ]
    .into_iter()
    .map(|c| MeasuredPath::new(&c, px(0.05)))
    .collect()
}
fn effect(trim: f32, dash: usize, phase: f32) -> Result<PathEffect> {
    let e = PathEffect::new().trim(0. ..trim)?;
    match dash {
        0 => Ok(e),
        1 => e.dash(&[px(16.), px(8.)], px(phase)),
        2 => e.dash(&[px(4.), px(7.)], px(phase)),
        _ => e.dash(&[px(18.), px(5.), px(3.)], px(phase)),
    }
}
fn place(mut path: Path<Pixels>, offset: Point<Pixels>, scale: f32) -> Path<Pixels> {
    for v in &mut path.vertices {
        v.xy_position = offset + v.xy_position * scale;
    }
    path.bounds.origin = offset + path.bounds.origin * scale;
    path.bounds.size = path.bounds.size.map(|v| v * scale);
    path
}
fn panel(path: &MeasuredPath, effect: PathEffect, width: f32, color: u32, height: f32) -> Div {
    let built = path.stroke(&effect, px(width));
    let error = built.as_ref().err().map(ToString::to_string);
    let built = built.ok();
    div()
        .flex_1()
        .min_w_0()
        .h(px(height))
        .rounded_lg()
        .bg(rgb(0x191f23))
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    if let Some(path) = &built {
                        let scale = (f32::from(bounds.size.width) / 340.)
                            .min(f32::from(bounds.size.height) / 180.);
                        let offset = bounds.origin
                            + point(
                                (bounds.size.width - px(320. * scale)) * 0.5,
                                (bounds.size.height - px(160. * scale)) * 0.5,
                            );
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            window.paint_path(place(path.clone(), offset, scale), rgb(color))
                        });
                    }
                },
            )
            .size_full(),
        )
        .children(error.map(|e| div().text_sm().text_color(rgb(0xffa99c)).child(e)))
}
pub fn render(lab: &mut Lab, window: &mut Window, cx: &mut Context<Lab>) -> Stateful<Div> {
    let animated = lab.path_animate && !cx.reduce_motion();
    if animated {
        window.request_animation_frame();
    }
    let time = lab.started.elapsed().as_secs_f32();
    let trim = if animated {
        0.15 + 0.85 * (time * 0.6).sin().mul_add(0.5, 0.5)
    } else {
        [1., 0.75, 0.5, 0.25, 0.][lab.path_trim]
    };
    let phase = lab.path_phase + if animated { time * 24. } else { 0. };
    let width = [4., 8., 14.][lab.path_width];
    let row_height = ((f32::from(window.viewport_size().height) - 390.) / 3.).clamp(120., 190.);
    let controls = div()
        .flex()
        .gap_2()
        .items_center()
        .child(
            button("path-back", "← Images", false).on_click(cx.listener(|this, _, _, cx| {
                this.path_mode = false;
                cx.notify();
            })),
        )
        .child(
            button(
                "path-trim",
                format!("Trim {:.0}%", trim * 100.),
                lab.path_trim != 0,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.path_trim = (this.path_trim + 1) % 5;
                cx.notify();
            })),
        )
        .child(
            button(
                "path-dash",
                ["Solid", "Dash 16 / 8", "Dash 4 / 7", "Odd dash 18 / 5 / 3"][lab.path_dash],
                lab.path_dash != 0,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.path_dash = (this.path_dash + 1) % 4;
                cx.notify();
            })),
        )
        .child(
            button("path-phase", format!("Phase {:.0}", lab.path_phase), false).on_click(
                cx.listener(|this, _, _, cx| {
                    this.path_phase = (this.path_phase + 5.).rem_euclid(60.);
                    cx.notify();
                }),
            ),
        )
        .child(
            button("path-width", format!("Stroke {width:.0}"), false).on_click(cx.listener(
                |this, _, _, cx| {
                    this.path_width = (this.path_width + 1) % 3;
                    cx.notify();
                },
            )),
        )
        .child(
            button(
                "path-animate",
                if lab.path_animate {
                    "Pause animation"
                } else {
                    "Animate trim / phase"
                },
                lab.path_animate,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.path_animate = !this.path_animate;
                this.started = Instant::now();
                cx.notify();
            })),
        );
    let labels = div()
        .flex()
        .gap_4()
        .child(div().flex_1().child("01 / ORIGINAL CONTOURS"))
        .child(div().flex_1().child("02 / TRIMMED"))
        .child(div().flex_1().child("03 / TRIM + DASH"));
    let mut root = div()
        .id("path-lab")
        .size_full()
        .overflow_y_scroll()
        .p_6()
        .flex()
        .flex_col()
        .gap_4()
        .bg(rgb(0x111619))
        .text_color(rgb(0xe8edeb))
        .font_family(if cfg!(target_os = "windows") {
            "Segoe UI"
        } else {
            "Helvetica"
        })
        .child(
            div()
                .text_size(px(28.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Path effects"),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(0x94a19f))
                .child("Retained Bézier contours · Arc-length trim · Native stroke tessellation"),
        )
        .child(controls)
        .child(labels.text_xs().text_color(rgb(0x94a19f)));
    for (i, path) in lab.path_samples.iter().enumerate() {
        root = root.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_xs().text_color(rgb(0x94a19f)).child(format!(
                    "{} · {:.1} logical px",
                    [
                        "Open quadratic + cubic",
                        "Closed heart / seam joins",
                        "Multiple contours / global trim"
                    ][i],
                    f32::from(path.length())
                )))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(panel(path, PathEffect::new(), width, 0x63716f, row_height))
                        .child(panel(
                            path,
                            effect(trim, 0, 0.).expect("valid trim"),
                            width,
                            0x9ee3bd,
                            row_height,
                        ))
                        .child(panel(
                            path,
                            effect(trim, lab.path_dash, phase).expect("valid dash"),
                            width,
                            0xc6b1ff,
                            row_height,
                        )),
                ),
        );
    }
    root.child(div().text_xs().text_color(rgb(0x94a19f)).child(if cx.reduce_motion(){"Reduced motion is enabled; animation is paused."}else{"Trim follows total path length across contours. Dash phase restarts per retained contour. Positive phase advances the pattern; odd arrays repeat. Cached length tolerance: 0.05 logical px per leaf."}))
}

pub fn capture(png: &FilePath, wgpu: bool) -> Result<()> {
    let mut renderer: Box<dyn PlatformHeadlessRenderer> = if wgpu {
        #[cfg(feature = "wgpu")]
        {
            Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)
        }
        #[cfg(not(feature = "wgpu"))]
        {
            anyhow::bail!("wgpu capture requires --features wgpu")
        }
    } else {
        gpui_platform::current_headless_renderer()?
            .ok_or_else(|| anyhow::anyhow!("no native headless renderer"))?
    };
    let samples = samples()?;
    let mut scene = Scene::default();
    let extent = size(DevicePixels(1200), DevicePixels(660));
    let frame = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(1200.), ScaledPixels(660.)),
    );
    scene.insert_primitive(Quad {
        bounds: frame,
        content_mask: ContentMask { bounds: frame },
        background: rgb(0x111619).into(),
        ..Default::default()
    });
    for (row, path) in samples.iter().enumerate() {
        for col in 0..3 {
            let e = effect(
                if col == 0 { 1. } else { 0.68 },
                if col == 2 { 1 } else { 0 },
                5.,
            )?;
            let native = place(
                path.stroke(&e, px(6.))?,
                point(px(col as f32 * 400. + 40.), px(row as f32 * 220. + 30.)),
                1.,
            );
            let mut physical = native.scale(1.);
            physical.content_mask = ContentMask { bounds: frame };
            physical.color = rgb([0x63716f, 0x9ee3bd, 0xc6b1ff][col]).into();
            scene.insert_primitive(physical);
        }
    }
    scene.finish();
    let pixels = renderer.render_scene_to_image(&scene, extent)?;
    for row in 0..3 {
        let mut counts = [0usize; 3];
        for (col, count) in counts.iter_mut().enumerate() {
            for y in row * 220..(row + 1) * 220 {
                for x in col * 400..(col + 1) * 400 {
                    let p = pixels.get_pixel(x as u32, y as u32).0;
                    if p[0].abs_diff(17) > 15 || p[1].abs_diff(22) > 15 || p[2].abs_diff(25) > 15 {
                        *count += 1;
                    }
                }
            }
        }
        ensure!(
            counts[0] > 200 && counts[1] > 100 && counts[2] > 50,
            "missing path pixels: {counts:?}"
        );
        ensure!(
            counts[0] > counts[1] && counts[1] > counts[2],
            "trim/dash coverage must decrease: {counts:?}"
        );
    }
    pixels.save(png)?;
    println!(
        "Saved {}: columns original / trim 68% / trim + dash; rows open curve / closed heart / multiple contours.",
        png.display()
    );
    Ok(())
}
