use gpui::content::*;
fn image(w: u32, h: u32) -> ImageDescriptor {
    ImageDescriptor::new(
        ImageExtent::new(w, h).unwrap(),
        PixelFormat::Rgba8,
        ImageInterpretation {
            color: ColorEncoding::Srgb,
            alpha: AlphaMode::Straight,
        },
        1,
    )
    .unwrap()
}
fn close(a: [f32; 2], b: [f32; 2]) {
    for i in 0..2 {
        assert!((a[i] - b[i]).abs() < 0.00001, "{a:?} != {b:?}");
    }
}
#[test]
fn stretch_preserves_full_source_and_default_sampling() {
    let m = ImageMapping::default()
        .resolve(&image(8, 4), [100., 100.])
        .unwrap();
    close(m.destination_origin, [0., 0.]);
    close(m.destination_size, [1., 1.]);
    close(m.uv_origin, [0., 0.]);
    close(m.uv_size, [1., 1.]);
    close(m.sample_uv([0.5, 0.5]).unwrap(), [0.5, 0.5]);
    assert_eq!(m.sampler, Sampler::linear_clamp());
}
#[test]
fn contain_letterboxes_and_alignment_moves_only_free_axis() {
    for alignment in [0., 0.5, 1.] {
        let m = ImageMapping::new(ImageFit::Contain)
            .alignment([1., alignment])
            .unwrap()
            .resolve(&image(8, 4), [8., 8.])
            .unwrap();
        close(m.destination_size, [1., 0.5]);
        close(m.destination_origin, [0., 0.5 * alignment]);
        if alignment > 0. {
            assert_eq!(m.sample_uv([0.5, 0.01]), None);
        }
        if alignment < 1. {
            assert_eq!(m.sample_uv([0.5, 0.99]), None);
        }
        close(
            m.sample_uv([0.5, 0.5 * alignment + 0.25]).unwrap(),
            [0.5, 0.5],
        );
    }
}
#[test]
fn cover_alignment_selects_excess_source_without_letterboxing() {
    for alignment in [0., 0.5, 1.] {
        let m = ImageMapping::new(ImageFit::Cover)
            .alignment([alignment, 1.])
            .unwrap()
            .resolve(&image(8, 4), [4., 4.])
            .unwrap();
        close(m.destination_size, [1., 1.]);
        close(m.uv_size, [0.5, 1.]);
        close(m.uv_origin, [0.5 * alignment, 0.]);
        close(
            m.sample_uv([0.5, 0.5]).unwrap(),
            [0.5 * alignment + 0.25, 0.5],
        );
    }
}
#[test]
fn crop_pixel_edges_and_half_texel_limits_prevent_neighbor_bleed() {
    let crop = ImageCrop::new([2., 1.], [4., 2.]).unwrap();
    let m = ImageMapping::default()
        .crop(Some(crop))
        .resolve(&image(8, 4), [4., 2.])
        .unwrap();
    close(m.uv_origin, [0.25, 0.25]);
    close(m.uv_size, [0.5, 0.5]);
    close(m.sample_uv([0., 0.]).unwrap(), [2.5 / 8., 1.5 / 4.]);
    close(m.sample_uv([0.999, 0.999]).unwrap(), [5.5 / 8., 2.5 / 4.]);
    let tiny = ImageMapping::default()
        .crop(Some(ImageCrop::new([2., 1.], [0.25, 0.5]).unwrap()))
        .resolve(&image(8, 4), [4., 2.])
        .unwrap();
    close(tiny.clamp_min, tiny.clamp_max);
    assert_eq!(m.sample_uv([1., 0.5]), None);
}
#[test]
fn retained_descriptor_controls_fit_and_crop_validation() {
    let mapping = ImageMapping::new(ImageFit::Contain);
    close(
        mapping
            .resolve(&image(8, 4), [8., 8.])
            .unwrap()
            .destination_size,
        [1., 0.5],
    );
    close(
        mapping
            .resolve(&image(4, 8), [8., 8.])
            .unwrap()
            .destination_size,
        [0.5, 1.],
    );
    let crop = ImageMapping::default().crop(Some(ImageCrop::new([4., 0.], [4., 4.]).unwrap()));
    assert!(crop.resolve(&image(8, 4), [8., 8.]).is_ok());
    assert!(crop.resolve(&image(4, 8), [8., 8.]).is_err());
}
#[test]
fn rejects_nonfinite_empty_and_out_of_bounds_inputs() {
    for value in [f32::NAN, f32::INFINITY, -1.] {
        assert!(ImageMapping::default().alignment([value, 0.5]).is_err());
        assert!(ImageCrop::new([value, 0.], [1., 1.]).is_err());
        assert!(ImageCrop::new([0., 0.], [value, 1.]).is_err());
        assert!(
            ImageMapping::default()
                .resolve(&image(4, 4), [value, 1.])
                .is_err()
        );
    }
    assert!(ImageCrop::new([0.; 2], [0., 1.]).is_err());
    assert!(ImageMapping::default().alignment([1.01, 0.]).is_err());
    assert!(
        ImageMapping::default()
            .resolve(&image(4, 4), [0., 1.])
            .is_err()
    );
    assert!(
        ImageMapping::default()
            .crop(Some(ImageCrop::new([3., 0.], [2., 1.]).unwrap()))
            .resolve(&image(4, 4), [1., 1.])
            .is_err()
    );
    let mut repeat = Sampler::nearest_clamp();
    repeat.address[0] = AddressMode::Repeat;
    assert!(ImageMapping::default().sampler(repeat).is_err());
}
#[test]
fn nearest_and_mip_policy_survive_resolution_without_changing_geometry() {
    let mut sampler = Sampler::nearest_clamp();
    sampler.mip_filter = Some(FilterMode::Linear);
    let linear = ImageMapping::new(ImageFit::Cover)
        .resolve(&image(16, 8), [4., 4.])
        .unwrap();
    let sampled = ImageMapping::new(ImageFit::Cover)
        .sampler(sampler)
        .unwrap()
        .resolve(&image(16, 8), [4., 4.])
        .unwrap();
    assert_eq!(sampled.sampler, sampler);
    assert_eq!(linear.uv_origin, sampled.uv_origin);
    assert_eq!(linear.uv_size, sampled.uv_size);
}
#[test]
fn mapping_small_pixel_grid_matches_independent_contain_reference() {
    let m = ImageMapping::new(ImageFit::Contain)
        .resolve(&image(4, 2), [8., 8.])
        .unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let uv = m.sample_uv([(x as f32 + 0.5) / 8., (y as f32 + 0.5) / 8.]);
            if (2..6).contains(&y) {
                let uv = uv.unwrap();
                let sx = ((uv[0] * 4.).floor() as u32).min(3);
                let sy = ((uv[1] * 2.).floor() as u32).min(1);
                assert_eq!([sx, sy], [x / 2, (y - 2) / 2]);
            } else {
                assert!(uv.is_none());
            }
        }
    }
}

