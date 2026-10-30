use super::*;
use std::sync::Arc;

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}
fn source(generation: u64) -> SourceId {
    SourceId {
        asset: 42,
        generation,
    }
}
fn pixel(rgba: [f64; 4]) -> WorkingPixel {
    WorkingPixel::new(ImagePrimaries::Srgb, rgba).unwrap()
}

#[test]
fn linear_exposure_has_known_encoded_srgb_reference() {
    let input = ImageColor::SRGB
        .decode([0.5, 0.5, 0.5, 0.25], ImagePrimaries::Srgb)
        .unwrap();
    near(input.rgba()[0], 0.21404114048223255);
    let exposed = ColorMatrix::identity(ImagePrimaries::Srgb)
        .exposure(1.)
        .unwrap()
        .apply(input)
        .unwrap();
    near(exposed.rgba()[0], 0.4280822809644651);
    let out = exposed
        .output(ImageColor::SRGB, GamutMapping::Clip)
        .unwrap();
    near(out[0], 0.6858361190643691);
    near(out[3], 0.25);
    // Existing ImageFilter still operates on encoded RGB and preserves alpha.
    assert_eq!(
        crate::ImageFilter::new().brightness(2.).rows()[0],
        [2., 0., 0., 0.]
    );
}

#[test]
fn association_precedes_transfer_decode_and_follows_encode() {
    let associated = ImageColor {
        alpha: AlphaMode::Premultiplied,
        ..ImageColor::SRGB
    };
    let input = associated
        .decode([0.25, 0.125, 0., 0.5], ImagePrimaries::Srgb)
        .unwrap();
    near(input.rgba()[0], 0.21404114048223255);
    let out = input.output(associated, GamutMapping::Clip).unwrap();
    for (a, b) in out.into_iter().zip([0.25, 0.125, 0., 0.5]) {
        near(a, b);
    }
    assert!(
        associated
            .decode([0.3, 0., 0., 0.2], ImagePrimaries::Srgb)
            .is_err()
    );
    assert_eq!(
        ImageColor::SRGB
            .decode([1., 0.5, 0.2, 0.], ImagePrimaries::Srgb)
            .unwrap()
            .rgba(),
        [0.; 4]
    );
    let offset = ColorMatrix::new(
        ImagePrimaries::Srgb,
        [[1., 0., 0., 0.5], [0., 1., 0., 0.5], [0., 0., 1., 0.5]],
    )
    .unwrap();
    assert_eq!(offset.apply(pixel([0.; 4])).unwrap().rgba(), [0.; 4]);
}

#[test]
fn matrices_compose_in_call_order_and_preserve_unclipped_intermediates() {
    let identity = ColorMatrix::identity(ImagePrimaries::Srgb);
    let input = pixel([0.25, 0.5, 0.75, 0.3]);
    let exposure_then_contrast = identity
        .exposure(1.)
        .unwrap()
        .contrast(2., 0.18)
        .unwrap()
        .apply(input)
        .unwrap();
    let contrast_then_exposure = identity
        .contrast(2., 0.18)
        .unwrap()
        .exposure(1.)
        .unwrap()
        .apply(input)
        .unwrap();
    near(exposure_then_contrast.rgba()[0], 0.82);
    near(contrast_then_exposure.rgba()[0], 0.64);
    near(exposure_then_contrast.rgba()[3], 0.3);
    assert!(exposure_then_contrast.rgba()[2] > 1.);
    let restored = identity
        .exposure(3.)
        .unwrap()
        .exposure(-3.)
        .unwrap()
        .apply(input)
        .unwrap();
    assert_eq!(restored, input);
    let out = exposure_then_contrast
        .output(
            ImageColor {
                transfer: TransferFunction::Linear,
                ..ImageColor::SRGB
            },
            GamutMapping::Clip,
        )
        .unwrap();
    near(out[2], 1.);
}

