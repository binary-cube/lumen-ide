//! The engine downloaded once, at the first page that needs it, and kept up to date (plan
//! 99b, 99g): the project's signed list of packages read ([`manifest`]), the newest for
//! this system and protocol downloaded (pinned to its SHA-256 and size, resumed after a
//! break, stopped on asking), unpacked by our code into `.staging`, checked whole, moved
//! into its place and chosen ([`layout`]). A host keeps running from the version it started
//! with; the next start takes the new one, and what's left behind is cleaned once nothing
//! runs from it. A version that doesn't start twice is given up for the one before. An
//! update never goes back to an older version, and nothing below the list's safe minimum is
//! installed (one installed below it is replaced as soon as a newer one is there).
//!
//! Network and disk: on a worker. The app says where (its data folder) and asks; it never
//! names a file of here.

pub mod layout;
pub mod manifest;
pub mod service;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use pinned_downloads::fetch::{self, Bounded, Fetch, FetchError, Pin};
use web_engine_versions::Wanted;

pub use layout::State;
pub use manifest::{Manifest, Package, Pick};
pub use web_engine_protocol::EngineInfo;

/// The hosts the list and the packages come from: GitHub's release pages and where they
/// send the files.
pub const HOSTS: [&str; 3] =
    ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];

/// The release holding the list and the packages (one, its files replaced as versions come).
const RELEASE_TAG: &str = "web-engine";
/// The list's file, and its signature beside it.
const MANIFEST: &str = "web-engine.json";
const SIGNATURE: &str = "web-engine.json.sig";

/// The most the list or its signature may weigh (a server sending on forever stops).
const SMALL: u64 = 256 * 1024;

/// Where the list of `repository` (`owner/name` on GitHub) is, and its signature.
pub fn manifest_urls(repository: &str) -> (String, String) {
    let base = format!("https://github.com/{repository}/releases/download/{RELEASE_TAG}");
    (format!("{base}/{MANIFEST}"), format!("{base}/{SIGNATURE}"))
}

/// What a look for the engine is asked with.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The repository (`owner/name`) it's published in.
    pub repository: &'a str,
    /// The project's public key (hex): the list is trusted only when it signed it.
    pub public: &'a str,
    /// The engine's place (its state: the one installed, those that failed here).
    pub root: &'a Path,
    /// The protocol the app speaks.
    pub protocol: u32,
    /// The app's version (the versions it found bad itself are never taken again).
    pub app: &'a str,
}

/// Why the engine wasn't installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The list or the package couldn't be had (the network, a pin, a stop).
    Fetch(FetchError),
    /// The list isn't trusted or readable.
    Manifest(manifest::ManifestError),
    /// None is made for this system and protocol.
    NoPackage,
    /// The package downloaded isn't whole, or isn't what the list said.
    Incomplete(String),
    /// Unpacking it or moving it into place (the disk).
    Unpack(String),
    /// Stopped by the user.
    Stopped,
}

impl InstallError {
    /// The network is the reason.
    pub fn offline(&self) -> bool {
        matches!(self, Self::Fetch(e) if e.offline())
    }

    /// Stopped by the user.
    pub fn stopped(&self) -> bool {
        matches!(self, Self::Fetch(FetchError::Stopped) | Self::Stopped)
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch(e) => e.fmt(f),
            Self::Manifest(e) => e.fmt(f),
            Self::NoPackage => f.write_str("no web engine is made for this system"),
            Self::Incomplete(why) | Self::Unpack(why) => f.write_str(why),
            Self::Stopped => f.write_str("stopped"),
        }
    }
}

impl From<FetchError> for InstallError {
    fn from(e: FetchError) -> Self {
        Self::Fetch(e)
    }
}

/// What the list says for this install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Newest {
    /// This package is newer than the one installed (or none is).
    Install(Package),
    /// The one installed is the newest that fits (an update never goes back); `below_minimum`:
    /// it's older than the list's safe minimum and nothing replaces it.
    UpToDate { below_minimum: bool },
}

/// What the signed list says for this system, speaking `ask.protocol`: the newest package
/// at least its minimum and none that failed here, if it's newer than the one installed.
pub fn newest(fetch: &dyn Fetch, ask: Ask<'_>) -> Result<Newest, InstallError> {
    let (list, signature) = manifest_urls(ask.repository);
    let bytes = small(fetch, &list)?;
    let signature = String::from_utf8_lossy(&small(fetch, &signature)?).into_owned();
    let manifest =
        Manifest::read(&bytes, &signature, ask.public).map_err(InstallError::Manifest)?;
    let platform = manifest::platform().ok_or(InstallError::NoPackage)?;
    let state = layout::read_state(ask.root);
    let current = layout::chosen(ask.root, ask.protocol).map(|(_, engine)| engine.version);
    let wanted = Wanted {
        platform,
        protocol: ask.protocol,
        refused: state.refused(ask.app),
        current: current.as_deref(),
    };
    match manifest.pick(wanted) {
        Pick::Install(package) => Ok(Newest::Install(package.clone())),
        Pick::Current { below_minimum } => Ok(Newest::UpToDate { below_minimum }),
        Pick::Nothing => Err(InstallError::NoPackage),
    }
}

