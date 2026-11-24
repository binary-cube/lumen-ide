//! The client against the fake server (in memory): its lifecycle, the documents kept in
//! step, every question, cancellation, timeouts, crashes and restarts, a server that hangs.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use code_intelligence::servers::Polled;
use document::TextBuffer;
use kernel::DocumentId;
use serde_json::json;

use super::*;
use crate::fake::{self, Behaviour, Seen};

const ROOT: &str = "/project";

struct Harness {
    client: Client,
    seen: Arc<Mutex<Seen>>,
    launches: Arc<Mutex<usize>>,
    events: Arc<Mutex<Vec<Event>>>,
}

fn config() -> Config {
    Config {
        name: "fake".into(),
        root: PathBuf::from(ROOT),
        project_root: PathBuf::from(ROOT),
        initialization_options: json!({"preferences": {"quote": "single"}}),
        settings: json!({"fake": {"strict": true}}),
        contents: Vec::new(),
        completion_kinds: Default::default(),
        on_initialized: Vec::new(),
        debounce: Duration::from_millis(5),
        timeouts: Timeouts {
            start: Duration::from_secs(5),
            request: Duration::from_secs(5),
            shutdown: Duration::from_millis(500),
        },
        restart: RestartPolicy {
            crashes: 3,
            window: Duration::from_secs(180),
            pause: Duration::from_millis(20),
            cooldown: Duration::from_millis(300),
        },
        idle: None,
        trace: Arc::new(AtomicBool::new(true)),
        secret_options: Vec::new(),
        secrets: None,
        show_messages: false,
        wake: Arc::new(|| {}),
    }
}

fn start_with(config: Config, behaviour: impl Fn(usize) -> Behaviour + Send + 'static) -> Harness {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let launches = Arc::new(Mutex::new(0));
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let heard = Arc::clone(&events);
    let launcher = fake::launcher(Arc::clone(&seen), Arc::clone(&launches), behaviour);
    let client =
        Client::start(config, Box::new(launcher), Arc::new(move |e| heard.lock().unwrap().push(e)))
            .unwrap();
    Harness { client, seen, launches, events }
}

fn start(behaviour: Behaviour) -> Harness {
    start_with(config(), move |_| behaviour.clone())
}

/// Waits (up to 5 s) until `done`.
fn wait(what: &str, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A reply's answer, waited for.
fn answer<T>(reply: Reply<T>) -> T {
    reply.wait(Duration::from_secs(5)).expect("an answer")
}

impl Harness {
    fn ready(&self) {
        wait("ready", || matches!(self.client.status().state(), State::Ready | State::Idle));
    }

    fn seen<R>(&self, f: impl FnOnce(&Seen) -> R) -> R {
        f(&self.seen.lock().unwrap())
    }

    /// Opens `text` as `name` (in the project); its document's id.
    fn open(&self, name: &str, text: &str) -> (DocumentId, PathBuf) {
        let doc = DocumentId::next();
        let path = Path::new(ROOT).join(name);
        self.client.open(
            doc,
            path.clone(),
            "typescript".into(),
            1,
            TextBuffer::new(text).snapshot(),
        );
        let uri = uri::from_path(&path);
        wait("the document opened", || self.seen(|s| s.documents.contains_key(&uri)));
        (doc, path)
    }

    fn server_text(&self, path: &Path) -> Option<String> {
        self.seen(|s| s.documents.get(&uri::from_path(path)).cloned())
    }

    fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }
}

