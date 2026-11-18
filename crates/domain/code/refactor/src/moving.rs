//! Moving a file or a folder to another folder, and the classes in it to the namespace of
//! where they go (PHP's PSR-4): the moved files' `namespace` declarations, the `use`
//! statements and fully qualified names pointing at them everywhere, the imports a file
//! now needs (a file of the old namespace that used a class without importing it, the
//! moved class using its old neighbours), and — unsure — strings naming them.
//!
//! Namespaces are written with their language's separator ([`NameSeparator`]: `\` in PHP's
//! `App\Models\User`). What a language's files declare and use comes through the
//! [`NamespaceRules`] port; the project's types through [`Types`].
//! The project is scanned once ([`scan_for_move`], on a background thread); the plan for a
//! namespace ([`plan_move`]) is then quick to make again as the namespace is typed.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_intelligence::FileSymbols;

use crate::application::Sources;
use crate::{Certainty, FilePlan, FileRename, Occurrence, Plan, line_of};

/// A class (or interface, trait, enum) name as written in code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRef {
    pub range: Range<usize>,
    /// As written, without a leading `\` (`Models\User`).
    pub name: String,
    /// Written with a leading `\`.
    pub fully_qualified: bool,
}

/// An imported class (`use App\Models\User as Member;`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// `App\Models\User`.
    pub fqn: String,
    /// The name it's known by in the file (`User`, or its alias).
    pub alias: String,
    /// The bytes of the imported name (in a group, the item's part: `User` in
    /// `use App\Models\{User, Post};`).
    pub range: Range<usize>,
    /// The whole item, alias included (`User as Member`).
    pub item: Range<usize>,
    /// The group statement it belongs to (an index into [`Block::groups`]).
    pub group: Option<usize>,
}

/// A grouped import statement (`use App\Models\{User, Post};`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportGroup {
    /// The whole statement.
    pub statement: Range<usize>,
}

/// A namespace's part of a file (most files have one).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Block {
    /// `App\Models` (empty: the global namespace).
    pub namespace: String,
    /// The namespace's name in its declaration (`None`: not declared).
    pub name_range: Option<Range<usize>>,
    /// The whole declaration (`namespace App\Models;`).
    pub statement: Option<Range<usize>>,
    /// Where a declaration goes when there's none (after `<?php`, after `declare(…);`).
    pub declare_at: usize,
    pub imports: Vec<Import>,
    pub groups: Vec<ImportGroup>,
    /// Where a new import goes: after the last one, else after the declaration.
    pub import_at: usize,
    /// The class names used (not declared, not imported).
    pub names: Vec<NameRef>,
    /// The types declared here (their short names).
    pub declared: Vec<String>,
}

/// What a file declares and uses, as far as namespaces go.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileScan {
    pub blocks: Vec<Block>,
    /// The contents of the string literals.
    pub strings: Vec<Range<usize>>,
}

/// A language's namespaces (PHP), contributed by its module.
pub trait NamespaceRules: Send + Sync {
    /// The files that can use classes (`php`).
    fn extensions(&self) -> &'static [&'static str];
    /// What separates a qualified name's parts (`\`).
    fn separator(&self) -> NameSeparator;
    fn scan(&self, text: &str) -> FileScan;
    /// `use App\Models\User;` (`fqn` may carry an alias: [`Self::aliased`]).
    fn import_statement(&self, fqn: &str) -> String;
    /// An import's name with its alias (`App\Models\User as Member`).
    fn aliased(&self, fqn: &str, alias: &str) -> String;
    /// `namespace App\Models;`
    fn namespace_statement(&self, namespace: &str) -> String;
    /// The language's own type (`Exception`, `DateTime`).
    fn builtin_type(&self, fqn: &str) -> bool;
    /// The project's (namespace prefix, folder) pairs — PSR-4 from `composer.json`;
    /// `read` reads a project file.
    fn roots(&self, root: &Path, read: &dyn Fn(&Path) -> Option<String>) -> NamespaceRoots;
}

