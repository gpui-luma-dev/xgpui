//! CPU image producer; no custom shader. World units map to complex units / 300.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub(super) struct Mandelbrot {
    pub enabled: bool,
    pub capture: Option<PathBuf>,
    pub refined: bool,
    generation: Arc<AtomicU64>,
    requested: Option<[u64; 6]>,
    image: Option<(Arc<RenderImage>, WorldBounds)>,
}

impl Mandelbrot {
    pub fn new(enabled: bool, capture: Option<PathBuf>) -> Self {
        Self {
            enabled,
            capture,
            ..Self::default()
        }
    }
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.requested = None;
        self.refined = false;
        self.image = None;
    }
}

fn escape(cx: f64, cy: f64, limit: u32) -> u32 {
    let (mut x, mut y) = (0., 0.);
    for i in 0..limit {
        if x * x + y * y > 4. {
            return i;
        }
        (x, y) = (x * x - y * y + cx, 2. * x * y + cy);
    }
    limit
}

fn render(
    bounds: WorldBounds,
    width: u32,
    height: u32,
    generation: &AtomicU64,
    token: u64,
) -> Option<image::RgbaImage> {
    let mut image = image::RgbaImage::new(width, height);
    for y in 0..height {
        if generation.load(Ordering::Relaxed) != token {
            return None;
        }
        for x in 0..width {
            let real = (bounds.min.x
                + (x as f64 + 0.5) / width as f64 * (bounds.max.x - bounds.min.x))
                / 300.
                - 0.5;
            let imaginary = (bounds.min.y
                + (y as f64 + 0.5) / height as f64 * (bounds.max.y - bounds.min.y))
                / 300.;
            let n = escape(real, imaginary, 256);
            let rgb = if n == 256 {
                [5, 8, 16]
            } else {
                let t = n as f64 * 0.11;
                [
                    ((t.sin() * 0.5 + 0.5) * 255.) as u8,
                    (((t + 2.).sin() * 0.5 + 0.5) * 255.) as u8,
                    (((t + 4.).sin() * 0.5 + 0.5) * 255.) as u8,
                ]
            };
            // RenderImage uses BGRA.
            image.put_pixel(x, y, image::Rgba([rgb[2], rgb[1], rgb[0], 255]));
        }
    }
    Some(image)
}

impl Lab {
    pub(super) fn schedule_mandelbrot(&mut self, cx: &mut Context<Self>) {
        let viewport = self.viewport();
        if !self.fractal.enabled || viewport.x <= 0. || viewport.y <= 0. {
            return;
        }
        let bounds = self.camera.visible_world_bounds(viewport);
        let key = [
            bounds.min.x.to_bits(),
            bounds.min.y.to_bits(),
            bounds.max.x.to_bits(),
            bounds.max.y.to_bits(),
            viewport.x.to_bits(),
            viewport.y.to_bits(),
        ];
        if self.fractal.requested == Some(key) {
            return;
        }
        self.fractal.requested = Some(key);
        self.fractal.refined = false;
        let generation = self.fractal.generation.clone();
        let token = generation.fetch_add(1, Ordering::Relaxed) + 1;
        cx.spawn(async move |this, cx| {
            for width in [240_u32, 960] {
                if width == 960 {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(180))
                        .await;
                }
                if generation.load(Ordering::Relaxed) != token {
                    return;
                }
                let height =
                    ((width as f64 * viewport.y / viewport.x).round() as u32).clamp(1, 960);
                let cancel = generation.clone();
                let pixels = cx
                    .background_executor()
                    .spawn(async move { render(bounds, width, height, &cancel, token) })
                    .await;
                let Some(pixels) = pixels else {
                    return;
                };
                if this
                    .update(cx, |lab, cx| {
                        if lab.fractal.enabled
                            && lab.fractal.generation.load(Ordering::Relaxed) == token
                        {
                            lab.fractal.refined = width == 960;
                            lab.fractal.image = Some((
                                Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
                                bounds,
                            ));
                            // Current-view capture must include the newly completed image.
                            lab.invalidate_preview();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    pub(super) fn fractal_element(&self) -> AnyElement {
        let image = self.fractal.image.clone();
        let camera = self.camera;
        let viewport = self.viewport();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                window.paint_quad(fill(bounds, rgb(0x050810)));
                if let Some((image, world)) = image {
                    let a = camera.world_to_screen(world.min, viewport);
                    let b = camera.world_to_screen(world.max, viewport);
                    let target = Bounds::new(
                        bounds.origin + point(px(a.x as f32), px(a.y as f32)),
                        size(px((b.x - a.x) as f32), px((b.y - a.y) as f32)),
                    );
                    if let Err(error) =
                        window.paint_image(bounds, target, Corners::default(), image, 0, false)
                    {
                        eprintln!("Mandelbrot image: {error}");
                    }
                }
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn interior_exterior_and_cancellation() {
        assert_eq!(escape(0., 0., 256), 256);
        assert_eq!(escape(-1., 0., 256), 256);
        assert!(escape(2., 2., 256) < 4);
        let bounds = WorldBounds {
            min: WorldPoint::new(-600., -300.),
            max: WorldPoint::new(600., 300.),
        };
        assert!(render(bounds, 4, 4, &AtomicU64::new(2), 1).is_none());
        assert_eq!(
            render(bounds, 4, 3, &AtomicU64::new(1), 1)
                .unwrap()
                .dimensions(),
            (4, 3)
        );
    }
}
