//! Files on disk as the workbench opens them: their text, their `.editorconfig`, their paths
//! simplified and shown from home, the virtual tabs' paths.

use super::*;

impl Workbench {
    /// Whether `path` is a folder (through the disk's port; a link followed).
    pub(super) fn is_dir(&self, path: &Path) -> bool {
        self.ports.disk.kind(path) == Some(NodeKind::Directory)
    }

    /// Whether anything is at `path` (through the disk's port; a link followed).
    pub(super) fn exists(&self, path: &Path) -> bool {
        self.ports.disk.kind(path).is_some()
    }
}

/// Applies the `.editorconfig` settings for `path` to the document. Returns
/// `max_line_length` (for the editor's vertical guide).
/// A closed file's text (UTF-8), read through the files port.
pub(super) fn read_text(fs: &dyn Disk, path: &Path) -> Option<String> {
    String::from_utf8(document::DocumentStorage::read(fs, path).ok()?).ok()
}

pub(super) fn apply_editorconfig(doc: &mut Document, path: &Path, fs: &dyn Disk) -> Option<u32> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    let config = editorconfig::resolve(&absolute, fs);
    if config.is_empty() {
        return None;
    }
    tracing::debug!(?config, path = %absolute.display(), ".editorconfig aplicat");
    match (config.indent_style, config.indent_size) {
        (Some(IndentStyle::Tab), _) => doc.set_indent(Indent::Tabs),
        (Some(IndentStyle::Space), Some(n)) => doc.set_indent(Indent::Spaces(n)),
        (Some(IndentStyle::Space), None) => doc.set_indent(Indent::Spaces(4)),
        (None, Some(n)) if matches!(doc.indent(), Indent::Spaces(_)) => {
            doc.set_indent(Indent::Spaces(n))
        }
        _ => {}
    }
    if let Some(width) = config.tab_width {
        doc.set_tab_width(u32::from(width));
    }
    match config.end_of_line {
        Some(EndOfLine::Lf) => doc.set_line_ending(LineEnding::Lf),
        Some(EndOfLine::CrLf) => doc.set_line_ending(LineEnding::CrLf),
        Some(EndOfLine::Cr) => {
            tracing::warn!(
                "end_of_line = cr isn't supported; keeping {}",
                doc.line_ending().label()
            )
        }
        None => {}
    }
    if let Some(charset) = config.charset {
        doc.set_encoding(match charset {
            Charset::Utf8 => Encoding::Utf8,
            Charset::Utf8Bom => Encoding::Utf8Bom,
            Charset::Latin1 => Encoding::Latin1,
            Charset::Utf16Le => Encoding::Utf16Le,
            Charset::Utf16Be => Encoding::Utf16Be,
        });
    }
    doc.set_save_cleanup(SaveCleanup {
        trim_trailing_whitespace: config.trim_trailing_whitespace.unwrap_or(false),
        insert_final_newline: config.insert_final_newline,
    });
    config.max_line_length
}

/// The displayed path, with the user's home directory shortened to `~`.
pub(super) fn home_relative(path: &Path) -> String {
    // Looked up once (on Windows it's a system call; the status bar asks on every frame).
    static HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    kernel::paths::tildify(path, HOME.get_or_init(dirs::home_dir).as_deref())
}

/// On Windows, `canonicalize` returns "verbatim" paths (`\\?\C:\project`), which shells
/// and the user don't recognize; we bring them to the usual form (`C:\project`).
/// Network paths (`\\?\UNC\…`) stay unchanged.
pub(super) fn simplify_path(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else { return path };
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// A virtual tab (the canvas, the comparison, the configurator): not a file on disk.
/// A tab that isn't a file (the canvas, a comparison, a module's view such as "Commit"):
/// their paths end in an internal extension, `view-*`.
pub(super) fn is_virtual(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.starts_with(VIRTUAL_EXT_PREFIX))
}