impl std::fmt::Debug for dyn NamespaceRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamespaceRules").field("extensions", &self.extensions()).finish()
    }
}

/// Whether a class exists in the project (the index).
pub trait Types {
    fn is_type(&self, fqn: &str) -> bool;
}

/// What separates a qualified name's parts, by its language (`\` in PHP); a leading one
/// writes a name fully qualified, and strings may write it doubled (escaped).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NameSeparator(pub char);

impl NameSeparator {
    /// `ns\name` (`name` alone in the global namespace).
    pub fn join(self, ns: &str, name: &str) -> String {
        if ns.is_empty() { name.to_owned() } else { format!("{ns}{}{name}", self.0) }
    }

    /// The namespace of `fqn` (`App\Models` of `App\Models\User`).
    pub fn namespace_of(self, fqn: &str) -> &str {
        fqn.rsplit_once(self.0).map_or("", |(ns, _)| ns)
    }

    /// The short name of `fqn` (`User`).
    pub fn short_name(self, fqn: &str) -> &str {
        fqn.rsplit_once(self.0).map_or(fqn, |(_, name)| name)
    }

    /// `name` without the separators around it (`\App\` → `App`).
    pub fn trim(self, name: &str) -> &str {
        name.trim_matches(self.0)
    }

    /// A namespace as typed is valid: names separated by it (or empty: the global one).
    pub fn valid_namespace(self, ns: &str) -> bool {
        ns.is_empty()
            || ns.split(self.0).all(|part| crate::valid_name(part, crate::NameStyle::CODE))
    }
}

/// Which folder holds which namespace (PSR-4: `"App\\": "app/"`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamespaceRoots {
    pub separator: NameSeparator,
    /// (prefix without a trailing separator, absolute folder).
    roots: Vec<(String, PathBuf)>,
}

impl NamespaceRoots {
    pub fn new(
        separator: NameSeparator,
        roots: impl IntoIterator<Item = (String, PathBuf)>,
    ) -> Self {
        let roots = roots
            .into_iter()
            .map(|(prefix, dir)| (separator.trim(&prefix).to_owned(), dir))
            .collect();
        Self { separator, roots }
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// The namespace of the files in `dir` (the deepest root containing it); `None` outside
    /// every root or under a folder that can't be a namespace's name (`my-dir`).
    pub fn namespace_of(&self, dir: &Path) -> Option<String> {
        let (prefix, rest) = self
            .roots
            .iter()
            .filter_map(|(prefix, root)| Some((prefix, dir.strip_prefix(root).ok()?)))
            .min_by_key(|(_, rest)| rest.components().count())?;
        let mut ns = prefix.clone();
        for part in rest.components() {
            let part = part.as_os_str().to_str()?;
            if !crate::valid_name(part, crate::NameStyle::CODE) {
                return None;
            }
            ns = self.separator.join(&ns, part);
        }
        Some(ns)
    }

    /// The folder of namespace `ns` (by the longest root prefix it starts with).
    pub fn dir_of(&self, ns: &str) -> Option<PathBuf> {
        let sep = self.separator.0;
        let (prefix, root) = self
            .roots
            .iter()
            .filter(|(prefix, _)| {
                prefix.is_empty()
                    || ns.eq_ignore_ascii_case(prefix)
                    || ns.get(..prefix.len() + 1).is_some_and(|start| {
                        start[..prefix.len()].eq_ignore_ascii_case(prefix) && start.ends_with(sep)
                    })
            })
            .max_by_key(|(prefix, _)| prefix.len())?;
        let rest = ns.get(prefix.len()..).unwrap_or("").trim_start_matches(sep);
        Some(rest.split(sep).filter(|p| !p.is_empty()).fold(root.clone(), |dir, p| dir.join(p)))
    }
}

/// `from` can be moved: an entry inside the project at `root` (not the root itself).
pub fn movable(root: &Path, from: &Path) -> bool {
    from.starts_with(root) && from != root
}

/// Moving `from` into folder `into` changes something: not into its own folder, not into
/// itself.
pub fn moves_somewhere(from: &Path, into: &Path) -> bool {
    from.parent() != Some(into) && !into.starts_with(from)
}

/// The classes in `from` (a file, or every file under a folder) named after their file,
/// among the indexed `files` with `extensions` — sorted by path.
pub fn classes_in<'a>(
    files: impl IntoIterator<Item = (&'a Path, &'a FileSymbols)>,
    from: &Path,
    is_dir: bool,
    extensions: &[&str],
) -> Vec<ClassFile> {
    let mut classes: Vec<ClassFile> = files
        .into_iter()
        .filter(|(path, _)| {
            (if is_dir { path.starts_with(from) } else { *path == from })
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
        })
        .filter_map(|(path, file)| {
            let stem = path.file_stem()?.to_str()?;
            let class = file.symbols.iter().find(|s| s.kind.is_type() && s.name == stem.into())?;
            let subdirs = path
                .parent()?
                .strip_prefix(from)
                .ok()
                .map(|rel| {
                    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()
                })
                .unwrap_or_default();
            Some(ClassFile { fqn: class.fqn.to_string(), path: path.to_path_buf(), subdirs })
        })
        .collect();
    classes.sort_by(|a, b| a.path.cmp(&b.path));
    classes
}

