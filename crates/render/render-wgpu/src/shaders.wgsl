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

// A rounded, bordered rectangle's color at `pixel`, premultiplied: the same formula as
// `quad_coverage` in render-software. `params`: corner radius, border width.
fn quad_coverage(
    rect: vec4<f32>,
    color: vec4<f32>,
    border_color: vec4<f32>,
    params: vec2<f32>,
    pixel: vec2<f32>,
) -> vec4<f32> {
    let half = rect.zw * 0.5;
    let radius = max(min(params.x, min(half.x, half.y)), 0.0);
    let d = abs(pixel - (rect.xy + half)) - (half - vec2<f32>(radius));
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;

    let shape = clamp(0.5 - dist, 0.0, 1.0);
    var fill = shape;
    var border = 0.0;
    if (params.y > 0.0) {
        fill = clamp(0.5 - (dist + params.y), 0.0, 1.0);
        border = shape - fill;
    }
    let fill_a = color.a * fill;
    let border_a = border_color.a * border;
    let rgb = border_color.rgb * border_a + color.rgb * fill_a * (1.0 - border_a);
    return vec4<f32>(rgb, border_a + fill_a * (1.0 - border_a));
}

// The background over a partial frame's areas.
@fragment
fn fs_quad(in: QuadVarying) -> @location(0) vec4<f32> {
    let color = quad_coverage(in.rect, in.color, in.border_color, in.params, in.pixel);
    return emphasize(color, in.pixel);
}

// ─── Primitives: every layer's rectangles and glyphs in one draw ───
//
// A primitive is a rectangle (rounded, bordered: the quads' SDF) or a glyph (copied pixel
// by pixel from the atlas), in the scene's order. Each carries its layer's clip (whole
// pixels, as a scissor would cut) and whether its layer is muted by the emphasis, so the
// layers need no draw of their own.

@group(1) @binding(0) var mask_atlas: texture_2d<f32>;
@group(1) @binding(1) var color_atlas: texture_2d<f32>;

// `flags`: the kind in bits 0–1, muted by the emphasis in bit 2 (`PrimGpu` in Rust).
const PRIM_QUAD: u32 = 0u;
const PRIM_MASK: u32 = 1u;
const PRIM_COLOR: u32 = 2u;
const PRIM_MUTED: u32 = 4u;

struct PrimInstance {
    @location(0) rect: vec4<f32>,          // x, y, w, h (a glyph's: whole pixels)
    @location(1) color: vec4<f32>,
    @location(2) border_color: vec4<f32>,
    @location(3) clip: vec4<u32>,          // x0, y0, x1, y1 (whole pixels)
    @location(4) params: vec4<f32>,        // corner radius, border width, a glyph's atlas x, y
    @location(5) flags: u32,
};

struct PrimVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) border_color: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) flags: u32,
};

@vertex
fn vs_prim(@builtin(vertex_index) vertex_index: u32, prim: PrimInstance) -> PrimVarying {
    // Its rectangle cut by its clip: the pixels whose centers are in both (as a scissor).
    let clip = vec4<f32>(prim.clip);
    let lo = max(prim.rect.xy, clip.xy);
    let hi = max(min(prim.rect.xy + prim.rect.zw, clip.zw), lo);
    var out: PrimVarying;
    out.position = to_clip(lo + strip_corner(vertex_index) * (hi - lo));
    out.rect = prim.rect;
    out.color = prim.color;
    out.border_color = prim.border_color;
    out.params = prim.params;
    out.flags = prim.flags;
    return out;
}

// A glyph's pixel at `pixel`, from the atlas: a mask in the instance's color, or its own
// colors faded by the instance's alpha (a dimmed row, a modal appearing).
fn glyph_color(in: PrimVarying, pixel: vec2<f32>, kind: u32) -> vec4<f32> {
    let texel = vec2<i32>(floor(in.params.zw + pixel - in.rect.xy));
    if (kind == PRIM_MASK) {
        let a = textureLoad(mask_atlas, texel, 0).r * in.color.a;
        return vec4<f32>(in.color.rgb * a, a);
    }
    // PRIM_COLOR
    let c = textureLoad(color_atlas, texel, 0);
    let a = c.a * in.color.a;
    return vec4<f32>(c.rgb * a, a);
}

@fragment
fn fs_prim(in: PrimVarying) -> @location(0) vec4<f32> {
    let pixel = in.position.xy;
    let kind = in.flags & 3u;
    var color: vec4<f32>;
    if (kind == PRIM_QUAD) {
        color = quad_coverage(in.rect, in.color, in.border_color, in.params.xy, pixel);
    } else {
        color = glyph_color(in, pixel, kind);
    }
    if ((in.flags & PRIM_MUTED) != 0u) {
        return emphasize(color, pixel);
    }
    return color;
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
