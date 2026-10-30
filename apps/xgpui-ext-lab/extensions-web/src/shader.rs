use gpui::{prelude::*, *};
use std::sync::Arc;
use web_time::Instant;
use xgpui_shader::{
    Gradient, GradientStop, MeshGradient, MeshGrid, Paint, SolidColor, WindowPaintExt, painted,
    shader_paint::{ShaderParameters, ShaderProgram},
};

pub struct ShaderDemo {
    paints: Vec<(&'static str, Paint)>,
    mesh: Arc<MeshGradient>,
    mesh_paint: Paint,
    wash: Paint,
    program: ShaderProgram,
    started: Instant,
    animated: bool,
    translucent: bool,
}

impl ShaderDemo {
    pub fn new() -> Self {
        let stops = || {
            vec![
                GradientStop::new(0.0, [0.95, 0.15, 0.35, 1.0]),
                GradientStop::new(0.45, [1.0, 0.75, 0.15, 0.6]),
                GradientStop::new(1.0, [0.1, 0.35, 0.95, 1.0]),
            ]
        };
        let paints = vec![
            (
                "Solid",
                Arc::new(SolidColor::new([0.15, 0.65, 0.9, 0.75]).unwrap()).into(),
            ),
            (
                "Linear",
                Arc::new(Gradient::linear(55.0, stops()).unwrap()).into(),
            ),
            (
                "Radial",
                Arc::new(Gradient::radial([0.5, 0.5], [0.5, 0.5], stops()).unwrap()).into(),
            ),
            (
                "Conic",
                Arc::new(Gradient::conic([0.5, 0.5], 0.0, stops()).unwrap()).into(),
            ),
            (
                "Diamond",
                Arc::new(Gradient::diamond([0.5, 0.5], [0.5, 0.5], 30.0, stops()).unwrap()).into(),
            ),
            (
                "Linear · hard stops",
                Arc::new(
                    Gradient::linear(
                        90.0,
                        vec![
                            GradientStop::new(0.0, [0.95, 0.15, 0.35, 1.0]),
                            GradientStop::new(0.5, [0.95, 0.15, 0.35, 1.0]),
                            GradientStop::new(0.5, [0.1, 0.35, 0.95, 1.0]),
                            GradientStop::new(1.0, [0.1, 0.35, 0.95, 1.0]),
                        ],
                    )
                    .unwrap(),
                )
                .into(),
            ),
            (
                "Radial · elliptical",
                Arc::new(Gradient::radial([0.5, 0.5], [0.65, 0.25], stops()).unwrap()).into(),
            ),
            (
                "Radial · off-center fade",
                Arc::new(
                    Gradient::radial(
                        [0.2, 0.25],
                        [0.8, 0.8],
                        vec![
                            GradientStop::new(0.0, [1.0, 0.85, 0.25, 1.0]),
                            GradientStop::new(0.55, [0.95, 0.15, 0.35, 0.8]),
                            GradientStop::new(1.0, [0.1, 0.35, 0.95, 0.0]),
                        ],
                    )
                    .unwrap(),
                )
                .into(),
            ),
            (
                "Conic · rainbow",
                Arc::new(
                    Gradient::conic(
                        [0.5, 0.5],
                        30.0,
                        vec![
                            GradientStop::new(0.0, [1.0, 0.1, 0.2, 1.0]),
                            GradientStop::new(0.2, [1.0, 0.8, 0.1, 1.0]),
                            GradientStop::new(0.4, [0.1, 0.85, 0.3, 1.0]),
                            GradientStop::new(0.6, [0.1, 0.65, 1.0, 1.0]),
                            GradientStop::new(0.8, [0.65, 0.15, 0.95, 1.0]),
                            GradientStop::new(1.0, [1.0, 0.1, 0.2, 1.0]),
                        ],
                    )
                    .unwrap(),
                )
                .into(),
            ),
        ];
        let mesh = Arc::new(MeshGradient::new(Self::grid(0.0), 18).unwrap());
        let program = ShaderProgram::new(include_str!("wash.wgsl")).unwrap();
        let wash = program.paint(Self::parameters(0.0)).unwrap().into();
        Self {
            paints,
            mesh_paint: mesh.clone().into(),
            mesh,
            program,
            wash,
            started: Instant::now(),
            animated: false,
            translucent: false,
        }
    }

