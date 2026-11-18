//! Atomic file writes.

use std::io;
use std::path::Path;

/// Writes `text` to `path` through a temporary file (synced) and a rename: a half-written
/// file (e.g. the app stopped abruptly, a power loss) never replaces a good one.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    files::write_atomic(path, text.as_bytes())
}
