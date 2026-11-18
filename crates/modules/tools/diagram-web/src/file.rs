//! The diagram's file, read and written for its draw.io tab (on its worker, never the UI
//! thread): read whole as text (bounded: a file too large or not text is refused, never cut
//! or mangled), written through a temporary file renamed over it — never over a change
//! someone else made meanwhile: then what the page holds is kept beside it, the file left as
//! they wrote it.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// The largest file read (bytes): a diagram with pictures is a few MB.
pub const MAX_FILE: u64 = 64 << 20;

/// The file's text; `None`: it isn't there (a new diagram when first opened; later, gone
/// or moved — not an empty diagram).
pub fn read(path: &Path) -> Result<Option<Arc<str>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(i18n::tr!("diagram_web.too_large", size = i18n::bytes(MAX_FILE)));
    }
    String::from_utf8(bytes)
        .map(|s| Some(Arc::from(s)))
        .map_err(|_| i18n::t("diagram_web.not_text"))
}

/// What a write came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrote {
    /// The file is `xml` now.
    Done,
    /// The file had changed meanwhile (it isn't what was last read or written): left as it
    /// is (`disk`), what was to be written kept beside it as `kept`.
    Conflict { disk: Arc<str>, kept: std::path::PathBuf },
}

/// `xml` written as the file, whole or not at all — if the file is still `expected` (what
/// was last read or written; `None`: none was, it may not be there); else kept beside it.
pub fn write_over(path: &Path, xml: &str, expected: Option<&str>) -> Result<Wrote, String> {
    let now = read(path)?;
    if now.as_deref() != expected
        && now.as_deref() != Some(xml)
        && let Some(disk) = now
    {
        let kept = beside(path);
        write(&kept, xml)?;
        return Ok(Wrote::Conflict { disk, kept });
    }
    write(path, xml).map(|()| Wrote::Done)
}

/// `xml` written as the file, whole or not at all.
pub fn write(path: &Path, xml: &str) -> Result<(), String> {
    files::write_atomic(path, xml.as_bytes()).map_err(|e| e.to_string())
}

/// A free name beside `path` for what the page held (`flow (mine).drawio`, then
/// `flow (mine 2).drawio`…).
pub fn beside(path: &Path) -> std::path::PathBuf {
    let dir = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let ext = path.extension().map_or_else(String::new, |e| e.to_string_lossy().into_owned());
    let mine = i18n::t("diagram_web.mine");
    (1..1000)
        .map(|n| {
            let tag = if n == 1 { mine.clone() } else { format!("{mine} {n}") };
            dir.join(format!("{stem} ({tag}).{ext}"))
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} ({mine} {}).{ext}", kernel::id::new_uuid())))
}

#[cfg(test)]
#[path = "../tests/unit/file.rs"]
mod tests;
