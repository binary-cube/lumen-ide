//! The language servers in the editor, against the fake server (in memory, never a real
//! one: a test's binary finds no bundled servers): documents announced as they open and
//! change, the answers used — completion, hover, signature help, definition, highlights,
//! rename's places, diagnostics — and typing that never waits for a server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once, RwLock};
use std::time::{Duration, Instant};

use lsp_client::fake::{self, Behaviour, Seen};

use super::*;

/// The fake servers' doings and behaviour, per project root (each test has its own), shared
/// with the launcher the module uses in these tests.
static SEEN: Mutex<Option<HashMap<PathBuf, Served>>> = Mutex::new(None);

/// A project's fake server: what it saw, how it behaves.
type Served = (Arc<Mutex<Seen>>, Behaviour);
/// The tests run side by side (each its own project, its own fake server); one choosing
/// PHP's server (the process's setting) runs alone among those with PHP files.
static PHP: RwLock<()> = RwLock::new(());
/// A question's timeout in these tests.
const TIMEOUT: Duration = Duration::from_secs(3);

/// The fake server described for TypeScript in a user file of a temporary folder (and for
/// HTML as a server reporting no diagnostics of its own, for CSS as one leaving the
/// deprecations and renames to the built-in engine, for C as a language whose built-in
/// engine is its syntax only, for PHP as phpactor is: the built-in engine keeping its
/// suggestions, renames and diagnostics).
fn describe() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("servers-user-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = std::env::current_exe().unwrap();
        let file = dir.join("language-servers.toml");
        std::fs::write(
            &file,
            format!(
                "[[language-servers]]\nname = \"fake\"\nlanguages = [\"typescript\"]\ncommand = {exe:?}\n\n[[language-servers]]\nname = \"quiet\"\nlanguages = [\"html\"]\ncommand = {exe:?}\nbuilt-in = [\"diagnostics\"]\n\n[[language-servers]]\nname = \"styles\"\nlanguages = [\"css\"]\ncommand = {exe:?}\nbuilt-in = [\"deprecated\", \"rename\"]\n\n[[language-servers]]\nname = \"native\"\nlanguages = [\"c\"]\ncommand = {exe:?}\n\n[[language-servers]]\nname = \"phpactor\"\nlanguages = [\"php\"]\ncommand = {exe:?}\nbuilt-in = [\"completion\", \"diagnostics\", \"rename\"]\n\n[[language-servers]]\nname = \"intelephense\"\nlanguages = [\"php\"]\ncommand = {exe:?}\nargs = [\"${{server}}/lib/intelephense.js\"]\nbuilt-in = [\"completion\", \"diagnostics\"]\nsecret-options = {{ licenceKey = \"intelephense-licence\" }}\n[language-servers.install]\nnpm = \"intelephense\"\nversion = \"1.0.0\"\nintegrity = \"sha512-none\"\nentry = \"lib/intelephense.js\"\n",
                exe = exe.to_string_lossy()
            ),
        )
        .unwrap();
        // Installed servers in a folder of the test's (intelephense, as the user installed
        // it), secrets in memory, no licence file: never the user's.
        let installs = dir.join("installs");
        let package = installs.join("intelephense").join("1.0.0");
        std::fs::create_dir_all(package.join("lib")).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name": "intelephense", "version": "1.0.0"}"#)
            .unwrap();
        std::fs::write(package.join("lib/intelephense.js"), "").unwrap();
        language_servers::use_install_dir(installs);
        language_servers::use_secrets(Arc::new(MemorySecrets::default()));
        language_servers::use_licence_files(Vec::new());
        language_servers::use_user_file(file);
        // A question never answered ends soon (the tests of a server that times out) — but
        // well after the graces, so what's shown before it is told apart.
        language_servers::request_timeout(TIMEOUT);
        language_servers::launch_with(Box::new(|_, _, root| {
            let (seen, behaviour) = SEEN
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|projects| projects.get(root).cloned())
                .unwrap_or_default();
            Box::new(fake::launcher(seen, Arc::new(Mutex::new(0)), move |_| behaviour.clone()))
        }));
    });
}

/// Secrets in memory (never the keychain).
#[derive(Default)]
struct MemorySecrets(Mutex<std::collections::HashMap<String, String>>);

