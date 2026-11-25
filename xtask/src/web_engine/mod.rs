//! The web engine's packages (plan 99b): the host built and bundled for a system made into
//! the archive the app downloads, and the signed list of them it reads.
//!
//! - `web-engine-trim --bundle <dir>`: Chromium's languages but [`KEPT_LANGUAGES`] removed
//!   from the bundle, before it's signed (the signature then seals what's shipped).
//! - `web-engine-package --bundle <dir> --platform <name> [--out <dir>]`: the bundle (what
//!   CEF's bundling made: `web-host.app` on macOS, the program and CEF's files elsewhere)
//!   packed by our own code ([`tar`]), refused while it holds languages but ours (it's
//!   trimmed before it's signed), `engine.json` beside
//!   it (what the host says of itself: `web-host --version`), as
//!   `web-engine-<platform>-<version>.tar.xz` with a `<archive>.json` saying what it is.
//! - `web-engine-manifest --dir <dir> --base-url <url> [--version <v>] [--previous <file>]
//!   [--minimum <version>] [--relist] [--signed]`: the archives' descriptions there merged into the
//!   published list (`--previous`, its signature checked) as `web-engine.json`, signed with
//!   the release key (`RELEASE_SIGNING_KEY`) into `web-engine.json.sig`, checked against the
//!   project's public key (the one built into the app) before it's written. A description
//!   the app would refuse (`web_engine_versions::well_formed`) is refused here. With
//!   `--relist`, no description: the published list again with a new `--minimum` (one every
//!   system still meets), signed anew — nothing rebuilt, no archive touched.
//! - `web-engine-verify --dir <dir> --version <v> [--published <file>]`: before publishing,
//!   the list's signature is the project's, every system has a package of `<v>` whose
//!   archive is there with the list's size and SHA-256, and nothing published now is dropped.

pub mod dto;
pub mod list;
pub mod tar;

use std::path::{Path, PathBuf};
use std::process::Command;

use dto::{Description, List, Version};
use tar::{Body, Entry};
use web_engine_protocol::EngineInfo;

/// The languages kept of Chromium's own (its error pages, its dialogs): ours, and the one
/// it falls back to.
const KEPT_LANGUAGES: [&str; 3] = ["en", "en-US", "ro"];

/// The program in a bundle made for `platform`, where the app finds it once unpacked
/// (`web_engine_versions::program`); `None` for a system no engine is made for.
fn program(bundle: &Path, platform: &str) -> Option<PathBuf> {
    let made_for = web_engine_versions::Platform::named(platform)?;
    Some(web_engine_versions::program(bundle, made_for.os))
}

/// Where Chromium keeps its languages: the macOS framework's `Resources` (a folder each,
/// beside files that aren't languages), or a `locales` folder (a `.pak` each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Languages {
    None,
    Folders,
    Files,
}

/// Whether `name`, in a folder holding `languages`, is kept: anything not a language, or
/// one of ours.
fn kept_language(name: &str, languages: Languages) -> bool {
    let language = match languages {
        Languages::None => None,
        Languages::Folders => name.strip_suffix(".lproj"),
        Languages::Files => name.strip_suffix(".pak"),
    };
    language.is_none_or(|l| KEPT_LANGUAGES.contains(&l))
}

/// What [`walk`] found in a bundle: its path on disk and in the archive (`/`-separated),
/// whether it's a folder, and whether it's one of Chromium's languages but ours.
struct Found<'a> {
    path: &'a Path,
    inside: &'a str,
    folder: bool,
    foreign: bool,
}

