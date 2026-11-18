//! Methods named in strings, as PHP's callables write them:
//! `[$obj, 'cart']`, `[$this, 'cart']`, `[self::class, 'cart']`, `[Shop::class, 'cart']`,
//! `['App\Shop', 'cart']` (a two-element array literal anywhere: `Closure::fromCallable(…)`,
//! `is_callable(…)`, `array_map(…)`…), the string form `'App\Shop::cart'`, and
//! `method_exists($x, 'cart')`. The string's name is then a use of the method: navigation,
//! hover, Find Usages and Rename (which rewrites only the name, its quotes and its class part
//! kept) take it; completion in it offers the class's methods. Whether it is the method is
//! decided by the class the first element names (or its inferred type: `[$x, 'm']` counts
//! only when `$x`'s type is known), and the method existing there (its ancestors, mixins).

use std::ops::Range;

use syntax_ts::tree_sitter::Node;

use crate::infer::named_children;

/// The class part of a callable.
#[derive(Debug, Clone)]
pub(crate) enum Class<'t> {
    /// A name written in a string (`'App\Shop'`, `'Shop::cart'`): its range in the text.
    Written(Range<usize>),
    /// `X::class`, `self::class`, `static::class`.
    Constant(Node<'t>),
    /// An object (`$obj`, `$this`, `$this->shop`): its type is the class.
    Object(Node<'t>),
}

/// A method named in a string: its class part and the name's range (the string's text).
#[derive(Debug, Clone)]
pub(crate) struct Callable<'t> {
    pub class: Class<'t>,
    pub name: Range<usize>,
}

fn is_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

pub(crate) fn is_class_name(name: &str) -> bool {
    let name = name.strip_prefix('\\').unwrap_or(name);
    !name.is_empty() && name.split('\\').all(is_name)
}

/// The string at `offset` (its node) and its text's range: a plain string only (nothing
/// interpolated).
pub(crate) fn string_at<'t>(root: Node<'t>, offset: usize) -> Option<(Node<'t>, Range<usize>)> {
    let node = root.descendant_for_byte_range(offset, offset)?;
    let string = std::iter::successors(Some(node), |n| n.parent())
        .take(3)
        .find(|n| matches!(n.kind(), "string" | "encapsed_string"))?;
    let inner: Vec<_> = named_children(string).collect();
    let content = match inner.as_slice() {
        [content] if content.kind() == "string_content" => content.byte_range(),
        [] if string.end_byte() >= string.start_byte() + 2 => {
            string.start_byte() + 1..string.start_byte() + 1
        }
        _ => return None,
    };
    (content.start <= offset && offset <= content.end).then_some((string, content))
}

/// The callable whose method name the string at `offset` of `text` is (the name may be
/// empty: completion in `[$this, '|']`).
pub(crate) fn at<'t>(root: Node<'t>, text: &str, offset: usize) -> Option<Callable<'t>> {
    let (string, content) = string_at(root, offset)?;
    let value = text.get(content.clone())?;
    // `'App\Shop::cart'`.
    if let Some(split) = value.rfind("::") {
        let (class, name) = (&value[..split], &value[split + 2..]);
        let start = content.start + split + 2;
        return (offset >= start && is_class_name(class) && (name.is_empty() || is_name(name)))
            .then(|| Callable {
                class: Class::Written(content.start..content.start + split),
                name: start..content.end,
            });
    }
    if !(value.is_empty() || is_name(value)) {
        return None;
    }
    let first = callable_array(string).or_else(|| method_exists(text, string))?;
    let class = match first.kind() {
        "class_constant_access_expression" => {
            let last = named_children(first).last()?;
            if !text.get(last.byte_range())?.eq_ignore_ascii_case("class") {
                return None;
            }
            Class::Constant(first)
        }
        "string" | "encapsed_string" => {
            let (_, range) = string_at(first, first.start_byte() + 1)?;
            if !is_class_name(text.get(range.clone())?) {
                return None;
            }
            Class::Written(range)
        }
        _ => Class::Object(first),
    };
    Some(Callable { class, name: content })
}

/// `[first, 'name']`: a two-element array literal without keys, the string second; the
/// first element.
fn callable_array(string: Node<'_>) -> Option<Node<'_>> {
    let element = string.parent().filter(|p| p.kind() == "array_element_initializer")?;
    let array = element.parent().filter(|p| p.kind() == "array_creation_expression")?;
    let elements: Vec<_> =
        named_children(array).filter(|c| c.kind() == "array_element_initializer").collect();
    let [first, second] = elements.as_slice() else { return None };
    let plain = |e: &Node<'_>| e.named_child_count() == 1;
    (second.id() == element.id() && plain(first) && plain(second)).then(|| first.named_child(0))?
}

/// `method_exists(first, 'name')`: the first argument.
fn method_exists<'t>(text: &str, string: Node<'t>) -> Option<Node<'t>> {
    let argument = string.parent().filter(|p| p.kind() == "argument")?;
    let arguments = argument.parent().filter(|p| p.kind() == "arguments")?;
    let call = arguments.parent().filter(|p| p.kind() == "function_call_expression")?;
    let function = call.child_by_field_name("function")?;
    let name = text.get(function.byte_range())?.trim_start_matches('\\');
    if !name.eq_ignore_ascii_case("method_exists") {
        return None;
    }
    let args: Vec<_> = named_children(arguments).filter(|c| c.kind() == "argument").collect();
    let [first, second] = args.as_slice() else { return None };
    (second.id() == argument.id()).then(|| first.named_child(0))?
}

