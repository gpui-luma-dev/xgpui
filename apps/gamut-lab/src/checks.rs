use super::*;
use gpui_apple::metal_renderer::MetalRenderer;
use parking_lot::Mutex;
use std::path::Path;
fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
fn quad(scene: &mut Scene, b: Bounds<ScaledPixels>, color: Rgba) {
    scene.insert_primitive(Quad {
        bounds: b,
        content_mask: ContentMask { bounds: b },
        background: color.into(),
        ..Default::default()
    });
}
fn surface(scene: &mut Scene, b: Bounds<ScaledPixels>, color: [f64; 4], wide: bool) {
    let source: Arc<dyn CustomContent> = if wide {
        Arc::new(content::WideColor(color))
    } else {
        Arc::new(SolidColor::new(color.map(|v| v as f32)).unwrap())
    };
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds: b,
        content_mask: ContentMask { bounds: b },
        corner_radii: Corners::all(ScaledPixels(0.)),
        opacity: 1.,
        content: source,
        coverage: None,
    });
}
pub(super) fn run(frames: usize, png: Option<&Path>, wide: bool) -> Result<()> {
    let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
    let mut scene = Scene::default();
    quad(&mut scene, bounds(0., 0., 256., 192.), rgb(0x101010));
    surface(
        &mut scene,
        bounds(16., 16., 160., 128.),
        [1., 0., 0., 1.],
        wide,
    );
    quad(&mut scene, bounds(48., 48., 160., 64.), rgba(0xffffff80));
    surface(
        &mut scene,
        bounds(112., 80., 112., 96.),
        [0., 1., 0., 1.],
        wide,
    );
    quad(&mut scene, bounds(144., 112., 80., 32.), rgb(0x334155));
    scene.finish();
    let extent = size(DevicePixels(256), DevicePixels(192));
    for _ in 0..frames {
        renderer.render_scene(&scene, extent)?;
    }
    let image = renderer.render_scene_to_image(&scene, extent)?;
    ensure!(
        image.get_pixel(150, 120).0 == [0x33, 0x41, 0x55, 255],
        "tooltip/order changed"
    );
    ensure!(
        image.get_pixel(120, 90).0 == [0, 255, 0, 255],
        "second wide surface did not cover UI overlay"
    );
    if let Some(path) = png {
        image.save(path)?;
        println!("Clipped sRGB preview: {}", path.display());
    }
    Ok(())
}
