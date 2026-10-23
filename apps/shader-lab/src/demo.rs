use anyhow::Result;
use xgpui_shader::MeshGrid;

pub fn grid(size: u32, palette: usize, bend: f32, time: f32) -> Result<MeshGrid> {
    let colors = [
        [
            [0.95, 0.20, 0.38],
            [0.66, 0.18, 0.88],
            [0.13, 0.55, 0.98],
            [1., 0.55, 0.12],
            [0.96, 0.78, 0.60],
            [0.10, 0.80, 0.77],
            [0.96, 0.85, 0.22],
            [0.36, 0.82, 0.39],
            [0.24, 0.42, 0.92],
        ],
        [
            [0.10, 0.25, 0.55],
            [0.12, 0.55, 0.72],
            [0.24, 0.85, 0.85],
            [0.10, 0.45, 0.45],
            [0.65, 0.95, 0.76],
            [0.26, 0.64, 0.95],
            [0.15, 0.32, 0.72],
            [0.53, 0.32, 0.85],
            [0.90, 0.55, 0.80],
        ],
        [
            [0.95, 0.20, 0.10],
            [1., 0.48, 0.12],
            [1., 0.80, 0.30],
            [0.68, 0.18, 0.30],
            [1., 0.60, 0.45],
            [0.82, 0.38, 0.66],
            [0.40, 0.12, 0.48],
            [0.74, 0.32, 0.50],
            [0.96, 0.70, 0.62],
        ],
    ];
    let colors = (0..size * size)
        .map(|i| {
            let color = colors[palette % 3][i as usize % 9];
            color.map(|c| c * (0.9 + 0.1 * (time * 0.8 + i as f32 * 0.3).cos()))
        })
        .collect();
    let regular = MeshGrid::regular(size, size, colors)?;
    let mut points = regular.points().to_vec();
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            let p = &mut points[(x + y * size) as usize];
            let phase = x as f32 * 0.4 + y as f32 * 0.3;
            p.position[0] +=
                bend / (size - 1) as f32 + 0.12 / (size - 1) as f32 * (time * 0.9 + phase).sin();
            p.position[1] += 0.12 / (size - 1) as f32 * (time * 0.7 + phase).cos();
        }
    }
    MeshGrid::new(size, size, points)
}

/// Two deliberately different keyframes. Boundary points stay on the boundary.
pub fn morph(size: u32, progress: f32) -> Result<MeshGrid> {
    let t = progress.clamp(0., 1.);
    let a = grid(size, 2, 0., 0.)?;
    let b = grid(size, 1, 0., 0.)?;
    let mut points = a.points().to_vec();
    for (i, p) in points.iter_mut().enumerate() {
        let x = (i as u32 % size) as f32 / (size - 1) as f32;
        let y = (i as u32 / size) as f32 / (size - 1) as f32;
        // Monotone warps move the interior and edge points without crossing them.
        let ax = x.powf(1.8);
        let ay = y.powf(1.6);
        let bx = 1. - (1. - x).powf(1.8);
        let by = 1. - (1. - y).powf(1.6);
        p.position = [ax + (bx - ax) * t, ay + (by - ay) * t];
        for c in 0..3 {
            p.color[c] += (b.points()[i].color[c] - p.color[c]) * t;
        }
    }
    MeshGrid::new(size, size, points)
}

#[cfg(test)]
mod tests {
    #[test]
    fn morph_interpolates_distinct_keyframes() {
        let a = super::morph(3, 0.).unwrap();
        let b = super::morph(3, 1.).unwrap();
        let mid = super::morph(3, 0.5).unwrap();
        assert!(b.points()[4].position[0] - a.points()[4].position[0] > 0.4);
        for ((a, b), m) in a.points().iter().zip(b.points()).zip(mid.points()) {
            for c in 0..2 {
                assert!((m.position[c] - (a.position[c] + b.position[c]) / 2.).abs() < 1e-6);
            }
            for c in 0..3 {
                assert!((m.color[c] - (a.color[c] + b.color[c]) / 2.).abs() < 1e-6);
            }
        }
    }
}
