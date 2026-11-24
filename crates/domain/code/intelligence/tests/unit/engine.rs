use super::completion::{NARROWED, keyword_insert};
use super::hover::strip_modifiers;
use super::*;
use crate::symbols::Visibility;
use crate::{CompletionContext, ContextKind, Receiver, Target};

#[test]
fn signatures_and_parameters() {
    assert_eq!(
        strip_modifiers(
            "(public Device $device, private readonly bool $created = true)",
            &["public", "protected", "private", "readonly"]
        ),
        "(Device $device, bool $created = true)"
    );
    assert_eq!(split_param_plainly("name: string = 'a'"), (Some("string".into()), "name".into()));
    assert_eq!(split_param_plainly("rest"), (None, "rest".into()));
}

#[test]
fn keywords_get_a_space_or_parentheses() {
    use crate::KeywordInsert;
    assert_eq!(keyword_insert("function", KeywordInsert::Spaced), ("function ".into(), 0));
    assert_eq!(keyword_insert("isset", KeywordInsert::Call), ("isset()".into(), 1));
    assert_eq!(keyword_insert("true", KeywordInsert::Exact), ("true".into(), 0));
}

/// A language whose receivers are typed knowing the project: whatever is before `->` is a
/// `Box`, which has a built-in `size()` besides its declared members.
#[derive(Debug)]
struct Typed;

impl LanguageIntelligence for Typed {
    fn language(&self) -> LanguageId {
        LanguageId::new("typed")
    }
    fn extensions(&self) -> &[&'static str] {
        &["t"]
    }
    fn extract(&self, _text: &str) -> FileSymbols {
        FileSymbols::default()
    }
    fn context(&self, text: &str, offset: usize) -> CompletionContext {
        let kind = if text[..offset].ends_with("->") {
            ContextKind::Member(Receiver::Unknown)
        } else {
            ContextKind::None
        };
        CompletionContext { replace: offset..offset, prefix: String::new(), kind }
    }
    fn target(&self, text: &str, offset: usize) -> Option<Target> {
        let word = crate::word_at(text, offset, "")?;
        Some(Target::Member { receiver: Receiver::Unknown, name: text[word].to_owned() })
    }
    fn locals(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }
    fn keywords(&self) -> &[&'static str] {
        &[]
    }
    fn receiver_type(
        &self,
        _text: &str,
        _offset: usize,
        types: &crate::Types<'_>,
    ) -> Option<String> {
        (types.index.type_by_fqn("Box").map(|(_, s)| s.fqn.clone())).map(String::from)
    }
    fn builtin_members(&self, class: &str, _types: &crate::Types<'_>) -> Vec<Symbol> {
        let mut size = symbol("size", SymbolKind::Method, Some(class));
        size.detail = Some("(): int".into());
        // Declared in the project too: not twice.
        vec![size, symbol("open", SymbolKind::Method, Some(class))]
    }
}

fn symbol(name: &str, kind: SymbolKind, container: Option<&str>) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        fqn: name.into(),
        container: container.map(Into::into),
        parents: Box::default(),
        range: 0..1,
        offset: 0,
        line: 3,
        detail: None,
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

#[test]
fn the_language_types_receivers_and_adds_its_own_members() {
    let mut index = SymbolIndex::default();
    let file = FileSymbols {
        symbols: vec![
            symbol("Box", SymbolKind::Class, None),
            symbol("open", SymbolKind::Method, Some("Box")),
        ],
        ..Default::default()
    };
    index.set_file(Path::new("/p/box.t"), file);
    let path = Path::new("/p/main.t");
    let (_, items) = complete(&Typed, &index, path, "x->", 3, &Project::none());
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["open", "size"]);
    let found = definition(&Typed, &index, path, "x->open", 4, &Project::none());
    assert_eq!(found.first().map(|l| l.line), Some(3), "through the receiver's type");
}

/// A language with untyped receivers: `x->` then a word is a member of no known type, a
/// word alone is a name.
#[derive(Debug)]
struct Plain;

impl LanguageIntelligence for Plain {
    fn language(&self) -> LanguageId {
        LanguageId::new("plain")
    }
    fn extensions(&self) -> &[&'static str] {
        &["pl"]
    }
    fn extract(&self, _text: &str) -> FileSymbols {
        FileSymbols::default()
    }
    fn context(&self, text: &str, offset: usize) -> CompletionContext {
        let before = &text[..offset];
        let word = before.len() - before.trim_end_matches(|c: char| c.is_alphanumeric()).len();
        let start = offset - word;
        let kind = if text[..start].ends_with("->") {
            ContextKind::Member(Receiver::Unknown)
        } else {
            ContextKind::Name
        };
        CompletionContext { replace: start..offset, prefix: text[start..offset].to_owned(), kind }
    }
    fn target(&self, _text: &str, _offset: usize) -> Option<Target> {
        None
    }
    fn locals(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }
    fn keywords(&self) -> &[&'static str] {
        &[]
    }
}

/// A library file's symbols, given back whole (its bytes are its own: never shared).
#[derive(Debug)]
struct Whole(FileSymbols);

impl crate::Unpack for Whole {
    fn unpack(&self, _bytes: &[u8]) -> Option<FileSymbols> {
        Some(self.0.clone())
    }
}

