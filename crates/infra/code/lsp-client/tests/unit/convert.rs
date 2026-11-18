use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

const TEXT: &str = "const 😀 = 1;\nlet total = 😀 + 2;\nconsole.lo";

fn lines() -> Lines {
    Lines::new(TEXT.to_owned())
}

/// Texts held in memory.
#[derive(Default)]
struct Held(HashMap<PathBuf, Lines>);

impl Texts for Held {
    fn lines(&mut self, path: &Path) -> Option<Cow<'_, Lines>> {
        self.0.get(path).map(Cow::Borrowed)
    }
}

fn held() -> Held {
    let mut held = Held::default();
    held.0.insert("/p/a.ts".into(), lines());
    held.0.insert("/p/b.ts".into(), Lines::new("export const b = 1;\n".into()));
    held
}

fn range(l0: u32, c0: u32, l1: u32, c1: u32) -> serde_json::Value {
    json!({"start": {"line": l0, "character": c0}, "end": {"line": l1, "character": c1}})
}

#[test]
fn completion_items_take_their_edit_their_snippet_and_their_imports() {
    let at = TEXT.len();
    let answer = json!({
        "isIncomplete": false,
        "items": [
            {"label": "log", "kind": 2, "detail": "(method) log(...data: any[]): void",
             "sortText": "11", "textEdit": {"range": range(2, 8, 2, 10), "newText": "log($1)$0"},
             "insertTextFormat": 2},
            {"label": "readFile", "kind": 3, "sortText": "16", "filterText": "readFile",
             "additionalTextEdits": [{"range": range(0, 0, 0, 0), "newText": "import { readFile } from 'fs';\n"}]},
            {"label": "#priv", "filterText": "priv", "insertText": "#priv", "kind": 5},
        ]
    });
    let served = completion(&answer, &lines(), Encoding::Utf16, at);
    assert!(!served.incomplete);
    assert_eq!(served.items.len(), 3);
    let log = &served.items[0];
    assert_eq!(log.item.label, "log");
    assert_eq!(log.item.kind, CompletionKind::Method);
    assert_eq!(log.item.insert, "log()");
    assert_eq!(log.item.cursor_back, 1);
    assert_eq!(log.item.replace, Some(at - 2..at));
    assert_eq!(log.sort, "11");
    assert_eq!(log.item.resolve, Some(0));
    let read = &served.items[1];
    assert_eq!(read.item.insert, "readFile");
    assert_eq!(read.item.edits, vec![(0..0, "import { readFile } from 'fs';\n".to_owned())]);
    assert_eq!(read.item.replace, None);
    assert_eq!(served.items[2].filter, "priv");
    assert_eq!(served.items[2].item.kind, CompletionKind::Property);
}

#[test]
fn completion_items_use_the_lists_defaults() {
    let answer = json!({
        "isIncomplete": true,
        "itemDefaults": {"editRange": {"insert": range(2, 8, 2, 10), "replace": range(2, 8, 2, 10)}, "insertTextFormat": 2},
        "items": [{"label": "log", "textEditText": "log(${1:x})"}]
    });
    let served = completion(&answer, &lines(), Encoding::Utf16, TEXT.len());
    assert!(served.incomplete);
    let item = &served.items[0].item;
    assert_eq!(item.insert, "log(x)");
    assert_eq!(item.cursor_back, 2);
    assert_eq!(item.replace, Some(TEXT.len() - 2..TEXT.len()));
}

#[test]
fn a_bare_list_of_items_is_an_answer_too() {
    let served = completion(&json!([{"label": "a"}, {"nolabel": 1}]), &lines(), Encoding::Utf16, 0);
    assert_eq!(served.items.len(), 1);
    assert_eq!(served.items[0].sort, "00000000");
    assert!(completion(&json!(null), &lines(), Encoding::Utf16, 0).items.is_empty());
}

