use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::CompletionKind;

fn served(label: &str, sort: &str) -> ServedItem {
    ServedItem {
        item: CompletionItem::word(label, CompletionKind::Function, None),
        filter: label.to_owned(),
        sort: sort.to_owned(),
    }
}

#[test]
fn engines_are_saved_by_id() {
    for engine in Engine::ALL {
        assert_eq!(Engine::from_id(engine.id()), Some(engine));
    }
    assert_eq!(Engine::from_id("lsp"), None);
    assert_eq!(Engine::default(), Engine::Server);
    assert!(Engine::Both.uses_server() && Engine::Both.uses_built_in());
    assert!(!Engine::BuiltIn.uses_server() && !Engine::Server.uses_built_in());
}

#[test]
fn a_reply_is_polled_until_answered() {
    let (tx, mut reply) = Reply::<u32>::pair();
    assert!(matches!(reply.poll(), Polled::Waiting));
    assert!(tx.send(7));
    assert!(matches!(reply.poll(), Polled::Ready(7)));
    // Taken: nothing more will come.
    assert!(matches!(reply.poll(), Polled::Gone));
}

#[test]
fn a_reply_whose_sender_is_gone_says_so() {
    let (tx, mut reply) = Reply::<u32>::pair();
    drop(tx);
    assert!(matches!(reply.poll(), Polled::Gone));
    assert!(Reply::<u32>::pair().1.wait(Duration::from_millis(1)).is_none());
}

#[test]
fn an_unanswered_reply_dropped_cancels_once() {
    let cancelled = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&cancelled);
    let (_tx, reply) = Reply::<u32>::pair();
    let reply = reply.on_cancel(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    drop(reply);
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
}

#[test]
fn an_answered_reply_dropped_cancels_nothing() {
    let cancelled = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&cancelled);
    let (tx, reply) = Reply::<u32>::pair();
    let mut reply = reply.on_cancel(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    tx.send(1);
    assert!(matches!(reply.poll(), Polled::Ready(1)));
    drop(reply);
    let ready = Reply::ready(2).on_cancel(|| panic!("never"));
    assert_eq!(ready.wait(Duration::from_secs(1)), Some(2));
    assert_eq!(cancelled.load(Ordering::SeqCst), 0);
}

#[test]
// A reply is waited for on another thread (a module's): this test makes one.
#[allow(clippy::disallowed_methods)]
fn a_reply_waited_for_on_another_thread() {
    let (tx, reply) = Reply::<String>::pair();
    let thread = std::thread::spawn(move || reply.wait(Duration::from_secs(5)));
    tx.send("done".into());
    assert_eq!(thread.join().ok().flatten().as_deref(), Some("done"));
}

#[test]
fn ranking_filters_by_the_prefix_then_the_servers_order() {
    let items = vec![served("toString", "1"), served("toFixed", "0"), served("valueOf", "2")];
    let ranked = rank(&items, "to");
    let labels: Vec<&str> = ranked.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["toFixed", "toString"]);
    assert!(ranked[0].score >= ranked[1].score);
    // No prefix: everything, in the server's order.
    let all: Vec<String> = rank(&items, "").into_iter().map(|i| i.label).collect();
    assert_eq!(all, ["toFixed", "toString", "valueOf"]);
}

#[test]
fn ranking_matches_what_the_server_filters_by() {
    let mut item = served("#private", "0");
    item.filter = "private".into();
    assert_eq!(rank(&[item], "pri").len(), 1);
}

#[test]
fn ranking_keeps_each_suggestion_once() {
    let items = vec![served("map", "0"), served("map", "1")];
    assert_eq!(rank(&items, "m").len(), 1);
}

#[test]
fn merged_completions_put_the_servers_first_without_duplicates() {
    let word = |l: &str| CompletionItem::word(l, CompletionKind::Keyword, None);
    let merged = merge_completions(
        vec![word("const"), word("console")],
        vec![word("const"), word("continue")],
    );
    let labels: Vec<&str> = merged.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["const", "console", "continue"]);
}

#[test]
fn an_empty_change_says_so() {
    assert!(WorkspaceChange::default().is_empty());
    let change = WorkspaceChange {
        files: vec![FileChange { path: "a.ts".into(), edits: Vec::new() }],
        renames: Vec::new(),
    };
    assert!(change.is_empty());
}

#[test]
fn server_data_registered_once() {
    let data = ServerData { module: "test.servers", toml: "[[language-servers]]" };
    register_data(data);
    register_data(data);
    assert_eq!(super::data().iter().filter(|d| **d == data).count(), 1);
}

#[test]
fn what_the_built_in_engine_keeps_is_read_by_name_and_joined() {
    let css = Kept::from_names(["deprecated", "rename", "unknown"]);
    assert_eq!(css, Kept { deprecated: true, rename: true, ..Kept::default() });
    let html = Kept::from_names(["diagnostics"]);
    assert_eq!(
        css.union(html),
        Kept { deprecated: true, rename: true, diagnostics: true, completion: false }
    );
    // PHP's: the union of suggestions too.
    let php = Kept::from_names(["completion", "diagnostics", "rename"]);
    assert!(php.completion && php.diagnostics && php.rename && !php.deprecated);
    assert_eq!(Kept::from_names([]), Kept::default());
    assert!(Kept::NAMES.iter().all(|n| Kept::from_names([*n]) != Kept::default()));
}

#[test]
fn a_servers_diagnostics_source_is_told_apart() {
    let source = diagnostics_source("vtsls");
    assert!(is_server_source(&source));
    assert!(!is_server_source("phpstan"));
}
