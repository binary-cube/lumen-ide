//! The refactor domain's ports, answered with the editor's own knowledge (the symbol index, the
//! languages' navigation): the project's
//! [`Sources`] (the indexed files of the languages that can use the symbol, open
//! documents as in the editor) and a [`Resolver`] that asks the language's navigation
//! where each found name is declared — the renamed symbol, another one, or unknown.
//! Both are snapshots, used on the rename preview's background thread.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use code_intelligence::{LanguageIntelligence, Location, Naming, PlaceMatch, SymbolIndex};
use kernel::LanguageId;
use refactor::application::{Conflict, ConflictCheck, Resolution, Resolver, Sources};

/// The project's files a rename looks into.
pub struct ProjectSources {
    pub root: PathBuf,
    /// The project's queries (its files); `None` without a project: only `only`.
    pub queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    /// Without a project: the one file looked into.
    pub only: Option<PathBuf>,
    /// The files that can use the symbol (their extensions; the project's own).
    pub filter: workspace::FileFilter,
    /// The open documents' text (unsaved changes included).
    pub open: Arc<HashMap<PathBuf, String>>,
}

impl std::fmt::Debug for ProjectSources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectSources").field("root", &self.root).finish_non_exhaustive()
    }
}

impl Sources for ProjectSources {
    fn root(&self) -> Option<&Path> {
        Some(&self.root)
    }

    fn candidates(&self) -> Vec<PathBuf> {
        match &self.queries {
            Some(queries) => queries.files(&self.filter),
            None => self.only.iter().cloned().collect(),
        }
    }

    fn read(&self, path: &Path) -> Option<String> {
        if let Some(text) = self.open.get(path) {
            return Some(text.clone());
        }
        if std::fs::metadata(path).ok()?.len() > workspace::MAX_SOURCE_FILE {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }
}

pub use code_intelligence::rename::Declared;
use code_intelligence::rename::{self, Match};

/// Resolves found names with the languages' navigation (go to definition).
#[derive(Debug)]
pub struct IndexResolver {
    pub root: Option<PathBuf>,
    pub index: Arc<RwLock<SymbolIndex>>,
    pub intelligence: Vec<Arc<dyn LanguageIntelligence>>,
    pub open: Arc<HashMap<PathBuf, String>>,
    pub levels: Vec<(LanguageId, String)>,
    pub target: Declared,
}

/// The language whose files have `path`'s extension.
pub fn intelligence_for<'a>(
    intelligence: &'a [Arc<dyn LanguageIntelligence>],
    path: &Path,
) -> Option<&'a dyn LanguageIntelligence> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    intelligence
        .iter()
        .find(|i| i.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
        .map(AsRef::as_ref)
}

/// The languages' modules answer how their files write names (the refactor domain's
/// [`refactor::application::Languages`]).
pub struct ModuleLanguages<'a>(pub &'a [Arc<dyn LanguageIntelligence>]);

impl refactor::application::Languages for ModuleLanguages<'_> {
    fn naming(&self, path: &Path) -> Naming {
        intelligence_for(self.0, path).map(|i| i.naming()).unwrap_or_default()
    }

    fn type_per_file(&self, extension: &str) -> bool {
        !extension.is_empty() && self.0.iter().any(|i| i.naming().type_per_file == Some(extension))
    }
}

impl IndexResolver {
    fn intelligence_for(&self, path: &Path) -> Option<&dyn LanguageIntelligence> {
        intelligence_for(&self.intelligence, path)
    }

    fn definitions(&self, path: &Path, text: &str, offset: usize) -> Vec<Location> {
        let Some(intel) = self.intelligence_for(path) else { return Vec::new() };
        let Ok(index) = self.index.read() else { return Vec::new() };
        let open = |p: &Path| self.open.get(p).cloned();
        let languages = &self.intelligence;
        crate::project_disk::with_project(
            self.root.as_deref(),
            &self.levels,
            languages,
            &open,
            |project| code_intelligence::definition(intel, &index, path, text, offset, project),
        )
    }
}