/// Many library files kept packed (their classes, their members of many names) and a few
/// of the project's.
fn many_libraries() -> SymbolIndex {
    // `vendor` a dependency folder before anything is scored: the registry is the process's,
    // and another test registering it mid-way (`query::fuzzy_names_with_filters`) would
    // otherwise score some of these files as a dependency's and others not.
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        ..Default::default()
    });
    const WORDS: &[&str] =
        &["get", "set", "gather", "target", "widget", "geometry", "agent", "edge"];
    let mut index = SymbolIndex::default();
    for i in 0..400 {
        let class = format!("Lib{i}Gadget");
        let mut symbols = vec![symbol(&class, SymbolKind::Class, None)];
        for (w, word) in WORDS.iter().enumerate() {
            let name = format!("{word}{}", (i * 7 + w) % 37);
            symbols.push(symbol(&name, SymbolKind::Method, Some(&class)));
        }
        symbols.push(symbol(&format!("lib_get_{i}"), SymbolKind::Function, None));
        let whole = FileSymbols { language: "plain".into(), symbols, ..FileSymbols::default() };
        let path = format!("/p/vendor/acme/f{i}.pl");
        let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(path.as_bytes());
        index.set_file_packed(
            Path::new(&path),
            &whole,
            bytes,
            std::sync::Arc::new(Whole(whole.clone())),
        );
    }
    for i in 0..20 {
        let class = format!("AppGetter{i}");
        let symbols = vec![
            symbol(&class, SymbolKind::Class, None),
            symbol(&format!("getValue{i}"), SymbolKind::Method, Some(&class)),
        ];
        let file = FileSymbols { language: "plain".into(), symbols, ..FileSymbols::default() };
        index.set_file(Path::new(&format!("/p/src/a{i}.pl")), file);
    }
    index
}

/// A word typed letter by letter (each looked for only among what the one before matched)
/// suggests what the whole word, looked for at once, does — members of no known type and
/// names alike; a word taken back looks at everything again.
#[test]
fn a_word_typed_on_suggests_what_it_would_at_once() {
    fn suggested(index: &SymbolIndex, text: &str) -> Vec<(String, Option<String>)> {
        let path = Path::new("/p/src/main.pl");
        let (_, items) = complete(&Plain, index, path, text, text.len(), &Project::none());
        items.into_iter().map(|i| (i.label, i.detail)).collect()
    }
    // Each run from nothing kept: the matches the keys before it left forgotten.
    fn fresh<T>(run: impl FnOnce() -> T) -> T {
        NARROWED.with_borrow_mut(Vec::clear);
        run()
    }
    let index = many_libraries();
    for typed in [["x->g", "x->ge", "x->get"], ["G", "Ga", "Gad"], ["l", "li", "lib_g"]] {
        let narrowed = fresh(|| {
            suggested(&index, typed[0]);
            assert!(NARROWED.with_borrow(|kept| !kept.is_empty()), "{typed:?}");
            suggested(&index, typed[1]);
            suggested(&index, typed[2])
        });
        let at_once = fresh(|| suggested(&index, typed[2]));
        assert!(!at_once.is_empty(), "{typed:?}");
        assert_eq!(narrowed, at_once, "{typed:?}");
    }
    // Taken back (`get` then `ge`): everything again.
    let back = fresh(|| {
        suggested(&index, "x->get");
        suggested(&index, "x->ge")
    });
    assert_eq!(back, fresh(|| suggested(&index, "x->ge")));
}

/// A script's language: every `x.name` a member of a receiver it can't type.
#[derive(Debug)]
struct Script;

impl LanguageIntelligence for Script {
    fn language(&self) -> LanguageId {
        LanguageId::new("script")
    }
    fn extensions(&self) -> &[&'static str] {
        &["sc"]
    }
    fn extract(&self, _text: &str) -> FileSymbols {
        FileSymbols::default()
    }
    fn context(&self, _text: &str, offset: usize) -> CompletionContext {
        CompletionContext {
            replace: offset..offset,
            prefix: String::new(),
            kind: ContextKind::Name,
        }
    }
    fn target(&self, text: &str, offset: usize) -> Option<Target> {
        let word = crate::word_at(text, offset, self.word_chars())?;
        Some(Target::Member { receiver: Receiver::Unknown, name: text[word].to_owned() })
    }
    fn locals(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }
    fn keywords(&self) -> &[&'static str] {
        &[]
    }
}

/// A member of a receiver the language can't type, found only by its name: one of its own
/// language's, never another's of the same name (a page script's `addEventListener` isn't
/// a PHP class's method).
#[test]
fn a_member_found_by_its_name_alone_stays_in_its_language() {
    let mut index = SymbolIndex::default();
    let other = vec![
        symbol("ButtonBuilder", SymbolKind::Class, None),
        symbol("addEventListener", SymbolKind::Method, Some("ButtonBuilder")),
    ];
    index.set_file(
        Path::new("/p/ButtonBuilder.php"),
        FileSymbols { language: "php".into(), symbols: other, ..FileSymbols::default() },
    );
    let path = Path::new("/p/page.sc");
    let text = "window.addEventListener";
    let found = definition(&Script, &index, path, text, 10, &Project::none());
    assert!(found.is_empty(), "another language's: {found:?}");
    let own = vec![
        symbol("Target", SymbolKind::Class, None),
        symbol("addEventListener", SymbolKind::Method, Some("Target")),
    ];
    index.set_file(
        Path::new("/p/target.sc"),
        FileSymbols { language: "script".into(), symbols: own, ..FileSymbols::default() },
    );
    let found = definition(&Script, &index, path, text, 10, &Project::none());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, Path::new("/p/target.sc"));
}
