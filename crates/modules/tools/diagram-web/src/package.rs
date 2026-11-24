//! The pinned draw.io package and its install: one version, downloaded from the project's
//! GitHub release only (its SHA-256 pinned here: GitHub reports it, jgraph doesn't sign it),
//! into the app's data folder (`<data>/web-apps/diagram/<version>/`, never a project's),
//! for every project and window.
//!
//! The war (a zip) is unpacked by our own code into a temporary folder beside its place and
//! renamed into it whole, its marker written last: a half install is never used, and what a
//! crash left (a temporary folder, an older version) is removed at the next install. Only
//! what the editor needs in embed mode is kept ([`kept`]): no server files, no viewers, no
//! templates, no service worker, no other languages than ours.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use pinned_downloads::archive::{self, Rules};
use pinned_downloads::fetch::{self, Fetch, FetchError, Pin};

/// The version installed.
pub const VERSION: &str = "32.6.10";

/// Where it comes from: the release's `draw.war`.
pub const URL: &str = "https://github.com/jgraph/drawio/releases/download/v32.6.10/draw.war";

/// Its SHA-256 (checked on 2026-11-17 against a download of our own).
pub const SHA256: &str = "a57b06b3b6a876a351dacfe65d027d9509319cd030e9bacf6ae04a034d66a38f";

/// What's downloaded (bytes).
pub const DOWNLOAD_SIZE: u64 = 49_729_549;

/// What it takes on disk once installed (bytes, the files kept: measured).
pub const INSTALLED_SIZE: u64 = 45_531_084;

/// The most a download may bring (bytes): a server sending on is stopped there.
const MAX_DOWNLOAD: u64 = 96 << 20;

/// The only hosts it's downloaded from: GitHub's releases and where they redirect.
pub const HOSTS: [&str; 3] =
    ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];

/// A complete install's marker, in its folder (written last).
pub const MARKER: &str = ".installed";

/// The license's notice, written beside the files (the package carries none of its own).
pub const NOTICE_FILE: &str = "NOTICE.txt";

/// What the notice says.
pub const NOTICE: &str = "draw.io (https://github.com/jgraph/drawio), version 32.6.10\n\
Copyright (c) JGraph Ltd and draw.io AG.\n\
\n\
Licensed under the Apache License, Version 2.0 (the \"License\"); you may not use these\n\
files except in compliance with the License. You may obtain a copy of the License at\n\
\n\
    http://www.apache.org/licenses/LICENSE-2.0\n\
\n\
Unless required by applicable law or agreed to in writing, software distributed under the\n\
License is distributed on an \"AS IS\" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,\n\
either express or implied. See the License for the specific language governing permissions\n\
and limitations under the License.\n\
\n\
These are the files of the official release, unchanged; only those its embedded editor\n\
doesn't use were left out. \"draw.io\" is a trademark of its owners: this copy is run\n\
locally by the editor and is not made or endorsed by them.\n";

/// Folders left out whole: the server's, the templates' (not offered in embed mode), the
/// plugins' (turned off), the integrations' (Atlassian, OneDrive, collaboration), the sources
/// of the development build (the release runs from its bundles).
const LEFT_OUT_FOLDERS: [&str; 11] = [
    "WEB-INF",
    "META-INF",
    "templates",
    "plugins",
    "connect",
    "js/diagramly",
    "js/grapheditor",
    "js/orgchart",
    "js/onedrive",
    "js/simplepeer",
    "mxgraph/src",
];

/// Files left out: the viewers and the Confluence integration (their own bundles), the
/// service worker (`pwa=0`), the pages of other hosts (the export server, Teams, OneDrive,
/// the importer, the shortcuts' page opened in a new window), the development build's
/// sources, a filler with no use.
const LEFT_OUT_FILES: [&str; 24] = [
    "js/integrate.min.js",
    "js/viewer.min.js",
    "js/viewer-static.min.js",
    "js/embed.dev.js",
    "service-worker.js",
    "workbox-05b6c01b.js",
    "tail.bin",
    "Graph.js",
    "Editor.js",
    "Init.js",
    "shortcuts.svg",
    "clear.html",
    "js/clear.js",
    "open.html",
    "js/open.js",
    "teams.html",
    "onedrive3.html",
    "vsdxImporter.html",
    "js/vsdxImporter.js",
    "export3.html",
    "export-fonts.css",
    "js/export.js",
    "js/export-init.js",
    "monday-app-association.json",
];

/// The languages kept of the interface's translations (`resources/dia_<lang>.txt`; English
/// is `dia.txt`): the app's own.
pub const LANGUAGES: [&str; 1] = ["ro"];

