//! Project paths every context agrees on: which directories hold libraries (installed
//! dependencies), what's generated, what's metadata — not the project's own code. The
//! languages' and frameworks' folders come from their modules ([`ProjectDirs`]).

use std::path::Path;

/// The last part of `path` (a file's or a folder's name), as text; empty for a root.
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// Version control's and other editors' metadata: never the project's files.
pub const METADATA_DIRS: &[&str] = &[".git", ".hg", ".svn", ".idea", ".vscode"];

/// The folders a language or framework puts in a project (contributed by its module,
/// registered at startup with [`register`]): what isn't the project's own code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectDirs {
    /// Installed dependencies (Composer's `vendor`, npm's `node_modules`): searched less
    /// eagerly, ranked after the project's own files, never renamed.
    pub dependencies: &'static [&'static str],
    /// Not listed at all (npm's packages: very many files nothing resolves into).
    pub unlisted: &'static [&'static str],
    /// What the code generates (build output, runtime data and caches; relative to the root
    /// or to a folder): not searched as text.
    pub generated: &'static [&'static str],
    /// How the dependencies are found where their manager can put them elsewhere (Composer's
    /// `config.vendor-dir`): read from the project when it opens ([`set_found`]).
    pub found_by: &'static [DependencyFinder],
}

/// How a package manager's dependency folders are found in a project: its file (relative to
/// the root: `composer.json`), and what it says of them (the folders, relative to the root,
/// from the file's text; none: it says nothing — the module's fixed names stand).
#[derive(Debug, Clone, Copy)]
pub struct DependencyFinder {
    pub manifest: &'static str,
    pub folders: fn(&str) -> Vec<String>,
}

/// One finder per manager's file: a module registering its own again changes nothing.
impl PartialEq for DependencyFinder {
    fn eq(&self, other: &Self) -> bool {
        self.manifest == other.manifest
    }
}

impl Eq for DependencyFinder {}

/// Every module's folders (what they registered, without repeats).
static REGISTERED: std::sync::RwLock<Folders> = std::sync::RwLock::new(Folders::new());

#[derive(Debug)]
struct Folders {
    dependencies: Vec<&'static str>,
    unlisted: Vec<&'static str>,
    generated: Vec<&'static str>,
    finders: Vec<DependencyFinder>,
}

impl Folders {
    const fn new() -> Self {
        Self {
            dependencies: Vec::new(),
            unlisted: Vec::new(),
            generated: Vec::new(),
            finders: Vec::new(),
        }
    }
}

/// Bumped each time what makes a dependency changes (a module's folders, a project's found
/// or marked ones): what's worked out from them is good while it stays.
static CHANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What [`dependency_at`] answers is the same while this stays.
pub fn generation() -> u64 {
    CHANGES.load(std::sync::atomic::Ordering::Acquire)
}

fn changed() {
    CHANGES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Adds a module's folders (again: no change).
pub fn register(dirs: ProjectDirs) {
    let Ok(mut folders) = REGISTERED.write() else { return };
    changed();
    let add = |list: &mut Vec<&'static str>, names: &[&'static str]| {
        for name in names {
            if !list.contains(name) {
                list.push(name);
            }
        }
    };
    add(&mut folders.dependencies, dirs.dependencies);
    add(&mut folders.unlisted, dirs.unlisted);
    add(&mut folders.generated, dirs.generated);
    for finder in dirs.found_by {
        if !folders.finders.contains(finder) {
            folders.finders.push(*finder);
        }
    }
}

/// The package managers' finders the modules registered (each read when a project opens).
pub fn finders() -> Vec<DependencyFinder> {
    read(|f| f.finders.clone())
}

/// The dependency folders found in the project at `root` (relative, with `/`), as its
/// managers' files say (none: forgotten).
static FOUND: std::sync::RwLock<Vec<(std::path::PathBuf, Vec<String>)>> =
    std::sync::RwLock::new(Vec::new());

