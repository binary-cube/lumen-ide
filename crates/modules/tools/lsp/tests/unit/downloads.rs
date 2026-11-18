use std::time::Duration;

use lsp_client::fake::Behaviour;

use super::*;
use crate::harness::{self, Harness, Project, fresh_downloads, wait};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn a_missing_server_is_offered_with_what_it_needs_and_starts_once_downloaded() {
    let h = Harness::new(Behaviour::default());
    let (root, memory) = fresh_downloads("offered");
    let offer = crate::bar::offer("yaml").expect("an offer");
    assert_eq!(offer.components, names(&["node", "yaml-srv"]));
    assert_eq!(offer.parts, "yaml-srv + Node.js");
    let manifest = manifest();
    assert_eq!(offer.size, manifest.components.iter().map(|c| c.size(platform())).sum::<u64>());
    // Nothing downloaded by offering; the file works without its server.
    assert!(memory.asked().is_empty());
    let project = Project::open("offered");
    project.open_doc("a.yaml", "yaml", "a: 1\n");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.launches(), 0);
    assert!(project.servers().missing("yaml").is_some());
    // Downloaded (the bar's Download): the server starts by itself, from the shared folder.
    ensure(&offer.components, None).unwrap();
    wait("the downloaded server started", || h.launches() == 1);
    assert!(crate::bar::offer("yaml").is_none());
    assert_eq!(
        node(),
        Some(root.join("node/1.0.0").join(if cfg!(windows) { "node.exe" } else { "bin/node" }))
    );
    assert_eq!(server("yaml-srv"), Some((root.join("servers/yaml-srv/2.0.0"), "2.0.0".to_owned())));
    assert!(root.join("servers/yaml-srv/2.0.0/node_modules/yaml-srv/server.js").is_file());
    let running =
        project.servers().servers(|s| s.iter().map(|r| r.version.clone()).collect::<Vec<_>>());
    assert_eq!(running, [Some("2.0.0".to_owned())]);
    // Each one an operation of the installs' part in the Task Manager, done.
    let parts = crate::report::parts(std::time::Instant::now());
    let installs = parts.iter().find(|p| p.project.is_none()).expect("the installs' part");
    for subject in ["Node.js 1.0.0", "yaml-srv 2.0.0"] {
        assert!(
            installs.activities.iter().any(|a| a.subject == subject
                && matches!(a.ended, Some((_, workbench_model::resources::Outcome::Done)))),
            "{subject}"
        );
    }
}

#[test]
fn node_is_counted_only_when_it_is_missing() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("node-first");
    install("node", None).unwrap();
    let offer = crate::bar::offer("yaml").unwrap();
    assert_eq!(offer.components, names(&["yaml-srv"]));
    assert_eq!(offer.parts, "yaml-srv");
    assert_eq!(offer.size, manifest().named("yaml-srv").unwrap().size(platform()));
}

#[test]
fn nothing_is_offered_where_nothing_can_be_downloaded() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("nothing");
    // A server that's a program of the user's, no downloads at all; one the user installs
    // is offered to install, nothing downloaded for it (plan 43j).
    assert!(crate::bar::offer("typescript").is_none());
    let php = crate::bar::offer("php").map(|o| o.choices);
    assert!(php.is_none_or(|c| c.iter().all(|c| c.install && c.components.is_empty())));
    use_downloads(None);
    assert!(crate::bar::offer("yaml").is_none());
}

#[test]
fn the_bundle_beside_the_app_comes_first() {
    let _h = Harness::new(Behaviour::default());
    let (root, _) = fresh_downloads("bundle-first");
    install("node", None).unwrap();
    let spec = crate::catalog::catalog().named("downloadable").unwrap().clone();
    let downloaded = crate::catalog::places(None, &spec, &root, 1024);
    assert_eq!(downloaded.node, node());
    assert_eq!(downloaded.lsp, Some(root.clone()));
    // A bundle with its own Node and the server.
    let bundle_dir = harness::temp().join("a-bundle");
    let _ = std::fs::remove_dir_all(&bundle_dir);
    std::fs::create_dir_all(bundle_dir.join("node/9/bin")).unwrap();
    std::fs::write(bundle_dir.join("node/9/bin/node"), "").unwrap();
    std::fs::create_dir_all(bundle_dir.join("servers/yaml-srv/9")).unwrap();
    std::fs::write(
        bundle_dir.join("manifest.toml"),
        "[node]\nversion = \"9\"\npath = \"node/9/bin/node\"\n\n[servers.yaml-srv]\nversion = \"9\"\npath = \"servers/yaml-srv/9\"\n",
    )
    .unwrap();
    let bundle = lsp_client::bundle::Bundle::open(&bundle_dir).unwrap();
    let places = crate::catalog::places(Some(&bundle), &spec, &root, 1024);
    assert_eq!(places.node, Some(bundle_dir.join("node/9/bin/node")));
    assert_eq!(places.server, Some(bundle_dir.join("servers/yaml-srv/9")));
    assert_eq!(places.lsp, Some(bundle_dir));
    assert_eq!(crate::catalog::bundled_version(Some(&bundle), &spec).as_deref(), Some("9"));
    assert_eq!(crate::catalog::bundled_version(None, &spec), None);
}

#[test]
fn no_network_is_said_and_the_download_can_be_tried_again() {
    let h = Harness::new(Behaviour::default());
    let (_, memory) = fresh_downloads("offline");
    memory.set_offline(true);
    let error = ensure(&names(&["node", "yaml-srv"]), None).unwrap_err();
    assert!(error.offline(), "{error}");
    assert!(failure("node").is_some_and(|f| f.offline));
    // What came after the failure wasn't waited for.
    assert_eq!(
        status("yaml-srv"),
        Status::Missing { size: manifest().named("yaml-srv").unwrap().size(platform()) }
    );
    memory.set_offline(false);
    let project = Project::open("offline");
    project.open_doc("a.yaml", "yaml", "a: 1\n");
    ensure(&names(&["node", "yaml-srv"]), None).unwrap();
    assert_eq!(failure("node"), None);
    wait("the server started", || h.launches() == 1);
}