#[test]
fn it_starts_initializes_and_is_configured() {
    let h = start(Behaviour::default());
    h.ready();
    let init = h.seen(|s| s.initialize.clone());
    assert_eq!(init["rootUri"], "file:///project");
    assert_eq!(init["workspaceFolders"][0]["name"], "project");
    assert_eq!(init["initializationOptions"]["preferences"]["quote"], "single");
    assert_eq!(
        init["capabilities"]["textDocument"]["completion"]["completionItem"]["snippetSupport"],
        true
    );
    assert_eq!(init["capabilities"]["general"]["positionEncodings"][1], "utf-16");
    wait("configured", || h.seen(|s| s.count("workspace/didChangeConfiguration") == 1));
    assert_eq!(h.seen(|s| s.settings.clone()), json!({"fake": {"strict": true}}));
    // The server asked for its section (and one nobody set).
    wait("asked", || h.seen(|s| !s.configuration.is_null()));
    assert_eq!(h.seen(|s| s.configuration.clone()), json!([{"strict": true}, null]));
    assert_eq!(h.seen(|s| s.methods[..2].to_vec()), ["initialize", "initialized"]);
    let caps = h.client.status().capabilities();
    assert!(caps.completion && caps.resolve && caps.prepare_rename);
    assert_eq!(caps.completion_triggers, ".");
    assert_eq!(caps.signature_triggers, "(,");
    // No document: idle.
    assert_eq!(h.client.status().state(), State::Idle);
}

#[test]
fn stopping_shuts_it_down_and_ends() {
    let h = start(Behaviour::default());
    h.ready();
    h.client.stop();
    wait("ended", || h.client.ended());
    assert!(h.seen(|s| s.count("shutdown") == 1 && s.count("exit") == 1));
    assert!(h.events().contains(&Event::Ended));
    assert_eq!(h.client.status().state(), State::Stopped { crashed: false });
}

#[test]
fn dropping_the_client_stops_the_server() {
    let h = start(Behaviour::default());
    h.ready();
    let seen = Arc::clone(&h.seen);
    drop(h.client);
    wait("shut down", || seen.lock().unwrap().count("exit") == 1);
}

#[test]
fn documents_are_kept_in_step_with_one_change_each() {
    let h = start(Behaviour::default());
    h.ready();
    let (doc, path) = h.open("a.ts", "const 😀 = 1;\nlet ț = 2;\n");
    assert_eq!(h.client.status().state(), State::Ready);
    assert_eq!(h.client.status().snapshot(Instant::now()).documents, 1);
    let mut buffer = TextBuffer::new("const 😀 = 1;\nlet ț = 2;\n");
    let edits: [(usize, usize, &str); 6] = [
        (0, 0, "// 🎉 start\n"),
        (17, 17, "x"),
        (5, 6, ""),
        (0, 3, "ăîș"),
        (10, 25, "\n\n"),
        (0, 0, ""),
    ];
    for (i, (start, end, text)) in edits.into_iter().enumerate() {
        let len = buffer.len();
        let start = buffer.floor_char_boundary(start.min(len));
        let end = buffer.floor_char_boundary(end.min(len)).max(start);
        buffer.replace(start..end, text);
        h.client.change(doc, i as u64 + 2, buffer.snapshot());
    }
    let expected = buffer.text();
    wait("in step", || h.server_text(&path).as_deref() == Some(expected.as_str()));
    // Gathered: fewer changes sent than made.
    assert!(h.seen(|s| s.changes) <= 6);
}

#[test]
fn a_utf8_server_and_a_whole_document_server_stay_in_step_too() {
    for behaviour in [
        Behaviour { utf8: true, ..Behaviour::default() },
        Behaviour { full_sync: true, ..Behaviour::default() },
    ] {
        let h = start(behaviour);
        h.ready();
        let (doc, path) = h.open("a.ts", "é😀\nx");
        let mut buffer = TextBuffer::new("é😀\nx");
        buffer.replace(2..2, "ț");
        h.client.change(doc, 2, buffer.snapshot());
        buffer.replace(buffer.len()..buffer.len(), "\n🎉");
        h.client.change(doc, 3, buffer.snapshot());
        let expected = buffer.text();
        wait("in step", || h.server_text(&path).as_deref() == Some(expected.as_str()));
        // Positions in its counting: the hover's word after the emoji.
        let at = expected.find('x').unwrap();
        let hover = answer(h.client.hover(doc, at, at..at + 1)).unwrap();
        assert!(hover.code.contains("const x"), "{hover:?}");
        assert_eq!(&expected[hover.range.clone()], "x");
    }
}

