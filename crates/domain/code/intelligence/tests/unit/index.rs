use super::*;

fn sym(
    name: &str,
    kind: SymbolKind,
    fqn: &str,
    container: Option<&str>,
    parents: &[&str],
) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        fqn: fqn.into(),
        container: container.map(Into::into),
        parents: parents.iter().map(|p| (*p).into()).collect(),
        range: 0..10,
        offset: 0,
        line: 0,
        detail: None,
        is_static: false,
        visibility: Default::default(),
        note: None,
        types: None,
    }
}

#[test]
fn members_include_parents_and_files_can_be_replaced() {
    let mut index = SymbolIndex::default();
    let base = FileSymbols {
        symbols: vec![
            sym("Model", SymbolKind::Class, "App\\Model", None, &[]),
            sym("save", SymbolKind::Method, "save", Some("App\\Model"), &[]),
        ],
        ..Default::default()
    };
    let user = FileSymbols {
        symbols: vec![
            sym("User", SymbolKind::Class, "App\\User", None, &["App\\Model"]),
            sym("name", SymbolKind::Property, "name", Some("App\\User"), &[]),
        ],
        ..Default::default()
    };
    index.set_file(Path::new("/p/Model.php"), base);
    index.set_file(Path::new("/p/User.php"), user.clone());
    let names: Vec<&str> = index.members_of("app\\user").iter().map(|(_, s)| &*s.name).collect();
    assert_eq!(names, ["name", "save"]);
    let own: Vec<&str> = index.declared_members("App\\User").map(|(_, s)| &*s.name).collect();
    assert_eq!(own, ["name"], "declared in it, not inherited");
    assert!(index.type_by_fqn("APP\\USER").is_some());

    index.set_file(Path::new("/p/User.php"), FileSymbols::default());
    assert!(index.type_by_fqn("App\\User").is_none());
    assert!(index.by_name("name").is_empty());
    index.set_file(Path::new("/p/User.php"), user);
    assert_eq!(index.by_name("user").len(), 1);
}

/// A project's worth of files alike (a class, its constructor and two dozen methods with the
/// same names in every file: the short-name buckets are as long as the project).
fn project(files: usize) -> SymbolIndex {
    let mut index = SymbolIndex::default();
    for f in 0..files {
        index.set_file(Path::new(&format!("/p/src/Module{}/Service{f}.php", f / 50)), service(f));
    }
    index
}

fn service(f: usize) -> FileSymbols {
    let class = format!("App\\Module{}\\Service{f}", f / 50);
    let mut symbols = vec![sym(&format!("Service{f}"), SymbolKind::Class, &class, None, &[])];
    for m in ["__construct", "jsonSerialize"]
        .into_iter()
        .map(str::to_owned)
        .chain((0..12).flat_map(|m| [format!("method{m}"), format!("helper{m}")]))
    {
        symbols.push(sym(&m, SymbolKind::Method, &m, Some(&class), &[]));
    }
    FileSymbols { symbols, ..Default::default() }
}

#[test]
fn replacing_a_file_in_a_large_index_leaves_the_others_alone() {
    let mut index = project(300);
    index.set_file(Path::new("/p/src/Module1/Service60.php"), service(60));
    index.set_file(Path::new("/p/src/Module1/Service60.php"), service(60));
    assert_eq!(index.by_name("method3").len(), 300);
    assert_eq!(index.members_of("app\\module1\\service60").len(), 26);
    index.remove_file(Path::new("/p/src/Module1/Service60.php"));
    assert_eq!(index.by_name("method3").len(), 299);
    assert!(index.type_by_fqn("App\\Module1\\Service60").is_none());
    assert!(index.type_by_fqn("App\\Module1\\Service61").is_some());
}

#[test]
fn the_top_level_of_some_files_is_the_top_level_filtered_by_file() {
    let mut index = project(120);
    let mut css = service(7);
    css.language = "css".into();
    index.set_file(Path::new("/p/style.css"), css);
    let mut all: Vec<(String, String)> = index
        .top_level()
        .filter(|(p, _)| index.file(p).is_some_and(|f| f.language.is_empty()))
        .map(|(p, s)| (p.display().to_string(), s.fqn.to_string()))
        .collect();
    let mut kept: Vec<(String, String)> = index
        .top_level_where(|f| f.language.is_empty())
        .map(|(p, s)| (p.display().to_string(), s.fqn.to_string()))
        .collect();
    all.sort();
    kept.sort();
    assert_eq!(all.len(), 120);
    assert_eq!(kept, all);
}

