use super::*;

fn compare(a: &str, b: &str) -> Compare {
    let mut c = Compare::default();
    c.set_data(&CompareInput::files(("/a".into(), a.into()), ("/b".into(), b.into())));
    found(&mut c);
    c
}

/// The differences (found on a thread), waited for.
fn found(c: &mut Compare) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !c.loaded {
        assert!(std::time::Instant::now() < deadline, "the comparison didn't end");
        c.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn rows_align_and_picks_compose() {
    let mut c = compare("a\nb\nc\n", "a\nB\nc\nd\n");
    assert_eq!(c.changes(), 2);
    // a | b/B | c | -/d
    assert_eq!(c.rows.len(), 4);
    assert_eq!((c.rows[3].left, c.rows[3].right), (None, Some(3)));
    c.merge = true;
    c.set_pick(1, Pick::Right);
    assert!(c.is_dirty());
    assert_eq!(c.result(), ["a", "b", "c", "d"]);
    c.set_pick(0, Pick::Both);
    assert_eq!(c.result(), ["a", "b", "B", "c", "d"]);
}

fn active(text: &str, selection: Option<std::ops::Range<usize>>) -> ActiveFile {
    ActiveFile { path: "/p/A.php".into(), text: text.into(), line: 0, cursor: 0, selection }
}

#[test]
fn clipboard_against_the_file_writes_only_the_file() {
    let input = clipboard_input(&active("a\nb\n", None), "a\nc\n".into());
    assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("a\nc\n", "a\nb\n"));
    assert_eq!(input.read_only, [true, false]);
    let mut c = Compare::default();
    c.set_data(&input);
    found(&mut c);
    assert_eq!(c.target(), Side::Right);
    assert!(c.writable());
}

#[test]
fn clipboard_against_a_selection_writes_nothing() {
    let input = clipboard_input(&active("one\ntwo\nthree\n", Some(4..8)), "two\n".into());
    assert_eq!(input.right.1, "two\n");
    assert_eq!(input.read_only, [true, true]);
    let mut c = Compare::default();
    c.set_data(&input);
    found(&mut c);
    assert_eq!(c.changes(), 0);
    // No "Compose" without anything to write to.
    assert!(!c.command("merge"));
    assert!(!c.merge);
}

#[test]
fn separators_resize_columns_within_limits() {
    let mut c = compare("a\n", "b\n");
    c.scale = 1.0;
    c.list = Rect::new(0.0, 0.0, 1030.0, 500.0);
    c.gutter = 30.0;
    c.columns_w = 1000.0;
    assert_eq!(c.ends(), [0.5, 1.0]);
    c.drag_separator(0, 315.0);
    assert!((c.ends()[0] - 0.3).abs() < 1e-4);
    // Never narrower than 120 px.
    c.drag_separator(0, -500.0);
    assert!((c.ends()[0] - 0.12).abs() < 1e-4);
    // "Compose" keeps its own split; the second separator stays right of the first.
    c.merge = true;
    c.columns_w = 970.0;
    assert_eq!(c.ends(), [1.0 / 3.0, 2.0 / 3.0]);
    c.drag_separator(1, 100.0);
    assert!(c.ends()[1] > c.ends()[0]);
    c.merge = false;
    assert!((c.ends()[0] - 0.12).abs() < 1e-4);
}

#[test]
fn a_click_on_a_difference_makes_it_current_and_its_pill_picks() {
    let mut c = compare("a\nb\nc\nd\n", "a\nB\nc\nD\n");
    c.merge = true;
    c.hits = vec![
        (Rect::new(0.0, 0.0, 500.0, 20.0), Hit::Select(1)),
        (Rect::new(400.0, 0.0, 40.0, 20.0), Hit::Pick(1, Pick::Right)),
    ];
    c.click(Point::new(10.0, 10.0));
    assert_eq!(c.current, 1);
    // The pill's chip wins over the row under it.
    c.click(Point::new(410.0, 10.0));
    assert_eq!(c.picks[1], Pick::Right);
    assert_eq!(c.result(), ["a", "b", "c", "D"]);
}

