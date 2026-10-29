//! Ordered sRGB UI runs and associated linear-P3 SDR composition.
use super::*;
use crate::metal;
use crate::metal::*;
use anyhow::ensure;
use std::collections::HashSet;

pub(super) fn validate_viewport(viewport: Size<DevicePixels>) -> Result<()> {
    ensure!(
        viewport.width.0 > 0
            && viewport.height.0 > 0
            && viewport.width.0 <= 16384
            && viewport.height.0 <= 16384,
        "invalid wide SDR viewport"
    );
    let bytes = viewport.width.0 as usize * viewport.height.0 as usize * 12;
    ensure!(
        bytes <= 256 * 1024 * 1024,
        "wide SDR targets exceed 256 MiB"
    );
    Ok(())
}
pub(super) fn configure_layer(layer: &metal::MetalLayerRef) -> Result<()> {
    use objc2_core_graphics::{CGColorSpace, kCGColorSpaceExtendedLinearDisplayP3};
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceExtendedLinearDisplayP3 }))
        .ok_or_else(|| anyhow::anyhow!("linear P3 color space unavailable"))?;
    layer.setPixelFormat(MTLPixelFormat::RGBA16Float);
    // CAMetalLayer retains the color space independently of this local owner.
    layer.setColorspace(Some(&space));
    layer.setWantsExtendedDynamicRangeContent(false);
    Ok(())
}