/// Whether the package's file `path` (relative, `/`-separated) is kept.
pub fn kept(path: &str) -> bool {
    if LEFT_OUT_FOLDERS.iter().any(|f| path.strip_prefix(f).is_some_and(|r| r.starts_with('/'))) {
        return false;
    }
    if LEFT_OUT_FILES.contains(&path) || path.ends_with(".map") {
        return false;
    }
    if let Some(lang) = path.strip_prefix("resources/dia_").and_then(|r| r.strip_suffix(".txt")) {
        return LANGUAGES.contains(&lang);
    }
    true
}

/// The version's folder under `root`.
pub fn version_dir(root: &Path) -> PathBuf {
    root.join(VERSION)
}

/// The installed version's folder under `root`, if it's whole (its marker and its page).
/// Reads the disk: on a worker.
pub fn installed(root: &Path) -> Option<PathBuf> {
    let dir = version_dir(root);
    let marker = std::fs::read_to_string(dir.join(MARKER)).ok()?;
    (marker.lines().next() == Some(VERSION) && dir.join("index.html").is_file()).then_some(dir)
}

/// Why an install didn't happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The download (the network, the pin, a stop).
    Fetch(FetchError),
    /// Unpacking it or moving it into place.
    Unpack(String),
}

impl InstallError {
    /// The network is the reason.
    pub fn offline(&self) -> bool {
        matches!(self, Self::Fetch(e) if e.offline())
    }

    /// Stopped by the user.
    pub fn stopped(&self) -> bool {
        matches!(self, Self::Fetch(FetchError::Stopped))
            || matches!(self, Self::Unpack(why) if why == "stopped")
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch(e) => e.fmt(f),
            Self::Unpack(why) => f.write_str(why),
        }
    }
}

/// Where a download under way is kept (resumed after a break).
fn part_path(root: &Path) -> PathBuf {
    root.join(".downloads").join(format!("draw-{VERSION}.war.part"))
}

/// The package downloaded (resumed if a part is there) and installed under `root`:
/// `progress(received, total)` as it comes, `unpacking()` once it's whole; `stop` ends it.
/// Network and disk: on a worker.
pub fn install(
    fetch: &dyn Fetch,
    root: &Path,
    progress: &dyn Fn(u64, Option<u64>),
    unpacking: &dyn Fn(),
    stop: &AtomicBool,
) -> Result<PathBuf, InstallError> {
    if let Some(dir) = installed(root) {
        return Ok(dir);
    }
    let part = part_path(root);
    let pin = Pin::Sha256(SHA256.to_owned());
    fetch::fetch_file(
        fetch::Going { fetch, progress, stop },
        fetch::Wanted { url: URL, hosts: &HOSTS, pin: &pin, limit: MAX_DOWNLOAD },
        &part,
    )
    .map_err(InstallError::Fetch)?;
    unpacking();
    let dir = install_archive(&part, root, stop)?;
    let _ = std::fs::remove_file(&part);
    Ok(dir)
}

/// The war at `archive` (already checked) installed under `root`: unpacked into a temporary
/// folder, its notice and marker written, renamed into its place; what earlier installs
/// left (other versions, temporary folders) removed.
pub fn install_archive(
    archive: &Path,
    root: &Path,
    stop: &AtomicBool,
) -> Result<PathBuf, InstallError> {
    let unpack = |e: String| InstallError::Unpack(e);
    std::fs::create_dir_all(root).map_err(|e| unpack(format!("{}: {e}", root.display())))?;
    clean(root);
    let staging = root.join(format!(".tmp-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    let rules = Rules { filter: Some(kept), ..Rules::default() };
    let unpacked = std::fs::File::open(archive)
        .map_err(|e| format!("{}: {e}", archive.display()))
        .and_then(|f| archive::unpack_zip(&mut std::io::BufReader::new(f), &staging, &rules, stop))
        .and_then(|_| {
            if staging.join("index.html").is_file() {
                Ok(())
            } else {
                Err("the package has no index.html".to_owned())
            }
        })
        .and_then(|()| {
            let size = folder_size(&staging);
            std::fs::write(staging.join(NOTICE_FILE), NOTICE)
                .and_then(|()| std::fs::write(staging.join(MARKER), format!("{VERSION}\n{size}\n")))
                .map_err(|e| format!("{}: {e}", staging.display()))
        });
    if let Err(e) = unpacked {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(unpack(e));
    }
    let dir = version_dir(root);
    // A broken install there (no marker) goes first.
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    if let Err(e) = std::fs::rename(&staging, &dir) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(unpack(format!("{}: {e}", dir.display())));
    }
    Ok(dir)
}

/// What earlier installs left under `root`: temporary folders (a crash, a stop), other
/// versions (this one replaces them).
fn clean(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let left = name.starts_with(".tmp-")
            || (!name.starts_with('.') && name != VERSION && entry.path().is_dir());
        if left {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The bytes of the files under `dir` (links not followed; at most a few thousand files).
pub fn folder_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                total = total.saturating_add(entry.metadata().map_or(0, |m| m.len()));
            }
        }
    }
    total
}

#[cfg(test)]
#[path = "../tests/unit/package.rs"]
mod tests;
