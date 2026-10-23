//! Debug/test-support experiment only. No exported configuration or output API.
//! The lab-only tuple is an explicit, versioned color contract, never inferred
//! from a texture's format. Replace it with a reviewed API before production.
use super::*;
use anyhow::ensure;
use std::collections::HashSet;

pub(super) type WideOutput = (crate::custom_draw::MetalCustomOutput, [u32; 4]);
pub(super) const WIDE_TAG: [u32; 4] = [0x50335344, 1, 1, 0]; // P3 SDR, v1, associated linear.
pub(super) fn enabled() -> bool {
    std::env::var_os("XGPUI_GAMUT_LAB").is_some_and(|v| v == "1")
}
pub(super) fn validate_viewport(viewport: Size<DevicePixels>) -> Result<()> {
    ensure!(
        viewport.width.0 > 0 && viewport.height.0 > 0,
        "invalid gamut-lab viewport"
    );
    let bytes = viewport.width.0 as u64 * viewport.height.0 as u64 * 12;
    ensure!(
        bytes <= 256 * 1024 * 1024,
        "gamut-lab targets exceed 256 MiB"
    );
    Ok(())
}
pub(super) fn configure_layer(layer: &metal::MetalLayerRef) {
    use core_graphics::color_space::{CGColorSpace, kCGColorSpaceExtendedLinearDisplayP3};
    let space = CGColorSpace::create_with_name(unsafe { kCGColorSpaceExtendedLinearDisplayP3 })
        .expect("linear P3 color space");
    layer.set_pixel_format(MTLPixelFormat::RGBA16Float);
    // Both wrappers borrow the same native objects; CAMetalLayer retains the
    // color space. The typed binding also verifies the correct Objective-C ABI.
    let native: &objc2_quartz_core::CAMetalLayer = unsafe { &*layer.as_ptr().cast() };
    native.setColorspace(Some(unsafe { &*space.as_ptr().cast() }));
    native.setWantsExtendedDynamicRangeContent(false);
}