impl kernel::SecretStore for MemorySecrets {
    fn get(&self, id: &str) -> Option<kernel::Secret> {
        self.0.lock().unwrap().get(id).map(|s| kernel::Secret::new(s.as_str()))
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(id.into(), secret.into());
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

struct Opened {
    wb: Workbench,
    text: TextSystem,
    dir: PathBuf,
    seen: Arc<Mutex<Seen>>,
    _lock: Guard,
}

/// A test's hold on [`PHP`]: none (no PHP file), beside the others, or alone.
#[allow(dead_code)] // held, never read
enum Guard {
    None,
    Beside(std::sync::RwLockReadGuard<'static, ()>),
    Alone(std::sync::RwLockWriteGuard<'static, ()>),
}

/// A project with `files`, `open` opened in a tab, the fake server behaving as `behaviour`;
/// the test runs beside the others.
fn opened(name: &str, files: &[(&str, &str)], open: &str, behaviour: Behaviour) -> Opened {
    let php = files.iter().any(|(f, _)| f.ends_with(".php"));
    let lock = if php {
        Guard::Beside(PHP.read().unwrap_or_else(std::sync::PoisonError::into_inner))
    } else {
        Guard::None
    };
    opened_with(lock, name, files, open, behaviour)
}

/// [`opened`], for a test choosing PHP's server: alone among those with PHP files.
fn opened_choosing_php(
    name: &str,
    files: &[(&str, &str)],
    open: &str,
    behaviour: Behaviour,
) -> Opened {
    let lock = Guard::Alone(PHP.write().unwrap_or_else(std::sync::PoisonError::into_inner));
    opened_with(lock, name, files, open, behaviour)
}

fn opened_with(
    lock: Guard,
    name: &str,
    files: &[(&str, &str)],
    open: &str,
    behaviour: Behaviour,
) -> Opened {
    describe();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let dir = std::env::temp_dir().join(format!("servers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, content) in files {
        std::fs::write(dir.join(file), content).unwrap();
    }
    let dir = crate::workbench::canonical(&dir);
    SEEN.lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(dir.clone(), (Arc::clone(&seen), behaviour));
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join(open)).unwrap();
    let mut text = TextSystem::new();
    frame(&mut wb, &mut text);
    Opened { wb, text, dir, seen, _lock: lock }
}

/// A frame: what came in polled, then painted (what's due used).
fn frame(wb: &mut Workbench, text: &mut TextSystem) {
    wb.poll_servers();
    wb.poll_diagnostics();
    wb.poll_symbols();
    wb.poll_jobs();
    wb.paint(text, &mut render_api::Scene::default());
}

impl Opened {
    /// Frames until `done` (5 s at most).
    fn settle(&mut self, what: &str, done: impl Fn(&mut Workbench, &Seen) -> bool) {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            frame(&mut self.wb, &mut self.text);
            let seen = Arc::clone(&self.seen);
            if done(&mut self.wb, &seen.lock().unwrap()) {
                return;
            }
            assert!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn ready(&mut self) {
        self.settle("the server ready", |wb, _| {
            wb.served_at(0) != code_intelligence::servers::Engine::BuiltIn
        });
    }

    fn server_text(&self, name: &str) -> Option<String> {
        let uri = lsp_client::uri::from_path(&self.dir.join(name));
        self.seen.lock().unwrap().documents.get(&uri).cloned()
    }

    /// Types `typed`, a key at a time.
    fn type_text(&mut self, typed: &str) {
        for c in typed.chars() {
            if c == '\n' {
                self.wb.edit_key(&mut self.text, EditKey::Enter { shift: false });
            } else {
                self.wb.type_text(&mut self.text, &c.to_string());
            }
        }
    }

    fn cursor_at_end(&mut self) {
        self.wb.edit(&mut self.text, |doc| {
            let end = doc.buffer().len();
            commands::click(doc, end, false, false);
        });
    }
}

#[test]
fn a_project_without_files_of_a_served_language_starts_no_server() {
    let mut o =
        opened("none", &[("a.txt", "a\n"), ("b.md", "# B\n")], "a.txt", Behaviour::default());
    o.wb.open_file(&o.dir.join("b.md")).unwrap();
    for _ in 0..5 {
        frame(&mut o.wb, &mut o.text);
    }
    assert!(o.seen.lock().unwrap().methods.is_empty(), "no server was started");
}

#[test]
fn a_documents_text_and_its_edits_reach_its_server() {
    let mut o = opened("sync", &[("a.ts", "let total = 1;\n")], "a.ts", Behaviour::default());
    o.ready();
    assert_eq!(o.server_text("a.ts").as_deref(), Some("let total = 1;\n"));
    o.cursor_at_end();
    o.type_text("const ț = 😀;");
    let expected = o.wb.tabs[o.wb.active].doc.buffer().text();
    o.settle("in step", |_, s| s.documents.values().any(|t| *t == expected));
    // Closing the tab closes it in the server.
    let active = o.wb.active;
    o.wb.close_tab(active);
    o.settle("closed", |_, s| s.count("textDocument/didClose") == 1);
}

#[test]
fn completion_comes_from_the_server_and_adds_its_import() {
    let mut o =
        opened("completion", &[("a.ts", "const total = 1;\n")], "a.ts", Behaviour::default());
    o.ready();
    o.cursor_at_end();
    o.type_text("console.");
    o.settle("the list", |wb, _| wb.completion.items().iter().any(|i| i.label == "log"));
    assert_eq!(o.wb.completion.items()[0].label, "log");
    // Filtered as it's typed (from the same answer).
    o.type_text("w");
    o.settle("filtered", |wb, _| wb.completion.items().first().is_some_and(|i| i.label == "warn"));
    assert!(o.wb.accept_completion(&mut o.text));
    let text = o.wb.tabs[o.wb.active].doc.buffer().text();
    assert!(text.ends_with("console.warn()"), "{text:?}");
    // A suggestion whose import the server works out when it's accepted.
    o.type_text(";\nrea");
    o.settle("the list", |wb, _| wb.completion.items().iter().any(|i| i.label == "readFile"));
    let at = o.wb.completion.items().iter().position(|i| i.label == "readFile").unwrap();
    o.wb.completion.move_selection(at as isize);
    assert!(o.wb.accept_completion(&mut o.text));
    o.settle("the import", |wb, _| {
        wb.tabs[wb.active].doc.buffer().text().starts_with("import { readFile } from 'fs';\n")
    });
}

#[test]
fn hover_signature_and_highlights_come_from_the_server() {
    let source = "const total = 1;\nfunction add(a, b) {}\nadd(total, total);\n";
    let mut o = opened("hover", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    let word = source.rfind("total").unwrap();
    o.settle("the hover", |wb, _| matches!(wb.served_hover(word), Some(Some(Some(_)))));
    let Some(Some(Some(info))) = o.wb.served_hover(word) else { unreachable!() };
    assert_eq!(info.code, "const total: number");
    // The parameters' help after `(` and `,`.
    let comma = source.rfind(", ").unwrap() + 1;
    o.settle("the signature", |wb, _| matches!(wb.served_signature(comma), Some(Some(Some(_)))));
    // The occurrences of the symbol at the cursor.
    o.wb.edit(&mut o.text, |doc| commands::click(doc, word + 1, false, false));
    o.settle("highlights", |wb, _| wb.tabs[wb.active].occurrences.len() == 3);
}

#[test]
fn go_to_declaration_jumps_where_the_server_says() {
    let source = "const total = 1;\nconsole.log(total);\n";
    let mut o = opened("definition", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    let usage = source.rfind("total").unwrap() + 2;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, usage, false, false));
    assert!(o.wb.goto_definition(&mut o.text));
    o.settle("the jump", |wb, _| {
        wb.tabs[wb.active].doc.selections().primary().head == source.find("total").unwrap()
    });
}

#[test]
fn a_rename_finds_the_servers_places() {
    let source = "const total = 1;\nconsole.log(total, totals);\n";
    let mut o =
        opened("rename", &[("a.ts", source), ("b.ts", "total;\n")], "a.ts", Behaviour::default());
    o.ready();
    o.wb.open_file(&o.dir.join("b.ts")).unwrap();
    o.wb.open_file(&o.dir.join("a.ts")).unwrap();
    o.settle("both open", |_, s| s.documents.len() == 2);
    let at = source.find("total").unwrap() + 1;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, at, false, false));
    let context = o.wb.rename_context().unwrap();
    assert_eq!(context.target.name, "total");
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    assert_eq!(plan.chosen(), (3, 2), "{plan:?}");
}

#[test]
fn the_servers_diagnostics_replace_the_built_in_engines() {
    let mut o =
        opened("diagnostics", &[("a.ts", "// TODO: x\nlet = ;\n")], "a.ts", Behaviour::default());
    o.ready();
    o.settle("the server's warning", |wb, _| {
        wb.tabs[wb.active].diagnostics.marks.iter().any(|m| m.message == "A task is left here.")
    });
    // Its syntax error (`let = ;`) isn't the built-in engine's twice: only the server's.
    let marks = &o.wb.tabs[o.wb.active].diagnostics.marks;
    assert!(
        marks.iter().all(|m| m.code.as_deref().is_some_and(|c| c.starts_with("fake"))),
        "{marks:?}"
    );
}

#[test]
fn a_server_without_diagnostics_leaves_the_built_in_engines_shown() {
    let mut o = opened(
        "quiet",
        &[("index.html", "<center>old</center>\n")],
        "index.html",
        Behaviour::default(),
    );
    o.ready();
    // HTML's server answers it, yet the built-in engine's analyses stay: `<center>` is
    // deprecated.
    o.settle("the built-in deprecation", |wb, _| {
        wb.tabs[wb.active].diagnostics.marks.iter().any(|m| m.struck)
    });
    assert!(!o.wb.server_alone());
}

#[test]
fn nothing_suggested_by_the_server_leaves_the_built_in_engines_suggestions() {
    let files =
        [("style.css", ".card {}\n.card-title {}\n"), ("index.html", "<p class=\"\"></p>\n")];
    let behaviour = Behaviour { silent_completion: true, ..Behaviour::default() };
    let mut o = opened("silent", &files, "index.html", behaviour);
    o.ready();
    let at = "<p class=\"".len();
    o.wb.edit(&mut o.text, |doc| commands::click(doc, at, false, false));
    o.type_text("car");
    // The project's classes, the built-in engine's: the server had nothing for there.
    o.settle("the classes", |wb, _| wb.completion.items().iter().any(|i| i.label == "card-title"));
    assert!(o.seen.lock().unwrap().count("textDocument/completion") >= 1);
}

#[test]
fn a_server_leaving_renames_to_the_built_in_engine_isnt_asked() {
    let files = [
        ("style.css", ".card {}\n.legacy { clip: rect(0 0 0 0); }\n"),
        ("index.html", "<p class=\"card\"></p>\n"),
    ];
    let mut o = opened("kept", &files, "style.css", Behaviour::default());
    o.ready();
    o.wb.edit(&mut o.text, |doc| commands::click(doc, 2, false, false));
    let context = o.wb.rename_context().unwrap();
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    // The class in the HTML file too (the server would know only the stylesheet's).
    assert!(plan.files.iter().any(|f| f.path.ends_with("index.html")), "{plan:?}");
    let seen = o.seen.lock().unwrap();
    assert_eq!(seen.count("textDocument/rename") + seen.count("textDocument/prepareRename"), 0);
    drop(seen);
    // Its deprecations stay shown (`clip`).
    o.settle("the deprecation", |wb, _| {
        wb.tabs[wb.active].diagnostics.marks.iter().any(|m| m.struck)
    });
}

#[test]
fn typing_never_waits_for_a_hung_server() {
    let mut o = opened(
        "hung",
        &[("a.ts", "let a = 1;\n")],
        "a.ts",
        Behaviour { hang: true, ..Behaviour::default() },
    );
    o.ready();
    o.cursor_at_end();
    let started = Instant::now();
    for _ in 0..200 {
        o.type_text("x");
        frame(&mut o.wb, &mut o.text);
    }
    // 200 keys and frames, the server reading nothing and answering nothing.
    let took = started.elapsed();
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert!(o.wb.tabs[o.wb.active].doc.buffer().text().ends_with(&"x".repeat(200)));
}

/// C has no built-in intelligence (its module gives only its grammar): completion, hover
/// and the declaration come from its server alone.
#[test]
fn a_language_with_only_its_syntax_built_in_is_answered_by_its_server() {
    let source = "int total = 1;\nint main(void) {\n  return ";
    let mut o = opened("native", &[("a.c", source)], "a.c", Behaviour::default());
    assert_eq!(o.wb.tabs[o.wb.active].syntax.language().as_str(), "c");
    assert!(o.wb.intelligence().is_none(), "no built-in engine of its own");
    o.ready();
    o.cursor_at_end();
    o.type_text("tot");
    o.settle("the list", |wb, _| wb.completion.items().iter().any(|i| i.label == "total"));
    assert_eq!(o.wb.completion.items()[0].label, "total");
    assert!(o.wb.accept_completion(&mut o.text));
    assert!(o.wb.tabs[o.wb.active].doc.buffer().text().ends_with("return total"));
    let word = source.find("total").unwrap();
    o.settle("the hover", |wb, _| matches!(wb.served_hover(word), Some(Some(Some(_)))));
    let usage = o.wb.tabs[o.wb.active].doc.buffer().len() - 2;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, usage, false, false));
    assert!(o.wb.goto_definition(&mut o.text));
    o.settle("the jump", |wb, _| wb.tabs[wb.active].doc.selections().primary().head == word);
}

// ─── Nothing the built-in engine did is lost (plan 43e) ───

/// How a server fails to answer `method`: an error, nothing (`null`), never (a timeout).
fn failing(method: &str) -> [Behaviour; 3] {
    let m = vec![method.to_owned()];
    [
        Behaviour { failing: m.clone(), ..Behaviour::default() },
        Behaviour { empty: m.clone(), ..Behaviour::default() },
        Behaviour { unanswered: m, ..Behaviour::default() },
    ]
}

#[test]
fn a_declaration_the_server_cant_give_is_the_built_in_engines() {
    let source = "function add(a, b) {}\nadd(1, 2);\n";
    for (i, behaviour) in failing("textDocument/definition").into_iter().enumerate() {
        let mut o = opened(&format!("no-definition-{i}"), &[("a.ts", source)], "a.ts", behaviour);
        o.ready();
        let usage = source.rfind("add").unwrap() + 1;
        o.wb.edit(&mut o.text, |doc| commands::click(doc, usage, false, false));
        assert!(o.wb.goto_definition(&mut o.text));
        o.settle("the built-in engine's jump", |wb, _| {
            wb.tabs[wb.active].doc.selections().primary().head == source.find("add").unwrap()
        });
        assert_eq!(o.seen.lock().unwrap().count("textDocument/definition"), 1, "case {i}");
    }
}

#[test]
fn a_hover_the_server_cant_give_is_the_built_in_engines() {
    let source = "function add(a, b) {}\nadd(1, 2);\n";
    for (i, behaviour) in failing("textDocument/hover").into_iter().enumerate() {
        let mut o = opened(&format!("no-hover-{i}"), &[("a.ts", source)], "a.ts", behaviour);
        o.ready();
        let start = source.rfind("add").unwrap();
        o.wb.code_hover.track(Some(start..start + 3), Point::new(10.0, 10.0), true);
        o.settle("the built-in engine's card", |wb, _| wb.code_hover.is_shown());
        assert_eq!(o.seen.lock().unwrap().count("textDocument/hover"), 1, "case {i}");
    }
}

#[test]
fn a_signature_the_server_cant_give_is_the_built_in_engines() {
    let source = "function add(first, second) {}\n";
    for (i, behaviour) in failing("textDocument/signatureHelp").into_iter().enumerate() {
        let mut o = opened(&format!("no-signature-{i}"), &[("a.ts", source)], "a.ts", behaviour);
        o.ready();
        o.cursor_at_end();
        o.type_text("add(1,");
        o.settle("the built-in engine's parameters", |wb, _| {
            wb.param_hint.card().is_some_and(|c| c.code.contains("second"))
        });
        // Not the server's (`b: string`).
        assert!(!o.wb.param_hint.card().unwrap().code.contains("string"), "case {i}");
    }
}

#[test]
fn suggestions_the_server_cant_give_are_the_built_in_engines_and_not_asked_again() {
    let source = "const totalCount = 1;\n";
    for (i, behaviour) in failing("textDocument/completion").into_iter().enumerate() {
        let mut o = opened(&format!("no-completion-{i}"), &[("a.ts", source)], "a.ts", behaviour);
        o.ready();
        o.cursor_at_end();
        o.type_text("totalC");
        o.settle("the built-in engine's list", |wb, _| {
            wb.completion.items().iter().any(|i| i.label == "totalCount")
        });
        // Never the server's (it always offers `readFile`).
        assert!(o.wb.completion.items().iter().all(|i| i.label != "readFile"), "case {i}");
        // Not asked again at each frame (at most once more: at the text as it is now).
        let asked = o.seen.lock().unwrap().count("textDocument/completion");
        assert!((1..=2).contains(&asked), "case {i}: {asked}");
        for _ in 0..20 {
            frame(&mut o.wb, &mut o.text);
        }
        let again = o.seen.lock().unwrap().count("textDocument/completion");
        assert!(again <= 2 && again >= asked, "case {i}: {again}");
        // Asked again at the next edit: a server failing once (still loading, the document
        // changed under it) isn't given up for the word for good.
        o.type_text("o");
        o.settle("asked again", |_, seen| seen.count("textDocument/completion") > again);
    }
}

/// The word erased: the server's list closes (it isn't left showing all it knows); Esc
/// closes it for good — a late answer doesn't open it again.
#[test]
fn an_erased_word_or_esc_closes_the_servers_list() {
    let source = "int total = 1;\nint main(void) {\n  return ";
    let behaviour = Behaviour {
        slow: vec![("textDocument/completion".into(), Duration::from_millis(300))],
        ..Behaviour::default()
    };
    let mut o = opened("erased", &[("a.c", source)], "a.c", behaviour);
    o.ready();
    o.cursor_at_end();
    o.type_text("tot");
    o.settle("the list", |wb, _| wb.completion.items().iter().any(|i| i.label == "total"));
    for _ in 0..3 {
        o.wb.edit_key(&mut o.text, EditKey::Backspace { word: false });
    }
    assert!(!o.wb.completion.is_open(), "{:?}", o.wb.completion.items());
    for _ in 0..40 {
        frame(&mut o.wb, &mut o.text);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!o.wb.completion.is_open(), "opened again: {:?}", o.wb.completion.items());
    // Asked, dismissed before the answer: it never shows.
    o.type_text("t");
    o.wb.edit_key(&mut o.text, EditKey::Escape);
    for _ in 0..60 {
        frame(&mut o.wb, &mut o.text);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!o.wb.completion.is_open(), "opened after Esc: {:?}", o.wb.completion.items());
    // Typed again: suggested again.
    o.type_text("o");
    o.settle("the list again", |wb, _| wb.completion.items().iter().any(|i| i.label == "total"));
}

#[test]
fn occurrences_the_server_cant_give_arent_asked_at_every_frame() {
    let source = "const total = 1;\nconsole.log(total);\n";
    let behaviour = Behaviour {
        failing: vec!["textDocument/documentHighlight".into()],
        ..Behaviour::default()
    };
    let mut o = opened("no-highlights", &[("a.ts", source)], "a.ts", behaviour);
    o.ready();
    o.wb.edit(&mut o.text, |doc| commands::click(doc, 8, false, false));
    o.settle("asked", |_, s| s.count("textDocument/documentHighlight") >= 1);
    for _ in 0..5 {
        frame(&mut o.wb, &mut o.text);
        std::thread::sleep(Duration::from_millis(20));
    }
    let asked = o.seen.lock().unwrap().count("textDocument/documentHighlight");
    for _ in 0..20 {
        frame(&mut o.wb, &mut o.text);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(o.seen.lock().unwrap().count("textDocument/documentHighlight"), asked);
}

/// A script's function used in a page: the server knows the scripts, the built-in engine
/// finds it in the page too — the rename (and Find Usages) has both.
#[test]
fn a_rename_joins_the_servers_places_and_the_built_in_engines_in_other_languages() {
    let script = "function greet() {}\ngreet();\n";
    let page = "<button>Hi</button>\n<script>greet();</script>\n";
    let files = [("a.ts", script), ("index.html", page)];
    let mut o = opened("union", &files, "a.ts", Behaviour::default());
    o.ready();
    o.wb.edit(&mut o.text, |doc| commands::click(doc, 10, false, false));
    let context = o.wb.rename_context().unwrap();
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    let script_file = plan.files.iter().find(|f| f.path.ends_with("a.ts")).expect("the script");
    assert_eq!(script_file.occurrences.len(), 2, "{plan:?}");
    let page_file = plan.files.iter().find(|f| f.path.ends_with("index.html"));
    assert!(page_file.is_some_and(|f| f.occurrences.len() == 1), "the page's use: {plan:?}");
    let seen = o.seen.lock().unwrap();
    assert!(seen.count("textDocument/rename") >= 1, "the server was asked");
}

#[test]
fn a_rename_the_server_cant_give_is_the_built_in_engines() {
    let script = "function greet() {}\ngreet();\n";
    for (i, behaviour) in failing("textDocument/rename").into_iter().enumerate() {
        let mut o = opened(
            &format!("no-rename-{i}"),
            &[("a.ts", script), ("b.ts", "greet();\n")],
            "a.ts",
            behaviour,
        );
        o.ready();
        o.wb.edit(&mut o.text, |doc| commands::click(doc, 10, false, false));
        let context = o.wb.rename_context().unwrap();
        let plan = refactor::application::plan_rename(
            &context.target,
            context.sources.as_ref(),
            context.resolver.as_ref(),
            &|| false,
        );
        let places: usize = plan.files.iter().map(|f| f.occurrences.len()).sum();
        assert!(places >= 2, "case {i}: {plan:?}");
        assert!(plan.files.iter().any(|f| f.path.ends_with("a.ts")), "case {i}: {plan:?}");
    }
}

/// A local variable the server can't rename stays in its function, as the built-in
/// engine renames it (never looked for in the other files).
#[test]
fn a_local_rename_the_server_cant_give_stays_in_its_scope() {
    let script = "function a() { let count = 1; return count; }\nfunction b() { let count = 2; }\n";
    let behaviour =
        Behaviour { failing: vec!["textDocument/rename".into()], ..Behaviour::default() };
    let mut o = opened("local", &[("a.ts", script), ("c.ts", "count;\n")], "a.ts", behaviour);
    o.ready();
    let at = script.find("count").unwrap() + 1;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, at, false, false));
    let context = o.wb.rename_context().unwrap();
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    assert_eq!(plan.files.len(), 1, "{plan:?}");
    assert_eq!(plan.chosen(), (2, 1), "{plan:?}");
}

