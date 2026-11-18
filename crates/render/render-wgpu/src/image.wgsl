// Images (after `common.wgsl`): RGBA8 textures (not premultiplied), or masks (R8 coverage
// drawn in the instance's color), scaled with bilinear filtering.

@group(1) @binding(0) var image_texture: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct ImageInstance {
    @location(0) rect: vec4<f32>,   // x, y, w, h
    @location(1) tint: vec4<f32>,   // a mask's color; alpha < 0: an RGBA image
    @location(2) opacity: f32,
};

struct ImageVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) opacity: f32,
    @location(2) @interpolate(flat) tint: vec4<f32>,
};

@vertex
fn vs_image(@builtin(vertex_index) vertex_index: u32, image: ImageInstance) -> ImageVarying {
    let corner = vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
    let p = image.rect.xy + corner * image.rect.zw;
    var out: ImageVarying;
    out.position = to_clip(p);
    out.uv = corner;
    out.opacity = image.opacity;
    out.tint = image.tint;
    return out;
}

@fragment
fn fs_image(in: ImageVarying) -> @location(0) vec4<f32> {
    let c = textureSample(image_texture, image_sampler, in.uv);
    if (in.tint.a >= 0.0) {
        // A mask: its coverage in the tint.
        let m = c.r * in.tint.a * in.opacity;
        return emphasize(vec4<f32>(in.tint.rgb * m, m), in.position.xy);
    }
    let a = c.a * in.opacity;
    return emphasize(vec4<f32>(c.rgb * a, a), in.position.xy);
}
