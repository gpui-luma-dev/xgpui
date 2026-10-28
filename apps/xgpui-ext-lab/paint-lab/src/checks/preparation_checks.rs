use super::*;
use std::sync::Mutex;

#[derive(Debug)]
struct RequestProbe {
    requests: Arc<Mutex<Vec<PreparationRequest>>>,
    source: SolidColor,
}

impl CustomContent for RequestProbe {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        panic!("renderer must dispatch through prepare_with_request")
    }

    fn prepare_with_request(
        &self,
        backend: &dyn std::any::Any,
        request: &PreparationRequest,
    ) -> Result<Box<dyn std::any::Any>> {
        self.requests.lock().unwrap().push(*request);
        // SolidColor is an unchanged legacy producer.
        self.source.prepare_with_request(backend, request)
    }
}

#[::core::prelude::v1::test]
fn native_preparation_request_preserves_unclipped_source() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let probe = Arc::new(RequestProbe {
        requests: requests.clone(),
        source: SolidColor::new([1., 0., 0., 1.])?,
    });
    let bounds = rect(10.25, 20.5, 100.3, 50.7);
    let mut draw = record(bounds, probe, None);
    draw.corner_radii = Corners::default();
    let viewport = size(DevicePixels(256), DevicePixels(128));
    let mut full = base();
    full.insert_primitive(draw.clone());
    full.finish();
    let full_image = renderer.render_scene_to_image(&full, viewport)?;
    draw.content_mask.bounds = rect(30., 30., 20., 15.);
    draw.opacity = 0.5;
    let mut clipped = base();
    clipped.insert_primitive(draw);
    clipped.finish();
    let clipped_image = renderer.render_scene_to_image(&clipped, viewport)?;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(requests[0].logical_size(), size(px(100.3), px(50.7)));
    assert_eq!(
        requests[0].raster_size(),
        size(DevicePixels(101), DevicePixels(51))
    );
    assert_eq!(requests[0].scale_factor(), 1.);
    assert!(full_image.get_pixel(40, 35).0[0] > 240);
    assert!(clipped_image.get_pixel(40, 35).0[0].abs_diff(128) <= 2);
    assert_eq!(clipped_image.get_pixel(15, 25).0[0], 0);
    Ok(())
}

#[derive(Debug)]
struct RejectPreparation;
impl CustomContent for RejectPreparation {
    // Deliberately implements only the original producer method.
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        anyhow::bail!("intentional preparation error")
    }
}

fn preparation_error_still_fails_frame(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let extent = size(DevicePixels(1024), DevicePixels(320));
    let source = Arc::new(SolidColor::new([1., 0., 0., 1.])?);
    let bounds = rect(40., 30., 320., 240.);
    let healthy = record(bounds, source, None);
    let mut failing = base();
    failing.insert_primitive(healthy.clone());
    failing.insert_primitive(record(
        rect(420., 30., 320., 240.),
        Arc::new(RejectPreparation),
        None,
    ));
    failing.finish();
    let error = renderer
        .render_scene_to_image(&failing, extent)
        .unwrap_err();
    ensure!(
        format!("{error:#}").contains("intentional preparation error"),
        "unexpected frame error: {error:#}"
    );
    // A cached output from the aborted frame must recover without changing the producer.
    let mut recovered = base();
    recovered.insert_primitive(healthy);
    recovered.finish();
    let image = renderer.render_scene_to_image(&recovered, extent)?;
    ensure!(
        image.get_pixel(100, 100).0 == [255, 0, 0, 255],
        "legacy producer did not recover after frame failure"
    );
    Ok(())
}

#[::core::prelude::v1::test]
fn native_preparation_error_still_fails_frame_and_legacy_producer_recovers() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    preparation_error_still_fails_frame(renderer.as_mut())
}

#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_preparation_error_still_fails_frame_and_legacy_producer_recovers() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    preparation_error_still_fails_frame(&mut renderer)
}