pub(super) struct Compositor {
    pub wide: HashSet<usize>,
    compose: metal::RenderPipelineState,
    preview: metal::RenderPipelineState,
    scratch: Option<metal::Texture>,
    target: Option<metal::Texture>,
}
impl Compositor {
    pub fn new(device: &metal::DeviceRef) -> Result<Self> {
        let library = device
            .newLibraryWithSource_options_error(
                &objc2_foundation::NSString::from_str(include_str!("wide_sdr.metal")),
                Some(&metal::CompileOptions::new()),
            )
            .map_err(anyhow::Error::msg)?;
        let pipeline = |fragment: &str, format, blend| -> Result<metal::RenderPipelineState> {
            let d = metal::RenderPipelineDescriptor::new();
            let vertex = library
                .newFunctionWithName(&objc2_foundation::NSString::from_str("gamut_vertex"))
                .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
                .map_err(anyhow::Error::msg)?;
            let fragment = library
                .newFunctionWithName(&objc2_foundation::NSString::from_str(fragment))
                .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
                .map_err(anyhow::Error::msg)?;
            d.setVertexFunction(Some(&vertex));
            d.setFragmentFunction(Some(&fragment));
            let c = unsafe { d.colorAttachments().objectAtIndexedSubscript(0) };
            c.setPixelFormat(format);
            c.setBlendingEnabled(blend);
            c.setSourceRGBBlendFactor(metal::MTLBlendFactor::One);
            c.setSourceAlphaBlendFactor(metal::MTLBlendFactor::One);
            c.setDestinationRGBBlendFactor(metal::MTLBlendFactor::OneMinusSourceAlpha);
            c.setDestinationAlphaBlendFactor(metal::MTLBlendFactor::OneMinusSourceAlpha);
            device
                .newRenderPipelineStateWithDescriptor_error(&d)
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
            t.width() == viewport.width.0 as usize && t.height() == viewport.height.0 as usize
        }) {
            return Ok(());
        }
        let texture = |format| {
            let d = metal::TextureDescriptor::new();
            unsafe { d.setWidth(viewport.width.0 as usize) };
            unsafe { d.setHeight(viewport.height.0 as usize) };
            d.setPixelFormat(format);
            d.setUsage(metal::MTLTextureUsage::ShaderRead | metal::MTLTextureUsage::RenderTarget);
            d.setStorageMode(metal::MTLStorageMode::Private);
            renderer
                .device
                .newTextureWithDescriptor(&d)
                .expect("Metal texture allocation failed")
        };
        self.scratch = Some(texture(MTLPixelFormat::BGRA8Unorm));
        self.target = Some(texture(MTLPixelFormat::RGBA16Float));
        Ok(())
    }
    fn params(
        viewport: Size<DevicePixels>,
        draw: Option<&gpui::CustomDraw>,
        mode: f32,
    ) -> [[f32; 4]; 9] {
        let base = if let Some(d) = draw {
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
        };
        let adjustment = draw.and_then(|d| d.content.image_linear_adjustments());
        let rows = adjustment.map(|a| a.rows()).unwrap_or([
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
        ]);
        [
            base[0],
            base[1],
            base[2],
            base[3],
            rows[0],
            rows[1],
            rows[2],
            adjustment.map(|a| a.vignette()).unwrap_or([0., 0., 1., 1.]),
            [
                if adjustment.is_some() { 1. } else { 0. },
                if adjustment.is_some_and(|a| a.is_p3()) {
                    1.
                } else {
                    0.
                },
                0.,
                0.,
            ],
        ]
    }
    fn draw(
        &self,
        encoder: &metal::RenderCommandEncoderRef,
        image: &metal::TextureRef,
        mask: Option<&metal::TextureRef>,
        params: [[f32; 4]; 9],
        preview: bool,
    ) {
        encoder.setRenderPipelineState(if preview {
            &self.preview
        } else {
            &self.compose
        });
        unsafe {
            encoder.setVertexBytes_length_atIndex(
                std::ptr::NonNull::new(params.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                mem::size_of_val(&params) as usize,
                0,
            )
        };
        unsafe {
            encoder.setFragmentBytes_length_atIndex(
                std::ptr::NonNull::new(params.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                    .expect("non-null bytes"),
                mem::size_of_val(&params) as usize,
                0,
            )
        };
        unsafe { encoder.setFragmentTexture_atIndex(Some(image), 0) };
        unsafe { encoder.setFragmentTexture_atIndex(Some(mask.unwrap_or(image)), 1) };
        unsafe {
            encoder.drawPrimitives_vertexStart_vertexCount(metal::MTLPrimitiveType::Triangle, 0, 6)
        };
    }
    fn flush(
        &self,
        command: &metal::CommandBufferRef,
        ui: &mut Option<metal::RenderCommandEncoder>,
        viewport: Size<DevicePixels>,
    ) {
        if let Some(encoder) = ui.take() {
            encoder.endEncoding();
            let encoder = new_command_encoder_for_texture(
                command,
                self.target.as_ref().unwrap(),
                viewport,
                None,
            );
            self.draw(
                &encoder,
                self.scratch.as_ref().unwrap(),
                None,
                Self::params(viewport, None, 1.),
                false,
            );
            encoder.endEncoding();
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
            crate::custom_draw::PreparedMetalOutput,
            Option<metal::Texture>,
            bool,
        )],
    ) -> Result<metal::CommandBuffer> {
        ensure!(
            renderer.opaque,
            "wide SDR does not support transparent top-level windows"
        );
        self.ensure_targets(renderer, viewport)?;
        let queue = renderer.command_queue.clone();
        let command = queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed");
        let initial = new_command_encoder_for_texture(
            &command,
            self.target.as_ref().unwrap(),
            viewport,
            Some(MTLClearColor {
                red: 0.,
                green: 0.,
                blue: 0.,
                alpha: 1.,
            }),
        );
        initial.endEncoding();
        let mut ui = None;
        for batch in scene.batches() {
            if let PrimitiveBatch::Custom(range) = &batch {
                for index in range.clone() {
                    if self.wide.contains(&index) {
                        self.flush(&command, &mut ui, viewport);
                        let e = new_command_encoder_for_texture(
                            &command,
                            self.target.as_ref().unwrap(),
                            viewport,
                            None,
                        );
                        let mask = outputs[index].1.as_deref();
                        self.draw(
                            &e,
                            &outputs[index].0.texture,
                            mask,
                            Self::params(
                                viewport,
                                Some(&scene.custom_draws[index]),
                                if mask.is_some() { -1. } else { 0. },
                            ),
                            false,
                        );
                        e.endEncoding();
                    } else {
                        let e = ui
                            .get_or_insert_with(|| {
                                new_command_encoder_for_texture(
                                    &command,
                                    self.scratch.as_ref().unwrap(),
                                    viewport,
                                    Some(MTLClearColor {
                                        red: 0.,
                                        green: 0.,
                                        blue: 0.,
                                        alpha: 0.,
                                    }),
                                )
                            })
                            .clone();
                        renderer.custom_compositor.as_ref().unwrap().draw(
                            &scene.custom_draws[index],
                            &outputs[index].0,
                            outputs[index].1.as_deref(),
                            outputs[index].2,
                            viewport,
                            &e,
                        );
                    }
                }
                continue;
            }
            if let PrimitiveBatch::Shaders {
                program_id,
                ref range,
            } = batch
            {
                if scene.shader_draws[range.start].program.output_encoding()
                    == gpui::ShaderOutputEncoding::LinearDisplayP3
                {
                    self.flush(&command, &mut ui, viewport);
                    let e = new_command_encoder_for_texture(
                        &command,
                        self.target.as_ref().unwrap(),
                        viewport,
                        None,
                    );
                    renderer.draw_shaders(
                        program_id,
                        range.clone(),
                        bindings,
                        viewport,
                        scene.shader_draws[range.start].glyph_mask,
                        false,
                        &e,
                    );
                    e.endEncoding();
                    continue;
                }
            }
            let mut e = ui
                .get_or_insert_with(|| {
                    new_command_encoder_for_texture(
                        &command,
                        self.scratch.as_ref().unwrap(),
                        viewport,
                        Some(MTLClearColor {
                            red: 0.,
                            green: 0.,
                            blue: 0.,
                            alpha: 0.,
                        }),
                    )
                })
                .clone();
            match batch {
                PrimitiveBatch::Shaders { program_id, range } => renderer.draw_shaders(
                    program_id,
                    range.clone(),
                    bindings,
                    viewport,
                    scene.shader_draws[range.start].glyph_mask,
                    false,
                    &e,
                ),
                PrimitiveBatch::Quads(r) => renderer.draw_quads(r, bindings, viewport, &e),
                PrimitiveBatch::Shadows(r) => renderer.draw_shadows(r, bindings, viewport, &e),
                PrimitiveBatch::Underlines(r) => {
                    renderer.draw_underlines(r, bindings, viewport, &e)
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                    renderer.draw_monochrome_sprites(texture_id, range, bindings, viewport, &e)
                }
                PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                    renderer.draw_polychrome_sprites(texture_id, range, bindings, viewport, &e)
                }
                PrimitiveBatch::Surfaces(r) => {
                    if let Err(error) = renderer.draw_surfaces(
                        &scene.surfaces[r.clone()],
                        r.start,
                        bindings,
                        viewport,
                        &e,
                        &command,
                    ) {
                        e.endEncoding();
                        return Err(error);
                    }
                }
                PrimitiveBatch::Paths(r) => {
                    e.endEncoding();
                    let paths = &scene.paths[r];
                    let did_draw =
                        renderer.draw_paths_to_intermediate(paths, writer, viewport, &command)?;
                    e = new_command_encoder_for_texture(
                        &command,
                        self.scratch.as_ref().unwrap(),
                        viewport,
                        None,
                    );
                    ui = Some(e.clone());
                    if did_draw {
                        if let Err(error) =
                            renderer.draw_paths_from_intermediate(paths, writer, viewport, &e)
                        {
                            e.endEncoding();
                            return Err(error);
                        }
                    }
                }
                PrimitiveBatch::SubpixelSprites { .. } => {
                    e.endEncoding();
                    anyhow::bail!("wide SDR requires grayscale text")
                }
                PrimitiveBatch::Custom(_)
                | PrimitiveBatch::Backdrop(_)
                | PrimitiveBatch::LayerBoundary(_) => unreachable!(),
            }
        }
        self.flush(&command, &mut ui, viewport);
        if destination.pixelFormat() == MTLPixelFormat::RGBA16Float {
            let blit = command
                .blitCommandEncoder()
                .expect("Metal blit encoder allocation failed");
            unsafe {
                blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(self.target.as_ref().unwrap(), 0, 0, metal::MTLOrigin { x: 0, y: 0, z: 0 }, metal::MTLSize {
                    width: viewport.width.0 as usize,
                    height: viewport.height.0 as usize,
                    depth: 1,
                }, destination, 0, 0, metal::MTLOrigin { x: 0, y: 0, z: 0 })
            };
            blit.endEncoding();
        } else {
            ensure!(
                destination.pixelFormat() == MTLPixelFormat::BGRA8Unorm,
                "unsupported wide SDR destination"
            );
            let e = new_command_encoder_for_texture(
                &command,
                destination,
                viewport,
                Some(MTLClearColor {
                    red: 0.,
                    green: 0.,
                    blue: 0.,
                    alpha: 1.,
                }),
            );
            self.draw(
                &e,
                self.target.as_ref().unwrap(),
                None,
                Self::params(viewport, None, 0.),
                true,
            );
            e.endEncoding();
        }
        Ok(command.to_owned())
    }
}

#[cfg(test)]
mod tests;
