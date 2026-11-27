use super::menu::*;
use super::*;

/// The message is a text field of several lines: the app's ⌘A (every line) / ⌘X / ⌘Z reach
/// it, typing replaces what's selected, ⌥ / Ctrl move by word, ⇧ selects, ↵ breaks a line,
/// a paste keeps its lines.
#[test]
fn the_message_takes_the_edit_commands() {
    let mut view = CommitView::default();
    assert!(view.wants_text());
    view.text_input("fix the cart");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("fix the cart"));
    view.text_input("Fix");
    assert_eq!(view.message.text(), "Fix");
    assert!(view.command("undo"));
    assert_eq!(view.message.text(), "fix the cart");
    view.edit_key(ViewerKey::End);
    let word = ui_components::KeyReach { word: true, extend: true };
    view.edit_key_with(ViewerKey::Left, word);
    assert_eq!(view.selected_text().as_deref(), Some("cart"));
    assert!(view.command("cut"));
    assert_eq!(view.message.text(), "fix the ");
    assert!(view.paste("totals\nand more"));
    assert_eq!(view.message.text(), "fix the totals\nand more");
    view.edit_key(ViewerKey::Enter);
    view.text_input("x");
    assert_eq!(view.message.text(), "fix the totals\nand more\nx", "↵: a new line");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("fix the totals\nand more\nx"));
}

/// The files' scrollbar: hidden at rest, shown when the list scrolls.
#[test]
fn the_files_scrollbar_shows_while_scrolled() {
    let files = (0..60)
        .map(|i| StagedFile {
            path: format!("f{i}.php"),
            staged: Some(FileState::Modified),
            unstaged: None,
        })
        .collect();
    let mut view = CommitView { files, ..CommitView::default() };
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |view: &mut CommitView| {
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 900.0, 600.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut render_api::Scene::default(),
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut view);
    assert!(view.frame_deadline().is_none(), "hidden at rest: no frames");
    view.pointer = view.panes[STAGED].origin;
    assert!(view.scroll(0.0, 200.0));
    paint(&mut view);
    assert!(view.frame_deadline().is_some(), "shown while scrolled");
    let later = std::time::Instant::now() + std::time::Duration::from_secs(5);
    assert_eq!(view.bars[STAGED].opacity_at(later), 0.0);
}

/// The files' scrollbar shown: its thumb pressed and dragged down scrolls the list, and a
/// divider pressed and dragged moves it, both through the view's own press, drag, release.
#[test]
fn the_files_thumb_and_a_divider_are_dragged() {
    let files = (0..60)
        .map(|i| StagedFile {
            path: format!("f{i}.php"),
            staged: Some(FileState::Modified),
            unstaged: None,
        })
        .collect();
    let mut view = CommitView { files, ..CommitView::default() };
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |view: &mut CommitView| {
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 900.0, 600.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut render_api::Scene::default(),
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut view);
    // A hidden bar shows once the list scrolls; back at the top after.
    let pane = view.panes[STAGED];
    view.pointer = pane.origin;
    assert!(view.scroll(0.0, 1.0));
    paint(&mut view);
    assert!(view.scroll(0.0, -1.0));
    paint(&mut view);
    let held = (0..20).find_map(|dy| {
        (0..16).find_map(|dx| {
            let at =
                Point::new(pane.max_x() - 1.0 - dx as f32, pane.min_y() + 4.0 + dy as f32 * 4.0);
            view.press(at, 1);
            if view.bars[STAGED].is_dragging() {
                return Some(at);
            }
            view.release();
            None
        })
    });
    let at = held.expect("the thumb found");
    assert!(view.drag(Point::new(at.x, at.y + 80.0)));
    assert!(view.scrolls[STAGED] > 0.0, "the files scrolled");
    // Held over the message: the bar's cursor, never the text's.
    let over = view
        .hits
        .iter()
        .find(|(_, h)| matches!(h, Hit::Message))
        .map(|(r, _)| r.center())
        .expect("the message");
    view.hover(over);
    assert_eq!(view.cursor_at(over), Some(ui_components::scrollbar::CURSOR), "held: the bar's");
    assert!(view.release());
    assert!(!view.bars[STAGED].is_dragging());
    // The divider between the lists and the changes: dragged to the right.
    let before = view.panes[DIFF].min_x();
    let y = pane.min_y() + pane.size.height / 2.0;
    let grip = (0..60)
        .map(|dx| Point::new(pane.max_x() + dx as f32, y))
        .find(|p| view.dividers.0[0].cursor(*p).is_some())
        .expect("the divider beside the lists");
    view.press(grip, 1);
    assert!(view.drag(Point::new(grip.x + 60.0, y)));
    assert!(view.release(), "it was dragged");
    paint(&mut view);
    assert!(view.panes[DIFF].min_x() > before, "the changes start further right");
}