/// Every entry of `from` (at `prefix` in the archive) shown to `visit`, folders before what
/// they hold; the system's own files ([`tar::system_file`]) passed over, and a language not
/// ours shown but not entered. Links are refused: the app's unpacking writes none. The one
/// walk the trimming and the packing share, so they can't disagree on what a language is.
fn walk(
    from: &Path,
    prefix: &str,
    languages: Languages,
    visit: &mut dyn FnMut(&Found<'_>) -> Result<(), String>,
) -> Result<(), String> {
    let listed = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in listed {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_symlink() {
            return Err(format!("{}: a link (the app writes none)", path.display()));
        }
        if tar::system_file(&name) {
            continue;
        }
        let inside = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let foreign = !kept_language(&name, languages);
        visit(&Found { path: &path, inside: &inside, folder: kind.is_dir(), foreign })?;
        if kind.is_dir() && !foreign {
            let held = match name.as_str() {
                "Resources" => Languages::Folders,
                "locales" => Languages::Files,
                _ => Languages::None,
            };
            walk(&path, &inside, held, visit)?;
        }
    }
    Ok(())
}

/// The entries of `bundle`, for its archive. A bundle still holding Chromium's languages but
/// ours is refused: it's trimmed (`web-engine-trim`) before it's signed, so packing it as it
/// is would ship what the signature doesn't seal, and leaving them out here would break it.
fn collect(bundle: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    walk(bundle, "", Languages::None, &mut |found| {
        if found.foreign {
            return Err(format!(
                "{}: a language not ours: trim the bundle first (web-engine-trim)",
                found.path.display()
            ));
        }
        let path = found.inside.to_owned();
        let body = if found.folder {
            Body::Folder
        } else {
            let source = found.path.to_path_buf();
            let exec =
                tar::executable(&source).map_err(|e| format!("{}: {e}", source.display()))?;
            Body::File { source, exec }
        };
        entries.push(Entry { path, body });
        Ok(())
    })?;
    Ok(entries)
}

/// `web-engine-trim`: `bundle`'s languages but ours removed. What was done.
pub fn trim(bundle: &Path) -> Result<String, String> {
    let mut removed = 0usize;
    walk(bundle, "", Languages::None, &mut |found| {
        if !found.foreign {
            return Ok(());
        }
        let gone = if found.folder {
            std::fs::remove_dir_all(found.path)
        } else {
            std::fs::remove_file(found.path)
        };
        gone.map_err(|e| format!("{}: {e}", found.path.display()))?;
        removed += 1;
        Ok(())
    })?;
    Ok(format!("{removed} languages removed from {}", bundle.display()))
}

/// The archive of `bundle` with `engine` (what its host said of itself) as `engine.json`.
fn entries_of(bundle: &Path, engine: &[u8]) -> Result<Vec<Entry>, String> {
    let mut entries = collect(bundle)?;
    entries.push(Entry { path: "engine.json".into(), body: Body::Bytes(engine.to_vec()) });
    Ok(entries)
}

/// The most of a program's error output quoted when it fails, in characters.
const QUOTED_ERRORS: usize = 4000;

/// What a run of `web-host --version` said: its JSON's text, the engine it names. It must
/// have ended well and said an engine's version (its error output quoted otherwise).
fn engine_from(
    program: &Path,
    said: &std::process::Output,
) -> Result<(Vec<u8>, EngineInfo), String> {
    let errors = String::from_utf8_lossy(&said.stderr);
    let errors = errors.trim();
    let quoted: String = errors.chars().take(QUOTED_ERRORS).collect();
    if !said.status.success() {
        return Err(format!("{} --version: {}: {quoted}", program.display(), said.status));
    }
    let engine: EngineInfo = serde_json::from_slice(&said.stdout)
        .map_err(|e| format!("{} --version: {e} {quoted}", program.display()))?;
    if Version::parse(&engine.version).is_none() {
        return Err(format!("{}: not an engine's version", engine.version));
    }
    Ok((said.stdout.trim_ascii().to_vec(), engine))
}

/// What the host says of itself (`web-host --version`): see [`engine_from`].
fn engine_said(program: &Path) -> Result<(Vec<u8>, EngineInfo), String> {
    let said = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    engine_from(program, &said)
}

/// `web-engine-package`: the archive and its description, in `out`. What was made.
pub fn package(bundle: &Path, platform: &str, out: &Path) -> Result<String, String> {
    let program = program(bundle, platform)
        .ok_or_else(|| format!("{platform}: not a system an engine is made for"))?;
    let (engine, EngineInfo { version, protocol }) = engine_said(&program)?;
    let file = format!("web-engine-{platform}-{version}.tar.xz");
    let mut entries = entries_of(bundle, &engine)?;
    let packed = tar::pack(&mut entries, &out.join(&file))?;
    let described = Description {
        version: version.clone(),
        protocol,
        platform: platform.to_owned(),
        name: file.clone(),
        size: packed.size,
        sha256: packed.sha256,
    };
    let text = serde_json::to_string(&described).map_err(|e| e.to_string())?;
    std::fs::write(out.join(format!("{file}.json")), text).map_err(|e| e.to_string())?;
    Ok(format!(
        "{file} ({} MB, {} entries), version {version}, protocol {protocol}",
        packed.size / 1_000_000,
        entries.len()
    ))
}

/// What `web-engine-manifest` is asked.
#[derive(Debug)]
pub struct ManifestAsk<'a> {
    /// The project's public key (hex): the one built into the app
    /// (`pinned_downloads::signature::PUBLIC_KEY`).
    pub public: &'a str,
    /// The folder of the new packages' descriptions (`*.tar.xz.json`), where the list and
    /// its signature are written.
    pub dir: &'a Path,
    /// Where the packages are downloaded from: an archive's address is `<base_url>/<name>`
    /// (the rolling `web-engine` release).
    pub base_url: &'a str,
    /// Every description must be of this version (the release's tag).
    pub version: Option<&'a str>,
    /// The published list (its signature beside it), merged with the new packages.
    pub previous: Option<&'a Path>,
    /// The oldest version still safe; `None`: the previous list's.
    pub minimum: Option<&'a str>,
    /// Only the published list again, with a new [`minimum`](Self::minimum): no package's
    /// description read, nothing built (`--relist`). Needs `previous` and `minimum`; every
    /// system must keep a package at least as new as the minimum.
    pub relist: bool,
    /// The release key's secret (hex of its Ed25519 seed, `RELEASE_SIGNING_KEY`); `None` or
    /// empty: the list is written unsigned (refused when [`signed`](Self::signed)).
    pub secret: Option<&'a str>,
    /// A signature is required.
    pub signed: bool,
}