#[test]
fn saturation_uses_working_primaries_luminance() {
    for (primaries, expected) in [
        (ImagePrimaries::Srgb, 0.21263900587151027),
        (ImagePrimaries::DisplayP3, 0.2289745640697488),
    ] {
        let red = WorkingPixel::new(primaries, [1., 0., 0., 0.7]).unwrap();
        let gray = ColorMatrix::identity(primaries)
            .saturation(0.)
            .unwrap()
            .apply(red)
            .unwrap();
        for value in &gray.rgba()[..3] {
            near(*value, expected);
        }
        near(gray.rgba()[3], 0.7);
    }
}

#[test]
fn p3_conversion_retains_extended_rgb_until_explicit_output_mapping() {
    let p3_red = ImageColor::LINEAR_P3
        .decode([1., 0., 0., 1.], ImagePrimaries::Srgb)
        .unwrap();
    assert!(p3_red.rgba()[0] > 1.2);
    assert!(p3_red.rgba()[1] < -0.04);
    let clipped = p3_red.output(ImageColor::SRGB, GamutMapping::Clip).unwrap();
    for (actual, expected) in clipped.into_iter().zip([1., 0., 0., 1.]) {
        near(actual, expected);
    }
    let reduced = p3_red
        .output(ImageColor::SRGB, GamutMapping::ReduceChroma)
        .unwrap();
    assert!(reduced[..3].iter().all(|v| (0.0..=1.0).contains(v)));
    assert_ne!(reduced, clipped);
    let p3_again = p3_red
        .output(ImageColor::LINEAR_P3, GamutMapping::Clip)
        .unwrap();
    for (a, b) in p3_again.into_iter().zip([1., 0., 0., 1.]) {
        near(a, b);
    }
}