/// The callable at `offset` of `text`, with the text's tree (only when the line says the
/// cursor may be in a string: no parse otherwise).
pub(crate) fn with_callable<R>(
    text: &str,
    offset: usize,
    ask: impl FnOnce(Node<'_>, &Callable<'_>) -> Option<R>,
) -> Option<R> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    if !crate::intelligence::maybe_in_string(&text[..offset]) {
        return None;
    }
    let parsed = crate::doctree::parsed(text)?;
    let root = parsed.tree.root_node();
    let callable = at(root, text, offset)?;
    ask(root, &callable)
}

/// The functions that take a class's name as a string, and which of their arguments
/// (0-based) do.
const CLASS_ARGUMENTS: &[(&str, &[usize])] = &[
    ("class_exists", &[0]),
    ("interface_exists", &[0]),
    ("trait_exists", &[0]),
    ("enum_exists", &[0]),
    ("get_parent_class", &[0]),
    ("method_exists", &[0]),
    ("property_exists", &[0]),
    ("is_a", &[1]),
    ("is_subclass_of", &[0, 1]),
];

/// The class a string at `offset` names, as PHP uses it (its full name, `\`-led), or
/// `Some(None)`: the cursor is in a string that names none — a word in a sentence, an
/// array key, `strlen('cart')`. `None`: not in a string. A string names a class where PHP
/// takes it as one: the class part of a callable (`['App\Shop', 'cart']`,
/// `'App\Shop::cart'`), the argument of `class_exists()`, `is_a()`… (see
/// [`CLASS_ARGUMENTS`]), and anywhere when it is a full name with a namespace
/// (`'App\Service\Mailer'`: a container's or a configuration's).
pub(crate) fn class_in_string(text: &str, offset: usize) -> Option<Option<String>> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    if !crate::intelligence::maybe_in_string(&text[..offset]) {
        return None;
    }
    let parsed = crate::doctree::parsed(text)?;
    let root = parsed.tree.root_node();
    let Some((string, content)) = string_at(root, offset) else {
        // A string's quote, an interpolated one: not ours to say.
        return None;
    };
    let value = text.get(content.clone())?;
    // `'App\Shop::cart'`: its class part.
    let name = match value.rfind("::") {
        Some(split) if offset <= content.start + split => &value[..split],
        Some(_) => return Some(None),
        None => value,
    };
    if !is_class_name(name) {
        return Some(None);
    }
    let qualified = name.trim_start_matches('\\').contains('\\');
    let taken =
        qualified || value.contains("::") || class_argument(text, string) || callable_class(string);
    Some(taken.then(|| format!("\\{}", name.trim_start_matches('\\'))))
}

/// Whether `string` is a callable's class part: `['Shop', 'cart']`.
fn callable_class(string: Node<'_>) -> bool {
    let Some(element) = string.parent().filter(|p| p.kind() == "array_element_initializer") else {
        return false;
    };
    let Some(array) = element.parent().filter(|p| p.kind() == "array_creation_expression") else {
        return false;
    };
    let elements: Vec<_> =
        named_children(array).filter(|c| c.kind() == "array_element_initializer").collect();
    let [first, second] = elements.as_slice() else { return false };
    let method = second.named_child(0).filter(|n| matches!(n.kind(), "string" | "encapsed_string"));
    first.id() == element.id()
        && first.named_child_count() == 1
        && second.named_child_count() == 1
        && method.is_some()
}

/// Whether `string` is an argument of a function taking a class's name there.
fn class_argument(text: &str, string: Node<'_>) -> bool {
    let Some(argument) = string.parent().filter(|p| p.kind() == "argument") else { return false };
    let Some(arguments) = argument.parent() else { return false };
    let Some(call) = arguments.parent().filter(|p| p.kind() == "function_call_expression") else {
        return false;
    };
    let Some(function) = call.child_by_field_name("function") else { return false };
    let name = text.get(function.byte_range()).unwrap_or_default().trim_start_matches('\\');
    let Some((_, positions)) = CLASS_ARGUMENTS.iter().find(|(f, _)| f.eq_ignore_ascii_case(name))
    else {
        return false;
    };
    let position = named_children(arguments)
        .filter(|c| c.kind() == "argument")
        .position(|a| a.id() == argument.id());
    position.is_some_and(|p| positions.contains(&p))
}

/// The class part as a navigation target writes it: `\App\Shop`, `Shop`, `self`; an
/// object's: empty (its type is worked out knowing the project).
pub(crate) fn written_class(text: &str, class: &Class<'_>) -> String {
    match class {
        Class::Written(range) => {
            let name = text.get(range.clone()).unwrap_or_default();
            // A string's class name is a full one.
            format!("\\{}", name.trim_start_matches('\\'))
        }
        Class::Constant(node) => named_children(*node)
            .next()
            .and_then(|scope| text.get(scope.byte_range()))
            .unwrap_or_default()
            .to_owned(),
        Class::Object(_) => String::new(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/callables.rs"]
mod tests;
