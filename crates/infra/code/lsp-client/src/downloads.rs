//! The language servers the app downloads when they're first needed instead of bundling
//! them: their manifest (made by `cargo xtask lsp-manifest` from `packaging/lsp.toml` and the
//! lockfiles beside it — every source pinned) and each component's install into the shared
//! folder ([`crate::store`]).
//!
//! A component is Node, PHP or a server; its recipe for a platform: the platform's archive
//! (its top folder left out, only what's kept, a universal macOS program thinned to the
//! platform's architecture), npm's packages from the lockfile each in its place in
//! `node_modules` (npm's `package/` left out, nothing run), the data files, the schemas made
//! self-contained; what the server leaves out (`prune`) never written. Everything is checked
//! against its pin before it's used, made in a temporary folder beside the final one and
//! renamed into it.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::archive::{self, Rules, Strip};
use crate::assemble::{self, MachArch};
use crate::fetch::{self, Fetch, FetchError, Pin};
use crate::store::{self, Found, Marker, path_in, safe};

/// The manifest's format this app reads.
pub const FORMAT: u32 = 1;

/// The most an npm package or a data file may weigh.
pub const MAX_PACKAGE: u64 = 200 * 1024 * 1024;

/// What can be downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    #[serde(default, rename = "component")]
    pub components: Vec<Component>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        if manifest.format != FORMAT {
            return Err(format!("format {} (this app reads {FORMAT})", manifest.format));
        }
        for c in &manifest.components {
            c.check()?;
        }
        Ok(manifest)
    }

    pub fn named(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }

    pub fn of(&self, role: Role) -> Option<&Component> {
        self.components.iter().find(|c| c.role == role)
    }
}

/// What a component is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Node, for the Node servers.
    Node,
    /// PHP, for the PHP servers.
    Php,
    #[default]
    Server,
}

/// Node, PHP or a server, and how it's put together.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Component {
    pub name: String,
    #[serde(default = "server_role")]
    pub role: Role,
    pub version: String,
    #[serde(default)]
    pub license: String,
    /// The platform's archive or program, by platform (none: the same files everywhere).
    #[serde(default)]
    pub platforms: BTreeMap<String, Archive>,
    /// npm's packages, from the lockfile.
    #[serde(default)]
    pub packages: Vec<Package>,
    /// Data files (schemas, a PHAR, licences).
    #[serde(default)]
    pub files: Vec<DataFile>,
    /// What's left out (paths relative to its folder, `*` any one name).
    #[serde(default)]
    pub prune: Vec<String>,
    /// Universal macOS programs thinned to the platform's architecture.
    #[serde(default)]
    pub universal: Vec<String>,
    /// Schemas made self-contained.
    #[serde(default)]
    pub inline: Vec<Inline>,
}

fn server_role() -> Role {
    Role::Server
}

/// A platform's archive (or one program).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Archive {
    pub url: String,
    pub sha256: String,
    /// Bytes downloaded.
    #[serde(default)]
    pub size: u64,
    /// Its top folder, left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip: Option<String>,
    /// Only these files kept (after `strip`); empty: everything.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keep: Vec<String>,
    /// A single gzipped program's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// A runtime's program in its folder (`bin/node`, `php.exe`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
}

/// An npm package from a lockfile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Package {
    /// Its place in the component's folder (`node_modules/a/node_modules/b`).
    pub path: String,
    pub url: String,
    pub integrity: String,
    #[serde(default)]
    pub size: u64,
    /// The systems and processors it's only for (npm's names), when it says.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cpu: Vec<String>,
}

/// A data file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DataFile {
    pub url: String,
    pub sha256: String,
    pub path: String,
    #[serde(default)]
    pub size: u64,
}

/// A schema made self-contained: what it refers to by URL put inside it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Inline {
    pub schema: String,
    pub out: String,
    /// A URL prefix and the folder its schemas are in.
    pub urls: BTreeMap<String, String>,
}

/// npm's name of `platform`'s system and processor.
fn npm_platform(platform: &str) -> (&'static str, &'static str) {
    let os = match platform.split('-').next() {
        Some("macos") => "darwin",
        Some("windows") => "win32",
        _ => "linux",
    };
    let cpu = if platform.ends_with("arm64") { "arm64" } else { "x64" };
    (os, cpu)
}

