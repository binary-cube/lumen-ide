//! A small picture of a window's last frame (what the open projects' list shows of each):
//! read from the GPU by the renderer, made small here — off the UI thread — once.

/// A picture: straight RGBA8, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Frame `data` (`width` × `height` pixels, `stride` bytes a row; BGRA when `bgra`, else
/// RGBA) made at most `max_width` wide: each pixel the average of the block it stands for
/// (a box filter: no shimmering text), opaque, in RGBA.
pub fn shrink(
    data: &[u8],
    (width, height, stride): (u32, u32, usize),
    bgra: bool,
    max_width: u32,
) -> Captured {
    let step = width.div_ceil(max_width.max(1)).max(1);
    let (w, h) = ((width / step).max(1), (height / step).max(1));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    let count = step * step;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for dy in 0..step {
                let row = ((y * step + dy).min(height - 1)) as usize * stride;
                for dx in 0..step {
                    let at = row + ((x * step + dx).min(width - 1)) as usize * 4;
                    let Some(px) = data.get(at..at + 3) else { continue };
                    for c in 0..3 {
                        sum[c] += u32::from(px[c]);
                    }
                }
            }
            let [a, b, c] = sum.map(|s| (s / count) as u8);
            if bgra {
                rgba.extend_from_slice(&[c, b, a, 255]);
            } else {
                rgba.extend_from_slice(&[a, b, c, 255]);
            }
        }
    }
    Captured { width: w, height: h, rgba }
}

#[cfg(test)]
#[path = "../tests/unit/capture.rs"]
mod tests;
