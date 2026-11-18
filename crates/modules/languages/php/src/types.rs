//! PHP types as the inference works with them: what the code writes (`?User`, `A|B`) and what
//! PHPDoc writes (`Collection<int, User>`, `list<User>`, `array{id: int, user: User}`,
//! `class-string<T>`, `callable(TValue): TMapped`, `($x is T ? A : B)`), read by our own
//! parser. Unknown syntax reads as `mixed`, never as an error.

use code_intelligence::FileSymbols;

/// A type.
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    /// A class (interface, enum…) by its full name, with its generic arguments.
    Named(Box<str>, Vec<Ty>),
    /// A template parameter (`TValue`), replaced when the type is used.
    Template(Box<str>),
    /// `static` / `$this` (with arguments: `static<TKey, TMapValue>`): the receiver.
    Static(Vec<Ty>),
    /// `self`: the declaring class.
    SelfType,
    /// `array<K, V>`, `list<V>`, `V[]`, `iterable<K, V>`: the keys' and the values' types.
    Array(Box<Ty>, Box<Ty>),
    /// `array{id: int, user: User}`.
    Shape(Vec<(Box<str>, Ty)>),
    /// `class-string<T>`: the name of a class of type `T`.
    ClassString(Box<Ty>),
    /// `callable(A, B): R`, `Closure(A): R`: the parameters' and the result's types.
    Callable(Vec<Ty>, Box<Ty>),
    /// `int`, `string`, `null`, `void`, `bool`…
    Scalar(Box<str>),
    Union(Vec<Ty>),
    /// `($x is T ? A : B)`: either, depending on an argument.
    Conditional(Box<Ty>, Box<Ty>),
    Mixed,
}

impl Ty {
    pub fn named(fqn: &str) -> Self {
        Self::Named(fqn.trim_start_matches('\\').into(), Vec::new())
    }