/// The draft stays with the repository: another commit view finds it, until it's committed.
#[test]
fn the_draft_is_kept_until_committed() {
    let repo: std::sync::Arc<dyn Repository> = std::sync::Arc::new(crate::fake::FakeRepository {
        // Drafts are per repository: its own, whatever the other tests commit.
        root: Some("/draft-kept".into()),
        ..crate::fake::FakeRepository::default()
    });
    let mut first = CommitView::default();
    first.set_data(&CommitData { repo: repo.clone(), project: None, docked: false });
    first.text_input("Subject\n\nBody");
    let mut second = CommitView::default();
    second.set_data(&CommitData { repo: repo.clone(), project: None, docked: false });
    assert_eq!(second.message.text(), "Subject\n\nBody");
    drafts().remove(repo.root());
}

fn settle(view: &mut CommitView) {
    for _ in 0..200 {
        if !view.poll() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Amend: the last commit's message brought in (a warning when it was pushed), the commit
/// made again with it; turned off, the message as it was.
#[test]
fn amending_brings_the_last_message_in() {
    let fake = std::sync::Arc::new(crate::fake::FakeRepository {
        // Its own repository: its draft ("draft") never reaches the other tests' messages
        // (drafts are kept per repository, for the whole process).
        root: Some("/amend-draft".into()),
        last_message: Some("Fix the cart\n\nWhy".into()),
        pushed: true,
        ..Default::default()
    });
    let repo: std::sync::Arc<dyn Repository> = fake.clone();
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo, project: None, docked: false });
    settle(&mut view);
    view.text_input("draft");
    view.set_amend(true);
    settle(&mut view);
    assert_eq!(view.message.text(), "Fix the cart\n\nWhy");
    assert!(view.amend_pushed, "it was pushed: a warning");
    view.set_amend(false);
    assert_eq!(view.message.text(), "draft", "back as it was");
    view.set_amend(true);
    settle(&mut view);
    view.message.select_all();
    view.text_input("Fix the cart, better");
    // Nothing staged: only the message changes.
    assert!(view.command("commit"));
    settle(&mut view);
    assert_eq!(
        *fake.amended.lock().unwrap(),
        [(Vec::<String>::new(), "Fix the cart, better".to_owned())]
    );
    assert!(!view.amend, "done: amending ends");
    drafts().clear();
}

fn with_a_staged_change() -> (Arc<crate::fake::FakeRepository>, CommitView) {
    let repo = Arc::new(crate::fake::FakeRepository {
        committed_texts: vec![("a.php".into(), "one\ntwo\n".into())],
        index_texts: vec![("a.php".into(), "one\nTWO\n".into())],
        ..crate::fake::FakeRepository::default()
    });
    repo.staged.lock().unwrap().push("a.php".into());
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo: repo.clone(), project: None, docked: false });
    settle(&mut view);
    (repo, view)
}

fn shown(view: &mut CommitView) {
    let i = view.list(true)[0];
    view.select(i, true);
    settle(view);
}

#[test]
fn a_file_is_on_one_list_or_the_other_and_moves_between_them() {
    let (repo, mut view) = with_a_staged_change();
    assert_eq!(view.staged_paths(), ["a.php"]);
    let unstaged = view.list(false).len();
    view.toggle(view.list(true)[0], true);
    settle(&mut view);
    assert!(view.staged_paths().is_empty());
    assert_eq!(view.list(false).len(), unstaged + 1);
    assert!(repo.staged.lock().unwrap().is_empty());
}

#[test]
fn a_staged_file_shows_what_changed_since_the_last_commit_in_hunks() {
    let (_, mut view) = with_a_staged_change();
    shown(&mut view);
    let diff = view.diff.as_ref().expect("its changes");
    assert_eq!(diff.hunks.len(), 1);
    assert_eq!(diff.rows[0], DiffRow::Hunk(0));
    assert_eq!(diff.rows.len(), 1 + diff.hunks[0].lines.len());
}

