//! The hover card: a project declaration described (its signature, its value, its docs),
//! or what the language itself says; signatures written plainly.

use super::{Project, definition, definition_of, kind_of};
use crate::CompletionKind;
use crate::LanguageIntelligence;
use crate::Naming;
use crate::Target;
use crate::index::Location;
use crate::index::SymbolIndex;
use crate::symbols::Symbol;
use crate::symbols::SymbolKind;
use std::path::Path;

/// What is under the mouse, explained: a project declaration (its signature) or, failing
/// that, what the language itself provides (see [`LanguageIntelligence::hover`]).
pub fn hover(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    offset: usize,
    project: &Project<'_>,
) -> Option<crate::HoverInfo> {
    let word = crate::word_at(text, offset, intelligence.word_chars())?;
    let place = intelligence.comment_place(text, offset);
    if place == Some(crate::CommentPlace::Text) {
        return None;
    }
    let declared =
        definition(intelligence, index, path, text, offset, project).into_iter().find_map(|l| {
            let file = index.file(&l.path)?;
            let symbol = file.symbols.iter().find(|s| s.offset == l.offset).cloned()?;
            Some((l, symbol))
        });
    if let Some((at, symbol)) = declared {
        return Some(describe(intelligence, index, (path, text), word, &at, &symbol, project));
    }
    // In a comment, no variable's type: only what the language itself has (a built-in class).
    if place.is_some() {
        return intelligence.hover(text, offset);
    }
    let types = crate::Types { index, path };
    intelligence.typed_hover(text, offset, &types).or_else(|| intelligence.hover(text, offset))
}

/// The hover card of `target`, named at `offset` of `text` (a module found it there: a PHP
/// class in a Twig comment): its declaration, as `intelligence`'s language finds it.
pub fn hover_of(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    offset: usize,
    target: &Target,
    project: &Project<'_>,
) -> Option<crate::HoverInfo> {
    let word = crate::word_at(text, offset, intelligence.word_chars())?;
    let found = definition_of(intelligence, index, path, text, offset, target, project);
    let (at, symbol) = found.into_iter().find_map(|l| {
        let file = index.file(&l.path)?;
        let symbol = file.symbols.iter().find(|s| s.offset == l.offset).cloned()?;
        Some((l, symbol))
    })?;
    Some(describe(intelligence, index, (path, text), word, &at, &symbol, project))
}

/// A project declaration on the hover card: its signature (a constructor's for
/// `new X(`), the declaring type, the doc comment's summary and parameters, its file.
fn describe(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    (path, text): (&Path, &str),
    word: std::ops::Range<usize>,
    at: &Location,
    symbol: &Symbol,
    project: &Project<'_>,
) -> crate::HoverInfo {
    // The declaration's language writes its signature: this text's, when it's declared in
    // it (another's, e.g. a script's function in an HTML page, is written plainly).
    let language = index.file(&at.path).map_or("", |f| &*f.language);
    let own = language.is_empty() || language == intelligence.language().as_str();
    let naming = if own { intelligence.naming() } else { Naming::default() };
    // `new X(`: the constructor, under the class.
    let constructing = symbol.kind.is_type() && intelligence.constructs_at(text, word.start);
    let constructor = constructing
        .then(|| {
            // `new` is written in this text: its language names the constructor.
            let naming = intelligence.naming();
            index.members_of(&symbol.fqn).into_iter().find(|(_, s)| naming.is_constructor(&s.name))
        })
        .flatten()
        .map(|(p, s)| {
            (Location { path: p.to_path_buf(), offset: s.offset, line: s.line }, s.clone())
        });
    let read =
        |file: &Path| if file == path { Some(text.to_owned()) } else { (project.read)(file) };
    let class_at = at;
    let (at, shown) = match &constructor {
        Some((l, s)) => (l, s),
        None => (at, symbol),
    };
    let short = |fqn: &str| naming.short_name(fqn).to_owned();
    let namespace = |fqn: &str| naming.namespace_of(fqn).map(str::to_owned);
    // The header: the declaring type (or the type itself), its namespace dimmed.
    let (title, subtitle) = match &shown.container {
        Some(class) => {
            let kind = index.type_by_fqn(class).map_or(CompletionKind::Class, |(_, t)| kind_of(t));
            (Some((short(class), Some(kind))), namespace(class))
        }
        None if shown.kind.is_type() => {
            (Some((shown.name.to_string(), Some(kind_of(shown)))), namespace(&shown.fqn))
        }
        None => (None, None),
    };
    let modifiers = naming.parameter_modifiers;
    let detail = shown.detail.as_deref().map(|d| strip_modifiers(d, modifiers));
    // The declaring file's language reads it (a CSS rule from a stylesheet, hovered in a
    // page); without the project's languages, this text's.
    let language_of = |file: &Path| project.intelligence_for(file).unwrap_or(intelligence);
    // What the declaration says whole, when its language shows it so (a CSS rule).
    let rule = read(&at.path)
        .and_then(|t| language_of(&at.path).declaration_code(&t, at.offset, shown.kind));
    // The declaring file's language writes its signature; one the project doesn't know
    // (none given) is written plainly.
    let code = rule.unwrap_or_else(|| {
        let detail = detail.as_deref();
        match project.intelligence_for(&at.path) {
            Some(declaring) => declaring.signature(shown, detail),
            None if own => intelligence.signature(shown, detail),
            None => signature_plainly(shown, detail),
        }
    });
    let doc_of = |file: &Path, text: &str, offset: usize| {
        let rules = language_of(file).doc_rules()?;
        crate::doc_before(text, offset, &rules)
    };
    let docs = read(&at.path).and_then(|t| doc_of(&at.path, &t, at.offset));
    // A constructor without its own doc: the class's.
    let summary =
        docs.as_ref().map(|d| d.summary.clone()).filter(|s| !s.is_empty()).or_else(|| {
            constructor.as_ref()?;
            let class_text = read(&class_at.path)?;
            doc_of(&class_at.path, &class_text, class_at.offset)
                .map(|d| d.summary)
                .filter(|s| !s.is_empty())
        });
    // A summary starting with the declaration's own name (a common doc convention)
    // repeats the header: that line goes.
    let summary = summary.map(|s| {
        let names = [&*shown.name, &*symbol.name];
        match s.split_once('\n') {
            Some((first, rest)) if names.contains(&first.trim()) => rest.to_owned(),
            None if names.contains(&s.trim()) => String::new(),
            _ => s,
        }
    });
    let callable = matches!(shown.kind, SymbolKind::Function | SymbolKind::Method);
    let params = callable
        .then(|| crate::parameters(&code))
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            let param = &code[r];
            let (ty, name) =
                if own { intelligence.split_param(param) } else { split_param_plainly(param) };
            let doc = docs.as_ref().and_then(|d| d.param(&name)).map(str::to_owned);
            // No type in the code (JavaScript): the doc comment's.
            let ty = ty.or_else(|| docs.as_ref()?.param_type(&name).map(str::to_owned));
            crate::HoverParam { ty, name, doc }
        })
        .collect();
    // Deprecated: its doc comment says so, or what its language writes on it.
    let deprecated = docs.as_ref().is_some_and(|d| d.deprecated)
        || language_of(&at.path).is_deprecated(shown)
        || (own && intelligence.is_deprecated(shown));
    let notes = if deprecated { vec![crate::HoverNote::Deprecated] } else { Vec::new() };
    // `new X(`: the class's card, its constructor's signature in it.
    let code = match &constructor {
        Some(_) => {
            let class = match project.intelligence_for(&class_at.path) {
                Some(declaring) => declaring.signature(symbol, None),
                None => signature_plainly(symbol, None),
            };
            format!("{class}\n{code}")
        }
        None => code,
    };
    let source = project
        .root
        .and_then(|root| at.path.strip_prefix(root).ok())
        .unwrap_or(&at.path)
        .display()
        .to_string();
    crate::HoverInfo {
        range: word,
        code,
        notes,
        doc: summary.filter(|s| !s.is_empty()).map(|s| shorten(&s, 600)),
        title,
        subtitle,
        params,
        params_label: None,
        source: Some(source),
    }
}

