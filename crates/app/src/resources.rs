//! The window's resource usage (= the process's: each window is a process).
//!
//! A thread measures CPU and RAM every 2 seconds and wakes the UI only on real changes
//! (hysteresis: ≥ 1 percentage point CPU or ≥ 2 MB RAM). Without hysteresis, small
//! fluctuations would trigger redraws which, in turn, would consume CPU. While every window
//! is hidden (minimized, another desktop, the screen locked) nothing shows the figures: the
//! thread sleeps (a condition variable) and measures again as soon as one is shown.
//!
//! The programs the app started (the terminals' shells and what runs in them, language
//! servers, agents — everything under the process) are counted too, every
//! [`PROGRAMS_EVERY`]-th look (a look at every process costs a few ms): what the system's
//! own monitor adds to the app's figure (it counts them as the app's).

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const INTERVAL: Duration = Duration::from_secs(2);
const CPU_STEP: f32 = 1.0;
const RAM_STEP: u64 = 2 * 1024 * 1024;
/// The programs started are counted every this many looks (10 s).
const PROGRAMS_EVERY: u32 = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// Percent of one core (can exceed 100% on multiple cores).
    pub cpu: f32,
    pub ram_bytes: u64,
    /// The programs running under the app (its descendants) and the memory they hold
    /// (resident), as last counted.
    pub programs: u32,
    pub programs_bytes: u64,
}

/// What the windows' handles and the measuring thread share.
#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct State {
    latest: Option<Sample>,
    /// The windows holding the monitor (none: the thread ends).
    handles: usize,
    /// Those of them shown (none: the thread sleeps).
    visible: usize,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A window's handle on the process's monitor.
#[derive(Debug)]
pub struct ResourceMonitor {
    shared: Arc<Shared>,
    /// This window counts among the shown ones.
    visible: bool,
}

impl ResourceMonitor {
    /// The process's monitor, shared by every window (they all measure the same process;
    /// one thread, stopped when the last window drops it).
    pub fn shared(waker: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        static SHARED: Mutex<std::sync::Weak<Shared>> = Mutex::new(std::sync::Weak::new());
        let mut slot = SHARED.lock().map_err(|_| std::io::Error::other("poisoned"))?;
        if let Some(shared) = slot.upgrade() {
            {
                let mut state = shared.lock();
                state.handles += 1;
                state.visible += 1;
            }
            shared.changed.notify_all();
            return Ok(Self { shared, visible: true });
        }
        let monitor = Self::spawn(waker, INTERVAL, measure_self())?;
        *slot = Arc::downgrade(&monitor.shared);
        Ok(monitor)
    }

    /// The thread, measuring with `measure` every `every` while a window is shown; the
    /// handle returned is its first window's (shown).
    fn spawn(
        waker: Arc<dyn Fn() + Send + Sync>,
        every: Duration,
        mut measure: impl FnMut() -> Option<Sample> + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        {
            let mut state = shared.lock();
            state.handles = 1;
            state.visible = 1;
        }
        let thread = Arc::clone(&shared);
        std::thread::Builder::new().name("resources".into()).spawn(move || {
            let mut shown: Option<Sample> = None;
            loop {
                {
                    let state = thread.lock();
                    let state = thread
                        .changed
                        .wait_while(state, |s| s.handles > 0 && s.visible == 0)
                        .unwrap_or_else(PoisonError::into_inner);
                    if state.handles == 0 {
                        return;
                    }
                }
                if let Some(sample) = measure()
                    && shown.is_none_or(|s| significant(&s, &sample))
                {
                    thread.lock().latest = Some(sample);
                    shown = Some(sample);
                    waker();
                }
                // Until the next look (sooner only when the last window goes).
                let state = thread.lock();
                let (state, _) = thread
                    .changed
                    .wait_timeout_while(state, every, |s| s.handles > 0)
                    .unwrap_or_else(PoisonError::into_inner);
                if state.handles == 0 {
                    return;
                }
            }
        })?;
        Ok(Self { shared, visible: true })
    }