#[test]
fn a_question_comes_after_the_changes_before_it() {
    let h = start(Behaviour { delay: Duration::ZERO, ..Behaviour::default() });
    h.ready();
    let (doc, _) = h.open("a.ts", "console");
    let text = "console.";
    h.client.change(doc, 2, TextBuffer::new(text).snapshot());
    // Asked at once (before the change's pause is over): the change is sent first.
    let served = answer(h.client.completion(doc, text.len()));
    let labels: Vec<&str> = served.items.iter().map(|i| i.item.label.as_str()).collect();
    assert_eq!(labels, ["log", "error", "warn", "table"]);
    assert_eq!(served.items[0].item.insert, "log()");
    assert_eq!(served.items[0].item.cursor_back, 1);
    assert_eq!(served.items[0].item.replace, Some(text.len()..text.len()));
}

#[test]
fn completion_resolves_an_items_import() {
    let h = start(Behaviour::default());
    h.ready();
    let (doc, _) = h.open("a.ts", "const total = 1;\nre");
    let served = answer(h.client.completion(doc, "const total = 1;\nre".len()));
    let read = served.items.iter().find(|i| i.item.label == "readFile").unwrap().item.clone();
    assert!(read.resolve.is_some() && read.edits.is_empty());
    assert!(served.items.iter().any(|i| i.item.label == "total"));
    let resolved = answer(h.client.resolve(doc, read));
    assert_eq!(resolved.edits, vec![(0..0, "import { readFile } from 'fs';\n".to_owned())]);
    assert_eq!(resolved.detail.as_deref(), Some("function readFile(path: string): string"));
    assert_eq!(resolved.resolve, None);
}

#[test]
fn every_question_is_answered_in_bytes() {
    let h = start(Behaviour::default());
    h.ready();
    let text = "function add(a, b) {}\nconst 😀total = add(1, 2);\nadd(total, 3);\n";
    let (doc, path) = h.open("a.ts", text);
    let (_, other) = h.open("b.ts", "add(4, 5);\n");
    let add = text.rfind("add").unwrap() + 1;
    let hover = answer(h.client.hover(doc, add, 0..0)).unwrap();
    assert_eq!(hover.code, "const add: number");
    assert_eq!(hover.doc.as_deref(), Some("The value of add."));
    let inside = text.find("1, 2").unwrap() + 3;
    let help = answer(h.client.signature_help(doc, inside)).unwrap();
    assert_eq!(help.code, "add(a: number, b: string): void");
    assert_eq!(help.active, Some(1));
    let definition = answer(h.client.definition(doc, add));
    assert_eq!(definition, vec![ServedLocation { path, range: 9..12, line: 0 }]);
    let references = answer(h.client.references(doc, add));
    assert_eq!(references.len(), 4);
    assert!(references.iter().any(|r| r.path == other));
    let highlights = answer(h.client.highlights(doc, add));
    assert_eq!(highlights.len(), 3);
    assert!(highlights.iter().all(|r| &text[r.clone()] == "add"));
    let symbols = answer(h.client.symbols(doc));
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "add".into());
    let places = answer(h.client.rename_places(doc, add)).unwrap();
    assert_eq!(places.len(), 4);
    let change = answer(h.client.rename(doc, add, "sum".into())).unwrap();
    assert_eq!(change.files.len(), 2);
    assert!(
        change.files.iter().flat_map(|f| &f.edits).all(|e| e.original == "add" && e.text == "sum")
    );
    // Nothing to rename on a brace.
    assert!(answer(h.client.rename_places(doc, text.find("{}").unwrap() + 1)).is_err());
}

