//! Portable content window demo. Application code imports no backend handles.
use gpui::{content::*, prelude::*, *};
#[derive(Debug)]
struct CellSource {
    index: usize,
    cool: bool,
}
impl ContentSource for CellSource {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> Result<ImageRef, ContentError> {
        let v = (self.index + 1) as f32 / 512.;
        let color = if self.cool {
            [0.1, v, 1., 1.]
        } else {
            [1., v, 0.1, 1.]
        };
        let image = cx.solid(request.extent(), ColorValue::srgb(color)?)?;
        cx.copy_image(image)
    }
}
actions!(
    portable_content_demo,
    [Replace, Density, Opacity, Clip, Quit]
);
struct Demo {
    focus: FocusHandle,
    slots: Vec<PresentationSlotId>,
    sources: Vec<Content>,
    cool: bool,
    dense: bool,
    faded: bool,
    clipped: bool,
}
impl Demo {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            slots: (0..512).map(|_| PresentationSlotId::new()).collect(),
            sources: (0..512)
                .map(|index| Content::new(CellSource { index, cool: false }))
                .collect(),
            cool: false,
            dense: true,
            faded: false,
            clipped: false,
        }
    }
    fn replace(&mut self) {
        self.cool = !self.cool;
        self.sources = (0..512)
            .map(|index| {
                Content::new(CellSource {
                    index,
                    cool: self.cool,
                })
            })
            .collect();
    }
}
impl Render for Demo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draws: Vec<_> = self
            .sources
            .iter()
            .cloned()
            .zip(self.slots.iter().copied())
            .take(if self.dense { 512 } else { 1 })
            .map(|(source, slot)| ContentDraw::new(source).slot(slot))
            .collect();
        let dense = self.dense;
        let faded = self.faded;
        let clipped = self.clipped;
        let content = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let (columns, rows) = if dense { (32, 16) } else { (1, 1) };
                let clip = Bounds::new(
                    point(
                        bounds.origin.x + bounds.size.width * 0.1,
                        bounds.origin.y + bounds.size.height * 0.1,
                    ),
                    bounds.size.map(|value| value * 0.8),
                );
                let paint = |window: &mut Window| {
                    for (index, draw) in draws.into_iter().enumerate() {
                        let width = bounds.size.width / columns as f32;
                        let height = bounds.size.height / rows as f32;
                        let cell = Bounds::new(
                            point(
                                bounds.origin.x + width * (index % columns) as f32,
                                bounds.origin.y + height * (index / columns) as f32,
                            ),
                            size(width - px(2.), height - px(2.)),
                        );
                        window.paint_content(
                            CustomGeometry::RoundedRect {
                                bounds: cell,
                                corner_radii: Corners::all(px(4.)),
                            },
                            draw,
                        );
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
                "Portable GPU content · resize and replace · completed textures stay on the GPU",
            ))
            .child(content)
    }
}
fn main() {
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
                window.set_window_title("Portable GPU Content · xgpui");
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
}
