use super::*;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

#[derive(Default)]
pub(super) struct Preview {
    image: Option<Arc<RenderImage>>,
    image_size: [u32; 2],
    signature: Option<u64>,
    pending: bool,
    pub capturing: bool,
    pub hidden: bool,
    last_capture: Option<Instant>,
}

fn signature(lab: &Lab, scale: f32) -> u64 {
    let mut hash = DefaultHasher::new();
    let visible = lab.camera.visible_world_bounds(lab.viewport());
    for value in [visible.min.x, visible.min.y, visible.max.x, visible.max.y] {
        value.to_bits().hash(&mut hash);
    }
    lab.scene.len().hash(&mut hash);
    lab.scene.revision().hash(&mut hash);
    lab.selected.hash(&mut hash);
    matches!(lab.grid_style, GridStyle::Dots).hash(&mut hash);
    scale.to_bits().hash(&mut hash);
    for value in [
        lab.bounds.origin.x,
        lab.bounds.origin.y,
        lab.bounds.size.width,
        lab.bounds.size.height,
    ] {
        f32::from(value).to_bits().hash(&mut hash);
    }
    if let Some(rect) = lab.marquee() {
        for value in [rect.min.x, rect.min.y, rect.max.x, rect.max.y] {
            value.to_bits().hash(&mut hash);
        }
    }
    for guide in &lab.snap_guides {
        matches!(guide.axis, xgpui_canvas::Axis::X).hash(&mut hash);
        for value in [guide.coordinate, guide.start, guide.end] {
            value.to_bits().hash(&mut hash);
        }
    }
    hash.finish()
}

pub(super) fn schedule(lab: &mut Lab, window: &mut Window, cx: &mut Context<Lab>) {
    if lab.whole_scene_map
        || !lab.show_minimap
        || lab.preview.capturing
        || lab.preview.pending
        || lab.preview.signature == Some(signature(lab, window.scale_factor()))
        || lab.bounds.size.width <= px(0.)
        || lab.bounds.size.height <= px(0.)
    {
        return;
    }
    lab.preview.pending = true;
    let entity = cx.entity();
    window.on_next_frame(move |window, cx| {
        if entity.read(cx).whole_scene_map || !entity.read(cx).show_minimap {
            entity.update(cx, |lab, _| lab.preview.pending = false);
            return;
        }
        let ready = entity
            .read(cx)
            .preview
            .last_capture
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(80));
        if ready {
            // Draw to the scene without presenting the HUD-free frame. Cropping
            // that frame gives the actual canvas, including text and effects,
            // with neither the toolbar nor a recursive preview of this panel.
            entity.update(cx, |lab, _| {
                lab.preview.capturing = true;
                lab.preview.hidden = true;
            });
            window.draw(cx).clear(cx);
            let captured = window.render_to_image();
            let bounds = entity.read(cx).bounds;
            let scale = window.scale_factor();
            entity.update(cx, |lab, cx| {
                lab.preview.hidden = false;
                match captured {
                    Ok(image) => {
                        let x = (f32::from(bounds.origin.x) * scale).max(0.).round() as u32;
                        let y = (f32::from(bounds.origin.y) * scale).max(0.).round() as u32;
                        let width = (f32::from(bounds.size.width) * scale).round().max(1.) as u32;
                        let height = (f32::from(bounds.size.height) * scale).round().max(1.) as u32;
                        if x < image.width() && y < image.height() {
                            let crop = image::imageops::crop_imm(
                                &image,
                                x,
                                y,
                                width.min(image.width() - x),
                                height.min(image.height() - y),
                            )
                            .to_image();
                            let mut pixels = image::DynamicImage::ImageRgba8(crop)
                                .thumbnail(432, 276)
                                .into_rgba8();
                            lab.preview.image_size = [pixels.width(), pixels.height()];
                            for pixel in pixels.pixels_mut() {
                                pixel.0.swap(0, 2);
                            }
                            lab.preview.image =
                                Some(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])));
                        }
                    }
                    Err(error) => eprintln!("canvas preview: {error}"),
                }
                lab.preview.signature = Some(signature(lab, scale));
                lab.preview.last_capture = Some(Instant::now());
                cx.notify();
            });
            // Restore the normal scene and hitboxes before accepting more input.
            // Neither offscreen draw advances the smoke/profile frame counters.
            window.draw(cx).clear(cx);
            entity.update(cx, |lab, cx| {
                lab.preview.capturing = false;
                lab.preview.pending = false;
                cx.notify();
            });
        } else {
            entity.update(cx, |lab, cx| {
                lab.preview.pending = false;
                cx.notify();
            });
        }
    });
    window.request_animation_frame();
}

impl Lab {
    pub(super) fn invalidate_preview(&mut self) {
        self.preview.signature = None;
        if self.whole_scene_map {
            self.refresh_overview();
        }
    }

    pub(super) fn minimap(&self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let image = self.preview.image.clone();
        let image_size = self.preview.image_size;
        div()
            .id("canvas-preview")
            .absolute()
            .right(px(12.))
            .bottom(px(12.))
            .w(px(230.))
            .h(px(170.))
            .bg(rgb(0x172238))
            .border_1()
            .border_color(rgb(0x6484a8))
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_move(cx.listener(|lab, event: &MouseMoveEvent, _, cx| {
                if lab.gesture.is_some() && event.pressed_button == Some(MouseButton::Left) {
                    lab.update_selection(event.position, event.modifiers.alt);
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|lab, event: &MouseUpEvent, _, cx| {
                    if lab.gesture.is_some() {
                        lab.finish_selection(event);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_pinch(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(px(6.))
                    .text_size(px(12.))
                    .child("Current view • T: map • M: hide"),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |lab, _| lab.map_bounds = bounds);
                    },
                    move |bounds, _, window, _| {
                        window.paint_quad(fill(bounds, rgb(0x101824)));
                        if let Some(image) = image {
                            let scale = (f32::from(bounds.size.width) / image_size[0] as f32)
                                .min(f32::from(bounds.size.height) / image_size[1] as f32);
                            let size = size(
                                px(image_size[0] as f32 * scale),
                                px(image_size[1] as f32 * scale),
                            );
                            let image_bounds = Bounds::new(
                                bounds.origin
                                    + point(
                                        (bounds.size.width - size.width) / 2.,
                                        (bounds.size.height - size.height) / 2.,
                                    ),
                                size,
                            );
                            if let Err(error) = window.paint_image(
                                bounds,
                                image_bounds,
                                Corners::default(),
                                image,
                                0,
                                false,
                            ) {
                                eprintln!("canvas preview paint: {error}");
                            }
                        }
                    },
                )
                .absolute()
                .left(px(6.))
                .right(px(6.))
                .top(px(24.))
                .bottom(px(6.)),
            )
            .into_any_element()
    }
}
