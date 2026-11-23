//! A snapshot's screen drawn on the GPU (without a window) and measured, for the renderer's
//! before / after comparisons (plan 93):
//!
//! - `EDITOR_SNAPSHOT_GPU=<frames>`: the screen painted and drawn whole that many times;
//!   printed: what the scene holds (layers, quads, glyphs), the draw calls, the CPU's time
//!   to paint and to encode a frame, and the text's work (lines shaped, from the cache);
//! - `EDITOR_SNAPSHOT_GPU_SAVE=<file>`: the GPU's picture kept as raw RGBA, to compare with
//!   later;
//! - `EDITOR_SNAPSHOT_GPU_REFERENCE=<file>`: the picture compared with one saved before:
//!   the pixels that differ and by how much (it fails past `EDITOR_SNAPSHOT_GPU_TOLERANCE`
//!   pixels, 0 by default).

use std::path::Path;
use std::time::Duration;

use crate::workbench::Workbench;
use anyhow::Context as _;
use render_api::Scene;
use render_wgpu::Offscreen;
use text_layout::TextSystem;

/// What a saved picture starts with: its width and height (little-endian `u32`s).
const HEADER: usize = 8;

/// Measures `workbench`'s screen on the GPU as the variables ask (nothing without them):
/// painted again for the times, while the picture kept or compared is `shown`'s, the
/// snapshot's own scene (the same whatever the clock: a spinner caught where it was).
pub fn measure(
    workbench: &mut Workbench,
    text: &mut TextSystem,
    size: kernel::PhysicalSize,
    shown: &Scene,
) -> anyhow::Result<()> {
    let frames: u32 =
        std::env::var("EDITOR_SNAPSHOT_GPU").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let save = std::env::var_os("EDITOR_SNAPSHOT_GPU_SAVE");
    let reference = std::env::var_os("EDITOR_SNAPSHOT_GPU_REFERENCE");
    if frames == 0 && save.is_none() && reference.is_none() {
        return Ok(());
    }
    let mut gpu = Offscreen::new(size).context("no GPU to draw on")?;
    let icons = std::sync::Arc::clone(workbench.icons());
    let mut paint = Duration::ZERO;
    let mut encode = Vec::with_capacity(frames as usize);
    let mut scene = Scene::default();
    let mut last = render_api::FrameStats::default();
    kernel::probe::start();
    for _ in 0..frames {
        scene.reset(workbench.background());
        let started = std::time::Instant::now();
        workbench.paint(text, &mut scene);
        paint += started.elapsed();
        let mut glyphs = icons::WithIcons { text: &mut *text, icons: &icons };
        let measured = gpu.draw(&scene, &mut glyphs, true);
        encode.push(measured.encode);
        last = measured.stats;
    }
    let gathered = kernel::probe::take();
    kernel::probe::stop();
    if frames > 0 {
        let counted = |what: &str| {
            gathered.counts.iter().find(|c| c.0 == "text" && c.1 == what).map_or(0, |c| c.2)
        };
        encode.sort();
        let n = encode.len().max(1) as u32;
        let average = encode.iter().sum::<Duration>() / n;
        eprintln!(
            "gpu: layers {} · quads {} · glyphs {} · draw calls {} · paint {:.2} ms · encode {:.2} ms (median {:.2}, max {:.2}) · lines shaped {} · from the cache {}",
            scene.layers().len(),
            last.quads,
            last.glyphs,
            last.draw_calls,
            ms(paint / n),
            ms(average),
            ms(encode[encode.len() / 2]),
            ms(*encode.last().unwrap_or(&Duration::ZERO)),
            counted("lines shaped"),
            counted("lines cached"),
        );
    }
    let mut glyphs = icons::WithIcons { text: &mut *text, icons: &icons };
    gpu.draw(shown, &mut glyphs, true);
    let (w, h, pixels) = gpu.pixels().context("nothing drawn on the GPU")?;
    if let Some(path) = save {
        write(Path::new(&path), w, h, &pixels)?;
    }
    if let Some(path) = reference {
        compare(Path::new(&path), w, h, &pixels)?;
    }
    Ok(())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn write(path: &Path, w: u32, h: u32, pixels: &[u8]) -> anyhow::Result<()> {
    let mut out = Vec::with_capacity(HEADER + pixels.len());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(pixels);
    std::fs::write(path, out).with_context(|| format!("cannot write {}", path.display()))
}

/// The picture compared with the one saved at `path`: printed, failing past the tolerance.
fn compare(path: &Path, w: u32, h: u32, pixels: &[u8]) -> anyhow::Result<()> {
    let saved = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let (Some(sw), Some(sh)) = (saved.get(0..4), saved.get(4..8)) else {
        anyhow::bail!("{} isn't a saved picture", path.display());
    };
    let size = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    anyhow::ensure!((size(sw), size(sh)) == (w, h), "the pictures' sizes differ");
    anyhow::ensure!(saved.len() == HEADER + pixels.len(), "{} is cut short", path.display());
    let difference = difference(&saved[HEADER..], pixels);
    let tolerance: usize = std::env::var("EDITOR_SNAPSHOT_GPU_TOLERANCE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    eprintln!(
        "gpu compare: {} of {} pixels differ, by {} at most",
        difference.pixels,
        w as usize * h as usize,
        difference.most
    );
    anyhow::ensure!(difference.pixels <= tolerance, "the picture differs from the reference");
    Ok(())
}

/// How two RGBA pictures of one size differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Difference {
    /// The pixels where any channel differs.
    pub pixels: usize,
    /// The largest difference of a channel.
    pub most: u8,
}

pub fn difference(a: &[u8], b: &[u8]) -> Difference {
    let mut out = Difference::default();
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let most = pa.iter().zip(pb).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        if most > 0 {
            out.pixels += 1;
            out.most = out.most.max(most);
        }
    }
    out
}

/// A screen's accessibility tree as text: one node a line, indented under its parent — its
/// role, name, value and state (what a screen reader reads), its place left out (it moves
/// with the layout; the picture shows it).
pub fn access_text(frame: &platform_api::access::Frame) -> String {
    use platform_api::access::ROOT;
    let nodes = frame.nodes();
    let children = frame.children();
    let mut out = String::new();
    let mut stack: Vec<(usize, usize)> =
        children.get(&ROOT).map_or_else(Vec::new, |c| c.iter().rev().map(|&i| (i, 0)).collect());
    while let Some((i, depth)) = stack.pop() {
        let n = &nodes[i];
        let mut line = format!("{}{:?} {:?}", "  ".repeat(depth), n.role, n.name);
        if let Some(value) = &n.value {
            line.push_str(&format!(" = {value:?}"));
        }
        let s = n.state;
        for (on, word) in [
            (s.disabled, "disabled"),
            (s.selected, "selected"),
            (s.checked == Some(true), "checked"),
            (s.mixed, "mixed"),
            (s.expanded == Some(true), "expanded"),
            (s.focused, "focused"),
        ] {
            if on {
                line.push(' ');
                line.push_str(word);
            }
        }
        out.push_str(&line);
        out.push('\n');
        if let Some(kids) = children.get(&n.id) {
            stack.extend(kids.iter().rev().map(|&k| (k, depth + 1)));
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/snapshot_gpu.rs"]
mod tests;
