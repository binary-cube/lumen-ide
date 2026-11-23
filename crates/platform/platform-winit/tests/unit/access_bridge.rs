use super::*;
use kernel::Rect;
use platform_api::access::Described;

/// The app's tree as the system takes it: the window as root over the top nodes, each
/// node's children listed, its role, name, value, state and actions kept; the focus on the
/// focused node.
#[test]
fn the_tree_reaches_the_system_whole() {
    let mut frame = Frame::new();
    let dialog =
        frame.open(Described::new(Role::Dialog, "Settings", Rect::new(0.0, 0.0, 400.0, 300.0)));
    let switch = frame.add(
        Described::new(Role::Switch, "Auto-save", Rect::new(10.0, 10.0, 36.0, 20.0))
            .checked(true)
            .focused(true),
    );
    frame.close();
    let update = tree_update(&frame, "Window", None);
    assert_eq!(update.tree.as_ref().map(|t| t.root), Some(KitId(ROOT.0)));
    assert_eq!(update.focus, KitId(switch.0));
    let node = |id: NodeId| &update.nodes.iter().find(|(k, _)| k.0 == id.0).expect("a node").1;
    assert_eq!(node(ROOT).children(), [KitId(dialog.0)]);
    assert_eq!(node(dialog).children(), [KitId(switch.0)]);
    let s = node(switch);
    assert_eq!(s.role(), KitRole::Switch);
    assert_eq!(s.label(), Some("Auto-save"));
    assert_eq!(s.toggled(), Some(Toggled::True));
    assert!(s.supports_action(Action::Click));
}

/// What a screen reader asks comes back as the app's request; an action it doesn't do is
/// left.
#[test]
fn the_system_s_requests_come_back() {
    let asked = |action, data| accesskit::ActionRequest {
        action,
        target_node: KitId(7),
        data,
        target_tree: accesskit::TreeId::ROOT,
    };
    assert_eq!(request(&asked(Action::Click, None)), Some(Request::Press(NodeId(7))));
    let value = Some(accesskit::ActionData::Value("php".into()));
    assert_eq!(
        request(&asked(Action::SetValue, value)),
        Some(Request::SetValue(NodeId(7), "php".into()))
    );
    assert_eq!(request(&asked(Action::ScrollDown, None)), None);
    // A caret moved from the screen reader: each end a line's run and a character in it.
    let at = |run: u64, character| TextPosition { node: KitId(run), character_index: character };
    let selection = TextSelection {
        anchor: at(run_id(NodeId(7), 0).0, 2),
        focus: at(run_id(NodeId(7), 1).0, 1),
    };
    let asked_selection =
        asked(Action::SetTextSelection, Some(accesskit::ActionData::SetTextSelection(selection)));
    let place = |line, character| platform_api::access::TextPlace {
        run: platform_api::access::run_id(NodeId(7), line),
        character,
    };
    assert_eq!(
        request(&asked_selection),
        Some(Request::Select { field: NodeId(7), anchor: place(0, 2), focus: place(1, 1) })
    );
}

/// A field's text reaches the system as runs, a line each (its break counted as one
/// character), the selection in them; a list's length is on it, a row's place on the row.
#[test]
fn a_field_s_lines_and_a_list_s_places_reach_the_system() {
    use platform_api::access::TextLines;
    let mut frame = Frame::new();
    let lines = TextLines {
        lines: vec![Rect::new(0.0, 0.0, 100.0, 20.0), Rect::new(0.0, 20.0, 100.0, 20.0)],
        anchor: 1,
        // In "dé", after the "é" (two bytes): the second run's end.
        focus: 6,
        caret: None,
    };
    let field = frame.add(
        Described::new(Role::TextArea, "Code", Rect::new(0.0, 0.0, 100.0, 40.0))
            .value("ab\ndé")
            .lines(lines),
    );
    let list = frame.open(Described::new(Role::List, "", Rect::default()).count(12));
    let item = frame.add(Described::new(Role::ListItem, "x", Rect::default()).index(4));
    frame.close();
    let update = tree_update(&frame, "Window", None);
    let node = |id: KitId| &update.nodes.iter().find(|(k, _)| *k == id).expect("a node").1;
    let runs = node(KitId(field.0)).children().to_vec();
    assert_eq!(runs.len(), 2);
    assert_eq!(node(runs[0]).value(), Some("ab\n"));
    assert_eq!(node(runs[0]).character_lengths(), [1, 1, 1]);
    assert_eq!(node(runs[1]).character_lengths(), [1, 2]);
    let selection = node(KitId(field.0)).text_selection().expect("the selection");
    assert_eq!((selection.anchor.node, selection.anchor.character_index), (runs[0], 1));
    assert_eq!((selection.focus.node, selection.focus.character_index), (runs[1], 2));
    assert_eq!(node(KitId(list.0)).size_of_set(), Some(12));
    assert_eq!(node(KitId(item.0)).position_in_set(), Some(4));
}