    pub fn null() -> Self {
        Self::Scalar("null".into())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Scalar(s) if &**s == "null")
    }

    /// Whether nothing is known.
    pub fn is_mixed(&self) -> bool {
        matches!(self, Self::Mixed)
    }

    /// Both, as one type (`A|B`; a union isn't nested, the same type isn't repeated).
    pub fn union(a: Ty, b: Ty) -> Ty {
        let mut parts = Vec::new();
        for t in [a, b] {
            match t {
                Ty::Union(inner) => parts.extend(inner),
                other => parts.push(other),
            }
        }
        let mut out: Vec<Ty> = Vec::new();
        for p in parts {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        // Something unknown in a union: only the classes known of it still help
        // (`mixed|null` is still `mixed`).
        if out.len() > 1 && out.iter().any(Ty::is_mixed) {
            out.retain(|t| !t.classes().is_empty() || matches!(t, Ty::Array(..) | Ty::Shape(_)));
        }
        match out.len() {
            0 => Ty::Mixed,
            1 => out.remove(0),
            _ => Ty::Union(out),
        }
    }

    /// Without `null` (`?User` → `User`).
    pub fn without_null(self) -> Ty {
        match self {
            Ty::Union(parts) => {
                let mut kept: Vec<Ty> = parts.into_iter().filter(|t| !t.is_null()).collect();
                match kept.len() {
                    0 => Ty::Mixed,
                    1 => kept.remove(0),
                    _ => Ty::Union(kept),
                }
            }
            other => other,
        }
    }

    /// The classes it can be (a union's each; `null` and scalars left out).
    pub fn classes(&self) -> Vec<(&str, &[Ty])> {
        match self {
            Ty::Named(fqn, args) => vec![(&**fqn, args.as_slice())],
            Ty::Union(parts) => parts.iter().flat_map(Ty::classes).collect(),
            Ty::Conditional(a, b) => {
                let mut out = a.classes();
                out.extend(b.classes());
                out
            }
            _ => Vec::new(),
        }
    }

    /// The first class it can be: the one whose members are offered.
    pub fn class(&self) -> Option<&str> {
        self.classes().into_iter().next().map(|(c, _)| c)
    }

    /// The type of its elements when iterated (`foreach`) or indexed: an array's values, a
    /// generic collection's last argument (`Collection<int, User>` → `User`).
    pub fn element(&self) -> Ty {
        match self {
            Ty::Array(_, value) => (**value).clone(),
            Ty::Shape(fields) => {
                fields.iter().map(|(_, t)| t.clone()).reduce(Ty::union).unwrap_or(Ty::Mixed)
            }
            Ty::Named(_, args) if !args.is_empty() => args.last().cloned().unwrap_or(Ty::Mixed),
            Ty::Union(parts) => parts
                .iter()
                .filter(|t| !t.is_null())
                .map(Ty::element)
                .reduce(Ty::union)
                .unwrap_or(Ty::Mixed),
            _ => Ty::Mixed,
        }
    }

    /// The type of its keys when iterated.
    pub fn key(&self) -> Ty {
        match self {
            Ty::Array(key, _) => (**key).clone(),
            Ty::Named(_, args) if args.len() >= 2 => args[0].clone(),
            _ => Ty::Mixed,
        }
    }

    /// The value at `key` (`$row['user']`): a shape's field, else its element.
    pub fn at_key(&self, key: Option<&str>) -> Ty {
        match (self, key) {
            (Ty::Shape(fields), Some(key)) => {
                fields.iter().find(|(k, _)| &**k == key).map_or(Ty::Mixed, |(_, t)| t.clone())
            }
            (Ty::Union(parts), _) => parts
                .iter()
                .filter(|t| !t.is_null())
                .map(|t| t.at_key(key))
                .reduce(Ty::union)
                .unwrap_or(Ty::Mixed),
            _ => self.element(),
        }
    }

    /// Every part replaced by `map` where it answers (bottom-up).
    pub fn map(&self, map: &dyn Fn(&Ty) -> Option<Ty>) -> Ty {
        if let Some(t) = map(self) {
            return t;
        }
        let each = |v: &[Ty]| v.iter().map(|t| t.map(map)).collect::<Vec<_>>();
        match self {
            Ty::Named(fqn, args) => Ty::Named(fqn.clone(), each(args)),
            Ty::Static(args) => Ty::Static(each(args)),
            Ty::Array(k, v) => Ty::Array(Box::new(k.map(map)), Box::new(v.map(map))),
            Ty::Shape(fields) => {
                Ty::Shape(fields.iter().map(|(k, t)| (k.clone(), t.map(map))).collect())
            }
            Ty::ClassString(t) => Ty::ClassString(Box::new(t.map(map))),
            Ty::Callable(params, ret) => Ty::Callable(each(params), Box::new(ret.map(map))),
            Ty::Union(parts) => {
                parts.iter().map(|t| t.map(map)).reduce(Ty::union).unwrap_or(Ty::Mixed)
            }
            Ty::Conditional(a, b) => Ty::Conditional(Box::new(a.map(map)), Box::new(b.map(map))),
            other => other.clone(),
        }
    }

    /// Whether it says anything of a class (a class, a template, `static`, a shape or an
    /// array holding one): what the inference can use.
    pub fn mentions_class(&self) -> bool {
        match self {
            Ty::Named(..) | Ty::Template(_) | Ty::Static(_) | Ty::SelfType => true,
            Ty::Array(k, v) => k.mentions_class() || v.mentions_class(),
            Ty::Shape(fields) => fields.iter().any(|(_, t)| t.mentions_class()),
            Ty::ClassString(t) => t.mentions_class(),
            Ty::Callable(params, ret) => {
                ret.mentions_class() || params.iter().any(Ty::mentions_class)
            }
            Ty::Union(parts) => parts.iter().any(Ty::mentions_class),
            Ty::Conditional(a, b) => a.mentions_class() || b.mentions_class(),
            Ty::Scalar(_) | Ty::Mixed => false,
        }
    }

    /// A conditional reads as the union of its branches.
    pub fn settled(self) -> Ty {
        match self {
            Ty::Conditional(a, b) => Ty::union(a.settled(), b.settled()),
            other => other,
        }
    }

    /// As shown to the user (short class names: `Collection<int, User>`, `?User`).
    pub fn show(&self) -> String {
        let list = |v: &[Ty]| v.iter().map(Ty::show).collect::<Vec<_>>().join(", ");
        match self {
            Ty::Named(fqn, args) => {
                let short = fqn.rsplit('\\').next().unwrap_or(fqn);
                if args.is_empty() { short.to_owned() } else { format!("{short}<{}>", list(args)) }
            }
            Ty::Template(name) => name.to_string(),
            Ty::Static(args) if args.is_empty() => "static".into(),
            Ty::Static(args) => format!("static<{}>", list(args)),
            Ty::SelfType => "self".into(),
            Ty::Array(k, v) if k.is_mixed() && v.is_mixed() => "array".into(),
            Ty::Array(k, v) if k.is_mixed() => format!("array<{}>", v.show()),
            Ty::Array(k, v) => format!("array<{}, {}>", k.show(), v.show()),
            Ty::Shape(fields) => format!(
                "array{{{}}}",
                fields
                    .iter()
                    .map(|(k, t)| format!("{k}: {}", t.show()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::ClassString(t) => format!("class-string<{}>", t.show()),
            Ty::Callable(params, ret) => format!("callable({}): {}", list(params), ret.show()),
            Ty::Scalar(s) => s.to_string(),
            Ty::Union(parts) => {
                let nullable = parts.len() == 2 && parts.iter().any(Ty::is_null);
                match parts.iter().find(|t| !t.is_null()) {
                    Some(t) if nullable => format!("?{}", t.show()),
                    _ => parts.iter().map(Ty::show).collect::<Vec<_>>().join("|"),
                }
            }
            Ty::Conditional(a, b) => format!("{}|{}", a.show(), b.show()),
            Ty::Mixed => "mixed".into(),
        }
    }
}

/// How the names in a type are read: the declaring file (its namespace and imports), where
/// in it (for `self` / `parent`), and the template parameters in scope.
#[derive(Clone, Copy)]
pub struct NameScope<'a> {
    pub file: Option<&'a FileSymbols>,
    pub at: usize,
    pub templates: &'a [Box<str>],
}

impl NameScope<'_> {
    /// Names as PHP itself writes them (the stubs: all global or fully qualified).
    pub const GLOBAL: NameScope<'static> = NameScope { file: None, at: 0, templates: &[] };

    fn name(&self, name: &str, args: Vec<Ty>) -> Ty {
        if self.templates.iter().any(|t| &**t == name) {
            return Ty::Template(name.into());
        }
        let fqn = match self.file {
            Some(file) => file.resolve_class(name, self.at, &crate::intelligence::naming()),
            None => name.trim_start_matches('\\').to_owned(),
        };
        Ty::Named(fqn.into(), args)
    }
}

/// The scalar and pseudo types (lowercase), read as themselves.
const SCALARS: &[&str] = &[
    "int",
    "integer",
    "string",
    "bool",
    "boolean",
    "float",
    "double",
    "null",
    "void",
    "false",
    "true",
    "never",
    "never-return",
    "never-returns",
    "no-return",
    "object",
    "resource",
    "closed-resource",
    "open-resource",
    "callable",
    "callable-string",
    "callable-object",
    "callable-array",
    "array-key",
    "scalar",
    "numeric",
    "numeric-string",
    "non-empty-string",
    "non-falsy-string",
    "truthy-string",
    "lowercase-string",
    "non-empty-lowercase-string",
    "uppercase-string",
    "literal-string",
    "positive-int",
    "negative-int",
    "non-positive-int",
    "non-negative-int",
    "non-zero-int",
    "trait-string",
    "enum-string",
    "interface-string",
];

/// `text` read as a type (`None` when empty).
pub fn parse(text: &str, scope: &NameScope<'_>) -> Option<Ty> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut p = Parser { s: text.as_bytes(), text, i: 0, scope, depth: 0 };
    let ty = p.union();
    Some(ty)
}

struct Parser<'a, 's> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
    scope: &'a NameScope<'s>,
    depth: usize,
}

