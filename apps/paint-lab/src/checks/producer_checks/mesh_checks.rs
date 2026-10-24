use super::*;
use xgpui_shader::{MeshGradient, MeshGrid};

// Independent tensor Hermite basis reference (not the shader matrix code).
fn reference(grid: &MeshGrid, x: u32, y: u32, u: f32, v: f32) -> ([f32; 2], [f32; 3]) {
    let basis = |t: f32| {
        [
            2. * t * t * t - 3. * t * t + 1.,
            -2. * t * t * t + 3. * t * t,
            t * t * t - 2. * t * t + t,
            t * t * t - t * t,
        ]
    };
    let a = basis(u);
    let b = basis(v);
    let points = grid.points();
    let w = grid.width() as usize;
    let p00 = points[x as usize + y as usize * w];
    let p10 = points[x as usize + 1 + y as usize * w];
    let p01 = points[x as usize + (y as usize + 1) * w];
    let p11 = points[x as usize + 1 + (y as usize + 1) * w];
    let position = std::array::from_fn(|c| {
        b[0] * (a[0] * p00.position[c]
            + a[1] * p10.position[c]
            + a[2] * p00.u_tangent[c]
            + a[3] * p10.u_tangent[c])
            + b[1]
                * (a[0] * p01.position[c]
                    + a[1] * p11.position[c]
                    + a[2] * p01.u_tangent[c]
                    + a[3] * p11.u_tangent[c])
            + b[2] * (a[0] * p00.v_tangent[c] + a[1] * p10.v_tangent[c])
            + b[3] * (a[0] * p01.v_tangent[c] + a[1] * p11.v_tangent[c])
    });
    let color = std::array::from_fn(|c| {
        b[0] * (a[0] * p00.color[c] + a[1] * p10.color[c])
            + b[1] * (a[0] * p01.color[c] + a[1] * p11.color[c])
    });
    (position, color)
}
pub fn verify_mesh(renderer: &mut dyn gpui::PlatformHeadlessRenderer) -> Result<()> {
    let extent = size(DevicePixels(257), DevicePixels(193));
    let bounds = rect(0., 0., 257., 193.);
    for (width, height) in [(2, 2), (3, 2), (3, 3)] {
        let colors = (0..width * height)
            .map(|i| {
                [
                    0.15 + 0.8 * (i % 3) as f32 / 2.,
                    0.2 + 0.7 * (i % 2) as f32,
                    0.25 + 0.65 * (i % 4) as f32 / 3.,
                ]
            })
            .collect();
        let regular = MeshGrid::regular(width, height, colors)?;
        let mut points = regular.points().to_vec();
        if width == 3 && height == 3 {
            points[4].position = [0.64, 0.39];
            points[4].u_tangent = [0.45, 0.09];
            points[4].v_tangent = [-0.08, 0.45];
        }
        let grid = MeshGrid::new(width, height, points)?;
        let mesh = Arc::new(MeshGradient::new(grid.clone(), 32)?);
        let mut scene = Scene::default();
        scene.insert_primitive(quad(bounds, 0));
        let mut record = draw(bounds, bounds, 1., mesh.clone());
        record.corner_radii = Corners::default();
        scene.insert_primitive(record);
        let length = scene.len();
        scene.finish();
        let image = renderer.render_scene_to_image(&scene, extent)?;
        ensure!(
            image.pixels().all(|p| p.0[..3].iter().any(|c| *c > 20)),
            "mesh has coverage holes at {width}x{height}"
        );
        for y in 0..height - 1 {
            for x in 0..width - 1 {
                for u in [0.25, 0.5, 0.75] {
                    for v in [0.25, 0.5, 0.75] {
                        let (position, expected) = reference(&grid, x, y, u, v);
                        let sample = image
                            .get_pixel(
                                (position[0] * 257.).floor() as u32,
                                (position[1] * 193.).floor() as u32,
                            )
                            .0;
                        for c in 0..3 {
                            ensure!(
                                sample[c].abs_diff((expected[c] * 255.).round() as u8) <= 8,
                                "mesh CPU/GPU mismatch {width}x{height}, patch {x},{y}, uv {u},{v}: actual {sample:?}, expected {expected:?}"
                            );
                        }
                    }
                }
            }
        }
        let mut replay = Scene::default();
        replay.replay(0..length, &scene);
        replay.finish();
        ensure!(
            renderer.render_scene_to_image(&replay, extent)? == image,
            "mesh replay differs"
        );
        ensure!(
            mesh.preparations() == 1 && mesh.pipeline_compilations() == 1,
            "mesh static cache failed"
        );
        // New parameter snapshot and extent reuse pipelines but not mutable output.
        let mut changed = grid.points().to_vec();
        for p in &mut changed {
            p.color = [0.2, 0.8, 0.3];
        }
        let next = Arc::new(
            mesh.with_grid(MeshGrid::new(width, height, changed)?)?
                .with_subdivisions(18)?,
        );
        let mut resized = Scene::default();
        resized.insert_primitive(quad(bounds, 0));
        resized.insert_primitive(draw(rect(0., 0., 65., 33.), bounds, 0.5, next.clone()));
        resized.finish();
        let result = renderer.render_scene_to_image(&resized, extent)?;
        ensure!(
            result.get_pixel(30, 16).0[1].abs_diff(102) <= 2,
            "mesh snapshot/resize/opacity failed"
        );
        ensure!(
            next.pipeline_compilations() == 1,
            "mesh snapshot recompiled pipelines"
        );
        // Old scene still has its original immutable output.
        ensure!(
            renderer.render_scene_to_image(&scene, extent)? == image,
            "mesh snapshot mutated older output"
        );
    }
    // Minimum tessellation, duplicate pending commands, and aborted preparation.
    let mesh = Arc::new(MeshGradient::new(
        MeshGrid::regular(2, 2, vec![[0.2, 0.8, 0.3]; 4])?,
        2,
    )?);
    let mut failed = Scene::default();
    failed.insert_primitive(draw(rect(0., 0., 48., 48.), bounds, 1., mesh.clone()));
    failed.insert_primitive(draw(rect(64., 0., 48., 48.), bounds, 1., mesh.clone()));
    failed.insert_primitive(draw(
        rect(128., 0., 48., 48.),
        bounds,
        1.,
        Arc::new(InvalidOutput),
    ));
    failed.finish();
    ensure!(
        renderer.render_scene_to_image(&failed, extent).is_err(),
        "bad producer should abort mesh frame"
    );
    let mut retry = Scene::default();
    retry.replay(0..2, &failed);
    retry.finish();
    let image = renderer.render_scene_to_image(&retry, extent)?;
    ensure!(
        image.get_pixel(24, 24).0[1].abs_diff(204) <= 2
            && image.get_pixel(88, 24).0[1].abs_diff(204) <= 2,
        "pending mesh outputs did not initialize on retry"
    );
    ensure!(
        mesh.preparations() == 1 && mesh.pipeline_compilations() == 1,
        "retry rebuilt mesh resources"
    );
    Ok(())
}