    pub fn latest(&self) -> Option<Sample> {
        self.shared.lock().latest
    }

    /// This window shown or hidden: with every window hidden the thread sleeps.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        {
            let mut state = self.shared.lock();
            if visible {
                state.visible += 1;
            } else {
                state.visible -= 1;
            }
        }
        self.shared.changed.notify_all();
    }
}

impl Drop for ResourceMonitor {
    fn drop(&mut self) {
        {
            let mut state = self.shared.lock();
            state.handles -= 1;
            if self.visible {
                state.visible -= 1;
            }
        }
        self.shared.changed.notify_all();
    }
}

/// This process measured (its CPU since the last look), its programs every
/// [`PROGRAMS_EVERY`]-th look.
fn measure_self() -> impl FnMut() -> Option<Sample> + Send + 'static {
    let pid = sysinfo::get_current_pid().ok();
    let mut system = System::new();
    let (mut looks, mut programs) = (0u32, (0u32, 0u64));
    move || {
        let pid = pid?;
        if looks % PROGRAMS_EVERY == 0 {
            programs = programs_under(&mut system, pid);
        }
        looks = looks.wrapping_add(1);
        let sample = measure(&mut system, pid)?;
        Some(Sample { programs: programs.0, programs_bytes: programs.1, ..sample })
    }
}

/// How many processes run under `pid` (its descendants, at any depth) and the memory they
/// hold (resident).
fn programs_under(system: &mut System, pid: Pid) -> (u32, u64) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    let mut children: std::collections::HashMap<Pid, Vec<Pid>> = std::collections::HashMap::new();
    for (child, process) in system.processes() {
        if let Some(parent) = process.parent() {
            children.entry(parent).or_default().push(*child);
        }
    }
    // Each as the system's monitor counts it (its own pages), not RSS: that counts the
    // system's libraries every program shares (measured: 1298 MB by RSS, 571 MB as Activity
    // Monitor says, for a coding agent and its helpers).
    descendants(&children, pid, |p| {
        process_stats::footprint_of(p.as_u32())
            .unwrap_or_else(|| system.process(p).map_or(0, sysinfo::Process::memory))
    })
}

/// The descendants of `root` in `children` (each counted once): how many, and the sum of
/// `memory` over them.
fn descendants(
    children: &std::collections::HashMap<Pid, Vec<Pid>>,
    root: Pid,
    memory: impl Fn(Pid) -> u64,
) -> (u32, u64) {
    let (mut count, mut bytes) = (0u32, 0u64);
    let mut seen = std::collections::HashSet::from([root]);
    let mut stack: Vec<Pid> = children.get(&root).cloned().unwrap_or_default();
    while let Some(p) = stack.pop() {
        if !seen.insert(p) {
            continue;
        }
        count = count.saturating_add(1);
        bytes = bytes.saturating_add(memory(p));
        stack.extend(children.get(&p).into_iter().flatten().copied());
    }
    (count, bytes)
}

fn significant(shown: &Sample, now: &Sample) -> bool {
    (shown.cpu - now.cpu).abs() >= CPU_STEP
        || shown.ram_bytes.abs_diff(now.ram_bytes) >= RAM_STEP
        || shown.programs != now.programs
        || shown.programs_bytes.abs_diff(now.programs_bytes) >= RAM_STEP
}

#[cfg(feature = "snapshot")]
/// One sample of this process now (the snapshots; the monitor samples on its thread).
pub fn measure_now() -> Sample {
    let mut system = System::new();
    let pid = sysinfo::get_current_pid().ok();
    pid.and_then(|pid| measure(&mut system, pid)).unwrap_or_default()
}

