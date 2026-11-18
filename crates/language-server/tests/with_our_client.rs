//! The program, started as an editor starts it, driven by our LSP client: each end checks
//! the other.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use document::Document;
use kernel::DocumentId;
use lsp_client::client::{Client, Config, Event, RestartPolicy, Timeouts};
use lsp_client::launch::ProcessLauncher;
use lsp_client::spec::{self, Places};
use lsp_client::status::State;

const WAIT: Duration = Duration::from_secs(30);

const CART: &str = "<?php\nclass Cart {\n    /** What's owed. */\n    public function total(int $tax): int { return $tax; }\n}\n";
const SHOP: &str =
    "<?php\nfunction shop(Cart $c) {\n    return $c->total(1);\n}\n$x = new Cart();\n$x->";
const BROKEN: &str = "<?php\nfunction broken( {\n";

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("language-server-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("cart.php"), CART).unwrap();
    std::fs::write(root.join("shop.php"), SHOP).unwrap();
    root
}

fn start(root: &Path) -> (Client, Arc<Mutex<Vec<Event>>>) {
    let toml = format!(
        "[[language-servers]]\nname = \"own\"\nlanguages = [\"php\"]\ncommand = {:?}\n",
        env!("CARGO_BIN_EXE_language-server")
    );
    let server = spec::parse(&toml, "test").unwrap().remove(0);
    let launch = server.launch(&Places::default()).unwrap();
    let config = Config {
        name: "own".into(),
        root: root.to_path_buf(),
        project_root: root.to_path_buf(),
        initialization_options: launch.initialization_options.clone(),
        settings: launch.settings.clone(),
        contents: Vec::new(),
        completion_kinds: launch.completion_kinds.clone(),
        on_initialized: Vec::new(),
        debounce: Duration::from_millis(10),
        timeouts: Timeouts { start: WAIT, request: WAIT, shutdown: Duration::from_secs(2) },
        restart: RestartPolicy::default(),
        idle: None,
        trace: Arc::new(AtomicBool::new(false)),
        secret_options: Vec::new(),
        secrets: None,
        show_messages: false,
        wake: Arc::new(|| {}),
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&events);
    let launcher = Box::new(ProcessLauncher { launch, dir: root.to_path_buf() });
    let client =
        Client::start(config, launcher, Arc::new(move |e| heard.lock().unwrap().push(e))).unwrap();
    let until = Instant::now() + WAIT;
    while !matches!(client.status().state(), State::Ready | State::Idle) {
        assert!(Instant::now() < until, "it didn't start");
        std::thread::sleep(Duration::from_millis(10));
    }
    (client, events)
}

fn open(client: &Client, root: &Path, name: &str, text: &str) -> DocumentId {
    let doc = DocumentId::next();
    let snapshot = Document::new(text).buffer().snapshot();
    client.open(doc, root.join(name), "php".into(), 1, snapshot);
    doc
}