/// `cargo test -p code-intelligence --release measure_set_file -- --ignored --nocapture`
#[test]
#[ignore = "a measurement"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the domain.
fn measure_set_file() {
    let mut index = project(5000);
    let path = Path::new("/p/src/Module3/Service170.php");
    let started = std::time::Instant::now();
    for _ in 0..100 {
        index.set_file(path, service(170));
    }
    println!("set_file in a 5000-file index: {:?} each", started.elapsed() / 100);
}

/// `cargo test -p code-intelligence --release measure_top_level -- --ignored --nocapture`
#[test]
#[ignore = "a measurement"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the domain.
fn measure_top_level() {
    let index = project(5000);
    let started = std::time::Instant::now();
    let mut n = 0;
    for _ in 0..100 {
        n += index.top_level().filter(|(_, s)| s.name.contains('x')).count();
    }
    println!("top_level over 5000 files: {:?} each ({n})", started.elapsed() / 100);
    let started = std::time::Instant::now();
    for _ in 0..100 {
        n += index
            .top_level_where(|f| f.language.is_empty())
            .filter(|(_, s)| s.name.contains('x'))
            .count();
    }
    println!("top_level_where over 5000 files: {:?} each ({n})", started.elapsed() / 100);
}

#[test]
fn a_key_finds_its_text_whatever_the_case_and_only_it() {
    assert!(same_text("App\\Models\\User", "app\\models\\user"));
    assert!(same_text("Ștefan", "ștefan"));
    assert!(!same_text("User", "users"));
    assert_eq!(key("user"), key("user"));
    assert_ne!(key("user"), key("users"));
}

#[test]
fn a_folder_accounts_for_its_files_and_the_shares_add_up_to_the_whole() {
    let mut index = SymbolIndex::default();
    let file = |name: &str| FileSymbols {
        symbols: vec![sym(name, SymbolKind::Class, name, None, &[])],
        ..FileSymbols::default()
    };
    index.set_file(Path::new("/p/src/a/One.php"), file("One"));
    index.set_file(Path::new("/p/vendor/Two.php"), file("Two"));
    index.set_file(Path::new("/elsewhere/Three.php"), file("Three"));
    let folders = index.memory_by_folder(Path::new("/p"), 2);
    assert!(folders["src"] > 0 && folders["vendor"] > 0);
    assert_eq!(folders["src"], folders["src/a"], "all of it is in there");
    assert!(folders["src"] + folders["vendor"] <= index.memory());
    assert!(!folders.contains_key("elsewhere"));
    assert_eq!(index.memory_by_folder(Path::new("/p"), 1).get("src/a"), None);
}

/// A copy of the index held (a long job reading it): a file changed after copies only the
/// shards it falls in, the rest stays shared; the copy keeps what it had.
#[test]
fn a_change_after_a_copy_copies_only_its_shards() {
    let mut index = project(300);
    let copy = index.clone();
    index.set_file(Path::new("/p/src/Module1/Service60.php"), service(60));
    let shared = |a: &Table, b: &Table| {
        a.shards.iter().zip(&b.shards).filter(|(x, y)| Arc::ptr_eq(x, y)).count()
    };
    // The file's 26 symbols in at most as many shards of the long tables: most stay shared.
    assert!(shared(&index.by_name, &copy.by_name) >= SHARDS - 26);
    assert!(shared(&index.by_fqn, &copy.by_fqn) >= SHARDS - 2);
    assert_eq!(copy.by_name("method3").len(), 300, "the copy keeps what it had");
    assert_eq!(index.by_name("method3").len(), 300);
}

/// `cargo test -p code-intelligence --release measure_set_file_while_copied -- --ignored --nocapture`
#[test]
#[ignore = "a measurement"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the domain.
fn measure_set_file_while_copied() {
    let mut index = project(5000);
    let path = Path::new("/p/src/Module3/Service170.php");
    let mut worst = std::time::Duration::ZERO;
    let mut total = std::time::Duration::ZERO;
    for _ in 0..20 {
        let copy = index.clone();
        let started = std::time::Instant::now();
        index.set_file(path, service(170));
        let took = started.elapsed();
        worst = worst.max(took);
        total += took;
        drop(copy);
    }
    println!("set_file while a copy is held, 5000 files: {:?} each, worst {worst:?}", total / 20);
}