fn measure(system: &mut System, pid: Pid) -> Option<Sample> {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    let process = system.process(pid)?;
    // What the process holds as its own (as Activity Monitor / Windows' private bytes count
    // it), not RSS: that counts the system's libraries shared with every other app.
    let own = (pid == sysinfo::get_current_pid().ok()?).then(process_stats::footprint).flatten();
    Some(Sample {
        cpu: process.cpu_usage(),
        ram_bytes: own.unwrap_or_else(|| process.memory()),
        ..Sample::default()
    })
}

/// The startup screen illustration, decoded on first use (a texture on the GPU).
pub fn splash_art() -> Option<render_api::ImageData> {
    let mut art = SPLASH_ART.lock().ok()?;
    if art.is_none() {
        *art = decode_splash_art();
    }
    art.clone()
}

fn decode_splash_art() -> Option<render_api::ImageData> {
    {
        // The art shown (About, the splash). The earlier ones are kept beside it
        // (`splash-city.jpg`, `splash-classic.jpg`, `splash-glass.jpg`): naming one here
        // brings it back. The full-size originals are in `assets/originals/` (not built in).
        let decoded = image_decode::decode(include_bytes!("../assets/splash-samurai.jpg"))
            .map_err(|e| tracing::warn!("the splash art couldn't be decoded: {e}"))
            .ok()?;
        // Cropped in the middle, to the card's aspect ratio, with its corners (radius in its
        // pixels).
        use ui_workbench::Splash;
        let (w, h) = (decoded.width, decoded.height);
        let crop_w = ((h as f32 * Splash::ASPECT).round() as u32).min(w);
        let x0 = (w - crop_w) / 2;
        let mut pixels = Vec::with_capacity((crop_w * h * 4) as usize);
        for y in 0..h {
            let row = ((y * w + x0) * 4) as usize;
            pixels.extend_from_slice(&decoded.pixels[row..row + (crop_w * 4) as usize]);
        }
        // At the bottom, frosted glass rising from the edge: the text stays readable.
        frost_bottom(&mut pixels, crop_w, h);
        let radius = Splash::RADIUS * crop_w as f32 / Splash::WIDTH;
        round_corners(&mut pixels, crop_w, h, radius);
        let decoded = image_decode::Decoded { width: crop_w, height: h, pixels, ..decoded };
        Some(render_api::ImageData::new(decoded.width, decoded.height, decoded.pixels))
    }
}

/// The splash illustration, while shown (then released, see [`release_splash_art`]).
static SPLASH_ART: std::sync::Mutex<Option<render_api::ImageData>> = std::sync::Mutex::new(None);

/// The splash is gone: the illustration's pixels are released (the GPU texture expires
/// on its own, unused).
pub fn release_splash_art() {
    if let Ok(mut art) = SPLASH_ART.lock() {
        *art = None;
    }
}

/// Where the frost starts (a share of the height, from the top).
const FROST_FROM: f32 = 0.40;