#[test]
fn portrait_fit_and_large_integer_source_extents_are_stable() {
    let portrait = image(2, 4);
    let contain = ImageMapping::new(ImageFit::Contain)
        .resolve(&portrait, [8., 8.])
        .unwrap();
    close(contain.destination_origin, [0.25, 0.]);
    close(contain.destination_size, [0.5, 1.]);
    let cover = ImageMapping::new(ImageFit::Cover)
        .resolve(&portrait, [8., 8.])
        .unwrap();
    close(cover.uv_origin, [0., 0.25]);
    close(cover.uv_size, [1., 0.5]);
    // Extents beyond f32's exact integer range must retain the whole source.
    let large = ImageMapping::default()
        .resolve(&image(16_777_219, 1), [1., 1.])
        .unwrap();
    close(large.uv_size, [1., 1.]);
    let huge = ImageMapping::new(ImageFit::Contain)
        .resolve(&image(4, 2), [f32::MAX, f32::MAX])
        .unwrap();
    close(huge.destination_size, [1., 0.5]);
    // Inputs can be finite while their normalized placement underflows in f32.
    assert!(
        ImageMapping::new(ImageFit::Contain)
            .resolve(&image(4, 2), [f32::MIN_POSITIVE, f32::MAX])
            .is_err()
    );
}

#[test]
fn viewport_preserves_halo_filter_domain_and_rejects_escape() {
    let image = ImageDescriptor::new(
        ImageExtent::new(258, 258).unwrap(),
        PixelFormat::Rgba8,
        ImageInterpretation {
            color: ColorEncoding::Srgb,
            alpha: AlphaMode::Straight,
        },
        1,
    )
    .unwrap();
    let viewport = ImageCrop::new([1., 1.], [256., 256.]).unwrap();
    let mapped = ImageMapping::default()
        .viewport(Some(viewport))
        .resolve(&image, [512., 512.])
        .unwrap()
        .shader_uniforms();
    let cropped = ImageMapping::default()
        .crop(Some(viewport))
        .resolve(&image, [512., 512.])
        .unwrap()
        .shader_uniforms();
    assert_eq!(mapped[1], cropped[1]);
    assert_ne!(
        mapped[2], cropped[2],
        "viewport must retain the halo's filter domain"
    );
    let invalid = ImageMapping::default()
        .crop(Some(viewport))
        .viewport(Some(ImageCrop::new([0., 0.], [1., 1.]).unwrap()));
    assert!(invalid.resolve(&image, [1., 1.]).is_err());
}
