use super::*;

/// A part as a form's control: where it is, whether it takes the focus; what reached it.
#[derive(Default)]
struct Part {
    area: Option<Rect>,
    takes_focus: bool,
    holds: bool,
    focused: bool,
    presses: u32,
    moves: u32,
    typed: String,
    over: bool,
}

impl Part {
    fn at(x: f32, takes_focus: bool, holds: bool) -> Self {
        Self { area: Some(Rect::new(x, 0.0, 10.0, 10.0)), takes_focus, holds, ..Self::default() }
    }
}

impl Control for Part {
    fn area(&self) -> Option<Rect> {
        self.area
    }

    fn press(&mut self, _p: Point, _clicks: u32) -> Press {
        self.presses += 1;
        if self.holds { Press::Held } else { Press::Took }
    }

    fn drag(&mut self, _p: Point) -> bool {
        self.moves += 1;
        true
    }

    fn release(&mut self) -> bool {
        true
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.over = p.is_some();
        true
    }

    fn focusable(&self) -> bool {
        self.takes_focus
    }

    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn text(&mut self, typed: &str) -> bool {
        self.typed.push_str(typed);
        true
    }
}

/// A press is the topmost part's under it; one that holds gets the moves until let go;
/// the focus goes to the part pressed that takes it, and nowhere on an empty press.
#[test]
fn a_press_its_hold_and_the_focus() {
    let (mut field, mut thumb) = (Part::at(0.0, true, false), Part::at(20.0, false, true));
    let mut c = Controls::default();
    {
        let mut parts: [&mut dyn Control; 2] = [&mut field, &mut thumb];
        assert_eq!(c.press(&mut parts, Point::new(5.0, 5.0), 1), Some((0, Press::Took)));
        assert_eq!(c.focused(), Some(0));
        assert_eq!(c.text(&mut parts, "a"), Some(0), "typing goes to the focused field");
        assert_eq!(c.press(&mut parts, Point::new(25.0, 5.0), 1), Some((1, Press::Held)));
        assert_eq!(c.focused(), Some(0), "a thumb doesn't take the focus");
        assert!(c.held());
        assert_eq!(c.drag(&mut parts, Point::new(90.0, 90.0)), Some((1, true)), "held anywhere");
        assert_eq!(c.release(&mut parts), Some((1, true)));
        assert!(!c.held());
        assert_eq!(c.drag(&mut parts, Point::new(25.0, 5.0)), None, "none holds it");
        assert_eq!(c.press(&mut parts, Point::new(300.0, 5.0), 1), None);
        assert_eq!(c.focused(), None, "an empty press: the focus nowhere");
    }
    assert_eq!((field.typed.as_str(), field.focused, thumb.moves), ("a", false, 1));
}

/// Tab moves the focus among the parts that take it, ⇧Tab back, around the ends.
#[test]
fn tab_moves_the_focus() {
    let (mut a, mut skip, mut b) =
        (Part::at(0.0, true, false), Part::at(20.0, false, false), Part::at(40.0, true, false));
    let mut c = Controls::default();
    let mut parts: [&mut dyn Control; 3] = [&mut a, &mut skip, &mut b];
    let back = KeyReach { word: false, extend: true };
    assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), Some(0));
    assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), Some(2), "past one without");
    assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), Some(0), "around");
    assert_eq!(c.key(&mut parts, ViewerKey::Tab, back), Some(2), "⇧Tab back");
}

/// The mouse over a part: it's told; leaving it for another, the first is told it left.
#[test]
fn the_hover_follows_the_mouse() {
    let (mut a, mut b) = (Part::at(0.0, false, false), Part::at(20.0, false, false));
    let mut c = Controls::default();
    {
        let mut parts: [&mut dyn Control; 2] = [&mut a, &mut b];
        assert!(c.hover(&mut parts, Some(Point::new(5.0, 5.0))));
        assert_eq!(c.hovered(), Some(0));
        assert!(c.hover(&mut parts, Some(Point::new(25.0, 5.0))));
        assert_eq!(c.hovered(), Some(1));
    }
    assert!(!a.over && b.over);
}

/// A field as a control: focused by Tab, it takes the text (one line), the editing keys and
/// the edit commands; what it has selected is copied.
#[test]
fn a_field_is_a_control() {
    let mut field = ui_kit::TextInput::new("");
    let mut c = Controls::default();
    let mut parts: [&mut dyn Control; 1] = [&mut field];
    assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), Some(0));
    assert_eq!(c.text(&mut parts, "a\nb"), Some(0));
    assert_eq!(c.key(&mut parts, ViewerKey::Backspace, KeyReach::default()), Some(0));
    assert_eq!(c.command(&mut parts, "select_all"), Some(0));
    assert_eq!(c.selected_text(&parts).as_deref(), Some("a "), "its breaks became spaces");
    assert_eq!(c.command(&mut parts, "cut"), Some(0));
    assert_eq!(c.selected_text(&parts), None);
    assert_eq!(c.command(&mut parts, "undo"), Some(0));
}