/// How a download goes: `progress(received, total)` as it comes, `unpacking()` once it's
/// whole, `stop` ends it.
pub struct Going<'a> {
    /// Where the package comes from (the network; a test's memory).
    pub fetch: &'a dyn Fetch,
    /// Told what came (bytes) of how much (`None`: not known yet), as it comes.
    pub progress: &'a dyn Fn(u64, Option<u64>),
    /// Told once the package is whole, before it's unpacked and checked.
    pub unpacking: &'a dyn Fn(),
    /// Set: the download and the unpacking end as soon as they look (what came is kept).
    pub stop: &'a AtomicBool,
}

impl std::fmt::Debug for Going<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Going").field("stop", &self.stop).finish_non_exhaustive()
    }
}

/// `package` installed under `root` and chosen (there already: only chosen). Its folder.
///
/// The download, its check, its unpacking, its move into place and its choice all happen
/// while `.staging` is held ([`layout::hold_staging`]): a second download (another window's,
/// another instance's) waits for this one, then finds it there; a clean waits for it to be
/// chosen (it never takes a version just moved into place for one nobody chose), and nobody
/// rewrites the `.part` between its check and its unpacking.
pub fn install(going: Going<'_>, root: &Path, package: &Package) -> Result<PathBuf, InstallError> {
    let malformed = |why: &str| {
        InstallError::Manifest(manifest::ManifestError::Malformed(format!(
            "package {}: {why}",
            package.version
        )))
    };
    if web_engine_versions::Version::parse(&package.version).is_none() {
        return Err(malformed("its version"));
    }
    let name = web_engine_versions::archive_name(&package.url)
        .ok_or_else(|| malformed("its archive's name"))?;
    let dir = root.join(&package.version);
    let staging = layout::hold_staging(root, going.stop).map_err(unpack_error)?;
    let Some(staging) = staging else { return Err(InstallError::Stopped) };
    if layout::engine_in(&dir).is_some_and(|e| e.version == package.version) {
        layout::choose(root, &package.version).map_err(unpack_error)?;
        return Ok(dir);
    }
    let part = staging.dir().join(format!("{}.{name}.part", package.version));
    fetch::fetch_file(
        fetch::Going { fetch: going.fetch, progress: going.progress, stop: going.stop },
        fetch::Wanted {
            url: &package.url,
            hosts: &HOSTS,
            pin: &Pin::Sha256(package.sha256.clone()),
            limit: package.size,
        },
        &part,
    )?;
    (going.unpacking)();
    let unpacked = staging.dir().join(&package.version);
    let _ = std::fs::remove_dir_all(&unpacked);
    let rules = pinned_downloads::archive::Rules::default();
    let made =
        pinned_downloads::archive::unpack_path(&part, name, &unpacked, &rules, None, going.stop);
    if let Err(why) = made {
        let _ = std::fs::remove_dir_all(&unpacked);
        if going.stop.load(Ordering::Relaxed) {
            return Err(InstallError::Stopped);
        }
        // What was checked can't be unpacked: it's let go, downloaded anew next time.
        let _ = std::fs::remove_file(&part);
        return Err(InstallError::Unpack(why));
    }
    // Whole and what the list said, or nothing of it is kept.
    let whole = layout::engine_in(&unpacked)
        .is_some_and(|e| e.version == package.version && e.protocol == package.protocol);
    if !whole {
        let _ = std::fs::remove_dir_all(&unpacked);
        let _ = std::fs::remove_file(&part);
        return Err(InstallError::Incomplete(format!(
            "the package {} isn't whole",
            package.version
        )));
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&unpacked, &dir).map_err(unpack_error)?;
    let _ = std::fs::remove_file(&part);
    ensure_loadable(&dir);
    layout::choose(root, &package.version).map_err(unpack_error)?;
    drop(staging);
    tracing::info!(version = %package.version, "the web engine installed");
    Ok(dir)
}

