// What every shader of the driver shares (prepended to each module): the globals — the
// viewport and the emphasis the batch is drawn with — and the helpers using them.
// Colors are sRGB (the surface is configured without sRGB conversion), and the fragment
// shaders output premultiplied alpha.

// At most `render_api::MAX_KEEPS` kept areas.
const MAX_KEEPS: u32 = 8u;

struct Globals {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
    // The emphasis (`render_api::Emphasis`): gray, dim, feather (pixels), kept areas.
    // A batch it doesn't mute is drawn with all of it zero.
    emphasis: vec4<f32>,
    // Each kept area: its rect (x, y, w, h) and (corner radius, weight, -, -).
    keep_rects: array<vec4<f32>, MAX_KEEPS>,
    keep_params: array<vec4<f32>, MAX_KEEPS>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x / globals.viewport.x * 2.0 - 1.0, 1.0 - p.y / globals.viewport.y * 2.0, 0.0, 1.0);
}

// The signed distance to a rounded rect (negative inside): `rounded_rect_distance`.
fn rounded_rect_distance(rect: vec4<f32>, radius: f32, p: vec2<f32>) -> f32 {
    let half = rect.zw * 0.5;
    let r = max(min(radius, min(half.x, half.y)), 0.0);
    let d = abs(p - (rect.xy + half)) - (half - vec2<f32>(r));
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - r;
}

// `color` (premultiplied) at `pixel` as the emphasis leaves it: `Emphasis::apply`. The
// operator is linear, so muting each primitive before blending is muting the frame.
fn emphasize(color: vec4<f32>, pixel: vec2<f32>) -> vec4<f32> {
    let count = u32(globals.emphasis.w);
    if (count == 0u) {
        return color;
    }
    let feather = globals.emphasis.z;
    var kept = 0.0;
    for (var i = 0u; i < min(count, MAX_KEEPS); i = i + 1u) {
        let d = rounded_rect_distance(globals.keep_rects[i], globals.keep_params[i].x, pixel);
        var inside: f32;
        if (feather > 0.0) {
            let t = clamp(d / feather, 0.0, 1.0);
            inside = 1.0 - t * t * (3.0 - 2.0 * t);
        } else {
            inside = clamp(0.5 - d, 0.0, 1.0);
        }
        kept = kept + globals.keep_params[i].y * inside;
    }
    let muted = 1.0 - min(kept, 1.0);
    if (muted <= 0.0) {
        return color;
    }
    let luma = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let gray = globals.emphasis.x * muted;
    let scale = 1.0 - globals.emphasis.y * muted;
    return vec4<f32>((color.rgb + gray * (vec3<f32>(luma) - color.rgb)) * scale, color.a);
}