/// The built-in engine's analyses stay beside the server's diagnostics, but not twice on
/// the same place.
#[test]
fn the_built_in_analyses_stay_but_not_where_the_server_marks() {
    let source = "import { readFile } from 'fs';\nimport { TODO } from 'fs';\n";
    let mut o = opened("analyses", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    o.settle("the server's warning and the unused import", |wb, _| {
        let marks = &wb.tabs[wb.active].diagnostics.marks;
        marks.iter().any(|m| m.message == "A task is left here.")
            && marks.iter().any(|m| m.weak && m.code.as_deref() == Some("readFile"))
    });
    // The second import is unused too, but the server marks a place in it: its mark only.
    let todo = source.find("TODO").unwrap();
    let marks = &o.wb.tabs[o.wb.active].diagnostics.marks;
    let there: Vec<_> =
        marks.iter().filter(|m| m.range.start <= todo && todo < m.range.end).collect();
    assert_eq!(there.len(), 1, "{marks:?}");
    assert!(there[0].code.as_deref().is_some_and(|c| c.starts_with("fake")), "{there:?}");
}

/// A framework's suggestions that join a list (React's hooks) join the server's, and its
/// card for a word (a hook) is shown without asking the server.
#[test]
fn the_frameworks_suggestions_and_cards_stay_beside_the_servers() {
    let files = [
        ("package.json", "{\"dependencies\": {\"react\": \"^18.0.0\"}}\n"),
        ("a.ts", "const usage = 1;\nuseEffect();\n"),
    ];
    let mut o = opened("framework", &files, "a.ts", Behaviour::default());
    o.ready();
    o.settle("React found", |wb, _| !wb.data.frameworks.active().is_empty());
    o.cursor_at_end();
    o.type_text("us");
    o.settle("both lists", |wb, _| {
        let items = wb.completion.items();
        // `useState`: only React says it (the file doesn't have it).
        items.iter().any(|i| i.label == "useState")
    });
    // The server's first (the document's names, as it says them).
    let labels: Vec<_> = o.wb.completion.items().iter().map(|i| i.label.clone()).collect();
    assert_eq!(labels[..2], ["usage", "useEffect"], "{labels:?}");
    assert!(o.seen.lock().unwrap().count("textDocument/completion") >= 1);
    // The hook's card: React's, the server not asked.
    let hook = "const usage = 1;\n".len();
    o.wb.code_hover.track(Some(hook..hook + 9), Point::new(10.0, 10.0), true);
    o.settle("the card", |wb, _| wb.code_hover.is_shown());
    assert_eq!(o.seen.lock().unwrap().count("textDocument/hover"), 0);
}

/// Where the built-in engine suggests the project's names (a class from another
/// stylesheet, which a stylesheet's server doesn't know), they join the server's list.
#[test]
fn the_projects_classes_join_the_servers_suggestions() {
    let files = [("other.css", ".card-title {}\n"), ("style.css", ".cardinal {}\n")];
    let mut o = opened("classes", &files, "style.css", Behaviour::default());
    o.ready();
    o.cursor_at_end();
    o.type_text(".car");
    o.settle("both", |wb, _| {
        let items = wb.completion.items();
        items.iter().any(|i| i.label == "card-title") && items.iter().any(|i| i.label == "cardinal")
    });
    assert!(o.seen.lock().unwrap().count("textDocument/completion") >= 1);
}

// ─── Nothing waits for a silent server (plan 43f) ───

/// A server that never answers completion: the built-in engine's list appears after the
/// grace, long before the request's timeout; the next word's doesn't wait at all.
#[test]
fn a_silent_servers_suggestions_dont_wait_for_its_timeout() {
    let source = "const totalCount = 1;\nconst other = 2;\n";
    let behaviour =
        Behaviour { unanswered: vec!["textDocument/completion".into()], ..Behaviour::default() };
    let mut o = opened("silent-completion", &[("a.ts", source)], "a.ts", behaviour);
    o.ready();
    o.cursor_at_end();
    let started = Instant::now();
    o.type_text("totalC");
    // Within the grace: the server's list is waited for (nothing shown, a redraw planned).
    assert!(!o.wb.completion.is_open());
    let deadline = o.wb.served_deadline().expect("the grace's end is scheduled");
    assert!(deadline <= Instant::now() + SERVER_GRACE);
    o.settle("the built-in engine's list", |wb, _| {
        wb.completion.items().iter().any(|i| i.label == "totalCount")
    });
    let took = started.elapsed();
    assert!(took < TIMEOUT / 2, "the list waited {took:?}");
    assert!(o.seen.lock().unwrap().cancelled.is_empty(), "shown before the timeout");
    // Another word: the server let the last grace pass, its list isn't waited for again.
    o.type_text(";\noth");
    assert!(
        o.wb.completion.items().iter().any(|i| i.label == "other"),
        "{:?}",
        o.wb.completion.items()
    );
}

/// A server answering after the grace: its suggestions join the built-in engine's list
/// after it — what was shown keeps its place and the selection stays.
#[test]
fn a_late_servers_suggestions_join_the_list_without_moving_it() {
    let source = "const totalCount = 1;\nconst totalSum = 2;\n// totalInComment\n";
    let behaviour = Behaviour {
        slow: vec![("textDocument/completion".into(), Duration::from_millis(600))],
        ..Behaviour::default()
    };
    let mut o = opened("late-completion", &[("a.ts", source)], "a.ts", behaviour);
    o.ready();
    o.cursor_at_end();
    o.type_text("total");
    o.settle("the built-in engine's list", |wb, _| wb.completion.items().len() >= 2);
    let before: Vec<String> = o.wb.completion.items().iter().map(|i| i.label.clone()).collect();
    assert!(!before.contains(&"totalInComment".to_owned()), "{before:?}");
    o.wb.completion.move_selection(1);
    let selected = o.wb.completion.selected().unwrap().label.clone();
    o.settle("the server's suggestions", |wb, _| {
        wb.completion.items().iter().any(|i| i.label == "totalInComment")
    });
    let after: Vec<String> = o.wb.completion.items().iter().map(|i| i.label.clone()).collect();
    assert_eq!(after[..before.len()], before[..], "{after:?}");
    assert_eq!(o.wb.completion.selected().unwrap().label, selected);
    // Typed further in the same word: still in that order.
    o.type_text("S");
    frame(&mut o.wb, &mut o.text);
    assert_eq!(o.wb.completion.items()[0].label, "totalSum");
}

/// A server that never answers a hover, a signature or a declaration: the built-in
/// engine's, after the grace, before the timeout — and no frames while it's waited for.
#[test]
fn a_silent_servers_card_signature_and_declaration_dont_wait_for_its_timeout() {
    let source = "function add(first, second) {}\nadd(1, 2);\n";
    let methods = ["textDocument/hover", "textDocument/signatureHelp", "textDocument/definition"];
    let behaviour = Behaviour {
        unanswered: methods.iter().map(|m| (*m).to_owned()).collect(),
        ..Behaviour::default()
    };
    let mut o = opened("silent-cards", &[("a.ts", source)], "a.ts", behaviour);
    o.ready();
    // The hover card.
    let start = source.rfind("add").unwrap();
    o.wb.code_hover.track(Some(start..start + 3), Point::new(10.0, 10.0), true);
    let started = Instant::now();
    frame(&mut o.wb, &mut o.text);
    // Waited for: the next frame is the grace's end, not at once (no frames meanwhile) —
    // told only while the grace lasts (a loaded machine may have spent it on this frame).
    let card = o.wb.hover_deadline();
    let served = o.wb.served_deadline();
    if started.elapsed() < SERVER_GRACE {
        assert!(!o.wb.code_hover.is_shown());
        assert!(card.is_none_or(|at| at > started), "{card:?}");
        assert!(served.is_some());
    }
    o.settle("the built-in engine's card", |wb, _| wb.code_hover.is_shown());
    // The server's card still awaited: the built-in engine's didn't wait for its timeout
    // (told by the question, not by the clock: a loaded machine's frames are slow).
    assert!(o.wb.data.served.hover.is_some(), "{:?}", started.elapsed());
    // Its answer still awaited, but no frame asked for it (its grace over; what else is
    // due, the other questions' graces, is theirs).
    let next = o.wb.data.served.hover.as_ref().and(o.wb.data.served.hover_grace);
    assert!(next.is_none(), "{next:?}");
    // The parameters.
    o.cursor_at_end();
    let started = Instant::now();
    o.type_text("add(1,");
    o.settle("the built-in engine's parameters", |wb, _| {
        wb.param_hint.card().is_some_and(|c| c.code.contains("second"))
    });
    assert!(o.wb.data.served.signature.is_some(), "{:?}", started.elapsed());
    // The declaration.
    let usage = source.rfind("add").unwrap() + 1;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, usage, false, false));
    let started = Instant::now();
    assert!(o.wb.goto_definition(&mut o.text));
    o.settle("the built-in engine's jump", |wb, _| {
        wb.tabs[wb.active].doc.selections().primary().head == source.find("add").unwrap()
    });
    let took = started.elapsed();
    assert!(took >= DEFINITION_GRACE && took < TIMEOUT, "{took:?}");
    // The server's question dropped (never a second jump).
    o.settle("the question cancelled", |_, s| !s.cancelled.is_empty());
}