pub(super) struct Prototype {
    pub wide: HashSet<usize>,
    compose: metal::RenderPipelineState,
    preview: metal::RenderPipelineState,
    scratch: Option<metal::Texture>,
    target: Option<metal::Texture>,
}
impl Prototype {
    pub fn new(device: &metal::DeviceRef) -> Result<Self> {
        let library = device
            .new_library_with_source(
                include_str!("gamut_lab.metal"),
                &metal::CompileOptions::new(),
            )
            .map_err(anyhow::Error::msg)?;
        let pipeline = |fragment: &str, format, blend| -> Result<metal::RenderPipelineState> {
            let d = metal::RenderPipelineDescriptor::new();
            let vertex = library
                .get_function("gamut_vertex", None)
                .map_err(anyhow::Error::msg)?;
            let fragment = library
                .get_function(fragment, None)
                .map_err(anyhow::Error::msg)?;
            d.set_vertex_function(Some(&vertex));
            d.set_fragment_function(Some(&fragment));
            let c = d.color_attachments().object_at(0).unwrap();
            c.set_pixel_format(format);
            c.set_blending_enabled(blend);
            c.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
            c.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
            c.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
            c.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
            device
                .new_render_pipeline_state(&d)
                .map_err(anyhow::Error::msg)
        };
        Ok(Self {
            wide: HashSet::new(),
            compose: pipeline("gamut_fragment", MTLPixelFormat::RGBA16Float, true)?,
            preview: pipeline("gamut_preview", MTLPixelFormat::BGRA8Unorm, false)?,
            scratch: None,
            target: None,
        })
    }
    fn ensure_targets(
        &mut self,
        renderer: &MetalRenderer,
        viewport: Size<DevicePixels>,
    ) -> Result<()> {
        validate_viewport(viewport)?;
        if self.scratch.as_ref().is_some_and(|t| {
            t.width() == viewport.width.0 as u64 && t.height() == viewport.height.0 as u64
        }) {
            return Ok(());
        }
        let texture = |format| {
            let d = metal::TextureDescriptor::new();
            d.set_width(viewport.width.0 as u64);
            d.set_height(viewport.height.0 as u64);
            d.set_pixel_format(format);
            d.set_usage(metal::MTLTextureUsage::ShaderRead | metal::MTLTextureUsage::RenderTarget);
            d.set_storage_mode(metal::MTLStorageMode::Private);
            renderer.device.new_texture(&d)
        };
        self.scratch = Some(texture(MTLPixelFormat::BGRA8Unorm));
        self.target = Some(texture(MTLPixelFormat::RGBA16Float));
        Ok(())
    }
    fn params(
        viewport: Size<DevicePixels>,
        draw: Option<&gpui::CustomDraw>,
        mode: f32,
    ) -> [[f32; 4]; 4] {
        if let Some(d) = draw {
            [
                [
                    d.bounds.origin.x.0,
                    d.bounds.origin.y.0,
                    d.bounds.size.width.0,
                    d.bounds.size.height.0,
                ],
                [
                    d.content_mask.bounds.origin.x.0,
                    d.content_mask.bounds.origin.y.0,
                    d.content_mask.bounds.size.width.0,
                    d.content_mask.bounds.size.height.0,
                ],
                [
                    d.corner_radii.top_left.0,
                    d.corner_radii.top_right.0,
                    d.corner_radii.bottom_right.0,
                    d.corner_radii.bottom_left.0,
                ],
                [
                    viewport.width.0 as f32,
                    viewport.height.0 as f32,
                    d.opacity,
                    mode,
                ],
            ]
        } else {
            let w = viewport.width.0 as f32;
            let h = viewport.height.0 as f32;
            [[0., 0., w, h], [0., 0., w, h], [0.; 4], [w, h, 1., mode]]
        }
    }
    fn draw(
        &self,
        encoder: &metal::RenderCommandEncoderRef,
        image: &metal::TextureRef,
        mask: Option<&metal::TextureRef>,
        params: [[f32; 4]; 4],
        preview: bool,
    ) {
        encoder.set_render_pipeline_state(if preview {
            &self.preview
        } else {
            &self.compose
        });
        encoder.set_vertex_bytes(0, mem::size_of_val(&params) as u64, params.as_ptr().cast());
        encoder.set_fragment_bytes(0, mem::size_of_val(&params) as u64, params.as_ptr().cast());
        encoder.set_fragment_texture(0, Some(image));
        encoder.set_fragment_texture(1, Some(mask.unwrap_or(image)));
        encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
    }
    fn flush(
        &self,
        command: &metal::CommandBufferRef,
        ui: &mut Option<&metal::RenderCommandEncoderRef>,
        viewport: Size<DevicePixels>,
    ) {
        if let Some(encoder) = ui.take() {
            encoder.end_encoding();
            let encoder = new_command_encoder_for_texture(
                command,
                self.target.as_ref().unwrap(),
                viewport,
                None,
            );
            self.draw(
                encoder,
                self.scratch.as_ref().unwrap(),
                None,
                Self::params(viewport, None, 1.),
                false,
            );
            encoder.end_encoding();
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        renderer: &mut MetalRenderer,
        scene: &Scene,
        bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        destination: &metal::TextureRef,
        viewport: Size<DevicePixels>,
        outputs: &[(
            crate::custom_draw::MetalCustomOutput,
            Option<metal::Texture>,
            bool,
        )],
    ) -> Result<metal::CommandBuffer> {
        ensure!(
            renderer.opaque,
            "gamut-lab does not support transparent top-level windows"
        );
        self.ensure_targets(renderer, viewport)?;
        let queue = renderer.command_queue.clone();
        let command = queue.new_command_buffer();
        let initial = new_command_encoder_for_texture(
            command,
            self.target.as_ref().unwrap(),
            viewport,
            Some(metal::MTLClearColor::new(0., 0., 0., 1.)),
        );
        initial.end_encoding();
        let mut ui = None;
        for batch in scene.batches() {
            if let PrimitiveBatch::Custom(range) = &batch {
                for index in range.clone() {
                    if self.wide.contains(&index) {
                        self.flush(command, &mut ui, viewport);
                        let e = new_command_encoder_for_texture(
                            command,
                            self.target.as_ref().unwrap(),
                            viewport,
                            None,
                        );
                        let mask = outputs[index].1.as_deref();
                        self.draw(
                            e,
                            &outputs[index].0.texture,
                            mask,
                            Self::params(
                                viewport,
                                Some(&scene.custom_draws[index]),
                                if mask.is_some() { -1. } else { 0. },
                            ),
                            false,
                        );
                        e.end_encoding();
                    } else {
                        let e = *ui.get_or_insert_with(|| {
                            new_command_encoder_for_texture(
                                command,
                                self.scratch.as_ref().unwrap(),
                                viewport,
                                Some(metal::MTLClearColor::new(0., 0., 0., 0.)),
                            )
                        });
                        renderer.custom_compositor.as_ref().unwrap().draw(
                            &scene.custom_draws[index],
                            &outputs[index].0,
                            outputs[index].1.as_deref(),
                            outputs[index].2,
                            viewport,
                            e,
                        );
                    }
                }
                continue;
            }
            let mut e = *ui.get_or_insert_with(|| {
                new_command_encoder_for_texture(
                    command,
                    self.scratch.as_ref().unwrap(),
                    viewport,
                    Some(metal::MTLClearColor::new(0., 0., 0., 0.)),
                )
            });
            match batch {
                PrimitiveBatch::Quads(r) => renderer.draw_quads(r, bindings, viewport, e),
                PrimitiveBatch::Shadows(r) => renderer.draw_shadows(r, bindings, viewport, e),
                PrimitiveBatch::Underlines(r) => renderer.draw_underlines(r, bindings, viewport, e),
                PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                    renderer.draw_monochrome_sprites(texture_id, range, bindings, viewport, e)
                }
                PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                    renderer.draw_polychrome_sprites(texture_id, range, bindings, viewport, e)
                }
                PrimitiveBatch::Surfaces(r) => renderer.draw_surfaces(
                    &scene.surfaces[r.clone()],
                    r.start,
                    bindings,
                    viewport,
                    e,
                ),
                PrimitiveBatch::Paths(r) => {
                    e.end_encoding();
                    let paths = &scene.paths[r];
                    let did_draw =
                        renderer.draw_paths_to_intermediate(paths, writer, viewport, command)?;
                    e = new_command_encoder_for_texture(
                        command,
                        self.scratch.as_ref().unwrap(),
                        viewport,
                        None,
                    );
                    ui = Some(e);
                    if did_draw {
                        if let Err(error) =
                            renderer.draw_paths_from_intermediate(paths, writer, viewport, e)
                        {
                            e.end_encoding();
                            return Err(error);
                        }
                    }
                }
                PrimitiveBatch::SubpixelSprites { .. } => {
                    e.end_encoding();
                    anyhow::bail!("gamut-lab requires grayscale text")
                }
                PrimitiveBatch::Custom(_) => unreachable!(),
            }
        }
        self.flush(command, &mut ui, viewport);
        if destination.pixel_format() == MTLPixelFormat::RGBA16Float {
            let blit = command.new_blit_command_encoder();
            blit.copy_from_texture(
                self.target.as_ref().unwrap(),
                0,
                0,
                metal::MTLOrigin { x: 0, y: 0, z: 0 },
                metal::MTLSize {
                    width: viewport.width.0 as u64,
                    height: viewport.height.0 as u64,
                    depth: 1,
                },
                destination,
                0,
                0,
                metal::MTLOrigin { x: 0, y: 0, z: 0 },
            );
            blit.end_encoding();
        } else {
            ensure!(
                destination.pixel_format() == MTLPixelFormat::BGRA8Unorm,
                "unsupported gamut-lab destination"
            );
            let e = new_command_encoder_for_texture(
                command,
                destination,
                viewport,
                Some(metal::MTLClearColor::new(0., 0., 0., 1.)),
            );
            self.draw(
                e,
                self.target.as_ref().unwrap(),
                None,
                Self::params(viewport, None, 0.),
                true,
            );
            e.end_encoding();
        }
        Ok(command.to_owned())
    }
}

#[cfg(test)]
mod tests;
