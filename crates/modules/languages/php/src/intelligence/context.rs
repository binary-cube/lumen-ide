//! What the text before the cursor says: the completion's context, the target of a navigation, the
//! receiver's type, the local variables, a parameter's parts.

use super::*;

pub(super) fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Identifier prefix before `end` (without `\`).
pub(super) fn ident_start(text: &str, end: usize) -> usize {
    text[..end]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c))
        .last()
        .map_or(end, |(i, _)| i)
}

/// Whether the name starting at `start` is an attribute's (`#[AsCommand(…)]`, the second of
/// `#[A, B]`): a class, instantiated through its constructor — not a function called.
pub(crate) fn in_attribute_name(text: &str, start: usize) -> bool {
    let before = text[..start].trim_end();
    if before.ends_with("#[") {
        return true;
    }
    if !before.ends_with(',') {
        return false;
    }
    // After a `,`: in an attribute list if the bracket around it opens with `#[` (not a
    // call's parentheses, not an array). Looked for in what's near only.
    let from = before.len().saturating_sub(4096);
    let from = before.floor_char_boundary(from);
    let mut depth = 0usize;
    for (i, c) in before[from..before.len() - 1].char_indices().rev() {
        match c {
            ')' | ']' => depth += 1,
            '(' if depth == 0 => return false,
            '[' if depth == 0 => return before[..from + i].ends_with('#'),
            '(' | '[' => depth -= 1,
            ';' | '{' | '}' => return false,
            _ => {}
        }
    }
    false
}

/// Class name (with `\`) ending at `end`.
pub(super) fn class_token_before(text: &str, end: usize) -> &str {
    let start = text[..end]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c) || *c == '\\')
        .last()
        .map_or(end, |(i, _)| i);
    &text[start..end]
}

/// Inside a comment or string on the current line (heuristic, enough while typing).
/// `before` ends in a class's (trait's, enum's, interface's) body, not in one of its
/// methods: the innermost open `{` follows a `class` / `trait` / `enum` / `interface`
/// declaration.
pub(super) fn in_class_body(before: &str) -> bool {
    let mut depth = 0usize;
    let Some(open) = before.char_indices().rev().find_map(|(i, c)| match c {
        '}' => {
            depth += 1;
            None
        }
        '{' if depth == 0 => Some(i),
        '{' => {
            depth -= 1;
            None
        }
        _ => None,
    }) else {
        return false;
    };
    let head = &before[..open];
    let head = &head[head.rfind([';', '{', '}']).map_or(0, |i| i + 1)..];
    head.split(|c: char| !is_ident(c)).any(|w| {
        matches!(w.to_ascii_lowercase().as_str(), "class" | "trait" | "enum" | "interface")
    })
}

pub(super) fn in_comment_or_string(before: &str) -> bool {
    if let (Some(open), close) = (before.rfind("/*"), before.rfind("*/"))
        && close.is_none_or(|c| c < open)
    {
        return true;
    }
    let line = before.rsplit('\n').next().unwrap_or(before);
    let (mut single, mut double, mut escaped) = (false, false, false);
    let chars: Vec<char> = line.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '/' if !single && !double && chars.get(i + 1) == Some(&'/') => return true,
            '#' if !single && !double && chars.get(i + 1) != Some(&'[') => return true,
            _ => {}
        }
    }
    single || double
}

/// The object ending at `end` (before `->`).
pub(super) fn receiver_before(text: &str, end: usize) -> Receiver {
    let trimmed = text[..end].trim_end();
    let start = ident_start(trimmed, trimmed.len());
    if start == 0 || !trimmed[..start].ends_with('$') {
        return Receiver::Unknown;
    }
    let var = &trimmed[start..];
    if var == "this" {
        return Receiver::This;
    }
    infer_variable(&text[..start], var).map_or(Receiver::Unknown, Receiver::Class)
}

