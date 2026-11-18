use super::*;
use std::path::PathBuf;

use code_intelligence::{ClassDiagram, SymbolHit, SymbolIndex, SymbolQuery};
use text_search::{Hit, SearchQuery};
use workspace::{PathQuery, PathResults};

/// A project of a few files, read from memory.
#[derive(Debug)]
struct Memory(Vec<(&'static str, String)>);

impl ProjectQueries for Memory {
    fn paths(&self, _: &PathQuery<'_>) -> PathResults {
        PathResults::default()
    }
    fn symbols(&self, _: &SymbolQuery<'_>) -> Vec<SymbolHit> {
        Vec::new()
    }
    fn text(&self, _: &SearchQuery) -> Vec<Hit> {
        Vec::new()
    }
    fn files(&self, filter: &FileFilter) -> Vec<PathBuf> {
        let files = self.0.iter().filter(|(p, _)| filter.accepts(p));
        files.map(|(p, _)| self.root().join(p)).collect()
    }
    fn class_diagram(&self, _: &str) -> Option<ClassDiagram> {
        None
    }
    fn root(&self) -> PathBuf {
        PathBuf::from("/p")
    }
    fn file_list(&self) -> Arc<Vec<String>> {
        Arc::default()
    }
    fn symbol_index(&self) -> SymbolIndex {
        SymbolIndex::default()
    }
    fn language_of(&self, _: &Path) -> String {
        String::new()
    }
    fn generation(&self) -> u64 {
        0
    }
    fn list_dir(&self, _: &Path) -> std::io::Result<Vec<workspace::DirEntry>> {
        Ok(Vec::new())
    }
    fn file_icon(&self, _: &str) -> (&'static str, String) {
        ("file", String::new())
    }
    fn read_source(&self, relative: &Path) -> Option<String> {
        let name = relative.to_string_lossy();
        self.0.iter().find(|(p, _)| *p == name).map(|(_, t)| t.clone())
    }
}

fn matcher(pattern: &str, word: bool) -> Matcher {
    let query = document::SearchQuery {
        pattern: pattern.into(),
        whole_word: word,
        case_sensitive: true,
        regex: false,
    };
    Matcher::new(&query).unwrap().unwrap()
}

/// Runs a search to its end: the lines found.
fn found(search: &mut ExactSearch) -> Vec<Found> {
    let mut all = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !search.done() && std::time::Instant::now() < deadline {
        all.extend(search.poll().0);
        std::thread::yield_now();
    }
    all.extend(search.poll().0);
    all
}

fn project() -> Arc<dyn ProjectQueries> {
    Arc::new(Memory(vec![
        ("src/Cart.php", "<?php\nclass Cart {}\n$cart = new Cart();\n".into()),
        ("lib/Cart.php", "Cart\n".into()),
        ("README", "no carts here\n".into()),
    ]))
}

/// Every line with an occurrence, file by file, as the options say; in a folder, only its
/// files.
#[test]
fn every_occurrence_is_found_in_the_folder() {
    let mut all = ExactSearch::start(project(), matcher("Cart", true), Files::default(), None);
    let lines = found(&mut all);
    let places: Vec<(&str, u32)> = lines.iter().map(|f| (f.path.as_str(), f.line.line)).collect();
    assert_eq!(places, [("src/Cart.php", 1), ("src/Cart.php", 2), ("lib/Cart.php", 0)]);
    assert!(all.done() && !all.capped());
    let mut src = ExactSearch::start(
        project(),
        matcher("Cart", true),
        Files { under: Some("src".into()), ..Files::default() },
        None,
    );
    assert!(found(&mut src).iter().all(|f| f.path.starts_with("src/")));
    // A mask: only its files (the others aren't even read).
    let mask = Files { mask: workspace::FileMask::parse("!lib/**"), ..Files::default() };
    let mut masked = ExactSearch::start(project(), matcher("Cart", true), mask, None);
    assert!(found(&mut masked).iter().all(|f| f.path.starts_with("src/")));
}

/// Past the limit, it stops and says there are more.
#[test]
fn it_stops_at_its_limit() {
    let many = "x ".repeat(LIMIT + 10);
    let project: Arc<dyn ProjectQueries> = Arc::new(Memory(vec![("a.txt", many)]));
    let mut search = ExactSearch::start(project, matcher("x", true), Files::default(), None);
    let lines = found(&mut search);
    assert_eq!(lines.iter().map(|f| f.line.ranges.len()).sum::<usize>(), LIMIT);
    assert!(search.capped());
}

/// What's found, as a refactoring's plan: each occurrence at its place in its file (the
/// bytes it was, checked before writing), replaced — a regular expression's groups filled in.
#[test]
fn what_s_found_becomes_a_plan_replacing_it() {
    let text = "<?php\n$cart = new Cart();\n";
    let query = document::SearchQuery {
        pattern: r"new (\w+)".into(),
        regex: true,
        case_sensitive: true,
        whole_word: false,
    };
    let matcher = Matcher::new(&query).unwrap().unwrap();
    let found: Vec<Found> = matcher
        .lines_matching(text, 10)
        .into_iter()
        .map(|line| Found { path: "src/a.php".into(), line })
        .collect();
    let plan = replace_plan(&found, &matcher, "make($1)", true, Path::new("/p"));
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].path, Path::new("/p/src/a.php"));
    let o = &plan.files[0].occurrences[0];
    assert_eq!(&text[o.range.clone()], "new Cart");
    assert_eq!((o.original.as_str(), o.replacement.as_str()), ("new Cart", "make(Cart)"));
    assert!(o.chosen && o.line == 1);
}