/// The navigation the servers sell (implementations, the type, the hierarchy) and what
/// they show in the code (hints, lenses, links), in bytes of the document.
#[test]
fn implementations_types_hierarchies_hints_lenses_and_links() {
    let h = start(Behaviour::default());
    h.ready();
    let text =
        "function add(a) {}\nadd(1);\nfunction more() {}\n// https://example.com/x\nadd(2);\n";
    let (doc, path) = h.open("a.ts", text);
    let add = text.find("add").unwrap() + 1;
    let found = answer(h.client.implementations(doc, add));
    assert_eq!(found.len(), 2, "every later `add`");
    assert!(found.iter().all(|l| l.path == path && &text[l.range.clone()] == "add"));
    let ty = answer(h.client.type_definition(doc, add));
    assert_eq!(ty[0].range.start, text.rfind("add").unwrap());
    let item = answer(h.client.prepare_type_hierarchy(doc, add)).expect("a type there");
    assert_eq!((item.place.name.as_str(), item.place.fqn.as_str()), ("add", "Fake\\add"));
    assert_eq!(item.place.path.as_deref(), Some(path.as_path()));
    let up = answer(h.client.hierarchy_step(&item, true));
    assert_eq!(up[0].place.name, "Base");
    assert!(answer(h.client.hierarchy_step(&up[0], true)).is_empty());
    let down = answer(h.client.hierarchy_step(&item, false));
    assert_eq!(down[0].place.name, "Child");
    let hints = answer(h.client.inlay_hints(doc, 0..text.len()));
    let at: Vec<usize> = hints.iter().map(|h| h.offset).collect();
    assert_eq!(at, [text.find("1)").unwrap(), text.find("2)").unwrap()]);
    assert!(hints.iter().all(|h| h.label == "n:"));
    // Only the range asked about.
    assert_eq!(answer(h.client.inlay_hints(doc, 0..text.find("more").unwrap())).len(), 1);
    let lenses = answer(h.client.code_lens(doc));
    assert_eq!(lenses.len(), 2);
    assert_eq!(lenses[0].lens.as_ref().map(|l| l.title.as_str()), Some("2 references"));
    assert!(lenses[1].lens.is_none(), "the second one to resolve");
    let resolved = answer(h.client.resolve_lens(&lenses[1])).unwrap();
    assert_eq!(
        (resolved.title.as_str(), resolved.offset),
        ("resolved", text.find("function more").unwrap())
    );
    let links = answer(h.client.document_links(doc));
    assert_eq!(links.len(), 1);
    assert_eq!(&text[links[0].range.clone()], "https://example.com/x");
    assert_eq!(
        links[0].target,
        code_intelligence::links::LinkTarget::Url("https://example.com/x".into())
    );
}

/// A server that doesn't say it does them isn't asked: the replies are gone at once.
#[test]
fn what_a_server_doesnt_offer_isnt_asked() {
    let without = [
        "implementationProvider",
        "typeHierarchyProvider",
        "inlayHintProvider",
        "codeLensProvider",
    ];
    let h = start(Behaviour {
        without: without.iter().map(|s| (*s).into()).collect(),
        ..Behaviour::default()
    });
    h.ready();
    let (doc, _) = h.open("a.ts", "add(1);\n");
    assert!(h.client.implementations(doc, 1).wait(Duration::from_secs(2)).is_none());
    assert!(h.client.prepare_type_hierarchy(doc, 1).wait(Duration::from_secs(2)).is_none());
    assert!(h.client.inlay_hints(doc, 0..8).wait(Duration::from_secs(2)).is_none());
    assert!(h.client.code_lens(doc).wait(Duration::from_secs(2)).is_none());
    let caps = h.client.status().capabilities();
    assert!(caps.type_definition && caps.document_links && !caps.code_lens && !caps.inlay_hints);
    h.seen(|s| assert_eq!(s.count("textDocument/implementation"), 0));
}