// ─── PHP: phpactor by default, nothing of the built-in engine lost (plan 43g) ───

/// The built-in engine's suggestions (the stubs' functions) join the server's — its first.
#[test]
fn php_suggestions_are_the_union_of_the_servers_and_the_built_in_engines() {
    // A word only the server knows (the fake suggests the document's words, comments too).
    let source = "<?php\n// array_mangle\n";
    let mut o = opened("php-union", &[("index.php", source)], "index.php", Behaviour::default());
    o.ready();
    o.cursor_at_end();
    o.type_text("array_ma");
    o.settle("both lists", |wb, _| {
        let items = wb.completion.items();
        items.iter().any(|i| i.label == "array_mangle")
            && items.iter().any(|i| i.label == "array_map")
    });
    let labels: Vec<_> = o.wb.completion.items().iter().map(|i| i.label.clone()).collect();
    let at = |l: &str| labels.iter().position(|x| x == l).unwrap();
    assert!(at("array_mangle") < at("array_map"), "{labels:?}");
    assert!(o.seen.lock().unwrap().count("textDocument/completion") >= 1);
}

/// Rename and Find Usages stay the built-in engine's (the server isn't asked), across the
/// project's PHP files.
#[test]
fn php_renames_stay_the_built_in_engines() {
    let files = [
        ("helpers.php", "<?php\nfunction format_total(int $n): string { return (string) $n; }\n"),
        ("index.php", "<?php\nrequire 'helpers.php';\necho format_total(3);\n"),
    ];
    let mut o = opened("php-rename", &files, "helpers.php", Behaviour::default());
    o.ready();
    let at = files[0].1.find("format_total").unwrap() + 2;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, at, false, false));
    let context = o.wb.rename_context().unwrap();
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    assert!(plan.files.iter().any(|f| f.path.ends_with("index.php")), "{plan:?}");
    let seen = o.seen.lock().unwrap();
    assert_eq!(seen.count("textDocument/rename") + seen.count("textDocument/prepareRename"), 0);
}

