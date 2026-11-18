use super::*;

const FILES: &[&str] = &[
    "/p/src/lib/db.php",
    "/p/src/main.php",
    "/p/web/logger.ts",
    "/p/web/ui/index.tsx",
    "/p/public/css/app.css",
    "/p/img/logo.png",
    "/p/config/schema.json",
];

fn exists(path: &Path) -> bool {
    FILES.iter().any(|f| Path::new(f) == path)
}

fn links(file: &str, line: &str) -> Vec<(String, LinkTarget)> {
    let base = LinkBase { file: Path::new(file), root: Some(Path::new("/p")), exists: &exists };
    line_links(line, 0, &base).into_iter().map(|l| (line[l.range].to_owned(), l.target)).collect()
}

fn file(path: &str) -> LinkTarget {
    LinkTarget::File { path: PathBuf::from(path), line: None }
}

#[test]
fn paths_in_quotes_lead_to_the_project_files_there() {
    assert_eq!(
        links("/p/src/main.php", "require 'lib/db.php';"),
        [("lib/db.php".into(), file("/p/src/lib/db.php"))]
    );
    assert_eq!(
        links("/p/src/main.php", "require __DIR__ . '/lib/db.php';"),
        [("/lib/db.php".into(), file("/p/src/lib/db.php"))]
    );
    assert_eq!(
        links("/p/web/app.ts", "import { log } from './logger';"),
        [("./logger".into(), file("/p/web/logger.ts"))]
    );
    assert_eq!(
        links("/p/web/app.ts", "import Ui from \"./ui\";"),
        [("./ui".into(), file("/p/web/ui/index.tsx"))]
    );
    // From the root, or its public folder (a web page's `/css/…`).
    assert_eq!(
        links("/p/views/a.html", "<link href=\"/css/app.css\">"),
        [("/css/app.css".into(), file("/p/public/css/app.css"))]
    );
    assert_eq!(
        links("/p/css/x.css", "background: url(../img/logo.png);"),
        [("../img/logo.png".into(), file("/p/img/logo.png"))]
    );
}

#[test]
fn what_isnt_a_file_there_isnt_a_link() {
    assert!(links("/p/src/main.php", "echo 'hello world';").is_empty());
    assert!(links("/p/src/main.php", "require 'missing.php';").is_empty());
    assert!(links("/p/src/main.php", "$x = 'App\\Models\\User';").is_empty());
    assert!(
        links("/p/src/main.php", "require '../../etc/passwd';").is_empty(),
        "outside the project"
    );
}

#[test]
fn urls_open_in_the_browser() {
    let found = links("/p/a.md", "See https://example.com/docs?a=1, or (http://x.org).");
    assert_eq!(
        found,
        [
            (
                "https://example.com/docs?a=1".into(),
                LinkTarget::Url("https://example.com/docs?a=1".into())
            ),
            ("http://x.org".into(), LinkTarget::Url("http://x.org".into())),
        ]
    );
    // In quotes: the URL, not a path.
    assert_eq!(links("/p/a.php", "$u = 'https://a.io/x.php';").len(), 1);
    assert!(links("/p/a.php", "phttps://no").is_empty());
}

#[test]
fn the_link_at_an_offset() {
    let text = "<?php\nrequire 'lib/db.php';\n";
    let base = LinkBase {
        file: Path::new("/p/src/main.php"),
        root: Some(Path::new("/p")),
        exists: &exists,
    };
    let at = text.find("db").unwrap();
    let link = link_at(text, at, &base).unwrap();
    assert_eq!(&text[link.range], "lib/db.php");
    assert!(link_at(text, 2, &base).is_none());
}

/// The YAML language server's line names its schema by a path (no quotes) or an address:
/// both lead there; a path to nothing isn't a link.
#[test]
fn a_yaml_schema_s_path_leads_to_it() {
    assert_eq!(
        links("/p/config/app.yaml", "# yaml-language-server: $schema=./schema.json"),
        [("./schema.json".into(), file("/p/config/schema.json"))]
    );
    assert_eq!(
        links("/p/config/app.yaml", "# yaml-language-server: $schema=https://e.com/s.json"),
        [("https://e.com/s.json".into(), LinkTarget::Url("https://e.com/s.json".into()))]
    );
    assert!(links("/p/config/app.yaml", "# $schema=./missing.json").is_empty());
}