impl Parser<'_, '_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self) -> &str {
        self.skip();
        let start = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'\\' || c == b'-' || c >= 0x80 {
                self.i += 1;
            } else {
                break;
            }
        }
        // A trailing `-` isn't part of a name (`int<0, max>-`… never valid anyway).
        while self.i > start && self.s[self.i - 1] == b'-' {
            self.i -= 1;
        }
        &self.text[start..self.i]
    }

    fn union(&mut self) -> Ty {
        self.depth += 1;
        if self.depth > 32 {
            self.i = self.s.len();
            return Ty::Mixed;
        }
        let mut ty = self.intersection();
        while self.eat(b'|') {
            let next = self.intersection();
            ty = Ty::union(ty, next);
        }
        self.depth -= 1;
        ty
    }

    /// `A&B`: what's offered is the first's (both are in the union for members).
    fn intersection(&mut self) -> Ty {
        let mut ty = self.postfix();
        while self.peek() == Some(b'&') && self.s.get(self.i + 1) != Some(&b'.') {
            self.i += 1;
            let next = self.postfix();
            ty = Ty::union(ty, next);
        }
        ty
    }

    fn postfix(&mut self) -> Ty {
        let mut ty = self.primary();
        while self.peek() == Some(b'[') && self.s.get(self.i + 1) == Some(&b']') {
            self.i += 2;
            ty = Ty::Array(Box::new(Ty::Mixed), Box::new(ty));
        }
        ty
    }

    fn args(&mut self) -> Vec<Ty> {
        let mut args = Vec::new();
        if !self.eat(b'<') {
            return args;
        }
        loop {
            if self.eat(b'>') || self.peek().is_none() {
                break;
            }
            // `int<0, max>`'s bounds and variance (`covariant T`) read as a type each.
            let start = self.i;
            let mut ty = self.union();
            if let Ty::Named(name, _) = &ty
                && matches!(&**name, "covariant" | "contravariant")
            {
                ty = self.union();
            }
            args.push(ty);
            if self.i == start {
                self.i += 1;
            }
            if !self.eat(b',') {
                self.eat(b'>');
                break;
            }
        }
        args
    }

    fn primary(&mut self) -> Ty {
        let Some(c) = self.peek() else { return Ty::Mixed };
        match c {
            b'?' => {
                self.i += 1;
                let inner = self.primary();
                Ty::union(inner, Ty::null())
            }
            b'(' => {
                self.i += 1;
                // `($x is T ? A : B)`, else a group.
                let checked = self.peek() == Some(b'$');
                if checked {
                    self.i += 1;
                    let _ = self.word();
                } else {
                    let first = self.union();
                    if !self.keyword("is") {
                        self.eat(b')');
                        return first;
                    }
                }
                if checked && !self.keyword("is") {
                    self.close();
                    return Ty::Mixed;
                }
                self.keyword("not");
                let _ = self.union();
                if !self.eat(b'?') {
                    self.close();
                    return Ty::Mixed;
                }
                let yes = self.union();
                self.eat(b':');
                let no = self.union();
                self.eat(b')');
                Ty::Conditional(Box::new(yes), Box::new(no))
            }
            b'\'' | b'"' => {
                self.i += 1;
                while self.i < self.s.len() && self.s[self.i] != c {
                    self.i += 1;
                }
                self.i += 1;
                Ty::Scalar("string".into())
            }
            b'-' | b'0'..=b'9' => {
                self.i += 1;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_digit() || matches!(self.s[self.i], b'.' | b'_'))
                {
                    self.i += 1;
                }
                Ty::Scalar("int".into())
            }
            b'$' => {
                self.i += 1;
                let word = self.word();
                if word == "this" { Ty::Static(Vec::new()) } else { Ty::Mixed }
            }
            b'*' => {
                self.i += 1;
                Ty::Mixed
            }
            _ if c.is_ascii_alphabetic() || c == b'_' || c == b'\\' || c >= 0x80 => {
                let word = self.word().to_owned();
                self.named(&word)
            }
            _ => {
                self.i += 1;
                Ty::Mixed
            }
        }
    }

    /// Skips to the `)` closing the group.
    fn close(&mut self) {
        let mut depth = 0usize;
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'(' => depth += 1,
                b')' if depth == 0 => {
                    self.i += 1;
                    return;
                }
                b')' => depth -= 1,
                _ => {}
            }
            self.i += 1;
        }
    }

    fn keyword(&mut self, word: &str) -> bool {
        self.skip();
        let rest = &self.s[self.i..];
        let ends = rest.get(word.len()).is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_');
        if rest.starts_with(word.as_bytes()) && ends {
            self.i += word.len();
            true
        } else {
            false
        }
    }

    fn named(&mut self, word: &str) -> Ty {
        let lower = word.to_ascii_lowercase();
        // `Foo::BAR`, `Foo::*`: a constant's value.
        if self.s[self.i..].starts_with(b"::") {
            self.i += 2;
            if self.peek() == Some(b'*') {
                self.i += 1;
            } else {
                let _ = self.word();
            }
            return Ty::Mixed;
        }
        match lower.as_str() {
            "array" | "list" | "non-empty-array" | "non-empty-list" | "iterable" => {
                if self.peek() == Some(b'{') {
                    return self.shape();
                }
                let mut args = self.args();
                let list = lower.contains("list");
                match args.len() {
                    0 if lower == "iterable" => Ty::Named("iterable".into(), Vec::new()),
                    0 => Ty::Array(Box::new(Ty::Mixed), Box::new(Ty::Mixed)),
                    1 => {
                        let key = if list { Ty::Scalar("int".into()) } else { Ty::Mixed };
                        Ty::Array(Box::new(key), Box::new(args.remove(0)))
                    }
                    _ => {
                        let value = args.remove(1);
                        Ty::Array(Box::new(args.remove(0)), Box::new(value))
                    }
                }
            }
            "object" if self.peek() == Some(b'{') => {
                let _ = self.shape();
                Ty::Scalar("object".into())
            }
            "callable" | "closure" | "\\closure" | "pure-callable" | "pure-closure"
                if self.peek() == Some(b'(') =>
            {
                self.callable(if lower.contains("closure") { "Closure" } else { "callable" })
            }
            "class-string" => {
                let mut args = self.args();
                match args.len() {
                    0 => Ty::Scalar("class-string".into()),
                    _ => Ty::ClassString(Box::new(args.remove(0))),
                }
            }
            "self" => Ty::SelfType,
            "static" => Ty::Static(self.args()),
            "parent" => self.scope.name("parent", Vec::new()),
            "mixed" => Ty::Mixed,
            "key-of" | "value-of" | "int-mask" | "int-mask-of" | "properties-of" => {
                let _ = self.args();
                Ty::Mixed
            }
            "int" | "integer" if self.peek() == Some(b'<') => {
                let _ = self.args();
                Ty::Scalar("int".into())
            }
            s if SCALARS.contains(&s) => Ty::Scalar(s.into()),
            _ => {
                let args = self.args();
                self.scope.name(word, args)
            }
        }
    }

    /// `array{id: int, 'name'?: string, 0: User}` (after its name).
    fn shape(&mut self) -> Ty {
        self.eat(b'{');
        let mut fields = Vec::new();
        let mut position = 0usize;
        loop {
            if self.eat(b'}') || self.peek().is_none() {
                break;
            }
            if self.s[self.i..].starts_with(b"...") {
                self.i += 3;
                self.eat(b',');
                continue;
            }
            let start = self.i;
            // A key (`name:`, `'name'?:`, `0:`) or a value alone (a list shape).
            let key = self.shape_key();
            let key = match key {
                Some(key) => key,
                None => {
                    self.i = start;
                    let key = position.to_string();
                    position += 1;
                    key
                }
            };
            let ty = self.union();
            fields.push((key.into(), ty));
            if self.i == start {
                self.i += 1;
            }
            if !self.eat(b',') {
                self.eat(b'}');
                break;
            }
        }
        Ty::Shape(fields)
    }

    fn shape_key(&mut self) -> Option<String> {
        self.skip();
        let key = match self.peek()? {
            q @ (b'\'' | b'"') => {
                self.i += 1;
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i] != q {
                    self.i += 1;
                }
                let key = self.text[start..self.i].to_owned();
                self.i += 1;
                key
            }
            _ => self.word().to_owned(),
        };
        self.eat(b'?');
        (!key.is_empty() && self.eat(b':')).then_some(key)
    }

    /// `callable(A, B): R` (after its name).
    fn callable(&mut self, _kind: &str) -> Ty {
        self.eat(b'(');
        let mut params = Vec::new();
        loop {
            if self.eat(b')') || self.peek().is_none() {
                break;
            }
            let start = self.i;
            params.push(self.union());
            // A parameter's name, a variadic's dots, an optional's `=`.
            while let Some(c) = self.peek() {
                if c == b',' || c == b')' {
                    break;
                }
                self.i += 1;
            }
            if self.i == start {
                self.i += 1;
            }
            if !self.eat(b',') {
                self.eat(b')');
                break;
            }
        }
        let ret = if self.eat(b':') { self.primary() } else { Ty::Mixed };
        Ty::Callable(params, Box::new(ret))
    }
}

/// The type written at the start of a PHPDoc tag's text (`array<int, User> $users the
/// list` → `array<int, User>`, the rest after it): brackets balanced, spaces inside them
/// and around `|` / `&` / `:` kept.
pub fn split_doc_type(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'<' | b'{' | b'(' | b'[' => depth += 1,
            b'>' | b'}' | b')' | b']' => depth -= 1,
            b'\'' | b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != c {
                    i += 1;
                }
            }
            c if c.is_ascii_whitespace() && depth <= 0 => {
                // `A | B`, `callable(): R`: the type goes on.
                let before = text[..i].trim_end();
                let after = text[i..].trim_start();
                let joined = before.ends_with(['|', '&', ':'])
                    || after.starts_with(['|', '&'])
                    || (after.starts_with(':') && before.ends_with(')'));
                if !joined {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let i = i.min(text.len());
    (text[..i].trim_end(), &text[i..])
}

#[cfg(test)]
#[path = "../tests/unit/types.rs"]
mod tests;
