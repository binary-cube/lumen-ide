use super::*;

fn button(name: &str) -> Described {
    Described::new(Role::Button, name, Rect::new(0.0, 0.0, 10.0, 10.0))
}

/// Nodes go under the group open when they're put; a group closed, the next go back up.
#[test]
fn nodes_go_under_the_group_open() {
    let mut frame = Frame::new();
    let dialog = frame.open(Described::new(Role::Dialog, "Settings", Rect::default()));
    let save = frame.add(button("Save"));
    frame.close();
    let other = frame.add(button("Close"));
    let parent = |id| frame.nodes().iter().find(|n| n.id == id).map(|n| n.parent);
    assert_eq!(parent(dialog), Some(ROOT));
    assert_eq!(parent(save), Some(dialog));
    assert_eq!(parent(other), Some(ROOT));
}

/// The same thing keeps its identity from frame to frame, wherever it is drawn; two alike
/// under one parent are told apart.
#[test]
fn identities_hold_from_frame_to_frame() {
    let ids = |y: f32| {
        let mut frame = Frame::new();
        let mut ok = button("OK");
        ok.bounds.origin.y = y;
        let a = frame.add(ok.clone());
        let b = frame.add(ok);
        (a, b)
    };
    let (a, b) = ids(0.0);
    assert_ne!(a, b, "two OK buttons are two nodes");
    assert_eq!(ids(40.0), (a, b), "moved (scrolled), the same nodes");
    assert_ne!(a, ROOT);
}

/// A frame like the last one is told as the same (nothing sent again); a state changed or
/// a node gone isn't.
#[test]
fn a_frame_like_the_last_is_the_same() {
    let frame = |on: bool, save: bool| {
        let mut f = Frame::new();
        if save {
            f.add(button("Save"));
        }
        f.add(Described::new(Role::Switch, "Auto-save", Rect::default()).checked(on));
        f
    };
    assert!(frame(true, true).same_as(&frame(true, true)));
    assert!(!frame(true, true).same_as(&frame(false, true)));
    assert!(!frame(true, false).same_as(&frame(true, true)));
}

/// A role brings the actions it usually has; a node disabled has none.
#[test]
fn roles_bring_their_actions() {
    assert!(button("Go").actions.press);
    let field = Described::new(Role::SearchField, "Search", Rect::default());
    assert!(field.actions.set_value);
    assert!(!Described::new(Role::Label, "Hi", Rect::default()).actions.press);
    assert_eq!(button("Go").disabled(true).actions, Actions::default());
}

/// A control put without a name takes the text on its row, the nearest; text on another
/// row or in another group isn't its own.
#[test]
fn an_unnamed_control_takes_the_text_on_its_row() {
    let mut frame = Frame::new();
    let label =
        |text: &str, x: f32, y: f32| Described::new(Role::Label, text, Rect::new(x, y, 60.0, 0.0));
    frame.add(label("Auto-save", 0.0, 15.0));
    frame.add(label("Far away", 0.0, 60.0));
    frame.add(Described::new(Role::Switch, "", Rect::new(200.0, 5.0, 36.0, 20.0)).checked(true));
    frame.add(label("Ignored", 0.0, 95.0));
    frame.add(Described::new(Role::CheckBox, "", Rect::new(200.0, 85.0, 16.0, 16.0)));
    frame.add(Described::new(Role::Button, "Named", Rect::new(200.0, 85.0, 16.0, 16.0)));
    frame.name_from_labels();
    let names: Vec<&str> = frame.nodes().iter().map(|n| &*n.name).collect();
    assert_eq!(names[2], "Auto-save");
    assert_eq!(names[4], "Ignored", "the text on its own row");
    assert_eq!(names[5], "Named", "a name of its own kept");
}

/// A text drawn inside the last control put with a name that says its name (a menu row's
/// label) is that control's; one that says more (its keys) and one outside it are read.
#[test]
fn a_text_inside_a_named_control_is_its_own() {
    let mut frame = Frame::new();
    frame.add(Described::new(Role::MenuItem, "Save", Rect::new(0.0, 0.0, 200.0, 24.0)));
    frame.add(Described::new(Role::Label, "Save", Rect::new(30.0, 16.0, 40.0, 0.0)));
    frame.add(Described::new(Role::Label, "⌘S", Rect::new(170.0, 16.0, 20.0, 0.0)));
    frame.add(Described::new(Role::Label, "Ready", Rect::new(0.0, 300.0, 50.0, 0.0)));
    let names: Vec<_> = frame.nodes().iter().map(|n| &*n.name).collect();
    assert_eq!(names, ["Save", "⌘S", "Ready"]);
}