/// A packed file's codec for the tests: its symbols kept aside, and how many times it was
/// asked to unpack them.
#[derive(Debug)]
struct Counted {
    whole: FileSymbols,
    unpacked: std::sync::atomic::AtomicUsize,
}

impl Unpack for Counted {
    fn unpack(&self, _bytes: &[u8]) -> Option<FileSymbols> {
        self.unpacked.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(self.whole.clone())
    }
}

fn library() -> FileSymbols {
    FileSymbols {
        language: "php".into(),
        symbols: vec![
            sym("Builder", SymbolKind::Class, "Lib\\Builder", None, &["Lib\\Base"]),
            sym("where", SymbolKind::Method, "where", Some("Lib\\Builder"), &[]),
            sym("orderBy", SymbolKind::Method, "orderBy", Some("Lib\\Builder"), &[]),
            sym("helper", SymbolKind::Function, "Lib\\helper", None, &[]),
        ],
        ..FileSymbols::default()
    }
}

fn packed(index: &mut SymbolIndex, path: &str) -> Arc<Counted> {
    let codec = Arc::new(Counted { whole: library(), unpacked: Default::default() });
    let unpack: Arc<dyn Unpack> = Arc::clone(&codec) as _;
    index.set_file_packed(Path::new(path), &library(), Arc::from(&b"packed"[..]), unpack);
    codec
}

fn unpacked(codec: &Counted) -> usize {
    codec.unpacked.load(std::sync::atomic::Ordering::Relaxed)
}

/// A library file kept packed (plan 74): its types and functions found with nothing
/// unpacked; its members unpacked once, at the first ask, then kept; the file whole when
/// asked for it.
#[test]
fn a_packed_file_unpacks_once_when_its_members_are_asked_for() {
    let mut index = SymbolIndex::default();
    let codec = packed(&mut index, "/p/vendor/Builder.php");
    assert_eq!(index.type_by_fqn("Lib\\Builder").map(|(_, s)| &*s.name), Some("Builder"));
    assert!(index.function_by_fqn("Lib\\helper").is_some());
    assert_eq!(index.top_level().count(), 2);
    assert_eq!(index.language_of(Path::new("/p/vendor/Builder.php")), Some("php"));
    assert_eq!(index.len(), 4, "its members counted");
    assert_eq!(unpacked(&codec), 0, "nothing unpacked for its types and functions");
    let members: Vec<&str> =
        index.declared_members("Lib\\Builder").map(|(_, s)| &*s.name).collect();
    assert_eq!(members, ["where", "orderBy"]);
    assert_eq!(index.by_name("orderby").len(), 1);
    assert_eq!(index.file(Path::new("/p/vendor/Builder.php")).unwrap().symbols.len(), 4);
    assert_eq!(unpacked(&codec), 1, "once, then kept");
    let kept = index.files().next().unwrap().1;
    assert_eq!(kept.symbols.len(), 2, "the walks over every file see its types and functions");
}

/// A member looked for by name over every file (the type unknown): a packed file is
/// unpacked only if one of its members' names matches, and not kept after.
#[test]
fn a_name_looked_for_unpacks_only_the_files_with_it_and_keeps_none() {
    let mut index = SymbolIndex::default();
    let codec = packed(&mut index, "/p/vendor/Builder.php");
    let mut found = Vec::new();
    index.for_each_member_named(&|name| name.starts_with("ord"), |_, s| found.push(s.name.clone()));
    assert_eq!(found, ["orderBy".into()]);
    assert_eq!(unpacked(&codec), 1);
    index.for_each_member_named(&|name| name == "nothing", |_, _| panic!("no such member"));
    assert_eq!(unpacked(&codec), 1, "no name of it matched: not unpacked");
    index.for_each_member_named(&|name| name == "where", |_, _| {});
    assert_eq!(unpacked(&codec), 2, "not kept by a walk");
    let symbols = index.file_once(Path::new("/p/vendor/Builder.php")).unwrap();
    assert_eq!(symbols.symbols.len(), 4);
    assert_eq!(unpacked(&codec), 3);
}

/// A packed file replaced or removed: its entries go (its members unpacked for that only).
#[test]
fn a_packed_file_removed_leaves_nothing() {
    let mut index = SymbolIndex::default();
    packed(&mut index, "/p/vendor/Builder.php");
    index.remove_file(Path::new("/p/vendor/Builder.php"));
    assert!(index.type_by_fqn("Lib\\Builder").is_none());
    assert_eq!(index.declared_members("Lib\\Builder").count(), 0);
    assert!(index.by_name("where").is_empty());
    assert!(index.is_empty());
}