/// A class moved with its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassFile {
    pub fqn: String,
    /// The file (absolute, before the move).
    pub path: PathBuf,
    /// The folders between the moved folder and the file (none for a moved file).
    pub subdirs: Vec<String>,
}

/// What is moved, and where to by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveTarget {
    /// The file or folder moved.
    pub from: PathBuf,
    pub is_dir: bool,
    /// The folder it goes into (dropped on, or its own for "Move…").
    pub into: PathBuf,
    /// The classes in it named after their file, whose namespace follows the move.
    pub classes: Vec<ClassFile>,
    /// The namespace of the moved file / folder now (`None`: its classes don't follow the
    /// folders).
    pub namespace: Option<String>,
    /// The namespace where it goes (by the roots).
    pub suggested: Option<String>,
    pub separator: NameSeparator,
}

impl MoveTarget {
    pub fn new(
        from: PathBuf,
        is_dir: bool,
        into: PathBuf,
        classes: Vec<ClassFile>,
        roots: &NamespaceRoots,
    ) -> Self {
        let sep = roots.separator;
        let namespace = if is_dir {
            roots.namespace_of(&from).or_else(|| {
                // The folders a class is under, taken off its namespace.
                classes.iter().find_map(|c| {
                    let mut ns = sep.namespace_of(&c.fqn);
                    for dir in c.subdirs.iter().rev() {
                        let (rest, last) = ns.rsplit_once(sep.0).unwrap_or(("", ns));
                        if !last.eq_ignore_ascii_case(dir) {
                            return None;
                        }
                        ns = rest;
                    }
                    Some(ns.to_owned())
                })
            })
        } else {
            classes.first().map(|c| sep.namespace_of(&c.fqn).to_owned())
        };
        let suggested = if classes.is_empty() {
            None
        } else if is_dir {
            from.file_name().and_then(|name| roots.namespace_of(&into.join(name)))
        } else {
            roots.namespace_of(&into)
        };
        Self { from, is_dir, into, classes, namespace, suggested, separator: sep }
    }

    /// The name of the moved file / folder.
    pub fn name(&self) -> &std::ffi::OsStr {
        self.from.file_name().unwrap_or_default()
    }

    /// The namespace shown first: where it goes, or where it is.
    pub fn initial_namespace(&self) -> String {
        self.suggested.clone().or_else(|| self.namespace.clone()).unwrap_or_default()
    }

    /// Where the moved entry ends up in namespace `ns`: its folder by the roots, else the
    /// folder it's moved into.
    pub fn destination(&self, ns: &str, roots: &NamespaceRoots) -> PathBuf {
        match roots.dir_of(ns).filter(|_| !self.classes.is_empty()) {
            Some(dir) if self.is_dir => dir,
            Some(dir) => dir.join(self.name()),
            None => self.into.join(self.name()),
        }
    }

