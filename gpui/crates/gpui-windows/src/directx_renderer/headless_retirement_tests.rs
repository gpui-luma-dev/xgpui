//! Headless frames must retire custom-content snapshots like windowed frames.
use super::*;
use crate::custom_draw::{Dx11CustomContext, Dx11CustomOutput};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug)]
struct Content {
    output: std::sync::Mutex<Option<Dx11CustomOutput>>,
    dropped: mpsc::Sender<()>,
}

impl CustomContent for Content {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        size: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        let context = backend
            .downcast_ref::<Dx11CustomContext>()
            .context("D3D11 preparation context")?;
        let mut cached = self.output.lock().unwrap();
        if let Some(output) = cached.as_ref().filter(|output| context.can_reuse(output)) {
            return Ok(Box::new(output.clone()));
        }
        let output = context.create_output(size)?;
        let mut rtv = None;
        unsafe {
            context
                .device
                .CreateRenderTargetView(output.texture(), None, Some(&mut rtv))?;
        }
        let rtv = rtv.context("missing render target view")?;
        context.record(|deferred| {
            unsafe { deferred.ClearRenderTargetView(&rtv, &[0., 1., 0., 1.]) };
            Ok(())
        })?;
        *cached = Some(output.clone());
        Ok(Box::new(output))
    }
}

impl Drop for Content {
    fn drop(&mut self) {
        let _ = self.dropped.send(());
    }
}

fn scene(content: Option<Arc<Content>>) -> Scene {
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    let mut scene = Scene::default();
    if let Some(content) = content {
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Default::default(),
            opacity: 1.,
            content,
            coverage: None,
        });
    }
    scene.finish();
    scene
}

#[::core::prelude::v1::test]
fn headless_render_scene_retires_custom_content() {
    let extent = size(DevicePixels(8), DevicePixels(8));
    let mut renderer = Dx11HeadlessRenderer::new().expect("WARP renderer");
    let (sender, dropped) = mpsc::channel();
    let content = Arc::new(Content {
        output: std::sync::Mutex::new(None),
        dropped: sender,
    });
    renderer
        .render_scene(&scene(Some(content.clone())), extent)
        .unwrap();
    drop(content);

    // A later frame reads the completed retirement query and releases the
    // snapshot. Without a flush, the query never completes.
    let empty = scene(None);
    let released = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        renderer.render_scene(&empty, extent).unwrap();
        dropped.try_recv().is_ok()
    });
    assert!(released, "headless frames never retired custom content");
    assert!(renderer.0.custom_retirement.is_empty());
}