#[test]
fn a_staged_hunk_is_taken_back_from_the_staging_area() {
    let (repo, mut view) = with_a_staged_change();
    shown(&mut view);
    view.toggle_hunk(0);
    settle(&mut view);
    let patches = repo.patches.lock().unwrap();
    assert_eq!(patches.len(), 1);
    assert!(patches[0].0.contains("-two\n+TWO\n"), "{}", patches[0].0);
    assert_eq!((patches[0].1, patches[0].2), (PatchTarget::Staged, true));
}

#[test]
fn nothing_is_thrown_away_before_the_user_answers() {
    let (repo, mut view) = with_a_staged_change();
    shown(&mut view);
    let action = view.discard_hunk(0);
    assert!(matches!(action, Some(ViewerAction::WithHost(_))));
    assert!(repo.patches.lock().unwrap().is_empty());
    // The dialog being open asks for no frames; "no" leaves nothing behind.
    assert!(!view.poll());
    drop(action);
    view.poll();
    assert!(view.act_rx.is_none());
}

#[test]
fn two_clicks_on_a_row_open_its_file_unless_it_was_deleted() {
    let (_, mut view) = with_a_staged_change();
    let i = view.list(true)[0];
    let row = Rect::new(0.0, 0.0, 100.0, 20.0);
    view.hits = vec![(row, Hit::Row(i, true))];
    let inside = Point::new(5.0, 5.0);
    let Some(ViewerAction::OpenFile { path, .. }) = view.open_at(inside) else {
        panic!("its file opened")
    };
    assert!(path.ends_with(&view.files[i].path));
    view.files[i].staged = Some(FileState::Deleted);
    assert!(view.open_at(inside).is_none());
}

#[test]
fn a_reset_takes_the_file_off_the_staging_area_and_throws_its_changes_away() {
    let repo = crate::fake::FakeRepository::default();
    repo.staged.lock().unwrap().push("a.php".into());
    reset(&repo, &["a.php".to_owned()], true).unwrap();
    assert!(repo.staged.lock().unwrap().is_empty());
    assert_eq!(*repo.discarded.lock().unwrap(), ["a.php"]);
}

#[test]
fn the_menu_of_a_file_offers_what_its_list_can_do() {
    let (_, view) = with_a_staged_change();
    let ids = |staged, i| -> Vec<String> {
        view.file_menu(i, staged)
            .unwrap()
            .into_iter()
            .filter_map(|row| match row {
                ViewMenuEntry::Item { id, .. } => Some(id),
                _ => None,
            })
            .collect()
    };
    let on_stage = ids(true, view.list(true)[0]);
    assert!(on_stage.contains(&format!("{UNSTAGE_FILE}a.php")));
    assert!(on_stage.contains(&format!("{RESET_FILE}a.php")));
    assert!(!on_stage.contains(&format!("{DISCARD_FILE}a.php")));
    let other = ids(false, view.list(false)[0]);
    assert!(other.iter().any(|id| id.starts_with(STAGE_FILE)));
    assert!(other.iter().any(|id| id.starts_with(DISCARD_FILE)));
}

#[test]
fn a_reset_asked_from_the_menu_waits_for_the_answer() {
    let (repo, mut view) = with_a_staged_change();
    assert!(view.command(&format!("{RESET_FILE}a.php")));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
    assert!(repo.discarded.lock().unwrap().is_empty());
    assert_eq!(repo.staged.lock().unwrap().len(), 1);
}

fn menu_ids(view: &CommitView, staged: bool, i: usize) -> Vec<String> {
    let rows = view.file_menu(i, staged).unwrap().into_iter();
    rows.filter_map(|row| match row {
        ViewMenuEntry::Item { id, .. } => Some(id),
        _ => None,
    })
    .collect()
}

#[test]
fn a_file_with_a_past_can_be_annotated_and_reset_to_a_commit_a_new_one_cannot() {
    let (_, mut view) = with_a_staged_change();
    let i = view.list(true)[0];
    let ids = menu_ids(&view, true, i);
    assert!(ids.iter().any(|id| id.starts_with(ANNOTATE_FILE)));
    assert!(ids.iter().any(|id| id.starts_with(RESET_TO_COMMIT)));
    view.files[i].staged = Some(FileState::Added);
    let ids = menu_ids(&view, true, i);
    assert!(!ids.iter().any(|id| id.starts_with(ANNOTATE_FILE)));
    assert!(!ids.iter().any(|id| id.starts_with(RESET_TO_COMMIT)));
}

