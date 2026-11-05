//! Live controls use the same descriptor for layout and paint on either backend.
use anyhow::{Context as _, Result, ensure};
use gpui::{accesskit::Role, prelude::*, *};
use xgpui_parley::VariationPreview;

const LIMIT: usize = 512;
const SLIDER_IDS: [&str; 4] = [
    "axis-weight",
    "axis-width",
    "axis-slant",
    "axis-optical-size",
];
const PRESET_IDS: [&str; 4] = [
    "axis-default",
    "axis-condensed",
    "axis-slanted",
    "axis-wide",
];
const SPECIMEN: &str = "Hamburgefonts 012345";
const WRAPPING: &str = "Hamburgefonts 012345 · Hamburgefonts 012345 · Hamburgefonts 012345";
const CONTROLS: [(&str, &str, f32); 4] = [
    ("wght", "Weight", 10.0),
    ("wdth", "Width", 1.0),
    ("slnt", "Slant", 0.25),
    ("opsz", "Optical size", 1.0),
];

struct Axis {
    info: VariationAxisInfo,
    label: &'static str,
    step: f32,
}

pub struct Controls {
    axes: Vec<Axis>,
    values: Vec<f32>,
    preview: VariationPreview,
    font: Font,
    error: Option<String>,
}

impl Controls {
    pub fn for_backend(backend: &dyn PlatformTextSystem) -> Result<Option<Self>> {
        if !backend.font_capabilities().static_variations {
            return Ok(None);
        }
        let id = backend.font_id(&font("Roboto Flex"))?;
        let FontQuery::Supported(available) = backend.font_axes(id)? else {
            return Ok(None);
        };
        Self::new(available, LIMIT).map(Some)
    }

    fn new(available: Vec<VariationAxisInfo>, capacity: usize) -> Result<Self> {
        let axes = CONTROLS
            .iter()
            .map(|&(tag, label, step)| {
                let info = available
                    .iter()
                    .find(|axis| axis.tag.to_string() == tag)
                    .with_context(|| format!("Roboto Flex is missing {tag}"))?
                    .clone();
                ensure!(
                    info.min_value < info.max_value,
                    "axis {tag} has no usable range"
                );
                Ok(Axis { info, label, step })
            })
            .collect::<Result<Vec<_>>>()?;
        let values = axes
            .iter()
            .map(|axis| axis.info.default_value)
            .collect::<Vec<_>>();
        let mut preview = VariationPreview::new(
            font("Roboto Flex"),
            axes.iter().map(|axis| (axis.info.clone(), axis.step)),
            capacity,
        )?;
        let settings = FontVariations::new(
            axes.iter()
                .zip(&values)
                .map(|(axis, value)| (axis.info.tag, *value)),
        )?;
        let font = preview.font(&settings)?;
        Ok(Self {
            axes,
            values,
            preview,
            font,
            error: None,
        })
    }

    fn apply(&mut self, values: Vec<f32>) {
        let result = FontVariations::new(
            self.axes
                .iter()
                .zip(values)
                .map(|(axis, value)| (axis.info.tag, value)),
        )
        .and_then(|settings| self.preview.font(&settings));
        match result {
            Ok(font) => {
                self.values = self
                    .axes
                    .iter()
                    .map(|axis| {
                        font.variations
                            .settings()
                            .iter()
                            .find(|(tag, _)| *tag == axis.info.tag)
                            .unwrap()
                            .1
                    })
                    .collect();
                self.font = font;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!(
                    "Preview held: {error}. Revisit earlier settings or restart the lab for a fresh session."
                ))
            }
        }
    }

    fn set(&mut self, axis: usize, value: f32) {
        let mut values = self.values.clone();
        values[axis] = value.clamp(
            self.axes[axis].info.min_value,
            self.axes[axis].info.max_value,
        );
        self.apply(values);
    }

    fn preset(&mut self, preset: usize) {
        let values = match preset {
            1 => vec![400.0, 50.0, 0.0, 14.0],
            2 => vec![400.0, 100.0, -10.0, 14.0],
            3 => vec![700.0, 150.0, -5.0, 72.0],
            _ => self
                .axes
                .iter()
                .map(|axis| axis.info.default_value)
                .collect(),
        };
        self.apply(values);
    }
}

pub struct AxisLab {
    name: String,
    controls: Controls,
    bounds: Vec<Option<Bounds<Pixels>>>,
    focus: Vec<FocusHandle>,
    dragging: Option<usize>,
}

impl AxisLab {
    pub fn new(name: String, controls: Controls, cx: &mut Context<Self>) -> Self {
        let count = controls.axes.len();
        Self {
            name,
            controls,
            bounds: vec![None; count],
            focus: (0..count).map(|_| cx.focus_handle()).collect(),
            dragging: None,
        }
    }

    fn scrub(&mut self, axis: usize, x: Pixels) {
        let Some(bounds) = self.bounds[axis] else {
            return;
        };
        if bounds.size.width <= px(0.0) {
            return;
        }
        let fraction = ((x - bounds.left()) / bounds.size.width).clamp(0.0, 1.0);
        let info = &self.controls.axes[axis].info;
        self.controls.set(
            axis,
            info.min_value + fraction * (info.max_value - info.min_value),
        );
    }