impl Resolver for IndexResolver {
    fn resolve(&self, path: &Path, text: &str, range: Range<usize>) -> Resolution {
        let sigil = self.intelligence_for(path).and_then(|i| i.naming().sigil);
        if self.target.declares(path, &range, sigil) {
            return Resolution::Target;
        }
        let Some(kind) = self.target.kind else {
            // A local variable: everything with its name in its scope, unless the language
            // says it's declared elsewhere.
            return match self.definitions(path, text, range.start).first() {
                Some(l) if !self.target.is(l) => Resolution::Other,
                _ => Resolution::Unknown,
            };
        };
        let fits = match self.intelligence_for(path) {
            Some(intel) => intel.fits_kind(text, &range, kind),
            None => code_intelligence::fits_kind_plainly(text, &range, kind, None),
        };
        if !fits {
            return Resolution::Other;
        }
        // Names known by their place alone (CSS's), not through definitions.
        let by_place = match self.intelligence_for(path) {
            Some(intel) => intel.place_match(text, &range, kind),
            None => code_intelligence::place_match(text, &range, kind, None),
        };
        if let Some(by_place) = by_place {
            return match by_place {
                PlaceMatch::Sure => Resolution::Target,
                PlaceMatch::Maybe => Resolution::Unknown,
                PlaceMatch::Other => Resolution::Other,
            };
        }
        let found = self.definitions(path, text, range.start);
        let matched = match self.index.read() {
            Ok(index) => self.target.by_definitions_in(&found, &index),
            Err(_) => self.target.by_definitions(&found),
        };
        match matched {
            Match::Target => Resolution::Target,
            Match::Unknown => Resolution::Unknown,
            Match::Other => Resolution::Other,
        }
    }
}

/// What a new name would clash with, from the index (or the scope's text for a local
/// variable).
#[derive(Debug)]
pub struct IndexConflicts {
    pub index: Arc<RwLock<SymbolIndex>>,
    pub target: Declared,
    /// A local variable's scope: its file's text and the range.
    pub local: Option<(String, Range<usize>)>,
    /// How the declaration's language writes names (whether case matters, its namespace
    /// separator).
    pub naming: Naming,
}

impl ConflictCheck for IndexConflicts {
    fn conflicts(&self, new_name: &str) -> Vec<Conflict> {
        let at = |path: &Path, text: Option<&str>, offset: usize, line: u32| {
            let line_text = text
                .map(|t| refactor::line_of(t, &(offset..offset)).1)
                .or_else(|| {
                    let source = std::fs::read_to_string(path).ok()?;
                    source.lines().nth(line as usize).map(str::to_owned)
                })
                .unwrap_or_default();
            Conflict { path: path.to_path_buf(), line, line_text: line_text.trim().to_owned() }
        };
        let Some(kind) = self.target.kind else {
            // A variable: the new name already used in the same function.
            let Some((text, scope)) = &self.local else { return Vec::new() };
            let style = refactor::NameStyle::CODE.with_sigil(self.naming.sigil);
            return refactor::find_word(text, new_name, style)
                .into_iter()
                .filter(|r| scope.start <= r.start && r.end <= scope.end)
                .take(3)
                .map(|r| {
                    let line = kernel::text::line_of(text, r.start);
                    at(&self.target.at.path, Some(text), r.start, line)
                })
                .collect();
        };
        // Where they are, taken from the index; its lock let go before any file is read,
        // and only the first three read (one line each).
        let mut found: Vec<(PathBuf, usize, u32)> = {
            let Ok(index) = self.index.read() else { return Vec::new() };
            rename::clashes(&index, &self.target, kind, new_name, &self.naming)
                .into_iter()
                .map(|(path, s)| (path.to_path_buf(), s.offset, s.line))
                .collect()
        };
        found.sort_by(|a, b| (&a.0, a.2).cmp(&(&b.0, b.2)));
        found.dedup_by(|a, b| a.0 == b.0 && a.2 == b.2);
        found.truncate(3);
        found.into_iter().map(|(path, offset, line)| at(&path, None, offset, line)).collect()
    }
}

