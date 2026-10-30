//! Cached CPU tiles in world space. No custom shader; complex units = world / 300.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const TILE: u32 = 256;
const CACHE_LIMIT: usize = 384;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    level: i32,
    x: i64,
    y: i64,
}
impl Key {
    fn bounds(self) -> WorldBounds {
        let span = TILE as f64 / 2_f64.powi(self.level);
        WorldBounds {
            min: WorldPoint::new(self.x as f64 * span, self.y as f64 * span),
            max: WorldPoint::new((self.x + 1) as f64 * span, (self.y + 1) as f64 * span),
        }
    }
}
fn keys(bounds: WorldBounds, level: i32) -> Vec<Key> {
    let span = TILE as f64 / 2_f64.powi(level);
    // Extremely distant pans can exceed the representable tile address domain.
    let coords = [
        bounds.min.x / span,
        bounds.max.x / span,
        bounds.min.y / span,
        bounds.max.y / span,
    ];
    if coords
        .iter()
        .any(|v| !v.is_finite() || v.abs() > (i64::MAX / 2) as f64)
    {
        return Vec::new();
    }
    let mut result = Vec::new();
    for y in coords[2].floor() as i64..coords[3].ceil() as i64 {
        for x in coords[0].floor() as i64..coords[1].ceil() as i64 {
            result.push(Key { level, x, y });
        }
    }
    let center = WorldPoint::new(
        (bounds.min.x + bounds.max.x) * 0.5,
        (bounds.min.y + bounds.max.y) * 0.5,
    );
    result.sort_by(|a, b| {
        let distance = |key: &Key| {
            let p = key.bounds().min;
            (p.x + span * 0.5 - center.x).hypot(p.y + span * 0.5 - center.y)
        };
        distance(a).total_cmp(&distance(b))
    });
    result
}
#[path = "../../shared/mandelbrot.rs"]
mod fractal_math;
#[cfg(test)]
use fractal_math::escape;
pub(super) use fractal_math::iterations;

