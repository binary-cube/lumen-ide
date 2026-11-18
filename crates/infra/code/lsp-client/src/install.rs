//! Servers that aren't bundled (their licence forbids it: intelephense), installed by the
//! user from the settings, once for every project, in the shared folder of the downloaded
//! servers ([`crate::store`]):
//!
//! ```text
//! <installs>/<server>/<version>/   the npm package's files (its `package/`)
//! ```
//!
//! The package's tarball comes from registry.npmjs.org only, at the version its description
//! pins, and is checked against the `integrity` (SHA-512) pinned beside it before anything
//! is unpacked; it's unpacked in a temporary folder beside the final one and moved there at
//! once (an interrupted install leaves nothing half there). No npm, no scripts: the packages
//! installed this way are whole in their published files (intelephense's `lib/` is one
//! bundle needing only Node's own modules).
//!
//! An install the user made already is found too ([`find_existing`]): the program in PATH
//! (npm's global `bin`, its link followed to the package) or npm's global folder
//! (`npm root -g`).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub use crate::fetch::{Fetch, Http};
use crate::fetch::{FetchError, Pin};
use crate::spec::Install;

/// The only place packages are downloaded from.
pub const REGISTRY: &str = "https://registry.npmjs.org/";

/// The most a tarball may weigh (a server sending on forever fills no memory).
pub const MAX_TARBALL: u64 = 200 * 1024 * 1024;

/// A server's package found installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The package's folder (`${server}`).
    pub dir: PathBuf,
    pub version: String,
    /// Installed by the app (in its folder: it can uninstall it); otherwise found where the
    /// user installed it.
    pub ours: bool,
}

/// Where the tarball of `install`'s package is on the registry.
pub fn tarball_url(install: &Install) -> String {
    // `@scope/name` → `@scope/name/-/name-<v>.tgz`.
    let base = install.npm.rsplit('/').next().unwrap_or(&install.npm);
    format!("{REGISTRY}{}/-/{base}-{}.tgz", install.npm, install.version)
}

/// Whether `bytes` are what `integrity` (`sha512-<base64>`) says.
pub fn verify(bytes: &[u8], integrity: &str) -> bool {
    Pin::Integrity(integrity.to_owned()).verify(bytes)
}

pub use pinned_downloads::digest::{base64, hex};

/// The folder of `name`'s install at `version` under `installs`.
pub fn version_dir(installs: &Path, name: &str, version: &str) -> PathBuf {
    installs.join(safe(name)).join(safe(version))
}

use crate::store::{safe, version_key};

/// Server `name`'s package installed by the app under `installs`, at its pinned version (an
/// older one left by an earlier version of the app counts too: it still runs).
pub fn installed_in(installs: &Path, name: &str, install: &Install) -> Option<Installed> {
    let pinned = version_dir(installs, name, &install.version);
    if let Some(found) = package_at(&pinned, install) {
        return Some(Installed { ours: true, ..found });
    }
    let mut others: Vec<Installed> = std::fs::read_dir(installs.join(safe(name)))
        .ok()?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| package_at(&e.path(), install))
        .collect();
    others.sort_by_key(|i| std::cmp::Reverse(version_key(&i.version)));
    others.into_iter().next().map(|found| Installed { ours: true, ..found })
}

/// The package `install` names at folder `dir`, if it's whole there (its `package.json`
/// names it, its entry is there).
pub fn package_at(dir: &Path, install: &Install) -> Option<Installed> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    if json.get("name").and_then(serde_json::Value::as_str) != Some(install.npm.as_str()) {
        return None;
    }
    if !dir.join(&install.entry).is_file() {
        return None;
    }
    let version = json.get("version").and_then(serde_json::Value::as_str)?.to_owned();
    Some(Installed { dir: dir.to_path_buf(), version, ours: false })
}

/// Where the user installed `install`'s package already, if anywhere: its program in PATH
/// (its link followed to the package; npm's `.cmd` shim on Windows, the package beside it),
/// else npm's global folder (`npm_root`, `npm root -g`'s answer, when it was asked).
pub fn find_existing(install: &Install, npm_root: Option<&Path>) -> Option<Installed> {
    let from_path = install.program.as_deref().and_then(process::which).and_then(|program| {
        // npm on Windows: `<prefix>\intelephense.cmd` and `<prefix>\node_modules\<package>`.
        let beside = program.parent().map(|d| d.join("node_modules").join(&install.npm));
        // Unix: `<prefix>/bin/intelephense` → `../lib/node_modules/<package>/lib/…`.
        let real = std::fs::canonicalize(&program).ok();
        let above = real.iter().flat_map(|r| r.ancestors().skip(1)).map(Path::to_path_buf);
        let lib = program
            .parent()
            .and_then(Path::parent)
            .map(|prefix| prefix.join("lib").join("node_modules").join(&install.npm));
        beside.into_iter().chain(lib).chain(above).find_map(|dir| package_at(&dir, install))
    });
    from_path.or_else(|| npm_root.and_then(|root| package_at(&root.join(&install.npm), install)))
}