    fn pointer_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.dragging = None;
        } else if let Some(axis) = self.dragging {
            self.scrub(axis, event.position.x);
            cx.notify();
        }
    }

    fn slider(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let axis = &self.controls.axes[index];
        let value = self.controls.values[index];
        let fraction = (value - axis.info.min_value) / (axis.info.max_value - axis.info.min_value);
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |mut bounds, _, cx| {
                bounds.origin.x += px(8.0);
                bounds.size.width -= px(16.0);
                entity
                    .update(cx, |this, _| this.bounds[index] = Some(bounds))
                    .ok();
            },
            |_, _, _, _| (),
        )
        .absolute()
        .size_full();
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .w(px(108.0))
                    .flex_shrink_0()
                    .child(format!("{} ({})", axis.label, axis.info.tag)),
            )
            .child(
                div()
                    .id(SLIDER_IDS[index])
                    .debug_selector(move || SLIDER_IDS[index].into())
                    .relative()
                    .flex_1()
                    .min_w(px(80.0))
                    .h(px(30.0))
                    .cursor_pointer()
                    .track_focus(&self.focus[index])
                    .tab_stop(true)
                    .role(Role::Slider)
                    .aria_label(SharedString::from(axis.label))
                    .aria_numeric_value(value as f64)
                    .aria_min_numeric_value(axis.info.min_value as f64)
                    .aria_max_numeric_value(axis.info.max_value as f64)
                    .on_a11y_action(AccessibleAction::Increment, {
                        let entity = cx.entity().downgrade();
                        move |_, _, cx| {
                            entity
                                .update(cx, |this, cx| {
                                    this.controls.set(
                                        index,
                                        this.controls.values[index]
                                            + this.controls.axes[index].step,
                                    );
                                    cx.notify();
                                })
                                .ok();
                        }
                    })
                    .on_a11y_action(AccessibleAction::Decrement, {
                        let entity = cx.entity().downgrade();
                        move |_, _, cx| {
                            entity
                                .update(cx, |this, cx| {
                                    this.controls.set(
                                        index,
                                        this.controls.values[index]
                                            - this.controls.axes[index].step,
                                    );
                                    cx.notify();
                                })
                                .ok();
                        }
                    })
                    .on_a11y_action(AccessibleAction::SetValue, {
                        let entity = cx.entity().downgrade();
                        move |data, _, cx| {
                            if let Some(gpui::accesskit::ActionData::NumericValue(value)) = data
                                && value.is_finite()
                            {
                                entity
                                    .update(cx, |this, cx| {
                                        this.controls.set(index, *value as f32);
                                        cx.notify();
                                    })
                                    .ok();
                            }
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.focus[index].focus(window, cx);
                            this.dragging = Some(index);
                            this.scrub(index, event.position.x);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        let axis = &this.controls.axes[index];
                        let step = axis.step
                            * if event.keystroke.modifiers.shift {
                                10.0
                            } else {
                                1.0
                            };
                        let next = match event.keystroke.key.as_str() {
                            "left" | "down" => this.controls.values[index] - step,
                            "right" | "up" => this.controls.values[index] + step,
                            "home" => axis.info.min_value,
                            "end" => axis.info.max_value,
                            _ => return,
                        };
                        this.controls.set(index, next);
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(8.0))
                            .right(px(8.0))
                            .top(px(13.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(rgb(0x4b5563))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded_full()
                                    .bg(rgb(0x60a5fa)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(relative(fraction))
                                    .ml(px(-6.0))
                                    .top(px(-4.0))
                                    .size(px(12.0))
                                    .rounded_full()
                                    .bg(rgb(0xf8fafc)),
                            ),
                    )
                    .child(measure),
            )
            .child(
                div()
                    .w(px(68.0))
                    .text_right()
                    .flex_shrink_0()
                    .child(format!("{value:.2}")),
            )
    }
}

impl Render for AxisLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let pointer = canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let moving = entity.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        moving
                            .update(cx, |this, cx| this.pointer_move(event, cx))
                            .ok();
                    }
                });
                let released = entity.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        released.update(cx, |this, _| this.dragging = None).ok();
                    }
                });
            },
        )
        .absolute()
        .size_full();
        let runs = [TextRun {
            len: SPECIMEN.len(),
            font: self.controls.font.clone(),
            ..Default::default()
        }];
        let advance = window
            .text_system()
            .layout_line(SPECIMEN, px(32.0), &runs, None)
            .width;
        let mut panel = div().id("live-axis-controls").relative().w_full().flex_none().p_3().rounded_md().bg(rgb(0x1f2937)).flex().flex_col().gap_2()
            .child(pointer)
            .child(div().text_size(px(18.0)).child(format!("{} · live Roboto Flex axes", self.name)))
            .child("Drag a slider; arrows adjust, Shift makes larger steps, Home/End reach the limits.");
        let mut presets = div().flex().flex_wrap().gap_2();
        for (index, label) in ["Default", "Condensed", "Slanted", "Wide + optical 72"]
            .into_iter()
            .enumerate()
        {
            presets = presets.child(
                div()
                    .id(PRESET_IDS[index])
                    .debug_selector(move || PRESET_IDS[index].into())
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(0x374151))
                    .cursor_pointer()
                    .role(Role::Button)
                    .aria_label(SharedString::from(label))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.controls.preset(index);
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(presets);
        for index in 0..self.controls.axes.len() {
            panel = panel.child(self.slider(index, cx));
        }
        panel
            .child(
                div()
                    .id("axis-large-specimen")
                    .text_size(px(32.0))
                    .font(self.controls.font.clone())
                    .child(SPECIMEN),
            )
            .child(
                div()
                    .id("axis-wrapping-specimen")
                    .text_size(px(16.0))
                    .font(self.controls.font.clone())
                    .child(WRAPPING),
            )
            .child(format!(
                "32px advance: {:.2}px · preview combinations: {}/{} · steps: 10 / 1 / 0.25 / 1",
                f32::from(advance),
                self.controls.preview.admitted_instances(),
                LIMIT
            ))
            .children(
                self.controls
                    .error
                    .as_ref()
                    .map(|error| div().text_color(rgb(0xfbbf24)).child(error.clone())),
            )
    }
}

#[cfg(test)]
mod tests;