/// The built-in engine's diagnostics (a syntax error) stay beside the server's.
#[test]
fn php_diagnostics_are_the_built_in_engines_and_the_servers() {
    let source = "<?php\n// TODO later\nfunction broken( {\n";
    let mut o =
        opened("php-diagnostics", &[("index.php", source)], "index.php", Behaviour::default());
    o.ready();
    o.settle("the server's warning and the syntax error", |wb, _| {
        let tab = &wb.tabs[wb.active];
        tab.diagnostics.marks.iter().any(|m| m.message == "A task is left here.")
            && !tab.diagnostics.errors.is_empty()
    });
    assert!(!o.wb.server_alone());
}

// ─── PHP: intelephense, installed by the user (plan 43h) ───

/// PHP answered by intelephense (chosen, as installed in the test's folder), its server
/// started; PHP's default again once `o` is done with.
fn choose_intelephense(o: &mut Opened) -> language_servers::settings::Pinned {
    let mut chosen = language_servers::settings::config();
    chosen.engines = vec![(
        "php".into(),
        language_servers::settings::Choice {
            engine: code_intelligence::servers::Engine::Server,
            server: Some("intelephense".into()),
        },
    )];
    // Pinned: another test loading the modules meanwhile applies the saved settings again.
    let pinned = language_servers::settings::pin(chosen);
    let project = o.wb.data.id;
    o.settle("intelephense started", |_, _| {
        language_servers::projects::project(project)
            .is_some_and(|p| p.servers(|s| s.iter().any(|r| r.spec.name == "intelephense")))
    });
    o.ready();
    pinned
}

