use super::*;

fn roots() -> NamespaceRoots {
    NamespaceRoots::new(
        NameSeparator('\\'),
        [
            ("App\\".to_owned(), PathBuf::from("/p/app")),
            ("Tests\\".to_owned(), PathBuf::from("/p/tests")),
        ],
    )
}

#[test]
fn folders_and_namespaces_map_both_ways() {
    let roots = roots();
    assert_eq!(roots.namespace_of(Path::new("/p/app/Models")).as_deref(), Some("App\\Models"));
    assert_eq!(roots.namespace_of(Path::new("/p/app")).as_deref(), Some("App"));
    assert_eq!(roots.namespace_of(Path::new("/p/other")), None);
    assert_eq!(roots.namespace_of(Path::new("/p/app/my-dir")), None, "not a name");
    assert_eq!(roots.dir_of("App\\Domain\\Users"), Some(PathBuf::from("/p/app/Domain/Users")));
    assert_eq!(roots.dir_of("App"), Some(PathBuf::from("/p/app")));
    assert_eq!(roots.dir_of("Application"), None, "a prefix only by whole names");
}

fn user_target(into: &str) -> MoveTarget {
    let class = ClassFile {
        fqn: "App\\Models\\User".into(),
        path: PathBuf::from("/p/app/Models/User.php"),
        subdirs: Vec::new(),
    };
    MoveTarget::new(
        PathBuf::from("/p/app/Models/User.php"),
        false,
        PathBuf::from(into),
        vec![class],
        &roots(),
    )
}

#[test]
fn a_moved_class_takes_the_namespace_of_its_folder() {
    let target = user_target("/p/app/Domain");
    assert_eq!(target.namespace.as_deref(), Some("App\\Models"));
    assert_eq!(target.initial_namespace(), "App\\Domain");
    assert_eq!(
        target.moves("App\\Domain"),
        vec![("App\\Models\\User".into(), "App\\Domain\\User".into())]
    );
    // A namespace typed: its folder.
    assert_eq!(
        target.destination("App\\Billing", &roots()),
        PathBuf::from("/p/app/Billing/User.php")
    );
}

#[test]
fn a_moved_folder_keeps_its_subfolders_namespaces() {
    let class = |fqn: &str, path: &str, subdirs: &[&str]| ClassFile {
        fqn: fqn.into(),
        path: PathBuf::from(path),
        subdirs: subdirs.iter().map(|s| (*s).to_owned()).collect(),
    };
    let target = MoveTarget::new(
        PathBuf::from("/p/app/Models"),
        true,
        PathBuf::from("/p/app/Domain"),
        vec![
            class("App\\Models\\User", "/p/app/Models/User.php", &[]),
            class("App\\Models\\Casts\\Money", "/p/app/Models/Casts/Money.php", &["Casts"]),
            // Not where its folders say: left alone.
            class("Legacy\\Thing", "/p/app/Models/Thing.php", &[]),
        ],
        &roots(),
    );
    assert_eq!(target.initial_namespace(), "App\\Domain\\Models");
    assert_eq!(
        target.moves("App\\Domain\\Models"),
        vec![
            ("App\\Models\\User".into(), "App\\Domain\\Models\\User".into()),
            ("App\\Models\\Casts\\Money".into(), "App\\Domain\\Models\\Casts\\Money".into()),
        ]
    );
    assert_eq!(
        target.destination("App\\Domain\\Models", &roots()),
        PathBuf::from("/p/app/Domain/Models")
    );
}

struct Rules;

impl NamespaceRules for Rules {
    fn extensions(&self) -> &'static [&'static str] {
        &["php"]
    }
    fn scan(&self, _text: &str) -> FileScan {
        FileScan::default()
    }
    fn separator(&self) -> NameSeparator {
        NameSeparator('\\')
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
        fqn == "Exception"
    }
    fn roots(&self, _root: &Path, _read: &dyn Fn(&Path) -> Option<String>) -> NamespaceRoots {
        NamespaceRoots::default()
    }
}

struct Known;

impl Types for Known {
    fn is_type(&self, fqn: &str) -> bool {
        fqn == "App\\Models\\Post"
    }
}

struct NoSources;

