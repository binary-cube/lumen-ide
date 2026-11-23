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
