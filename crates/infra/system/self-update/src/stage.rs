//! An update made ready: this system's file downloaded into the updates' folder (resumed if
//! it broke off, checked against the manifest's SHA-256 and size), unpacked there by our
//! own code, the new app found in it and noted (`staged.toml`) for what puts it
//! in place. Nothing of the app running is touched.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use pinned_downloads::archive::{Rules, unpack_path};
use pinned_downloads::fetch::{Fetch, Going, Pin, Wanted, fetch_file};

use crate::check::Available;

/// What putting it in place reads in a staged update's folder.
pub const STAGED: &str = "staged.toml";

/// An update ready to be put in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// Its folder (the version's, in the updates' folder).
    pub dir: PathBuf,
    pub version: String,
    /// The new app in it: `Lumen.app` on macOS, the program elsewhere.
    pub app: PathBuf,
}

/// An installer's name ends so (`…-windows-x64-setup.exe`).
pub const SETUP_SUFFIX: &str = "-setup.exe";

impl Staged {
    /// Whether it's an installer, run to update (not an app put in place).
    pub fn is_setup(&self) -> bool {
        self.app.file_name().is_some_and(|n| n.to_string_lossy().ends_with(SETUP_SUFFIX))
    }
}

/// The new app's name in what's unpacked: the bundle on macOS, the program elsewhere.
pub fn app_name() -> String {
    if cfg!(target_os = "macos") {
        format!("{}.app", kernel::brand::NAME)
    } else if cfg!(windows) {
        format!("{}.exe", kernel::brand::ID)
    } else {
        kernel::brand::ID.to_owned()
    }
}

/// `available`'s file downloaded under `root` (its version's folder) and unpacked there;
/// `progress(received, total)` as it comes, `stop` ends it (what came is kept: resumed
/// next time). The new app is looked for by `name` ([`app_name`]).
pub fn stage(
    fetch: &dyn Fetch,
    available: &Available,
    root: &Path,
    name: &str,
    progress: &dyn Fn(u64, Option<u64>),
    stop: &AtomicBool,
) -> Result<Staged, String> {
    let version = available.release.version.to_string();
    let dir = root.join(&version);
    let file = &available.file;
    let archive = dir.join(&file.name);
    let part = dir.join(format!("{}.part", file.name));
    let pin = Pin::Sha256(file.sha256.clone());
    // Other versions' downloads (skipped, failed, older) go: one at a time is kept.
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() && entry.file_name().to_string_lossy() != version {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    // Downloaded before (another run): checked again (streamed), not fetched again.
    let whole = sha256(&archive).is_some_and(|digest| pin.matches(&digest));
    if !whole {
        fetch_file(
            Going { fetch, progress, stop },
            Wanted { url: &available.url, hosts: crate::HOSTS, pin: &pin, limit: file.size },
            &part,
        )
        .map_err(String::from)?;
        std::fs::rename(&part, &archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    }
    // An installer (the app its install updates through): run as it is, nothing unpacked.
    if file.name.ends_with(SETUP_SUFFIX) {
        let staged = Staged { dir: dir.clone(), version, app: archive };
        write(&staged).map_err(|e| format!("{}: {e}", dir.join(STAGED).display()))?;
        return Ok(staged);
    }
    let unpacked = dir.join("app");
    if unpacked.exists() {
        std::fs::remove_dir_all(&unpacked).map_err(|e| format!("{}: {e}", unpacked.display()))?;
    }
    unpack_path(&archive, &file.name, &unpacked, &Rules::default(), None, stop)?;
    let app = unpacked.join(name);
    if !app.exists() {
        return Err(format!("{}: no {name} in it", file.name));
    }
    let staged = Staged { dir: dir.clone(), version, app };
    write(&staged).map_err(|e| format!("{}: {e}", dir.join(STAGED).display()))?;
    Ok(staged)
}

/// The SHA-256 of the file at `path`, read in pieces (an archive isn't held in memory).
fn sha256(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        context.update(&buffer[..n]);
    }
    Some(context.finish().as_ref().to_vec())
}

/// `staged` noted in its folder, for putting it in place.
fn write(staged: &Staged) -> std::io::Result<()> {
    let mut table = toml::Table::new();
    table.insert("version".into(), toml::Value::String(staged.version.clone()));
    let app = staged.app.strip_prefix(&staged.dir).unwrap_or(&staged.app);
    table.insert("app".into(), toml::Value::String(app.to_string_lossy().into_owned()));
    files::write_atomic(
        &staged.dir.join(STAGED),
        toml::to_string(&table).unwrap_or_default().as_bytes(),
    )
}

/// The update staged in `dir`, as noted there; `None` if it isn't one (or its app is gone).
pub fn read(dir: &Path) -> Option<Staged> {
    let text = std::fs::read_to_string(dir.join(STAGED)).ok()?;
    let table: toml::Table = text.parse().ok()?;
    let version = table.get("version")?.as_str()?.to_owned();
    let app = Path::new(table.get("app")?.as_str()?);
    // Only inside its folder (plain names, no root, drive or `..`): a note pointing
    // elsewhere is no update.
    if app.as_os_str().is_empty()
        || !app.components().all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    let app = dir.join(app);
    app.exists().then(|| Staged { dir: dir.to_path_buf(), version, app })
}

#[cfg(test)]
#[path = "../tests/unit/stage.rs"]
mod tests;