impl Sources for NoSources {
    fn candidates(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn read(&self, _path: &Path) -> Option<String> {
        None
    }
}

fn span(text: &str, what: &str) -> Range<usize> {
    let at = text.find(what).unwrap();
    at..at + what.len()
}

fn name(text: &str, what: &str, fully: bool) -> NameRef {
    let mut range = span(text, what);
    if fully {
        range.start -= 1;
    }
    NameRef { range, name: what.into(), fully_qualified: fully }
}

fn context(target: MoveTarget) -> MoveContext {
    MoveContext {
        target,
        roots: roots(),
        root: PathBuf::from("/p"),
        sources: Arc::new(NoSources),
        rules: Some(Arc::new(Rules)),
        types: Arc::new(Known),
    }
}

fn apply(file: &ScannedFile, plan: &Plan) -> String {
    let edits = plan.edits().into_iter().find(|e| e.path == file.path).map(|e| e.edits);
    crate::apply_edits(&file.text, &edits.unwrap_or_default()).unwrap()
}

#[test]
fn the_moved_file_gets_its_namespace_and_imports_its_old_neighbours() {
    let text = "<?php\nnamespace App\\Models;\n\nclass User extends Post {}\n";
    let block = Block {
        namespace: "App\\Models".into(),
        name_range: Some(span(text, "App\\Models")),
        statement: Some(span(text, "namespace App\\Models;")),
        declare_at: 5,
        import_at: span(text, "namespace App\\Models;").end,
        names: vec![name(text, "Post", false)],
        declared: vec!["User".into()],
        ..Block::default()
    };
    let file = ScannedFile {
        path: PathBuf::from("/p/app/Models/User.php"),
        text: text.into(),
        scan: FileScan { blocks: vec![block], strings: Vec::new() },
    };
    let context = context(user_target("/p/app/Domain"));
    let facts = MoveFacts { files: vec![file.clone()] };
    let to = PathBuf::from("/p/app/Domain/User.php");
    let plan = plan_move(&context, &facts, "App\\Domain", &to);
    assert_eq!(plan.renames[0].to, to);
    assert_eq!(
        apply(&file, &plan),
        "<?php\nnamespace App\\Domain;\n\nuse App\\Models\\Post;\n\nclass User extends Post {}\n"
    );
}

#[test]
fn users_follow_imports_full_names_and_strings() {
    let text = "<?php\nnamespace App\\Http;\n\nuse App\\Models\\User;\n\n$a = new \\App\\Models\\User();\n$b = 'App\\Models\\User';\n";
    let string = span(text, "App\\Models\\User';");
    let block = Block {
        namespace: "App\\Http".into(),
        name_range: Some(span(text, "App\\Http")),
        statement: Some(span(text, "namespace App\\Http;")),
        imports: vec![Import {
            fqn: "App\\Models\\User".into(),
            alias: "User".into(),
            range: span(text, "App\\Models\\User;").start..span(text, "App\\Models\\User;").end - 1,
            item: 0..0,
            group: None,
        }],
        import_at: span(text, "use App\\Models\\User;").end,
        names: vec![NameRef {
            range: span(text, "\\App\\Models\\User()").start
                ..span(text, "\\App\\Models\\User()").end - 2,
            name: "App\\Models\\User".into(),
            fully_qualified: true,
        }],
        ..Block::default()
    };
    let file = ScannedFile {
        path: PathBuf::from("/p/app/Http/C.php"),
        text: text.into(),
        scan: FileScan {
            blocks: vec![block],
            strings: std::iter::once(string.start..string.end - 2).collect(),
        },
    };
    let context = context(user_target("/p/app/Domain"));
    let mut plan = plan_move(
        &context,
        &MoveFacts { files: vec![file.clone()] },
        "App\\Domain",
        Path::new("/x"),
    );
    let strings: Vec<_> =
        plan.files[0].occurrences.iter().filter(|o| o.certainty == Certainty::Uncertain).collect();
    assert_eq!(strings.len(), 1, "the string is unsure");
    plan.choose_all(true);
    assert_eq!(
        apply(&file, &plan),
        "<?php\nnamespace App\\Http;\n\nuse App\\Domain\\User;\n\n$a = new \\App\\Domain\\User();\n$b = 'App\\Domain\\User';\n"
    );
}

#[test]
fn only_what_changes_is_moved() {
    let root = Path::new("/p");
    assert!(movable(root, Path::new("/p/app/User.php")));
    assert!(!movable(root, root), "not the project itself");
    assert!(!movable(root, Path::new("/q/a.php")), "not outside it");
    assert!(moves_somewhere(Path::new("/p/app/User.php"), Path::new("/p/src")));
    assert!(!moves_somewhere(Path::new("/p/app/User.php"), Path::new("/p/app")), "its own folder");
    assert!(!moves_somewhere(Path::new("/p/app"), Path::new("/p/app/Models")), "into itself");
}

#[test]
fn the_classes_moved_are_those_named_after_their_files() {
    use code_intelligence::{FileSymbols, Symbol, SymbolKind};
    let file = |name: &str, fqn: &str, kind| FileSymbols {
        symbols: vec![Symbol {
            name: name.into(),
            kind,
            fqn: fqn.into(),
            container: None,
            parents: Box::default(),
            range: 0..0,
            offset: 0,
            line: 0,
            detail: None,
            is_static: false,
            visibility: Default::default(),
            note: None,
            types: None,
        }],
        ..FileSymbols::default()
    };
    let indexed = [
        (
            PathBuf::from("/p/app/Models/User.php"),
            file("User", "App\\Models\\User", SymbolKind::Class),
        ),
        (
            PathBuf::from("/p/app/Models/Sub/Post.PHP"),
            file("Post", "App\\Models\\Sub\\Post", SymbolKind::Class),
        ),
        (
            PathBuf::from("/p/app/Models/helpers.php"),
            file("helper", "helper", SymbolKind::Function),
        ),
        (
            PathBuf::from("/p/app/Models/Odd.php"),
            file("Other", "App\\Models\\Other", SymbolKind::Class),
        ),
        (PathBuf::from("/p/app/Models/Card.js"), file("Card", "Card", SymbolKind::Class)),
        (
            PathBuf::from("/p/app/Http/Kernel.php"),
            file("Kernel", "App\\Http\\Kernel", SymbolKind::Class),
        ),
    ];
    let files = || indexed.iter().map(|(p, f)| (p.as_path(), f));
    let folder = classes_in(files(), Path::new("/p/app/Models"), true, &["php"]);
    let found: Vec<(&str, Vec<String>)> =
        folder.iter().map(|c| (c.fqn.as_str(), c.subdirs.clone())).collect();
    assert_eq!(
        found,
        [("App\\Models\\Sub\\Post", vec!["Sub".to_owned()]), ("App\\Models\\User", Vec::new())]
    );
    let one = classes_in(files(), Path::new("/p/app/Models/User.php"), false, &["php"]);
    assert_eq!(one.len(), 1);
    assert!(one[0].subdirs.is_empty());
}