#[derive(Default)]
pub(super) struct Mandelbrot {
    pub enabled: bool,
    pub capture: Option<PathBuf>,
    pub refined: bool,
    generation: Arc<AtomicU64>,
    requested: Option<[u64; 5]>,
    // Oldest first; touching current tiles moves them to the end.
    tiles: Vec<(Key, Arc<RenderImage>)>,
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
    }
}
fn render(key: Key, generation: &AtomicU64, token: u64) -> Option<image::RgbaImage> {
    let bounds = key.bounds();
    let step = (bounds.max.x - bounds.min.x) / TILE as f64;
    let limit = iterations(2_f64.powi(key.level));
    let mut image = image::RgbaImage::new(TILE + 2, TILE + 2);
    for y in 0..TILE + 2 {
        if generation.load(Ordering::Relaxed) != token {
            return None;
        }
        for x in 0..TILE + 2 {
            let real = (bounds.min.x + (x as f64 - 0.5) * step) / 300. - 0.5;
            let imaginary = (bounds.min.y + (y as f64 - 0.5) * step) / 300.;
            image.put_pixel(
                x,
                y,
                image::Rgba(fractal_math::bgra(real, imaginary, limit)),
            );
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
        let level = self.camera.zoom().log2().ceil() as i32;
        let signature = [
            bounds.min.x.to_bits(),
            bounds.min.y.to_bits(),
            bounds.max.x.to_bits(),
            bounds.max.y.to_bits(),
            level as u64,
        ];
        if self.fractal.requested == Some(signature) {
            return;
        }
        self.fractal.requested = Some(signature);
        let fine = keys(bounds, level);
        let coarse = keys(bounds, level - 2);
        let existing: std::collections::BTreeSet<_> =
            self.fractal.tiles.iter().map(|(k, _)| *k).collect();
        self.fractal.refined = fine.iter().all(|k| existing.contains(k));
        // Retain recently visible detail instead of replacing it with a coarse frame.
        self.fractal
            .tiles
            .sort_by_key(|(k, _)| fine.contains(k) || coarse.contains(k));
        let missing: Vec<_> = coarse
            .into_iter()
            .chain(fine.iter().copied())
            .filter(|k| !existing.contains(k))
            .collect();
        let generation = self.fractal.generation.clone();
        let token = generation.fetch_add(1, Ordering::Relaxed) + 1;
        cx.spawn(async move |this, cx| {
            for key in missing {
                if generation.load(Ordering::Relaxed) != token {
                    return;
                }
                let cancel = generation.clone();
                let pixels = cx
                    .background_executor()
                    .spawn(async move { render(key, &cancel, token) })
                    .await;
                let Some(pixels) = pixels else {
                    return;
                };
                if this
                    .update(cx, |lab, cx| {
                        if lab.fractal.enabled
                            && lab.fractal.generation.load(Ordering::Relaxed) == token
                        {
                            lab.fractal.tiles.push((
                                key,
                                Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
                            ));
                            if lab.fractal.tiles.len() > CACHE_LIMIT {
                                lab.fractal.tiles.remove(0);
                            }
                            lab.fractal.refined = fine
                                .iter()
                                .all(|k| lab.fractal.tiles.iter().any(|(cached, _)| cached == k));
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
        let camera = self.camera;
        let viewport = self.viewport();
        let visible = camera.visible_world_bounds(viewport);
        let mut tiles: Vec<_> = self
            .fractal
            .tiles
            .iter()
            .filter(|(k, _)| k.bounds().intersects(visible))
            .cloned()
            .collect();
        // Finer cached tiles always cover coarser ones, even during a zoom-out.
        tiles.sort_by_key(|(k, _)| k.level);
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                window.paint_quad(fill(bounds, rgb(0x050810)));
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for (key, image) in tiles {
                        let world = key.bounds();
                        let a = camera.world_to_screen(world.min, viewport);
                        let b = camera.world_to_screen(world.max, viewport);
                        let core = Bounds::new(
                            bounds.origin + point(px(a.x as f32), px(a.y as f32)),
                            size(px((b.x - a.x) as f32), px((b.y - a.y) as f32)),
                        );
                        // A one-texel gutter prevents atlas filtering from sampling
                        // transparent neighbours at tile boundaries.
                        let padding = core.size.width / TILE as f32;
                        let target = Bounds::new(
                            core.origin - point(padding, padding),
                            core.size + size(padding * 2., padding * 2.),
                        );
                        if let Err(error) = window.paint_image(
                            bounds.intersect(&core),
                            target,
                            Corners::default(),
                            image,
                            0,
                            false,
                        ) {
                            eprintln!("Mandelbrot tile: {error}");
                        }
                    }
                });
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
        let key = Key {
            level: 0,
            x: 0,
            y: 0,
        };
        assert!(render(key, &AtomicU64::new(2), 1).is_none());
        assert_eq!(
            render(key, &AtomicU64::new(1), 1).unwrap().dimensions(),
            (TILE + 2, TILE + 2)
        );
    }
    #[::core::prelude::v1::test]
    fn adjacent_tiles_share_gutter_samples() {
        let generation = AtomicU64::new(1);
        let a = render(
            Key {
                level: 0,
                x: -1,
                y: 0,
            },
            &generation,
            1,
        )
        .unwrap();
        let b = render(
            Key {
                level: 0,
                x: 0,
                y: 0,
            },
            &generation,
            1,
        )
        .unwrap();
        for y in 0..TILE + 2 {
            assert_eq!(a.get_pixel(TILE + 1, y), b.get_pixel(1, y));
            assert_eq!(a.get_pixel(TILE, y), b.get_pixel(0, y));
        }
    }
    #[::core::prelude::v1::test]
    fn tiles_cover_negative_coordinates_and_reuse_on_pan_at_deep_zoom() {
        for level in [-7, 0, 7, 33] {
            let span = TILE as f64 / 2_f64.powi(level);
            let bounds = WorldBounds {
                min: WorldPoint::new(-1.5 * span, -0.5 * span),
                max: WorldPoint::new(1.5 * span, 0.5 * span),
            };
            let a = keys(bounds, level);
            assert_eq!(a.len(), 8);
            let b = keys(bounds.translated(WorldPoint::new(span * 0.1, 0.)), level);
            assert_eq!(a.iter().filter(|k| b.contains(k)).count(), 8);
            assert_eq!(a[0].bounds().max.x - a[0].bounds().min.x, span);
        }
        assert!(iterations(1e10) > iterations(64.));
    }
}
