use super::*;
use crate::{DevicePixels, px, size};

#[derive(Debug)]
struct LegacyProducer;

// Deliberately implements only the original method.
impl CustomContent for LegacyProducer {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        raster: Size<DevicePixels>,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        anyhow::ensure!(backend.is::<u32>(), "unsupported test backend");
        Ok(Box::new((raster, *backend.downcast_ref::<u32>().unwrap())))
    }
}

#[derive(Debug)]
struct RequestProducer;

impl CustomContent for RequestProducer {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        panic!("request override must be called")
    }

    fn prepare_with_request(
        &self,
        _: &dyn std::any::Any,
        request: &PreparationRequest,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        Ok(Box::new(*request))
    }
}

#[test]
fn preparation_legacy_adapter_preserves_extent_backend_and_error() {
    let raster = size(DevicePixels(126), DevicePixels(64));
    let request = PreparationRequest::new(size(px(100.3), px(50.7)), raster, 1.25).unwrap();
    let producer: &dyn CustomContent = &LegacyProducer;
    let output = producer.prepare_with_request(&42_u32, &request).unwrap();
    assert_eq!(
        *output.downcast::<(Size<DevicePixels>, u32)>().unwrap(),
        (raster, 42)
    );
    assert_eq!(
        producer
            .prepare_with_request(&(), &request)
            .unwrap_err()
            .to_string(),
        "unsupported test backend"
    );
}

#[test]
fn preparation_request_override_receives_fractional_geometry() {
    let request = PreparationRequest::new(
        size(px(100.3), px(50.7)),
        size(DevicePixels(126), DevicePixels(64)),
        1.25,
    )
    .unwrap();
    let producer: &dyn CustomContent = &RequestProducer;
    let output = producer.prepare_with_request(&(), &request).unwrap();
    assert_eq!(*output.downcast::<PreparationRequest>().unwrap(), request);
    assert_eq!(request.logical_size(), size(px(100.3), px(50.7)));
    assert_eq!(request.scale_factor(), 1.25);
    assert_ne!(
        request.logical_size().width.0 * request.scale_factor(),
        request.raster_size().width.0 as f32
    );
}

#[test]
fn preparation_geometry_adapter_retains_source_and_renderer_extent() {
    let logical = size(px(100.3), px(50.7));
    let producer =
        content_with_preparation_geometry(std::sync::Arc::new(RequestProducer), logical, 1.25);
    let raster = size(DevicePixels(126), DevicePixels(64));
    let default_request = PreparationRequest::new(size(px(125.4), px(63.4)), raster, 1.).unwrap();
    for retained in [producer.clone(), producer] {
        let output = retained
            .prepare_with_request(&(), &default_request)
            .unwrap();
        let request = output.downcast::<PreparationRequest>().unwrap();
        assert_eq!(request.logical_size(), logical);
        assert_eq!(request.raster_size(), raster);
        assert_eq!(request.scale_factor(), 1.25);
    }
    let legacy =
        content_with_preparation_geometry(std::sync::Arc::new(LegacyProducer), logical, 1.25);
    let output = legacy
        .prepare_with_request(&42_u32, &default_request)
        .unwrap();
    assert_eq!(
        *output.downcast::<(Size<DevicePixels>, u32)>().unwrap(),
        (raster, 42)
    );
}

#[test]
fn preparation_request_rejects_invalid_geometry() {
    let logical = size(px(10.), px(20.));
    let raster = size(DevicePixels(10), DevicePixels(20));
    for scale in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(PreparationRequest::new(logical, raster, scale).is_err());
    }
    for value in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(PreparationRequest::new(size(px(value), px(20.)), raster, 1.).is_err());
        assert!(PreparationRequest::new(size(px(10.), px(value)), raster, 1.).is_err());
    }
    assert!(PreparationRequest::new(logical, size(DevicePixels(0), DevicePixels(20)), 1.).is_err());
    assert!(
        PreparationRequest::new(logical, size(DevicePixels(10), DevicePixels(-1)), 1.).is_err()
    );
}

#[test]
fn preparation_frame_survives_window_geometry_adapter() {
    #[derive(Debug)]
    struct FrameProducer;
    impl CustomContent for FrameProducer {
        fn prepare(
            &self,
            _: &dyn std::any::Any,
            _: Size<DevicePixels>,
        ) -> anyhow::Result<Box<dyn std::any::Any>> {
            panic!("frame-aware preparation expected")
        }
        fn prepare_in_frame(
            &self,
            _: &dyn std::any::Any,
            request: &PreparationRequest,
            frame: &crate::PreparationFrame,
        ) -> anyhow::Result<Box<dyn std::any::Any>> {
            Ok(Box::new((*request, frame.try_reserve_upload(42, 8, 8, 1))))
        }
    }
    let logical = size(px(10.5), px(20.25));
    let wrapped =
        content_with_preparation_geometry(std::sync::Arc::new(FrameProducer), logical, 2.);
    let raster = size(DevicePixels(21), DevicePixels(41));
    let request = PreparationRequest::new(size(px(21.), px(41.)), raster, 1.).unwrap();
    let frame = crate::PreparationFrame::default();
    let first = *wrapped
        .prepare_in_frame(&(), &request, &frame)
        .unwrap()
        .downcast::<(PreparationRequest, bool)>()
        .unwrap();
    assert_eq!(first.0.logical_size(), logical);
    assert_eq!(first.0.scale_factor(), 2.);
    assert!(first.1);
    let second = *wrapped
        .prepare_in_frame(&(), &request, &frame)
        .unwrap()
        .downcast::<(PreparationRequest, bool)>()
        .unwrap();
    assert!(!second.1, "adapter must preserve the shared frame budget");
    // An old producer remains compatible with the new frame-aware dispatch.
    assert!(
        LegacyProducer
            .prepare_in_frame(&42_u32, &request, &frame)
            .is_ok()
    );
}
