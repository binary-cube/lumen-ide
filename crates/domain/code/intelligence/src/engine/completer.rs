//! What completion suggests for each kind of context: members after `->` and `::`,
//! variables, namespaces, types, names, keywords, declarations, paths, the language's own
//! words.

use super::completion::{
    LIMIT, Narrowing, accessible, builtin_item, item, keep_narrowed, keyword_insert, narrowed,
    receiver_class,
};
use super::{Project, in_language, is_vendor, of_language};
use crate::CompletionContext;
use crate::CompletionItem;
use crate::CompletionKind;
use crate::ContextKind;
use crate::LanguageIntelligence;
use crate::Naming;
use crate::Receiver;
use crate::fuzzy::fuzzy_score;
use crate::index::SymbolIndex;
use crate::symbols::FileSymbols;
use crate::symbols::Symbol;
use crate::symbols::SymbolKind;
use kernel::LanguageId;
use std::path::Path;

/// What completion at one position works with: the language, the index, the file and the
/// prefix being completed (`query`).
pub(super) struct Completer<'a, 'p> {
    pub(super) intelligence: &'a dyn LanguageIntelligence,
    pub(super) index: &'a SymbolIndex,
    pub(super) path: &'a Path,
    pub(super) text: &'a str,
    pub(super) offset: usize,
    pub(super) project: &'a Project<'p>,
    pub(super) file: &'a FileSymbols,
    pub(super) query: String,
    pub(super) language: LanguageId,
    pub(super) naming: Naming,
}

impl Completer<'_, '_> {
    /// The suggestions for the context's kind.
    pub(super) fn items(&self, kind: &ContextKind) -> Vec<CompletionItem> {
        match kind {
            ContextKind::None | ContextKind::Vocabulary(_) => Vec::new(),
            ContextKind::Member(receiver) => self.members(receiver),
            ContextKind::Methods(receiver) => self
                .members(receiver)
                .into_iter()
                .filter(|i| i.kind == CompletionKind::Method)
                .map(|i| CompletionItem { insert: i.label.clone(), cursor_back: 0, ..i })
                .collect(),
            ContextKind::Static(class) => self.statics(class),
            ContextKind::Variable => self.variables(),
            ContextKind::Name | ContextKind::TypeName if self.query.is_empty() => self.namespaces(),
            ContextKind::TypeName => {
                let mut items = self.type_names();
                items.extend(self.namespaces());
                items
            }
            ContextKind::Name => {
                let mut items = self.names();
                items.extend(self.namespaces());
                items
            }
            ContextKind::Symbols(kinds) => self.symbols(kinds),
            ContextKind::Path(dir) => self.paths(dir),
        }
    }

    /// The members after `->`: those of the receiver's class, accessible from here (its
    /// type as the language works it out knowing the project, else the context's).
    fn members(&self, receiver: &Receiver) -> Vec<CompletionItem> {
        let (file, offset, query) = (self.file, self.offset, &self.query);
        let types = crate::Types { index: self.index, path: self.path };
        // Every class it can be (`$flag ? new A() : new B()`: both), each member once.
        let mut classes = self.intelligence.receiver_types(self.text, offset, &types);
        if classes.is_empty() {
            classes.extend(receiver_class(receiver, file, offset, &self.naming));
        }
        match classes.is_empty() {
            false => {
                let current = file.type_at(offset).map(|t| &*t.fqn);
                let mut seen = std::collections::HashSet::new();
                let mut items = Vec::new();
                for class in &classes {
                    let own = *receiver == Receiver::This
                        || current.is_some_and(|c| c.eq_ignore_ascii_case(class));
                    let instance = |s: &Symbol| {
                        accessible(s, current, own)
                            && (!s.is_static || s.kind == SymbolKind::Method)
                            && s.kind != SymbolKind::ClassConstant
                            && s.kind != SymbolKind::EnumCase
                    };
                    for item in self.class_members(class, instance) {
                        if seen.insert((item.label.clone(), item.kind)) {
                            items.push(item);
                        }
                    }
                }
                items
            }
            // Unknown type: methods and properties with a matching name, from the whole index
            // (a library file looked at only if one of its members' names matches).
            true if !query.is_empty() => {
                let mut items = Vec::new();
                let score = |name: &str| fuzzy_score(query, name);
                let language = self.language.as_str();
                let version = self.index.library_version();
                let within = narrowed(Narrowing::Members, version, language, query);
                // Twice the suggestions shown: room for the names whose members aren't
                // methods or properties, or of another language.
                let taken = self.index.for_each_best_member(
                    &score,
                    LIMIT * 2,
                    within.as_deref(),
                    |p, s| {
                        if matches!(s.kind, SymbolKind::Method | SymbolKind::Property)
                            && in_language(self.index, p, language)
                            && let Some(mut i) = item(s, false, query, p, &self.naming)
                        {
                            i.score -= 10;
                            items.push(i);
                        }
                    },
                );
                keep_narrowed(Narrowing::Members, version, language, query, taken);
                items
            }
            true => Vec::new(),
        }
    }

