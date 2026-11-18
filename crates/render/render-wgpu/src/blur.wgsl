// The backdrop blur (render-api's `BlurPlan`): the frame drawn so far reduced by a factor
// (each texel the average of factor × factor pixels, from bilinear taps between four of
// them), blurred by a separable gaussian at that size, then drawn back scaled up.
// Colors stay premultiplied throughout.

struct BlurParams {
    // The source area's corner (pixels of the source), 1 / the source texture's size.
    p0: vec4<f32>,
    // The source area: min, max (pixels), never read outside it.
    p1: vec4<f32>,
    // Direction (texels), the factor, the gaussian's sigma (texels).
    p2: vec4<f32>,
    // Texels taken on each side.
    p3: vec4<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;

// A triangle covering the whole target.
@vertex
fn vs_full(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// One reduced texel: the average of its factor × factor source pixels.
@fragment
fn fs_reduce(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let factor = params.p2.z;
    let base = params.p0.xy + floor(position.xy) * factor;
    let lo = params.p1.xy + vec2<f32>(1.0);
    let hi = params.p1.zw - vec2<f32>(1.0);
    let half = i32(factor) / 2;
    var sum = vec4<f32>(0.0);
    for (var j = 0; j < half; j++) {
        for (var i = 0; i < half; i++) {
            // Between four pixels: bilinear filtering averages them.
            let p = clamp(base + vec2<f32>(f32(2 * i + 1), f32(2 * j + 1)), lo, hi);
            sum += textureSampleLevel(source, source_sampler, p * params.p0.zw, 0.0);
        }
    }
    return sum / f32(half * half);
}

// The gaussian along one direction (texel centers: no filtering in between; the edges
// clamped).
@fragment
fn fs_blur(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = params.p0.zw;
    let center = position.xy * texel;
    let step = params.p2.xy * texel;
    let sigma = params.p2.w;
    let taps = i32(params.p3.x);
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var k = -taps; k <= taps; k++) {
        let w = exp(-f32(k * k) / (2.0 * sigma * sigma));
        sum += textureSampleLevel(source, source_sampler, center + step * f32(k), 0.0) * w;
        total += w;
    }
    return sum / total;
}
