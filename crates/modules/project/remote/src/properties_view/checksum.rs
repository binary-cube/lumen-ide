//! The properties' Checksum tab: the algorithm (SHA-512 … MD5, the last one chosen kept),
//! **Calculate** — each chosen file's digest computed on the server where it can (its
//! tools, FTP's `HASH`), else downloaded and computed here —, each compared with the local
//! copy's (the same, different, not here) and copied to the clipboard.
//!
//! It runs in the background on one session of the project's pool, one file after the
//! other, stopped by its button or when the dialog closes.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};

use remote::application::Connector;
use remote::properties::{Algo, Checksum};

/// The algorithm chosen last (the next dialog starts with it).
static LAST: Mutex<Algo> = Mutex::new(Algo::Sha256);

pub(crate) fn last_algo() -> Algo {
    *LAST.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn remember_algo(algo: Algo) {
    *LAST.lock().unwrap_or_else(PoisonError::into_inner) = algo;
}

/// How a file compares with its local copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Local {
    Same,
    Different,
    /// No local copy (not brought here, on demand), or unreadable.
    Missing,
}

/// A file's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Sum {
    /// Not computed (yet).
    Waiting,
    Done {
        sum: Checksum,
        local: Local,
    },
    Failed(String),
}

/// The tab's state: the algorithm, each file (relative path) and its digest, what runs.
#[derive(Debug, Default)]
pub(crate) struct Checksums {
    pub algo: Option<Algo>,
    pub rows: Vec<(String, Sum)>,
    running: Option<Running>,
}

#[derive(Debug)]
struct Running {
    stop: Arc<AtomicBool>,
    done: Receiver<(usize, Sum)>,
}

impl Checksums {
    /// The files to compute (folders have none).
    pub fn for_files(paths: impl IntoIterator<Item = String>) -> Self {
        let rows = paths.into_iter().map(|p| (p, Sum::Waiting)).collect();
        Self { algo: Some(last_algo()), rows, running: None }
    }

    pub fn algo(&self) -> Algo {
        self.algo.unwrap_or_else(last_algo)
    }

    /// Another algorithm: what was computed with the last one is let go.
    pub fn choose(&mut self, algo: Algo) {
        if self.algo() != algo {
            self.stop();
            for (_, sum) in &mut self.rows {
                *sum = Sum::Waiting;
            }
        }
        self.algo = Some(algo);
        remember_algo(algo);
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    pub fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Computes every file's digest in the background (`local`: the local copy's root).
    pub fn start(
        &mut self,
        connector: Arc<dyn Connector>,
        local: std::path::PathBuf,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) {
        self.stop();
        let algo = self.algo();
        let paths: Vec<String> = self.rows.iter().map(|(p, _)| p.clone()).collect();
        for (_, sum) in &mut self.rows {
            *sum = Sum::Waiting;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, done) = std::sync::mpsc::channel();
        let flag = Arc::clone(&stop);
        kernel::work::run("remote-checksum", move || {
            compute(connector.as_ref(), &local, &paths, algo, &flag, &mut |i, sum| {
                let _ = tx.send((i, sum));
                wake();
            });
        });
        self.running = Some(Running { stop, done });
    }

    /// What came from the background: `true` if something did.
    pub fn poll(&mut self) -> bool {
        let Some(running) = &self.running else { return false };
        let mut changed = false;
        loop {
            match running.done.try_recv() {
                Ok((i, sum)) => {
                    if let Some(row) = self.rows.get_mut(i) {
                        row.1 = sum;
                    }
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.running = None;
                    return true;
                }
            }
        }
        changed
    }
}

impl Drop for Checksums {
    fn drop(&mut self) {
        // The dialog closed: what still runs stops.
        self.stop();
    }
}