/// Type of a variable, from the preceding text: `$x = new Foo`, `Foo $x` (parameter),
/// `@var Foo $x`. The last occurrence wins.
pub(super) fn infer_variable(before: &str, var: &str) -> Option<String> {
    // Compiled once, for any variable (its name captured, then compared): not three
    // regexes built at every `->`.
    // Each with where it captures the name and the type.
    static PATTERNS: LazyLock<Vec<(Regex, usize, usize)>> = LazyLock::new(|| {
        let class = r"(\\?[A-Za-z_][\w\\]*)";
        [
            (r"\$(\w+)\s*=\s*new\s+{ty}", 1, 2),
            (r"[(,]\s*\??{ty}\s+&?(?:\.\.\.)?\$(\w+)\b", 2, 1),
            (r"@var\s+\??{ty}\s+\$(\w+)\b", 2, 1),
        ]
        .into_iter()
        .filter_map(|(p, name, ty)| Some((Regex::new(&p.replace("{ty}", class)).ok()?, name, ty)))
        .collect()
    });
    if !before.contains(var) {
        return None;
    }
    PATTERNS
        .iter()
        .filter_map(|(re, name, ty)| {
            re.captures_iter(before)
                .filter(|c| c.get(*name).is_some_and(|m| m.as_str() == var))
                .last()
                .and_then(|c| c.get(*ty))
                .map(|m| (m.start(), m.as_str().to_owned()))
        })
        .max_by_key(|(start, _)| *start)
        .map(|(_, ty)| ty)
        .filter(|t| {
            !matches!(
                t.to_ascii_lowercase().as_str(),
                "int"
                    | "string"
                    | "float"
                    | "bool"
                    | "array"
                    | "mixed"
                    | "callable"
                    | "iterable"
                    | "object"
            )
        })
}

/// The type written for the variable at `offset` (the cursor on it, or on its `$`): its
/// `new`, its parameter's hint, its `@var` — the last before it (its own declaration
/// included).
pub(super) fn type_at(text: &str, offset: usize) -> Option<String> {
    let mut offset = text.floor_char_boundary(offset.min(text.len()));
    if text[offset..].starts_with('$') {
        offset += 1;
    }
    let start = ident_start(text, offset);
    let end = offset + text[offset..].find(|c: char| !is_ident(c)).unwrap_or(text.len() - offset);
    if start == 0 || start == end || !text[..start].ends_with('$') {
        return None;
    }
    let var = &text[start..end];
    if var == "this" {
        return None;
    }
    // Its own line counted (`$big = new Big();` with the cursor on `$big`).
    let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
    infer_variable(&text[..line_end], var)
}

/// The string of an `include` / `require` the cursor is in: its content up to the cursor.
pub(super) fn include_string(text: &str, offset: usize) -> Option<(usize, &str)> {
    static OPEN: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r#"\b(?:include|require)(?:_once)?\s*\(?\s*(?:__DIR__\s*\.\s*)?['"]([^'"\n]*)$"#)
            .ok()
    });
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let captures = OPEN.as_ref()?.captures(&text[line_start..offset])?;
    let content = captures.get(1)?;
    Some((line_start + content.start(), content.as_str()))
}

