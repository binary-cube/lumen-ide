use std::time::Duration;

use code_intelligence::servers::{Engine, Kept, LanguageServers, ServerAt};
use lsp_client::fake::Behaviour;
use lsp_client::status::State;
use workbench_model::DiagnosticsScope;

use crate::harness::{Harness, Project, wait};
use crate::settings::{self, Choice, Config};

fn ready(project: &Project) {
    wait("a server ready", || {
        project.servers().servers(|s| {
            s.iter().any(|r| matches!(r.client.status().state(), State::Ready | State::Idle))
        })
    });
}

fn count(project: &Project) -> usize {
    project.servers().servers(<[_]>::len)
}

#[test]
fn nothing_starts_without_a_document_of_a_served_language() {
    let h = Harness::new(Behaviour::default());
    let project = Project::open("nothing");
    project.open_doc("style.css", "css", "a {}");
    project.open_doc("index.php", "php", "<?php");
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(count(&project), 0);
    // This project's only: a server the test before left starting may come late, on these
    // fakes (one test at a time, the fakes the process's).
    assert_eq!(h.made_for(&project.root), 0);
}

#[test]
fn one_server_per_project_for_all_its_languages_documents() {
    let h = Harness::new(Behaviour::default());
    let project = Project::open("one");
    let a = project.open_doc("a.ts", "typescript", "let a = 1;");
    project.open_doc("b.ts", "typescript", "let b = 2;");
    project.open_doc("c.tsx", "tsx", "const c = <div/>;");
    ready(&project);
    assert_eq!(count(&project), 1);
    assert_eq!(h.launches(), 1);
    assert_eq!(h.made_for(&project.root), 1, "counted for its project");
    wait("three documents", || h.seen(|s| s.documents.len() == 3));
    // TSX as LSP names it.
    assert!(h.seen(|s| s.methods.iter().filter(|m| *m == "textDocument/didOpen").count() == 3));
    project.change(a, "a.ts", 2, "let a = 10;");
    let uri = lsp_client::uri::from_path(&project.root.join("a.ts"));
    wait("changed", || {
        h.seen(|s| s.documents.get(&uri).map(String::as_str) == Some("let a = 10;"))
    });
    let languages = project.servers().servers(|s| s[0].languages.clone());
    assert_eq!(languages.into_iter().collect::<Vec<_>>(), ["tsx", "typescript"]);
}

#[test]
fn its_diagnostics_are_announced_on_the_projects_bus() {
    let _h = Harness::new(Behaviour::default());
    let project = Project::open("diagnostics");
    project.open_doc("src/a.ts", "typescript", "// TODO later");
    wait("diagnostics", || {
        project.diagnostics.lock().unwrap().iter().any(|d| !d.diagnostics.is_empty())
    });
    let found = project
        .diagnostics
        .lock()
        .unwrap()
        .iter()
        .find(|d| !d.diagnostics.is_empty())
        .cloned()
        .unwrap();
    assert_eq!(found.source, "lsp-fake");
    assert_eq!(found.scope, DiagnosticsScope::File("src/a.ts".into()));
    assert_eq!(found.diagnostics[0].message, "A task is left here.");
}

#[test]
fn choosing_the_built_in_engine_stops_the_server_and_choosing_it_back_starts_it() {
    let h = Harness::new(Behaviour::default());
    let project = Project::open("engines");
    project.open_doc("a.ts", "typescript", "let a;");
    ready(&project);
    let servers = crate::port::Servers;
    let at = |doc| ServerAt {
        project: project.id,
        document: doc,
        path: &project.root,
        language: "typescript",
        version: 1,
        offset: 0,
    };
    assert_eq!(servers.served(project.id, "typescript"), Engine::Server);
    let built_in = Choice { engine: Engine::BuiltIn, server: None };
    settings::set(Config {
        idle: None,
        engines: vec![("typescript".into(), built_in)],
        ..Config::default()
    });
    assert_eq!(count(&project), 0);
    assert_eq!(servers.served(project.id, "typescript"), Engine::BuiltIn);
    wait("closed and stopped", || {
        h.seen(|s| s.count("textDocument/didClose") == 1 && s.count("shutdown") == 1)
    });
    // Not started for a new document either.
    let b = project.open_doc("b.ts", "typescript", "let b;");
    assert!(servers.complete(&at(b)).is_none());
    assert_eq!(h.launches(), 1);
    // Both: started again, the documents open in it, its answers come.
    settings::set(Config {
        idle: None,
        engines: vec![("typescript".into(), Choice { engine: Engine::Both, server: None })],
        ..Config::default()
    });
    ready(&project);
    assert_eq!(h.launches(), 2);
    wait("reopened", || h.seen(|s| s.documents.len() == 2));
    assert_eq!(servers.served(project.id, "typescript"), Engine::Both);
    let reply = servers.complete(&at(b)).unwrap();
    assert!(reply.wait(Duration::from_secs(5)).is_some_and(|c| !c.items.is_empty()));
}

