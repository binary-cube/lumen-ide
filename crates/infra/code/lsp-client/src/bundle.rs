//! The language servers that come with the app: an `lsp/` folder next to it, once for every
//! project, read-only —
//!
//! ```text
//! lsp/manifest.toml
//! lsp/node/<version>/…            Node (bin/node, or node.exe on Windows)
//! lsp/php/<version>/…             PHP (php, or php.exe on Windows)
//! lsp/servers/<server>/<version>/ a server's files
//! ```
//!
//! On macOS it's in the bundle's `Contents/Resources/lsp`, on Windows `lsp\` beside the
//! `.exe`; while developing, `target/lsp/` (made by `cargo xtask lsp-bundle`), found from the
//! binary in `target/debug/`. `EDITOR_LSP_DIR` names another one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The manifest's name in the folder.
pub const MANIFEST: &str = "manifest.toml";

/// What the folder holds.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    /// What it was made for (`macos-arm64`, `windows-x64`…).
    #[serde(default)]
    pub platform: String,
    pub node: Option<Component>,
    /// The PHP its PHP servers run on.
    #[serde(default)]
    pub php: Option<Component>,
    #[serde(default)]
    pub servers: BTreeMap<String, Component>,
}

/// Node, PHP, or a server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Component {
    pub version: String,
    /// Relative to the folder: Node's program, a server's folder.
    pub path: String,
    #[serde(default)]
    pub license: String,
}

/// The folder and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub root: PathBuf,
    pub manifest: Manifest,
}

impl Bundle {
    /// The folder at `root`, if it has a readable manifest.
    pub fn open(root: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(root.join(MANIFEST)).ok()?;
        match toml::from_str(&text) {
            Ok(manifest) => Some(Self { root: root.to_path_buf(), manifest }),
            Err(e) => {
                tracing::warn!(root = %root.display(), "the language servers' manifest can't be read: {e}");
                None
            }
        }
    }

    /// The app's folder, for the program at `exe` (see the module's doc).
    pub fn locate(exe: &Path) -> Option<Self> {
        if let Some(dir) = std::env::var_os("EDITOR_LSP_DIR") {
            return Self::open(Path::new(&dir));
        }
        candidates(exe).into_iter().find_map(|dir| Self::open(&dir))
    }

    /// Node's program, if it's there.
    pub fn node(&self) -> Option<PathBuf> {
        let node = self.manifest.node.as_ref()?;
        Some(self.root.join(&node.path)).filter(|p| p.is_file())
    }

    /// PHP's program, if it's there.
    pub fn php(&self) -> Option<PathBuf> {
        let php = self.manifest.php.as_ref()?;
        Some(self.root.join(&php.path)).filter(|p| p.is_file())
    }

    /// Server `name`'s folder and version, if it's there.
    pub fn server(&self, name: &str) -> Option<(PathBuf, &str)> {
        let server = self.manifest.servers.get(name)?;
        Some((self.root.join(&server.path), server.version.as_str())).filter(|(p, _)| p.is_dir())
    }
}

/// Where the folder may be, for the program at `exe`, in order.
pub fn candidates(exe: &Path) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else { return Vec::new() };
    let mut out = vec![dir.join("lsp")];
    // macOS: `<App>.app/Contents/MacOS/<id>` → `Contents/Resources/lsp`.
    if let Some(contents) = dir.parent() {
        out.push(contents.join("Resources").join("lsp"));
    }
    // Developing: `target/debug/<id>` (or `target/<triple>/release/<id>`) → `target/lsp`.
    // Not a test's binary (`target/debug/deps/<test>`): tests never start the real servers
    // by chance (those that do say where they are, `EDITOR_LSP_DIR`).
    if dir.file_name().is_some_and(|n| n == "deps") {
        return out;
    }
    for ancestor in dir.ancestors().skip(1).take(2) {
        if ancestor.file_name().is_some_and(|n| n == "target") {
            out.push(ancestor.join("lsp"));
        }
    }
    out
}

/// The platform the app runs on, as the bundle names it.
pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-arm64",
        ("macos", _) => "macos-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", _) => "windows-x64",
        ("linux", "aarch64") => "linux-arm64",
        _ => "linux-x64",
    }
}

#[cfg(test)]
#[path = "../tests/unit/bundle.rs"]
mod tests;
