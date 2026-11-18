// Shaders for the wgpu driver (after `common.wgsl`). Input coordinates are in physical
// pixels.

// The corner of a quad drawn as a 4-vertex triangle strip: (0,0) (1,0) (0,1) (1,1).
fn strip_corner(vertex_index: u32) -> vec2<f32> {
    return vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
}

// ─── Quads: rectangles with rounded corners and a border (SDF) ───

struct QuadInstance {
    @location(0) rect: vec4<f32>,          // x, y, w, h
    @location(1) color: vec4<f32>,
    @location(2) border_color: vec4<f32>,
    @location(3) params: vec2<f32>,        // corner_radius, border_width
};

struct QuadVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) pixel: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) border_color: vec4<f32>,
    @location(4) @interpolate(flat) params: vec2<f32>,
};

@vertex
fn vs_quad(@builtin(vertex_index) vertex_index: u32, quad: QuadInstance) -> QuadVarying {
    let pixel = quad.rect.xy + strip_corner(vertex_index) * quad.rect.zw;
    var out: QuadVarying;
    out.position = to_clip(pixel);
    out.pixel = pixel;
    out.rect = quad.rect;
    out.color = quad.color;
    out.border_color = quad.border_color;
    out.params = quad.params;
    return out;
}

// Same formula as `quad_coverage` in render-software.
@fragment
fn fs_quad(in: QuadVarying) -> @location(0) vec4<f32> {
    let half = in.rect.zw * 0.5;
    let radius = max(min(in.params.x, min(half.x, half.y)), 0.0);
    let d = abs(in.pixel - (in.rect.xy + half)) - (half - vec2<f32>(radius));
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;

    let shape = clamp(0.5 - dist, 0.0, 1.0);
    var fill = shape;
    var border = 0.0;
    if (in.params.y > 0.0) {
        fill = clamp(0.5 - (dist + in.params.y), 0.0, 1.0);
        border = shape - fill;
    }

    let fill_a = in.color.a * fill;
    let border_a = in.border_color.a * border;
    let rgb = in.border_color.rgb * border_a + in.color.rgb * fill_a * (1.0 - border_a);
    return emphasize(vec4<f32>(rgb, border_a + fill_a * (1.0 - border_a)), in.pixel);
}

// ─── Glyphs: copied pixel by pixel from the atlas ───

@group(1) @binding(0) var mask_atlas: texture_2d<f32>;
@group(1) @binding(1) var color_atlas: texture_2d<f32>;

struct GlyphInstance {
    @location(0) rect: vec4<f32>,          // x, y, w, h (whole pixels)
    @location(1) atlas_origin: vec2<f32>,  // the glyph's corner in the atlas, in texels
    @location(2) color: vec4<f32>,
    @location(3) kind: u32,                // 0 = mask, 1 = color
};

struct GlyphVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
};

@vertex
fn vs_glyph(@builtin(vertex_index) vertex_index: u32, glyph: GlyphInstance) -> GlyphVarying {
    let corner = strip_corner(vertex_index);
    var out: GlyphVarying;
    out.position = to_clip(glyph.rect.xy + corner * glyph.rect.zw);
    out.texel = glyph.atlas_origin + corner * glyph.rect.zw;
    out.color = glyph.color;
    out.kind = glyph.kind;
    return out;
}

@fragment
fn fs_glyph(in: GlyphVarying) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(floor(in.texel));
    if (in.kind == 0u) {
        let a = textureLoad(mask_atlas, texel, 0).r * in.color.a;
        return emphasize(vec4<f32>(in.color.rgb * a, a), in.position.xy);
    }
    // Its own colors; the instance color's alpha still fades it (a dimmed row, a modal
    // appearing).
    let c = textureLoad(color_atlas, texel, 0);
    let a = c.a * in.color.a;
    return emphasize(vec4<f32>(c.rgb * a, a), in.position.xy);
}

// ─── The window's corners: what's outside them is erased (left transparent) ───

// A quad over one corner of the window; `border_color` carries the window's rect and
// `params.x` its corner radius. Blended as `dst * (1 - alpha)`: alpha is how much of the
// pixel lies outside the rounded window (antialiased like the quads).
@fragment
fn fs_window_corner(in: QuadVarying) -> @location(0) vec4<f32> {
    let window = in.border_color;
    let half = window.zw * 0.5;
    let radius = max(min(in.params.x, min(half.x, half.y)), 0.0);
    let d = abs(in.pixel - (window.xy + half)) - (half - vec2<f32>(radius));
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
    return vec4<f32>(0.0, 0.0, 0.0, clamp(0.5 + dist, 0.0, 1.0));
}