/// A declaration's signature on one line as most languages write it: `name(…)` for a
/// method, `function name(…)`, `class Name`, `const NAME` (see
/// [`LanguageIntelligence::signature`]).
pub fn signature_plainly(s: &Symbol, detail: Option<&str>) -> String {
    let detail = detail.unwrap_or("");
    match s.kind {
        SymbolKind::Method => format!("{}{detail}", s.name),
        SymbolKind::Function if detail.starts_with("=>") => format!("{} {detail}", s.name),
        SymbolKind::Function => format!("function {}{detail}", s.name),
        SymbolKind::Class => format!("class {}", s.name),
        SymbolKind::Interface => format!("interface {}", s.name),
        SymbolKind::Trait => format!("trait {}", s.name),
        SymbolKind::Enum => format!("enum {}", s.name),
        // With its value, when the declaration's language gave it (` = 50`).
        SymbolKind::Constant | SymbolKind::ClassConstant => {
            format!("const {}{}", s.name, value_of(detail))
        }
        SymbolKind::EnumCase => format!("case {}{}", s.name, value_of(detail)),
        _ => format!("{}{detail}", crate::label(s, None)),
    }
}

/// A constant's value as a symbol's detail gives it (` = 50`); anything else: nothing.
fn value_of(detail: &str) -> &str {
    if detail.starts_with(" = ") { detail } else { "" }
}

/// `(public Device $device, private readonly bool $x = true)` → without the parameters'
/// `modifiers` (PHP's promotion: `Device $device, bool $x = true`).
pub(super) fn strip_modifiers(detail: &str, modifiers: &[&str]) -> String {
    let mut out = String::with_capacity(detail.len());
    let mut rest = detail;
    while !rest.is_empty() {
        let at_word_start = out.is_empty() || out.ends_with(['(', ' ', ',']);
        let modifier = modifiers
            .iter()
            .find(|m| rest.strip_prefix(**m).is_some_and(|after| after.starts_with(' ')));
        if at_word_start && let Some(m) = modifier {
            rest = &rest[m.len() + 1..];
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// A parameter's type and name as most languages write it: `name: Type = 1` → (`Type`,
/// `name`) (see [`LanguageIntelligence::split_param`]).
pub fn split_param_plainly(param: &str) -> (Option<String>, String) {
    let param = param.split_once(" = ").map_or(param, |(p, _)| p).trim();
    match param.split_once(':') {
        Some((name, ty)) => (Some(ty.trim().to_owned()), name.trim().to_owned()),
        None => (None, param.to_owned()),
    }
}

/// At most `max` characters, cut at a word.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head))
}