    /// The suggestions for `class`'s members `keep` accepts: the index's (inherited ones
    /// included), then those its language provides itself (built-in types, an enum's own),
    /// each name once.
    fn class_members(&self, class: &str, keep: impl Fn(&Symbol) -> bool) -> Vec<CompletionItem> {
        let static_access = false;
        let members = self.index.members_of(class);
        let mut seen: std::collections::HashSet<(String, SymbolKind)> =
            members.iter().map(|(_, s)| (s.name.to_lowercase(), s.kind)).collect();
        let mut items: Vec<CompletionItem> = members
            .into_iter()
            .filter(|(_, s)| keep(s))
            .filter_map(|(p, s)| item(s, static_access, &self.query, p, &self.naming))
            .collect();
        let types = crate::Types { index: self.index, path: self.path };
        for s in self.intelligence.builtin_members(class, &types) {
            if keep(&s) && seen.insert((s.name.to_lowercase(), s.kind)) {
                items.extend(builtin_item(&s, static_access, &self.query, &self.naming));
            }
        }
        items
    }

    /// The members after `Class::`: static ones, constants and cases, then `class`.
    fn statics(&self, class: &str) -> Vec<CompletionItem> {
        let (index, query) = (self.index, &self.query);
        let class = self.file.resolve_class(class, self.offset, &self.naming);
        let current = self.file.type_at(self.offset).map(|t| t.fqn.clone());
        let own = current.as_deref().is_some_and(|c| {
            c.eq_ignore_ascii_case(&class)
                || index
                    .type_by_fqn(c)
                    .is_some_and(|(_, t)| t.parents.iter().any(|p| p.eq_ignore_ascii_case(&class)))
        });
        let mut items: Vec<CompletionItem> = index
            .members_of(&class)
            .into_iter()
            .filter(|(_, s)| accessible(s, current.as_deref(), own))
            .filter(|(_, s)| {
                s.is_static || matches!(s.kind, SymbolKind::ClassConstant | SymbolKind::EnumCase)
            })
            .filter_map(|(p, s)| item(s, true, query, p, &self.naming))
            .collect();
        // What the language provides itself (an enum's `cases()`, a built-in class's statics).
        let types = crate::Types { index, path: self.path };
        let builtin = self.intelligence.builtin_members(&class, &types);
        for s in builtin.iter().filter(|s| s.is_static || s.kind == SymbolKind::ClassConstant) {
            if !items.iter().any(|i| i.label.eq_ignore_ascii_case(&s.name)) {
                items.extend(builtin_item(s, true, query, &self.naming));
            }
        }
        if let Some(keyword) = self.naming.type_name_member
            && let Some(score) = fuzzy_score(query, keyword)
        {
            items.push(CompletionItem {
                label: keyword.into(),
                kind: CompletionKind::Keyword,
                detail: Some(class),
                insert: keyword.into(),
                cursor_back: 0,
                score: score - 5,
                ..Default::default()
            });
        }
        items
    }