/// The extensions a symbol of `kind` in `language` can be used in, by its language's module.
pub(crate) fn extensions(
    intelligence: &[Arc<dyn LanguageIntelligence>],
    language: &str,
    kind: code_intelligence::SymbolKind,
) -> &'static [&'static str] {
    let module = intelligence.iter().find(|i| i.language().as_str() == language);
    code_intelligence::usages::usage_extensions(
        module.map(|i| i.as_ref() as &dyn LanguageIntelligence),
        kind,
    )
}

/// A rename's places as a language server says them: waited for on the preview's
/// background thread (never on the UI's), found again in the files by their line and their
/// place in it (a file read from disk may end its lines with `\r\n`, the server counts
/// them as `\n`). In the files of the server's languages its places are the symbol's;
/// in the others' (a script's function used in HTML, PHP, Twig) the built-in engine's
/// ports answer, as they did before servers — the union, each file looked at once. If the
/// server can't say (nothing, an error, a timeout), the built-in engine's answer
/// everywhere (when it has some).
/// A server's answer to "where would a rename change".
type PlacesReply = code_intelligence::servers::Reply<
    Result<Vec<code_intelligence::servers::ServedLocation>, String>,
>;
/// The places by file: (line, byte in the line) of each.
type PlacesByFile = HashMap<PathBuf, std::collections::HashSet<(u32, usize)>>;

/// Whether a file is one the server knows (its languages'): its places there are the
/// server's; elsewhere (another language's file) the built-in engine's.
pub type Covered = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// What a server is asked for a symbol's places: a rename's (where it would change the
/// name), or its references (Find Usages).
pub enum ServedPlaces {
    Rename(PlacesReply),
    References(code_intelligence::servers::Reply<Vec<code_intelligence::servers::ServedLocation>>),
}

impl ServedPlaces {
    /// The places, waited for up to `timeout` (`None`: the server said nothing — an error, a
    /// timeout — or nothing at all).
    fn wait(
        self,
        timeout: std::time::Duration,
    ) -> Option<Vec<code_intelligence::servers::ServedLocation>> {
        let places = match self {
            Self::Rename(reply) => reply.wait(timeout)?.ok()?,
            Self::References(reply) => reply.wait(timeout)?,
        };
        Some(places).filter(|p| !p.is_empty())
    }
}

pub struct ServedRename {
    reply: std::sync::Mutex<Option<ServedPlaces>>,
    /// The built-in engine's places joined with the server's in the server's own files too
    /// (Find Usages where it keeps renaming, or both engines answer): each line once.
    joined: bool,
    covered: Covered,
    /// The places by file: (line, byte in the line) of each; `None`: the server didn't say.
    places: std::sync::OnceLock<Option<PlacesByFile>>,
    open: Arc<HashMap<PathBuf, String>>,
    fallback: Option<(Arc<dyn Sources + Send + Sync>, Arc<dyn Resolver + Send + Sync>)>,
    /// The built-in engine's symbol is a local one (a variable, a parameter): its places are
    /// in that file and range only — the built-in engine looks nowhere else.
    local: Option<(PathBuf, Range<usize>)>,
}

impl std::fmt::Debug for ServedRename {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServedRename").finish_non_exhaustive()
    }
}

/// How long a rename's preview waits for the server.
const SERVED_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Where byte `offset` of `text` is: its line and its place in the line.
fn line_place(text: &str, offset: usize) -> (u32, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (u32::try_from(line).unwrap_or(u32::MAX), offset - start)
}

impl ServedRename {
    pub fn new(
        reply: ServedPlaces,
        open: Arc<HashMap<PathBuf, String>>,
        fallback: Option<(Arc<dyn Sources + Send + Sync>, Arc<dyn Resolver + Send + Sync>)>,
        covered: Covered,
        local: Option<(PathBuf, Range<usize>)>,
    ) -> Self {
        Self {
            reply: std::sync::Mutex::new(Some(reply)),
            joined: false,
            covered,
            local,
            places: std::sync::OnceLock::new(),
            open,
            fallback,
        }
    }

    /// The built-in engine's places joined with the server's in its own files too.
    #[must_use]
    pub fn joined(mut self, joined: bool) -> Self {
        self.joined = joined;
        self
    }

