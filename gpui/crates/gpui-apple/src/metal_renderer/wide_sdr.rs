//! Ordered sRGB UI runs and associated extended linear-P3 SDR composition.
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
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn readback(
        &self,
        device: &metal::DeviceRef,
        queue: &metal::CommandQueueRef,
    ) -> Result<gpui::LinearP3Image> {
        let target = self
            .target
            .as_ref()
            .context("no linear P3 composition target")?;
        let width = target.width();
        let height = target.height();
        let stride = (width * 8).div_ceil(256) * 256;
        let output = device
            .newBufferWithLength_options(
                stride * height,
                metal::MTLResourceOptions::StorageModeShared,
            )
            .context("FP16 readback allocation failed")?;
        let command = queue
            .commandBuffer()
            .context("FP16 readback command allocation failed")?;
        let blit = command
            .blitCommandEncoder()
            .context("FP16 readback encoder allocation failed")?;
        // The copy follows scene submission on the same queue. Shared buffers work
        // on both discrete and unified-memory Macs; no CPU read of a private texture.
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage_options(
                target, 0, 0, metal::MTLOrigin { x: 0, y: 0, z: 0 },
                metal::MTLSize { width, height, depth: 1 }, &output, 0, stride, stride * height, metal::MTLBlitOption::empty());
        }
        blit.endEncoding();
        command.commit();
        command.waitUntilCompleted();
        ensure!(
            command.status() == metal::MTLCommandBufferStatus::Completed,
            "FP16 readback GPU command failed"
        );
        // SAFETY: GPU copy completed, the shared buffer is alive, and the slice
        // covers its allocated extent. Remove padded rows from the public result.
        let words = unsafe {
            slice::from_raw_parts(
                output.contents().cast::<u16>().as_ptr(),
                stride * height / 2,
            )
        };
        let pixels = words
            .chunks_exact(stride / 2)
            .flat_map(|row| {
                row[..width * 4]
                    .chunks_exact(4)
                    .map(|p| [p[0], p[1], p[2], p[3]])
            })
            .collect();
        Ok(gpui::LinearP3Image {
            size: size(DevicePixels(width as i32), DevicePixels(height as i32)),
            pixels,
        })
    }

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
                if draw.is_some_and(|d| {
                    d.content.image_color_mode() == gpui::ImageColorMode::LinearRec2020Wide
                }) {
                    1.
                } else {
                    0.
                },
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
        if let Some(program) = scene
            .backdrops
            .first()
            .map(|d| &d.program)
            .or_else(|| scene.layer_boundaries.first().map(|b| &b.capture.program))
        {
            if renderer
                .backdrops
                .as_ref()
                .is_none_or(|b| b.format() != MTLPixelFormat::RGBA16Float)
            {
                renderer.backdrops = Some(backdrop::Backdrops::with_format(
                    &renderer.device,
                    program,
                    MTLPixelFormat::RGBA16Float,
                )?);
            }
            renderer.backdrops.as_mut().unwrap().prepare(
                &renderer.device,
                scene,
                viewport,
                &[renderer.opaque as u64, 2],
            )?;
        }
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
        let mut executed_groups = Vec::new();
        let mut executed_backdrops = Vec::new();
        let mut skip_group_end = None;
        for batch in scene.batches() {
            if let Some(end) = skip_group_end {
                if !matches!(&batch, PrimitiveBatch::LayerBoundary(index) if *index == end) {
                    continue;
                }
                skip_group_end = None;
            }
            // Captures observe all preceding UI and wide draws in linear P3.
            // End isolation and restore the saved destination before subsequent UI.
            if let PrimitiveBatch::LayerBoundary(index) = &batch {
                self.flush(&command, &mut ui, viewport);
                let effects = renderer.backdrops.as_mut().unwrap();
                let (slot, begin) = effects.groups.boundary(*index);
                if begin {
                    effects.groups.start(slot);
                    executed_groups.push(slot);
                }
                let hit = effects.groups.hit(slot);
                if begin || !hit {
                    effects
                        .groups
                        .copy(&command, self.target.as_ref().unwrap(), slot, begin);
                }
                if !begin {
                    effects.group_filter(&command, slot);
                }
                if begin && hit {
                    skip_group_end = Some(effects.groups.end(slot));
                } else {
                    let e = new_command_encoder_for_texture(
                        &command,
                        self.target.as_ref().unwrap(),
                        viewport,
                        None,
                    );
                    effects.group_draw(&e, slot, begin);
                    e.endEncoding();
                }
                continue;
            }
            if let PrimitiveBatch::Backdrop(index) = &batch {
                self.flush(&command, &mut ui, viewport);
                let effects = renderer.backdrops.as_mut().unwrap();
                executed_backdrops.push(*index);
                effects.filter(&command, self.target.as_ref().unwrap(), *index);
                let e = new_command_encoder_for_texture(
                    &command,
                    self.target.as_ref().unwrap(),
                    viewport,
                    None,
                );
                effects.composite(&e, *index);
                e.endEncoding();
                continue;
            }
            if let PrimitiveBatch::Shaders { ref range, .. } = batch
                && scene.shader_draws[range.start].path_mask.is_some()
            {
                self.flush(&command, &mut ui, viewport);
                let paths: Vec<_> = scene.shader_draws[range.clone()]
                    .iter()
                    .map(|d| d.path_mask.as_ref().unwrap().as_ref().clone())
                    .collect();
                if !renderer.draw_paths_to_intermediate(&paths, writer, viewport, &command)? {
                    continue;
                }
            }
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
                    != gpui::ShaderOutputEncoding::EncodedSrgb
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
                        scene.shader_draws[range.start].path_mask.is_some(),
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
                    scene.shader_draws[range.start].path_mask.is_some(),
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
        if let Some(effects) = &mut renderer.backdrops {
            effects.groups.commit(&executed_groups);
            effects.cache.commit(&executed_backdrops);
        }
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
