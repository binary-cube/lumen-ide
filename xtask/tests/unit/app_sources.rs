use super::*;

fn source(path: &str, text: &str) -> Source {
    Source { path: path.into(), text: text.into() }
}

fn names() -> ModuleNames {
    let mut names = ModuleNames::default();
    names.add(
        r#"
pub const LOG: CommandId = CommandId("git.log");
pub const THREAD: &str = "tree-search";
fn go() { kernel::work::run("git-status", || ()); Pool::new("image-decode", 2); }
fn named() { std::thread::Builder::new().name("remote-sync".into()); }
ctx.contribute(workbench_model::ModuleThreads(&[
    "refactor-", // a prefix
    "find-usages",
]));
"#,
    );
    names
}

#[test]
fn a_module_s_commands_and_threads_are_read() {
    let names = names();
    assert_eq!(names.commands.iter().collect::<Vec<_>>(), ["git.log"]);
    let threads: Vec<_> = names.threads.iter().map(String::as_str).collect();
    assert_eq!(
        threads,
        ["find-usages", "git-status", "image-decode", "refactor-", "remote-sync", "tree-search"]
    );
    assert_eq!(names.kind_of("git.log"), Some("command"));
    assert_eq!(names.kind_of("refactor-rename"), Some("thread"), "under a prefix");
    assert_eq!(names.kind_of("git"), None);
}

#[test]
fn the_app_writing_a_module_s_command_or_thread_is_found() {
    let sources = [
        source("workbench/task_data.rs", r#"part("tasks.index", vec!["symbols", "git-status"]);"#),
        source("workbench/menus.rs", r#"let id = "git.log"; // "find-usages" in a comment"#),
        source("app/actions.rs", r#"cmd::PASTE => Some("paste"),"#),
        source("commands.rs", r#"let quoted = "say \"git.log\""; let c = '"';"#),
    ];
    let mut names = names();
    names.add(r#"work::run("paste", || ());"#);
    let found = violations(&sources, &[], &names);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("crates/app/src/workbench/task_data.rs:1:"));
    assert!(found[0].contains("thread") && found[0].contains("git-status"));
    assert!(found[1].contains("command") && found[1].contains("git.log"));
}

#[test]
fn the_workbench_names_no_adapter() {
    let sources = [
        source("workbench/files.rs", "let store = storage::TomlSessionStore;"),
        source("workbench/disk.rs", "use fs_local as local;"),
        source("workbench/tab.rs", "let s = project_storage::x(); crate::storage::y();"),
        source("ports.rs", "disk: Arc::new(fs_local::LocalFileSystem),"),
    ];
    let found = violations(&sources, &[], &ModuleNames::default());
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|v| v.contains("adapter")));
}

#[test]
fn only_the_loading_and_the_snapshots_name_modules() {
    let modules = ["project_tree".to_owned(), "vcs_git".to_owned()];
    let sources = [
        source("main.rs", "mod modules;\n#[cfg(feature = \"snapshot\")]\nmod snapshot;\nmod x;"),
        source("modules.rs", "registry.register(Box::new(project_tree::TreeModule));"),
        source("snapshot.rs", "let _ = vcs_git::LOG; run(\"git.log\");"),
        source("snapshot/steps.rs", "use vcs_git as git;"),
        source("x.rs", "let _ = project_tree::TreeModule; run(\"git.log\");"),
        source("workbench/y.rs", "use vcs_git as git; let _ = my_vcs_git::A;"),
    ];
    let mut names = ModuleNames::default();
    names.add(r#"CommandId("git.log")"#);
    let found = violations(&sources, &modules, &names);
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(found.iter().filter(|v| v.starts_with("crates/app/src/x.rs:1:")).count(), 2);
    assert!(
        found[2].starts_with("crates/app/src/workbench/y.rs:1:") && found[2].contains("vcs_git")
    );
    assert_eq!(snapshot_only(&sources), ["snapshot"]);
}

#[test]
fn a_snapshot_module_deeper_down_is_found_where_it_lives() {
    let sources = [
        source(
            "workbench/mod.rs",
            "#[cfg(feature = \"snapshot\")]\n#[allow(dead_code)]\npub(crate) mod scenes;",
        ),
        source("app.rs", "#[cfg(feature = \"snapshot\")]\nfn only_here() {}\nmod later;"),
    ];
    assert_eq!(snapshot_only(&sources), ["workbench/scenes"]);
    assert!(is_within("workbench/scenes.rs", &["workbench/scenes".to_owned()]));
    assert!(is_within("workbench/scenes/a.rs", &["workbench/scenes".to_owned()]));
    assert!(!is_within("workbench/scenes_more.rs", &["workbench/scenes".to_owned()]));
}