/// Each of `paths`' digests, told to `told` as they come: the server's where it can,
/// else the file downloaded and computed here; compared with the local copy under `local`.
pub(crate) fn compute(
    connector: &dyn Connector,
    local: &Path,
    paths: &[String],
    algo: Algo,
    stop: &AtomicBool,
    told: &mut dyn FnMut(usize, Sum),
) {
    let mut fs = match connector.connect() {
        Ok(fs) => fs,
        Err(e) => {
            for i in 0..paths.len() {
                told(i, Sum::Failed(e.to_string()));
            }
            return;
        }
    };
    for (i, path) in paths.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let sum = match fs.checksum(path, algo) {
            Ok(Some(hex)) => Ok(Checksum { algo, hex, on_server: true }),
            Ok(None) => fs
                .read_moving(path, &mut |_| !stop.load(Ordering::Relaxed))
                .map_err(|e| e.to_string())
                .and_then(|bytes| digest(algo, bytes.as_slice()).map_err(|e| e.to_string()))
                .map(|hex| Checksum { algo, hex, on_server: false }),
            Err(e) => Err(e.to_string()),
        };
        let row = match sum {
            Ok(sum) => {
                let local = compare(&local_file(local, path), algo, &sum.hex);
                Sum::Done { sum, local }
            }
            Err(_) if stop.load(Ordering::Relaxed) => return,
            Err(why) => Sum::Failed(why),
        };
        told(i, row);
    }
}

/// A relative path (with `/`) under `root`, part by part (the system's separators).
fn local_file(root: &Path, relative: &str) -> std::path::PathBuf {
    relative.split('/').filter(|p| !p.is_empty()).fold(root.to_path_buf(), |p, part| p.join(part))
}

/// How the local copy at `file` compares with digest `hex`.
fn compare(file: &Path, algo: Algo, hex: &str) -> Local {
    match std::fs::File::open(file).and_then(|f| digest(algo, std::io::BufReader::new(f))) {
        Ok(here) if here == hex => Local::Same,
        Ok(_) => Local::Different,
        Err(_) => Local::Missing,
    }
}

/// The digest of what `read` gives, in lower-case hex (read in pieces: a big file is never
/// held whole).
pub(crate) fn digest(algo: Algo, read: impl Read) -> std::io::Result<String> {
    fn run<D: sha2::Digest>(mut read: impl Read) -> std::io::Result<String> {
        let mut hasher = D::new();
        let mut chunk = vec![0; 64 * 1024];
        loop {
            let n = read.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            hasher.update(&chunk[..n]);
        }
        Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }
    match algo {
        Algo::Sha512 => run::<sha2::Sha512>(read),
        Algo::Sha384 => run::<sha2::Sha384>(read),
        Algo::Sha256 => run::<sha2::Sha256>(read),
        Algo::Sha224 => run::<sha2::Sha224>(read),
        Algo::Sha1 => run::<sha1::Sha1>(read),
        Algo::Md5 => run::<md5::Md5>(read),
    }
}

