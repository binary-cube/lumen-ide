use super::*;
use crate::fake::{FakeRepository, commit, entry};

/// A log of `log`'s commits (Ana's and Bob's), read.
fn view(log: Vec<version_control::LogEntry>) -> LogView {
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log,
        refs: vec![
            RefLabel { name: "main".into(), kind: RefKind::Branch },
            RefLabel { name: "feature/login".into(), kind: RefKind::Branch },
            RefLabel { name: "origin/main".into(), kind: RefKind::Remote },
            RefLabel { name: "v1.0".into(), kind: RefKind::Tag },
        ],
        authors: vec!["Ana".into(), "Bob".into()],
        me: Some("Ana".into()),
        files: vec!["a.php".into(), "src/chat/bot.php".into(), "src/shop.php".into()],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    // The references too (read on their own worker): the branches' list is made of them.
    for _ in 0..3000 {
        if view.refs.is_some() {
            break;
        }
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(view.refs.is_some(), "the references read");
    view
}

/// The background work done (the log, its lists): at once when it's quick, up to 15 s with
/// every test running (2 s was too little then: a list read empty).
fn settle(view: &mut LogView) {
    for _ in 0..3000 {
        // The paths matched in the background too (a busy machine: they come later).
        if !view.poll()
            && view.loading.is_none()
            && view.bar.lists_loading.is_none()
            && view.bar.paths_matching.is_none()
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn by(id: &str, author: &str) -> version_control::LogEntry {
    let mut e = entry(id, &[]);
    e.commit =
        version_control::Commit { author: author.into(), ..commit(id, &format!("{id} done")) };
    e
}

fn ids(view: &LogView) -> Vec<String> {
    view.rows().iter().map(|r| r.entry.commit.id.clone()).collect()
}

fn labels(view: &LogView) -> Vec<String> {
    view.bar.popup.as_ref().unwrap().rows.iter().map(|r| r.label.clone()).collect()
}

/// The text applies once typing pauses (or at ↵); a regular expression that isn't one says
/// so and leaves the log as it was.
#[test]
fn the_text_filters_once_typing_pauses() {
    let mut view = view(vec![by("cart", "Ana"), by("chat", "Bob"), by("shop", "Ana")]);
    assert!(view.press_filter(Hit::TextField, Point::default(), 1));
    assert!(view.wants_text());
    view.text_input("ch");
    assert_eq!(ids(&view).len(), 3, "not yet");
    assert!(view.is_animating(), "frames until it applies");
    view.bar.typed_at = Some(std::time::Instant::now() - filters::TYPING_PAUSE);
    settle(&mut view);
    assert_eq!(ids(&view), ["chat"]);
    // A regular expression: `.*`; one that isn't: red, nothing changes.
    view.bar.text.set_text("^(cart|shop)");
    view.press_filter(Hit::Regex, Point::default(), 1);
    settle(&mut view);
    assert_eq!(ids(&view), ["cart", "shop"]);
    view.bar.text.set_text("(");
    view.edit_key(ViewerKey::Enter);
    assert!(view.bar.invalid);
    assert_eq!(ids(&view), ["cart", "shop"]);
    view.clear_text();
    settle(&mut view);
    assert_eq!(ids(&view).len(), 3);
    assert!(!view.bar.invalid);
}

/// The lists: branches by section (searched fuzzily), the authors with "me" first, the
/// folders and files; a row chosen toggles its filter (the list stays open), the pill says
/// what's set and its × clears it.
#[test]
fn the_pills_lists_set_their_filters() {
    let mut view = view(vec![by("a1", "Ana"), by("b1", "Bob"), by("a2", "Ana")]);
    view.toggle_popup(filters::FilterKind::Branch);
    assert_eq!(view.bar.focus, Some(filters::Field::Query), "its search has the keys");
    assert_eq!(
        labels(&view),
        [
            "HEAD",
            &i18n::t("git.branches_local"),
            "feature/login",
            "main",
            &i18n::t("git.branches_remote"),
            "origin/main",
            &i18n::t("git.branches_tags"),
            "v1.0"
        ]
    );
    view.text_input("flog");
    assert!(labels(&view).contains(&"feature/login".to_owned()));
    assert!(!labels(&view).contains(&"main".to_owned()));
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.filter.references, ["feature/login"]);
    assert!(view.pill_label(filters::FilterKind::Branch).1);
    assert!(view.bar.popup.is_some(), "it stays open");
    view.edit_key(ViewerKey::Escape);
    assert!(view.bar.popup.is_none());
    view.clear_filter(filters::FilterKind::Branch);
    assert!(view.filter.references.is_empty());

    view.toggle_popup(filters::FilterKind::User);
    settle(&mut view);
    let me = tr!("git.filter_me", name = "Ana");
    assert_eq!(labels(&view), [me.as_str(), "Ana", "Bob"]);
    view.choose_row(2);
    settle(&mut view);
    assert_eq!(ids(&view), ["b1"]);
    let (label, set) = view.pill_label(filters::FilterKind::User);
    assert!(set && label.ends_with("Bob"), "{label}");
    assert!(view.bar.popup.as_ref().unwrap().rows[2].checked);

    // The paths matched in the background (thousands of them: the typing never waits).
    view.toggle_popup(filters::FilterKind::Paths);
    settle(&mut view);
    assert_eq!(labels(&view), ["src", "src/chat", "a.php"], "folders, then the top's files");
    view.text_input("bot");
    settle(&mut view);
    assert_eq!(labels(&view), ["src/chat/bot.php"]);
    view.choose_row(0);
    settle(&mut view);
    assert_eq!(view.filter.paths, ["src/chat/bot.php"]);
    assert_eq!(labels(&view)[0], "src/chat/bot.php", "the chosen ones first");
    assert!(view.bar.popup.as_ref().unwrap().rows[0].checked);
}

/// Date ▾: the last days, or an interval typed (checked first); the order: by date or
/// topologically.
#[test]
fn dates_and_the_order() {
    let mut view = view(vec![by("a1", "Ana")]);
    // Date ▾: its picker. A preset applies at once (local days, today included).
    view.toggle_popup(filters::FilterKind::Date);
    let picker = view.bar.picker.as_ref().expect("the picker");
    assert_eq!(picker.preset, Some(0), "any time");
    let today = crate::local_time::today();
    assert_eq!(picker.max, Some(today), "no commits in the future");
    view.picker_event(ui_components::PickerEvent::Preset(3));
    assert!(view.bar.picker.is_none(), "applied: it closes");
    assert_eq!(view.filter.since, crate::local_time::date_start(today.add_days(-6)));
    assert_eq!(view.filter.until, crate::local_time::date_start(today.add_days(1)));
    let (label, set) = view.pill_label(filters::FilterKind::Date);
    assert!(set && label.ends_with(&i18n::t("time.picker_last_7")), "{label}");
    // A range picked: from its first day's midnight to the end of its last.
    view.toggle_popup(filters::FilterKind::Date);
    assert_eq!(view.bar.picker.as_ref().unwrap().preset, Some(3), "checked again");
    let (a, b) = (
        ui_components::Date::new(2026, 1, 5).unwrap(),
        ui_components::Date::new(2026, 1, 9).unwrap(),
    );
    view.picker_event(ui_components::PickerEvent::Apply(a, b));
    assert_eq!(view.filter.since, crate::local_time::date_start(a));
    assert_eq!(
        view.filter.until,
        crate::local_time::date_start(b.add_days(1)),
        "the last day whole"
    );
    let (label, set) = view.pill_label(filters::FilterKind::Date);
    assert!(set && label.contains("5 – 9"), "{label}");
    // Any time: no filter; Cancel: nothing changes.
    view.toggle_popup(filters::FilterKind::Date);
    view.picker_event(ui_components::PickerEvent::Cancel);
    assert!(view.filter.since.is_some());
    view.toggle_popup(filters::FilterKind::Date);
    view.picker_event(ui_components::PickerEvent::Preset(0));
    assert!(view.filter.since.is_none() && !view.pill_label(filters::FilterKind::Date).1);
    view.clear_filter(filters::FilterKind::Date);
    assert!(view.filter.since.is_none() && view.filter.until.is_none());

    view.toggle_popup(filters::FilterKind::Order);
    view.choose_row(1);
    assert_eq!(view.filter.order, version_control::LogOrder::Topological);
    assert!(view.pill_label(filters::FilterKind::Order).1);
}

/// Filtered to nothing: what's filtered by, under "No commits match".
#[test]
fn nothing_found_says_what_s_filtered() {
    let mut view = view(vec![by("a1", "Ana")]);
    view.bar.text.set_text("nope");
    view.apply_text();
    settle(&mut view);
    assert!(view.rows().is_empty());
    view.filter_branch(Some("main".into()));
    let summary = view.filters_summary();
    assert!(summary.contains("\u{201c}nope\u{201d}") && summary.contains("main"), "{summary}");
}

/// The branches panel: its filter keeps the branches matching (their folders open); a
/// click marks a branch, a double click shows its log; a local branch's arrows say what's
/// to push and to pull.
#[test]
fn the_branches_panel_filters_and_a_double_click_shows_a_branch() {
    let mut view = view(vec![by("a1", "Ana")]);
    view.collapsed.insert("local/feature".into());
    view.rebuild_branches();
    view.press_filter(Hit::BranchSearch, Point::default(), 1);
    view.text_input("log");
    let names: Vec<String> = view
        .branch_rows
        .iter()
        .filter_map(|r| match &r.item {
            BranchItem::Ref { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["feature/login"], "its folder opened");
    let at =
        view.branch_rows.iter().position(|r| matches!(&r.item, BranchItem::Ref { .. })).unwrap();
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.filter.references, ["feature/login"], "↵: the first one");
    view.filter_branch(None);
    view.bar.focus = None;
    // Clicks on its row: one marks it, two show it.
    view.hits = vec![(Rect::new(0.0, 0.0, 100.0, 20.0), Hit::Branch(at))];
    view.press(Point::new(5.0, 5.0), 1);
    assert!(view.filter.references.is_empty());
    assert_eq!(view.branch_selected, Some(at));
    view.press(Point::new(5.0, 5.0), 2);
    assert_eq!(view.filter.references, ["feature/login"]);
}

/// The bar's fields are text fields: the app's ⌘A / ⌘X / ⌘Z reach the one with the keys,
/// typing replaces what's selected, ⌥ / Ctrl move by word and ⇧ selects; a popover's
/// search the same.
#[test]
fn the_fields_take_the_edit_commands() {
    let mut view = view(vec![by("a1", "Ana")]);
    use ui_components::KeyReach;
    // The text field, in the view.
    assert!(view.press_filter(Hit::TextField, Point::default(), 1));
    view.text_input("chat bot");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("chat bot"));
    view.text_input("x");
    assert_eq!(view.bar.text.text(), "x", "typing replaces the selection");
    assert!(view.command("undo"));
    assert_eq!(view.bar.text.text(), "chat bot");
    view.edit_key(ViewerKey::End);
    view.edit_key_with(ViewerKey::Left, KeyReach { word: true, extend: true });
    assert_eq!(view.selected_text().as_deref(), Some("bot"), "a word, selected");
    assert!(view.command("cut"));
    assert_eq!(view.bar.text.text(), "chat ");
    view.edit_key_with(ViewerKey::Backspace, KeyReach { word: true, extend: false });
    assert_eq!(view.bar.text.text(), "");
    // A popover's search.
    view.toggle_popup(filters::FilterKind::Branch);
    view.text_input("main");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("main"));
    assert!(view.paste("feat"));
    assert_eq!(view.bar.popup.as_ref().unwrap().query.text(), "feat");
    assert!(labels(&view).contains(&"feature/login".to_owned()), "the list follows");
    // The Date picker's fields.
    view.toggle_popup(filters::FilterKind::Date);
    view.bar.picker.as_mut().unwrap().field = Some(ui_components::date_range::PickerField::From);
    assert!(view.wants_text());
    view.text_input("01.01.2026");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("01.01.2026"));
}

