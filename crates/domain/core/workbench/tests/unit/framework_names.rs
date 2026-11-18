use super::*;
use code_intelligence::{ClassDiagram, Location, SymbolHit, SymbolIndex, SymbolQuery};

/// A project held in memory: its files' text, by path relative to `/p`.
struct Memory(Vec<(&'static str, &'static str)>);

impl ProjectQueries for Memory {
    fn paths(&self, _: &workspace::PathQuery<'_>) -> workspace::PathResults {
        workspace::PathResults::default()
    }
    fn symbols(&self, _: &SymbolQuery<'_>) -> Vec<SymbolHit> {
        Vec::new()
    }
    fn text(&self, _: &text_search::SearchQuery) -> Vec<text_search::Hit> {
        Vec::new()
    }
    fn files(&self, _: &workspace::FileFilter) -> Vec<PathBuf> {
        Vec::new()
    }
    fn class_diagram(&self, _: &str) -> Option<ClassDiagram> {
        None
    }
    fn root(&self) -> PathBuf {
        PathBuf::from("/p")
    }
    fn file_list(&self) -> Arc<Vec<String>> {
        Arc::new(self.0.iter().map(|(p, _)| (*p).to_owned()).collect())
    }
    fn symbol_index(&self) -> SymbolIndex {
        SymbolIndex::default()
    }
    fn language_of(&self, _: &Path) -> String {
        String::new()
    }
    fn generation(&self) -> u64 {
        1
    }
    fn list_dir(&self, _: &Path) -> std::io::Result<Vec<workspace::DirEntry>> {
        Ok(Vec::new())
    }
    fn file_icon(&self, _: &str) -> (&'static str, String) {
        ("file", String::new())
    }
    fn read_source(&self, relative: &Path) -> Option<String> {
        let relative = relative.to_string_lossy();
        self.0.iter().find(|(p, _)| *p == relative).map(|(_, t)| (*t).to_owned())
    }
}

/// Routes: written in `.php` and `.yaml` files, as `route('x')` (a declaration: `x:`).
#[derive(Debug)]
struct Routes;

impl FrameworkSupport for Routes {
    fn names_in(&self, kind: &str, relative: &str) -> bool {
        kind == "route" && (relative.ends_with(".php") || relative.ends_with(".yaml"))
    }

    fn name_places(&self, _: &str, name: &str, _: &str, text: &str) -> Vec<Range<usize>> {
        [format!("route('{name}')"), format!("{name}:")]
            .iter()
            .flat_map(|w| text.match_indices(w.as_str()).map(|(at, m)| (at, m.to_owned())))
            .map(|(at, m)| {
                let start = at + m.find(name).unwrap_or(0);
                start..start + name.len()
            })
            .collect()
    }

    fn name_taken(&self, _: &str, name: &str) -> Vec<Location> {
        (name == "home")
            .then(|| Location { path: "/p/routes.yaml".into(), offset: 0, line: 0 })
            .into_iter()
            .collect()
    }
}

#[test]
fn a_frameworks_name_is_renamed_where_the_framework_places_it() {
    let queries = Arc::new(Memory(vec![
        ("routes.yaml", "home:\n    path: /\nblog:\n    path: /blog\n"),
        ("src/A.php", "route('blog'); // the blog\n$blog = 1;\n"),
        ("README.md", "route('blog')"),
        (".git/B.php", "route('blog')"),
    ]));
    let open = Arc::new(HashMap::from([(
        PathBuf::from("/p/src/A.php"),
        "route('blog'); route('blog'); // the blog\n".to_owned(),
    )]));
    let name = FrameworkName { kind: "route", name: "blog".into(), range: 7..11 };
    let context = rename_context(Arc::new(Routes), name, queries, open);
    let mut candidates = context.sources.candidates();
    candidates.sort();
    assert_eq!(
        candidates,
        [PathBuf::from("/p/routes.yaml"), PathBuf::from("/p/src/A.php")],
        "its files, not the repository's nor the others'"
    );
    let plan = refactor::application::plan_rename(
        &context.target,
        &*context.sources,
        &*context.resolver,
        &|| false,
    );
    let chosen: Vec<(String, usize)> = plan
        .files
        .iter()
        .map(|f| {
            let name = f.path.file_name().unwrap().to_string_lossy().into_owned();
            (name, f.occurrences.iter().filter(|o| o.chosen).count())
        })
        .collect();
    assert!(chosen.contains(&("routes.yaml".to_owned(), 1)), "{chosen:?}");
    assert!(
        chosen.contains(&("A.php".to_owned(), 2)),
        "the open text; not the comment: {chosen:?}"
    );
    let clash = context.conflicts.conflicts("home");
    assert_eq!(clash.len(), 1);
    assert_eq!(clash[0].line_text, "home:");
    assert!(context.conflicts.conflicts("news").is_empty());
}