#[test]
fn a_resolved_item_brings_its_imports() {
    let item = CompletionItem { label: "x".into(), resolve: Some(3), ..CompletionItem::default() };
    let value = json!({"detail": "const x", "additionalTextEdits": [{"range": range(1, 0, 1, 0), "newText": "y"}]});
    let resolved = resolved(&value, &item, &lines(), Encoding::Utf16);
    let line1 = TEXT.find("let").unwrap();
    assert_eq!(resolved.edits, vec![(line1..line1, "y".to_owned())]);
    assert_eq!(resolved.detail.as_deref(), Some("const x"));
    assert_eq!(resolved.resolve, None);
}

#[test]
fn a_hover_splits_its_signature_from_its_documentation() {
    let value = json!({
        "contents": {"kind": "markdown", "value": "```typescript\nlet total: number\n```\nThe **sum**."},
        "range": range(1, 4, 1, 9)
    });
    let info = hover(&value, &lines(), Encoding::Utf16, 0..0).unwrap();
    assert_eq!(info.code, "let total: number");
    assert_eq!(info.doc.as_deref(), Some("The sum."));
    let start = TEXT.find("total").unwrap();
    assert_eq!(info.range, start..start + 5);
    // Marked strings, old style.
    let old = json!({"contents": [{"language": "ts", "value": "const x: 1"}, "doc"]});
    let info = hover(&old, &lines(), Encoding::Utf16, 3..4).unwrap();
    assert_eq!(
        (info.code.as_str(), info.doc.as_deref(), info.range),
        ("const x: 1", Some("doc"), 3..4)
    );
    assert_eq!(hover(&json!({"contents": ""}), &lines(), Encoding::Utf16, 0..0), None);
}

#[test]
fn a_signature_finds_its_parameters_by_text_or_offsets() {
    let by_text = json!({"signatures": [{"label": "add(a: number, b: number): number",
        "parameters": [{"label": "a: number"}, {"label": "b: number"}]}], "activeParameter": 1});
    let help = signature_help(&by_text).unwrap();
    assert_eq!(help.code, "add(a: number, b: number): number");
    assert_eq!(&help.code[help.params[0].clone()], "a: number");
    assert_eq!(&help.code[help.params[1].clone()], "b: number");
    assert_eq!(help.active, Some(1));
    // Offsets in UTF-16 units of the label.
    let by_offsets = json!({"signatures": [{"label": "f(😀: T, y)", "parameters": [{"label": [2, 7]}, {"label": [9, 10]}],
        "activeParameter": 0}]});
    let help = signature_help(&by_offsets).unwrap();
    assert_eq!(&help.code[help.params[0].clone()], "😀: T");
    assert_eq!(&help.code[help.params[1].clone()], "y");
    assert_eq!(help.active, Some(0));
    assert_eq!(signature_help(&json!({"signatures": []})), None);
}

#[test]
fn locations_come_as_one_a_list_or_links() {
    let mut texts = held();
    let one = json!({"uri": "file:///p/b.ts", "range": range(0, 13, 0, 14)});
    let found = locations(&one, &mut texts, Encoding::Utf16);
    assert_eq!(found, vec![ServedLocation { path: "/p/b.ts".into(), range: 13..14, line: 0 }]);
    let links = json!([{"targetUri": "file:///p/a.ts", "targetRange": range(1, 0, 1, 18), "targetSelectionRange": range(1, 4, 1, 9)},
        {"uri": "file:///p/missing.ts", "range": range(0, 0, 0, 1)}]);
    let found = locations(&links, &mut texts, Encoding::Utf16);
    let start = TEXT.find("total").unwrap();
    assert_eq!(
        found,
        vec![ServedLocation { path: "/p/a.ts".into(), range: start..start + 5, line: 1 }]
    );
    assert!(locations(&json!(null), &mut texts, Encoding::Utf16).is_empty());
}

#[test]
fn highlights_are_byte_ranges_in_order() {
    let value = json!([{"range": range(1, 12, 1, 14)}, {"range": range(0, 6, 0, 8)}, {"range": range(0, 6, 0, 8)}]);
    let found = highlights(&value, &lines(), Encoding::Utf16);
    let first = TEXT.find('😀').unwrap();
    let second = TEXT.rfind('😀').unwrap();
    assert_eq!(found, vec![first..first + 4, second..second + 4]);
}