#[test]
fn the_commits_are_read_first_and_nothing_changes_before_one_is_chosen() {
    let repo = Arc::new(crate::fake::FakeRepository {
        file_history: vec![crate::fake::commit("abc", "First")],
        ..crate::fake::FakeRepository::default()
    });
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo: repo.clone(), project: None, docked: false });
    settle(&mut view);
    assert!(view.command(&format!("{RESET_TO_COMMIT}a.php")));
    assert!(view.take_action().is_none(), "nothing on the UI thread");
    settle(&mut view);
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
    assert!(repo.discarded.lock().unwrap().is_empty());
}

#[test]
fn a_file_without_commits_offers_nothing_and_leaves_nothing_waiting() {
    let (_, mut view) = with_a_staged_change();
    assert!(view.command(&format!("{RESET_TO_COMMIT}a.php")));
    settle(&mut view);
    assert!(view.take_action().is_none());
    assert!(view.act_rx.is_none());
}

fn three_files() -> CommitView {
    let files = ["a", "b", "c"]
        .map(|path| StagedFile {
            path: path.to_owned(),
            staged: None,
            unstaged: Some(FileState::Modified),
        })
        .to_vec();
    let mut view = CommitView { files, ..CommitView::default() };
    view.select(0, false);
    view
}

fn chosen(view: &CommitView) -> Vec<&str> {
    view.marked.iter().map(String::as_str).collect()
}

#[test]
fn shift_chooses_a_run_and_a_plain_click_goes_back_to_one() {
    let mut view = three_files();
    view.mods = ui_components::PressMods { shift: true, ..Default::default() };
    view.pick(2, false);
    assert_eq!(chosen(&view), ["a", "b", "c"]);
    assert_eq!(view.selected.as_ref().unwrap().0, "c");
    view.pick(1, false);
    assert_eq!(chosen(&view), ["b"]);
}

#[test]
fn primary_adds_and_takes_one_never_the_last() {
    let mut view = three_files();
    let primary = ui_components::PressMods { primary: true, ..Default::default() };
    view.mods = primary;
    view.pick(2, false);
    assert_eq!(chosen(&view), ["a", "c"]);
    view.mods = primary;
    view.pick(0, false);
    assert_eq!(chosen(&view), ["c"]);
    view.mods = primary;
    view.pick(2, false);
    assert_eq!(chosen(&view), ["c"]);
}

#[test]
fn what_is_done_on_one_of_those_chosen_reaches_them_all_and_only_then() {
    let mut view = three_files();
    view.mods = ui_components::PressMods { shift: true, ..Default::default() };
    view.pick(1, false);
    assert_eq!(view.reached(0, false), ["a", "b"]);
    assert_eq!(view.reached(2, false), ["c"]);
    let rows = menu_ids(&view, false, 0);
    assert!(rows.contains(&format!("{STAGE_FILE}a\0b")));
    assert!(!rows.iter().any(|id| id.starts_with(OPEN_FILE)));
}

/// Docked in the Git window (a bottom window's press): ⇧ and ⌘ choose there too.
#[test]
fn a_press_in_the_git_window_keeps_its_modifiers() {
    let mut view = three_files();
    let row = Rect::new(0.0, 40.0, 200.0, 20.0);
    view.hits = vec![(row, Hit::Row(2, false))];
    let shift = ui_components::PressMods { shift: true, ..Default::default() };
    view.press_with(Point::new(10.0, 50.0), 1, shift);
    assert_eq!(chosen(&view), ["a", "b", "c"]);
}