/// The engine's program made loadable on macOS: its quarantine attributes removed (the app
/// that downloaded it may itself be quarantined, and macOS passes that to files it writes),
/// and its executables re-signed ad-hoc with `disable-library-validation` when the build's
/// certificate has no Apple Team ID (without it, the hardened runtime refuses to load the
/// CEF framework into the host and its helpers: "different Team IDs").
#[cfg(target_os = "macos")]
fn ensure_loadable(dir: &Path) {
    for attr in ["com.apple.quarantine", "com.apple.provenance"] {
        let _ = std::process::Command::new("xattr")
            .args(["-d", "-r", attr])
            .arg(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    resign_if_needed(dir);
}

/// Re-signs every executable in the engine's bundle ad-hoc with
/// `disable-library-validation` when the main program doesn't have it: a build signed
/// with a certificate that has no Apple Team ID needs it (the workflow adds it for ad-hoc
/// builds, but not for certificate builds without a team). Already-correct builds (ad-hoc
/// with the entitlement, or a real Team ID) are left alone.
#[cfg(target_os = "macos")]
fn resign_if_needed(dir: &Path) {
    let app = dir.join("web-host.app");
    let binary = app.join("Contents/MacOS/web-host");
    if !binary.is_file() {
        return;
    }
    if has_disable_library_validation(&binary) {
        return;
    }
    let entitlements = match write_entitlements_plist() {
        Some(path) => path,
        None => return,
    };
    let jit_entitlements = match write_jit_entitlements_plist() {
        Some(path) => path,
        None => {
            let _ = std::fs::remove_file(&entitlements);
            return;
        }
    };
    let framework = app.join("Contents/Frameworks/Chromium Embedded Framework.framework");
    // Inside out: the framework's libraries, the framework, each helper, then the host.
    let libraries = framework.join("Libraries");
    if let Ok(entries) = std::fs::read_dir(&libraries) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "dylib") {
                codesign(&entry.path(), None);
            }
        }
    }
    codesign(&framework, None);
    let helpers = app.join("Contents/Frameworks");
    if let Ok(entries) = std::fs::read_dir(&helpers) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("web-host Helper") && name.ends_with(".app") {
                let ent = if name.contains("Renderer") || name.contains("GPU") {
                    &jit_entitlements
                } else {
                    &entitlements
                };
                codesign(&entry.path(), Some(ent));
            }
        }
    }
    codesign(&app, Some(&entitlements));
    let _ = std::fs::remove_file(&entitlements);
    let _ = std::fs::remove_file(&jit_entitlements);
    let ok = std::process::Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        tracing::info!("the web engine re-signed for library validation");
    } else {
        tracing::warn!("the web engine's re-signing couldn't be verified");
    }
}

#[cfg(target_os = "macos")]
fn has_disable_library_validation(binary: &Path) -> bool {
    std::process::Command::new("codesign")
        .args(["-d", "--xml", "--entitlements", "-"])
        .arg(binary)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("disable-library-validation"))
}

#[cfg(target_os = "macos")]
fn codesign(target: &Path, entitlements: Option<&Path>) {
    let mut cmd = std::process::Command::new("codesign");
    cmd.args(["--force", "--options", "runtime", "--sign", "-"]);
    if let Some(ent) = entitlements {
        cmd.arg("--entitlements").arg(ent);
    }
    cmd.arg(target).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let _ = cmd.status();
}

#[cfg(target_os = "macos")]
fn write_entitlements_plist() -> Option<PathBuf> {
    let path = std::env::temp_dir().join(format!("web-engine-ent-{}.plist", std::process::id()));
    std::fs::write(&path, ENTITLEMENTS_PLIST).ok()?;
    Some(path)
}

#[cfg(target_os = "macos")]
fn write_jit_entitlements_plist() -> Option<PathBuf> {
    let path =
        std::env::temp_dir().join(format!("web-engine-jit-ent-{}.plist", std::process::id()));
    std::fs::write(&path, JIT_ENTITLEMENTS_PLIST).ok()?;
    Some(path)
}

#[cfg(target_os = "macos")]
const ENTITLEMENTS_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
</dict>
</plist>"#;

/// The renderer and the GPU helper compile code at run time (JavaScript, shaders).
#[cfg(target_os = "macos")]
const JIT_ENTITLEMENTS_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
    <key>com.apple.security.cs.allow-jit</key>
    <true/>
</dict>
</plist>"#;

#[cfg(not(target_os = "macos"))]
fn ensure_loadable(_dir: &Path) {}

fn unpack_error(e: std::io::Error) -> InstallError {
    InstallError::Unpack(e.to_string())
}

/// A small file (the list, its signature), at most [`SMALL`] bytes.
fn small(fetch: &dyn Fetch, url: &str) -> Result<Vec<u8>, InstallError> {
    Ok(fetch::small(fetch, Bounded { url, hosts: &HOSTS, limit: SMALL })?)
}

#[cfg(test)]
#[path = "../../tests/unit/install/mod.rs"]
mod tests;