    /// The server's places (waited for once; none: it had nothing to say, the built-in
    /// engine's are used).
    fn places(&self) -> Option<&PlacesByFile> {
        self.places
            .get_or_init(|| {
                let reply = self.reply.lock().ok()?.take()?;
                let places = reply.wait(SERVED_WAIT)?;
                let mut out: HashMap<PathBuf, std::collections::HashSet<(u32, usize)>> =
                    HashMap::new();
                for place in places {
                    let text = match self.open.get(&place.path) {
                        Some(text) => text.clone(),
                        None => std::fs::read_to_string(&place.path).ok()?.replace("\r\n", "\n"),
                    };
                    out.entry(place.path).or_default().insert(line_place(&text, place.range.start));
                }
                Some(out)
            })
            .as_ref()
    }
}

/// The served rename's files.
#[derive(Debug)]
pub struct ServedSources(pub Arc<ServedRename>);

impl Sources for ServedSources {
    fn candidates(&self) -> Vec<PathBuf> {
        match (self.0.places(), &self.0.fallback) {
            (Some(places), fallback) => {
                let mut files: Vec<PathBuf> = places.keys().cloned().collect();
                if let Some((path, _)) = self.0.local.as_ref().filter(|_| self.0.joined) {
                    // Joined: a local symbol's file, as the built-in engine finds it.
                    files.push(path.clone());
                } else if let Some((sources, _)) =
                    fallback.as_ref().filter(|_| self.0.local.is_none())
                {
                    // The other languages' files the built-in engine would look into (none
                    // for a local symbol) — joined, the server's languages' too.
                    let joined = self.0.joined;
                    files.extend(
                        sources.candidates().into_iter().filter(|p| joined || !(self.0.covered)(p)),
                    );
                }
                files.sort();
                files.dedup();
                files
            }
            (None, Some(_)) if self.0.local.is_some() => {
                self.0.local.iter().map(|(path, _)| path.clone()).collect()
            }
            (None, Some((sources, _))) => sources.candidates(),
            (None, None) => Vec::new(),
        }
    }

    fn read(&self, path: &Path) -> Option<String> {
        if let Some(text) = self.0.open.get(path) {
            return Some(text.clone());
        }
        if std::fs::metadata(path).ok()?.len() > workspace::MAX_SOURCE_FILE {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }
}

/// The served rename's places: the server's are the symbol, the others aren't.
#[derive(Debug)]
pub struct ServedResolver(pub Arc<ServedRename>);

impl Resolver for ServedResolver {
    fn resolve(&self, path: &Path, text: &str, range: Range<usize>) -> Resolution {
        match (self.0.places(), &self.0.fallback) {
            (Some(_), Some(_)) if self.0.local.is_none() && !(self.0.covered)(path) => {
                self.built_in(path, text, range)
            }
            (Some(places), _) => {
                let found =
                    places.get(path).is_some_and(|p| p.contains(&line_place(text, range.start)));
                match found {
                    true => Resolution::Target,
                    // Joined: what the built-in engine finds there too (each place once:
                    // the occurrences are the text's).
                    false if self.0.joined && self.0.fallback.is_some() => {
                        self.built_in(path, text, range)
                    }
                    false => Resolution::Other,
                }
            }
            (None, _) => self.built_in(path, text, range),
        }
    }
}

impl ServedResolver {
    /// The built-in engine's answer for an occurrence (none: unknown).
    fn built_in(&self, path: &Path, text: &str, range: Range<usize>) -> Resolution {
        let Some((_, resolver)) = &self.0.fallback else { return Resolution::Unknown };
        let Some((local, scope)) = &self.0.local else {
            return resolver.resolve(path, text, range);
        };
        // A local symbol: everything with its name in its scope, as the built-in engine
        // renames it.
        let inside = local == path && scope.start <= range.start && range.end <= scope.end;
        match resolver.resolve(path, text, range) {
            Resolution::Other => Resolution::Other,
            _ if inside => Resolution::Target,
            _ => Resolution::Other,
        }
    }
}

/// No conflicts known (the server checks the new name itself).
#[derive(Debug)]
pub struct NoConflicts;

impl ConflictCheck for NoConflicts {
    fn conflicts(&self, _new_name: &str) -> Vec<Conflict> {
        Vec::new()
    }
}