/// ⇧↓ / ⇧↑ in the lists: the run grows and shrinks from where it started; the menu then
/// reaches them all (copying their paths, one per line).
#[test]
fn shift_and_the_arrows_choose_a_run_from_the_keyboard() {
    let mut view = three_files();
    view.set_keys(Keys::Lists);
    let extend = ui_components::KeyReach { extend: true, ..Default::default() };
    view.edit_key_with(ViewerKey::Down, extend);
    view.edit_key_with(ViewerKey::Down, extend);
    assert_eq!(chosen(&view), ["a", "b", "c"]);
    view.edit_key_with(ViewerKey::Up, extend);
    assert_eq!(chosen(&view), ["a", "b"]);
    assert_eq!(view.selected.as_ref().unwrap().0, "b");
    let rows = menu_ids(&view, false, 0);
    assert!(rows.contains(&format!("{DISCARD_FILE}a\0b")), "{rows:?}");
    assert!(view.command(&format!("{COPY_PATH}a\0b")));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText(text))) if text == "a\nb"
    ));
}

/// ⇧↑ on the first file changes nothing (its changes not read again); a ⌘-click taking a
/// file out moves the run's start to the shown one.
#[test]
fn the_list_s_ends_and_a_file_taken_out_keep_the_run_right() {
    let mut view = three_files();
    view.set_keys(Keys::Lists);
    view.diff_rx = None;
    let extend = ui_components::KeyReach { extend: true, ..Default::default() };
    view.edit_key_with(ViewerKey::Up, extend);
    assert!(view.diff_rx.is_none(), "nothing read at the list's end");
    assert_eq!(chosen(&view), ["a"]);
    let primary = ui_components::PressMods { primary: true, ..Default::default() };
    view.mods = primary;
    view.pick(2, false);
    view.mods = primary;
    view.pick(2, false);
    assert_eq!(view.anchor.as_deref(), Some("a"));
    view.edit_key_with(ViewerKey::Down, extend);
    assert_eq!(chosen(&view), ["a", "b"]);
}

#[test]
fn a_file_gone_from_the_list_is_no_longer_chosen() {
    let mut view = three_files();
    view.mods = ui_components::PressMods { shift: true, ..Default::default() };
    view.pick(2, false);
    view.files.remove(1);
    view.keep_marked();
    assert_eq!(chosen(&view), ["a", "c"]);
}

#[test]
fn a_reset_of_several_is_asked_once_for_all() {
    let (repo, mut view) = with_a_staged_change();
    assert!(view.command(&format!("{RESET_FILE}a.php\0b.php")));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
    assert!(repo.discarded.lock().unwrap().is_empty());
}

#[test]
fn the_changes_scroll_sideways_only_as_far_as_their_widest_line() {
    let mut view = CommitView { diff_overflow: 100.0, ..CommitView::default() };
    view.panes[DIFF] = Rect::new(0.0, 0.0, 200.0, 200.0);
    view.pointer = Point::new(10.0, 10.0);
    assert!(view.scroll(60.0, 0.0));
    assert_eq!(view.diff_x, 60.0);
    assert!(view.scroll(500.0, 0.0));
    assert_eq!(view.diff_x, 100.0, "no further than the widest line");
    assert!(!view.scroll(10.0, 0.0));
    view.scroll(-500.0, 0.0);
    assert_eq!(view.diff_x, 0.0);
}

#[test]
fn lines_chosen_are_what_the_hunk_s_action_moves_then_all_of_it_again() {
    let (repo, mut view) = with_a_staged_change();
    shown(&mut view);
    let whole = view.hunk_patch(0, true).unwrap();
    let removed = view.diff.as_ref().unwrap().hunks[0]
        .lines
        .iter()
        .position(|line| line.kind == LineKind::Removed)
        .unwrap();
    view.pick_line(0, removed);
    let some = view.hunk_patch(0, true).unwrap();
    assert_ne!(some, whole);
    view.toggle_hunk(0);
    settle(&mut view);
    assert_eq!(repo.patches.lock().unwrap()[0].0, some);
    assert!(view.lines.is_empty(), "the list read again: places start over");
    assert_eq!(view.hunk_patch(0, true).unwrap(), whole);
}

#[test]
fn shift_chooses_the_changed_lines_of_a_run_and_never_context() {
    let (_, mut view) = with_a_staged_change();
    shown(&mut view);
    let kinds: Vec<LineKind> =
        view.diff.as_ref().unwrap().hunks[0].lines.iter().map(|line| line.kind).collect();
    let first = kinds.iter().position(|k| *k != LineKind::Context).unwrap();
    let last = kinds.iter().rposition(|k| *k != LineKind::Context).unwrap();
    view.pick_line(0, first);
    view.mods = ui_components::PressMods { shift: true, ..Default::default() };
    view.pick_line(0, last);
    assert!(view.lines.iter().all(|(_, l)| kinds[*l] != LineKind::Context));
    assert_eq!(view.lines.len(), last - first + 1);
}

