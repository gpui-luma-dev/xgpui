//! Direct instanced companion to portable-content; the original lab stays independent.
use gpui::{prelude::*, *};
mod benchmark;
mod workload;
use workload::{cells, parameters, program, snapped};
use xgpui_shader::instanced::{ShaderProgram, WindowShaderExt, presets::EffectInstance};
actions!(
    portable_content_instanced_demo,
    [Replace, Density, Opacity, Clip, Quit]
);
struct Demo {
    focus: FocusHandle,
    program: ShaderProgram<EffectInstance>,
    cool: bool,
    dense: bool,
    faded: bool,
    clipped: bool,
}
impl Demo {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            program: program().expect("instanced rectangle shader"),
            cool: false,
            dense: true,
            faded: false,
            clipped: false,
        }
    }
    fn replace(&mut self) {
        self.cool = !self.cool;
    }
}
impl Render for Demo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let program = self.program.clone();
        let cool = self.cool;
        let dense = self.dense;
        let faded = self.faded;
        let clipped = self.clipped;
        let content = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let clip = Bounds::new(
                    point(
                        bounds.origin.x + bounds.size.width * 0.1,
                        bounds.origin.y + bounds.size.height * 0.1,
                    ),
                    bounds.size.map(|value| value * 0.8),
                );
                let paint = |window: &mut Window| {
                    let scale = window.scale_factor();
                    let origin = snapped(Bounds::new(bounds.origin, bounds.size), scale).origin;
                    for (cell, color) in cells(bounds.size, dense, cool) {
                        let mut cell = snapped(cell, scale);
                        cell.origin += origin;
                        window
                            .paint_custom_shader(cell, &program, parameters(color))
                            .expect("finite instanced rectangle");
                    }
                };
                let paint = |window: &mut Window| {
                    if faded {
                        window.with_element_opacity(Some(0.5), paint)
                    } else {
                        paint(window)
                    }
                };
                if clipped {
                    window.with_content_mask(Some(ContentMask { bounds: clip }), paint)
                } else {
                    paint(window)
                }
            },
        )
        .w_full()
        .flex_1();
        div()
            .size_full()
            .bg(rgb(0x101820))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Replace, _, cx| {
                this.replace();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Density, _, cx| {
                this.dense = !this.dense;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Opacity, _, cx| {
                this.faded = !this.faded;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Clip, _, cx| {
                this.clipped = !this.clipped;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .gap_4()
                    .text_color(white())
                    .child(
                        div()
                            .id("replace")
                            .cursor_pointer()
                            .child("Replace warm / cool")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.replace();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("density")
                            .cursor_pointer()
                            .child(if self.dense { "512 → 1" } else { "1 → 512" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dense = !this.dense;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("opacity")
                            .cursor_pointer()
                            .child("Toggle opacity")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.faded = !this.faded;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("clip")
                            .cursor_pointer()
                            .child("Toggle clip")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clipped = !this.clipped;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().text_color(rgb(0xaabbcc)).child(
                "Instanced GPU content · same controls/layout · one compatible shader batch · no producer textures",
            ))
            .child(content)
    }
}
fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let options = benchmark::Options::parse();
    options.validate()?;
    if options.benchmark {
        return benchmark::run(&options);
    }
    gpui_platform::application().run(|cx| {
        cx.bind_keys([
            KeyBinding::new("r", Replace, None),
            KeyBinding::new("d", Density, None),
            KeyBinding::new("o", Opacity, None),
            KeyBinding::new("c", Clip, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(720.)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Instanced GPU Content · xgpui");
                {
                    let view = cx.new(Demo::new);
                    let focus = view.read(cx).focus.clone();
                    focus.focus(window, cx);
                    view
                }
            },
        )
        .expect("open portable content demo");
        cx.activate(true);
    });
    Ok(())
}