#[test]
fn another_server_can_be_chosen_for_a_language() {
    let _h = Harness::new(Behaviour::default());
    let project = Project::open("other");
    project.open_doc("a.ts", "typescript", "let a;");
    ready(&project);
    assert_eq!(project.servers().servers(|s| s[0].spec.name.clone()), "fake");
    let other = Choice { engine: Engine::Server, server: Some("other".into()) };
    settings::set(Config {
        idle: None,
        engines: vec![("typescript".into(), other)],
        ..Config::default()
    });
    ready(&project);
    assert_eq!(
        project.servers().servers(|s| s.iter().map(|r| r.spec.name.clone()).collect::<Vec<_>>()),
        ["other"]
    );
}

#[test]
fn a_server_turned_off_isnt_started() {
    let h = Harness::new(Behaviour::default());
    settings::set(Config {
        idle: None,
        disabled: vec!["fake".into(), "other".into()],
        ..Config::default()
    });
    let project = Project::open("off");
    project.open_doc("a.ts", "typescript", "let a;");
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(h.launches(), 0);
    assert_eq!(project.servers().missing("typescript").as_deref(), Some("turned off"));
}

#[test]
fn closing_the_project_stops_its_servers() {
    let h = Harness::new(Behaviour::default());
    let mut project = Project::open("closing");
    project.open_doc("a.ts", "typescript", "let a;");
    ready(&project);
    let id = project.id;
    project.subscriptions.clear();
    assert!(crate::projects::project(id).is_none());
    wait("shut down", || h.seen(|s| s.count("exit") == 1));
}

#[test]
fn files_changed_on_disk_reach_the_servers_that_watch_them() {
    let h = Harness::new(Behaviour::default());
    let project = Project::open("disk");
    project.open_doc("a.ts", "typescript", "let a;");
    ready(&project);
    let written = vec![
        project.root.join("b.ts"),
        project.root.join("README.md"),
        project.root.join("node_modules/x/index.ts"),
    ];
    project.events.publish(&workspace::events::FilesWritten { paths: written });
    project.events.publish(&workspace::events::EntriesDeleted {
        paths: vec![project.root.join("tsconfig.json")],
    });
    wait("heard", || h.seen(|s| s.watched.len() == 2));
    let watched = h.seen(|s| s.watched.clone());
    assert!(
        watched[0].ends_with("/disk/b.ts") && watched[1].ends_with("/disk/tsconfig.json"),
        "{watched:?}"
    );
}

#[test]
fn the_port_answers_through_the_documents_server() {
    let _h = Harness::new(Behaviour::default());
    let project = Project::open("port");
    let doc = project.open_doc("a.ts", "typescript", "console.");
    let servers = crate::port::Servers;
    let at = ServerAt {
        project: project.id,
        document: doc,
        path: &project.root,
        language: "typescript",
        version: 1,
        offset: 8,
    };
    ready(&project);
    assert_eq!(servers.completion_triggers(project.id, "typescript"), ".");
    let completion = servers.complete(&at).unwrap().wait(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.items[0].item.label, "log");
    let hover =
        servers.hover(&ServerAt { offset: 2, ..at }).unwrap().wait(Duration::from_secs(5)).unwrap();
    assert!(hover.unwrap().code.contains("console"));
    assert!(
        servers.definition(&at).is_some()
            && servers.highlights(&at).is_some()
            && servers.symbols(&at).is_some()
    );
    // Another project, another document: no answer.
    let other = ServerAt { project: kernel::ProjectId::new(), ..at };
    assert!(servers.complete(&other).is_none());
    assert_eq!(servers.served(other.project, "typescript"), Engine::BuiltIn);
}