#[test]
fn what_can_be_pressed_shows_a_hand_and_the_message_a_text_cursor() {
    let mut view = CommitView::default();
    let at = Point::new(5.0, 5.0);
    let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
    view.hits = vec![(rect, Hit::Refresh)];
    assert_eq!(view.cursor_at(at), Some(ViewerCursor::Pointer));
    view.hits = vec![(rect, Hit::Message)];
    assert_eq!(view.cursor_at(at), Some(ViewerCursor::Text));
    view.hits = vec![(rect, Hit::Commit)];
    assert_eq!(view.cursor_at(at), Some(ViewerCursor::Arrow), "nothing to commit");
}

#[test]
fn opened_again_in_a_split_it_shows_the_same_repository() {
    let mut view = CommitView::default();
    assert!(view.mirror_data().is_none(), "nothing to show yet");
    let repo = Arc::new(crate::fake::FakeRepository::default());
    view.set_data(&CommitData { repo, project: None, docked: false });
    let data = view.mirror_data().expect("its repository");
    let mut other = CommitView::default();
    other.set_data(&*data);
    settle(&mut other);
    assert!(other.repo.is_some());
    assert!(!other.files.is_empty());
}

/// A file clicked: the keyboard goes through the lists — ↑ ↓ across both, Space moves the
/// shown one to the other list, ↵ opens it, Tab goes to the message; a letter typed starts
/// the message.
#[test]
fn the_lists_are_gone_through_with_the_keyboard() {
    // Its own repository: the message typed is kept as its draft, apart from the others'.
    let repo = Arc::new(crate::fake::FakeRepository {
        root: Some("/keys".into()),
        committed_texts: vec![("a.php".into(), "one\ntwo\n".into())],
        index_texts: vec![("a.php".into(), "one\nTWO\n".into())],
        ..crate::fake::FakeRepository::default()
    });
    repo.staged.lock().unwrap().push("a.php".into());
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo: repo.clone(), project: None, docked: false });
    settle(&mut view);
    let i = view.list(true)[0];
    view.hits = vec![(Rect::new(0.0, 0.0, 100.0, 20.0), Hit::Row(i, true))];
    view.click(Point::new(5.0, 5.0));
    settle(&mut view);
    assert_eq!(view.keys(), Keys::Lists);
    let rows = view.rows().len();
    view.edit_key(ViewerKey::End);
    assert_eq!(view.shown_row(), view.rows().last().copied());
    view.edit_key(ViewerKey::Home);
    assert_eq!(view.shown_row(), Some((i, true)), "{rows} rows, the staged one first");
    assert!(view.reveal, "moved to by key: its row is brought into view");
    view.edit_key(ViewerKey::Enter);
    assert!(matches!(view.take_action(), Some(ViewerAction::OpenFile { .. })));
    view.text_input(" ");
    settle(&mut view);
    assert!(repo.staged.lock().unwrap().is_empty(), "Space unstaged it");
    view.edit_key(ViewerKey::Tab);
    assert_eq!(view.keys(), Keys::Message);
    view.edit_key(ViewerKey::Tab);
    assert_eq!(view.keys(), Keys::Lists);
    view.text_input("F");
    assert!(view.keys() == Keys::Message && view.message.text() == "F");
    drafts().remove(repo.root());
}

/// Docked (the Git window's Commit tab): a fixed tab with the repository's places; a section
/// clicked folds, a branch shows its history in the log, a submodule its folder.
#[test]
fn the_places_beside_the_screen_fold_and_lead_to_the_log() {
    let repo = Arc::new(crate::fake::FakeRepository::default());
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo, project: None, docked: true });
    assert!(!view.closable() && view.restorable() && view.panel_key() == Some(KEY));
    view.places = places::Places {
        refs: vec![version_control::RefLabel {
            name: "main".into(),
            kind: version_control::RefKind::Branch,
        }],
        submodules: vec!["libs/x".into()],
        ..places::Places::default()
    };
    view.lay_places();
    let at = |view: &CommitView, wanted: &dyn Fn(&places::Place) -> bool| {
        view.place_rows.iter().position(|r| wanted(&r.place)).expect("its row")
    };
    let branch = at(&view, &|p| matches!(p, places::Place::Ref { .. }));
    assert!(matches!(view.pick_place(branch), Some(ViewerAction::WithHost(_))));
    let module = at(&view, &|p| matches!(p, places::Place::Submodule(_)));
    let Some(ViewerAction::Request(AppRequest::ShowFolder(path))) = view.pick_place(module) else {
        panic!("its folder shown")
    };
    assert!(path.ends_with("libs/x"));
    let rows = view.place_rows.len();
    let branches = at(&view, &|p| matches!(p, places::Place::Section { key: "branches", .. }));
    assert!(view.pick_place(branches).is_none());
    assert!(view.place_rows.len() < rows, "folded");
}