/// A list's search emptied by its ×: every row again, its search keeping the keys.
#[test]
fn a_list_s_search_clears_with_its_x() {
    let mut view = view(vec![by("a1", "Ana")]);
    view.toggle_popup(filters::FilterKind::Branch);
    let all = labels(&view);
    view.text_input("login");
    assert_eq!(labels(&view), [i18n::t("git.branches_local").as_str(), "feature/login"]);
    assert!(view.press_filter(Hit::ClearPopupQuery, Point::new(0.0, 0.0), 1));
    assert!(view.bar.popup.as_ref().unwrap().query.text().is_empty());
    assert_eq!(labels(&view), all);
    assert_eq!(view.bar.focus, Some(filters::Field::Query));
}

/// The order's list (no search in it): ↑ / ↓ move in it, ↵ chooses, as its footer says —
/// never the log's rows behind it.
#[test]
fn the_order_s_list_is_moved_in_by_the_keys() {
    let mut view = view(vec![by("a1", "Ana"), by("b2", "Bob")]);
    settle(&mut view);
    let selected = view.selected;
    view.toggle_popup(filters::FilterKind::Order);
    assert!(view.edit_key(ViewerKey::Down));
    assert!(view.edit_key(ViewerKey::Down));
    assert_eq!(view.selected, selected, "the log's rows stay as they were");
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(view.filter.order, version_control::LogOrder::Topological);
    assert!(view.edit_key(ViewerKey::Escape) || view.bar.popup.is_none());
}
