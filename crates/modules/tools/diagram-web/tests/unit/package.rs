use std::sync::atomic::AtomicBool;

use pinned_downloads::fetch::Memory;
use pinned_downloads::samples::{ZipEntry, zip};

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("diagram-web-{name}-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn only_what_embed_mode_uses_is_kept() {
    for path in [
        "index.html",
        "js/app.min.js",
        "js/bootstrap.js",
        "js/main.js",
        "js/PreConfig.js",
        "js/PostConfig.js",
        "js/extensions.min.js",
        "js/stencils.min.js",
        "js/orgchart.min.js",
        "js/mermaid/drawio-mermaid.min.js",
        "styles/grapheditor.css",
        "mxgraph/css/common.css",
        "math4/es5/startup.js",
        "resources/dia.txt",
        "resources/dia_ro.txt",
        "img/lib/active_directory/user.svg",
        "favicon.ico",
    ] {
        assert!(kept(path), "{path}");
    }
    for path in [
        "WEB-INF/web.xml",
        "WEB-INF/lib/a.jar",
        "META-INF/MANIFEST.MF",
        "templates/index.xml",
        "plugins/anonymize.js",
        "js/diagramly/App.js",
        "js/grapheditor/Graph.js",
        "js/orgchart/bridge.min.js",
        "mxgraph/src/mxClient.js",
        "js/integrate.min.js",
        "js/viewer.min.js",
        "js/viewer-static.min.js",
        "service-worker.js",
        "workbox-05b6c01b.js",
        "workbox-05b6c01b.js.map",
        "tail.bin",
        "resources/dia_de.txt",
        "resources/dia_zh-tw.txt",
    ] {
        assert!(!kept(path), "{path}");
    }
    // A folder's name as a file's prefix isn't the folder.
    assert!(kept("templates.txt"));
}

/// A small war: what's kept and what isn't.
fn war() -> Vec<u8> {
    zip(&[
        ZipEntry("index.html", Some(0o644), b"<html>draw.io</html>", true),
        ZipEntry("js/app.min.js", Some(0o644), b"app()", true),
        ZipEntry("WEB-INF/web.xml", Some(0o644), b"<web/>", false),
        ZipEntry("js/integrate.min.js", Some(0o644), b"big", true),
        ZipEntry("resources/dia_de.txt", Some(0o644), b"de", true),
        ZipEntry("resources/dia_ro.txt", Some(0o644), b"ro", true),
    ])
}

#[test]
fn an_archive_is_installed_whole_and_what_was_left_goes() {
    let root = temp("install");
    // What an older install and a crash left.
    std::fs::create_dir_all(root.join("1.0.0/js")).unwrap();
    std::fs::create_dir_all(root.join(".tmp-left")).unwrap();
    let archive = root.join("draw.war");
    std::fs::write(&archive, war()).unwrap();
    assert_eq!(installed(&root), None);
    let dir = install_archive(&archive, &root, &AtomicBool::new(false)).unwrap();
    assert_eq!(dir, version_dir(&root));
    assert_eq!(installed(&root), Some(dir.clone()));
    assert!(dir.join("index.html").is_file());
    assert!(dir.join("js/app.min.js").is_file());
    assert!(dir.join("resources/dia_ro.txt").is_file());
    assert!(!dir.join("WEB-INF").exists());
    assert!(!dir.join("js/integrate.min.js").exists());
    assert!(!dir.join("resources/dia_de.txt").exists());
    let notice = std::fs::read_to_string(dir.join(NOTICE_FILE)).unwrap();
    assert!(notice.contains("Apache License, Version 2.0"));
    assert!(notice.contains(VERSION));
    let marker = std::fs::read_to_string(dir.join(MARKER)).unwrap();
    assert!(marker.starts_with(&format!("{VERSION}\n")));
    assert!(!root.join("1.0.0").exists());
    assert!(!root.join(".tmp-left").exists());
    // Installed again (a broken one there, no marker): replaced whole.
    std::fs::remove_file(dir.join(MARKER)).unwrap();
    assert_eq!(installed(&root), None);
    install_archive(&archive, &root, &AtomicBool::new(false)).unwrap();
    assert!(installed(&root).is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_archive_without_the_editor_is_refused_and_leaves_nothing() {
    let root = temp("refused");
    let archive = root.join("draw.war");
    std::fs::write(&archive, zip(&[ZipEntry("other.txt", None, b"x", false)])).unwrap();
    assert!(install_archive(&archive, &root, &AtomicBool::new(false)).is_err());
    std::fs::write(&archive, b"not a zip").unwrap();
    assert!(install_archive(&archive, &root, &AtomicBool::new(false)).is_err());
    let left: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["draw.war".to_owned()]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_download_that_is_not_the_pinned_one_is_never_installed() {
    let root = temp("pinned");
    let memory = Memory::default();
    memory.serve(URL, war());
    let error = install(&memory, &root, &|_, _| {}, &|| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(matches!(error, InstallError::Fetch(FetchError::Mismatch(_))), "{error}");
    assert_eq!(installed(&root), None);
    assert!(!part_path(&root).exists());
    // Offline: said as such; a stop: said as such.
    memory.set_offline(true);
    let error = install(&memory, &root, &|_, _| {}, &|| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(error.offline());
    assert!(InstallError::Fetch(FetchError::Stopped).stopped());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn it_comes_from_github_s_release_only() {
    assert!(pinned_downloads::fetch::allowed(URL, &HOSTS));
    assert!(URL.contains(VERSION));
    assert_eq!(SHA256.len(), 64);
    assert!(!pinned_downloads::fetch::allowed("https://app.diagrams.net/draw.war", &HOSTS));
}

/// The real release, installed by our code from a war downloaded beforehand
/// (`DIAGRAM_WEB_WAR=<path>`): what's kept, measured.
#[test]
#[ignore = "needs the real draw.war (DIAGRAM_WEB_WAR)"]
fn the_real_release_installs() {
    let war = PathBuf::from(std::env::var("DIAGRAM_WEB_WAR").unwrap());
    let bytes = std::fs::read(&war).unwrap();
    assert_eq!(pinned_downloads::digest::sha256_hex(&bytes), SHA256);
    assert_eq!(bytes.len() as u64, DOWNLOAD_SIZE);
    let root = temp("real");
    let dir = install_archive(&war, &root, &AtomicBool::new(false)).unwrap();
    let size = folder_size(&dir);
    let files = {
        let mut n = 0;
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                } else {
                    n += 1;
                }
            }
        }
        n
    };
    println!("installed: {size} bytes in {files} files at {}", dir.display());
    assert!(size.abs_diff(INSTALLED_SIZE) < 1 << 20, "{size}");
    if std::env::var("DIAGRAM_WEB_KEEP").is_err() {
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// The real download, through our own code (the network: GitHub and where it redirects),
/// installed under `DIAGRAM_WEB_ROOT`.
#[test]
#[ignore = "downloads ~50 MB from GitHub (DIAGRAM_WEB_ROOT)"]
fn the_real_download_installs() {
    let root = PathBuf::from(std::env::var("DIAGRAM_WEB_ROOT").unwrap());
    let http = pinned_downloads::fetch::Http::new(&HOSTS);
    let started = std::time::Instant::now();
    let dir = install(&http, &root, &|_, _| {}, &|| {}, &AtomicBool::new(false)).unwrap();
    println!(
        "installed {} bytes at {} in {:?}",
        folder_size(&dir),
        dir.display(),
        started.elapsed()
    );
    assert_eq!(installed(&root), Some(dir));
}
