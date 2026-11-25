// Images (after `common.wgsl`): RGBA8 textures (straight alpha, or premultiplied: a live
// image's, a web engine's frames), or masks (R8 coverage drawn in the instance's color),
// scaled with bilinear filtering.

@group(1) @binding(0) var image_texture: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

// `ImageGpu::flags`: its colors are premultiplied by their alpha.
const IMAGE_PREMULTIPLIED: u32 = 1u;

struct ImageInstance {
    @location(0) rect: vec4<f32>,   // x, y, w, h
    @location(1) tint: vec4<f32>,   // a mask's color; alpha < 0: an RGBA image
    @location(2) shape: vec2<f32>,  // opacity, corners' radius
    @location(3) bits: vec2<u32>,   // which corners are rounded, flags
};

struct ImageVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) opacity: f32,
    @location(2) @interpolate(flat) tint: vec4<f32>,
    @location(3) @interpolate(flat) rect: vec4<f32>,
    @location(4) @interpolate(flat) radius: f32,
    @location(5) @interpolate(flat) bits: vec2<u32>,
};

// How much of the pixel at `p` is inside the image's rounded corners (1 inside, 0 past a
// rounded one; antialiased on the edge). Bits: 1 top left, 2 top right, 4 bottom left,
// 8 bottom right. Only a rounded corner's square is cut: the straight edges are drawn as
// a square image's, never softened (a fractional edge would otherwise show a seam where
// a rounded quarter meets a square one).
fn corner_coverage(rect: vec4<f32>, radius: f32, which: u32, p: vec2<f32>) -> f32 {
    if (radius <= 0.0 || which == 0u) {
        return 1.0;
    }
    let half = rect.zw * 0.5;
    let r = min(radius, min(half.x, half.y));
    let center = rect.xy + half;
    let right = p.x >= center.x;
    let bottom = p.y >= center.y;
    var bit = 1u;
    if (right) {
        bit = 2u;
    }
    if (bottom) {
        bit = bit * 4u;
    }
    if ((which & bit) == 0u) {
        return 1.0;
    }
    // From the corner's circle's center, outward (toward the corner).
    let circle = vec2<f32>(
        select(rect.x + r, rect.x + rect.z - r, right),
        select(rect.y + r, rect.y + rect.w - r, bottom),
    );
    let d = vec2<f32>(
        select(circle.x - p.x, p.x - circle.x, right),
        select(circle.y - p.y, p.y - circle.y, bottom),
    );
    if (d.x <= 0.0 || d.y <= 0.0) {
        return 1.0;
    }
    return clamp(0.5 - (length(d) - r), 0.0, 1.0);
}

@vertex
fn vs_image(@builtin(vertex_index) vertex_index: u32, image: ImageInstance) -> ImageVarying {
    let corner = vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
    let p = image.rect.xy + corner * image.rect.zw;
    var out: ImageVarying;
    out.position = to_clip(p);
    out.uv = corner;
    out.opacity = image.shape.x;
    out.tint = image.tint;
    out.rect = image.rect;
    out.radius = image.shape.y;
    out.bits = image.bits;
    return out;
}

@fragment
fn fs_image(in: ImageVarying) -> @location(0) vec4<f32> {
    let c = textureSample(image_texture, image_sampler, in.uv);
    let opacity = in.opacity * corner_coverage(in.rect, in.radius, in.bits.x, in.position.xy);
    if (in.tint.a >= 0.0) {
        // A mask: its coverage in the tint.
        let m = c.r * in.tint.a * opacity;
        return emphasize(vec4<f32>(in.tint.rgb * m, m), in.position.xy);
    }
    if ((in.bits.y & IMAGE_PREMULTIPLIED) != 0u) {
        // Already premultiplied: scaled as a whole.
        return emphasize(c * opacity, in.position.xy);
    }
    let a = c.a * opacity;
    return emphasize(vec4<f32>(c.rgb * a, a), in.position.xy);
}