pub(super) fn context(text: &str, offset: usize) -> CompletionContext {
    let offset = text.floor_char_boundary(offset);
    let before = &text[..offset];
    // Path in `include '…'`: entries of the directory typed so far.
    if let Some((start, typed)) = include_string(text, offset) {
        let split = typed.rfind('/').map_or(0, |i| i + 1);
        return CompletionContext {
            replace: start + split..offset,
            prefix: typed[split..].to_owned(),
            kind: ContextKind::Path(typed[..split].to_owned()),
        };
    }
    // In doc blocks: tags, types, parameters.
    if let Some(context) = docblock::context(text, offset) {
        return context;
    }
    let start = ident_start(text, offset);
    let prefix = text[start..offset].to_owned();
    let none = CompletionContext {
        replace: offset..offset,
        prefix: String::new(),
        kind: ContextKind::None,
    };
    // In a callable's string (`[$this, '|']`): the class's methods.
    if let Some(context) = crate::callables::with_callable(text, offset, |_, c| {
        let receiver = match &c.class {
            crate::callables::Class::Object(_) => Receiver::Unknown,
            class => Receiver::Class(crate::callables::written_class(text, class)),
        };
        Some(CompletionContext {
            replace: c.name.start..offset,
            prefix: text.get(c.name.start..offset)?.to_owned(),
            kind: ContextKind::Methods(receiver),
        })
    }) {
        return context;
    }
    if in_comment_or_string(before) || prefix.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return none;
    }
    let pre = &text[..start];
    let kind = if pre.ends_with('$') {
        return CompletionContext {
            replace: start - 1..offset,
            prefix,
            kind: ContextKind::Variable,
        };
    } else if let Some(rest) = pre.strip_suffix("?->").or_else(|| pre.strip_suffix("->")) {
        ContextKind::Member(receiver_before(text, rest.len()))
    } else if let Some(rest) = pre.strip_suffix("::") {
        let class = class_token_before(text, rest.len());
        if class.is_empty() {
            return none;
        }
        ContextKind::Static(class.to_owned())
    } else if prefix.is_empty()
        && !(pre.ends_with('\\') && pre[..pre.len() - 1].ends_with(is_ident))
    {
        // Nothing typed (but after a namespace's `\`: its namespaces are offered).
        return none;
    } else {
        let previous = pre.trim_end().rsplit(|c: char| !is_ident(c)).next().unwrap_or("");
        // An attribute's name (`#[AsC`, `#[ORM\Col`): only classes.
        let name_start = start - class_token_before(text, start).len();
        if in_attribute_name(text, name_start) {
            return CompletionContext {
                replace: start..offset,
                prefix,
                kind: ContextKind::TypeName,
            };
        }
        match previous.to_ascii_lowercase().as_str() {
            "new" | "extends" | "implements" | "instanceof" | "catch" => ContextKind::TypeName,
            // A name being declared: a method's may be one PHP calls by itself; nothing to
            // suggest for a function's.
            "function" if in_class_body(before) => ContextKind::Vocabulary(METHOD_NAME),
            "function" => return none,
            _ => ContextKind::Name,
        }
    };
    CompletionContext { replace: start..offset, prefix, kind }
}