#[test]
fn the_composed_result_is_saved_from_its_file_s_tab() {
    // The server's copy (read-only) on the left, ours on the right: the result goes right.
    let mut input = CompareInput::files(
        ("/p/a.txt".into(), "a\nserver\n".into()),
        ("/p/a.txt".into(), "a\nmine\n".into()),
    );
    input.read_only = [true, false];
    let mut c = Compare::default();
    c.set_data(&input);
    found(&mut c);
    c.merge = true;
    assert!(!c.is_dirty(), "nothing chosen: nothing to save");
    c.set_pick(0, Pick::Both);
    c.save().unwrap();
    // Not written here: the app saves it from the file's tab (announced like any save).
    match c.take_action() {
        Some(ViewerAction::SaveText { path, text }) => {
            assert_eq!(path, PathBuf::from("/p/a.txt"));
            assert_eq!(text, "a\nserver\nmine\n");
        }
        other => panic!("expected the result to save, got {other:?}"),
    }
    // The comparison continues from the saved file (found again on a thread: nothing to
    // save meanwhile).
    assert!(!c.is_dirty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while c.diffing.is_some() && std::time::Instant::now() < deadline {
        c.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!c.is_dirty());
    assert_eq!(c.right, ["a", "server", "mine"]);
}

/// Its actions do what its keys did (next / previous change, the picks in "Compose");
/// the arrows still go from change to change.
#[test]
fn the_actions_do_what_the_keys_did() {
    let mut c = compare("a\nb\nc\nd\n", "a\nB\nc\nD\n");
    assert_eq!(c.current, 0);
    assert!(c.command("next"));
    assert_eq!(c.current, 1);
    assert!(c.command("previous") && c.current == 0);
    assert!(c.edit_key(ViewerKey::Down) && c.current == 1);
    assert!(!c.command("take_left"), "only in Compose");
    c.merge = true;
    assert!(c.command("take_right"));
    assert_eq!(c.picks[1], Pick::Right);
    assert!(!c.key('n'), "a key alone does nothing: the keymap sends the action");
}

/// The differences are found on a thread (the UI doesn't wait): not there at once,
/// `poll` brings them.
#[test]
fn the_differences_are_found_in_the_background() {
    let left: String = (0..3000).map(|i| format!("line {i}\n")).collect();
    let right = left.replace("line 1500\n", "changed\n");
    let mut c = Compare::default();
    c.set_data(&diff::CompareInput::files(("a".into(), left), ("b".into(), right)));
    found(&mut c);
    assert_eq!(c.changes(), 1);
}

/// A window's tabs (one without a file), the files' texts, the comparison opened.
#[derive(Default)]
struct Tabs {
    tabs: Vec<workbench_model::TabInfo>,
    opened: Option<(CompareInput, String)>,
    events: kernel::EventBus,
}

impl Tabs {
    fn new() -> Self {
        let tab = |title: &str, path: Option<&str>| workbench_model::TabInfo {
            id: kernel::DocumentId::next(),
            title: title.into(),
            path: path.map(PathBuf::from),
        };
        Self {
            tabs: vec![
                tab("A.php", Some("/p/A.php")),
                tab("untitled", None),
                tab("B.php", Some("/p/lib/B.php")),
            ],
            ..Self::default()
        }
    }
}

impl WorkbenchHost for Tabs {
    fn project_root(&self) -> Option<PathBuf> {
        None
    }
    fn active_file(&self) -> Option<ActiveFile> {
        None
    }
    fn tabs(&self) -> Vec<workbench_model::TabInfo> {
        self.tabs.clone()
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn project_storage(&self) -> Option<PathBuf> {
        None
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, _: workbench_model::Notification) {}
}

impl workbench_model::ViewHost for Tabs {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
    fn compare(&mut self, input: CompareInput, title: &str) -> bool {
        self.opened = Some((input, title.to_owned()));
        true
    }
}

impl workbench_model::EditorHost for Tabs {
    fn file_text(&self, path: &Path) -> Option<String> {
        Some(format!("text of {}", path.display()))
    }
}

impl workbench_model::RefactoringHost for Tabs {}
impl workbench_model::ServicesHost for Tabs {}
impl workbench_model::SystemHost for Tabs {}

/// A tab's "Compare with ›" lists the other tabs with files; a row compares the two files
/// (the menu's tab on the left).
#[test]
fn a_tab_is_compared_with_another() {
    let mut host = Tabs::new();
    let on = |path: &str| {
        ActionContext::new(MenuPlace::Tab, vec![workbench_model::Target::new(path.into(), false)])
    };
    let rows = OtherTabs.rows(&host, &on("/p/A.php"));
    assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["B.php"]);
    let without = ActionContext::empty(MenuPlace::Tab);
    assert!(OtherTabs.rows(&host, &without).is_empty(), "a tab without a file: nothing");
    assert!(OtherTabs.run(&mut host, &rows[0].argument));
    let (input, title) = host.opened.take().expect("compared");
    assert_eq!(title, "A.php ↔ B.php");
    assert_eq!(input.left, (PathBuf::from("/p/A.php"), "text of /p/A.php".to_owned()));
    assert_eq!(input.right.0, PathBuf::from("/p/lib/B.php"));
    assert!(!OtherTabs.run(&mut host, "1:x"), "a tab that's gone");
}

#[test]
fn the_separators_handles_are_drawn_over_the_columns_and_their_choices() {
    let mut c = compare("a\nb\nc\n", "a\nB\nc\nd\n");
    c.merge = true;
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    c.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 500.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert_eq!(c.splitters.len(), 2);
    for splitter in &c.splitters {
        assert!(splitter.is_on_top(&scene), "{:?}", splitter.bounds());
    }
}

/// The columns' places are the project's: a separator let go keeps them in its sizes, and
/// the next comparison (or the next session's) starts with them.
#[test]
fn the_columns_places_are_kept_in_the_project_s_sizes() {
    let sizes = workbench_model::LayoutSizes::new();
    let mut c = compare("a\n", "b\n");
    c.set_data(&sizes);
    c.scale = 1.0;
    c.list = Rect::new(0.0, 0.0, 1030.0, 500.0);
    c.gutter = 30.0;
    c.columns_w = 1000.0;
    c.drag_separator(0, 315.0);
    c.keep_splits();
    assert_eq!(sizes.get(DIFF_SPLIT), Some(0.3));
    let mut next = compare("x\n", "y\n");
    assert_eq!(next.ends(), [0.5, 1.0]);
    next.set_data(&sizes);
    assert!((next.ends()[0] - 0.3).abs() < 1e-4);
    // "Compose": both of its places, kept only if they're in order.
    sizes.set(MERGE_LEFT, 0.25);
    sizes.set(MERGE_RESULT, 0.6);
    next.set_data(&sizes);
    next.merge = true;
    assert_eq!(next.ends(), [0.25, 0.6]);
    sizes.set(MERGE_RESULT, 0.1);
    let mut wrong = compare("x\n", "y\n");
    wrong.set_data(&sizes);
    wrong.merge = true;
    assert_eq!(wrong.ends(), [1.0 / 3.0, 2.0 / 3.0]);
}

/// A conflicted file: ours changed line 2, theirs line 4 (taken already), both line 6.
fn resolving() -> Compare {
    let mut c = Compare::default();
    c.set_data(&MergeInput {
        result: "/p/a.php".into(),
        base: "1\n2\n3\n4\n5\n6\n".into(),
        ours: "1\nours 2\n3\n4\n5\nours 6\n".into(),
        theirs: "1\n2\n3\ntheirs 4\n5\ntheirs 6\n".into(),
        labels: ["a.php · HEAD".into(), "a.php · feature".into()],
    });
    found(&mut c);
    c
}

#[test]
fn a_conflicted_file_is_resolved_in_three_parts_and_saved_once_none_is_left() {
    let mut c = resolving();
    assert!(c.merge && c.writable(), "always composing, into the file");
    assert_eq!(c.changes(), 3);
    let resolving_state = c.resolving.as_ref().unwrap();
    assert_eq!(resolving_state.conflicts, [false, false, true]);
    assert_eq!(resolving_state.left(), 1);
    // Each side's change alone is in the result already; the conflict has neither.
    assert_eq!(c.result(), ["1", "ours 2", "3", "theirs 4", "5"]);
    assert_eq!(c.current, 2, "straight to the conflict");
    assert!(!c.can_save(), "a conflict is left");
    assert_eq!(c.status().unwrap(), tr!("compare.conflicts_left", count = 1));
    // "All from the left" picks only the conflicts: theirs' change alone stays.
    c.pick_all(Pick::Left);
    assert_eq!(c.result(), ["1", "ours 2", "3", "theirs 4", "5", "ours 6"]);
    assert!(c.can_save() && c.is_dirty());
    c.save().unwrap();
    assert_eq!(
        c.take_action(),
        Some(ViewerAction::SaveText {
            path: "/p/a.php".into(),
            text: "1\nours 2\n3\ntheirs 4\n5\nours 6\n".into()
        })
    );
    assert!(!c.can_save() && !c.is_dirty(), "written");
    assert_eq!(c.status().unwrap(), t("compare.resolved_written"));
}

#[test]
fn a_conflict_s_choice_toggles_back_to_none_and_the_mode_stays_composing() {
    let mut c = resolving();
    c.set_pick(2, Pick::Both);
    assert_eq!(c.result()[5..], ["ours 6", "theirs 6"]);
    // Both again: back to how it started (neither), still chosen.
    c.set_pick(2, c.initial_pick(2));
    assert_eq!(c.picks[2], Pick::None);
    assert_eq!(c.initial_pick(0), Pick::Left);
    assert_eq!(c.initial_pick(1), Pick::Right);
    assert!(!c.command("merge"), "no plain comparison while resolving");
    assert!(c.merge);
    // A plain comparison given after: no longer resolving.
    c.set_data(&CompareInput::files(("/a".into(), "a\n".into()), ("/b".into(), "b\n".into())));
    found(&mut c);
    assert!(c.resolving.is_none());
}

/// Written by hand, the result is saved whatever the conflicts' choices; back to the choices,
/// they count again.
#[test]
fn a_result_written_by_hand_is_saved_as_written() {
    let mut c = resolving();
    assert!(!c.can_save(), "a conflict is left");
    c.toggle_by_hand();
    assert!(c.wants_text() && c.can_save());
    assert_eq!(c.by_hand().unwrap().text(), "1\nours 2\n3\ntheirs 4\n5");
    c.by_hand().unwrap().select_all();
    assert_eq!(
        ui_components::Viewer::selected_text(&c).as_deref(),
        Some("1\nours 2\n3\ntheirs 4\n5"),
        "the field's selection is what's copied"
    );
    c.text_input("mine");
    c.save().unwrap();
    assert_eq!(
        c.take_action(),
        Some(ViewerAction::SaveText { path: "/p/a.php".into(), text: "mine\n".into() })
    );
    let mut c = resolving();
    c.toggle_by_hand();
    c.toggle_by_hand();
    assert!(!c.wants_text() && !c.can_save());
    assert_eq!(ui_components::Viewer::selected_text(&c), None, "closed: the columns'");
}

/// A line with tabs: its changed part lit where it's drawn (each tab shown as four spaces),
/// not where it is in the line's bytes.
#[test]
fn a_change_after_tabs_is_lit_where_it_is_drawn() {
    let (left, right) = ("\t\tx = 1;", "\t\tx = 2;");
    let (range, _) = diff::inline_change(left, right);
    let shown = crate::paint::tab_shifted(left, range).unwrap();
    let drawn = left.replace('\t', "    ");
    assert_eq!(&drawn[shown], "1");
    assert_eq!(crate::paint::tab_shifted("abc", 2..9), None);
}

/// `c` painted in a 1200×500 view.
fn paint(c: &mut Compare) {
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    c.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 500.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// Each column scrolls across on its own (the one under the mouse), up to its longest
/// line's end, with its own bar; a column of short lines has neither.
#[test]
fn each_column_scrolls_across_on_its_own() {
    let long = format!("<?php echo '{}';\n", "x".repeat(400));
    let mut c = compare(&format!("a\n{long}"), "b\nshort\n");
    paint(&mut c);
    let (left, right) = (c.across[0].area, c.across[2].area);
    let mid =
        |r: kernel::Rect| kernel::Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + 40.0);
    assert!(c.across[0].max > 0.0 && c.across[0].bar.is_some(), "the long side: a bar");
    assert_eq!(c.across[2].max, 0.0);
    assert!(c.across[2].bar.is_none(), "the short side: none");
    // Over the left: only it moves.
    c.hover(mid(left));
    assert!(c.scroll(120.0, 0.0));
    assert_eq!((c.across[0].offset, c.across[2].offset, c.scroll), (120.0, 0.0, 0.0));
    assert!(c.scroll(1e6, 0.0));
    assert_eq!(c.across[0].offset, c.across[0].max, "up to its end");
    // Over the right: nowhere to go, the left stays.
    c.hover(mid(right));
    assert!(!c.scroll(50.0, 0.0));
    assert_eq!(c.across[0].offset, c.across[0].max);
    // The left's bar dragged back to its start.
    let track = c.across[0].bar.as_ref().unwrap().track_rect();
    let y = track.min_y() + track.size.height / 2.0;
    c.click(kernel::Point::new(track.min_x() + 2.0, y));
    c.drag(kernel::Point::new(track.min_x() - 50.0, y));
    assert!(c.release());
    assert!(c.across[0].offset < c.across[0].max * 0.1, "{}", c.across[0].offset);
}

/// The overview's thumb dragged: taken where it's pressed, the rows following the mouse;
/// pressed elsewhere: centred there and dragged on; let go: the drag ends.
#[test]
fn the_overview_s_thumb_is_dragged() {
    let lines: String = (0..400).map(|i| format!("line {i}\n")).collect();
    let mut c = compare(&lines, &lines.replace("line 200", "LINE 200"));
    paint(&mut c);
    c.scroll = 0.0;
    paint(&mut c);
    let thumb = c.overview_thumb();
    let x = thumb.min_x() + thumb.size.width / 2.0;
    // Taken 5 px into it, moved 100 px down: its top 100 px lower.
    c.click(kernel::Point::new(x, thumb.min_y() + 5.0));
    assert!(c.drag(kernel::Point::new(x, thumb.min_y() + 105.0)));
    let moved = c.overview_thumb();
    assert!((moved.min_y() - (thumb.min_y() + 100.0)).abs() < 1.0, "{moved:?} from {thumb:?}");
    assert!(c.release());
    assert!(!c.drag(kernel::Point::new(x, thumb.min_y() + 300.0)), "let go: no longer moving");
    // Pressed below it: centred there, then dragged on.
    let o = c.overview;
    let at = o.min_y() + o.size.height * 0.75;
    c.click(kernel::Point::new(x, at));
    let centred = c.overview_thumb();
    assert!((centred.min_y() + centred.size.height / 2.0 - at).abs() < 1.0, "{centred:?}");
    assert!(c.drag(kernel::Point::new(x, o.max_y() + 500.0)));
    assert!(c.release());
    let max = c.rows.len() as f32 * c.row_h - c.list.size.height + c.row_h;
    assert_eq!(c.scroll, max.max(0.0), "kept within its end");
}

/// Hovered, a column's bar only thickens its thumb (no wide rail under it): whole inside
/// its track (nothing of its rounded ends cut by the track's edges).
#[test]
fn a_hovered_bar_s_thumb_fits_its_track() {
    let long = format!("<?php echo '{}';\n", "x".repeat(400));
    let mut c = compare(&format!("a\n{long}"), &format!("b\n{long}"));
    paint(&mut c);
    let track = c.across[0].bar.as_ref().unwrap().track_rect();
    let on = kernel::Point::new(track.min_x() + 4.0, track.min_y() + track.size.height / 2.0);
    c.hover(on);
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    c.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 500.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let bar = scene.layers().iter().find(|l| l.clip == Some(track)).expect("the bar's layer");
    // The thin line and the thumb: no rail.
    assert!(!bar.quads.is_empty() && bar.quads.iter().all(|q| q.border_width == 0.0), "no rail");
    for q in &bar.quads {
        let b = q.bounds;
        assert!(
            b.min_y() >= track.min_y() && b.max_y() <= track.max_y(),
            "{b:?} outside {track:?}"
        );
    }
}

/// A separator over a difference's rows (a file empty on one side: one difference, as tall
/// as the columns) is grabbed there and dragged, never a selection started under it.
#[test]
fn a_separator_over_a_difference_is_dragged() {
    let lines: String = (0..40).map(|i| format!("line {i}\n")).collect();
    let mut c = compare("", &lines);
    paint(&mut c);
    let bar = c.splitters[0].bounds();
    let at = kernel::Point::new(bar.center().x, c.list.min_y() + c.row_h * 5.5);
    assert!(
        c.hits.iter().any(|(r, h)| matches!(h, Hit::Select(_)) && r.contains(at)),
        "the difference's rows reach under the separator"
    );
    let _ = c.pointer_down(at, 1, ui_components::PressMods::default());
    assert!(c.splitters[0].is_dragging(), "grabbed");
    assert!(!c.selecting, "no selection started");
    let before = bar.center().x;
    c.drag(kernel::Point::new(before - 120.0, at.y));
    c.release();
    paint(&mut c);
    assert!(c.splitters[0].bounds().center().x < before - 60.0, "moved");
}

/// A column with a line longer than its room: its bar across, along its bottom, has a thumb
/// that pressed and dragged right scrolls the code across, through the view's own press,
/// drag and release.
#[test]
fn a_column_s_thumb_across_is_dragged() {
    let long = "x".repeat(600);
    let mut c = compare(&format!("{long}\nb\n"), "a\nb\n");
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    c.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 500.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let bar = c.across[0].bar.as_ref().expect("the left column's bar across");
    let thumb = bar.thumb_rect(c.across[0].metrics()).expect("its thumb");
    let at = thumb.center();
    c.hover(at);
    assert!(c.click(at).is_none());
    assert!(c.drag(kernel::Point::new(at.x + 80.0, at.y)));
    assert!(c.across[0].offset > 0.0, "the code scrolled across");
    assert!(c.release());
    assert!(!c.drag(kernel::Point::new(at.x + 160.0, at.y)), "let go: the moves don't reach it");
}

/// While the result is written by hand its field keeps the keys, even after a press on a
/// column: the text and the edit commands are its, Tab moves nothing, the comparison's own
/// keys and actions wait; closed, they're the comparison's again.
#[test]
fn the_field_written_by_hand_keeps_the_keys_while_open() {
    let mut c = resolving();
    c.toggle_by_hand();
    paint(&mut c);
    let left = Point::new(c.list.min_x() + 80.0, c.list.min_y() + c.row_h * 0.5);
    let _ = c.pointer_down(left, 1, ui_components::PressMods::default());
    c.release();
    c.by_hand().unwrap().select_all();
    assert!(c.text_input("mine"));
    assert_eq!(c.by_hand().unwrap().text(), "mine");
    assert!(!c.edit_key_with(ViewerKey::Tab, ui_components::KeyReach::default()));
    assert!(c.edit_key_with(ViewerKey::Enter, ui_components::KeyReach::default()));
    assert_eq!(c.by_hand().unwrap().text(), "mine\n");
    assert!(c.command("undo"), "the field's");
    assert!(!c.command("next"), "the comparison's actions wait");
    assert!(!c.select_all(), "⌘A is the field's");
    c.toggle_by_hand();
    assert!(!c.text_input("x"), "closed: nothing takes the text");
    assert!(c.command("next"));
}

/// A button on a separator wins over it: pressed, the button acts and nothing is held; the
/// mouse over it shows the hand and leaves the separator unlit. Beside it, the separator is
/// grabbed, its resize cursor shown wherever the pointer goes until let go.
#[test]
fn a_button_on_a_separator_wins_and_the_separator_is_held_beside_it() {
    let mut c = compare("a\nb\nc\nd\n", "a\nB\nc\nD\n");
    paint(&mut c);
    let bar = c.splitters[0].bounds();
    let button = Rect::new(bar.min_x(), bar.min_y(), bar.size.width, 20.0);
    c.hits = vec![(button, Hit::Next)];
    let on = button.center();
    c.hover(on);
    assert_eq!(c.cursor_at(on), Some(ViewerCursor::Pointer));
    assert!(!c.splitters[0].is_hovered(), "unlit under a button");
    let _ = c.pointer_down(on, 1, ui_components::PressMods::default());
    assert!(!c.splitters[0].is_dragging(), "the button took it");
    let beside = Point::new(bar.center().x, bar.max_y() - 20.0);
    c.hover(beside);
    assert!(c.splitters[0].is_hovered());
    assert_eq!(c.cursor_at(beside), Some(ViewerCursor::ResizeHorizontal));
    let _ = c.pointer_down(beside, 1, ui_components::PressMods::default());
    assert!(c.splitters[0].is_dragging());
    let far = Point::new(beside.x - 100.0, beside.y);
    assert!(c.drag(far));
    assert_eq!(c.cursor_at(far), Some(ViewerCursor::ResizeHorizontal), "held: wherever it goes");
    assert!(c.release());
    assert!(!c.splitters[0].is_dragging());
}

/// The wheel over the field written by hand scrolls only it, never the rows behind.
#[test]
fn the_wheel_over_the_field_written_by_hand_is_its_own() {
    let mut c = resolving();
    c.toggle_by_hand();
    paint(&mut c);
    let between = (c.splitters[0].bounds().center().x + c.splitters[1].bounds().center().x) / 2.0;
    let field = Point::new(between, c.list.center().y);
    assert!(c.by_hand().unwrap().contains(field));
    c.hover(field);
    assert!(c.scroll(0.0, 400.0));
    assert_eq!(c.scroll, 0.0, "the rows stayed");
}

/// The toolbar's buttons are the shared ones and keep their behavior: the steps and the
/// mode's two segments are hit, Save is pressable only once something can be saved, and
/// "By hand" is a toggle that stays hit while on.
#[test]
fn the_toolbar_s_buttons_are_hit_as_drawn() {
    let area = |c: &Compare, hit: Hit| c.hits.iter().find(|(_, h)| *h == hit).map(|(r, _)| *r);
    let mut c = compare("a\nb\n", "a\nc\n");
    paint(&mut c);
    let prev = area(&c, Hit::Prev).expect("previous");
    let next = area(&c, Hit::Next).expect("next");
    assert_eq!(prev.size.width, ui_components::ds::button_size::MEDIUM);
    assert!(next.min_x() > prev.max_x(), "side by side");
    let diff = area(&c, Hit::Mode(false)).expect("the mode's first segment");
    let merge = area(&c, Hit::Mode(true)).expect("its second");
    assert!(merge.min_x() >= diff.max_x(), "in order");
    let mut c = resolving();
    paint(&mut c);
    assert!(area(&c, Hit::Save).is_none(), "a conflict is left: Save can't be pressed");
    assert!(area(&c, Hit::ByHand).is_some());
    c.toggle_by_hand();
    paint(&mut c);
    assert!(area(&c, Hit::ByHand).is_some(), "on: still a toggle");
    assert!(area(&c, Hit::Save).is_some(), "written by hand: saved as written");
}

/// Word wrap on: a long line takes several rows on its side (the row as tall on both), no
/// scroll across; a press on its second row lands past where that row starts.
#[test]
fn long_lines_wrap_in_their_column() {
    let long = "the quick brown fox jumps over the lazy dog ".repeat(6);
    let mut c = compare(&format!("{long}\nshort\n"), &format!("{long}x\nshort\n"));
    c.set_text_options(&ui_components::TextOptions { word_wrap: true });
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |c: &mut Compare| {
        let mut scene = render_api::Scene::default();
        c.paint(ViewerPaint {
            bounds: kernel::Rect::new(0.0, 0.0, 700.0, 500.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut c);
    assert!(c.wraps());
    assert!(c.row_height(0) > c.row_h, "the long line's row is taller");
    assert_eq!(c.row_top(1), c.row_height(0), "the next row under all of it");
    assert!(c.across.iter().all(|a| a.max == 0.0), "nothing scrolls across");
    let second = kernel::Point::new(c.code_x[0] + 2.0, c.list.min_y() + c.row_h * 1.5);
    let at = c.text_pos(0, second);
    assert_eq!(at.row, 0);
    assert!(at.col > 10, "on the second row of the line: {}", at.col);
    // Off: one row each, as before.
    c.set_text_options(&ui_components::TextOptions { word_wrap: false });
    paint(&mut c);
    assert_eq!(c.row_height(0), c.row_h);
}
