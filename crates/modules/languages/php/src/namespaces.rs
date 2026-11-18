//! PHP's namespaces, for moving classes (the `refactor` domain's [`NamespaceRules`]): a
//! light lexer finds each `namespace` block, its `use` imports (grouped ones too), the
//! class names used in code and in doc comments (`@param User $user`), and the strings —
//! comments, heredocs and the HTML around `<?php … ?>` skipped. Names are kept liberally
//! (a constant may look like a class): the refactoring only changes those that resolve to
//! a moved class or, in a moved file, to a known type.

use std::ops::Range;
use std::path::Path;

use refactor::moving::{
    Block, FileScan, Import, ImportGroup, NameRef, NameSeparator, NamespaceRoots, NamespaceRules,
};

/// `App\Models\User`.
const SEPARATOR: NameSeparator = NameSeparator('\\');

#[derive(Debug, Default)]
pub struct PhpNamespaces;

impl NamespaceRules for PhpNamespaces {
    fn extensions(&self) -> &'static [&'static str] {
        &["php", "phtml", "inc"]
    }

    fn scan(&self, text: &str) -> FileScan {
        let (tokens, strings) = lex(text);
        let mut scan = parse(text, &tokens);
        scan.strings = strings;
        scan
    }

    fn separator(&self) -> NameSeparator {
        SEPARATOR
    }

    fn aliased(&self, fqn: &str, alias: &str) -> String {
        format!("{fqn} as {alias}")
    }

    fn import_statement(&self, fqn: &str) -> String {
        format!("use {fqn};")
    }

    fn namespace_statement(&self, namespace: &str) -> String {
        format!("namespace {namespace};")
    }

    fn builtin_type(&self, fqn: &str) -> bool {
        !fqn.contains('\\') && crate::stubs::get().class(fqn).is_some()
    }

    fn roots(&self, root: &Path, read: &dyn Fn(&Path) -> Option<String>) -> NamespaceRoots {
        let Some(json) = read(&root.join("composer.json")) else {
            return NamespaceRoots::new(SEPARATOR, []);
        };
        let mappings = crate::intelligence::psr4(&json);
        NamespaceRoots::new(
            SEPARATOR,
            mappings.iter().map(|(prefix, dir)| {
                (prefix.clone(), root.join(dir.trim_start_matches("./").trim_end_matches('/')))
            }),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A name (keywords included), maybe qualified, maybe with a leading `\`.
    Name,
    /// A class name in a doc comment.
    DocName,
    Variable,
    String,
    /// `->`, `?->`.
    Arrow,
    /// `::`.
    Scope,
    /// `#[`.
    Attribute,
    /// `<?php`, `<?=`, `?>`.
    Tag,
    Punct(u8),
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    range: Range<usize>,
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_name_char(b: u8) -> bool {
    is_name_start(b) || b.is_ascii_digit()
}

/// The end of the name starting at `i` (a `\` continues it when a name follows).
fn name_end(bytes: &[u8], mut i: usize) -> usize {
    loop {
        while i < bytes.len() && is_name_char(bytes[i]) {
            i += 1;
        }
        if i + 1 < bytes.len() && bytes[i] == b'\\' && is_name_start(bytes[i + 1]) {
            i += 1;
        } else {
            return i;
        }
    }
}

/// Where `needle` is next, from `from` (ASCII case-insensitive).
fn find_ci(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    (from..=bytes.len().checked_sub(needle.len())?)
        .find(|&i| bytes[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// The tokens of the PHP code, and the string literals' contents.
fn lex(text: &str) -> (Vec<Token>, Vec<Range<usize>>) {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut strings = Vec::new();
    let mut i = 0;
    let mut code = false;
    while i < bytes.len() {
        if !code {
            // HTML until `<?php` / `<?=` / `<?`.
            let Some(at) = find_ci(bytes, i, b"<?") else { break };
            let len = if bytes[at..].len() >= 5 && bytes[at..at + 5].eq_ignore_ascii_case(b"<?php")
            {
                5
            } else if bytes.get(at + 2) == Some(&b'=') {
                3
            } else {
                2
            };
            tokens.push(Token { kind: Kind::Tag, range: at..at + len });
            i = at + len;
            code = true;
            continue;
        }
        let b = bytes[i];
        let next = bytes.get(i + 1).copied().unwrap_or(0);
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'?' if next == b'>' => {
                tokens.push(Token { kind: Kind::Tag, range: i..i + 2 });
                i += 2;
                code = false;
            }
            b'#' if next == b'[' => {
                tokens.push(Token { kind: Kind::Attribute, range: i..i + 2 });
                i += 2;
            }
            b'#' => i = line_comment_end(bytes, i),
            b'/' if next == b'/' => i = line_comment_end(bytes, i),
            b'/' if next == b'*' => {
                let end = find_ci(bytes, i + 2, b"*/").map_or(bytes.len(), |e| e + 2);
                if bytes.get(i + 2) == Some(&b'*') {
                    doc_names(text, i..end, &mut tokens);
                }
                i = end;
            }
            b'\'' | b'"' | b'`' => {
                let end = string_end(bytes, i + 1, b);
                if b != b'`' {
                    strings.push(i + 1..end.saturating_sub(1).max(i + 1));
                }
                tokens.push(Token { kind: Kind::String, range: i..end });
                i = end;
            }
            b'<' if bytes[i..].starts_with(b"<<<") => {
                let end = heredoc_end(bytes, i + 3);
                tokens.push(Token { kind: Kind::String, range: i..end });
                i = end;
            }
            b'$' if is_name_start(next) => {
                let end =
                    (i + 1..bytes.len()).find(|&j| !is_name_char(bytes[j])).unwrap_or(bytes.len());
                tokens.push(Token { kind: Kind::Variable, range: i..end });
                i = end;
            }
            b'\\' if is_name_start(next) => {
                let end = name_end(bytes, i + 1);
                tokens.push(Token { kind: Kind::Name, range: i..end });
                i = end;
            }
            _ if is_name_start(b) => {
                let end = name_end(bytes, i);
                tokens.push(Token { kind: Kind::Name, range: i..end });
                i = end;
            }
            _ if b.is_ascii_digit() => {
                while i < bytes.len() && (is_name_char(bytes[i]) || bytes[i] == b'.') {
                    i += 1;
                }
            }
            b'-' if next == b'>' => {
                tokens.push(Token { kind: Kind::Arrow, range: i..i + 2 });
                i += 2;
            }
            b'?' if next == b'-' && bytes.get(i + 2) == Some(&b'>') => {
                tokens.push(Token { kind: Kind::Arrow, range: i..i + 3 });
                i += 3;
            }
            b':' if next == b':' => {
                tokens.push(Token { kind: Kind::Scope, range: i..i + 2 });
                i += 2;
            }
            _ => {
                tokens.push(Token { kind: Kind::Punct(b), range: i..i + 1 });
                i += 1;
            }
        }
    }
    (tokens, strings)
}

/// The end of a `//` / `#` comment: the line's end, or a `?>` before it.
fn line_comment_end(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        if bytes[i] == b'?' && bytes.get(i + 1) == Some(&b'>') {
            return i;
        }
        i += 1;
    }
    i
}

/// Just past the closing `quote` (escapes skipped).
fn string_end(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Just past a heredoc / nowdoc whose label starts at `i` (after `<<<`).
fn heredoc_end(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i < bytes.len() && (bytes[i] == b'\'' || bytes[i] == b'"') {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && is_name_char(bytes[i]) {
        i += 1;
    }
    let label = &bytes[start..i];
    if label.is_empty() {
        return i;
    }
    // The closing label: first on a line (indented or not), not followed by a name char.
    let mut at = bytes[i..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |p| i + p);
    while at < bytes.len() {
        let mut j = at + 1;
        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        if bytes[j..].starts_with(label)
            && bytes.get(j + label.len()).is_none_or(|&b| !is_name_char(b))
        {
            return j + label.len();
        }
        at = bytes[j..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |p| j + p);
    }
    bytes.len()
}

/// The class names of a doc comment's tags: the type after `@param`, `@var`, `@return`,
/// `@throws`… (`?User|Collection<int, Post>[]`).
fn doc_names(text: &str, comment: Range<usize>, tokens: &mut Vec<Token>) {
    let bytes = text.as_bytes();
    let mut i = comment.start;
    while let Some(at) = text[i..comment.end].find('@').map(|p| i + p) {
        let mut j = at + 1;
        while j < comment.end && (is_name_char(bytes[j]) || bytes[j] == b'-') {
            j += 1;
        }
        // A tag naming types (not `@author`, not an e-mail address).
        let tag = text[at + 1..j].trim_start_matches("psalm-").trim_start_matches("phpstan-");
        let starts = at == 0 || matches!(bytes[at - 1], b' ' | b'\t' | b'*' | b'{');
        if !starts || !TYPE_TAGS.contains(&tag) {
            i = j.max(at + 1);
            continue;
        }
        while j < comment.end && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        let word_end = (j..comment.end)
            .find(|&k| {
                bytes[k].is_ascii_whitespace()
                    || (bytes[k] == b'*' && bytes.get(k + 1) == Some(&b'/'))
            })
            .unwrap_or(comment.end);
        let mut k = j;
        while k < word_end {
            let lead = bytes[k] == b'\\' && bytes.get(k + 1).is_some_and(|&b| is_name_start(b));
            if is_name_start(bytes[k]) || lead {
                let end = name_end(bytes, if lead { k + 1 } else { k });
                // `$var`, `Foo::bar`'s `bar` and `foo()` aren't classes.
                let after_scope = k >= 2 && &bytes[k - 2..k] == b"::";
                if bytes.get(k.wrapping_sub(1)) != Some(&b'$')
                    && !after_scope
                    && bytes.get(end) != Some(&b'(')
                {
                    tokens.push(Token { kind: Kind::DocName, range: k..end });
                }
                k = end;
            } else {
                k += 1;
            }
        }
        i = word_end.max(at + 1);
    }
}

/// The doc comment tags followed by a type.
const TYPE_TAGS: &[&str] = &[
    "param",
    "var",
    "return",
    "throws",
    "property",
    "property-read",
    "property-write",
    "method",
    "mixin",
    "see",
    "uses",
    "extends",
    "implements",
    "use",
    "template-extends",
    "template-implements",
    "template-use",
    "type",
];

/// Words that are never a class's name.
const KEYWORDS: &[&str] = &[
    "abstract",
    "and",
    "array",
    "as",
    "bool",
    "boolean",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "die",
    "do",
    "double",
    "echo",
    "else",
    "elseif",
    "empty",
    "enddeclare",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "enum",
    "eval",
    "exit",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "fn",
    "for",
    "foreach",
    "from",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "int",
    "integer",
    "interface",
    "isset",
    "iterable",
    "list",
    "match",
    "mixed",
    "namespace",
    "never",
    "new",
    "null",
    "numeric",
    "object",
    "or",
    "parent",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "resource",
    "return",
    "self",
    "static",
    "string",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "unset",
    "use",
    "var",
    "void",
    "while",
    "xor",
    "yield",
    "this",
    "__class__",
    "__dir__",
    "__file__",
    "__function__",
    "__line__",
    "__method__",
    "__namespace__",
    "__trait__",
    "__halt_compiler",
];

fn is_keyword(name: &str) -> bool {
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name))
}

struct Parser<'a> {
    text: &'a str,
    tokens: &'a [Token],
    blocks: Vec<Block>,
    /// A namespace was declared (the first block is no longer the implicit global one).
    declared_namespace: bool,
    depth: usize,
    /// The depth imports are at (1 inside `namespace X { … }`).
    ns_depth: usize,
}

impl Parser<'_> {
    fn word(&self, k: usize) -> Option<&str> {
        let t = self.tokens.get(k)?;
        (t.kind == Kind::Name).then(|| &self.text[t.range.clone()])
    }

    fn is(&self, k: usize, word: &str) -> bool {
        self.word(k).is_some_and(|w| w.eq_ignore_ascii_case(word))
    }

    fn punct(&self, k: usize, p: u8) -> bool {
        self.tokens.get(k).is_some_and(|t| t.kind == Kind::Punct(p))
    }

    /// Token `k` starts a statement.
    fn statement_start(&self, k: usize) -> bool {
        k == 0
            || matches!(
                self.tokens[k - 1].kind,
                Kind::Tag | Kind::Punct(b';') | Kind::Punct(b'{') | Kind::Punct(b'}')
            )
    }

    /// The index just past the `;` ending the statement at `k` (or the last token).
    fn statement_end(&self, k: usize) -> usize {
        (k..self.tokens.len()).find(|&j| self.punct(j, b';')).map_or(self.tokens.len(), |j| j + 1)
    }

    fn block(&mut self) -> &mut Block {
        if self.blocks.is_empty() {
            self.blocks.push(Block::default());
        }
        let last = self.blocks.len() - 1;
        &mut self.blocks[last]
    }

    /// `namespace X;` / `namespace X {` at `k`. Returns the next token.
    fn namespace(&mut self, k: usize) -> usize {
        let start = self.tokens[k].range.start;
        let (name, range) = match self.tokens.get(k + 1) {
            Some(t) if t.kind == Kind::Name => {
                (self.text[t.range.clone()].to_owned(), Some(t.range.clone()))
            }
            _ => (String::new(), None),
        };
        let after = k + 1 + usize::from(range.is_some());
        let braced = self.punct(after, b'{');
        let end = self.tokens.get(after).map_or(self.text.len(), |t| t.range.end);
        let block = Block {
            namespace: name,
            statement: if braced { range.clone() } else { Some(start..end) },
            name_range: range,
            declare_at: start,
            import_at: end,
            ..Block::default()
        };
        let reuse = !self.declared_namespace && {
            let first = &self.blocks[0];
            first.imports.is_empty() && first.names.is_empty() && first.declared.is_empty()
        };
        self.declared_namespace = true;
        if reuse {
            self.blocks[0] = block;
        } else {
            self.blocks.push(block);
        }
        if braced {
            self.depth += 1;
            self.ns_depth = self.depth;
        }
        after + 1
    }

    /// `use A\B [as C], …;` / `use A\{B, C as D};` at `k`. Returns the next token.
    fn import(&mut self, k: usize) -> usize {
        let end = self.statement_end(k);
        let statement = self.tokens[k].range.start..self.tokens[end - 1].range.end;
        if self.is(k + 1, "function") || self.is(k + 1, "const") {
            self.block().import_at = statement.end;
            return end;
        }
        let mut j = k + 1;
        while j < end {
            let Some(t) = self.tokens.get(j).filter(|t| t.kind == Kind::Name) else {
                j += 1;
                continue;
            };
            let name_range = t.range.clone();
            // A group: `Prefix\{…}`.
            if self.punct(j + 1, b'\\') && self.punct(j + 2, b'{') {
                let prefix = self.text[name_range].trim_start_matches('\\').to_owned();
                let group = self.block().groups.len();
                self.block().groups.push(ImportGroup { statement: statement.clone() });
                let mut g = j + 3;
                while g < end && !self.punct(g, b'}') {
                    if self.is(g, "function") || self.is(g, "const") {
                        // Not a class: up to the next item.
                        while g < end && !self.punct(g, b',') && !self.punct(g, b'}') {
                            g += 1;
                        }
                        continue;
                    }
                    if self.tokens[g].kind == Kind::Name {
                        let (import, next) = self.item(g, &prefix, Some(group));
                        self.block().imports.push(import);
                        g = next;
                    } else {
                        g += 1;
                    }
                }
                j = g + 1;
                continue;
            }
            let (import, next) = self.item(j, "", None);
            self.block().imports.push(import);
            j = next;
        }
        self.block().import_at = statement.end;
        end
    }

    /// An imported name at `k` (`B as C`), under `prefix` (a group's).
    fn item(&self, k: usize, prefix: &str, group: Option<usize>) -> (Import, usize) {
        let t = &self.tokens[k];
        let written = &self.text[t.range.clone()];
        let lead = written.len() - written.trim_start_matches('\\').len();
        let name = &written[lead..];
        let fqn = if prefix.is_empty() { name.to_owned() } else { format!("{prefix}\\{name}") };
        let (alias, item_end, next) = if self.is(k + 1, "as")
            && let Some(alias) = self.word(k + 2)
        {
            (alias.to_owned(), self.tokens[k + 2].range.end, k + 3)
        } else {
            (SEPARATOR.short_name(name).to_owned(), t.range.end, k + 1)
        };
        let import = Import {
            fqn,
            alias,
            range: t.range.start + lead..t.range.end,
            item: t.range.start..item_end,
            group,
        };
        (import, next)
    }

    /// A name used at `k`: kept unless it's a keyword, a member, a function called or
    /// declared, a constant declared, an enum case.
    fn name(&mut self, k: usize) {
        let t = &self.tokens[k];
        let written = &self.text[t.range.clone()];
        if is_keyword(written) {
            return;
        }
        let prev = k.checked_sub(1).map(|p| &self.tokens[p]);
        if prev.is_some_and(|p| matches!(p.kind, Kind::Arrow | Kind::Scope)) {
            return;
        }
        let prev_word = k.checked_sub(1).and_then(|p| self.word(p)).map(str::to_ascii_lowercase);
        if matches!(prev_word.as_deref(), Some("function" | "fn" | "const" | "goto" | "case"))
            && !matches!(self.tokens.get(k + 1).map(|t| t.kind), Some(Kind::Scope))
        {
            return;
        }
        // A named argument (`f(name: 1)`, `#[A(name: 'x')]`): not a name used.
        let opens_argument =
            k.checked_sub(1).is_some_and(|p| self.punct(p, b'(') || self.punct(p, b','));
        if opens_argument && self.punct(k + 1, b':') {
            return;
        }
        // `foo(…)` is a call, unless it's `new Foo(…)` or an attribute (`#[Foo(…)]`, the
        // second of `#[A, Foo(…)]`).
        if self.punct(k + 1, b'(')
            && !matches!(prev_word.as_deref(), Some("new"))
            && !prev.is_some_and(|p| p.kind == Kind::Attribute)
            && !crate::intelligence::in_attribute_name(self.text, t.range.start)
        {
            return;
        }
        let fully = written.starts_with('\\');
        let range = t.range.clone();
        self.block().names.push(NameRef {
            range,
            name: written.trim_start_matches('\\').to_owned(),
            fully_qualified: fully,
        });
    }
}

fn parse(text: &str, tokens: &[Token]) -> FileScan {
    // The implicit global block: a namespace or a `use` goes after the opening tag (and a
    // `declare(…);` after it).
    let opening = tokens.first().filter(|t| t.kind == Kind::Tag).map_or(0, |t| t.range.end);
    let global = Block { declare_at: opening, import_at: opening, ..Block::default() };
    let mut p = Parser {
        text,
        tokens,
        blocks: vec![global],
        declared_namespace: false,
        depth: 0,
        ns_depth: 0,
    };
    let mut k = 0;
    while k < tokens.len() {
        let token = &tokens[k];
        match token.kind {
            Kind::Punct(b'{') => p.depth += 1,
            Kind::Punct(b'}') => {
                p.depth = p.depth.saturating_sub(1);
                p.ns_depth = p.ns_depth.min(p.depth);
            }
            Kind::DocName => {
                let written = &text[token.range.clone()];
                if !is_keyword(written) {
                    p.block().names.push(NameRef {
                        range: token.range.clone(),
                        name: written.trim_start_matches('\\').to_owned(),
                        fully_qualified: written.starts_with('\\'),
                    });
                }
            }
            Kind::Name
                if p.statement_start(k) && p.is(k, "namespace") && !p.punct(k + 1, b'\\') =>
            {
                k = p.namespace(k);
                continue;
            }
            Kind::Name if p.statement_start(k) && p.is(k, "declare") && !p.declared_namespace => {
                let end = p.statement_end(k);
                let at = tokens[end - 1].range.end;
                let block = p.block();
                block.declare_at = at;
                block.import_at = block.import_at.max(at);
                k = end;
                continue;
            }
            Kind::Name if p.is(k, "use") && p.depth == p.ns_depth && p.statement_start(k) => {
                k = p.import(k);
                continue;
            }
            Kind::Name
                if ["class", "interface", "trait", "enum"].iter().any(|w| p.is(k, w))
                    && !k.checked_sub(1).is_some_and(|q| tokens[q].kind == Kind::Scope)
                    && p.tokens.get(k + 1).is_some_and(|t| t.kind == Kind::Name) =>
            {
                let name = text[tokens[k + 1].range.clone()].to_owned();
                if !is_keyword(&name) {
                    p.block().declared.push(name);
                }
                k += 2;
                continue;
            }
            Kind::Name => p.name(k),
            _ => {}
        }
        k += 1;
    }
    FileScan { blocks: p.blocks, strings: Vec::new() }
}

#[cfg(test)]
#[path = "../tests/unit/namespaces.rs"]
mod tests;
