use std::{fs, path::Path, time::Instant};
use wgpu::util::DeviceExt;
const W: u32 = 96;
const MAX: usize = 8;
// Four vec4 records per shape. Private experiment ABI, not GPUI's ContentMask.
#[derive(Clone, Copy, Debug)]
struct Shape([f32; 16]);
impl Shape {
    fn rounded(b: [f32; 4], mut r: [f32; 4], border: [f32; 4]) -> Self {
        // Normalize outer circular radii with one common scale, then inset
        // geometrically, producing elliptical inner corners. Border: L/T/R/B.
        let k = [
            1.,
            b[2] / (r[0] + r[1]).max(0.0001),
            b[2] / (r[3] + r[2]).max(0.0001),
            b[3] / (r[0] + r[3]).max(0.0001),
            b[3] / (r[1] + r[2]).max(0.0001),
        ]
        .into_iter()
        .fold(1f32, f32::min);
        r.iter_mut().for_each(|v| *v = v.max(0.) * k);
        let mut a = [0.; 16];
        a[..4].copy_from_slice(&[
            b[0] + border[0],
            b[1] + border[1],
            (b[2] - border[0] - border[2]).max(0.),
            (b[3] - border[1] - border[3]).max(0.),
        ]);
        for c in 0..4 {
            a[4 + c] = (r[c] - border[if c == 0 || c == 3 { 0 } else { 2 }]).max(0.);
            a[8 + c] = (r[c] - border[if c < 2 { 1 } else { 3 }]).max(0.);
        }
        Self(a)
    }
    fn ellipse(b: [f32; 4]) -> Self {
        let mut a = [0.; 16];
        a[..4].copy_from_slice(&b);
        a[12] = 1.;
        Self(a)
    }
    fn contains(&self, p: [f32; 2]) -> bool {
        let a = self.0;
        let q = [p[0] - a[0], p[1] - a[1]];
        if a[2] <= 0. || a[3] <= 0. || q[0] < 0. || q[1] < 0. || q[0] > a[2] || q[1] > a[3] {
            return false;
        }
        if a[12] == 1. {
            return ((q[0] - a[2] * 0.5) / (a[2] * 0.5)).powi(2)
                + ((q[1] - a[3] * 0.5) / (a[3] * 0.5)).powi(2)
                <= 1.;
        }
        let centers = [
            [a[4], a[8]],
            [a[2] - a[5], a[9]],
            [a[2] - a[6], a[3] - a[10]],
            [a[7], a[3] - a[11]],
        ];
        for c in 0..4 {
            let rx = a[4 + c];
            let ry = a[8 + c];
            if rx == 0. || ry == 0. {
                continue;
            }
            let d = [q[0] - centers[c][0], q[1] - centers[c][1]];
            let sx = if c == 1 || c == 2 { 1. } else { -1. };
            let sy = if c >= 2 { 1. } else { -1. };
            if d[0] * sx > 0. && d[1] * sy > 0. && (d[0] / rx).powi(2) + (d[1] / ry).powi(2) > 1. {
                return false;
            }
        }
        true
    }
}
fn coverage(shapes: &[Shape], x: u32, y: u32, samples: u32) -> f32 {
    let mut n = 0;
    for sy in 0..samples {
        for sx in 0..samples {
            let p = [
                x as f32 + (sx as f32 + 0.5) / samples as f32,
                y as f32 + (sy as f32 + 0.5) / samples as f32,
            ];
            n += u32::from(shapes.iter().all(|s| s.contains(p)));
        }
    }
    n as f32 / (samples * samples) as f32
}
fn checked_chain(shapes: &[Shape]) -> Result<(), &'static str> {
    if shapes.len() > MAX {
        return Err("clip-chain probe supports at most eight shapes; no rectangle fallback");
    }
    if shapes
        .iter()
        .any(|s| s.0.iter().any(|v| !v.is_finite()) || s.0[2] <= 0. || s.0[3] <= 0.)
    {
        return Err("invalid or empty geometry (production empty clips must cull)");
    }
    Ok(())
}
fn fixtures() -> Vec<(String, Vec<Shape>)> {
    let r = Shape::rounded([8.25, 9.5, 76., 72.], [24., 8., 31., 16.], [0.; 4]);
    let e = Shape::ellipse([15.5, 4.25, 69., 83.]);
    let mut cases = vec![
        ("unclipped".into(), vec![]),
        (
            "rectangle".into(),
            vec![Shape::rounded([9.25, 9.5, 74., 72.], [0.; 4], [0.; 4])],
        ),
        ("per-corner".into(), vec![r]),
        ("ellipse".into(), vec![e]),
        ("nested".into(), vec![r, e]),
        ("coincident".into(), vec![r, r]),
        ("depth-eight".into(), vec![r; 8]),
        (
            "oversized-border".into(),
            vec![Shape::rounded(
                [4.25, 5.5, 84., 81.],
                [95., 80., 70., 120.],
                [7., 3., 11., 5.],
            )],
        ),
        (
            "disjoint".into(),
            vec![
                Shape::ellipse([2., 2., 20., 20.]),
                Shape::ellipse([60., 60., 20., 20.]),
            ],
        ),
    ];
    for scale in [1.25, 1.5, 2.] {
        let mut s = r;
        s.0[..12].iter_mut().for_each(|v| *v *= scale);
        cases.push((format!("dpi-{scale}").replace('.', "_"), vec![s]));
    }
    cases
}
fn main() {
    pollster::block_on(run());
}
async fn run() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: clip-chain-probe OUTPUT_DIRECTORY");
    fs::create_dir_all(&out).unwrap();
    fs::write(
        Path::new(&out).join("demo.html"),
        include_str!("../demo.html"),
    )
    .unwrap();
    fs::write(
        Path::new(&out).join("names.txt"),
        fixtures()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&Default::default())
        .await
        .expect("GPU adapter");
    println!("adapter: {:?}", adapter.get_info());
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut results = String::new();
    let json = fixtures()
        .iter()
        .map(|(name, shapes)| {
            let records: Vec<[f32; 16]> = shapes.iter().map(|s| s.0).collect();
            format!("{{\"name\":\"{name}\",\"shapes\":{records:?}}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        Path::new(&out).join("fixtures.js"),
        format!("const FIXTURES=[{json}];"),
    )
    .unwrap();
    for textured in [false, true] {
        let transport = if textured {
            "@group(0) @binding(0) var records: texture_2d<u32>;\nfn record(i:u32)->vec4<f32>{return bitcast<vec4<f32>>(textureLoad(records,vec2<i32>(i32(i),0),0));}"
        } else {
            "@group(0) @binding(0) var<storage,read> records: array<vec4<f32>>;\nfn record(i:u32)->vec4<f32>{return records[i];}"
        };
        let source = include_str!("../clip.wgsl").replace("__TRANSPORT__", transport);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clip probe"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clip probe"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R32Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        for (name, shapes) in fixtures() {
            checked_chain(&shapes).unwrap();
            let mut words = vec![0f32; MAX * 16];
            for (i, s) in shapes.iter().enumerate() {
                words[i * 16..i * 16 + 16].copy_from_slice(&s.0);
            }
            let data = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&words),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let input = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: (MAX * 4) as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                input.as_image_copy(),
                bytemuck::cast_slice(&words),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: None,
                    rows_per_image: None,
                },
                input.size(),
            );
            let input_view = input.create_view(&Default::default());
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&[shapes.len() as u32, 0, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: if textured {
                            wgpu::BindingResource::TextureView(&input_view)
                        } else {
                            data.as_entire_binding()
                        },
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: params.as_entire_binding(),
                    },
                ],
            });
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: W,
                    height: W,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (512 * W) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let start = Instant::now();
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(512),
                        rows_per_image: None,
                    },
                },
                target.size(),
            );
            queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            read.map_async(wgpu::MapMode::Read, .., move |r| {
                tx.send(r).unwrap();
            });
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            rx.recv().unwrap().unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            let mapped = read.get_mapped_range(..).unwrap();
            let mut expected = Vec::new();
            let mut max_error = 0f32;
            let mut sum = 0f64;
            let mut worst = 0f32;
            for y in 0..W {
                for x in 0..W {
                    let offset = (y * 512 + x * 4) as usize;
                    let gpu = f32::from_le_bytes(mapped[offset..offset + 4].try_into().unwrap());
                    let cpu = coverage(&shapes, x, y, 4);
                    expected.push(cpu);
                    max_error = max_error.max((gpu - cpu).abs());
                    let error = (cpu - coverage(&shapes, x, y, 32)).abs();
                    sum += error as f64;
                    worst = worst.max(error);
                }
            }
            assert!(
                max_error <= 0.0625,
                "{name}: GPU disagrees by more than one sample: {max_error}"
            );
            let mode = if textured { "uint-texture" } else { "storage" };
            let line = format!(
                "{mode}/{name}: gpu_cpu_max={max_error:.6}, reference_mae={:.6}, reference_max={worst:.6}, submit_wait_readback_ms={elapsed:.3}\n",
                sum / (W * W) as f64
            );
            print!("{line}");
            results.push_str(&line);
            if !textured {
                let base = Path::new(&out).join(&name);
                fs::write(base.with_extension("shapes"), bytemuck::cast_slice(&words)).unwrap();
                fs::write(
                    base.with_extension("expected"),
                    bytemuck::cast_slice(&expected),
                )
                .unwrap();
                fs::write(base.with_extension("count"), shapes.len().to_string()).unwrap();
            }
        }
    }
    fs::write(Path::new(&out).join("wgpu-results.txt"), results).unwrap();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coincidence_is_idempotent_and_corners_reject_input() {
        let r = fixtures()[2].1[0];
        assert!(!r.contains([8.25, 9.5]));
        assert!(r.contains([45., 45.]));
        let mut product_darkening = 0f32;
        for y in 0..W {
            for x in 0..W {
                let alpha = coverage(&[r], x, y, 4);
                product_darkening = product_darkening.max(alpha - alpha * alpha);
                assert_eq!(coverage(&[r], x, y, 4), coverage(&[r, r], x, y, 4));
            }
        }
        assert!(
            product_darkening >= 0.24,
            "negative control must expose multiplied-edge darkening"
        );
    }
    #[test]
    fn bounded_chain_rejects_overflow_and_invalid_geometry() {
        let r = fixtures()[2].1[0];
        assert!(checked_chain(&[r; 8]).is_ok());
        assert!(checked_chain(&[r; 9]).is_err());
        assert!(checked_chain(&[Shape::ellipse([0., 0., 0., 10.])]).is_err());
        assert!(checked_chain(&[Shape::ellipse([f32::NAN, 0., 10., 10.])]).is_err());
    }
    #[test]
    fn insets_are_geometric_and_empty_clip_is_rejected() {
        let s = Shape::rounded([0., 0., 40., 20.], [30.; 4], [4., 2., 6., 3.]);
        assert_eq!(&s.0[..4], &[4., 2., 30., 15.]);
        assert_eq!(&s.0[4..8], &[6., 4., 4., 6.]);
        assert!(!s.contains([4., 2.]));
    }
}