/// A table's row has its own texts inside it: it never takes a text beside it on its line.
#[test]
fn a_row_takes_no_name_from_beside_it() {
    let mut frame = Frame::new();
    frame.add(Described::new(Role::Label, "Users", Rect::new(0.0, 15.0, 60.0, 0.0)));
    frame.add(Described::new(Role::Row, "", Rect::new(100.0, 5.0, 400.0, 20.0)));
    frame.name_from_labels();
    assert_eq!(&*frame.nodes()[1].name, "");
}

/// A named group's texts that say its name or its value aren't read again; its other texts
/// (a row's detail) are, under it.
#[test]
fn a_group_keeps_the_texts_its_name_doesnt_say() {
    let mut frame = Frame::new();
    let row =
        frame.open(Described::new(Role::ListItem, "Shop.php", Rect::new(0.0, 0.0, 300.0, 24.0)));
    frame.add(Described::new(Role::Label, "Shop.php", Rect::new(10.0, 4.0, 60.0, 16.0)));
    frame.add(Described::new(Role::Label, "src/App", Rect::new(100.0, 4.0, 60.0, 16.0)));
    frame.close();
    let under: Vec<_> =
        frame.nodes().iter().filter(|n| n.parent == row).map(|n| &*n.name).collect();
    assert_eq!(under, ["src/App"]);
}

/// An unnamed control's identity follows the name it takes, not its place among the
/// unnamed: one appearing above it changes nothing of it; a text far along its line isn't
/// its name.
#[test]
fn an_unnamed_control_keeps_its_identity_when_another_appears() {
    let switch_id = |with_other: bool| {
        let mut frame = Frame::new();
        if with_other {
            frame.add(Described::new(Role::Label, "Wrap", Rect::new(0.0, 0.0, 60.0, 16.0)));
            frame.add(Described::new(Role::Switch, "", Rect::new(100.0, 0.0, 36.0, 16.0)));
        }
        frame.add(Described::new(Role::Label, "Auto-save", Rect::new(0.0, 40.0, 60.0, 16.0)));
        frame.add(Described::new(Role::Switch, "", Rect::new(100.0, 40.0, 36.0, 16.0)));
        frame.add(Described::new(Role::Label, "Far", Rect::new(2000.0, 80.0, 60.0, 16.0)));
        frame.add(Described::new(Role::CheckBox, "", Rect::new(0.0, 80.0, 16.0, 16.0)));
        frame.name_from_labels();
        let n = frame.nodes();
        let far = n.iter().find(|n| n.role == Role::CheckBox).map(|n| n.name.to_string());
        assert_eq!(far.as_deref(), Some(""), "a text far along its line isn't its name");
        n.iter().find(|n| &*n.name == "Auto-save" && n.role == Role::Switch).map(|n| n.id)
    };
    assert_eq!(switch_id(false), switch_id(true));
    assert!(switch_id(false).is_some());
}

/// The group open is known by its role (a fold inside a row told whole adds nothing); its
/// place and lines are kept on the node.
#[test]
fn the_open_group_s_role_and_a_node_s_place_are_kept() {
    let mut frame = Frame::new();
    assert_eq!(frame.open_role(), None);
    frame.open(Described::new(Role::List, "", Rect::default()).count(40));
    assert_eq!(frame.open_role(), Some(Role::List));
    let item = frame.add(Described::new(Role::ListItem, "a", Rect::default()).index(7));
    frame.close();
    assert_eq!(frame.open_role(), None);
    let node = frame.nodes().iter().find(|n| n.id == item).expect("the item");
    assert_eq!(node.place.index, Some(7));
    assert_eq!(frame.nodes()[0].place.count, Some(40));
}