#[test]
fn formatting_and_code_actions_change_the_files() {
    let h = start(Behaviour::default());
    h.ready();
    let text = "let total = 1;   \nfoo();\n";
    let (doc, path) = h.open("a.ts", text);
    let options = FormatOptions { tab_size: 2, insert_spaces: true };
    let change = answer(h.client.format(doc, None, options)).unwrap();
    assert_eq!(change.files[0].path, path);
    assert_eq!(change.files[0].edits[0].range, 14..17);
    let actions = answer(h.client.code_actions(doc, 4..4));
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].title, "Uppercase 'total'");
    let change = answer(h.client.run_action(actions[0].id)).unwrap();
    assert_eq!(change.files[0].edits[0].text, "TOTAL");
    // A command: the server runs it and asks for its edit.
    let change = answer(h.client.run_action(actions[1].id)).unwrap();
    assert!(change.is_empty());
    wait("asked to apply", || h.events().iter().any(|e| matches!(e, Event::ApplyEdit { .. })));
    let applied = h.events().into_iter().find_map(|e| match e {
        Event::ApplyEdit { label, change } => Some((label, change)),
        _ => None,
    });
    let (label, change) = applied.unwrap();
    assert_eq!(label.as_deref(), Some("Run a command"));
    assert_eq!(change.files[0].edits[0].text, "// done\n");
    wait("answered", || h.seen(|s| s.applied == Some(true)));
    // An open document's diagnostics there go with the question (its quick fixes' context).
    let (fix, _) = h.open(
        "b.ts", "// TODO
",
    );
    wait("published", || {
        h.events()
            .iter()
            .any(|e| matches!(e, Event::Diagnostics { diagnostics, .. } if !diagnostics.is_empty()))
    });
    let _ = answer(h.client.code_actions(fix, 3..5));
    assert!(h.seen(|s| s.action_diagnostics.last().is_some_and(|n| *n > 0)), "sent back");
}

#[test]
fn a_server_that_doesnt_push_is_asked_for_its_diagnostics() {
    let h = start(Behaviour { pull: true, ..Behaviour::default() });
    h.ready();
    let (doc, _) = h.open("a.ts", "// TODO\n");
    let last = || {
        h.events().into_iter().rev().find_map(|e| match e {
            Event::Diagnostics { diagnostics, .. } => Some(diagnostics.len()),
            _ => None,
        })
    };
    wait("asked", || last() == Some(1));
    h.client.change(doc, 2, TextBuffer::new("// nothing left\n").snapshot());
    wait("asked again", || last() == Some(0));
}