/// npm's global folder (`npm root -g`), with the system's npm, if it has one (a few hundred
/// milliseconds: never on the UI thread).
pub fn npm_global_root() -> Option<PathBuf> {
    process::which("npm")?;
    let run = process::Run::shell("npm root -g")
        .owner("lsp-install")
        .timeout(std::time::Duration::from_secs(10));
    let output = process::run(&run).ok()?;
    if !output.success() {
        return None;
    }
    let line = output.stdout.lines().next()?.trim().to_owned();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// How an install goes, as it's told.
pub trait Progress: Sync {
    /// `received` bytes of `total` downloaded.
    fn downloaded(&self, received: u64, total: Option<u64>);
}

impl<F: Fn(u64, Option<u64>) + Sync> Progress for F {
    fn downloaded(&self, received: u64, total: Option<u64>) {
        self(received, total);
    }
}

/// Installs server `name`'s package (`install`) under `installs`, from `fetch`: downloaded,
/// checked, unpacked beside its folder and moved there, under its lock (another window or
/// app installing it meanwhile: waited for). `stop` ends it (nothing left).
pub fn install(
    installs: &Path,
    name: &str,
    install: &Install,
    fetch: &dyn Fetch,
    progress: &dyn Progress,
    stop: &AtomicBool,
) -> Result<Installed, String> {
    let url = tarball_url(install);
    // Its lock among the installs (`<installs>/.locks`).
    let _lock = crate::store::Lock::acquire(installs, name, stop, &|| {})?;
    let target = version_dir(installs, name, &install.version);
    if let Some(found) = package_at(&target, install).filter(|f| f.version == install.version) {
        return Ok(Installed { ours: true, ..found });
    }
    let total = install.size;
    let bytes = crate::fetch::fetch_bytes(
        fetch,
        &url,
        &Pin::Integrity(install.integrity.clone()),
        MAX_TARBALL,
        &|got, said| progress.downloaded(got, total.or(said)),
        stop,
    )
    .map_err(|e| match e {
        FetchError::Mismatch(url) => {
            format!("{url}: the download doesn't match its pinned integrity")
        }
        other => other.to_string(),
    })?;
    let parent = target.parent().ok_or("no folder")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // Beside the final folder (the same disk: the move is one rename).
    let temp =
        parent.join(format!(".tmp-{}-{:032x}", safe(&install.version), kernel::id::new_uuid()));
    let unpacked = unpack(&bytes, &temp, stop).and_then(|()| {
        let marker = crate::store::Marker {
            name: name.to_owned(),
            version: install.version.clone(),
            fingerprint: install.integrity.clone(),
            size: crate::store::dir_size(&temp),
            platform: String::new(),
        };
        crate::store::write_marker(&temp, &marker)
    });
    let done = unpacked.and_then(|()| {
        if target.exists() {
            crate::store::remove_dir(&target)?;
        }
        std::fs::rename(&temp, &target).map_err(|e| e.to_string())
    });
    if let Err(e) = done {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(e);
    }
    package_at(&target, install)
        .map(|found| Installed { ours: true, ..found })
        .ok_or_else(|| format!("{}: the package has no {}", install.npm, install.entry))
}

/// Removes every install of server `name` the app made under `installs`.
pub fn uninstall(installs: &Path, name: &str) -> Result<(), String> {
    let dir = installs.join(safe(name));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

/// A gzipped tarball's files (npm's: everything under `package/`) written under `into`,
/// `package/` left out. Only files and folders: links and devices are skipped, and a path
/// that would leave `into` (absolute, `..`) refuses the whole archive.
pub fn unpack(gzipped: &[u8], into: &Path, stop: &AtomicBool) -> Result<(), String> {
    let rules = crate::archive::Rules { strip: crate::archive::Strip::First, ..Default::default() };
    crate::archive::unpack_tar(flate2::read::GzDecoder::new(gzipped), into, &rules, stop).map(drop)
}

#[cfg(test)]
#[path = "../tests/unit/install.rs"]
mod tests;