#[test]
fn rejects_invalid_color_parameters_space_mismatch_and_overflow() {
    let id = ColorMatrix::identity(ImagePrimaries::Srgb);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(id.exposure(value).is_err());
        assert!(id.saturation(value).is_err());
        assert!(id.contrast(value, 0.18).is_err());
        assert!(WorkingPixel::new(ImagePrimaries::Srgb, [value, 0., 0., 1.]).is_err());
    }
    assert!(id.exposure(2000.).is_err());
    assert!(id.saturation(-1.).is_err());
    assert!(id.contrast(1., 1.1).is_err());
    assert!(
        ImageColor::SRGB
            .decode([1.1, 0., 0., 1.], ImagePrimaries::Srgb)
            .is_err()
    );
    assert!(WorkingPixel::new(ImagePrimaries::Srgb, [0., 0., 0., -0.1]).is_err());
    let p3 = ColorMatrix::identity(ImagePrimaries::DisplayP3);
    assert!(id.then(p3).is_err());
    assert!(p3.apply(pixel([0.5; 4])).is_err());
    let huge = ColorMatrix::new(
        ImagePrimaries::Srgb,
        [[f64::MAX, 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
    )
    .unwrap();
    assert!(huge.then(huge).is_err());
    assert!(huge.apply(pixel([2., 0., 0., 1.])).is_err());
}

#[test]
fn vignette_has_monotone_feather_and_pixel_aspect_correct_roundness() {
    let v = Vignette::new(0.8, 0.5, 0.5, 1.).unwrap();
    near(v.factor([0.5, 0.5], [100, 100]).unwrap(), 1.);
    near(v.factor([0.75, 0.5], [100, 100]).unwrap(), 1.);
    near(v.factor([0.875, 0.5], [100, 100]).unwrap(), 0.6);
    near(v.factor([1., 0.5], [100, 100]).unwrap(), 0.2);
    // Equal physical offsets from center on a rectangular source.
    near(
        v.factor([0.75, 0.5], [200, 100]).unwrap(),
        v.factor([0.5, 1.], [200, 100]).unwrap(),
    );
    let ellipse = Vignette::new(0.8, 0.5, 0.5, 0.).unwrap();
    near(ellipse.factor([0.75, 0.5], [200, 100]).unwrap(), 1.);
    let output = v
        .apply(pixel([1., 0.5, 0.25, 0.3]), [1., 0.5], [100, 100])
        .unwrap();
    near(output.rgba()[0], 0.2);
    near(output.rgba()[3], 0.3);
    for (intensity, radius, feather, roundness) in [
        (1.1, 0.5, 0.5, 1.),
        (1., -1., 0.5, 1.),
        (1., 0.5, 0., 1.),
        (1., 0.5, 0.5, f64::NAN),
    ] {
        assert!(Vignette::new(intensity, radius, feather, roundness).is_err());
    }
    assert!(v.factor([f64::NAN, 0.], [100, 100]).is_err());
    assert!(v.factor([0., 0.], [0, 100]).is_err());
}

#[test]
fn final_only_provider_needs_no_preview_and_ready_is_terminal() {
    let mut image = ProgressiveImage::new(source(1), SourceCapabilities::default());
    let request = image.request();
    assert!(!image.publish(SourceUpdate {
        request,
        sequence: 1,
        event: SourceEvent::Preview(Arc::new("preview"))
    }));
    assert_eq!(image.status(), &ProgressiveStatus::Loading);
    assert!(image.publish(SourceUpdate {
        request,
        sequence: 1,
        event: SourceEvent::Complete(Arc::new("final"))
    }));
    assert_eq!(image.status(), &ProgressiveStatus::Ready);
    assert_eq!(**image.snapshot().unwrap(), "final");
    assert!(!image.publish(SourceUpdate {
        request,
        sequence: 2,
        event: SourceEvent::Failed("late".into())
    }));
    assert_eq!(image.status(), &ProgressiveStatus::Ready);
}

#[test]
fn progressive_order_restart_and_cancellation_reject_stale_work() {
    let caps = SourceCapabilities {
        preview: true,
        refinement: true,
    };
    let mut image = ProgressiveImage::new(source(1), caps);
    let old = image.request();
    let preview = Arc::new(1);
    let held = preview.clone();
    assert!(image.publish(SourceUpdate {
        request: old,
        sequence: 0,
        event: SourceEvent::Preview(preview)
    }));
    assert!(image.publish(SourceUpdate {
        request: old,
        sequence: 2,
        event: SourceEvent::Refinement(Arc::new(2))
    }));
    assert!(!image.publish(SourceUpdate {
        request: old,
        sequence: 1,
        event: SourceEvent::Complete(Arc::new(99))
    }));
    assert!(!image.publish(SourceUpdate {
        request: old,
        sequence: 3,
        event: SourceEvent::Preview(Arc::new(99))
    }));
    assert_eq!(**image.snapshot().unwrap(), 2);
    image.restart(source(2), caps).unwrap();
    let next = image.restart(source(1), caps).unwrap();
    assert!(next.revision() > old.revision());
    assert_eq!(next.source(), old.source());
    assert!(!image.publish(SourceUpdate {
        request: old,
        sequence: 100,
        event: SourceEvent::Complete(Arc::new(99))
    }));
    assert!(image.snapshot().is_none());
    // Old scene ownership survives state replacement.
    assert_eq!(*held, 1);
    image.cancel();
    assert!(!image.publish(SourceUpdate {
        request: next,
        sequence: 0,
        event: SourceEvent::Complete(Arc::new(99))
    }));
    assert_eq!(image.status(), &ProgressiveStatus::Cancelled);
}

#[test]
fn failed_refinement_preserves_preview_until_explicit_release() {
    let caps = SourceCapabilities {
        preview: true,
        refinement: true,
    };
    let mut image = ProgressiveImage::new(source(1), caps);
    let request = image.request();
    let preview = Arc::new(1);
    let weak = Arc::downgrade(&preview);
    image.publish(SourceUpdate {
        request,
        sequence: 0,
        event: SourceEvent::Preview(preview),
    });
    assert!(image.publish(SourceUpdate {
        request,
        sequence: 1,
        event: SourceEvent::Failed("decode failed".into())
    }));
    assert_eq!(
        image.status(),
        &ProgressiveStatus::Failed("decode failed".into())
    );
    assert!(weak.upgrade().is_some());
    assert!(!image.publish(SourceUpdate {
        request,
        sequence: 2,
        event: SourceEvent::Complete(Arc::new(2))
    }));
    image.cancel();
    assert!(weak.upgrade().is_none());
}

#[test]
fn tiles_cover_odd_edges_and_account_for_clipped_halos_and_staging() {
    let image =
        ImageDescriptor::new(1025, 513, PixelFormat::Rgba16Float, ImageColor::LINEAR_P3).unwrap();
    let pyramid = TilePyramid::new(image, TileLayout::new(512, 2).unwrap());
    assert_eq!(pyramid.tile_count(0).unwrap(), [3, 2]);
    let middle = pyramid.tile(source(1), 0, 1, 0).unwrap();
    assert_eq!(
        middle.content(),
        PixelRect {
            x: 512,
            y: 0,
            width: 512,
            height: 512
        }
    );
    assert_eq!(
        middle.upload(),
        PixelRect {
            x: 510,
            y: 0,
            width: 515,
            height: 513
        }
    );
    let edge = pyramid.tile(source(1), 0, 2, 1).unwrap();
    assert_eq!(
        edge.content(),
        PixelRect {
            x: 1024,
            y: 512,
            width: 1,
            height: 1
        }
    );
    assert_eq!(
        edge.upload(),
        PixelRect {
            x: 1022,
            y: 510,
            width: 3,
            height: 3
        }
    );
    assert_eq!(edge.payload_bytes(), 72);
    assert_eq!(edge.staging_bytes(), 768);
    assert_eq!(pyramid.tile_count(1).unwrap(), [1, 1]);
    assert_eq!(image.mip_size(1).unwrap(), [512, 256]);
    assert!(pyramid.tile(source(1), 0, 3, 0).is_err());
    assert!(pyramid.tile_count(image.mip_count()).is_err());
}

#[test]
fn large_image_planning_is_bounded_and_overflow_checked() {
    let image =
        ImageDescriptor::new(8000, 6000, PixelFormat::Rgba16Float, ImageColor::LINEAR_P3).unwrap();
    assert!(image.full_mip_bytes().unwrap() > 500_000_000);
    let tile = TilePyramid::new(image, TileLayout::new(512, 2).unwrap())
        .tile(source(1), 0, 4, 3)
        .unwrap();
    assert!(tile.payload_bytes() < 3 * 1024 * 1024);
    TileBudgets::default().check_tile(&tile).unwrap();
    let huge = ImageDescriptor::new(
        u32::MAX,
        u32::MAX,
        PixelFormat::Rgba16Float,
        ImageColor::LINEAR_P3,
    )
    .unwrap();
    assert!(huge.full_mip_bytes().is_err());
    let pyramid = TilePyramid::new(huge, TileLayout::new(512, 2).unwrap());
    let [cols, rows] = pyramid.tile_count(0).unwrap();
    let edge = pyramid.tile(source(1), 0, cols - 1, rows - 1).unwrap();
    assert_eq!(edge.content().width, 511);
    assert!(edge.payload_bytes() < 3 * 1024 * 1024);
    assert!(ImageDescriptor::new(0, 1, PixelFormat::Rgba8, ImageColor::SRGB).is_err());
    assert!(TileLayout::new(0, 0).is_err());
    assert!(TileLayout::new(16384, 1).is_err());
}

#[test]
fn tile_cache_identity_includes_generation_layout_and_color_representation() {
    let descriptor = ImageDescriptor::new(512, 512, PixelFormat::Rgba8, ImageColor::SRGB).unwrap();
    let layout = TileLayout::new(256, 0).unwrap();
    let base = TilePyramid::new(descriptor, layout)
        .tile(source(1), 0, 0, 0)
        .unwrap()
        .key();
    let mut identities = std::collections::HashSet::new();
    identities.insert(base);
    for key in [
        TileKey {
            source: source(2),
            ..base
        },
        TileKey { mip: 1, ..base },
        TileKey { x: 1, ..base },
        TileKey {
            layout: TileLayout::new(256, 1).unwrap(),
            ..base
        },
        TileKey {
            image: ImageDescriptor::new(512, 512, PixelFormat::Rgba8, ImageColor::LINEAR_P3)
                .unwrap(),
            ..base
        },
        TileKey {
            image: ImageDescriptor::new(512, 512, PixelFormat::Rgba16Float, ImageColor::SRGB)
                .unwrap(),
            ..base
        },
    ] {
        assert!(identities.insert(key));
    }
}

#[test]
fn tile_policy_rejects_independent_pool_and_frame_limits() {
    let descriptor = ImageDescriptor::new(513, 1, PixelFormat::Rgba8, ImageColor::SRGB).unwrap();
    let tile = TilePyramid::new(descriptor, TileLayout::new(512, 0).unwrap())
        .tile(source(1), 0, 1, 0)
        .unwrap();
    assert_eq!((tile.payload_bytes(), tile.staging_bytes()), (4, 256));
    let budgets = TileBudgets::default();
    for invalid in [
        TileBudgets {
            cpu_bytes: 3,
            ..budgets
        },
        TileBudgets {
            gpu_bytes: 3,
            ..budgets
        },
        TileBudgets {
            staging_bytes: 255,
            ..budgets
        },
        TileBudgets {
            upload_bytes_per_frame: 255,
            ..budgets
        },
        TileBudgets {
            decode_jobs: 0,
            ..budgets
        },
        TileBudgets {
            uploads_per_frame: 0,
            ..budgets
        },
    ] {
        assert!(invalid.check_tile(&tile).is_err());
    }
    TileBudgets {
        cpu_bytes: 4,
        gpu_bytes: 4,
        staging_bytes: 256,
        upload_bytes_per_frame: 256,
        uploads_per_frame: 1,
        decode_jobs: 1,
    }
    .check_tile(&tile)
    .unwrap();
}

#[test]
fn phase_five_capabilities_include_preview_blur() {
    let caps = PipelineCapabilities::current();
    for feature in [
        PipelineFeature::LinearAdjustments,
        PipelineFeature::Vignette,
        PipelineFeature::Rgba16FloatImages,
        PipelineFeature::ProgressiveImages,
        PipelineFeature::TiledImages,
        PipelineFeature::ImageBlur,
    ] {
        let available = matches!(
            feature,
            PipelineFeature::LinearAdjustments
                | PipelineFeature::Vignette
                | PipelineFeature::Rgba16FloatImages
                | PipelineFeature::ProgressiveImages
                | PipelineFeature::TiledImages
                | PipelineFeature::ImageBlur
        );
        assert_eq!(caps.supports(feature), available);
        assert_eq!(caps.require(feature).is_ok(), available);
    }
}

#[test]
fn request_identity_is_private_to_each_view_even_for_the_same_asset() {
    let mut first = ProgressiveImage::new(source(1), SourceCapabilities::default());
    let mut second = ProgressiveImage::new(source(1), SourceCapabilities::default());
    assert_ne!(first.request(), second.request());
    assert!(!second.publish(SourceUpdate {
        request: first.request(),
        sequence: 0,
        event: SourceEvent::Complete(Arc::new(1)),
    }));
    assert!(first.publish(SourceUpdate {
        request: first.request(),
        sequence: 0,
        event: SourceEvent::Complete(Arc::new(1)),
    }));
    assert_eq!(second.status(), &ProgressiveStatus::Loading);
}

#[test]
fn gpu_adjustments_reject_unavailable_spaces_and_unrepresentable_parameters() {
    assert!(ImageAdjustments::new(ColorMatrix::identity(ImagePrimaries::DisplayP3), None).is_ok());
    let huge = ColorMatrix::identity(ImagePrimaries::Srgb)
        .exposure(200.)
        .unwrap();
    assert!(ImageAdjustments::new(huge, None).is_err());
    let feather = Vignette::new(0.4, 0.5, 1e-100, 1.).unwrap();
    assert!(
        ImageAdjustments::new(ColorMatrix::identity(ImagePrimaries::Srgb), Some(feather)).is_err()
    );
    let subnormal = Vignette::new(0.4, 0.5, 1e-40, 1.).unwrap();
    assert!(
        ImageAdjustments::new(ColorMatrix::identity(ImagePrimaries::Srgb), Some(subnormal))
            .is_err()
    );
    let valid = ImageAdjustments::new(
        ColorMatrix::identity(ImagePrimaries::Srgb)
            .exposure(1.)
            .unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(valid.paint().rows()[0], [2., 0., 0., 0.]);
}

#[test]
fn image_blur_rejects_invalid_support_and_accepts_quality_modes() {
    for r in [-1., 16.01, f32::NAN, f32::INFINITY] {
        assert!(ImageBlur::new(gpui::px(r)).is_err());
    }
    for r in [0., 0.5, 4., 16.] {
        let blur = ImageBlur::new(gpui::px(r)).unwrap();
        let _ = blur
            .clone()
            .with_quality(gpui::LayerBlurQuality::FullResolution);
    }
}

#[test]
fn adobe_rgb_source_alpha_and_transfer_are_independent_of_primaries() {
    let adobe = ImageColor::ADOBE_RGB_1998;
    let rgba = [0.5, 0.18, 0.003, 0.25];
    let linear = adobe.decode(rgba, ImagePrimaries::AdobeRgb1998).unwrap();
    for (actual, expected) in linear.rgba().into_iter().zip([
        0.21775552814439456,
        0.023024029316635192,
        0.0000028290313452536574,
        0.25,
    ]) {
        near(actual, expected);
    }
    let associated = ImageColor {
        alpha: AlphaMode::Premultiplied,
        ..adobe
    };
    let input = associated
        .decode([0.125, 0.045, 0.00075, 0.25], ImagePrimaries::AdobeRgb1998)
        .unwrap();
    for (actual, expected) in input.rgba().into_iter().zip(linear.rgba()) {
        near(actual, expected);
    }
    for (actual, expected) in linear
        .output(associated, GamutMapping::Clip)
        .unwrap()
        .into_iter()
        .zip([0.125, 0.045, 0.00075, 0.25])
    {
        near(actual, expected);
    }
    assert_eq!(
        adobe
            .decode([1., 0.5, 0.3, 0.], ImagePrimaries::AdobeRgb1998)
            .unwrap()
            .rgba(),
        [0.; 4]
    );
    assert!(
        associated
            .decode([0.3, 0., 0., 0.25], ImagePrimaries::AdobeRgb1998)
            .is_err()
    );

    // A transfer function describes channel encoding, independently of primaries.
    for primaries in [
        ImagePrimaries::Srgb,
        ImagePrimaries::DisplayP3,
        ImagePrimaries::AdobeRgb1998,
    ] {
        for (transfer, value) in [
            (TransferFunction::Linear, 0.5),
            (TransferFunction::Srgb, 0.21404114048223255),
            (TransferFunction::AdobeRgb1998, 0.21775552814439456),
        ] {
            let source = ImageColor {
                primaries,
                transfer,
                alpha: AlphaMode::Straight,
            };
            let input = source.decode([0.5, 0.5, 0.5, 0.25], primaries).unwrap();
            for actual in &input.rgba()[..3] {
                near(*actual, value);
            }
            for actual in &input.output(source, GamutMapping::Clip).unwrap()[..3] {
                near(*actual, 0.5);
            }
        }
    }
}

#[test]
fn adobe_working_matrix_is_cpu_only_and_gpu_rejection_is_explicit() {
    let matrix = ColorMatrix::identity(ImagePrimaries::AdobeRgb1998)
        .saturation(0.)
        .unwrap();
    let gray = matrix
        .apply(WorkingPixel::new(ImagePrimaries::AdobeRgb1998, [1., 0., 0., 0.5]).unwrap())
        .unwrap()
        .rgba();
    for actual in &gray[..3] {
        near(*actual, 0.29734497525053605);
    }
    near(gray[3], 0.5);
    let error = ImageAdjustments::new(matrix, None).unwrap_err().to_string();
    assert!(error.contains("convert Adobe RGB input first"), "{error}");
    assert!(
        !ImageAdjustments::new(ColorMatrix::identity(ImagePrimaries::Srgb), None)
            .unwrap()
            .paint()
            .is_p3()
    );
    assert!(
        ImageAdjustments::new(ColorMatrix::identity(ImagePrimaries::DisplayP3), None)
            .unwrap()
            .paint()
            .is_p3()
    );
}
