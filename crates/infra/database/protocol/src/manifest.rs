//! How an external driver is installed: a folder of its own (under the app's config
//! folder, `drivers/<name>/`) holding its program and its manifest, `driver.toml`:
//!
//! ```toml
//! protocol = { min = 1, max = 1 }   # the protocol versions it speaks
//! command = "bin/duck-driver"        # its program: in its folder, or on PATH
//! args = ["--stdio"]
//! timeout = 600                      # seconds a request may take (optional)
//! [env]                              # added to its environment (never a password)
//! JAVA_HOME = "/opt/java"
//!
//! [driver]                           # what it is and can do (see `dto::Description`)
//! id = "duckdb"
//! name = "DuckDB"
//! icon = "hard-drive"                # one of the editor's (`ICONS`), in a tone (optional)
//! icon_tone = "yellow"
//! sql = "none"
//! [driver.capabilities]
//! edit_rows = true
//! file_based = true
//! [driver.files]
//! extensions = ["duckdb"]
//! header = "DUCK"
//! ```
//!
//! Reading the manifests starts nothing: a driver's program runs only when a session of it
//! opens. Its program is started directly (no shell; on Windows `.exe`, `.cmd` or `.bat`
//! found next to the manifest or on PATH), in its folder.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use database::{DataFiles, DbError, DriverFactory, DriverFailure};
use serde::Deserialize;

use crate::client::Transport;
use crate::driver::{Launch, ProtocolDriver, REQUEST_TIMEOUT};
use crate::dto;

/// A driver's manifest's file name.
pub const FILE: &str = "driver.toml";

/// The icons of the editor's set a driver may name for its sources (a drawing of its own
/// isn't read: nothing of a driver's folder is drawn).
pub const ICONS: &[&str] =
    &["database", "server", "hard-drive", "file-text", "table", "cloud", "globe", "layers"];

/// A manifest, as written.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Manifest {
    pub protocol: dto::Versions,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Seconds a request may take.
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub driver: dto::Description,
}

/// A driver found: its manifest and its folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub manifest: Manifest,
    pub dir: PathBuf,
}

/// A driver's id: lowercase letters, digits, `.`, `-`, `_`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// Bytes written in hexadecimal (`53514c`).
fn hex(text: &str) -> Result<Vec<u8>, String> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !text.len().is_multiple_of(2) {
        return Err("an odd number of hexadecimal digits".to_owned());
    }
    (0..text.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(&text[at..at + 2], 16)
                .map_err(|_| format!("`{}` isn't hexadecimal", &text[at..at + 2]))
        })
        .collect()
}

impl Manifest {
    /// `text` read as a manifest, checked.
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        manifest.check()?;
        Ok(manifest)
    }

    fn check(&self) -> Result<(), String> {
        let d = &self.driver;
        if !valid_id(&d.id) {
            return Err(format!("`{}` isn't a driver id (a-z, 0-9, `.`, `-`, `_`)", d.id));
        }
        if d.name.trim().is_empty() {
            return Err("the driver has no name".to_owned());
        }
        if self.command.trim().is_empty() {
            return Err("no command".to_owned());
        }
        if self.protocol.min > self.protocol.max {
            return Err("its protocol versions are backwards".to_owned());
        }
        if self.env.keys().any(|k| database::source::is_secret_option(k)) {
            return Err("a password doesn't go in a manifest's environment".to_owned());
        }
        if let Some(files) = &d.files {
            if files.extensions.is_empty() {
                return Err("files without extensions".to_owned());
            }
            if let Some(header) = &files.header_hex {
                hex(header)?;
            }
        }
        Ok(())
    }

    /// The first bytes its files start with (none: any).
    pub fn header(&self) -> Vec<u8> {
        let Some(files) = &self.driver.files else { return Vec::new() };
        match (&files.header_hex, &files.header) {
            (Some(hex_text), _) => hex(hex_text).unwrap_or_default(),
            (None, Some(text)) => text.as_bytes().to_vec(),
            (None, None) => Vec::new(),
        }
    }

    pub fn timeout(&self) -> Duration {
        self.timeout.map_or(REQUEST_TIMEOUT, Duration::from_secs)
    }
}

/// The manifest `path` (a `driver.toml`), read and checked.
pub fn read(path: &Path) -> Result<Installed, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let manifest = Manifest::parse(&text)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    Ok(Installed { manifest, dir })
}

