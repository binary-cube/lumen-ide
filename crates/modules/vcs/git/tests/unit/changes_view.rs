use super::*;

/// Without the project's changes (not followed): nothing read, nothing shown.
#[test]
fn without_the_project_s_changes_the_list_is_empty() {
    let mut list = ChangesList::default();
    assert!(!list.poll());
    assert!(list.files.is_empty());
    assert_eq!(list.generation, 0);
}

/// Many changed files: the tree's thumb (on the list's right edge) pressed and dragged down
/// scrolls them, through the view's own press, drag and release.
#[test]
fn the_files_scroll_by_their_thumb() {
    let files = (0..80)
        .map(|i| FileStatus { path: format!("f{i:02}.php"), state: FileState::Modified })
        .collect();
    let mut list = ChangesList { files: Arc::new(files), ..ChangesList::default() };
    list.tree.set_flat(true);
    list.refresh();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
    let mut paint = |list: &mut ChangesList| {
        list.paint(ViewerPaint {
            bounds,
            scale: 1.0,
            text: &mut text,
            scene: &mut render_api::Scene::default(),
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut list);
    // A hidden bar shows once the list scrolls; back at the top after.
    assert!(list.scroll(0.0, 1.0));
    paint(&mut list);
    assert!(list.scroll(0.0, -1.0));
    paint(&mut list);
    let held = (0..40).find_map(|dy| {
        (0..16).find_map(|dx| {
            let at = Point::new(bounds.max_x() - 1.0 - dx as f32, 60.0 + dy as f32 * 4.0);
            list.press(at, 1);
            // The search's field takes a drag too (its text selected): not the thumb.
            if !list.tree.searching && list.drag(Point::new(at.x, at.y + 80.0)) {
                return Some(at);
            }
            list.release();
            None
        })
    });
    assert!(held.is_some(), "the thumb found and dragged");
    assert!(list.release());
    assert!(list.scroll(0.0, -1.0), "it had scrolled down");
}

/// The tree has the keys without a press (through its parts' focus): ↓ walks its rows, Tab
/// isn't used (not searching), ↵ on a file opens it; the wheel over the header scrolls it.
#[test]
fn the_tree_has_the_keys_and_the_wheel_through_its_controls() {
    let files = (0..80)
        .map(|i| FileStatus { path: format!("f{i:02}.php"), state: FileState::Modified })
        .collect();
    let mut list = ChangesList { files: Arc::new(files), ..ChangesList::default() };
    list.tree.set_flat(true);
    list.refresh();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    list.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert!(list.edit_key(ViewerKey::Down), "the tree's keys");
    assert_eq!(list.controls.focused(), Some(0));
    assert!(!list.edit_key(ViewerKey::Tab), "Tab isn't the tree's");
    assert!(list.edit_key(ViewerKey::Enter));
    list.hover(Point::new(20.0, 10.0));
    assert!(list.scroll(0.0, 40.0), "the wheel over the header scrolls the files");
}