#[test]
fn removing_a_component_stops_the_servers_using_it() {
    let h = Harness::new(Behaviour::default());
    let (root, _) = fresh_downloads("removed");
    ensure(&names(&["node", "yaml-srv"]), None).unwrap();
    let project = Project::open("removed");
    project.open_doc("a.yaml", "yaml", "a: 1\n");
    wait("the server started", || h.launches() == 1);
    remove("node").unwrap();
    assert!(!root.join("node/1.0.0").exists());
    wait("the server stopped", || project.servers().servers(<[_]>::is_empty));
    assert_eq!(crate::bar::offer("yaml").unwrap().components, names(&["node"]));
    // Back: it starts again for the document still open.
    ensure(&names(&["node"]), None).unwrap();
    wait("the server started again", || h.launches() == 2);
}

#[test]
fn an_older_version_runs_until_it_is_brought_up_to_date() {
    let h = Harness::new(Behaviour::default());
    let (root, _) = fresh_downloads("older");
    install("node", None).unwrap();
    // yaml-srv 1.0.0, left by an earlier version of the app.
    let old = root.join("servers/yaml-srv/1.0.0");
    std::fs::create_dir_all(old.join("node_modules/yaml-srv")).unwrap();
    std::fs::write(old.join("node_modules/yaml-srv/server.js"), "").unwrap();
    let marker = lsp_client::store::Marker {
        name: "yaml-srv".into(),
        version: "1.0.0".into(),
        fingerprint: "older".into(),
        size: 1,
        platform: platform().into(),
    };
    lsp_client::store::write_marker(&old, &marker).unwrap();
    use_manifest((*manifest()).clone());
    // Used: nothing offered, it starts from there.
    assert!(crate::bar::offer("yaml").is_none());
    assert_eq!(server("yaml-srv").map(|(_, v)| v).as_deref(), Some("1.0.0"));
    let project = Project::open("older");
    project.open_doc("a.yaml", "yaml", "a: 1\n");
    wait("the older server started", || h.launches() == 1);
    // The cleanup keeps it (it's used) and brings it up to date in the background.
    maintain_now(None);
    wait("brought up to date", || found("yaml-srv").is_some_and(|f| f.current));
    assert_eq!(server("yaml-srv").map(|(_, v)| v).as_deref(), Some("2.0.0"));
    // The next cleanup removes the old one: nothing uses it now.
    maintain_now(None);
    assert!(!old.exists());
}

#[test]
fn the_cleanup_removes_only_what_nothing_uses_and_says_what_this_version_uses() {
    let _h = Harness::new(Behaviour::default());
    let (root, _) = fresh_downloads("cleanup");
    install("node", None).unwrap();
    let stray = root.join("node/0.1.0");
    std::fs::create_dir_all(&stray).unwrap();
    lsp_client::store::write_marker(
        &stray,
        &lsp_client::store::Marker {
            name: "node".into(),
            version: "0.1.0".into(),
            fingerprint: String::new(),
            size: 0,
            platform: String::new(),
        },
    )
    .unwrap();
    maintain_now(None);
    assert!(!stray.exists());
    assert!(root.join("node/1.0.0/.installed").is_file());
    let refs: Vec<_> = std::fs::read_dir(root.join("refs")).unwrap().flatten().collect();
    assert_eq!(refs.len(), 1);
    let text = std::fs::read_to_string(refs[0].path()).unwrap();
    assert!(text.contains("node/1.0.0"), "{text}");
}

#[test]
fn an_install_of_43h_moves_to_the_shared_folder() {
    let _h = Harness::new(Behaviour::default());
    let (root, _) = fresh_downloads("migrated");
    let old = harness::temp().join("old-config-lsp");
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(old.join("installed/intelephense/1.18.5/lib")).unwrap();
    std::fs::write(old.join("installed/intelephense/1.18.5/package.json"), "{}").unwrap();
    std::fs::create_dir_all(old.join("data/intelephense")).unwrap();
    std::fs::write(old.join("data/intelephense/licence"), "activated").unwrap();
    migrate_from(&old, &root);
    assert!(root.join("servers/intelephense/1.18.5/package.json").is_file());
    assert!(root.join("data/intelephense/licence").is_file());
    assert!(!old.exists());
}

#[test]
#[allow(clippy::disallowed_methods)] // two installers at once, on threads of their own
fn a_second_install_of_the_same_component_waits_for_the_first() {
    let _h = Harness::new(Behaviour::default());
    let (_, memory) = fresh_downloads("twice");
    let size = manifest().named("node").unwrap().size(platform());
    memory.stall_after(Some(size / 2));
    let first = std::thread::spawn(|| install("node", None));
    wait(
        "the first one under way",
        || matches!(status("node"), Status::Downloading(p) if p.received > 0),
    );
    let second = std::thread::spawn(|| install("node", None));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!second.is_finished(), "it waits");
    memory.stall_after(None);
    memory.release();
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert!(first.is_err(), "the first was broken off");
    // The second saw it wasn't installed (said as the first's failure).
    assert!(second.is_err());
    install("node", None).unwrap();
    assert!(
        memory.asked_from().iter().any(|(_, from)| *from > 0),
        "resumed: {:?}",
        memory.asked_from()
    );
}
