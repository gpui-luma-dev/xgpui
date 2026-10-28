use super::*;

#[cfg(feature = "wgpu")]
#[test]
fn effect_removal_releases_textures_and_restores_captures() {
    #[derive(Debug)]
    struct NativeBackground;
    impl gpui::CustomContent for NativeBackground {
        fn prepare(
            &self,
            _: &dyn std::any::Any,
            _: gpui::Size<DevicePixels>,
        ) -> Result<Box<dyn std::any::Any>> {
            Ok(Box::new(gpui::Background::from(gpui::rgb(0x00ff00))))
        }
    }
    let extent = size(DevicePixels(96), DevicePixels(64));
    for (group, backdrop_enabled) in [(true, false), (false, true), (true, true)] {
        let mut source = Scene::default();
        rect(&mut source, 32., 16., 32., 32., gpui::rgb(0xe62814).into());
        source.finish();
        let mut active = Scene::default();
        rect(&mut active, 0., 0., 96., 64., gpui::white());
        if backdrop_enabled {
            active.insert_primitive(Primitive::Backdrop(gpui::BackdropDraw {
                order: 0,
                bounds: viewport().bounds,
                content_mask: viewport(),
                surface_bounds: Bounds::new(point(px(16.), px(12.)), size(px(64.), px(40.)))
                    .scale(1.),
                clip: viewport(),
                corner_radii: Corners::all(ScaledPixels(8.)),
                radius: 8.,
                opacity: 1.,
                program: backdrop::shared_program().unwrap(),
            }));
        }
        if group {
            let mut c = capture(0.6);
            Arc::get_mut(&mut c).unwrap().blur_radius = ScaledPixels(8.);
            active.begin_layer_capture(c.clone(), viewport());
            active.replay(0..source.len(), &source);
            active.end_layer_capture(c, viewport());
        }
        // Native background producers render internal coverage subframes. Those
        // must leave the main scene's retained effect textures and counters intact.
        active.insert_primitive(gpui::CustomDraw {
            order: 0,
            bounds: Bounds::new(point(px(88.), px(56.)), size(px(8.), px(8.))).scale(1.),
            content_mask: viewport(),
            corner_radii: Corners::default(),
            opacity: 1.,
            content: Arc::new(NativeBackground),
            coverage: None,
        });
        active.finish();
        let mut removed = Scene::default();
        rect(&mut removed, 0., 0., 96., 64., gpui::rgb(0x336699).into());
        removed.finish();
        for mut renderer in renderers() {
            let original = renderer.render_scene_to_image(&active, extent).unwrap();
            let textures = usize::from(group) * 4 + usize::from(backdrop_enabled) * 3;
            assert_eq!(renderer.effect_texture_count(), textures);
            // Replay the entire frame so both raw and backdrop dependencies hit.
            let mut replay = Scene::default();
            replay.replay(0..active.len(), &active);
            replay.finish();
            renderer.render_scene_to_image(&replay, extent).unwrap();
            let layers_before = renderer.stats();
            let backdrops_before = renderer.backdrop_stats();
            assert_eq!(layers_before.hits, u64::from(group));
            assert_eq!(backdrops_before.hits, u64::from(backdrop_enabled));
            for scene in [&removed, &Scene::default()] {
                renderer.render_scene_to_image(scene, extent).unwrap();
                assert_eq!(
                    renderer.effect_texture_count(),
                    0,
                    "last effect removal must release retained textures"
                );
                assert_eq!(
                    renderer.stats(),
                    layers_before,
                    "cleanup preserves lifetime counters"
                );
                assert_eq!(renderer.backdrop_stats(), backdrops_before);
            }
            let restored = renderer.render_scene_to_image(&replay, extent).unwrap();
            assert_eq!(restored.as_raw(), original.as_raw());
            assert_eq!(renderer.effect_texture_count(), textures);
            assert_eq!(
                renderer.stats().captures,
                layers_before.captures + u64::from(group)
            );
            assert_eq!(
                renderer.stats().hits,
                layers_before.hits,
                "released raw pixels must be recaptured"
            );
            assert_eq!(
                renderer.backdrop_stats().captures,
                backdrops_before.captures + u64::from(backdrop_enabled)
            );
            assert_eq!(renderer.backdrop_stats().hits, backdrops_before.hits);
        }
    }
}
