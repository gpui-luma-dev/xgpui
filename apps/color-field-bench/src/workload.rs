//! Adapted from gpui-luma-color's paint_vector.rs and SvAtHueModel,
//! gpui-luma revision b5a4a23ce6ed98731c1e3aef0abec02da4879274 (Apache-2.0).
//! Changes: standalone deterministic workload, explicit fast/fallback selection,
//! direct bounded HSV-to-sRGB conversion, and shared UI/offscreen quad output.
use clap::ValueEnum;
use gpui::{
    Bounds, Hsla, PaintQuad, Pixels, Rgba, fill, hsla, linear_color_stop, linear_gradient, point,
    px, size,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    Layered,
    Sampled,
}

pub fn hsv(hue: f32, saturation: f32, value: f32) -> Rgba {
    let h = hue.rem_euclid(360.) / 60.;
    let c = value * saturation;
    let x = c * (1. - (h % 2. - 1.).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = value - c;
    Rgba {
        r: r + m,
        g: g + m,
        b: b + m,
        a: 1.,
    }
}

// Preserve luma-color's aspect-aware sampling and minimum 0.25 scale.
// --samples is a target along the longest axis, not a literal square grid.
pub fn sampling(width: f32, height: f32, samples: usize) -> (u32, u32) {
    let scale = (samples.max(16) as f32 / width.max(height).max(1.)).clamp(0.25, 1.);
    (
        (width * scale).round().max(1.) as u32,
        (height * scale).round().max(1.) as u32,
    )
}

pub fn quads(method: Method, bounds: Bounds<Pixels>, hue: f32, samples: usize) -> Vec<PaintQuad> {
    if method == Method::Layered {
        return vec![
            fill(bounds, hsv(hue, 1., 1.)),
            fill(
                bounds,
                linear_gradient(
                    90.,
                    linear_color_stop(hsla(0., 0., 1., 1.), 0.),
                    linear_color_stop(hsla(0., 0., 1., 0.), 1.),
                ),
            ),
            fill(
                bounds,
                linear_gradient(
                    180.,
                    linear_color_stop(hsla(0., 0., 0., 0.), 0.),
                    linear_color_stop(hsla(0., 0., 0., 1.), 1.),
                ),
            ),
        ];
    }
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let (cols, rows) = sampling(width, height, samples);
    let mut output = Vec::with_capacity((cols * rows) as usize);
    for y in 0..rows {
        let y0 = height * y as f32 / rows as f32;
        let y1 = height * (y + 1) as f32 / rows as f32;
        for x in 0..cols {
            let x0 = width * x as f32 / cols as f32;
            let x1 = width * (x + 1) as f32 / cols as f32;
            let color: Hsla = hsv(
                hue,
                (x as f32 + 0.5) / cols as f32,
                1. - (y as f32 + 0.5) / rows as f32,
            )
            .into();
            output.push(fill(
                Bounds::new(
                    bounds.origin + point(px(x0), px(y0)),
                    size(px(x1 - x0), px(y1 - y0)),
                ),
                color,
            ));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hsv_corners_and_hue_wrap_match_the_sv_model() {
        assert_eq!(hsv(0., 1., 1.), gpui::rgb(0xff0000));
        assert_eq!(hsv(120., 1., 1.), gpui::rgb(0x00ff00));
        assert_eq!(hsv(240., 1., 1.), gpui::rgb(0x0000ff));
        assert_eq!(hsv(360., 1., 1.), hsv(0., 1., 1.));
        assert_eq!(hsv(210., 0., 1.), gpui::rgb(0xffffff));
        assert_eq!(hsv(210., 1., 0.), gpui::rgb(0));
    }
    #[test]
    fn fast_path_is_three_layers_and_fallback_covers_the_rectangle() {
        let bounds = Bounds::new(point(px(11.), px(7.)), size(px(512.), px(384.)));
        assert_eq!(quads(Method::Layered, bounds, 210., 128).len(), 3);
        assert_eq!(sampling(512., 384., 128), (128, 96));
        // The minimum scale is intentional, even with a smaller sample target.
        assert_eq!(sampling(512., 384., 16), (128, 96));
        let output = quads(Method::Sampled, bounds, 210., 128);
        assert_eq!(output.len(), 12288);
        assert_eq!(output.first().unwrap().bounds.origin, bounds.origin);
        assert_eq!(
            output.last().unwrap().bounds.bottom_right(),
            bounds.bottom_right()
        );
        let area: f32 = output
            .iter()
            .map(|q| f32::from(q.bounds.size.width) * f32::from(q.bounds.size.height))
            .sum();
        assert!((area - 512. * 384.).abs() < 0.01);
    }
}