/// The places' filter is the app's search field: pressed, the keyboard types in it (only it
/// has the caret, not the message) and the rows follow (from the top); ⌘A selects its text,
/// × empties it (every row again), Esc empties it, Esc again gives the keyboard back to the
/// message; a click elsewhere too.
#[test]
fn the_places_filter_takes_the_keyboard_while_it_s_clicked() {
    let repo = Arc::new(crate::fake::FakeRepository::default());
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo, project: None, docked: true });
    view.places = places::Places {
        submodules: vec!["libs/x".into(), "vendor/y".into()],
        ..places::Places::default()
    };
    view.lay_places();
    let all = view.place_rows.len();
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |view: &mut CommitView| {
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, 700.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut render_api::Scene::default(),
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut view);
    let field = view.places_filter.area();
    assert!(field.size.width > 0.0, "the filter drawn");
    let inside = Point::new(field.min_x() + 40.0, field.min_y() + field.size.height / 2.0);
    view.places_scroll = 50.0;
    view.press(inside, 1);
    view.release();
    assert_eq!(view.keys(), Keys::Places, "the filter has the keys, the message not");
    view.text_input("vend");
    let shown: Vec<_> = view.place_rows.iter().map(|r| r.place.clone()).collect();
    assert_eq!(shown.last(), Some(&places::Place::Submodule("vendor/y".into())));
    assert!(!shown.contains(&places::Place::Submodule("libs/x".into())));
    assert_eq!(view.places_scroll, 0.0, "filtered: from the top");
    assert!(!view.message.text().contains("vend"), "typed in the filter, not the message");
    view.edit_key(ViewerKey::Backspace);
    assert_eq!(view.places_filter.text(), "ven");
    // ⌘A selects the filter's text (not the message's), and copy takes it.
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("ven"));
    // × (at the shared field's right end) empties it: every place again.
    paint(&mut view);
    let clear = Point::new(field.max_x() - 17.0, field.min_y() + field.size.height / 2.0);
    view.hover(clear);
    assert_eq!(view.cursor_at(clear), Some(ViewerCursor::Pointer), "a hand on ×");
    view.press(clear, 1);
    view.release();
    assert!(view.places_filter.text().is_empty());
    assert_eq!(view.place_rows.len(), all, "every place again");
    assert_eq!(view.keys(), Keys::Places);
    view.text_input("v");
    view.edit_key(ViewerKey::Escape);
    assert!(view.places_filter.text().is_empty() && view.keys() == Keys::Places);
    view.edit_key(ViewerKey::Escape);
    assert_eq!(view.keys(), Keys::Message);
    view.press(inside, 1);
    view.release();
    assert_eq!(view.keys(), Keys::Places);
    view.hits = vec![(Rect::new(0.0, 900.0, 100.0, 20.0), Hit::Refresh)];
    view.click(Point::new(5.0, 905.0));
    assert_eq!(view.keys(), Keys::Message, "a click elsewhere takes it away");
}

/// A message pasted comes whole: its summary, the blank line, its details (the app gives a
/// field of several lines all of the clipboard); the lists and the places' filter aren't one.
#[test]
fn a_pasted_message_keeps_its_lines() {
    let repo = Arc::new(crate::fake::FakeRepository {
        root: Some("/paste".into()),
        ..crate::fake::FakeRepository::default()
    });
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo: repo.clone(), project: None, docked: true });
    assert!(view.field_takes_lines(), "the message has the keyboard");
    let message = "feat: a summary\n\n  - one detail\n  - another";
    assert!(view.paste(message));
    assert_eq!(view.message.text(), message);
    view.set_keys(Keys::Places);
    assert!(!view.field_takes_lines());
    view.set_keys(Keys::Lists);
    assert!(!view.field_takes_lines());
    drafts().remove(repo.root());
}