pub(super) fn target(text: &str, offset: usize) -> Option<Target> {
    let offset = text.floor_char_boundary(offset);
    // Path in `include '…'`: the whole string (including what follows the cursor).
    if let Some((start, _)) = include_string(text, offset) {
        let end = text[offset..].find(['\'', '"', '\n']).map_or(text.len(), |i| offset + i);
        return Some(Target::File(text[start..end].to_owned()));
    }
    // A method named in a callable's string (`[$this, 'cart']`, `'App\Shop::cart'`).
    if let Some(target) = crate::callables::with_callable(text, offset, |_, c| {
        let name = text.get(c.name.clone()).filter(|n| !n.is_empty())?.to_owned();
        Some(Target::Static { class: crate::callables::written_class(text, &c.class), name })
    }) {
        return Some(target);
    }
    // In a string: a class only where PHP takes the string as a class's name.
    if let Some(class) = crate::callables::class_in_string(text, offset) {
        return class.map(Target::Class);
    }
    // The word under the cursor (with `\` for qualified names).
    let word_char = |c: char| is_ident(c) || c == '\\';
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| word_char(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = text[offset..]
        .char_indices()
        .find(|(_, c)| !word_char(*c))
        .map_or(text.len(), |(i, _)| offset + i);
    let word = text[start..end].trim_matches('\\');
    if word.is_empty() || text[..start].ends_with('$') {
        return None;
    }
    let pre = &text[..start];
    let next = text[end..].trim_start().chars().next();
    if let Some(rest) = pre.strip_suffix("?->").or_else(|| pre.strip_suffix("->")) {
        return Some(Target::Member {
            receiver: receiver_before(text, rest.len()),
            name: word.to_owned(),
        });
    }
    if let Some(rest) = pre.strip_suffix("::") {
        let class = class_token_before(text, rest.len()).to_owned();
        if word.eq_ignore_ascii_case("class") {
            return Some(Target::Class(class));
        }
        return Some(Target::Static { class, name: word.to_owned() });
    }
    // `use App\Models\User;` at line start: the name is already complete (`use function`:
    // a function's, `use const`: a constant's).
    let line = pre.rsplit('\n').next().unwrap_or(pre).trim_start();
    if line.starts_with("use ") && !line.contains('(') {
        let rest = line["use ".len()..].trim_start();
        if rest.starts_with("function ") {
            return Some(Target::Function(format!("\\{word}")));
        }
        if rest.starts_with("const ") {
            let name = word.rsplit('\\').next().unwrap_or(word).to_owned();
            return Some(Target::Named { name, kinds: vec![SymbolKind::Constant] });
        }
        // In a class's body, `use Trait;` names a trait as the file resolves it; an
        // import's name is a full one.
        if in_class_body(pre) {
            return Some(Target::Class(word.to_owned()));
        }
        return Some(Target::Class(format!("\\{word}")));
    }
    // A method being declared (`function name(` in a class): itself, as its class's member.
    if pre.trim_end().ends_with("function") && in_class_body(pre) {
        return Some(Target::Member { receiver: Receiver::This, name: word.to_owned() });
    }
    // `new X(` and `#[X(` make an object of class X: not a function call.
    let after_new = pre.trim_end().ends_with("new") || in_attribute_name(text, start);
    if !after_new && names_constant(word, pre, next) {
        let name = word.rsplit('\\').next().unwrap_or(word).to_owned();
        return Some(Target::Named { name, kinds: vec![SymbolKind::Constant] });
    }
    if next == Some('(') && !after_new {
        return Some(Target::Function(text[start..end].to_owned()));
    }
    Some(Target::Class(text[start..end].to_owned()))
}

/// Whether `word` (written after `pre`, `next` after it) names a constant — `LIMIT`,
/// `App\\LIMIT` — rather than a class: upper case, not where a type is written (`new X`,
/// `instanceof X`, `extends X`, `X $param`, `): X`, `X::`), not declared here (`const X` in a
/// class is the class's).
fn names_constant(word: &str, pre: &str, next: Option<char>) -> bool {
    let last = word.rsplit('\\').next().unwrap_or(word);
    let upper = last.chars().any(char::is_alphabetic) && !last.chars().any(char::is_lowercase);
    if !upper || matches!(next, Some('(' | ':' | '$' | '\\' | '&')) {
        return false;
    }
    let head = pre.trim_end();
    let typed = ["new", "instanceof", "extends", "implements", "insteadof", "const", "case", "):"]
        .iter()
        .any(|w| head.ends_with(w))
        || head.ends_with("function")
        || head.ends_with("enum");
    !typed
}

pub(super) fn locals(text: &str, offset: usize) -> Vec<String> {
    static VAR: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").ok());
    let offset = text.floor_char_boundary(offset);
    let scope_start = text[..offset].rfind("function").unwrap_or(0);
    let mut names: Vec<String> = Vec::new();
    if let Some(re) = &*VAR {
        for c in re.captures_iter(&text[scope_start..offset]) {
            let name = &c[1];
            if name != "this" && !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

/// A parameter's type and name: the type is what's before the `$variable`.
pub(super) fn split_param(param: &str) -> (Option<String>, String) {
    let param = param.split_once(" = ").map_or(param, |(p, _)| p).trim();
    let at = param.find('$').map(|i| if param[..i].ends_with("...") { i - 3 } else { i });
    match at {
        Some(i) => {
            let ty = param[..i].trim().trim_end_matches('&').trim();
            ((!ty.is_empty()).then(|| ty.to_owned()), param[i..].to_owned())
        }
        None => (None, param.to_owned()),
    }
}

/// The place's surroundings fit the symbol's kind: a property after `->` (or `self::$`), not
/// a `$variable`; a method after `->` / `::` or `function`; a constant / case after `::`,
/// `const`, `case`; a function called, not a member.
pub(super) fn fits_kind(text: &str, range: &std::ops::Range<usize>, kind: SymbolKind) -> bool {
    // Text in a string or a comment isn't a use (a doc comment's types are) — but a method
    // named in a callable's string is (`[$this, 'cart']`).
    if in_string_or_comment(text, range.start) {
        return kind == SymbolKind::Method
            && crate::callables::with_callable(text, range.start, |_, c| {
                (c.name == *range).then_some(())
            })
            .is_some();
    }
    let head = text[..range.start].trim_end_matches([' ', '\t']);
    // `use function App\Support\format_name;`, `use const App\LIMIT;`.
    let line = head.rsplit('\n').next().unwrap_or(head).trim_start();
    let imported = |what: &str| {
        line.strip_prefix("use ").is_some_and(|rest| rest.trim_start().starts_with(what))
    };
    if imported("function ") {
        return kind == SymbolKind::Function;
    }
    if imported("const ") {
        return kind == SymbolKind::Constant;
    }
    let tail = text[range.end..].trim_start();
    let member_access = head.ends_with("->") || head.ends_with("::");
    let dollar = text[..range.start].ends_with('$');
    let called = tail.starts_with('(');
    match kind {
        // `$this->name`, `self::$name`, but not a `$name` variable.
        // `$this->name`, `self::$name`, but not a `$name` variable, nor a method called
        // `->name(`.
        SymbolKind::Property => {
            if dollar {
                text[..range.start - 1].ends_with("::")
            } else {
                member_access && !called
            }
        }
        // `->name(`, `::name(`, `->name(...)`, `function name(`: not a property `->name`.
        SymbolKind::Method => (member_access && called) || head.ends_with("function"),
        SymbolKind::ClassConstant | SymbolKind::EnumCase => {
            head.ends_with("::") || head.ends_with("const") || head.ends_with("case")
        }
        SymbolKind::Function => {
            !member_access && !dollar && (tail.starts_with('(') || head.ends_with("function"))
        }
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Trait | SymbolKind::Enum => {
            !head.ends_with("->") && !dollar
        }
        _ => !dollar,
    }
}

/// Whether `offset` is in a string's text or in a comment other than a doc comment
/// (`/** … */`), by the text's tree (what a string interpolates, `{$user->name}`, is code).
pub(super) fn in_string_or_comment(text: &str, offset: usize) -> bool {
    // What the line says first (no quote nor comment before it on its line, no comment
    // open: code), the tree only when it may be one.
    if !in_comment_or_string(&text[..offset]) && !multiline_string_open(&text[..offset]) {
        return false;
    }
    let Some(parsed) = crate::doctree::parsed(text) else { return false };
    let root = parsed.tree.root_node();
    let Some(node) = root.descendant_for_byte_range(offset, offset + 1) else { return false };
    let named = std::iter::successors(Some(node), |n| n.parent()).find(|n| n.is_named());
    match named.map(|n| (n.kind(), n)) {
        Some(("string_content" | "string_value" | "nowdoc_string", _)) => true,
        Some(("comment", n)) => !text.get(n.byte_range()).is_some_and(|c| c.starts_with("/**")),
        _ => false,
    }
}

/// Whether a string may be open across lines before the end of `before` (a heredoc, a
/// quote left open on an earlier line): the line alone can't tell.
fn multiline_string_open(before: &str) -> bool {
    before.contains("<<<") || before.matches(['\'', '"']).count() % 2 == 1
}
