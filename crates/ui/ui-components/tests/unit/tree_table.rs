//! The tree on columns: the rows shown as rows open and fold, the keys, `*`, the choice kept
//! through folding, the text copied, numbers written, and — drawn — a chevron's press, a
//! column's edge dragged, the tooltips, the rows in view only.

use render_api::Scene;

use super::*;

const SCALE: f32 = 2.0;

/// A (B (C), D), E — five rows, two that fold.
fn table() -> TreeTable {
    let mut t = TreeTable::new(vec![
        TableColumn::new("Operation", 0.0),
        TableColumn::new("Rows", 80.0).right().with_tip("The rows expected"),
    ]);
    let row = |parent, name: &str, n: f64| {
        TableRow::new(
            parent,
            vec![TableCell::text(name), TableCell::number(n, 0, Some("rows".into()))],
        )
    };
    t.set_rows(
        vec![
            row(None, "A", 1.0),
            row(Some(0), "B", 2.0),
            row(Some(1), "C", 1234.0),
            row(Some(0), "D", 4.0),
            row(None, "E", 5.0),
        ],
        |_| true,
    );
    t
}

fn paint(t: &mut TreeTable, text: &mut TextSystem, w: f32, h: f32) {
    painted(t, text, w, h);
}

fn painted(t: &mut TreeTable, text: &mut TextSystem, w: f32, h: f32) -> Scene {
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    t.paint(TablePaint {
        bounds: Rect::new(0.0, 0.0, w, h),
        scale: SCALE,
        text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
    });
    scene
}

#[test]
fn folded_rows_hide_what_s_under_them() {
    let mut t = table();
    assert_eq!(t.visible(), [0, 1, 2, 3, 4]);
    assert_eq!((t.depth(2), t.has_children(1), t.has_children(2)), (2, true, false));
    assert!(t.set_open(1, false));
    assert_eq!(t.visible(), [0, 1, 3, 4]);
    assert!(!t.set_open(2, false), "a leaf doesn't fold");
    t.set_all_open(false);
    assert_eq!(t.visible(), [0, 4]);
    // Choosing a hidden row opens its parents.
    t.select(Some(2));
    assert_eq!(t.visible(), [0, 1, 2, 3, 4]);
    // Folding its parent chooses the parent.
    t.set_open(0, false);
    assert_eq!(t.selected(), Some(0));
    // `*` opens everything under the choice.
    t.set_all_open(false);
    t.select(Some(0));
    t.set_open(0, false);
    assert!(t.char_key('*'));
    assert_eq!(t.visible(), [0, 1, 2, 3, 4]);
}

#[test]
fn the_keys_move_open_and_fold() {
    let mut t = table();
    assert!(t.key(ViewerKey::Down));
    assert_eq!(t.selected(), Some(0));
    t.key(ViewerKey::Down);
    assert_eq!(t.selected(), Some(1));
    // ← folds B, ← again goes to A; → opens, → again goes to the first child.
    assert!(t.key(ViewerKey::Left));
    assert_eq!((t.is_open(1), t.selected()), (false, Some(1)));
    t.key(ViewerKey::Left);
    assert_eq!(t.selected(), Some(0));
    t.select(Some(1));
    t.key(ViewerKey::Right);
    assert!(t.is_open(1));
    t.key(ViewerKey::Right);
    assert_eq!(t.selected(), Some(2));
    assert!(!t.key(ViewerKey::Right), "a leaf");
    t.key(ViewerKey::End);
    assert_eq!(t.selected(), Some(4));
    t.key(ViewerKey::Home);
    assert_eq!(t.selected(), Some(0));
    assert!(t.key(ViewerKey::Enter));
    assert_eq!(t.visible(), [0, 4]);
}

#[test]
fn rows_are_copied_as_text() {
    let mut t = table();
    assert_eq!(t.row_text(2).unwrap(), "C\t1\u{2009}234 rows");
    t.set_open(1, false);
    assert_eq!(t.text(), "A\t1 rows\n  B\t2 rows\n  D\t4 rows\nE\t5 rows\n");
}

#[test]
fn numbers_are_grouped() {
    assert_eq!(number_text(12340.0, 0), "12\u{2009}340");
    assert_eq!(number_text(0.4200, 3), "0.42");
    assert_eq!(number_text(-1204.5, 2), "-1\u{2009}204.5");
    assert_eq!(number_text(7.0, 2), "7");
}