/// An open list is hit anywhere (a press outside closes it) and tells its view what it did.
#[test]
fn an_open_list_is_a_control_over_everything() {
    let mut list = crate::picker::Picker::default();
    list.open(vec![crate::picker::PickerItem { label: "a".into(), ..Default::default() }], None);
    let mut c = Controls::default();
    {
        let mut parts: [&mut dyn Control; 1] = [&mut list];
        assert_eq!(c.press(&mut parts, Point::new(5000.0, 5000.0), 1), Some((0, Press::Took)));
    }
    assert_eq!(list.take_event(), Some(crate::picker::PickerEvent::Closed));
    assert!(!list.is_open());
}

/// A separator as a control: pressed on it, held; the moves say where its line goes.
#[test]
fn a_divider_is_held_and_moved() {
    use crate::splitter::{Orientation, Splitter, SplitterStyle};
    let mut splitter = Splitter::new(SplitterStyle::default(), Orientation::Vertical);
    splitter.set_bounds(Rect::new(100.0, 0.0, 6.0, 400.0), 1.0);
    let mut at = None;
    let mut c = Controls::default();
    {
        let mut divider = Divider { splitter: &mut splitter, at: &mut at };
        let mut parts: [&mut dyn Control; 1] = [&mut divider];
        assert_eq!(c.press(&mut parts, Point::new(103.0, 50.0), 1), Some((0, Press::Held)));
        assert_eq!(c.drag(&mut parts, Point::new(160.0, 50.0)), Some((0, true)));
        assert!(c.release(&mut parts).is_some());
    }
    assert_eq!(at, Some(160.0), "its line's center under the pointer");
    assert!(!splitter.is_dragging());
}

/// A view inside a view: what it does with a press is kept for its owner; its keys and
/// text are its own.
#[test]
fn an_embedded_view_is_a_control() {
    #[derive(Default)]
    struct Inner {
        typed: String,
        asked: Option<(Rect, Vec<ViewMenuEntry>)>,
    }
    impl crate::Viewer for Inner {
        fn paint(&mut self, _ctx: crate::ViewerPaint<'_>) {}
        fn press(&mut self, p: Point, _clicks: u32) -> Option<crate::ViewerAction> {
            self.asked =
                Some((Rect::new(p.x, p.y, 10.0, 10.0), vec![ViewMenuEntry::item("a", "A")]));
            Some(crate::ViewerAction::Close)
        }
        fn take_menu(&mut self) -> Option<(Rect, Vec<ViewMenuEntry>)> {
            self.asked.take()
        }
        fn wants_text(&self) -> bool {
            true
        }
        fn text_input(&mut self, text: &str) -> bool {
            self.typed.push_str(text);
            true
        }
    }
    let mut inner = Inner::default();
    let mut action = None;
    let mut menu = None;
    let mut c = Controls::default();
    {
        let mut part = Embedded {
            view: &mut inner,
            area: Rect::new(0.0, 0.0, 50.0, 50.0),
            action: &mut action,
            menu: &mut menu,
        };
        let mut parts: [&mut dyn Control; 1] = [&mut part];
        assert!(c.press(&mut parts, Point::new(5.0, 5.0), 1).is_some());
        assert_eq!(c.text(&mut parts, "x"), Some(0), "focused by the press");
        c.release(&mut parts);
    }
    assert!(matches!(action, Some(crate::ViewerAction::Close)));
    assert_eq!(inner.typed, "x");
    // The menu it asked for under its button: kept for the owner to give the app.
    let (anchor, rows) = menu.expect("the asked menu kept for its owner");
    assert_eq!(anchor.origin, Point::new(5.0, 5.0));
    assert_eq!(rows.len(), 1);
    assert!(inner.asked.is_none(), "taken from the view");
}

/// What a view asks without its parts (from `&self`) is answered from the last hover and
/// settle: the cursor and the tooltip where the mouse was, whether a part moves.
#[test]
fn read_only_questions_are_answered_from_the_last_hover_and_settle() {
    struct Lit(Rect);
    impl Control for Lit {
        fn area(&self) -> Option<Rect> {
            Some(self.0)
        }
        fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
            Some(ViewerCursor::Pointer)
        }
        fn animating(&self) -> bool {
            true
        }
    }
    let mut lit = Lit(Rect::new(0.0, 0.0, 10.0, 10.0));
    let mut c = Controls::default();
    let at = Point::new(5.0, 5.0);
    {
        let mut parts: [&mut dyn Control; 1] = [&mut lit];
        c.hover(&mut parts, Some(at));
        c.settle(&parts);
    }
    assert_eq!(c.cursor_at(at), Some(ViewerCursor::Pointer));
    assert_eq!(c.cursor_at(Point::new(6.0, 6.0)), None, "only where the mouse was");
    assert!(c.is_animating());
}