/// The drivers installed under `dir` (each in a folder of its own with its `driver.toml`),
/// by folder name; those that can't be read, with why. A missing `dir`: none.
pub fn discover(dir: &Path) -> Vec<Result<Installed, (PathBuf, String)>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut folders: Vec<PathBuf> =
        entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    folders.sort();
    folders
        .into_iter()
        .map(|folder| folder.join(FILE))
        .filter(|file| file.is_file())
        .map(|file| read(&file).map_err(|why| (file, why)))
        .collect()
}

/// Where `command` is: in `dir` (as written, or with Windows' extensions), else on PATH,
/// else as written (the system says it's missing when it starts).
pub fn program(dir: &Path, command: &str) -> PathBuf {
    let written = Path::new(command);
    let windows = |path: &Path| -> Option<PathBuf> {
        if !cfg!(windows) || path.extension().is_some() {
            return None;
        }
        ["exe", "cmd", "bat"].iter().map(|e| path.with_extension(e)).find(|p| p.is_file())
    };
    if written.is_absolute() {
        return windows(written).unwrap_or_else(|| written.to_path_buf());
    }
    let local = dir.join(written);
    if local.is_file() {
        return local;
    }
    if let Some(found) = windows(&local) {
        return found;
    }
    // A bare name (`java`): on PATH.
    if written.components().count() == 1
        && let Some(found) = process::which(command)
    {
        return found;
    }
    local
}

/// Starts an installed driver's program, in its folder.
#[derive(Debug, Clone)]
pub struct ProgramLaunch {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub dir: PathBuf,
    pub env: Vec<(String, String)>,
}

impl ProgramLaunch {
    pub fn of(installed: &Installed) -> Self {
        let m = &installed.manifest;
        Self {
            name: m.driver.name.clone(),
            program: program(&installed.dir, &m.command),
            args: m.args.clone(),
            dir: installed.dir.clone(),
            env: m.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        }
    }
}

impl Launch for ProgramLaunch {
    fn launch(&self) -> Result<Transport, DbError> {
        let mut run =
            process::Run::new(self.program.as_os_str(), self.args.iter().map(OsString::from))
                .owner("driver")
                .dir(&self.dir);
        for (key, value) in &self.env {
            run = run.env(key, value);
        }
        let not_started = |why: String| {
            DbError::Driver(DriverFailure::NotStarted { driver: self.name.clone(), why })
        };
        let mut started = process::start(&run).map_err(|e| match e {
            process::RunError::NotFound(program) => not_started(format!("{program} not found")),
            other => not_started(other.to_string()),
        })?;
        let (Some(output), Some(input)) = (started.stdin.take(), started.stdout.take()) else {
            return Err(not_started("its pipes weren't given".to_owned()));
        };
        let errors = started.stderr.take().map(|e| Box::new(e) as Box<dyn std::io::Read + Send>);
        Ok(Transport {
            input: Box::new(input),
            output: Box::new(output),
            errors,
            process: Some(started),
        })
    }
}

/// The driver as the app lists it: made from its manifest (nothing started), each session
/// launched by `launch`.
pub fn factory(manifest: &Manifest, launch: Arc<dyn Launch>) -> DriverFactory {
    let description = Arc::new(manifest.driver.clone());
    let speaks = manifest.protocol;
    let timeout = manifest.timeout();
    let made = Arc::clone(&description);
    DriverFactory {
        id: description.id.clone().into(),
        name: description.name.clone().into(),
        kind: description.kind.into(),
        default_port: description.default_port,
        create: Arc::new(move || {
            Arc::new(ProtocolDriver::new(Arc::clone(&made), speaks, timeout, Arc::clone(&launch)))
        }),
        external: true,
        icon: icon(&description),
    }
}

/// The icon `description` names, if it's one of [`ICONS`], in its tone (unknown: neutral).
fn icon(description: &dto::Description) -> Option<database::DriverIcon> {
    use database::IconTone;
    let name = ICONS.iter().copied().find(|i| description.icon.as_deref() == Some(*i))?;
    let tone = match description.icon_tone.as_deref() {
        Some("red") => IconTone::Red,
        Some("green") => IconTone::Green,
        Some("yellow") => IconTone::Yellow,
        Some("blue") => IconTone::Blue,
        Some("magenta") => IconTone::Magenta,
        Some("cyan") => IconTone::Cyan,
        _ => IconTone::Neutral,
    };
    Some(database::DriverIcon { name, tone })
}

/// The files it opens by themselves, if any.
pub fn files(manifest: &Manifest) -> Option<DataFiles> {
    let files = manifest.driver.files.as_ref()?;
    Some(DataFiles::starting_with(
        manifest.driver.id.clone(),
        files.extensions.clone(),
        manifest.header(),
        files.text,
    ))
}

#[cfg(test)]
#[path = "../tests/unit/manifest.rs"]
mod tests;