/// The descriptions (`*.tar.xz.json`) in `dir`, sorted by name.
fn descriptions(dir: &Path) -> Result<Vec<Description>, String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tar.xz.json"))
        .collect();
    names.sort();
    names
        .iter()
        .map(|name| {
            let text = std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
            serde_json::from_slice(&text).map_err(|e| format!("{name}: {e}"))
        })
        .collect()
}

/// Every system has a package of `list` at least as new as its minimum: raising it past what
/// a system has would leave that system without an engine.
fn met_everywhere(list: &List) -> Result<(), String> {
    let Some(minimum) = list.minimum.as_deref().and_then(Version::parse) else {
        return Ok(());
    };
    for platform in list::platforms() {
        let met = list
            .packages
            .iter()
            .filter(|p| p.platform == platform)
            .any(|p| Version::parse(&p.version).is_some_and(|v| v >= minimum));
        if !met {
            return Err(format!("no {platform} package meets the minimum: no engine would do"));
        }
    }
    Ok(())
}

/// `web-engine-manifest`: `web-engine.json` (and its signature) in `dir`. What was listed.
pub fn manifest(ask: &ManifestAsk<'_>) -> Result<String, String> {
    if ask.relist && (ask.previous.is_none() || ask.minimum.is_none()) {
        return Err("--relist lists the published list (--previous) with a new --minimum".into());
    }
    let described = if ask.relist { Vec::new() } else { descriptions(ask.dir)? };
    if described.is_empty() && !ask.relist {
        return Err(format!("no web-engine-*.tar.xz.json in {}", ask.dir.display()));
    }
    if let Some(version) = ask.version
        && let Some(other) = described.iter().find(|d| d.version != version)
    {
        return Err(format!("{}: version {}, the release is {version}", other.name, other.version));
    }
    if let Some(minimum) = ask.minimum {
        let Some(oldest) = Version::parse(minimum) else {
            return Err(format!("{minimum}: not an engine's version"));
        };
        let release = ask.version.and_then(Version::parse);
        if release.is_some_and(|release| oldest > release) {
            return Err(format!(
                "minimum {minimum} is newer than the release's version: no package would do"
            ));
        }
    }
    let previous = match ask.previous {
        Some(path) => list::read_signed(path, ask.public)?,
        None => List::default(),
    };
    let new =
        described.iter().map(|d| list::listed(d, ask.base_url)).collect::<Result<Vec<_>, _>>()?;
    let made = List {
        minimum: ask.minimum.map(str::to_owned).or(previous.minimum.clone()),
        packages: list::merged(&previous.packages, &new),
    };
    if let Some(minimum) = made.minimum.as_deref().filter(|m| Version::parse(m).is_none()) {
        return Err(format!("{minimum}: not an engine's version"));
    }
    if ask.relist {
        met_everywhere(&made)?;
    }
    let text = list::text(&made)?;
    std::fs::write(ask.dir.join(list::LIST), &text).map_err(|e| e.to_string())?;
    match ask.secret.map(str::trim).filter(|s| !s.is_empty()) {
        Some(secret) => {
            let signature = crate::release::sign(&text, secret)?;
            if !crate::release::verify(&text, &signature, ask.public) {
                return Err("the signing key isn't the project's".into());
            }
            std::fs::write(ask.dir.join(list::SIGNATURE), format!("{signature}\n"))
                .map_err(|e| e.to_string())?;
        }
        None if ask.signed => return Err("RELEASE_SIGNING_KEY isn't set".into()),
        None => {}
    }
    Ok(made
        .packages
        .iter()
        .map(|p| format!("{}  protocol {}  {}", p.platform, p.protocol, p.version))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// `web-engine-verify`: the list in `dir` signed by the project, holding a package of
/// `version` for every system, each archive there as the list says; and, given the list
/// published now (`published`, its signature checked), keeping every package of it or a
/// newer one (a draft made before another was published would drop it). What was checked.
pub fn verify(
    public: &str,
    dir: &Path,
    version: &str,
    published: Option<&Path>,
) -> Result<String, String> {
    let draft = list::read_signed(&dir.join(list::LIST), public)?;
    let mut checked = list::check_release(&draft, dir, version, &list::platforms())?;
    if let Some(published) = published {
        let now = list::read_signed(published, public)?;
        if list::merged(&now.packages, &draft.packages) != draft.packages {
            return Err("the list drops or downgrades a package published since it was made: \
                 make the draft again"
                .into());
        }
        checked.push(format!("keeps the {} packages published", now.packages.len()));
    }
    Ok(checked.join("\n"))
}

#[cfg(test)]
#[path = "../../tests/unit/web_engine/mod.rs"]
mod tests;