impl Package {
    /// Whether it's installed on `platform` (npm's rule: listed, or `!`-excluded).
    pub fn applies(&self, platform: &str) -> bool {
        let (os, cpu) = npm_platform(platform);
        let fits = |list: &[String], value: &str| {
            if list.is_empty() {
                return true;
            }
            if list.iter().any(|v| v.strip_prefix('!') == Some(value)) {
                return false;
            }
            list.iter().all(|v| v.starts_with('!')) || list.iter().any(|v| v == value)
        };
        fits(&self.os, os) && fits(&self.cpu, cpu)
    }
}

impl Component {
    /// Its folder relative to the store's (and the bundle's): `node/<v>`, `php/<v>`,
    /// `servers/<name>/<v>`.
    pub fn folder(&self) -> String {
        format!("{}/{}", self.group(), safe(&self.version))
    }

    /// The folder its versions are in.
    pub fn group(&self) -> String {
        match self.role {
            Role::Node => "node".to_owned(),
            Role::Php => "php".to_owned(),
            Role::Server => format!("servers/{}", safe(&self.name)),
        }
    }

    /// How it's called to the user.
    pub fn label(&self) -> String {
        match self.role {
            Role::Node => "Node.js".to_owned(),
            Role::Php => "PHP".to_owned(),
            Role::Server => self.name.clone(),
        }
    }

    /// Whether it can be installed on `platform`.
    pub fn available(&self, platform: &str) -> bool {
        self.platforms.is_empty() || self.platforms.contains_key(platform)
    }

    /// What's downloaded for it on `platform` (bytes).
    pub fn size(&self, platform: &str) -> u64 {
        self.platforms.get(platform).map_or(0, |a| a.size)
            + self.packages.iter().filter(|p| p.applies(platform)).map(|p| p.size).sum::<u64>()
            + self.files.iter().map(|f| f.size).sum::<u64>()
    }

    /// A runtime's program in its folder on `platform`.
    pub fn bin(&self, platform: &str) -> Option<&str> {
        self.platforms.get(platform)?.bin.as_deref()
    }

    /// Its recipe on `platform`, as a short fingerprint: another recipe of the same version
    /// (a schema more, a different prune) is made again.
    pub fn fingerprint(&self, platform: &str) -> String {
        #[derive(Serialize)]
        struct Recipe<'a> {
            name: &'a str,
            version: &'a str,
            archive: Option<&'a Archive>,
            packages: Vec<&'a Package>,
            files: &'a [DataFile],
            prune: &'a [String],
            universal: &'a [String],
            inline: &'a [Inline],
        }
        let recipe = Recipe {
            name: &self.name,
            version: &self.version,
            archive: self.platforms.get(platform),
            packages: self.packages.iter().filter(|p| p.applies(platform)).collect(),
            files: &self.files,
            prune: &self.prune,
            universal: &self.universal,
            inline: &self.inline,
        };
        let json = serde_json::to_vec(&recipe).unwrap_or_default();
        crate::install::hex(ring::digest::digest(&ring::digest::SHA256, &json).as_ref())[..16]
            .to_owned()
    }

    /// Where it's installed under `root`, if it is (see [`store::find`]).
    pub fn find(&self, root: &Path, platform: &str) -> Option<Found> {
        store::find(root, &self.group(), &self.name, &self.version, &self.fingerprint(platform))
    }

    /// Its paths stay inside its folder; its sources are the allowed ones.
    fn check(&self) -> Result<(), String> {
        let bad = |what: &str| Err(format!("{}: {what}", self.name));
        if self.name.is_empty()
            || safe(&self.name) != self.name
            || safe(&self.version) != self.version
        {
            return bad("not a plain name and version");
        }
        for archive in self.platforms.values() {
            if !fetch::allowed(&archive.url) {
                return bad(&format!("{} isn't an allowed source", archive.url));
            }
            let paths = archive.keep.iter().chain(&archive.program).chain(&archive.bin);
            if !paths.chain(&archive.strip).all(|p| assemble::safe_relative(p)) {
                return bad("a path outside its folder");
            }
        }
        for p in &self.packages {
            if !p.url.starts_with(crate::install::REGISTRY) {
                return bad(&format!("{}: npm's packages come only from its registry", p.url));
            }
            if !assemble::safe_relative(&p.path) || !p.path.starts_with("node_modules/") {
                return bad(&format!("{}: not a place in node_modules", p.path));
            }
        }
        for f in &self.files {
            if !fetch::allowed(&f.url) {
                return bad(&format!("{} isn't an allowed source", f.url));
            }
            if !assemble::safe_relative(&f.path) {
                return bad(&format!("{}: a path outside its folder", f.path));
            }
        }
        let inlined = self.inline.iter().flat_map(|i| {
            [i.schema.as_str(), i.out.as_str()]
                .into_iter()
                .chain(i.urls.values().map(String::as_str))
        });
        if !self.universal.iter().map(String::as_str).chain(inlined).all(assemble::safe_relative) {
            return bad("a path outside its folder");
        }
        Ok(())
    }
}