    /// The variables in scope (and in the included files), then the receiver (`$this`), each
    /// written with the language's sigil.
    fn variables(&self) -> Vec<CompletionItem> {
        let (intelligence, project, query) = (self.intelligence, self.project, &self.query);
        let mut names = intelligence.locals(self.text, self.offset);
        // Variables defined in included files (`include 'config.php'`).
        for include in intelligence.includes(self.text) {
            if let Some(file) = project.resolve_include(self.path, &include)
                && let Some(source) = (project.read)(&file)
            {
                for name in intelligence.locals(&source, source.len()) {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
        let sigil = self.naming.sigil.map(String::from).unwrap_or_default();
        if let Some(receiver) = self.naming.receiver {
            names.push(receiver.trim_start_matches(sigil.as_str()).to_owned());
        }
        names
            .into_iter()
            .filter(|n| n != query)
            .filter_map(|n| {
                let score = fuzzy_score(query, &n)?;
                Some(CompletionItem {
                    label: format!("{sigil}{n}"),
                    kind: CompletionKind::Variable,
                    detail: None,
                    insert: format!("{sigil}{n}"),
                    cursor_back: 0,
                    score,
                    ..Default::default()
                })
            })
            .collect()
    }

    /// The namespace written before the name being typed (`App\Models` of `App\Models\Us|`,
    /// a leading separator left out), if any.
    fn qualifier(&self) -> Option<&str> {
        let separator = self.naming.namespace_separator?;
        let start = self.offset - self.query.len();
        let head = self.text.get(..start)?.strip_suffix(separator)?;
        let from = head
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == separator)
            .last()
            .map_or(head.len(), |(i, _)| i);
        let qualifier = head[from..].trim_start_matches(separator);
        (!qualifier.is_empty()).then_some(qualifier)
    }

    /// After a namespace and its separator (`use App\Mo|`): the namespaces in it the project
    /// declares things in, inserted with their separator (the next part comes right after).
    fn namespaces(&self) -> Vec<CompletionItem> {
        let (Some(qualifier), Some(separator)) =
            (self.qualifier(), self.naming.namespace_separator)
        else {
            return Vec::new();
        };
        let prefix = format!("{qualifier}{separator}").to_lowercase();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut items = Vec::new();
        for (_, s) in self.index.top_level_where(|f| of_language(f, self.language.as_str())) {
            let Some(rest) = s.fqn.get(prefix.len()..) else { continue };
            if !s.fqn[..prefix.len()].eq_ignore_ascii_case(&prefix) {
                continue;
            }
            let Some((segment, _)) = rest.split_once(separator) else { continue };
            if segment.is_empty() || !seen.insert(segment.to_lowercase()) {
                continue;
            }
            let Some(score) = fuzzy_score(&self.query, segment) else { continue };
            items.push(CompletionItem {
                label: segment.to_owned(),
                kind: CompletionKind::Namespace,
                detail: Some(format!("{qualifier}{separator}{segment}")),
                insert: format!("{segment}{separator}"),
                score: score + 1,
                ..Default::default()
            });
        }
        items
    }

    /// Where only a type fits: the language's types.
    fn type_names(&self) -> Vec<CompletionItem> {
        self.best_items(Narrowing::Types, |s| s.kind.is_type())
    }

    /// The language's types and functions `keep` takes that can be among the suggestions
    /// shown: each scored first (its name, as [`item`] scores it, its file's being a
    /// dependency's known with the index), only the best made into suggestions — not every
    /// type and function of a project and its libraries (tens of thousands match a letter)
    /// made, copied and sorted at each key. A few times the most shown: room for those the
    /// list then merges (the same name and kind).
    fn best_items(&self, what: Narrowing, keep: impl Fn(&Symbol) -> bool) -> Vec<CompletionItem> {
        let (index, language) = (self.index, self.language.as_str());
        let query = self.query.as_str();
        // The project's, all of them (they change between keys: the file typed in).
        let mut scored: Vec<(i32, &Path, &Symbol)> = index
            .project_top_level(|f| of_language(f, language))
            .filter(|(_, s)| keep(s))
            .filter_map(|(p, s)| {
                let score = fuzzy_score(query, &s.name)?;
                Some((if is_vendor(p) { score - 15 } else { score }, p, s))
            })
            .collect();
        // The libraries': those the word before this one matched, when it goes on from it.
        let version = index.library_version();
        let within = narrowed(what, version, language, query);
        let mut places = Vec::new();
        let mut take = |place: u32| {
            let (p, s, dependency, file) = index.library_top_at(place)?;
            if !of_language(file, language) || !keep(s) {
                return None;
            }
            let score = fuzzy_score(query, &s.name)?;
            places.push(place);
            Some((if dependency { score - 15 } else { score }, p, s))
        };
        match &within {
            Some(within) => scored.extend(within.iter().filter_map(|&place| take(place))),
            None => scored.extend((0..index.library_top_count() as u32).filter_map(take)),
        }
        keep_narrowed(what, version, language, query, places);
        let most = LIMIT * 3;
        if scored.len() > most {
            scored.select_nth_unstable_by(most, |a, b| {
                (b.0)
                    .cmp(&a.0)
                    .then(a.2.name.len().cmp(&b.2.name.len()))
                    .then(a.2.name.cmp(&b.2.name))
            });
            scored.truncate(most);
        }
        scored
            .into_iter()
            .filter_map(|(_, p, s)| item(s, false, &self.query, p, &self.naming))
            .collect()
    }

    /// A name: types, functions, constants and globals, then the keywords.
    fn names(&self) -> Vec<CompletionItem> {
        let mut items = self.best_items(Narrowing::Names, |s| {
            s.kind.is_type()
                || matches!(
                    s.kind,
                    SymbolKind::Function | SymbolKind::Constant | SymbolKind::Variable
                )
        });
        items.extend(self.keywords());
        items
    }

    /// The language's keywords available at the project's level.
    fn keywords(&self) -> Vec<CompletionItem> {
        let (intelligence, project, query) = (self.intelligence, self.project, &self.query);
        intelligence
            .keywords_for(self.path)
            .iter()
            .filter(|k| project.available(intelligence, k))
            .filter_map(|k| {
                let score = fuzzy_score(query, k)?;
                let (insert, cursor_back) = keyword_insert(k, intelligence.keyword_insert(k));
                Some(CompletionItem {
                    label: (*k).into(),
                    kind: CompletionKind::Keyword,
                    detail: None,
                    insert,
                    cursor_back,
                    // A capitalized prefix is almost certainly looking for a class.
                    score: if query.starts_with(char::is_uppercase) {
                        score - 12
                    } else {
                        score + 2
                    },
                    ..Default::default()
                })
            })
            .collect()
    }

    /// The declarations of the given kinds (selectors, mixins…).
    fn symbols(&self, kinds: &[SymbolKind]) -> Vec<CompletionItem> {
        self.index
            .top_level()
            .filter(|(_, s)| kinds.contains(&s.kind))
            .filter_map(|(p, s)| item(s, false, &self.query, p, &self.naming))
            .collect()
    }

    /// The entries of directory `dir` (a path being written).
    fn paths(&self, dir: &str) -> Vec<CompletionItem> {
        let query = &self.query;
        self.project
            .entries(self.path, dir)
            .into_iter()
            .filter(|(name, _)| !name.starts_with('.') || query.starts_with('.'))
            .filter_map(|(name, is_dir)| {
                let score = fuzzy_score(query, &name)? + if is_dir { 2 } else { 0 };
                let insert = if is_dir { format!("{name}/") } else { name };
                Some(CompletionItem {
                    label: insert.clone(),
                    kind: if is_dir { CompletionKind::Selector } else { CompletionKind::Value },
                    detail: Some(i18n::t(if is_dir {
                        "completion.directory"
                    } else {
                        "completion.file"
                    })),
                    insert,
                    cursor_back: 0,
                    score,
                    ..Default::default()
                })
            })
            .collect()
    }

    /// The language's own words for the context (CSS values, HTML attributes…), available at
    /// the project's level and matching the prefix.
    pub(super) fn vocabulary(
        &self,
        context: &CompletionContext,
    ) -> impl Iterator<Item = CompletionItem> + '_ {
        let (intelligence, project, query) = (self.intelligence, self.project, &self.query);
        intelligence
            .vocabulary(context, self.text)
            .into_iter()
            .filter(move |w| project.available(intelligence, &w.label))
            .filter_map(move |mut word| {
                word.score += fuzzy_score(query, &word.label)?;
                Some(word)
            })
    }
}