    fn parameters(t: f32) -> ShaderParameters {
        ShaderParameters::new()
            .float("progress", t.sin() * 0.35)
            .vec3("tint", [0.95, 0.15, 0.35])
            .float("alpha", 0.85)
    }

    fn grid(t: f32) -> MeshGrid {
        let base = MeshGrid::regular(
            3,
            3,
            vec![
                [1.0, 0.1, 0.2],
                [1.0, 0.8, 0.1],
                [0.2, 0.4, 1.0],
                [0.6, 0.1, 0.8],
                [0.1, 0.8, 0.6],
                [0.1, 0.3, 0.8],
                [1.0, 0.4, 0.1],
                [0.3, 0.8, 0.9],
                [0.6, 0.2, 0.8],
            ],
        )
        .unwrap();
        let mut points = base.points().to_vec();
        points[4].position = [0.5 + 0.2 * t.sin(), 0.5 + 0.18 * (t * 0.7).sin()];
        MeshGrid::new(3, 3, points).unwrap()
    }
}

fn card(label: &'static str, paint: Paint, opacity: f32) -> impl IntoElement {
    let path_paint = paint.clone();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .w(px(270.0))
        .child(label)
        .child(
            div()
                .opacity(opacity)
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    painted()
                        .w_full()
                        .h(px(120.0))
                        .rounded_lg()
                        .bg_paint(paint.clone()),
                )
                .child(
                    painted()
                        .w_full()
                        .h(px(65.0))
                        .rounded_lg()
                        .border_8()
                        .border_paint(paint)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child("Painted border"),
                )
                .child(
                    canvas(
                        |bounds, _, _| bounds,
                        move |bounds, _, window, _| {
                            let mut path = PathBuilder::fill();
                            let origin = bounds.origin;
                            let w = bounds.size.width;
                            let h = bounds.size.height;
                            path.move_to(origin + point(w * 0.5, px(0.0)));
                            path.line_to(origin + point(w, h * 0.5));
                            path.line_to(origin + point(w * 0.5, h));
                            path.line_to(origin + point(px(0.0), h * 0.5));
                            path.close();
                            window.paint_path_with_paint(path.build().unwrap(), path_paint.clone());
                        },
                    )
                    .w_full()
                    .h(px(70.0)),
                ),
        )
}

impl Render for ShaderDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.animated {
            let t = self.started.elapsed().as_secs_f32();
            self.mesh_paint = Arc::new(self.mesh.with_grid(Self::grid(t)).unwrap()).into();
            self.wash = self.program.paint(Self::parameters(t)).unwrap().into();
            window.request_animation_frame();
        }
        let opacity = if self.translucent { 0.45 } else { 1.0 };
        div().id("shader-demo").size_full().overflow_y_scroll().p_4()
            .bg(rgb(0x111827)).text_color(rgb(0xeff4fc)).font_family("IBM Plex Sans")
            .flex().flex_col().gap_4()
            .child(div().text_xl().child("Shader paints · WebAssembly"))
            .child("Rounded backgrounds, borders and paths share each paint. Resize the browser to test caching and clipping.")
            .child(div().flex().gap_3()
                .child(div().id("animate").p_2().bg(rgb(0x374151)).cursor_pointer()
                    .child(if self.animated { "Pause animation" } else { "Animate mesh + WGSL" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.animated = !this.animated;
                        cx.notify();
                    })))
                .child(div().id("opacity").p_2().bg(rgb(0x374151)).cursor_pointer()
                    .child(if self.translucent { "Restore opacity" } else { "Group opacity 45%" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.translucent = !this.translucent;
                        cx.notify();
                    }))))
            .child(div().flex().flex_wrap().gap_4()
                .children(self.paints.iter().map(|(label, paint)| card(label, paint.clone(), opacity)))
                .child(card("Hermite mesh", self.mesh_paint.clone(), opacity))
                .child(card("Portable WGSL", self.wash.clone(), opacity)))
    }
}