#[test]
fn a_documents_answers_wait_for_its_server_to_be_ready() {
    let _h = Harness::new(Behaviour { delay: Duration::from_millis(200), ..Behaviour::default() });
    let project = Project::open("starting");
    let doc = project.open_doc("a.ts", "typescript", "x");
    let servers = crate::port::Servers;
    let at = ServerAt {
        project: project.id,
        document: doc,
        path: &project.root,
        language: "typescript",
        version: 1,
        offset: 0,
    };
    // Starting: the built-in engine answers.
    assert_eq!(servers.served(project.id, "typescript"), Engine::BuiltIn);
    assert!(servers.hover(&at).is_none());
    ready(&project);
    assert!(servers.hover(&at).is_some());
}

#[test]
fn closing_its_last_document_leaves_it_idle() {
    let _h = Harness::new(Behaviour::default());
    let project = Project::open("idle");
    let doc = project.open_doc("a.ts", "typescript", "x");
    ready(&project);
    project.close(doc, "a.ts");
    wait("idle", || project.servers().servers(|s| s[0].client.status().state() == State::Idle));
}

#[test]
fn a_server_without_diagnostics_of_its_own_says_so() {
    let _h = Harness::new(Behaviour::default());
    let project = Project::open("quiet");
    let servers = crate::port::Servers;
    // Nothing running: nothing kept (the built-in engine answers everything anyway).
    assert_eq!(servers.kept(project.id, "html"), Kept::default());
    project.open_doc("index.html", "html", "<div></div>");
    project.open_doc("a.ts", "typescript", "let a = 1;");
    wait("both ready", || {
        project.servers().servers(|s| {
            s.len() == 2
                && s.iter().all(|r| matches!(r.client.status().state(), State::Ready | State::Idle))
        })
    });
    assert_eq!(servers.served(project.id, "html"), Engine::Server);
    assert_eq!(servers.kept(project.id, "html"), Kept { diagnostics: true, ..Kept::default() });
    assert_eq!(servers.kept(project.id, "typescript"), Kept::default());
    // The languages each server answers: a rename's places from it cover their files.
    assert_eq!(servers.languages_with(project.id, "typescript"), ["tsx", "typescript"]);
    assert_eq!(servers.languages_with(project.id, "html"), ["html"]);
    assert_eq!(servers.languages_with(project.id, "css"), ["css"]);
}

#[test]
fn clearing_the_caches_removes_the_servers_folders_and_starts_them_again() {
    let h = Harness::new(Behaviour::default());
    let project = Project::open("purge");
    project.open_doc("init.lua", "lua", "print(1)");
    project.open_doc("a.ts", "typescript", "let a = 1;");
    wait("two servers", || count(&project) == 2);
    // Both launched (on their drivers' threads, a moment after they're known): counted
    // before the purge, so only the relaunch is counted after it.
    wait("two launches", || h.launches() == 2);
    let folder = project.root.join(kernel::brand::PROJECT_DIR).join("lsp");
    let own = lsp_client::spec::cache_dir(&project.root, "cached");
    // Its folder, made when it started; what it wrote there; an old server's folder.
    assert!(own.is_dir(), "{}", own.display());
    std::fs::write(own.join("log"), "written by the server").unwrap();
    std::fs::create_dir_all(folder.join("gone-server")).unwrap();
    let id_of = |name: &str| {
        project.servers().servers(|s| s.iter().find(|r| r.spec.name == name).map(|r| r.id))
    };
    let (cached, fake) = (id_of("cached"), id_of("fake"));
    let launches = h.launches();
    project.events.publish(&workspace::events::CachesCleared { root: project.root.clone() });
    // Started again for the Lua document, its folder new (nothing it wrote is left); the
    // TypeScript server, writing nothing there, untouched.
    wait("started again", || id_of("cached").is_some_and(|id| Some(id) != cached));
    // Launched on its driver's thread, a moment after it's known.
    wait("launched again", || h.launches() == launches + 1);
    assert_eq!(id_of("fake"), fake);
    assert!(own.is_dir());
    assert!(!own.join("log").exists());
    assert!(!folder.join("gone-server").exists());
    let uri = lsp_client::uri::from_path(&project.root.join("init.lua"));
    wait("the document reopened", || h.seen(|s| s.documents.contains_key(&uri)));
}