/// The dependency folders the managers' files of the project at `root` say (relative to it,
/// with `/`): what [`in_dependencies`] knows besides the modules' fixed names.
pub fn set_found(root: &Path, folders: Vec<String>) {
    let Ok(mut all) = FOUND.write() else { return };
    changed();
    all.retain(|(r, _)| r != root);
    if !folders.is_empty() {
        all.push((root.to_path_buf(), folders));
    }
}

/// The folders found in the project at `root` (relative, with `/`).
pub fn found(root: &Path) -> Vec<String> {
    FOUND.read().map_or_else(
        |_| Vec::new(),
        |all| all.iter().find(|(r, _)| r == root).map(|(_, f)| f.clone()).unwrap_or_default(),
    )
}

/// `relative` (to the project at `root`) is a dependency: through a folder a module names
/// (only `relative`'s own components: a project kept under a folder named `vendor` isn't
/// one), in one its managers' files name, or in a folder the user marked as a library.
/// The second: whether `relative` is such a folder itself (not something in it).
pub fn dependency_at(root: &Path, relative: &Path) -> Option<bool> {
    let named = |name: &str| read(|f| f.dependencies.contains(&name));
    let components: Vec<&str> =
        relative.components().filter_map(|c| c.as_os_str().to_str()).collect();
    if let Some(at) = components.iter().position(|c| named(c)) {
        return Some(at + 1 == components.len());
    }
    let found = found(root);
    if let Some(folder) = found.iter().find(|f| relative.starts_with(Path::new(f.as_str()))) {
        return Some(relative == Path::new(folder.as_str()));
    }
    marked_library(&root.join(relative)).then_some(false)
}

/// `path` is in a folder its project's managers' files say holds dependencies: absolute,
/// under that project; relative, from a project's root.
fn found_dependency(path: &Path) -> bool {
    let within = |relative: &Path, folders: &[String]| {
        folders.iter().any(|f| relative.starts_with(Path::new(f)))
    };
    FOUND.read().is_ok_and(|all| {
        all.iter().any(|(root, folders)| match path.strip_prefix(root) {
            Ok(relative) => within(relative, folders),
            Err(_) => !path.is_absolute() && within(path, folders),
        })
    })
}

fn read<R>(f: impl FnOnce(&Folders) -> R) -> R {
    match REGISTERED.read() {
        Ok(folders) => f(&folders),
        Err(poisoned) => f(&poisoned.into_inner()),
    }
}

/// `name` is a dependency folder (`vendor`, `node_modules`).
pub fn is_dependency_dir(name: &str) -> bool {
    read(|f| f.dependencies.contains(&name))
}

/// `name` isn't listed at all (`node_modules`).
pub fn is_unlisted_dir(name: &str) -> bool {
    read(|f| f.unlisted.contains(&name))
}

/// The dependency and generated folders (relative paths), for skipping them in searches.
pub fn skipped_from_search() -> Vec<&'static str> {
    read(|f| f.dependencies.iter().chain(&f.generated).copied().collect())
}

/// `relative` (to the project's root) is in a folder the code generates (`var/cache/…`,
/// `storage/…`, `dist/…`: registered relative to the root or to any folder).
pub fn in_generated(relative: &Path) -> bool {
    let path = relative.to_string_lossy();
    // Asked for every path that changes on disk: no text made unless it has `\\`.
    let path: std::borrow::Cow<'_, str> =
        if path.contains('\\') { path.replace('\\', "/").into() } else { path };
    let path = path.trim_start_matches('/');
    read(|f| f.generated.iter().any(|g| has_folder(path, g)))
}

/// Folder `folder` (one or more names, with `/`) is a whole part of `path`: at its start or
/// after a `/`, followed by its end or a `/`.
fn has_folder(path: &str, folder: &str) -> bool {
    !folder.is_empty()
        && path.match_indices(folder).any(|(at, _)| {
            let end = at + folder.len();
            (at == 0 || path.as_bytes()[at - 1] == b'/')
                && (end == path.len() || path.as_bytes()[end] == b'/')
        })
}

