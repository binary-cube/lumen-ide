use super::*;
use code_intelligence::{SymbolHit, SymbolQuery};

/// A project of one file, as the queries port tells it.
struct OneFile {
    root: PathBuf,
}

impl ProjectQueries for OneFile {
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
        vec![self.root.join("a.php")]
    }
    fn class_diagram(&self, _: &str) -> Option<code_intelligence::ClassDiagram> {
        None
    }
    fn root(&self) -> PathBuf {
        self.root.clone()
    }
    fn file_list(&self) -> Arc<Vec<String>> {
        Arc::new(vec!["a.php".to_owned()])
    }
    fn symbol_index(&self) -> code_intelligence::SymbolIndex {
        code_intelligence::SymbolIndex::default()
    }
    fn language_of(&self, _: &Path) -> String {
        "php".to_owned()
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
}

/// The module builds its graph itself, from the port, in the background: asked for a file,
/// the graph around it comes back (its line kept), and a newer request makes an older one
/// give up.
#[test]
fn builds_the_graph_from_the_project_s_queries() {
    let root = std::env::temp_dir().join(format!("canvas-graph-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.php"), "<?php\nfunction a() {}\n").unwrap();
    let mut builder = Builder::default();
    builder.request(root.join("a.php"), Some(1));
    assert!(!builder.is_building(), "no project yet: nothing asked");
    builder.set_queries(Arc::new(OneFile { root: root.clone() }));
    builder.request(root.join("missing.php"), None);
    builder.request(root.join("a.php"), Some(1));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let graph = loop {
        if let Some(graph) = builder.poll() {
            break graph;
        }
        assert!(std::time::Instant::now() < deadline, "no graph");
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(graph.files[graph.focus].absolute, root.join("a.php"));
    assert_eq!(graph.cursor_line, Some(1));
    assert!(!builder.is_building());
}
