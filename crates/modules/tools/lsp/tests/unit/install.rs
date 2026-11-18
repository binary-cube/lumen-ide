use std::time::Duration;

use code_intelligence::servers::Engine;
use lsp_client::fake::Behaviour;

use super::*;
use crate::harness::{self, Harness, Project, wait};
use crate::settings::{self, Choice};

fn installable() -> ServerSpec {
    crate::catalog::catalog().named("installable").unwrap().clone()
}

/// Until what was found is known (not looking any more).
fn known(spec: &ServerSpec) -> State {
    wait("the install looked for", || {
        !matches!(state(spec).found, Found::Unknown | Found::Looking)
    });
    state(spec)
}

#[test]
fn a_server_to_install_is_offered_and_started_only_once_installed() {
    let h = Harness::new(Behaviour::default());
    let dir = harness::fresh_installs("offered");
    let spec = installable();
    assert_eq!(known(&spec).found, Found::Missing);
    // Nothing downloaded by looking.
    assert!(harness::registry().asked.lock().unwrap().is_empty());
    // Not offered; PHP's first server is the built-in engine's by default here (as phpactor
    // on Windows): the built-in engine answers.
    assert_eq!(settings::offered("php"), ["php-first"]);
    assert_eq!(settings::default_choice("php"), Choice { engine: Engine::BuiltIn, server: None });
    let project = Project::open("offered");
    project.open_doc("a.php", "php", "<?php\n");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.launches(), 0);
    // Installed (on the user's click: here, asked directly), from the pinned registry URL.
    let found = install(&spec, None).unwrap();
    assert_eq!(
        harness::registry().asked.lock().unwrap().as_slice(),
        ["https://registry.npmjs.org/installable-server/-/installable-server-1.0.0.tgz"]
    );
    assert!(found.ours);
    assert!(found.dir.starts_with(&dir), "{found:?}");
    assert!(found.dir.join("lib/server.js").is_file());
    assert_eq!(installed(&spec), Some(found));
    // Offered now, and the default: the document open starts it, from its folder.
    assert_eq!(settings::offered("php"), ["php-first", "installable"]);
    assert_eq!(
        settings::default_choice("php"),
        Choice { engine: Engine::Server, server: Some("installable".into()) }
    );
    wait("the installed server started", || h.launches() == 1);
    let running = project.servers().servers(|s| {
        s.iter().map(|r| (r.spec.name.clone(), r.version.clone())).collect::<Vec<_>>()
    });
    assert_eq!(running, [("installable".to_owned(), Some("1.0.0".to_owned()))]);
    // Its download in the Task Manager: an operation of the installs' part, done.
    let parts = crate::report::parts(std::time::Instant::now());
    let installs = parts.iter().find(|p| p.project.is_none()).expect("the installs' part");
    assert!(installs.activities.iter().any(|a| a.subject == "installable-server 1.0.0"
        && matches!(a.ended, Some((_, workbench_model::resources::Outcome::Done)))));
    // Uninstalled: it stops, isn't offered, its folder is gone.
    uninstall(&spec).unwrap();
    assert_eq!(settings::offered("php"), ["php-first"]);
    wait("the server stopped", || project.servers().servers(<[_]>::is_empty));
    assert!(!dir.join("installable").exists());
    assert_eq!(known(&spec).found, Found::Missing);
}

#[test]
fn a_download_that_doesnt_match_installs_nothing() {
    let _h = Harness::new(Behaviour::default());
    let dir = harness::fresh_installs("tampered");
    *harness::registry().body.lock().unwrap() = b"not the package".to_vec();
    let spec = installable();
    known(&spec);
    let error = install(&spec, None).unwrap_err();
    assert!(error.contains("integrity"), "{error}");
    let state = state(&spec);
    assert_eq!(state.found, Found::Missing);
    assert!(state.error.is_some_and(|e| e.contains("integrity")));
    assert_eq!(state.installing, None);
    assert!(!dir.join("installable").join("1.0.0").exists());
    assert_eq!(settings::offered("php"), ["php-first"]);
}

#[test]
fn the_licence_key_reaches_the_server_from_the_secrets_only() {
    let h = Harness::new(Behaviour {
        message: Some((3, "licence {options}".into())),
        ..Behaviour::default()
    });
    harness::fresh_installs("licence");
    let spec = installable();
    known(&spec);
    install(&spec, None).unwrap();
    crate::licence::save("installable-licence", "  KEY-LICENCE-42  ").unwrap();
    assert_eq!(
        harness::secrets().0.lock().unwrap().get("installable-licence").map(String::as_str),
        Some("KEY-LICENCE-42")
    );
    crate::settings::tracing_flag().store(true, std::sync::atomic::Ordering::Relaxed);
    let project = Project::open("licence");
    project.open_doc("a.php", "php", "<?php\n");
    wait("started", || h.launches() == 1);
    wait("initialized", || h.seen(|s| !s.initialize.is_null()));
    let init = h.seen(|s| s.initialize.clone());
    assert_eq!(init["initializationOptions"]["licenceKey"], "KEY-LICENCE-42");
    // Never in its log (the traffic logged: its initialize, its message echoing it).
    let client = project.servers().servers(|s| Arc::clone(&s[0].client));
    // Waited for by its redaction: the project's folder is named `licence` too, so the
    // didOpen's line would do before the message came.
    wait("its message logged", || {
        client.status().log_lines().0.iter().any(|l| l.contains(lsp_client::status::REDACTED))
    });
    let log = client.status().log_lines().0.join("\n");
    assert!(!log.contains("KEY-LICENCE-42"), "{log}");
    assert!(log.contains(lsp_client::status::REDACTED), "{log}");
    crate::settings::tracing_flag().store(false, std::sync::atomic::Ordering::Relaxed);
    // Removed: the server starts again without it.
    crate::licence::save("installable-licence", "").unwrap();
    assert!(harness::secrets().0.lock().unwrap().is_empty());
    wait("started again", || h.launches() == 2);
    wait("initialized again", || {
        h.seen(|s| s.initialize["initializationOptions"].get("licenceKey").is_none())
    });
    uninstall(&spec).unwrap();
}

#[test]
fn an_older_install_of_the_apps_is_used_until_the_pinned_one() {
    let _h = Harness::new(Behaviour::default());
    let dir = harness::fresh_installs("elsewhere");
    // An older one the app made: found, its version said.
    let older = lsp_client::install::version_dir(&dir, "installable", "0.9.0");
    std::fs::create_dir_all(older.join("lib")).unwrap();
    std::fs::write(
        older.join("package.json"),
        r#"{"name": "installable-server", "version": "0.9.0"}"#,
    )
    .unwrap();
    std::fs::write(older.join("lib/server.js"), "").unwrap();
    let spec = installable();
    let Found::Installed(found) = known(&spec).found else { panic!("not found") };
    assert_eq!((found.version.as_str(), found.ours), ("0.9.0", true));
    assert_eq!(settings::offered("php"), ["php-first", "installable"]);
    uninstall(&spec).unwrap();
    assert!(!older.exists());
}