impl super::PropertiesView {
    /// The tab: the algorithm and Calculate (Stop while it runs), then each file — its name,
    /// where its digest was computed, how it compares with the local copy, its digest (a
    /// button copies it).
    pub(super) fn paint_checksums(
        &mut self,
        pen: &mut ui_components::LayerPainter<'_>,
        (x0, x1): (f32, f32),
        mut y: f32,
    ) {
        let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
        use i18n::t;
        use kernel::Rect;
        use text_layout::Font;
        use ui_components::ds::{self, ButtonKind, ButtonSpec, Tone, button_size};
        use ui_kit::draw_text;

        use super::{Hit, LABEL_W, Pick, ROW_H};
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        y += px(4.0);
        // The algorithm, Calculate.
        let label = text.shape_line(&t("remote.props_algo"), &font);
        draw_text(layer, &label, x0, ds::optical_baseline(y, px(ROW_H), &font), p.text_muted);
        let field = Rect::new(x0 + px(LABEL_W), y, px(240.0), px(ROW_H));
        let spec = ds::DropdownSpec {
            value: self.sums.algo().label(),
            open: self.picking == Some(Pick::Algo),
            hovered: self.hover == Some(Hit::Algo),
            enabled: true,
            note: None,
            placeholder: None,
        };
        ds::dropdown(layer, text, icons, ds, field, spec);
        self.hits.push((field, Hit::Algo));
        self.anchors.push((Pick::Algo, field));
        let running = self.sums.is_running();
        let go = if running { t("remote.props_measure_stop") } else { t("remote.props_calculate") };
        let w = ds::primary_width(text, ds, &[&go]);
        let button = Rect::new(field.max_x() + px(12.0), y, w, px(ROW_H));
        let hovered = self.hover == Some(Hit::Calculate);
        if running {
            ds::secondary_button(layer, text, ds, button, &go, true, hovered);
        } else {
            ds::primary_button(layer, text, ds, button, &go, self.state.is_some(), hovered);
        }
        if self.state.is_some() {
            self.hits.push((button, Hit::Calculate));
        }
        y += px(ROW_H) + px(14.0);
        ds::divider(layer, ds, x0, x1, y);
        y += px(6.0);
        // The files.
        let strong = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.0);
        let mono = ds.code_font(-1.0);
        let row_h = px(58.0);
        let bottom = self.footer_top - px(12.0);
        let fits = ((bottom - y) / row_h).floor().max(1.0) as usize;
        let count = self.sums.rows.len();
        let shown = if count > fits { fits - 1 } else { count };
        for i in 0..shown {
            let (path, sum) = self.sums.rows[i].clone();
            let top = y + px(6.0);
            let name = path.rsplit('/').next().unwrap_or(&path);
            // On the right: the copy button, how it compares.
            let mut right = x1;
            if let Sum::Done { local, .. } = &sum {
                let s = px(button_size::MEDIUM);
                let copy = Rect::new(right - s, top, s, s);
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .icon("copy")
                    .rest_tone(Tone::Neutral)
                    .hovered(self.hover == Some(Hit::Copy(i)));
                ds::button(layer, text, icons, ds, copy, spec);
                self.hits.push((copy, Hit::Copy(i)));
                right = copy.min_x() - px(8.0);
                let (key, tone) = match local {
                    Local::Same => ("remote.props_same", Tone::Success),
                    Local::Different => ("remote.props_different", Tone::Danger),
                    Local::Missing => ("remote.props_missing", Tone::Neutral),
                };
                let badge = t(key);
                let spec = ds::BadgeSpec {
                    label: &badge,
                    x: right,
                    mid: top + s / 2.0,
                    align_right: true,
                    tone,
                };
                right = ds::badge(layer, text, ds, spec).min_x() - px(10.0);
            }
            let shown_name = ds::ellipsize_file(text, name, &strong, (right - x0).max(0.0));
            let shaped = text.shape_line(&shown_name, &strong);
            draw_text(layer, &shaped, x0, ds::optical_baseline(top, px(26.0), &strong), p.text);
            // Under it: the digest and where it was computed, or what's going on.
            let line = top + px(26.0);
            let (body, color, font) = match &sum {
                Sum::Done { sum, .. } => (sum.hex.clone(), p.text, &mono),
                Sum::Failed(why) => (why.clone(), p.danger, &small),
                Sum::Waiting if running => (t("remote.props_sum_running"), p.text_muted, &small),
                Sum::Waiting => (t("remote.props_sum_waiting"), p.text_muted, &small),
            };
            let mut room = x1 - x0;
            if let Sum::Done { sum, .. } = &sum {
                let key =
                    if sum.on_server { "remote.props_sum_server" } else { "remote.props_sum_here" };
                let shaped = text.shape_line(&t(key), &small);
                let wx = x1 - shaped.width;
                draw_text(
                    layer,
                    &shaped,
                    wx,
                    ds::optical_baseline(line, px(22.0), &small),
                    p.text_muted,
                );
                room = wx - x0 - px(12.0);
            }
            let body = ds::ellipsize_middle(text, &body, font, room.max(0.0));
            let shaped = text.shape_line(&body, font);
            draw_text(layer, &shaped, x0, ds::optical_baseline(line, px(22.0), font), color);
            y += row_h;
        }
        if shown < count {
            let more = i18n::tr!("remote.props_more", count = count - shown);
            let shaped = text.shape_line(&more, &small);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(22.0), &small), p.text_muted);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/properties_view/checksum.rs"]
mod tests;