/// Frosted glass over the bottom of an RGBA image, rising from its edge: blurred more and
/// more towards the bottom and darkened to the night background, so the text over it stays
/// readable whatever the picture. Done once, when the art is prepared: the blur is made at
/// a quarter of the size (a box blur, three passes ≈ gaussian) and blended back smoothly.
fn frost_bottom(pixels: &mut [u8], width: u32, height: u32) {
    const NIGHT: [f32; 3] = [13.0, 15.0, 36.0];
    const STEP: u32 = 4;
    let y0 = (height as f32 * FROST_FROM) as u32;
    let (sw, sh) = (width.div_ceil(STEP), (height - y0).div_ceil(STEP));
    if sw == 0 || sh == 0 {
        return;
    }
    // A quarter-size copy of the frosted part (each cell the mean of its pixels).
    let mut small = vec![[0.0_f32; 3]; (sw * sh) as usize];
    for sy in 0..sh {
        for sx in 0..sw {
            let (mut sum, mut n) = ([0.0_f32; 3], 0.0);
            for y in (y0 + sy * STEP)..(y0 + (sy + 1) * STEP).min(height) {
                for x in (sx * STEP)..((sx + 1) * STEP).min(width) {
                    let i = ((y * width + x) * 4) as usize;
                    for c in 0..3 {
                        sum[c] += f32::from(pixels[i + c]);
                    }
                    n += 1.0;
                }
            }
            small[(sy * sw + sx) as usize] = sum.map(|v| v / n);
        }
    }
    for _ in 0..3 {
        box_blur(&mut small, sw, sh, 3);
    }
    for y in y0..height {
        let t = (y - y0) as f32 / (height - y0).max(1) as f32;
        // Fully frosted from 60% of the way down (where the text sits), smoothly from nothing
        // where it starts (no visible seam); darkened most at the very bottom.
        let b = (t / 0.6).min(1.0);
        let blur = b * b * (3.0 - 2.0 * b);
        let dark = 0.9 * (t / 0.85).min(1.0).powf(1.1);
        // Bilinear from the small copy (cell centres).
        let fy = ((y - y0) as f32 + 0.5) / STEP as f32 - 0.5;
        let (y_a, wy) = (fy.floor().clamp(0.0, (sh - 1) as f32) as u32, fy - fy.floor());
        let y_b = (y_a + 1).min(sh - 1);
        for x in 0..width {
            let fx = (x as f32 + 0.5) / STEP as f32 - 0.5;
            let (x_a, wx) = (fx.floor().clamp(0.0, (sw - 1) as f32) as u32, fx - fx.floor());
            let x_b = (x_a + 1).min(sw - 1);
            let at = |sx: u32, sy: u32| small[(sy * sw + sx) as usize];
            let i = ((y * width + x) * 4) as usize;
            for c in 0..3 {
                let top = at(x_a, y_a)[c] * (1.0 - wx) + at(x_b, y_a)[c] * wx;
                let bottom = at(x_a, y_b)[c] * (1.0 - wx) + at(x_b, y_b)[c] * wx;
                let blurred = top * (1.0 - wy) + bottom * wy;
                let v = f32::from(pixels[i + c]);
                let frosted = v + (blurred - v) * blur;
                pixels[i + c] = (frosted + (NIGHT[c] - frosted) * dark).round() as u8;
            }
        }
    }
}

/// One box blur pass (radius `r`, horizontal then vertical) over a small RGB image.
fn box_blur(img: &mut [[f32; 3]], w: u32, h: u32, r: i32) {
    let mut out = img.to_vec();
    let (wi, hi) = (w as i32, h as i32);
    let n = (2 * r + 1) as f32;
    for y in 0..hi {
        for x in 0..wi {
            let mut s = [0.0_f32; 3];
            for k in -r..=r {
                let p = img[(y * wi + (x + k).clamp(0, wi - 1)) as usize];
                for c in 0..3 {
                    s[c] += p[c];
                }
            }
            out[(y * wi + x) as usize] = s.map(|v| v / n);
        }
    }
    for y in 0..hi {
        for x in 0..wi {
            let mut s = [0.0_f32; 3];
            for k in -r..=r {
                let p = out[((y + k).clamp(0, hi - 1) * wi + x) as usize];
                for c in 0..3 {
                    s[c] += p[c];
                }
            }
            img[(y * wi + x) as usize] = s.map(|v| v / n);
        }
    }
}

/// Makes the corners of an RGBA image transparent (radius `r` in pixels), with a smooth edge.
fn round_corners(pixels: &mut [u8], width: u32, height: u32, r: f32) {
    let (w, h) = (width as f32, height as f32);
    for y in 0..height {
        for x in 0..width {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = px.clamp(r, w - r);
            let cy = py.clamp(r, h - r);
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let coverage = (r - d + 0.5).clamp(0.0, 1.0);
            if coverage < 1.0 {
                let i = ((y * width + x) * 4 + 3) as usize;
                pixels[i] = (f32::from(pixels[i]) * coverage).round() as u8;
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/resources.rs"]
mod tests;