/// Measured (not a check): the whole tree of a busy screen made into AccessKit's nodes.
#[test]
#[ignore = "a measurement: cargo test -p platform-winit --release measure_tree -- --ignored --nocapture"]
fn measure_tree() {
    let mut frame = Frame::new();
    let table =
        frame.open(Described::new(Role::Table, "", Rect::new(0.0, 0.0, 900.0, 900.0)).count(600));
    for r in 0..600 {
        let row = Rect::new(0.0, r as f32 * 20.0, 900.0, 20.0);
        frame.open(Described::new(Role::Row, "", row).index(r));
        for c in 0..4 {
            frame.add(Described::new(Role::Label, format!("cell {r}:{c}"), row));
        }
        frame.close();
    }
    let _ = table;
    frame.close();
    let started = std::time::Instant::now();
    let runs = 50;
    for _ in 0..runs {
        std::hint::black_box(tree_update(&frame, "Window", None));
    }
    let whole = started.elapsed() / runs;
    // The same screen, one cell's text changed (a frame of a busy screen, most often).
    let mut next = Frame::new();
    next.open(Described::new(Role::Table, "", Rect::new(0.0, 0.0, 900.0, 900.0)).count(600));
    for r in 0..600 {
        let row = Rect::new(0.0, r as f32 * 20.0, 900.0, 20.0);
        next.open(Described::new(Role::Row, "", row).index(r));
        for c in 0..4 {
            let name =
                if r == 7 && c == 2 { "changed".to_owned() } else { format!("cell {r}:{c}") };
            next.add(Described::new(Role::Label, name, row));
        }
        next.close();
    }
    next.close();
    let started = std::time::Instant::now();
    let mut sent = 0;
    for _ in 0..runs {
        sent = std::hint::black_box(tree_update(&next, "Window", Some(&frame))).nodes.len();
    }
    let changed = started.elapsed() / runs;
    println!(
        "{} nodes: {whole:?} the whole tree; one changed: {changed:?}, {sent} nodes sent",
        frame.nodes().len()
    );
}

/// After the first tree, only what changed is sent: a node changed alone, a node added with
/// its parent (its new children), nothing the same; the tree's own facts only the first time.
#[test]
fn only_what_changed_is_sent() {
    let make = |checked: bool, extra: bool| {
        let mut frame = Frame::new();
        frame.open(Described::new(Role::Dialog, "Settings", Rect::new(0.0, 0.0, 400.0, 300.0)));
        frame.add(
            Described::new(Role::Switch, "Auto-save", Rect::new(10.0, 10.0, 36.0, 20.0))
                .checked(checked),
        );
        frame.add(Described::new(Role::Button, "Close", Rect::new(10.0, 40.0, 60.0, 20.0)));
        if extra {
            frame.add(Described::new(Role::Button, "Reset", Rect::new(10.0, 70.0, 60.0, 20.0)));
        }
        frame.close();
        frame
    };
    let first = make(false, false);
    let whole = tree_update(&first, "Window", None);
    assert!(whole.tree.is_some());
    assert_eq!(whole.nodes.len(), 4, "the window, the dialog and its two");
    let switched = make(true, false);
    let update = tree_update(&switched, "Window", Some(&first));
    assert!(update.tree.is_none(), "the tree's facts told once");
    let labels: Vec<_> =
        update.nodes.iter().filter_map(|(_, n)| n.label().map(str::to_owned)).collect();
    assert_eq!(labels, ["Window", "Auto-save"], "the root and what changed");
    let added = make(true, true);
    let update = tree_update(&added, "Window", Some(&switched));
    let labels: Vec<_> =
        update.nodes.iter().filter_map(|(_, n)| n.label().map(str::to_owned)).collect();
    assert_eq!(labels, ["Window", "Settings", "Reset"], "the new one and its parent");
}

/// A screen reader back after leaving is a new ask (never the last one's number again):
/// what was made for the old one is left, the whole tree told to the new.
#[test]
fn an_ask_after_a_reader_left_is_a_new_one() {
    let mut asks = Asks::default();
    assert!(!asks.current(0));
    asks.asked();
    let first = asks.asked.load(Ordering::Relaxed);
    assert!(asks.current(first));
    asks.left();
    assert!(!asks.current(first), "gone: nothing applied");
    asks.asked();
    let second = asks.asked.load(Ordering::Relaxed);
    assert_ne!(second, first, "a new ask");
    assert!(!asks.current(first), "an update made for the old one is left");
    assert!(asks.current(second));
}