/// Asks again until the debug form of the answer holds `needle` (the index fills meanwhile).
fn eventually<T: std::fmt::Debug>(what: &str, needle: &str, ask: impl Fn() -> Option<T>) -> String {
    let until = Instant::now() + WAIT;
    loop {
        let answer = format!("{:?}", ask());
        if answer.contains(needle) {
            return answer;
        }
        assert!(Instant::now() < until, "{what}: no {needle:?} in {answer}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn it_answers_our_client_across_the_project() {
    let root = project("answers");
    let (client, _events) = start(&root);
    let doc = open(&client, &root, "shop.php", SHOP);
    let call = SHOP.find("total").unwrap();

    let definition =
        eventually("definition", "cart.php", || client.definition(doc, call).wait(WAIT));
    assert!(definition.contains("line: 3"), "{definition}");
    eventually("hover", "total(int $tax)", || client.hover(doc, call, call..call + 5).wait(WAIT));
    eventually("references", "shop.php", || client.references(doc, call).wait(WAIT));
    eventually("signature", "$tax", || client.signature_help(doc, call + 6).wait(WAIT));
    eventually("suggestions", "total", || client.completion(doc, SHOP.len()).wait(WAIT));
    eventually("symbols", "shop", || client.symbols(doc).wait(WAIT));
    client.stop();
}

#[test]
fn a_syntax_error_is_published_and_goes_when_fixed() {
    let root = project("diagnostics");
    let (client, events) = start(&root);
    let doc = open(&client, &root, "broken.php", BROKEN);
    let last = || {
        events.lock().unwrap().iter().rev().find_map(|e| match e {
            Event::Diagnostics { path, diagnostics } if path.ends_with("broken.php") => {
                Some(diagnostics.clone())
            }
            _ => None,
        })
    };
    let until = Instant::now() + WAIT;
    while last().is_none_or(|d| d.is_empty()) {
        assert!(Instant::now() < until, "nothing published");
        std::thread::sleep(Duration::from_millis(10));
    }
    client.change(doc, 2, Document::new("<?php\nfunction fixed() {}\n").buffer().snapshot());
    let until = Instant::now() + WAIT;
    while !last().is_some_and(|d| d.is_empty()) {
        assert!(Instant::now() < until, "still reported: {:?}", last());
        std::thread::sleep(Duration::from_millis(10));
    }
    client.stop();
}

#[test]
fn it_leaves_nothing_in_the_project_and_keeps_to_its_php_level() {
    let root = project("level");
    std::fs::write(root.join("composer.json"), r#"{"require": {"php": "^7.4"}}"#).unwrap();
    let (client, _events) = start(&root);
    let text = "<?php\nmatc";
    let doc = open(&client, &root, "level.php", text);
    let answer =
        eventually("suggestions", "items", || client.completion(doc, text.len()).wait(WAIT));
    assert!(!answer.contains("label: \"match\""), "newer than the project's PHP: {answer}");
    client.stop();
    let names: Vec<_> =
        std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names.len(), 3, "only its own files: {names:?}");
}

#[test]
fn a_rename_reaches_every_use_across_the_project() {
    let root = project("rename");
    let (client, _events) = start(&root);
    let doc = open(&client, &root, "shop.php", SHOP);
    let call = SHOP.find("total").unwrap();
    eventually("the index", "cart.php", || client.definition(doc, call).wait(WAIT));
    let renamed =
        eventually("rename", "cart.php", || client.rename(doc, call, "sum".into()).wait(WAIT));
    assert!(renamed.contains("shop.php"), "{renamed}");
    assert_eq!(renamed.matches("\"sum\"").count(), 2, "{renamed}");
    client.stop();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_local_variable_is_renamed_only_in_its_function() {
    let text =
        "<?php\nfunction a() {\n    $v = 1;\n    return $v;\n}\nfunction b() {\n    $v = 2;\n}\n";
    let root = project("rename-local");
    std::fs::write(root.join("vars.php"), text).unwrap();
    let (client, _events) = start(&root);
    let doc = open(&client, &root, "vars.php", text);
    let at = text.find("$v").unwrap();
    let renamed = eventually("rename", "$w", || client.rename(doc, at, "w".into()).wait(WAIT));
    assert_eq!(renamed.matches("\"$w\"").count(), 2, "{renamed}");
    client.stop();
    let _ = std::fs::remove_dir_all(&root);
}

/// A framework's names reach another editor: Laravel's routes, in a project that uses it.
#[test]
fn a_framework_s_names_are_suggested() {
    let root = project("framework");
    std::fs::write(root.join("composer.json"), r#"{"require":{"laravel/framework":"^11"}}"#)
        .unwrap();
    std::fs::create_dir_all(root.join("routes")).unwrap();
    std::fs::write(root.join("artisan"), "#!/usr/bin/env php\n").unwrap();
    std::fs::write(
        root.join("routes/web.php"),
        "<?php\nRoute::get('/', fn () => 1)->name('home.page');\n",
    )
    .unwrap();
    let text = "<?php\nroute('ho');\n";
    let (client, _events) = start(&root);
    let doc = open(&client, &root, "page.php", text);
    let at = text.find("ho").unwrap() + 2;
    eventually("routes", "home.page", || client.completion(doc, at).wait(WAIT));
    client.stop();
}

/// The other languages answer too, templates by their own words.
#[test]
fn each_language_suggests_its_own() {
    let root = project("languages");
    let (client, _events) = start(&root);
    for (name, text, needle) in [
        ("page.html.twig", "{{ name|upp", "upper"),
        ("page.blade.php", "@fore", "@foreach"),
        ("page.html", "<sec", "section"),
        ("page.css", "a { colo", "color"),
    ] {
        let doc = open(&client, &root, name, text);
        eventually(name, needle, || client.completion(doc, text.len()).wait(WAIT));
    }
    client.stop();
}