/// A rename of `format_total` in helpers.php planned; how often the server was asked.
fn rename_asks(o: &mut Opened, source: &str) -> (refactor::Plan, usize) {
    let at = source.find("format_total").unwrap() + 2;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, at, false, false));
    let context = o.wb.rename_context().unwrap();
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    let seen = o.seen.lock().unwrap();
    (plan, seen.count("textDocument/rename") + seen.count("textDocument/prepareRename"))
}

const RENAMED: [(&str, &str); 2] = [
    ("helpers.php", "<?php\nfunction format_total(int $n): string { return (string) $n; }\n"),
    ("index.php", "<?php\nrequire 'helpers.php';\necho format_total(3);\n"),
];

#[test]
fn intelephense_without_a_licence_leaves_rename_to_the_built_in_engine() {
    // Without a licence key it has no rename (a premium feature).
    let behaviour = Behaviour { without: vec!["renameProvider".into()], ..Behaviour::default() };
    let mut o = opened_choosing_php("intelephense-rename", &RENAMED, "helpers.php", behaviour);
    let _pinned = choose_intelephense(&mut o);
    let (plan, asked) = rename_asks(&mut o, RENAMED[0].1);
    assert!(plan.files.iter().any(|f| f.path.ends_with("index.php")), "{plan:?}");
    assert_eq!(asked, 0);
}

#[test]
fn intelephense_with_a_licence_is_asked_for_renames() {
    let mut o =
        opened_choosing_php("intelephense-licensed", &RENAMED, "helpers.php", Behaviour::default());
    let _pinned = choose_intelephense(&mut o);
    let (_, asked) = rename_asks(&mut o, RENAMED[0].1);
    assert!(asked > 0);
}

// ─── Find Usages: the server's references (plan 46a) ───

/// PHP answered by `server` (chosen), started; PHP's default again once `o` is done with.
fn choose_php(o: &mut Opened, server: &str) -> language_servers::settings::Pinned {
    let mut chosen = language_servers::settings::config();
    chosen.engines = vec![(
        "php".into(),
        language_servers::settings::Choice {
            engine: code_intelligence::servers::Engine::Server,
            server: Some(server.into()),
        },
    )];
    // Pinned: another test loading the modules meanwhile applies the saved settings again.
    let pinned = language_servers::settings::pin(chosen);
    let project = o.wb.data.id;
    o.settle("the server started", |_, _| {
        language_servers::projects::project(project)
            .is_some_and(|p| p.servers(|s| s.iter().any(|r| r.spec.name == server)))
    });
    o.ready();
    pinned
}