/// A branch of the places, right-clicked: the same menu as the log's branches panel (a
/// branch from it, renamed, a tag, its pushes); a row chosen opens its dialog.
#[test]
fn a_place_s_branch_has_the_log_s_menu() {
    let repo = Arc::new(crate::fake::FakeRepository::default());
    let mut view = CommitView::default();
    view.set_data(&CommitData { repo, project: None, docked: true });
    view.places = places::Places {
        refs: vec![version_control::RefLabel {
            name: "main".into(),
            kind: version_control::RefKind::Branch,
        }],
        states: vec![version_control::SyncState {
            branch: "main".into(),
            upstream: Some("origin/main".into()),
            ahead: 1,
            behind: 0,
        }],
        ..places::Places::default()
    };
    view.remotes = vec!["origin".into()];
    view.lay_places();
    let i = view
        .place_rows
        .iter()
        .position(|r| matches!(&r.place, places::Place::Ref { name, .. } if name == "main"))
        .expect("its row");
    let r = Rect::new(0.0, 0.0, 100.0, 20.0);
    view.hits = vec![(r, Hit::Place(i))];
    let menu = view.context_menu(Point::new(5.0, 5.0)).expect("its menu");
    let ids: Vec<String> = menu
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        [
            format!("branch.switch:{i}"),
            format!("branch.new-branch:{i}"),
            format!("branch.rename:{i}"),
            format!("branch.new-tag:{i}"),
            format!("branch.delete:{i}"),
            format!("branch.push:{i}"),
        ]
    );
    assert!(view.command(&format!("branch.rename:{i}")));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
}

/// The keyboard's lanes are its parts' focus: the lists have it, a press in the message
/// gives it to the message (its text typed there, kept), a ⇧ press there selects nothing
/// more than a press (⇧ chooses files), the wheel over it scrolls its own lines only.
#[test]
fn a_press_in_the_message_gives_it_the_keys_back() {
    let mut view = three_files();
    view.set_keys(Keys::Lists);
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 900.0, 600.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let (field, _) = *view.hits.iter().find(|(_, h)| *h == Hit::Message).unwrap();
    let inside = Point::new(field.min_x() + 10.0, field.min_y() + 10.0);
    let shift = ui_components::PressMods { shift: true, ..Default::default() };
    view.press_with(inside, 1, shift);
    view.release();
    assert_eq!(view.keys(), Keys::Message);
    assert_eq!(chosen(&view), ["a"], "the press chose no files");
    assert!(view.text_input("x"));
    assert_eq!(view.message.text(), "x");
    assert!(view.edit_key(ViewerKey::Tab), "Tab: to the lists");
    assert_eq!(view.keys(), Keys::Lists);
    view.hover(inside);
    view.pointer = inside;
    let scrolls = view.scrolls;
    view.scroll(0.0, 40.0);
    assert_eq!(view.scrolls, scrolls, "the panes stay: the wheel was the message's");
}

/// Each list shows what's new first, then what's deleted, then the rest, each group by
/// path: a new file is never lost among the changes. A file changed on both sides takes its
/// place in each list by that side's state.
#[test]
fn the_lists_show_new_files_first_then_deleted_then_the_rest() {
    let file = |path: &str, staged, unstaged| StagedFile { path: path.into(), staged, unstaged };
    let view = CommitView {
        files: vec![
            file("a.txt", None, Some(FileState::Modified)),
            file("b.txt", None, Some(FileState::Deleted)),
            file("c.txt", Some(FileState::Added), Some(FileState::Modified)),
            file("d.txt", None, Some(FileState::Untracked)),
            file("e.txt", Some(FileState::Modified), None),
            file("f.txt", Some(FileState::Deleted), None),
            file("g.txt", None, Some(FileState::Renamed)),
        ],
        ..CommitView::default()
    };
    let paths = |staged| -> Vec<String> {
        view.list(staged).into_iter().map(|i| view.files[i].path.clone()).collect()
    };
    assert_eq!(paths(true), ["c.txt", "f.txt", "e.txt"]);
    assert_eq!(paths(false), ["d.txt", "b.txt", "a.txt", "c.txt", "g.txt"]);
}