#[test]
fn drawn_its_chevrons_edges_and_tips_answer() {
    let mut text = TextSystem::new();
    let mut t = table();
    paint(&mut t, &mut text, 1200.0, 600.0);
    let row_h = ROW_H * SCALE;
    let top = HEADER_H * SCALE;
    // B's chevron (depth 1): folds it.
    let chevron = Point::new((6.0 + INDENT + 7.0) * SCALE, top + row_h * 1.5);
    assert_eq!(t.press(chevron, 1), Some(TableEvent::Toggled(1)));
    assert!(!t.is_open(1));
    paint(&mut t, &mut text, 1200.0, 600.0);
    // A row's text: chosen; a double click on one that folds toggles it.
    let a = Point::new(600.0, top + row_h * 0.5);
    assert_eq!(t.press(a, 1), Some(TableEvent::Selected(0)));
    assert_eq!(t.press(a, 2), Some(TableEvent::Toggled(0)));
    // The Rows column's left edge dragged left widens it.
    let edge = Point::new(1200.0 - 80.0 * SCALE, top / 2.0);
    assert_eq!(t.cursor_at(edge), Some(ViewerCursor::ResizeHorizontal));
    t.press(edge, 1);
    assert!(t.drag(Point::new(edge.x - 40.0, edge.y)));
    assert!(t.release());
    assert_eq!(t.columns()[1].width, 100.0);
    // The header's tooltip says what the column is.
    paint(&mut t, &mut text, 1200.0, 600.0);
    let tip = t.tooltip_at(Point::new(1200.0 - 40.0, top / 2.0)).unwrap();
    assert_eq!(tip.label, "The rows expected");
}

#[test]
fn only_the_rows_in_view_scroll() {
    let mut text = TextSystem::new();
    let mut t = TreeTable::new(vec![TableColumn::new("Op", 0.0)]);
    t.set_rows(
        (0..500).map(|i| TableRow::new(None, vec![TableCell::text(format!("row {i}"))])).collect(),
        |_| true,
    );
    paint(&mut t, &mut text, 800.0, 400.0);
    assert!(t.scroll(0.0, 1e9));
    assert!(!t.scroll(0.0, 10.0), "at the end");
    // A key brings the choice back into view at the next paint.
    t.key(ViewerKey::Home);
    paint(&mut t, &mut text, 800.0, 400.0);
    assert!(!t.scroll(0.0, -10.0), "at the top again");
}

/// The filter shows the rows having its text and their parents, whatever is folded; empty,
/// the rows as opened again.
#[test]
fn the_filter_keeps_the_matches_and_their_parents() {
    let mut t = TreeTable::new(vec![TableColumn::new("Op", 0.0)]);
    let row = |p: Option<usize>, s: &str| TableRow::new(p, vec![TableCell::text(s)]);
    t.set_rows(
        vec![
            row(None, "Limit"),
            row(Some(0), "Hash Join"),
            row(Some(1), "Seq Scan orders"),
            row(Some(1), "Hash"),
            row(Some(3), "Seq Scan users"),
        ],
        |_| true,
    );
    t.set_all_open(false);
    assert_eq!(t.visible(), [0]);
    let before = t.generation();
    assert!(t.set_filter("USERS"));
    assert!(!t.set_filter("users"), "the same, any case");
    assert!(t.generation() != before);
    assert_eq!(t.visible(), [0, 1, 3, 4]);
    assert!(t.matches(4) && !t.matches(2));
    assert!(t.set_filter("nothing like it"));
    assert!(t.visible().is_empty());
    t.set_filter("");
    assert_eq!(t.visible(), [0], "folded as before");
}

/// A row's sign says what it means; a cell's outline, its lead icon and a folded row's note
/// are drawn; a severity's or heat's ink resolves with the theme.
#[test]
fn signs_outlines_and_folded_notes_are_drawn() {
    use crate::severity::Level;
    let mut text = TextSystem::new();
    let mut t = TreeTable::new(vec![TableColumn::new("Op", 0.0), TableColumn::new("Spill", 120.0)]);
    let parent = TableRow::new(
        None,
        vec![
            TableCell::text("Sort"),
            TableCell::text("50 MB")
                .with_lead("hard-drive", Level::Warning)
                .outlined(Some(Ink::Severity(Level::Warning))),
        ],
    )
    .with_sign("alert", Level::Warning, "A spill".into())
    .tinted(Some(Ink::Severity(Level::Warning)))
    .with_folded_note(Some("+1 step".into()));
    let child = TableRow::new(Some(0), vec![TableCell::text("Seq Scan"), TableCell::empty()]);
    t.set_rows(vec![parent, child], |_| false);
    let scene = painted(&mut t, &mut text, 800.0, 300.0);
    let p = Palette::dark();
    let warning = Level::Warning.color(&p);
    let quads: Vec<_> = scene.layers().iter().flat_map(|l| l.quads.iter()).collect();
    assert!(
        quads.iter().any(|q| q.border_width > 0.0 && q.border_color == warning.with_alpha(0.85)),
        "the outline"
    );
    assert!(quads.iter().any(|q| q.color == warning.with_alpha(0.08)), "the tint");
    // The sign's tooltip, in the gutter.
    let y = (HEADER_H + ROW_H / 2.0) * SCALE;
    let tip = t.tooltip_at(Point::new(4.0, y)).unwrap();
    assert_eq!(tip.label, "A spill");
    // The heat's ink: hotter is a different shade.
    assert_ne!(Ink::heat(0.1).color(&p), Ink::heat(0.9).color(&p));
    assert_eq!(Ink::heat(2.0), Ink::Heat(1000));
    assert_eq!(Ink::from(Tone::Accent).color(&p), p.accent);
    // A cell's text as copied keeps the bar's label.
    assert_eq!(TableCell::bar(0.5, Tone::Accent, "412 ms · 48 %").plain(), "412 ms · 48 %");
}