    /// Each moved class's new name in namespace `ns` (those that change).
    pub fn moves(&self, ns: &str) -> Vec<(String, String)> {
        let sep = self.separator;
        self.classes
            .iter()
            .filter_map(|class| {
                let base =
                    |root: &str| class.subdirs.iter().fold(root.to_owned(), |n, d| sep.join(&n, d));
                // A class not where its folders say stays in its namespace.
                if let Some(old) = &self.namespace
                    && !base(old).eq_ignore_ascii_case(sep.namespace_of(&class.fqn))
                {
                    return None;
                }
                let new = sep.join(&base(ns), sep.short_name(&class.fqn));
                (new != class.fqn).then(|| (class.fqn.clone(), new))
            })
            .collect()
    }
}

/// The files a move changes: scanned once, planned again for each namespace typed.
#[derive(Debug, Default, Clone)]
pub struct MoveFacts {
    pub files: Vec<ScannedFile>,
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub text: String,
    pub scan: FileScan,
}

/// Everything a move needs, snapshots for a background thread.
#[derive(Clone)]
pub struct MoveContext {
    pub target: MoveTarget,
    pub roots: NamespaceRoots,
    /// The project's root (a typed folder is relative to it).
    pub root: PathBuf,
    pub sources: Arc<dyn Sources + Send + Sync>,
    /// `None`: no language's namespaces apply (a plain move).
    pub rules: Option<Arc<dyn NamespaceRules>>,
    pub types: Arc<dyn Types + Send + Sync>,
}

impl std::fmt::Debug for MoveContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveContext").field("target", &self.target).finish_non_exhaustive()
    }
}

/// The project's files that may use the moved classes (they name one of them), and the
/// moved ones. Nothing without classes.
pub fn scan_for_move(
    target: &MoveTarget,
    sources: &dyn Sources,
    rules: &dyn NamespaceRules,
    cancelled: &dyn Fn() -> bool,
) -> MoveFacts {
    let mut facts = MoveFacts::default();
    if target.classes.is_empty() {
        return facts;
    }
    let sep = rules.separator();
    let shorts: Vec<&str> = target.classes.iter().map(|c| sep.short_name(&c.fqn)).collect();
    // Many classes (a big folder): every file is scanned, not searched name by name.
    let filter = shorts.len() <= 32;
    for path in sources.candidates() {
        if cancelled() {
            break;
        }
        let moved = path.starts_with(&target.from);
        let Some(text) = sources.read(&path) else { continue };
        if !moved && filter && !shorts.iter().any(|s| text.contains(s)) {
            continue;
        }
        let scan = rules.scan(&text);
        facts.files.push(ScannedFile { path, text, scan });
    }
    facts
}

/// The old names of the moved classes and their new ones (case-insensitive, like PHP).
struct Moves(HashMap<String, String>);

impl Moves {
    fn get(&self, fqn: &str) -> Option<&String> {
        self.0.get(&fqn.to_ascii_lowercase())
    }
}

/// What `name` refers to with `imports` (alias, class) in namespace `ns`.
fn resolve(name: &NameRef, imports: &[(String, String)], ns: &str, sep: NameSeparator) -> String {
    if name.fully_qualified {
        return name.name.clone();
    }
    let (first, rest) = match name.name.split_once(sep.0) {
        Some((first, rest)) => (first, Some(rest)),
        None => (name.name.as_str(), None),
    };
    if let Some((_, fqn)) = imports.iter().find(|(alias, _)| alias.eq_ignore_ascii_case(first)) {
        return match rest {
            Some(rest) => sep.join(fqn, rest),
            None => fqn.clone(),
        };
    }
    sep.join(ns, &name.name)
}

