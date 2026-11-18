// A backdrop blur drawn back (scaled up, bilinear) in a layer's rectangles, at its opacity
// (after `common.wgsl`); the blurred image is premultiplied. Never muted again: it's made
// from the frame drawn, already muted where the emphasis is.

@group(1) @binding(0) var blurred: texture_2d<f32>;
@group(1) @binding(1) var blurred_sampler: sampler;

struct BlurInstance {
    @location(0) rect: vec4<f32>,    // x, y, w, h (pixels)
    // The blurred image's place: its corner and the size it covers (pixels).
    @location(1) image: vec4<f32>,
    @location(2) opacity: f32,
    // Its corners' radius (pixels; 0: square).
    @location(3) corner: f32,
};

struct BlurVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) opacity: f32,
    // Where the pixel is from the rectangle's center, its half size, its corners' radius.
    @location(2) local: vec2<f32>,
    @location(3) @interpolate(flat) half_size: vec2<f32>,
    @location(4) @interpolate(flat) corner: f32,
};

@vertex
fn vs_composite(@builtin(vertex_index) vertex_index: u32, blur: BlurInstance) -> BlurVarying {
    let corner = vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
    let p = blur.rect.xy + corner * blur.rect.zw;
    var out: BlurVarying;
    out.position = to_clip(p);
    out.uv = (p - blur.image.xy) / blur.image.zw;
    out.opacity = blur.opacity;
    out.half_size = blur.rect.zw * 0.5;
    out.local = p - (blur.rect.xy + out.half_size);
    out.corner = blur.corner;
    return out;
}

@fragment
fn fs_composite(in: BlurVarying) -> @location(0) vec4<f32> {
    // Rounded corners: the pixel's share inside them (as `rounded_coverage` on the CPU).
    let r = clamp(in.corner, 0.0, min(in.half_size.x, in.half_size.y));
    let q = abs(in.local) - in.half_size + vec2<f32>(r, r);
    let d = length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
    let inside = clamp(0.5 - d, 0.0, 1.0);
    return textureSampleLevel(blurred, blurred_sampler, in.uv, 0.0) * in.opacity * inside;
}
