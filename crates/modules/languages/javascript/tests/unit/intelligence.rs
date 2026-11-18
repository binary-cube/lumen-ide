use super::*;

const SRC: &str = r#"
import { x } from './x.js';
export function formatPrice(value, currency = 'RON') { return value; }
export const api = { base: '/api' };
const debounce = (fn, ms) => fn;
export default class Cart extends Store {
    items = [];
    static create() { return new Cart(); }
    add(product, qty) { this.items.push(product); }
}
function inner() { const hidden = 1; }
"#;

#[test]
fn extracts_top_level_declarations() {
    let file = JsIntelligence::default().extract(SRC);
    let find = |n: &str| file.symbols.iter().find(|s| s.name == n.into());
    assert_eq!(find("formatPrice").map(|s| s.kind), Some(SymbolKind::Function));
    assert_eq!(
        find("formatPrice").and_then(|s| s.detail.clone()).as_deref(),
        Some("(value, currency = 'RON')")
    );
    assert_eq!(find("api").map(|s| s.kind), Some(SymbolKind::Variable));
    assert_eq!(find("debounce").map(|s| s.kind), Some(SymbolKind::Function));
    let cart = find("Cart").unwrap();
    assert_eq!((cart.kind, cart.parents.to_vec()), (SymbolKind::Class, vec!["Store".into()]));
    assert_eq!(find("add").and_then(|s| s.container.clone()).as_deref(), Some("Cart"));
    assert!(find("create").is_some_and(|s| s.is_static));
    assert!(find("items").is_some_and(|s| s.kind == SymbolKind::Property));
    assert!(find("hidden").is_none(), "local variables aren't indexed");
}

fn ctx(src: &str) -> ContextKind {
    let at = src.find('|').unwrap();
    context(&src.replace('|', ""), at).kind
}

#[test]
fn contexts_and_targets() {
    assert_eq!(ctx("this.ad|"), ContextKind::Member(Receiver::This));
    assert_eq!(ctx("console.lo|"), ContextKind::Vocabulary("console"));
    assert_eq!(ctx("cart.ad|"), ContextKind::Member(Receiver::Unknown));
    assert_eq!(ctx("new Ca|"), ContextKind::TypeName);
    assert_eq!(ctx("const t = formatP|"), ContextKind::Name);
    assert_eq!(ctx("// formatP|"), ContextKind::None);
    assert_eq!(ctx("'formatP|"), ContextKind::None);
    let src = "formatP|rice(1)";
    let at = src.find('|').unwrap();
    assert!(
        matches!(target(&src.replace('|', ""), at), Some(Target::Named { name, .. }) if name == "formatPrice")
    );
    assert_eq!(
        locals("function f() { let a = 1; const b = 2; ", 39),
        vec!["a".to_owned(), "b".to_owned()]
    );
}

/// A class's `constructor` (case matters); no sigils, no file per type.
#[test]
fn naming_rules() {
    let naming = JsIntelligence::default().naming();
    assert!(naming.is_constructor("constructor") && !naming.is_constructor("Constructor"));
    assert_eq!((naming.sigil, naming.type_per_file), (None, None));
}

/// A word of a comment's text leads nowhere; the JSDoc type does (plan 75d).
#[test]
fn only_what_a_comment_writes_as_code_leads_somewhere() {
    use code_intelligence::{Project, SymbolIndex};
    let model = "export class Runtime {\n  start() {}\n}\n";
    let text = "/**\n * Starts the runtime.\n * @param {Runtime} runtime the one to start\n */\nfunction start(runtime) {}\n";
    let js = JsIntelligence::default();
    let mut index = SymbolIndex::default();
    for (path, src) in [("/p/runtime.js", model), ("/p/start.js", text)] {
        let mut symbols = js.extract(src);
        symbols.language = "javascript".into();
        index.set_file(std::path::Path::new(path), symbols);
    }
    let path = std::path::Path::new("/p/start.js");
    let found = |at: usize| {
        let found = code_intelligence::definition(&js, &index, path, text, at, &Project::none());
        found.first().map(|l| (l.path.display().to_string(), l.offset))
    };
    assert_eq!(found(text.find("runtime.").unwrap() + 2), None, "a word of the text");
    let ty = text.find("{Runtime}").unwrap() + 3;
    assert_eq!(found(ty).map(|f| f.0).as_deref(), Some("/p/runtime.js"), "the type");
    let parameter = text.find("runtime the").unwrap() + 2;
    let declared = text.find("runtime) {").unwrap();
    assert_eq!(found(parameter), Some(("/p/start.js".to_owned(), declared)), "the parameter");
}

#[test]
fn a_doc_comment_completes_its_tags() {
    let intel = JsIntelligence::default();
    let text = "/**\n * @ret\n */\nfunction f() {}";
    let offset = text.find("@ret").unwrap() + 4;
    let context = intel.context(text, offset);
    assert_eq!(context.kind, ContextKind::Vocabulary(DOC_TAG));
    assert_eq!(&text[context.replace.clone()], "@ret");
    let items = intel.vocabulary(&context, text);
    let returns = items.iter().find(|i| i.label == "@returns").expect("@returns");
    assert_eq!(returns.insert, "@returns ", "a type follows");
    // A plain comment or an address: nothing.
    assert_eq!(intel.context("// @ret", 7).kind, ContextKind::None);
    assert_eq!(intel.context("/** mail me@ex", 14).kind, ContextKind::None);
}

#[test]
fn a_doc_tag_s_braces_complete_type_names() {
    let intel = JsIntelligence::default();
    let at = |text: &str| {
        let offset = text.find('^').unwrap();
        let text = text.replacen('^', "", 1);
        (intel.context(&text, offset), text)
    };
    let (context, text) = at("/**\n * @param {string|Us^} user\n */");
    assert_eq!(context.kind, ContextKind::TypeName);
    assert_eq!(context.prefix, "Us");
    let (context, text2) = at("/** @type {Pro^} */");
    assert_eq!((context.kind.clone(), context.prefix.as_str()), (ContextKind::TypeName, "Pro"));
    let labels: Vec<String> =
        intel.vocabulary(&context, &text2).into_iter().map(|i| i.label).collect();
    assert!(labels.contains(&"Promise".to_owned()), "{labels:?}");
    let _ = text;
    // A link's braces, a comment's plain braces: not a type.
    assert_ne!(at("/** see {@link Us^} */").0.kind, ContextKind::TypeName);
    assert_ne!(at("/** a {Us^} b */").0.kind, ContextKind::TypeName);
    assert_ne!(at("// @param {Us^}").0.kind, ContextKind::TypeName);
}