/// The plan moving `target` to `destination`, its classes into namespace `ns`.
pub fn plan_move(context: &MoveContext, facts: &MoveFacts, ns: &str, destination: &Path) -> Plan {
    let target = &context.target;
    let mut plan = Plan::default();
    plan.renames.push(FileRename {
        from: target.from.clone(),
        to: destination.to_path_buf(),
        chosen: true,
    });
    let Some(rules) = &context.rules else { return plan };
    let moves = target.moves(ns);
    let new_ns: HashMap<&Path, String> = target
        .classes
        .iter()
        .filter_map(|c| {
            let (_, new) = moves.iter().find(|(old, _)| *old == c.fqn)?;
            Some((c.path.as_path(), target.separator.namespace_of(new).to_owned()))
        })
        .collect();
    let moves =
        Moves(moves.into_iter().map(|(old, new)| (old.to_ascii_lowercase(), new)).collect());
    if moves.0.is_empty() {
        return plan;
    }
    for file in &facts.files {
        let moved_to = new_ns.get(file.path.as_path()).map(String::as_str);
        let occurrences = plan_file(file, &moves, moved_to, rules.as_ref(), context.types.as_ref());
        if !occurrences.is_empty() {
            plan.files.push(FilePlan { path: file.path.clone(), occurrences });
        }
    }
    plan
}

/// A change: `range` of the text becomes `text`.
fn occurrence(text: &str, range: Range<usize>, replacement: String, certain: bool) -> Occurrence {
    let certainty = if certain { Certainty::Certain } else { Certainty::Uncertain };
    let (line, line_text, in_line) = line_of(text, &range);
    Occurrence {
        original: text[range.clone()].to_owned(),
        range,
        replacement,
        certainty,
        chosen: certain,
        line,
        line_text,
        in_line,
    }
}

/// An insertion at `at` (shown as the inserted lines alone).
fn insertion(text: &str, at: usize, inserted: String) -> Occurrence {
    let (line, _, _) = line_of(text, &(at..at));
    Occurrence {
        range: at..at,
        original: String::new(),
        replacement: inserted,
        certainty: Certainty::Certain,
        chosen: true,
        line,
        line_text: String::new(),
        in_line: 0..0,
    }
}