/// The macOS architecture of `platform` (`None`: not macOS).
fn macos_arch(platform: &str) -> Option<MachArch> {
    match platform {
        "macos-arm64" => Some(MachArch::Arm64),
        "macos-x64" => Some(MachArch::X86_64),
        _ => None,
    }
}

/// The file name at the end of `url`.
fn file_name(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().unwrap_or(path)
}

/// Installs component `c` for `platform` under `root` from `fetch` (see the module's doc):
/// `progress(received)` across all its downloads, `waiting()` once if another process is
/// installing it (it's then taken as it left it), `stop` ends it (what's downloaded of a
/// large archive kept, to be resumed).
pub fn install(
    root: &Path,
    c: &Component,
    platform: &str,
    fetch: &dyn Fetch,
    progress: &dyn Fn(u64),
    waiting: &dyn Fn(),
    stop: &AtomicBool,
) -> Result<Found, FetchError> {
    let failed = |e: String| FetchError::Failed(e);
    if !c.available(platform) {
        return Err(failed(format!("{} isn't made for {platform}", c.name)));
    }
    let _lock = store::Lock::acquire(root, &c.name, stop, waiting)
        .map_err(|e| if e == "stopped" { FetchError::Stopped } else { failed(e) })?;
    // Another window, another app, made it meanwhile.
    if let Some(found) = c.find(root, platform).filter(|f| f.current) {
        progress(c.size(platform));
        return Ok(found);
    }
    let target = path_in(root, &c.folder());
    let parent = target.parent().ok_or_else(|| failed("no folder".into()))?.to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|e| failed(format!("{}: {e}", parent.display())))?;
    let staging = parent.join(format!(".tmp-{}-{:032x}", safe(&c.version), kernel::id::new_uuid()));
    let part = c
        .platforms
        .get(platform)
        .map(|a| root.join(".downloads").join(format!("{}.part", safe(&a.sha256))));
    let made = assemble_into(&staging, c, platform, fetch, part.as_deref(), progress, stop)
        .and_then(|size| {
            if target.exists() {
                // Another recipe of the version: set aside, then removed.
                let old = parent.join(format!(".old-{:032x}", kernel::id::new_uuid()));
                std::fs::rename(&target, &old)
                    .map_err(|e| failed(format!("{}: {e}", target.display())))?;
                let _ = store::remove_dir(&old);
            }
            std::fs::rename(&staging, &target)
                .map_err(|e| failed(format!("{}: {e}", target.display())))?;
            Ok(size)
        });
    match made {
        Ok(size) => {
            if let Some(part) = &part {
                let _ = std::fs::remove_file(part);
            }
            Ok(Found { dir: target, version: c.version.clone(), size, current: true })
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

/// Component `c`'s files made in `staging` (its archive downloaded into `part`): their size.
fn assemble_into(
    staging: &Path,
    c: &Component,
    platform: &str,
    fetch: &dyn Fetch,
    part: Option<&Path>,
    progress: &dyn Fn(u64),
    stop: &AtomicBool,
) -> Result<u64, FetchError> {
    let failed = |e: String| FetchError::Failed(e);
    let unpacked = |e: String| if e == "stopped" { FetchError::Stopped } else { failed(e) };
    std::fs::create_dir_all(staging).map_err(|e| failed(format!("{}: {e}", staging.display())))?;
    let mut before = 0u64;
    // The platform's archive (or program).
    if let (Some(archive), Some(part)) = (c.platforms.get(platform), part) {
        let limit =
            if archive.size > 0 { archive.size + archive.size / 2 + (1 << 20) } else { 1 << 30 };
        fetch::fetch_file(
            fetch,
            &archive.url,
            part,
            &Pin::Sha256(archive.sha256.clone()),
            limit,
            &|got, _| progress(got),
            stop,
        )?;
        before += std::fs::metadata(part).map_or(archive.size, |m| m.len());
        let rules = Rules {
            strip: archive.strip.as_deref().map_or(Strip::None, Strip::Folder),
            keep: &archive.keep,
            prune: &c.prune,
            prefix: "",
            filter: None,
        };
        archive::unpack_path(
            part,
            file_name(&archive.url),
            staging,
            &rules,
            archive.program.as_deref(),
            stop,
        )
        .map_err(unpacked)?;
        if let Some(arch) = macos_arch(platform) {
            for program in &c.universal {
                thin_in_place(&path_in(staging, program), arch).map_err(failed)?;
            }
        }
    }
    // npm's packages, each in its place.
    for package in c.packages.iter().filter(|p| p.applies(platform)) {
        let at = before;
        let bytes = fetch::fetch_bytes(
            fetch,
            &package.url,
            &Pin::Integrity(package.integrity.clone()),
            MAX_PACKAGE,
            &|got, _| progress(at + got),
            stop,
        )?;
        before += bytes.len() as u64;
        let rules = Rules {
            strip: Strip::First,
            prune: &c.prune,
            prefix: &package.path,
            ..Rules::default()
        };
        archive::unpack_tar(flate2::read::GzDecoder::new(&bytes[..]), staging, &rules, stop)
            .map_err(|e| unpacked(format!("{}: {e}", package.path)))?;
    }
    // Its data files.
    for file in &c.files {
        let at = before;
        let bytes = fetch::fetch_bytes(
            fetch,
            &file.url,
            &Pin::Sha256(file.sha256.clone()),
            MAX_PACKAGE,
            &|got, _| progress(at + got),
            stop,
        )?;
        before += bytes.len() as u64;
        if pinned_downloads::paths::pruned_any(&c.prune, &file.path) {
            continue;
        }
        let path = path_in(staging, &file.path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
        }
        std::fs::write(&path, &bytes).map_err(|e| failed(format!("{}: {e}", path.display())))?;
    }
    // Schemas made self-contained, their parts left out afterwards.
    let mut parts = Vec::new();
    for inline in &c.inline {
        let read = |path: &Path| -> Option<serde_json::Value> {
            serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
        };
        let root_schema = read(&path_in(staging, &inline.schema))
            .ok_or_else(|| failed(format!("{}: not JSON", inline.schema)))?;
        let load = |url: &str| {
            let (dir, rest) = inline
                .urls
                .iter()
                .find_map(|(prefix, dir)| Some((dir, url.strip_prefix(prefix.as_str())?)))?;
            assemble::safe_relative(rest).then(|| read(&path_in(&path_in(staging, dir), rest)))?
        };
        let made = assemble::inline_schema(root_schema, &load)
            .map_err(|e| failed(format!("{}: {e}", inline.schema)))?;
        let bytes = serde_json::to_vec(&made).map_err(|e| failed(e.to_string()))?;
        std::fs::write(path_in(staging, &inline.out), bytes)
            .map_err(|e| failed(format!("{}: {e}", inline.out)))?;
        parts.extend(inline.urls.values().cloned());
    }
    for dir in parts {
        let dir = path_in(staging, &dir);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
        }
    }
    let size = store::dir_size(staging);
    let marker = Marker {
        name: c.name.clone(),
        version: c.version.clone(),
        fingerprint: c.fingerprint(platform),
        size,
        platform: platform.to_owned(),
    };
    store::write_marker(staging, &marker).map_err(failed)?;
    Ok(size)
}

/// The universal program at `path` thinned to `arch`, in place (a thin one left as it is).
fn thin_in_place(path: &Path, arch: MachArch) -> Result<(), String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(slice) =
        assemble::thin(&bytes, arch).map_err(|e| format!("{}: {e}", path.display()))?
    else {
        return Ok(());
    };
    let permissions = std::fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    std::fs::write(path, slice).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, permissions).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/downloads.rs"]
mod tests;
