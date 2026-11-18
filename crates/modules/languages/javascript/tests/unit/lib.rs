use super::*;
use code_intelligence::{LanguageIntelligence, SymbolKind};

/// Each grammar's queries compile on its own grammar (JavaScript's highlights work on
/// TypeScript's; JSX's only in TSX).
#[test]
fn every_grammar_s_queries_compile() {
    for g in [grammar(), typescript_grammar(), tsx_grammar()] {
        for (what, query) in
            [("highlights", &g.highlights), ("injections", &g.injections), ("folds", &g.folds)]
        {
            assert!(
                Query::new(&g.ts, query).is_ok(),
                "{:?} {what}: {:?}",
                g.language,
                Query::new(&g.ts, query).err()
            );
        }
    }
}

/// TypeScript's declarations are indexed: interfaces, enums, types, classes (abstract too)
/// with their typed members and parent, functions.
#[test]
fn typescript_declarations_are_indexed() {
    let ts = JsIntelligence::new(Dialect::TypeScript);
    let src = "export interface Shape { area(): number }\n\
               export enum Color { Red, Green }\n\
               type Id = string | number;\n\
               export abstract class Base {}\n\
               export class Square extends Base implements Shape {\n\
                 private side: number = 1;\n\
                 area(): number { return this.side ** 2; }\n\
               }\n\
               export function make(side: number): Square { return new Square(); }\n";
    let symbols = ts.extract(src).symbols;
    let found: Vec<(&str, SymbolKind)> = symbols.iter().map(|s| (&*s.name, s.kind)).collect();
    for expected in [
        ("Shape", SymbolKind::Interface),
        ("Color", SymbolKind::Enum),
        ("Id", SymbolKind::Class),
        ("Base", SymbolKind::Class),
        ("Square", SymbolKind::Class),
        ("side", SymbolKind::Property),
        ("area", SymbolKind::Method),
        ("make", SymbolKind::Function),
    ] {
        assert!(found.contains(&expected), "{expected:?} in {found:?}");
    }
    let square = symbols.iter().find(|s| &*s.name == "Square").unwrap();
    assert_eq!(square.parents.iter().map(|p| &**p).collect::<Vec<_>>(), ["Base"]);
    assert_eq!(ts.extensions(), ["ts", "mts", "cts"]);
}

/// TSX: components and their JSX parse; an import used only as a type isn't unused.
#[test]
fn tsx_parses_and_type_only_uses_count() {
    let tsx = JsIntelligence::new(Dialect::Tsx);
    let src = "import { Props } from './props';\n\
               export function Card(props: Props) { return <div className=\"card\">{props.title}</div>; }\n";
    let names: Vec<String> = tsx.extract(src).symbols.iter().map(|s| s.name.to_string()).collect();
    assert_eq!(names, ["Card"]);
    assert!(tsx.unused(src).is_empty(), "{:?}", tsx.unused(src));
}