/// Find Usages of `format_total` in helpers.php: its plan; how often the server was asked
/// for references and for a rename.
fn usages_asks(o: &mut Opened) -> (refactor::Plan, usize, usize) {
    let at = USED[0].1.find("format_total").unwrap() + 2;
    o.wb.edit(&mut o.text, |doc| commands::click(doc, at, false, false));
    let context = o.wb.usages_context().unwrap();
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    let seen = o.seen.lock().unwrap();
    let renames = seen.count("textDocument/rename") + seen.count("textDocument/prepareRename");
    (plan, seen.count("textDocument/references"), renames)
}

/// The files of `plan` (the template's place is offered, unsure: not chosen).
fn files_of(plan: &refactor::Plan) -> Vec<String> {
    let mut files: Vec<String> = plan
        .files
        .iter()
        .filter_map(|f| Some(f.path.file_name()?.to_string_lossy().into_owned()))
        .collect();
    files.sort();
    files
}

/// helpers.php (open, the server knows it), index.php (never opened: the fake server knows
/// only the documents it was given — what the built-in engine alone finds), a Twig template
/// calling it (another language's file: always the built-in engine's).
const USED: [(&str, &str); 3] = [
    (
        "helpers.php",
        "<?php\nfunction format_total(int $n): string { return (string) $n; }\necho format_total(1);\n",
    ),
    ("index.php", "<?php\nrequire 'helpers.php';\necho format_total(3);\n"),
    ("total.twig", "<p>{{ format_total(2) }}</p>\n"),
];

/// intelephense without a licence has no rename, but answers references: Find Usages is
/// its places in PHP files, the built-in engine's in the template; the rename isn't asked.
#[test]
fn find_usages_are_the_servers_references_without_its_rename() {
    let behaviour = Behaviour { without: vec!["renameProvider".into()], ..Behaviour::default() };
    let mut o = opened_choosing_php("usages-intelephense", &USED, "helpers.php", behaviour);
    let _pinned = choose_php(&mut o, "intelephense");
    let (plan, references, renames) = usages_asks(&mut o);
    assert!(references >= 1, "the server was asked");
    assert_eq!(renames, 0);
    // index.php: unknown to the server, and the server's word is the PHP files'.
    assert_eq!(files_of(&plan), ["helpers.php", "total.twig"], "{plan:?}");
    assert_eq!(plan.chosen(), (2, 1), "{plan:?}");
}

/// phpactor keeps the rename the built-in engine's, and its references join the built-in
/// engine's: what either finds (index.php, unknown to the server), each place once.
#[test]
fn find_usages_join_phpactors_references_and_the_built_in_engines() {
    let mut o = opened_choosing_php("usages-phpactor", &USED, "helpers.php", Behaviour::default());
    let _pinned = choose_php(&mut o, "phpactor");
    let (plan, references, renames) = usages_asks(&mut o);
    assert!(references >= 1, "the server was asked");
    assert_eq!(renames, 0, "the rename stays the built-in engine's");
    assert_eq!(files_of(&plan), ["helpers.php", "index.php", "total.twig"], "{plan:?}");
    assert_eq!(plan.chosen(), (3, 2), "each place once: {plan:?}");
}

/// The server failing or answering nothing: the built-in engine's usages (one never
/// answered ends as an error does, after the request's timeout).
#[test]
fn find_usages_the_server_cant_give_are_the_built_in_engines() {
    for (i, behaviour) in failing("textDocument/references").into_iter().take(2).enumerate() {
        let behaviour = Behaviour { without: vec!["renameProvider".into()], ..behaviour };
        let mut o =
            opened_choosing_php(&format!("usages-failing-{i}"), &USED, "helpers.php", behaviour);
        let _pinned = choose_php(&mut o, "intelephense");
        let (plan, _, _) = usages_asks(&mut o);
        assert_eq!(files_of(&plan), ["helpers.php", "index.php", "total.twig"], "case {i}");
    }
}

// ─── The premium features, ours and the servers' (plan 43j) ───

/// The rows of the navigation list open now (`None`: none open).
fn navigation_rows(wb: &Workbench) -> Option<usize> {
    match &wb.choice_purpose {
        ChoicePurpose::Navigate(rows) if wb.choice_menu.is_open() => Some(rows.len()),
        _ => None,
    }
}