/// Two projects with the same version of a library (the same bytes, the same codec): its
/// file held once for both, unpacked once for both (plan 76b); another version is its own;
/// one project's index gone, the other's keeps it.
#[test]
fn the_same_library_file_in_two_projects_is_held_once() {
    let codec = Arc::new(Counted { whole: library(), unpacked: Default::default() });
    let unpack: Arc<dyn Unpack> = Arc::clone(&codec) as _;
    let bytes = |text: &str| -> Arc<[u8]> { Arc::from(text.as_bytes()) };
    let (mut one, mut two) = (SymbolIndex::default(), SymbolIndex::default());
    let path = |project: &str| PathBuf::from(format!("/{project}/vendor/Builder.php"));
    let a = one.set_file_packed(&path("one"), &library(), bytes("v1"), Arc::clone(&unpack));
    let b = two.set_file_packed(&path("two"), &library(), bytes("v1"), Arc::clone(&unpack));
    assert!(Arc::ptr_eq(&a, &b), "the second project keeps the first one's bytes");
    assert_eq!(one.declared_members("Lib\\Builder").count(), 2);
    assert_eq!(two.declared_members("Lib\\Builder").count(), 2);
    assert_eq!(unpacked(&codec), 1, "unpacked once for both");
    // Each project's own paths.
    assert_eq!(two.type_by_fqn("Lib\\Builder").map(|(p, _)| p.to_path_buf()), Some(path("two")));
    // Another version in a third project: its own.
    let mut three = SymbolIndex::default();
    let c = three.set_file_packed(&path("three"), &library(), bytes("v2"), Arc::clone(&unpack));
    assert!(!Arc::ptr_eq(&a, &c));
    assert_eq!(three.declared_members("Lib\\Builder").count(), 2);
    assert_eq!(unpacked(&codec), 2);
    // A package only the first project has: never found in the second (each project's
    // tables list its own files only; only the content is shared).
    let only = FileSymbols {
        language: "php".into(),
        symbols: vec![sym("Only", SymbolKind::Class, "Pkg\\Only", None, &[])],
        ..FileSymbols::default()
    };
    one.set_file_packed(
        Path::new("/one/vendor/Only.php"),
        &only,
        bytes("only"),
        Arc::clone(&unpack),
    );
    assert!(one.type_by_fqn("Pkg\\Only").is_some());
    assert!(two.type_by_fqn("Pkg\\Only").is_none(), "not the other project's");
    assert!(two.by_name("only").is_empty());
    drop(one);
    assert_eq!(two.declared_members("Lib\\Builder").count(), 2, "the other keeps it");
    assert_eq!(unpacked(&codec), 2);
}

/// The same bytes read by another codec (another format) are another file: each one
/// unpacked by its own.
#[test]
fn the_same_bytes_with_another_codec_are_not_shared() {
    let first = Arc::new(Counted { whole: library(), unpacked: Default::default() });
    let second = Arc::new(Counted { whole: library(), unpacked: Default::default() });
    let bytes: Arc<[u8]> = Arc::from(&b"codec-apart"[..]);
    let (mut one, mut two) = (SymbolIndex::default(), SymbolIndex::default());
    let codec = |c: &Arc<Counted>| -> Arc<dyn Unpack> { Arc::clone(c) as _ };
    one.set_file_packed(Path::new("/a/x.php"), &library(), Arc::clone(&bytes), codec(&first));
    two.set_file_packed(Path::new("/b/x.php"), &library(), bytes, codec(&second));
    assert_eq!(one.declared_members("Lib\\Builder").count(), 2);
    assert_eq!(two.declared_members("Lib\\Builder").count(), 2);
    assert_eq!((unpacked(&first), unpacked(&second)), (1, 1), "each by its own");
}

/// Each change of an index gives it a version none had (what's found in it is kept while
/// the version stays); a copy keeps the one it was copied with.
#[test]
fn every_change_of_an_index_gives_it_a_new_version() {
    let mut index = SymbolIndex::default();
    let first = index.version();
    index.set_file(Path::new("/p/a.php"), FileSymbols::default());
    let second = index.version();
    assert_ne!(first, second);
    let copy = index.clone();
    assert_eq!(copy.version(), second);
    index.remove_file(Path::new("/p/a.php"));
    assert!(index.version() != second && copy.version() == second);
}