#[test]
fn symbols_keep_their_members_in_their_type() {
    let text = "class Cart {\n  total() {}\n}\nfunction main() {}\n";
    let lines = Lines::new(text.into());
    let tree = json!([
        {"name": "Cart", "kind": 5, "range": range(0, 0, 2, 1), "selectionRange": range(0, 6, 0, 10),
         "children": [{"name": "total", "kind": 6, "detail": "()", "range": range(1, 2, 1, 12), "selectionRange": range(1, 2, 1, 7)}]},
        {"name": "main", "kind": 12, "range": range(3, 0, 3, 18), "selectionRange": range(3, 9, 3, 13)},
        {"name": "ns", "kind": 3, "range": range(3, 0, 3, 1)},
    ]);
    let found = symbols(&tree, &lines, Encoding::Utf16);
    let names: Vec<(&str, SymbolKind, Option<&str>)> =
        found.iter().map(|s| (s.name.as_ref(), s.kind, s.container.as_deref())).collect();
    assert_eq!(
        names,
        [
            ("Cart", SymbolKind::Class, None),
            ("total", SymbolKind::Method, Some("Cart")),
            ("main", SymbolKind::Function, None)
        ]
    );
    assert_eq!(found[1].fqn, "Cart.total".into());
    assert_eq!(found[1].offset, text.find("total").unwrap());
    assert_eq!(found[1].line, 1);
    // A flat list (symbol information).
    let flat = json!([{"name": "main", "kind": 12, "location": {"uri": "file:///x", "range": range(3, 0, 3, 18)}, "containerName": ""}]);
    assert_eq!(symbols(&flat, &lines, Encoding::Utf16)[0].container, None);
}

#[test]
fn a_workspace_edit_by_uri_or_document_with_renames() {
    let mut texts = held();
    let by_uri =
        json!({"changes": {"file:///p/b.ts": [{"range": range(0, 13, 0, 14), "newText": "c"}]}});
    let change = workspace_edit(&by_uri, &mut texts, Encoding::Utf16);
    assert_eq!(change.files.len(), 1);
    assert_eq!(
        change.files[0].edits,
        vec![ChangeEdit { range: 13..14, original: "b".into(), text: "c".into(), line: 0 }]
    );
    let documents = json!({"documentChanges": [
        {"textDocument": {"uri": "file:///p/a.ts", "version": 3}, "edits": [{"range": range(1, 4, 1, 9), "newText": "sum"}]},
        {"kind": "rename", "oldUri": "file:///p/b.ts", "newUri": "file:///p/c.ts"},
        {"kind": "create", "uri": "file:///p/d.ts"},
    ]});
    let change = workspace_edit(&documents, &mut texts, Encoding::Utf16);
    assert_eq!(change.files[0].edits[0].original, "total");
    assert_eq!(change.renames, vec![(PathBuf::from("/p/b.ts"), PathBuf::from("/p/c.ts"))]);
    assert!(workspace_edit(&json!({}), &mut texts, Encoding::Utf16).is_empty());
}

#[test]
fn a_formattings_overlapping_edits_keep_the_first() {
    let edits = json!([{"range": range(0, 0, 0, 5), "newText": "let"}, {"range": range(0, 2, 0, 8), "newText": "x"}, {"range": range(1, 0, 1, 0), "newText": "  "}]);
    let change = file_change(Path::new("/p/a.ts"), &edits, &lines(), Encoding::Utf16);
    assert_eq!(change.edits.len(), 2);
    assert_eq!(change.edits[0].original, "const");
    assert_eq!(change.edits[1].line, 1);
}

