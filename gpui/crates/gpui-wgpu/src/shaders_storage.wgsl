// Storage-buffer instance transport. Concatenated after `shaders.wgsl` on backends
// with storage buffer support; `shaders_webgl.wgsl` is the WebGL2 counterpart that
// decodes the same records from a texture instead.
//
// All buffers share `@group(1) @binding(0)` because each pipeline binds only the
// buffer its own entry points read.

@group(1) @binding(0) var<storage, read> b_quads: array<Quad>;
@group(1) @binding(0) var<storage, read> b_shadows: array<Shadow>;
@group(1) @binding(0) var<storage, read> b_path_vertices: array<PathRasterizationVertex>;
@group(1) @binding(0) var<storage, read> b_path_sprites: array<PathSprite>;
@group(1) @binding(0) var<storage, read> b_underlines: array<Underline>;
@group(1) @binding(0) var<storage, read> b_mono_sprites: array<MonochromeSprite>;
@group(1) @binding(0) var<storage, read> b_poly_sprites: array<PolychromeSprite>;

fn load_quad(instance_id: u32) -> Quad {
    return b_quads[instance_id];
}

fn load_shadow(instance_id: u32) -> Shadow {
    return b_shadows[instance_id];
}

fn load_path_vertex(vertex_id: u32) -> PathRasterizationVertex {
    return b_path_vertices[vertex_id];
}

fn load_path_sprite(instance_id: u32) -> PathSprite {
    return b_path_sprites[instance_id];
}

fn load_underline(instance_id: u32) -> Underline {
    return b_underlines[instance_id];
}

fn load_mono_sprite(instance_id: u32) -> MonochromeSprite {
    return b_mono_sprites[instance_id];
}

fn load_poly_sprite(instance_id: u32) -> PolychromeSprite {
    return b_poly_sprites[instance_id];
}

// --- path tiles --- //

struct PathTile {
    paint: u32,
    curve_start: u32,
    curve_count: u32,
    backdrop: i32,
    top_left: vec2<f32>,
    bottom_right: vec2<f32>,
    reference: vec2<f32>,
    curves_top_left: vec2<f32>,
    curves_bottom_right: vec2<f32>,
}

struct PathCurve {
    p0: vec2<f32>,
    p1: vec2<f32>,
    a: vec2<f32>,
    b: vec2<f32>,
    sign: f32,
    pad: u32,
}

struct PathPaint {
    bounds: Bounds,
    content_mask: Bounds,
    color: Background,
    even_odd: u32,
    pad: u32,
}

struct PathTileVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) tile_id: u32,
    // Inter-stage values can't be structs, so `PreparedBackground` is flattened.
    @location(1) @interpolate(flat) background_color0: vec4<f32>,
    @location(2) @interpolate(flat) background_color1: vec4<f32>,
    @location(3) @interpolate(flat) background_solid: vec4<f32>,
    @location(4) @interpolate(flat) curve_box: vec4<f32>,
}