/// "Go to Implementation", "Go to Type Declaration" and "Type Hierarchy" ask the server
/// first: its places listed, its type jumped to, its hierarchy shown.
#[test]
fn implementations_types_and_hierarchies_come_from_the_server_first() {
    let source = "function add() {}\nadd();\nadd();\n";
    let mut o = opened("served-navigation", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    o.wb.edit(&mut o.text, |doc| commands::click(doc, 10, false, false));
    assert!(o.wb.go_to_related(&mut o.text, Related::Implementations));
    o.settle("the server's implementations", |wb, _| navigation_rows(wb) == Some(2));
    o.wb.choice_menu.close();
    assert!(o.wb.go_to_related(&mut o.text, Related::TypeDefinition));
    let last = source.rfind("add").unwrap();
    o.settle("its type jumped to", |wb, _| {
        wb.tabs[wb.active].doc.selections().primary().head == last
    });
    o.wb.edit(&mut o.text, |doc| commands::click(doc, 10, false, false));
    // Its chain of answers (the type, its supertypes, its subtypes) waited for longer than
    // the app does: a loaded machine slows it past the grace.
    navigation::TEST_GRACE.set(Some(Duration::from_secs(4)));
    assert!(o.wb.go_to_related(&mut o.text, Related::Hierarchy));
    o.settle("its hierarchy", |wb, _| navigation_rows(wb).is_some());
    navigation::TEST_GRACE.set(None);
    let seen = o.seen.lock().unwrap();
    assert_eq!(seen.count("typeHierarchy/supertypes"), 2, "the type's, then Base's");
    assert_eq!(seen.count("typeHierarchy/subtypes"), 2, "the type's, then Child's");
}

/// A server failing, or not doing it at all: the built-in engine's places (the index), at
/// once when it isn't asked.
#[test]
fn implementations_the_server_cant_give_are_the_built_in_engines() {
    let shape = "<?php\nnamespace App;\ninterface Shape {}\n";
    let files = [
        ("Shape.php", shape),
        ("Circle.php", "<?php\nnamespace App;\nclass Circle implements Shape {}\n"),
        ("Square.php", "<?php\nnamespace App;\nclass Square implements Shape {}\n"),
    ];
    let failing =
        Behaviour { failing: vec!["textDocument/implementation".into()], ..Behaviour::default() };
    for (name, behaviour) in [
        ("implementations-failing", failing),
        (
            "implementations-none",
            Behaviour { without: vec!["implementationProvider".into()], ..Behaviour::default() },
        ),
    ] {
        let mut o = opened(name, &files, "Shape.php", behaviour);
        o.ready();
        o.settle("indexed", |wb, _| {
            let index = wb.data.symbols.read().unwrap();
            index.type_by_fqn("App\\Square").is_some() && index.type_by_fqn("App\\Circle").is_some()
        });
        let at = shape.find("Shape").unwrap() + 1;
        o.wb.edit(&mut o.text, |doc| commands::click(doc, at, false, false));
        assert!(o.wb.go_to_related(&mut o.text, Related::Implementations));
        o.settle("the built-in engine's list", |wb, _| navigation_rows(wb) == Some(2));
    }
}

/// The server's code actions join the list (after ours, each title once); one chosen is
/// applied as a refactoring; the bulb shows for them once the cursor rests.
#[test]
fn the_servers_code_actions_are_listed_and_applied() {
    let source = "let total = 1;\n";
    let mut o = opened("served-actions", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    o.wb.edit(&mut o.text, |doc| commands::click(doc, 5, false, false));
    o.wb.focus = Focus::Editor;
    assert!(o.wb.show_context_actions(&mut o.text));
    o.settle("the list with the server's actions", |wb, _| {
        matches!(&wb.choice_purpose, ChoicePurpose::Actions(rows)
            if rows.iter().filter(|r| matches!(r, context_actions::ContextAction::Served(_))).count() == 2)
    });
    let ChoicePurpose::Actions(rows) = &o.wb.choice_purpose else { unreachable!() };
    let first =
        rows.iter().position(|r| matches!(r, context_actions::ContextAction::Served(_))).unwrap();
    o.wb.choice_picked(first);
    o.settle("applied", |wb, _| wb.tabs[wb.active].doc.buffer().text() == "let TOTAL = 1;\n");
    // The bulb: the server's actions there (a refactoring: not a fix).
    o.wb.edit(&mut o.text, |doc| commands::click(doc, 6, false, false));
    o.settle("the bulb", |wb, _| wb.bulb().is_some_and(|b| !b.fix && b.line == 0));
}

/// The server's document links come first (asked once per version while ⌘ is held).
#[test]
fn the_servers_document_links_come_first() {
    let source = "// see https://example.com/a\n";
    let mut o = opened("served-links", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    let at = source.find("example").unwrap();
    // Ours meanwhile (the same URL).
    assert!(o.wb.document_link_at(at).is_some());
    o.settle("the server's links", |wb, _| wb.data.served.links_answer.is_some());
    assert_eq!(o.wb.document_link_at(at).map(|l| l.range), Some(7..source.len() - 1));
    assert_eq!(o.seen.lock().unwrap().count("textDocument/documentLink"), 1);
}

/// The server's inlay hints and code lenses come first where it has them (resolved
/// lenses too); without them, the built-in engine's.
#[test]
fn the_servers_hints_and_lenses_come_first() {
    let source = "function add(a) {}\nadd(1);\nfunction more() {}\n";
    let mut o = opened("served-insight", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    o.settle("the server's hints", |wb, _| {
        let inlays = wb.tabs[wb.active].view.inlays();
        inlays.iter().any(|i| i.label == "n:" && i.offset == source.find("1)").unwrap())
    });
    o.settle("its lenses, one resolved", |wb, _| {
        let lines = &wb.tabs[wb.active].insight.lines;
        lines.len() == 2 && lines[0].parts == ["2 references"] && lines[1].parts == ["resolved"]
    });
    let seen = o.seen.lock().unwrap();
    assert_eq!(seen.count("codeLens/resolve"), 1);
}

/// The status bar names what answers the file's language once one of its servers can, and
/// a click lists the engines to switch to (each server with its state, the built-in engine,
/// both).
#[test]
fn the_status_bar_names_the_engine_and_lists_the_others() {
    let mut o = opened("engine-switch", &[("a.ts", "let a = 1;\n")], "a.ts", Behaviour::default());
    o.ready();
    let tab = &o.wb.tabs[o.wb.active];
    assert_eq!(o.wb.engine_label(tab).as_deref(), Some("fake"));
    assert_eq!(o.wb.run_command(workbench_model::CHOOSE_ENGINE), Some(true));
    assert!(o.wb.choice_menu.is_open());
    assert!(matches!(o.wb.choice_purpose, ChoicePurpose::Contributed(_)));
}

// ─── The Structure panel from the server's declarations (plan 46d) ───

/// A language with no structure of the built-in engine's (C, its syntax only): the
/// Structure panel's declarations are the server's, asked once per version, the last ones
/// kept meanwhile.
#[test]
fn a_language_without_our_structure_gets_the_servers() {
    let source = "/* function area */\nint area(int w, int h) { return w * h; }\n";
    let mut o = opened("outline", &[("shapes.c", source)], "shapes.c", Behaviour::default());
    o.ready();
    o.settle("the server's declarations", |wb, _| {
        wb.served_outline().is_some_and(|f| f.symbols.iter().any(|s| s.name == "area".into()))
    });
    for _ in 0..10 {
        frame(&mut o.wb, &mut o.text);
        o.wb.served_outline();
    }
    assert_eq!(o.seen.lock().unwrap().count("textDocument/documentSymbol"), 1, "once a version");
    // A new version: asked again, the old ones shown until it answers.
    o.cursor_at_end();
    o.type_text("/* function volume */\n");
    assert!(o.wb.served_outline().is_some(), "the last ones kept");
    o.settle("asked again", |wb, s| {
        wb.served_outline();
        s.count("textDocument/documentSymbol") == 2
    });
    o.settle("the new ones", |wb, _| {
        wb.served_outline().is_some_and(|f| f.symbols.iter().any(|s| s.name == "volume".into()))
    });
}

/// Where the built-in engine has a structure (TypeScript's), the server isn't asked.
#[test]
fn our_structure_stays_where_we_have_one() {
    let source = "function total() {}\nclass Cart {}\n";
    let mut o = opened("outline-ours", &[("a.ts", source)], "a.ts", Behaviour::default());
    o.ready();
    for _ in 0..30 {
        frame(&mut o.wb, &mut o.text);
        assert!(o.wb.served_outline().is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(o.seen.lock().unwrap().count("textDocument/documentSymbol"), 0);
}

/// A test's chosen engine (pinned) outlives the saved settings applied again when other
/// tests load the modules side by side — what once undid a PHP test's server mid-test
/// (plan 55d).
#[test]
fn a_pinned_engine_outlives_modules_loaded_meanwhile() {
    let _alone = PHP.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut chosen = language_servers::settings::config();
    let choice = language_servers::settings::Choice {
        engine: code_intelligence::servers::Engine::Server,
        server: Some("phpactor".into()),
    };
    chosen.engines = vec![("php".into(), choice)];
    let before = language_servers::settings::config().engines;
    {
        let _pinned = language_servers::settings::pin(chosen.clone());
        let _ = crate::modules::load(&extensions::ModuleSelection::default());
        assert_eq!(language_servers::settings::config().engines, chosen.engines);
    }
    assert_eq!(language_servers::settings::config().engines, before, "given back");
}