/// The changes in one file; `moved_to`: the file's class moves to this namespace.
fn plan_file(
    file: &ScannedFile,
    moves: &Moves,
    moved_to: Option<&str>,
    rules: &dyn NamespaceRules,
    types: &dyn Types,
) -> Vec<Occurrence> {
    let text = file.text.as_str();
    let sep = rules.separator();
    let mut out = Vec::new();
    // Insertions by place (several at one place make one edit).
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let mut insert = |at: usize, s: String| match inserts.iter_mut().find(|(a, _)| *a == at) {
        Some((_, existing)) => existing.push_str(&s),
        None => inserts.push((at, s)),
    };
    let single = file.scan.blocks.len() == 1;
    for block in &file.scan.blocks {
        let old_ns = block.namespace.as_str();
        let new_ns = moved_to.filter(|_| single).unwrap_or(old_ns);
        let ns_changed = !new_ns.eq_ignore_ascii_case(old_ns);
        // The namespace declaration.
        if ns_changed {
            match (&block.name_range, &block.statement) {
                (Some(range), _) if !new_ns.is_empty() => {
                    out.push(occurrence(text, range.clone(), new_ns.to_owned(), true));
                }
                (_, Some(statement)) => {
                    out.push(occurrence(text, statement.clone(), String::new(), true))
                }
                _ => insert(block.declare_at, format!("\n\n{}", rules.namespace_statement(new_ns))),
            }
        }
        // The imports of moved classes.
        let old_imports: Vec<(String, String)> =
            block.imports.iter().map(|i| (i.alias.clone(), i.fqn.clone())).collect();
        let mut new_imports: Vec<(String, String)> = block
            .imports
            .iter()
            .map(|i| (i.alias.clone(), moves.get(&i.fqn).cloned().unwrap_or_else(|| i.fqn.clone())))
            .collect();
        let mut added: Vec<String> = Vec::new();
        for import in block.imports.iter().filter(|i| i.group.is_none()) {
            if let Some(new) = moves.get(&import.fqn) {
                out.push(occurrence(text, import.range.clone(), new.clone(), true));
            }
        }
        // A grouped import loses its moved items; they're imported on their own.
        for (g, group) in block.groups.iter().enumerate() {
            let items: Vec<&Import> = block.imports.iter().filter(|i| i.group == Some(g)).collect();
            if !items.iter().any(|i| moves.get(&i.fqn).is_some()) {
                continue;
            }
            let kept: Vec<&str> = items
                .iter()
                .filter(|i| moves.get(&i.fqn).is_none())
                .map(|i| &text[i.item.clone()])
                .collect();
            let replacement = match (items.first(), items.last()) {
                (Some(first), Some(last)) if !kept.is_empty() => format!(
                    "{}{}{}",
                    &text[group.statement.start..first.item.start],
                    kept.join(", "),
                    &text[last.item.end..group.statement.end]
                ),
                _ => String::new(),
            };
            out.push(occurrence(text, group.statement.clone(), replacement, true));
            for item in items.iter().filter(|i| moves.get(&i.fqn).is_some()) {
                let new = moves.get(&item.fqn).cloned().unwrap_or_default();
                if item.alias.eq_ignore_ascii_case(sep.short_name(&new)) {
                    added.push(new);
                } else {
                    // Aliased: kept as written.
                    added.push(rules.aliased(&new, &item.alias));
                }
            }
        }
        // The names used.
        for name in &block.names {
            let old = resolve(name, &old_imports, old_ns, sep);
            let wanted = match moves.get(&old) {
                Some(new) => new.clone(),
                // Not moved: it only matters if the namespace around it changed and it
                // resolved through it.
                None if ns_changed && (types.is_type(&old) || rules.builtin_type(&old)) => old,
                None => continue,
            };
            let now = resolve(name, &new_imports, new_ns, sep);
            if now.eq_ignore_ascii_case(&wanted) {
                continue;
            }
            let unqualified = !name.fully_qualified && !name.name.contains(sep.0);
            let alias = name.name.as_str();
            let taken = new_imports.iter().any(|(a, _)| a.eq_ignore_ascii_case(alias))
                || block.declared.iter().any(|d| d.eq_ignore_ascii_case(alias));
            if unqualified && !taken && sep.short_name(&wanted).eq_ignore_ascii_case(alias) {
                // Imported (once): the name stays as written.
                new_imports.push((alias.to_owned(), wanted.clone()));
                added.push(wanted);
            } else {
                out.push(occurrence(text, name.range.clone(), format!("{}{wanted}", sep.0), true));
            }
        }
        if !added.is_empty() {
            let sep = if block.imports.is_empty() { "\n\n" } else { "\n" };
            let lines: Vec<String> = added.iter().map(|fqn| rules.import_statement(fqn)).collect();
            insert(block.import_at, format!("{sep}{}", lines.join("\n")));
        }
    }
    // Strings naming a moved class (`'App\Models\User'`, `"App\\Models\\User"`): unsure.
    for range in &file.scan.strings {
        let s = &text[range.clone()];
        if !s.contains(sep.0) {
            continue;
        }
        let bare = s.trim_start_matches(sep.0);
        let single = sep.0.to_string();
        let double = single.repeat(2);
        let doubled = bare.contains(&double);
        let fqn = if doubled { bare.replace(&double, &single) } else { bare.to_owned() };
        if let Some(new) = moves.get(&fqn) {
            let lead = &s[..s.len() - bare.len()];
            let new = if doubled { new.replace(&single, &double) } else { new.clone() };
            out.push(occurrence(text, range.clone(), format!("{lead}{new}"), false));
        }
    }
    out.extend(inserts.into_iter().map(|(at, s)| insertion(text, at, s)));
    out.sort_by_key(|o| (o.range.start, o.range.end));
    out
}

#[cfg(test)]
#[path = "../tests/unit/moving.rs"]
mod tests;
