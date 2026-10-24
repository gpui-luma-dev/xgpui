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