@vertex
fn vs_path_tile(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> PathTileVarying {
    let tile = load_path_tile(instance_id);
    let paint = load_path_paint(tile.paint);

    let mask_top_left = paint.content_mask.origin;
    let mask_bottom_right = paint.content_mask.origin + paint.content_mask.size;
    let top_left = clamp(tile.top_left, mask_top_left, mask_bottom_right);
    let bottom_right = clamp(tile.bottom_right, top_left, mask_bottom_right);
    let position = vec2<f32>(
        select(top_left.x, bottom_right.x, (vertex_id & 1u) != 0u),
        select(top_left.y, bottom_right.y, (vertex_id & 2u) != 0u),
    );

    var out = PathTileVarying();
    out.position = to_device_position_impl(position);
    out.tile_id = instance_id;
    out.curve_box = vec4<f32>(tile.curves_top_left, tile.curves_bottom_right);
    let background = prepare_gradient_color(paint.color.tag, paint.color.color_space, paint.color.solid, paint.color.colors);
    out.background_color0 = background.color0;
    out.background_color1 = background.color1;
    out.background_solid = background.solid;
    return out;
}

// Solves a*t^2 + b*t + c = 0 for the root in [0, 1] on a monotone piece.
fn monotone_quadratic_root(a: f32, b: f32, c: f32, direction: f32) -> f32 {
    let discriminant = max(b * b - 4.0 * a * c, 0.0);
    let denominator = b + direction * sqrt(discriminant);
    if (denominator == 0.0) {
        return 0.0;
    }
    return saturate(-2.0 * c / denominator);
}

// Coverage estimate adapted from https://github.com/EricLengyel/Slug/blob/be3c13eb7d63f9e8aa5c583e42d92c374cb91d98/SlugPixelShader.hlsl
// Copyright (c) 2017 Eric Lengyel.
// Licensed under the MIT license.

// Accumulates the curve's winding along the pixel's leftward and upward rays. Like Slug, a crossing
// within half a pixel of the center counts fractionally.
fn add_curve_winding(curve: PathCurve, center: vec2<f32>, winding: ptr<function, vec2<f32>>,
    weight: ptr<function, vec2<f32>>) {
    let min_x = min(curve.p0.x, curve.p1.x);
    let max_x = max(curve.p0.x, curve.p1.x);

    if (curve.p0.y <= center.y && center.y < curve.p1.y) {
        if (max_x <= center.x - 0.5) {
            (*winding).x += curve.sign;
        } else {
            let t = monotone_quadratic_root(curve.a.y, curve.b.y, curve.p0.y - center.y, 1.0);
            let x = (curve.a.x * t + curve.b.x) * t + curve.p0.x;
            let distance = center.x - x;
            (*winding).x += curve.sign * saturate(distance + 0.5);
            (*weight).x = max((*weight).x, saturate(1.0 - 2.0 * abs(distance)));
        }
    }

    if (min_x <= center.x && center.x < max_x) {
        let direction = select(-1.0, 1.0, curve.p1.x > curve.p0.x);
        if (curve.p1.y <= center.y - 0.5) {
            (*winding).y -= curve.sign * direction;
        } else if (curve.p0.y < center.y + 0.5) {
            let t = monotone_quadratic_root(curve.a.x, curve.b.x, curve.p0.x - center.x, direction);
            let y = (curve.a.y * t + curve.b.y) * t + curve.p0.y;
            let distance = center.y - y;
            (*winding).y -= curve.sign * direction * saturate(distance + 0.5);
            (*weight).y = max((*weight).y, saturate(1.0 - 2.0 * abs(distance)));
        }
    }
}

// The backdrop gives the unlisted curves' winding along a leftward ray from the tile's reference
// corner. We need it at the current pixel, along both leftward and upward rays. Because paths are
// closed, chains of unlisted curves can only begin or end where a listed curve does. So, we can
// compute the winding at the current pixel by correcting the backdrop for each listed endpoint
// between its ray and the pixel.
fn correct_for_endpoint(endpoint: vec2<f32>, step: f32, reference: vec2<f32>, center: vec2<f32>,
    winding: ptr<function, vec2<f32>>) {
    if (endpoint.x <= reference.x) {
        (*winding).x -= step * (f32(endpoint.y <= center.y) - f32(endpoint.y <= reference.y));
    }
    if (endpoint.y <= reference.y && endpoint.x <= center.x) {
        (*winding).y += step;
    }
}

@fragment
fn fs_path_tile(input: PathTileVarying) -> @location(0) vec4<f32> {
    let center = floor(input.position.xy) + 0.5;
    let tile = load_path_tile(input.tile_id);
    let paint = load_path_paint(tile.paint);

    var winding = vec2<f32>(f32(tile.backdrop));
    var weight = vec2<f32>(0.0);
    for (var i = 0u; i < tile.curve_count; i++) {
        let curve = load_path_curve(tile.curve_start + i);
        if (min(curve.p0.x, curve.p1.x) >= center.x + 0.5) {
            break;
        }
        if (curve.p0.y < center.y + 0.5) {
            add_curve_winding(curve, center, &winding, &weight);
            correct_for_endpoint(curve.p0, curve.sign, tile.reference, center, &winding);
            correct_for_endpoint(curve.p1, -curve.sign, tile.reference, center, &winding);
        }
    }

    // Weight the winding computed by each ray by its nearest crossing's distance from the pixel center,
    // using Slug's technique.
    var coverage = max(
        abs(dot(winding, weight)) / max(weight.x + weight.y, 1.0 / 65536.0),
        min(abs(winding.x), abs(winding.y)));
    if (paint.even_odd != 0u) {
        coverage = 1.0 - abs(1.0 - fract(coverage * 0.5) * 2.0);
    } else {
        coverage = saturate(coverage);
    }

    if (coverage == 0.0) {
        return vec4<f32>(0.0);
    }

    let color = gradient_color(paint.color, input.position.xy, paint.bounds,
        input.background_solid, input.background_color0, input.background_color1);
    return blend_color(color, coverage);
}

@group(1) @binding(0) var<storage, read> b_path_tiles: array<PathTile>;
@group(1) @binding(1) var<storage, read> b_path_curves: array<PathCurve>;
@group(1) @binding(2) var<storage, read> b_path_paints: array<PathPaint>;

fn load_path_tile(instance_id: u32) -> PathTile {
    return b_path_tiles[instance_id];
}

fn load_path_curve(index: u32) -> PathCurve {
    return b_path_curves[index];
}

fn load_path_paint(index: u32) -> PathPaint {
    return b_path_paints[index];
}