/// A part shown only sometimes is never hit while hidden; an open list keeps Tab; the
/// wheel without a point goes to the part the mouse is over.
#[test]
fn shown_parts_tab_kept_and_the_wheel_over_the_hovered() {
    let mut a = Part::at(0.0, true, false);
    let mut c = Controls::default();
    {
        let mut hidden = Shown { part: &mut a, shown: false };
        let mut parts: [&mut dyn Control; 1] = [&mut hidden];
        assert_eq!(c.press(&mut parts, Point::new(5.0, 5.0), 1), None, "hidden: not hit");
    }
    let mut list = crate::picker::Picker::default();
    list.open(vec![crate::picker::PickerItem { label: "a".into(), ..Default::default() }], None);
    let mut field = Part::at(0.0, true, false);
    let mut parts: [&mut dyn Control; 2] = [&mut list, &mut field];
    c.focus(&mut parts, Some(0));
    assert_eq!(
        c.key(&mut parts, ViewerKey::Tab, KeyReach::default()),
        Some(0),
        "the list keeps Tab"
    );
    assert_eq!(c.focused(), Some(0));
    assert_eq!(c.scroll_hovered(&mut parts, 0.0, 10.0), None, "nothing hovered");
}

/// A part over everything takes the keys it uses and passes the rest (and the text) on to
/// the focused one; Tab with nowhere else to go isn't used; a sticky focus stays on a
/// press beside its parts; the wheel kept by a list at its end; a missing part is nothing.
#[test]
fn keys_over_everything_sticky_focus_wheel_and_missing_parts() {
    struct Over {
        uses: ViewerKey,
    }
    impl Control for Over {
        fn captures_keys(&self) -> bool {
            true
        }
        fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
            key == self.uses
        }
        fn hit(&self, _p: Point) -> bool {
            true
        }
        fn takes_wheel(&self, _p: Point) -> bool {
            true
        }
    }
    let mut over = Over { uses: ViewerKey::Down };
    let mut field = ui_kit::TextInput::new("");
    let mut c = Controls::sticky();
    let mut parts: [&mut dyn Control; 2] = [&mut over, &mut field];
    c.focus(&mut parts, Some(1));
    assert_eq!(c.key(&mut parts, ViewerKey::Down, KeyReach::default()), Some(0), "its key");
    assert_eq!(c.text(&mut parts, "a"), Some(1), "the text goes on to the field");
    assert_eq!(c.key(&mut parts, ViewerKey::Left, KeyReach::default()), Some(1), "passed on");
    assert_eq!(c.scroll(&mut parts, Point::new(1.0, 1.0), 0.0, 5.0), Some(0), "kept wheel");
    assert_eq!(c.press(&mut parts[1..], Point::new(9000.0, 9000.0), 1), None);
    assert_eq!(c.focused(), Some(1), "sticky: the focus stays");

    let mut only = ui_kit::TextInput::new("");
    let mut c = Controls::default();
    {
        let mut parts: [&mut dyn Control; 1] = [&mut only];
        assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), Some(0));
        assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), None, "nowhere else");
        assert_eq!(c.paste(&mut parts, "pasted"), Some(0));
        c.command(&mut parts, "select_all");
    }
    // Asked from `&self`: the view lends its field by its place.
    let lent = c.selected_text_with(|i| (i == 0).then_some(&only as &dyn Control));
    assert_eq!(lent.as_deref(), Some("pasted"));

    let mut missing: Option<ui_kit::TextInput> = None;
    let mut c = Controls::default();
    let mut parts: [&mut dyn Control; 2] = [&mut missing, &mut Absent];
    assert_eq!(c.key(&mut parts, ViewerKey::Tab, KeyReach::default()), None, "nothing there");
}

/// A scrolled area: its bar's thumb held, the wheel anywhere over it, within its range.
#[test]
fn a_scrolled_area_takes_the_wheel_over_it() {
    let mut bar = crate::scrollbar::EdgeScrollbar::default();
    let mut offset = 0.0;
    let area = Rect::new(0.0, 0.0, 100.0, 100.0);
    let mut c = Controls::default();
    {
        let mut scrolled = Scrolled { bar: &mut bar, offset: &mut offset, area, max: 50.0 };
        let mut parts: [&mut dyn Control; 1] = [&mut scrolled];
        assert_eq!(c.scroll(&mut parts, Point::new(10.0, 10.0), 0.0, 500.0), Some(0));
    }
    assert_eq!(offset, 50.0, "never past its end");
}

/// A focused list that closes has no more keys: typed text and keys aren't swallowed by
/// it (they go nowhere; the view gives the focus back to its field).
#[test]
fn a_closed_list_has_no_keys() {
    let mut list = crate::picker::Picker::default();
    list.open(vec![crate::picker::PickerItem { label: "a".into(), ..Default::default() }], None);
    let mut c = Controls::default();
    {
        let mut parts: [&mut dyn Control; 1] = [&mut list];
        c.focus(&mut parts, Some(0));
    }
    list.close();
    let mut parts: [&mut dyn Control; 1] = [&mut list];
    assert_eq!(c.text(&mut parts, "x"), None);
    assert_eq!(c.key(&mut parts, ViewerKey::Down, KeyReach::default()), None);
}