#[test]
fn diagnostics_are_published_relative_to_the_project() {
    let h = start(Behaviour::default());
    h.ready();
    h.open("src/a.ts", "// TODO: 😀 FIXME\n");
    wait("diagnostics", || {
        h.events()
            .iter()
            .any(|e| matches!(e, Event::Diagnostics { diagnostics, .. } if !diagnostics.is_empty()))
    });
    let (path, diagnostics) = h
        .events()
        .into_iter()
        .find_map(|e| match e {
            Event::Diagnostics { path, diagnostics } if !diagnostics.is_empty() => {
                Some((path, diagnostics))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(path, Path::new("src/a.ts"));
    assert_eq!(diagnostics.len(), 2);
    assert_eq!((diagnostics[0].column, diagnostics[0].end), (Some(3), Some((0, 7))));
    // After the emoji: characters, not units.
    assert_eq!(diagnostics[1].column, Some(11));
    assert_eq!(diagnostics[1].severity, workbench_model::Severity::Error);
}

#[test]
fn a_newer_question_cancels_the_older_one() {
    let h = start(Behaviour { delay: Duration::from_millis(60), ..Behaviour::default() });
    h.ready();
    let (doc, _) = h.open("a.ts", "console.");
    let mut first = h.client.completion(doc, 8);
    let second = h.client.completion(doc, 8);
    assert!(!answer(second).items.is_empty());
    assert!(matches!(first.poll(), Polled::Gone));
    wait("cancelled", || h.seen(|s| s.cancelled.len() == 1));
}

#[test]
fn a_reply_dropped_unanswered_cancels_its_request() {
    let h = start(Behaviour { delay: Duration::from_millis(80), ..Behaviour::default() });
    h.ready();
    let (doc, _) = h.open("a.ts", "total");
    drop(h.client.hover(doc, 1, 0..0));
    wait("cancelled", || h.seen(|s| s.cancelled.len() == 1));
}

#[test]
fn a_question_not_answered_in_time_is_dropped_and_said() {
    let mut config = config();
    config.timeouts.request = Duration::from_millis(30);
    let h = start_with(config, |_| Behaviour {
        delay: Duration::from_millis(300),
        ..Behaviour::default()
    });
    h.ready();
    let (doc, _) = h.open("a.ts", "total");
    let reply = Mutex::new(h.client.hover(doc, 1, 0..0));
    wait("gone", || matches!(reply.lock().unwrap().poll(), Polled::Gone));
    assert_eq!(
        h.client.status().snapshot(Instant::now()).last_error.as_deref(),
        Some("textDocument/hover timed out")
    );
}

#[test]
fn a_crash_restarts_it_with_its_documents() {
    // The first instance crashes on a hover; the next ones don't.
    let h = start_with(config(), |n| Behaviour {
        crash_on: (n == 0).then(|| "textDocument/hover".into()),
        ..Behaviour::default()
    });
    h.ready();
    let (doc, path) = h.open("a.ts", "TODO total");
    let reply = h.client.hover(doc, 6, 0..0);
    assert!(reply.wait(Duration::from_secs(5)).is_none());
    wait("restarted", || *h.launches.lock().unwrap() == 2);
    h.ready();
    // Its document is opened again, and its diagnostics were cleared meanwhile.
    wait("reopened", || h.server_text(&path).as_deref() == Some("TODO total"));
    assert!(h.events().contains(&Event::DiagnosticsCleared));
    assert_eq!(h.client.status().snapshot(Instant::now()).crashes, 1);
    assert!(answer(h.client.hover(doc, 6, 0..0)).is_some());
}

#[test]
fn three_crashes_in_a_row_rest_it_then_it_is_tried_again() {
    // It dies right after starting (on `initialized`), three times; then it works.
    let h = start_with(config(), |n| Behaviour {
        crash_on: (n < 3).then(|| "initialized".into()),
        ..Behaviour::default()
    });
    wait("stopped", || h.client.status().state() == State::Stopped { crashed: true });
    assert_eq!(*h.launches.lock().unwrap(), 3);
    let error = h.client.status().snapshot(Instant::now()).last_error.unwrap();
    assert!(error.contains("crashed 3 times") && error.contains("tried again"), "{error}");
    // It rests (not started again at once)…
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(*h.launches.lock().unwrap(), 3);
    // …then it's tried again by itself: never given up for good.
    h.ready();
    assert_eq!(*h.launches.lock().unwrap(), 4);
}

#[test]
fn a_crashed_server_rested_is_restarted_by_hand_at_once() {
    let mut cfg = config();
    cfg.restart.cooldown = Duration::from_secs(60);
    let h = start_with(cfg, |n| Behaviour {
        crash_on: (n < 3).then(|| "initialized".into()),
        ..Behaviour::default()
    });
    wait("stopped", || h.client.status().state() == State::Stopped { crashed: true });
    h.client.restart();
    h.ready();
    assert_eq!(*h.launches.lock().unwrap(), 4);
}

#[test]
fn a_server_that_cant_start_says_why() {
    let launcher =
        crate::launch::FnLauncher(Box::new(|| Err(std::io::Error::other("no such program"))));
    let client = Client::start(config(), Box::new(launcher), Arc::new(|_| {})).unwrap();
    wait("failed", || matches!(client.status().state(), State::Failed(_)));
    assert!(
        client.status().snapshot(Instant::now()).last_error.unwrap().contains("no such program")
    );
}

#[test]
fn typing_never_waits_for_a_hung_server() {
    let h = start(Behaviour { hang: true, ..Behaviour::default() });
    h.ready();
    let doc = DocumentId::next();
    let big = "x".repeat(256 * 1024);
    let started = Instant::now();
    h.client.open(
        doc,
        Path::new(ROOT).join("a.ts"),
        "typescript".into(),
        1,
        TextBuffer::new(&big).snapshot(),
    );
    let mut buffer = TextBuffer::new(&big);
    let mut replies = Vec::new();
    for version in 2..200 {
        buffer.replace(0..0, "y");
        h.client.change(doc, version, buffer.snapshot());
        replies.push(h.client.completion(doc, 1));
    }
    // 200 changes of a 256 KB text and 200 questions, the server reading nothing: none waited.
    assert!(started.elapsed() < Duration::from_millis(500), "{:?}", started.elapsed());
    assert!(replies.iter_mut().all(|r| matches!(r.poll(), Polled::Waiting | Polled::Gone)));
}

#[test]
fn unused_for_long_it_stops_by_itself() {
    let mut config = config();
    config.idle = Some(Duration::from_millis(50));
    let h = start_with(config, |_| Behaviour::default());
    h.ready();
    let (doc, _) = h.open("a.ts", "x");
    std::thread::sleep(Duration::from_millis(120));
    // A document open: it stays.
    assert!(!h.client.ended());
    h.client.close(doc);
    wait("ended", || h.client.ended());
    assert!(h.seen(|s| s.count("textDocument/didClose") == 1 && s.count("shutdown") == 1));
}

#[test]
fn watched_files_and_saves_reach_it() {
    let h = start(Behaviour::default());
    h.ready();
    let (_, path) = h.open("a.ts", "x");
    h.client.watched(vec![(Path::new(ROOT).join("tsconfig.json"), FileEvent::Changed)]);
    h.client.saved(path);
    wait("heard", || h.seen(|s| s.count("textDocument/didSave") == 1 && !s.watched.is_empty()));
    assert_eq!(h.seen(|s| s.watched.clone()), ["file:///project/tsconfig.json"]);
}

#[test]
fn closing_a_document_closes_it_on_the_server() {
    let h = start(Behaviour::default());
    h.ready();
    let (doc, path) = h.open("a.ts", "x");
    h.client.close(doc);
    wait("closed", || h.server_text(&path).is_none());
    assert_eq!(h.client.status().state(), State::Idle);
}

#[test]
fn its_progress_and_traffic_are_shown() {
    let h = start(Behaviour { progress: true, ..Behaviour::default() });
    h.ready();
    wait("progress", || h.client.status().snapshot(Instant::now()).progress.is_some());
    assert_eq!(
        h.client.status().snapshot(Instant::now()).progress,
        Some(("Indexing".into(), Some(40)))
    );
    let (log, _) = h.client.status().log_lines();
    assert!(log.iter().any(|l| l.starts_with("→ ") && l.contains("initialize")));
    assert!(log.iter().any(|l| l.starts_with("← ")));
}

#[test]
fn requests_are_counted_with_their_latency() {
    let h = start(Behaviour::default());
    h.ready();
    let (doc, _) = h.open("a.ts", "total");
    for _ in 0..5 {
        answer(h.client.hover(doc, 1, 0..0));
    }
    let snapshot = h.client.status().snapshot(Instant::now());
    assert_eq!(snapshot.per_minute, 5);
    assert!(snapshot.p50.is_some() && snapshot.p95 >= snapshot.p50);
}

#[test]
fn settings_given_later_reach_it() {
    let h = start(Behaviour::default());
    h.ready();
    h.client.configure(json!({"fake": {"strict": false}}));
    wait("configured", || h.seen(|s| s.settings == json!({"fake": {"strict": false}})));
}

#[test]
fn sections_of_the_settings() {
    let settings = json!({"typescript": {"preferences": {"quote": "single"}}});
    assert_eq!(
        super::section(&settings, Some("typescript.preferences")),
        json!({"quote": "single"})
    );
    assert_eq!(super::section(&settings, Some("missing")), json!(null));
    assert_eq!(super::section(&settings, None), settings);
}

#[test]
fn contents_are_answered_from_their_folders_never_downloaded() {
    let dir = std::env::temp_dir().join(format!("lsp-contents-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("store")).unwrap();
    std::fs::write(dir.join("store/package.json"), r#"{"type": "object"}"#).unwrap();
    std::fs::write(dir.join("secret.json"), "{}").unwrap();
    let mut config = config();
    config.contents = vec![("https://schemas.test/".into(), dir.join("store"))];
    let bundled = "https://schemas.test/package.json#/definitions/x".to_owned();
    let missing = "https://schemas.test/missing.json".to_owned();
    let outside = "https://schemas.test/../secret.json".to_owned();
    let elsewhere = "https://example.com/schema.json".to_owned();
    let asked = vec![bundled.clone(), missing.clone(), outside.clone(), elsewhere.clone()];
    let h =
        start_with(config, move |_| Behaviour { contents: asked.clone(), ..Behaviour::default() });
    h.ready();
    wait("answered", || h.seen(|s| s.contents.len() == 4));
    let contents = h.seen(|s| s.contents.clone());
    assert_eq!(contents[&bundled], Ok(r#"{"type": "object"}"#.to_owned()));
    for url in [&missing, &outside, &elsewhere] {
        assert!(contents[url].as_ref().is_err_and(|e| e.contains("downloads are off")), "{url}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_notifications_it_waits_for_are_sent_once_it_is_initialized() {
    let mut config = config();
    config.on_initialized = vec!["yaml/registerContentRequest".into()];
    let h = start_with(config, |_| Behaviour::default());
    h.ready();
    wait("notified", || h.seen(|s| s.count("yaml/registerContentRequest") == 1));
    assert_eq!(
        h.seen(|s| s.methods[..3].to_vec()),
        ["initialize", "initialized", "yaml/registerContentRequest"]
    );
    // Again after a restart (a new instance knows nothing of the first one's).
    h.client.restart();
    wait("notified again", || h.seen(|s| s.count("yaml/registerContentRequest") == 2));
}

/// Secrets in memory (never the keychain).
#[derive(Default)]
struct Secrets(Mutex<std::collections::HashMap<String, String>>);

impl kernel::SecretStore for Secrets {
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

#[test]
fn a_secret_option_is_given_at_start_and_never_logged() {
    let secrets = Arc::new(Secrets::default());
    kernel::SecretStore::set(&*secrets, "fake-licence", "KEY-12345-SECRET").unwrap();
    let mut config = config();
    config.secret_options = vec![("licenceKey".into(), "fake-licence".into())];
    config.secrets = Some(Arc::clone(&secrets) as Arc<dyn kernel::SecretStore>);
    config.show_messages = true;
    let h = start_with(config, |_| Behaviour {
        // It echoes its options back (a licence's activation message): never logged as is.
        message: Some((3, "activated with {options}".into())),
        ..Behaviour::default()
    });
    h.ready();
    let init = h.seen(|s| s.initialize.clone());
    assert_eq!(init["initializationOptions"]["licenceKey"], "KEY-12345-SECRET");
    // The description's own options kept beside it.
    assert_eq!(init["initializationOptions"]["preferences"]["quote"], "single");
    wait("its message", || h.events().iter().any(|e| matches!(e, Event::Message { .. })));
    let (lines, _) = h.client.status().log_lines();
    let log = lines.join("\n");
    // The trace is on (the test's config): the initialize request and the message logged.
    assert!(log.contains("initialize"), "{log}");
    assert!(log.contains(crate::status::REDACTED), "{log}");
    assert!(!log.contains("KEY-12345-SECRET"), "{log}");
    let message = h.events().into_iter().find_map(|e| match e {
        Event::Message { kind, text } => Some((kind, text)),
        _ => None,
    });
    let (kind, text) = message.unwrap();
    assert_eq!(kind, MessageKind::Info);
    assert!(text.starts_with("activated with"));
}

#[test]
fn without_its_secret_nothing_is_added_and_messages_are_only_logged() {
    let mut config = config();
    config.secret_options = vec![("licenceKey".into(), "missing".into())];
    config.secrets = Some(Arc::new(Secrets::default()) as Arc<dyn kernel::SecretStore>);
    let h = start_with(config, |_| Behaviour {
        message: Some((1, "something failed".into())),
        ..Behaviour::default()
    });
    h.ready();
    let init = h.seen(|s| s.initialize.clone());
    assert!(init["initializationOptions"].get("licenceKey").is_none());
    wait("its message logged", || {
        h.client.status().log_lines().0.iter().any(|l| l.contains("something failed"))
    });
    assert!(!h.events().iter().any(|e| matches!(e, Event::Message { .. })));
}