/// A text cut short to fit is still the control's own: not read a second time.
#[test]
fn a_name_cut_short_is_still_the_control_s() {
    let mut frame = Frame::new();
    frame.open(Described::new(Role::TreeItem, "controllers", Rect::new(0.0, 0.0, 100.0, 20.0)));
    frame.add(Described::new(Role::Label, "contr…", Rect::new(10.0, 2.0, 40.0, 16.0)));
    frame.add(Described::new(Role::Label, "…llers", Rect::new(10.0, 2.0, 40.0, 16.0)));
    frame.close();
    assert_eq!(frame.nodes().len(), 1);
}

/// A paragraph drawn a word at a time reads as one text a line: a space where the words
/// stand apart, none between the pieces of one word, nothing joined across lines or parents.
#[test]
fn pieces_of_a_line_read_as_one_text() {
    let mut frame = Frame::new();
    let piece = |name: &str, x: f32, y: f32, w: f32| {
        Described::new(Role::Label, name, Rect::new(x, y, w, 16.0))
    };
    frame.add(piece("Save ", 0.0, 0.0, 40.0));
    frame.add(piece("your", 40.0, 0.0, 30.0));
    frame.add(piece("work", 74.0, 0.0, 30.0));
    frame.add(piece("ind", 0.0, 20.0, 20.0));
    frame.add(piece("ex.php", 20.0, 20.0, 40.0));
    frame.add(piece("far away", 400.0, 20.0, 60.0));
    frame.join_pieces();
    let names: Vec<_> = frame.nodes().iter().map(|n| n.name.to_string()).collect();
    assert_eq!(names, ["Save your work", "index.php", "far away"]);
}

/// A dialog's title drawn in it isn't read again; its other texts are, even one its title
/// holds.
#[test]
fn a_dialog_s_title_is_its_own_and_nothing_else() {
    let mut frame = Frame::new();
    frame.open(Described::new(Role::Dialog, "Settings", Rect::new(0.0, 0.0, 400.0, 300.0)));
    frame.add(Described::new(Role::Label, "Settings", Rect::new(10.0, 10.0, 60.0, 16.0)));
    frame.add(Described::new(Role::Label, "Set", Rect::new(10.0, 60.0, 30.0, 16.0)));
    frame.close();
    let names: Vec<_> = frame.nodes().iter().map(|n| n.name.to_string()).collect();
    assert_eq!(names, ["Settings", "Set"]);
}

/// A button showing only an icon is named by its tooltip and says its keys; a title beside
/// it never names it; a control with a name keeps it.
#[test]
fn an_icon_button_is_named_by_its_tooltip_never_by_a_title_beside_it() {
    let mut frame = Frame::new();
    frame.add(Described::new(Role::Label, "Project", Rect::new(0.0, 0.0, 60.0, 16.0)));
    let icon = frame.add(Described::new(Role::Button, "", Rect::new(70.0, 0.0, 16.0, 16.0)));
    frame.add(Described::new(Role::Button, "Save", Rect::new(100.0, 0.0, 40.0, 16.0)));
    frame.name_from_labels();
    assert!(frame.nodes()[1].name.is_empty(), "not the title beside it");
    frame.name_from_tips(|r| {
        (r.min_x() >= 70.0).then(|| ("Collapse All".into(), Some("⌘-".into())))
    });
    let named = &frame.nodes()[1];
    assert_eq!((&*named.name, named.shortcut.as_deref()), ("Collapse All", Some("⌘-")));
    assert_ne!(named.id, icon, "its identity follows its name");
    assert_eq!(&*frame.nodes()[2].name, "Save");
}

/// A place a screen reader tells in a field (a line, a character in it) is a byte of its
/// text: characters of any width, the line's break counted, the end of the text too.
#[test]
fn a_place_in_a_text_is_its_byte() {
    let text = "ăb\ncd";
    assert_eq!(byte_of(text, 0, 0), Some(0));
    assert_eq!(byte_of(text, 0, 1), Some(2), "after ă (two bytes)");
    assert_eq!(byte_of(text, 1, 1), Some(5));
    assert_eq!(byte_of(text, 1, 9), Some(text.len()), "past a line's end: its end");
    assert_eq!(byte_of("", 0, 0), Some(0));
    assert_eq!(byte_of("a\n", 1, 0), Some(2), "the empty line after a last break");
    assert_eq!(byte_of(text, 5, 0), None);
    assert_ne!(run_id(NodeId(1), 0), run_id(NodeId(1), 1));
    assert!(run_id(NodeId(1), 0) >> 63 == 1, "never one of the app's nodes");
}