#[test]
fn diagnostics_count_characters_and_say_who_found_them() {
    let items = json!([
        {"range": range(1, 12, 1, 14), "severity": 1, "code": 2304, "message": "Cannot find name.", "source": "ts"},
        {"range": range(0, 0, 0, 5), "severity": 4, "message": "Unused."},
        {"range": range(0, 0, 0, 1)},
    ]);
    let found = diagnostics(&items, Path::new("src/a.ts"), &lines(), Encoding::Utf16, "vtsls");
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].path, Path::new("src/a.ts"));
    // `let total = ` is 12 characters, the emoji one more.
    assert_eq!((found[0].line, found[0].column, found[0].end), (1, Some(12), Some((1, 13))));
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].rule.as_deref(), Some("2304"));
    assert_eq!(found[0].source, "ts");
    assert_eq!(found[1].severity, Severity::Info);
    assert_eq!(found[1].source, "vtsls");
}

#[test]
fn code_actions_skip_the_disabled_ones() {
    let value = json!([
        {"title": "Add import", "kind": "quickfix", "isPreferred": true, "edit": {}},
        {"title": "Extract", "kind": "refactor.extract", "disabled": {"reason": "no"}},
        {"title": "Organize imports", "command": "x"},
    ]);
    let actions = code_actions(&value);
    let titles: Vec<&str> = actions.iter().map(|(a, _)| a.title.as_str()).collect();
    assert_eq!(titles, ["Add import", "Organize imports"]);
    assert!(actions[0].0.preferred);
    assert_eq!(actions[1].0.id, 1);
}

#[test]
fn texts_read_from_disk_have_the_editors_line_endings() {
    let dir = std::env::temp_dir().join(format!("lsp-convert-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("crlf.ts");
    std::fs::write(&file, "a\r\nbc\r\n").unwrap();
    let mut disk = DiskTexts::default();
    assert_eq!(disk.read(&file).unwrap().text(), "a\nbc\n");
    assert!(disk.read(&dir.join("none.ts")).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn kinds_are_mapped() {
    assert_eq!(completion_kind(3), CompletionKind::Function);
    assert_eq!(completion_kind(7), CompletionKind::Class);
    assert_eq!(completion_kind(14), CompletionKind::Keyword);
    assert_eq!(completion_kind(99), CompletionKind::Value);
    assert_eq!(symbol_kind(11), Some(SymbolKind::Interface));
    assert_eq!(symbol_kind(3), None);
}

#[test]
fn a_descriptions_completion_kinds_change_only_those_it_names() {
    let names: std::collections::BTreeMap<String, String> =
        [("property".to_owned(), "tag".to_owned()), ("unit".to_owned(), "value".to_owned())].into();
    let kinds = CompletionKinds::parse(&names).unwrap();
    assert_eq!(kinds.of(10), CompletionKind::Tag);
    assert_eq!(kinds.of(11), CompletionKind::Value);
    // The rest keep LSP's general meaning.
    assert_eq!(kinds.of(3), CompletionKind::Function);
    assert_eq!(kinds.of(6), CompletionKind::Variable);
    assert_eq!(CompletionKinds::default().of(10), CompletionKind::Property);
    let answer = json!([{"label": "color", "kind": 10}, {"label": "red", "kind": 12}]);
    let served = completion_with(&answer, &lines(), Encoding::Utf16, 0, &kinds);
    let shown: Vec<CompletionKind> = served.items.iter().map(|i| i.item.kind).collect();
    assert_eq!(shown, [CompletionKind::Tag, CompletionKind::Value]);
    assert_eq!(LSP_COMPLETION_KINDS.len(), 25);
}

#[test]
fn the_css_and_html_servers_kinds_are_the_built_in_engines() {
    let read = |toml: &str| crate::spec::parse(toml, "m").unwrap().remove(0).completion_kinds;
    let css = read(include_str!("../../../../../modules/languages/css/data/language-servers.toml"));
    let css = CompletionKinds::parse(&css).unwrap();
    assert_eq!(css.of(10), CompletionKind::Attribute, "a CSS property");
    let html =
        read(include_str!("../../../../../modules/languages/html/data/language-servers.toml"));
    let html = CompletionKinds::parse(&html).unwrap();
    assert_eq!(
        [html.of(10), html.of(12), html.of(11)],
        [CompletionKind::Tag, CompletionKind::Attribute, CompletionKind::Value]
    );
}