/// A server whose description makes the built-in engine its languages' default (phpactor on
/// Windows) starts only once it's chosen; choosing the default again saves nothing.
#[test]
fn a_server_the_built_in_engine_answers_for_by_default_starts_only_once_chosen() {
    let h = Harness::new(Behaviour::default());
    assert_eq!(settings::default_choice("toml").engine, Engine::BuiltIn);
    assert_eq!(settings::default_choice("typescript").engine, Engine::Server);
    assert_eq!(Config::default().choice("toml").engine, Engine::BuiltIn);
    let project = Project::open("opt-in");
    project.open_doc("Cargo.toml", "toml", "[package]\n");
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(count(&project), 0);
    assert_eq!(project.servers().served("toml"), Engine::BuiltIn);
    // Chosen: it starts for the document open.
    let server = Choice { engine: Engine::Server, server: None };
    settings::set(Config {
        engines: vec![("toml".into(), server)],
        idle: None,
        ..Config::default()
    });
    ready(&project);
    assert_eq!(h.launches(), 1);
    assert_eq!(project.servers().served("toml"), Engine::Server);
}

#[test]
fn a_project_behind_anothers_gives_its_servers_up_until_it_comes_forward() {
    let h = Harness::new(Behaviour::default());
    crate::projects::behind_after(Duration::from_millis(20));
    let (behind, front) = (Project::open("behind"), Project::open("front"));
    behind.open_doc("a.ts", "typescript", "let a = 1;");
    ready(&behind);
    front.open_doc("b.ts", "typescript", "let b = 2;");
    ready(&front);

    front.events.publish(&workspace::events::ProjectBroughtForward);
    wait("stopped behind", || count(&behind) == 0);
    assert_eq!(count(&front), 1, "the one in front keeps its own");
    behind.events.publish(&workspace::events::ProjectBroughtForward);
    ready(&behind);
    assert_eq!(h.launches(), 3, "started again once");
    let uri = lsp_client::uri::from_path(&behind.root.join("a.ts"));
    wait("its document opened again", || h.seen(|s| s.documents.contains_key(&uri)));
    wait("the other one behind now", || count(&front) == 0);
}

#[test]
fn a_project_whose_window_was_left_gives_its_servers_up_until_it_comes_forward() {
    let h = Harness::new(Behaviour::default());
    let project = Project::open("left");
    project.open_doc("a.ts", "typescript", "let a = 1;");
    ready(&project);
    // Another program in front (or minimized): after the long wait (short here), stopped.
    crate::projects::left_after(Some(Duration::from_millis(20)));
    project.events.publish(&workspace::events::ProjectLeft);
    wait("stopped once left", || count(&project) == 0);
    project.events.publish(&workspace::events::ProjectBroughtForward);
    ready(&project);
    assert_eq!(h.launches(), 2, "started again once");
}

#[test]
fn coming_back_before_the_wait_ends_stops_nothing() {
    let h = Harness::new(Behaviour::default());
    crate::projects::behind_after(Duration::from_secs(60));
    let (one, two) = (Project::open("back-one"), Project::open("back-two"));
    one.open_doc("a.ts", "typescript", "let a = 1;");
    ready(&one);
    two.events.publish(&workspace::events::ProjectBroughtForward);
    one.events.publish(&workspace::events::ProjectBroughtForward);
    assert_eq!(count(&one), 1);
    assert_eq!(h.launches(), 1);
}