/// What the user marked as libraries, by project (absolute paths).
static LIBRARIES: std::sync::RwLock<Vec<(std::path::PathBuf, Vec<std::path::PathBuf>)>> =
    std::sync::RwLock::new(Vec::new());

/// The folders marked as libraries in the project at `root` (none: it's forgotten).
pub fn set_libraries(root: &Path, libraries: Vec<std::path::PathBuf>) {
    let Ok(mut all) = LIBRARIES.write() else { return };
    changed();
    all.retain(|(r, _)| r != root);
    if !libraries.is_empty() {
        all.push((root.to_path_buf(), libraries));
    }
}

fn marked_library(path: &Path) -> bool {
    path.is_absolute()
        && LIBRARIES.read().is_ok_and(|all| {
            all.iter().flat_map(|(_, list)| list).any(|library| path.starts_with(library))
        })
}

/// `path` goes through a dependency directory (`vendor/…`, `node_modules/…`), is in one its
/// project's managers' files name (Composer's `vendor-dir`), or is (absolute) in a folder the
/// user marked as a library.
pub fn in_dependencies(path: &Path) -> bool {
    marked_library(path) || found_dependency(path) || through_dependency_dir(path)
}

/// `path` goes through a folder named as a dependency one (`vendor`, `node_modules`): its
/// names compared as bytes, split where its separators are — asked for each suggestion of a
/// completion (tens of thousands at a key), not its components parsed and checked as text.
fn through_dependency_dir(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    let separator = |b: &u8| *b == b'/' || (cfg!(windows) && *b == b'\\');
    read(|f| {
        bytes
            .split(separator)
            .any(|name| !name.is_empty() && f.dependencies.iter().any(|d| d.as_bytes() == name))
    })
}

/// `path` goes through a dependency directory or `.git`.
pub fn in_dependencies_or_git(path: &Path) -> bool {
    in_dependencies(path) || path.components().any(|c| c.as_os_str() == ".git")
}

/// `path` as shown to people: under `home`, with `~` for it (`~/work/shop`, `~\work\shop`
/// on Windows); elsewhere (or relative) as it is.
pub fn tildify(path: &Path, home: Option<&Path>) -> String {
    match home.filter(|_| path.is_absolute()).and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// Whether `path` (relative, with `/`) matches glob `pattern`, as gitignore reads one: `*`
/// and `?` stay within a component, `**` crosses them (`**/` also matches nothing), `[a-z]`
/// / `[!…]` a set — `src/**/*.php`, `config/**`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    glob(pattern.as_bytes(), path.as_bytes())
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) if rest.first() == Some(&b'*') => {
            // `**/` also matches nothing; `**` alone matches everything.
            let rest = &rest[1..];
            let rest = rest.strip_prefix(b"/").unwrap_or(rest);
            (0..=text.len()).any(|i| glob(rest, &text[i..]))
        }
        Some((b'*', rest)) => {
            let stop = text.iter().position(|&c| c == b'/').unwrap_or(text.len());
            (0..=stop).any(|i| glob(rest, &text[i..]))
        }
        Some((b'?', rest)) => text.first().is_some_and(|&c| c != b'/') && glob(rest, &text[1..]),
        Some((b'[', rest)) => {
            let Some(close) = rest.iter().skip(1).position(|&c| c == b']').map(|i| i + 1) else {
                return text.first() == Some(&b'[') && glob(rest, &text[1..]);
            };
            let Some(&c) = text.first() else { return false };
            let (negated, set) = match rest[0] {
                b'!' | b'^' => (true, &rest[1..close]),
                _ => (false, &rest[..close]),
            };
            let mut found = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == b'-' {
                    found |= (set[i]..=set[i + 2]).contains(&c);
                    i += 3;
                } else {
                    found |= set[i] == c;
                    i += 1;
                }
            }
            found != negated && c != b'/' && glob(&rest[close + 1..], &text[1..])
        }
        Some((&p, rest)) => text.first() == Some(&p) && glob(rest, &text[1..]),
    }
}

#[cfg(test)]
#[path = "../tests/unit/paths.rs"]
mod tests;
