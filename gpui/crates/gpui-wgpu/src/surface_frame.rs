//! Copyable frame storage for presentation surfaces without COPY_SRC (WebGL2).
use super::*;
use wgpu::*;

pub(super) struct SurfaceFrame {
    pub view: TextureView,
    pipeline: RenderPipeline,
    binding: BindGroup,
}
impl SurfaceFrame {
    pub fn new(device: &Device, format: TextureFormat, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("copyable effect frame"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_SRC
                | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("effect frame presentation"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: false },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let binding = device.create_bind_group(&BindGroupDescriptor {
            label: Some("effect frame presentation"),
            layout: &layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            }],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("effect frame presentation"),
            source: ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var frame: texture_2d<f32>;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * vec2<f32>(2., -2.) + vec2<f32>(-1., 1.), 0., 1.);
}
@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(frame, vec2<i32>(p.xy), 0);
}
"#
                .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("effect frame presentation"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("effect frame presentation"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            view,
            pipeline,
            binding,
        }
    }
    pub fn present(&self, device: &Device, queue: &Queue, target: &TextureView) -> SubmissionIndex {
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("effect frame presentation"),
        });
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("effect frame presentation"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()))
    }
}

impl WgpuRendererCore {
    pub(super) fn render_surface_frame(
        &mut self,
        scene: &Scene,
        target: &TextureView,
        size: Size<DevicePixels>,
        premultiplied: bool,
    ) -> Result<SubmissionIndex> {
        let needs_copy = !scene.backdrops.is_empty() || !scene.layer_boundaries.is_empty();
        if !needs_copy || target.texture().usage().contains(TextureUsages::COPY_SRC) {
            self.surface_frame = None;
            return self.render_frame(scene, target, size, premultiplied, Color::TRANSPARENT);
        }
        let mut frame = self.surface_frame.take();
        if frame.as_ref().is_none_or(|f| {
            f.view.texture().width() != size.width.0 as u32
                || f.view.texture().height() != size.height.0 as u32
        }) {
            frame = Some(SurfaceFrame::new(
                &self.resources.device,
                self.target_format,
                size.width.0 as u32,
                size.height.0 as u32,
            ));
        }
        let frame = frame.unwrap();
        let result = self.render_frame(scene, &frame.view, size, premultiplied, Color::TRANSPARENT);
        let result =
            result.map(|_| frame.present(&self.resources.device, &self.resources.queue, target));
        self.surface_frame = Some(frame);
        result
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::*;
    use gpui::{ContentMask, Corners, Quad, point, size};

    #[test]
    fn surface_frame_preserves_pixels_alpha_orientation_and_resize() -> Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        for (width, height) in [(13, 7), (29, 11)] {
            let extent = size(DevicePixels(width), DevicePixels(height));
            let mut scene = Scene::default();
            for (y, h, color) in [
                (0., 2., gpui::rgba(0xe0204080)),
                (2., (height - 2) as f32, gpui::rgba(0x2060e0ff)),
            ] {
                let bounds = Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(y)),
                    size(ScaledPixels(width as f32), ScaledPixels(h)),
                );
                scene.insert_primitive(Quad {
                    bounds,
                    content_mask: ContentMask { bounds },
                    background: color.into(),
                    corner_radii: Corners::default(),
                    ..Default::default()
                });
            }
            scene.finish();
            renderer.ensure_render_target(extent)?;
            let target = renderer.render_target.as_ref().unwrap().view.clone();
            renderer
                .core
                .render_frame(&scene, &target, extent, true, Color::TRANSPARENT)?;
            let reference = renderer.read_image()?;
            let frame = SurfaceFrame::new(
                &renderer.core.resources.device,
                renderer.core.target_format,
                width as u32,
                height as u32,
            );
            renderer
                .core
                .render_frame(&scene, &frame.view, extent, true, Color::TRANSPARENT)?;
            frame.present(
                &renderer.core.resources.device,
                &renderer.core.resources.queue,
                &target,
            );
            assert_eq!(renderer.read_image()?, reference);
            // Presentation also accepts a surface-like target without copy usage.
            let no_copy = renderer
                .core
                .resources
                .device
                .create_texture(&TextureDescriptor {
                    label: Some("surface without COPY_SRC"),
                    size: Extent3d {
                        width: width as u32,
                        height: height as u32,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: renderer.core.target_format,
                    usage: TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
            frame.present(
                &renderer.core.resources.device,
                &renderer.core.resources.queue,
                &no_copy.create_view(&Default::default()),
            );
            renderer.check_gpu_errors()?;
            renderer.core.surface_frame = Some(frame);
            renderer.core.render_surface_frame(
                &Scene::default(),
                &no_copy.create_view(&Default::default()),
                extent,
                true,
            )?;
            assert!(
                renderer.core.surface_frame.is_none(),
                "effect-free frames must release the retained full-window target"
            );
            renderer.check_gpu_errors()?;
        }
        Ok(())
    }
}
