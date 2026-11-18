use std::collections::HashMap;

use super::*;

struct Files(HashMap<PathBuf, String>);

impl Sources for Files {
    fn candidates(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.0.keys().cloned().collect();
        paths.sort();
        paths
    }
    fn read(&self, path: &Path) -> Option<String> {
        self.0.get(path).cloned()
    }
}

/// Resolves `save` after `$repo->` to the target, after `$other->` to another symbol, and
/// anything else as unknown.
struct ByReceiver;

impl Resolver for ByReceiver {
    fn resolve(&self, _: &Path, text: &str, range: Range<usize>) -> Resolution {
        let before = &text[..range.start];
        if before.ends_with("$repo->") || before.ends_with("function ") {
            Resolution::Target
        } else if before.ends_with("$other->") {
            Resolution::Other
        } else {
            Resolution::Unknown
        }
    }
}

#[test]
fn plans_a_rename_across_files() {
    let files = Files(HashMap::from([
        ("/p/Repo.php".into(), "<?php class Repo { function save() {} }".into()),
        ("/p/use.php".into(), "<?php $repo->save(); $other->save(); $x->save();".into()),
        ("/p/none.php".into(), "<?php echo 1;".into()),
    ]));
    let target = RenameTarget {
        name: "save".into(),
        style: NameStyle::CODE.with_sigil(Some('$')),
        scope: Scope::Project,
        file: None,
    };
    let plan = plan_rename(&target, &files, &ByReceiver, &|| false);
    assert_eq!(plan.files.len(), 2);
    let used = &plan.files[1];
    assert_eq!(used.occurrences.len(), 2, "the other symbol's call is left out");
    assert_eq!((used.occurrences[0].chosen, used.occurrences[1].chosen), (true, false));
    assert_eq!(used.occurrences[1].certainty, Certainty::Uncertain);
}

#[test]
fn local_variables_stay_in_their_scope() {
    let text = "<?php function a() { $n = 1; return $n; } function b() { $n = 2; }";
    let files = Files(HashMap::from([("/p/a.php".into(), text.to_owned())]));
    let end = text.find("function b").unwrap();
    let target = RenameTarget {
        name: "$n".into(),
        style: NameStyle::CODE.with_sigil(Some('$')),
        scope: Scope::Local { path: "/p/a.php".into(), range: 0..end },
        file: None,
    };
    let plan = plan_rename(&target, &files, &ByReceiver, &|| false);
    assert_eq!(plan.files[0].occurrences.len(), 2);
    assert!(plan.files[0].occurrences.iter().all(|o| o.chosen));
}

/// PHP's ways: `$` before variables, one class per `.php` file named after it.
struct Php;

impl Languages for Php {
    fn naming(&self, path: &Path) -> Naming {
        let php = path.extension().is_some_and(|e| e == "php");
        Naming {
            sigil: php.then_some('$'),
            type_per_file: php.then_some("php"),
            ..Naming::default()
        }
    }
    fn type_per_file(&self, extension: &str) -> bool {
        extension == "php"
    }
}

fn subject(name: &str, path: &str, kind: Option<SymbolKind>) -> Subject {
    use code_intelligence::Location;
    use code_intelligence::rename::Declared;
    Subject {
        name: name.into(),
        declared: Declared {
            at: Location { path: path.into(), offset: 0, line: 0 },
            kind,
            fqn: name.into(),
            container: None,
        },
        local: None,
        language: "php".into(),
    }
}

#[test]
fn a_class_named_after_its_file_renames_the_file() {
    let class = subject("User", "/p/app/User.php", Some(SymbolKind::Class));
    let target = rename_target(&class, Path::new("/p/use.php"), Some(Path::new("/p")), &Php);
    let target = target.unwrap();
    assert_eq!(target.file, Some(PathBuf::from("/p/app/User.php")));
    assert_eq!(target.style, NameStyle::CODE.with_sigil(Some('$')));
    assert_eq!(target.scope, Scope::Project);
    // Not named after it, or not a type: the file stays.
    let other = subject("Other", "/p/app/User.php", Some(SymbolKind::Class));
    assert_eq!(rename_target(&other, Path::new("/p/a.php"), None, &Php).unwrap().file, None);
    let method = subject("User", "/p/app/User.php", Some(SymbolKind::Method));
    assert_eq!(rename_target(&method, Path::new("/p/a.php"), None, &Php).unwrap().file, None);
}

#[test]
fn a_library_symbol_isnt_renamed() {
    // (The dependency folders are the frameworks' to say; Git's is always one.)
    let library = subject("Model", "/p/.git/lib/Model.php", Some(SymbolKind::Class));
    let root = Some(Path::new("/p"));
    let target = rename_target(&library, Path::new("/p/app/User.php"), root, &Php);
    assert_eq!(target, Err(NotRenamed::InLibrary));
    assert!(!in_library(root, Path::new("/p/app/a.php")));
    assert!(!in_library(None, Path::new("/p/.git/a.php")), "no project: nothing is a library");
}

#[test]
fn names_with_characters_of_their_own_take_no_sigil_and_locals_stay_local() {
    let css = subject("card", "/p/a.css", Some(SymbolKind::CssClass));
    let target = rename_target(&css, Path::new("/p/a.php"), None, &Php).unwrap();
    assert_eq!(target.style, NameStyle { extra: "-", sigil: None });
    let mut local = subject("$n", "/p/a.php", None);
    local.local = Some(3..40);
    let target = rename_target(&local, Path::new("/p/a.php"), None, &Php).unwrap();
    assert_eq!(target.scope, Scope::Local { path: "/p/a.php".into(), range: 3..40 });
    assert_eq!(target.file, None);
}

#[test]
fn the_type_named_after_a_file_is_found_in_it() {
    let symbol = |name: &str, kind, offset| code_intelligence::Symbol {
        name: name.into(),
        kind,
        fqn: name.into(),
        container: None,
        parents: Box::default(),
        range: 0..0,
        offset,
        line: 0,
        detail: None,
        is_static: false,
        visibility: Default::default(),
        note: None,
        types: None,
    };
    let symbols = FileSymbols {
        symbols: vec![
            symbol("User", SymbolKind::Function, 6),
            symbol("User", SymbolKind::Class, 20),
        ],
        ..FileSymbols::default()
    };
    assert_eq!(type_named_after(Path::new("/p/User.php"), &symbols, &Php), Some(20));
    assert_eq!(type_named_after(Path::new("/p/Other.php"), &symbols, &Php), None);
    assert_eq!(type_named_after(Path::new("/p/User.js"), &symbols, &Php), None, "not PSR-4");
}
