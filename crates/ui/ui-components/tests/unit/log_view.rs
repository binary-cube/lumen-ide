//! The log: its limit (the oldest lines leave), the rows laid out (wrapped at the width, new
//! lines only appended, everything again when the width or the wrapping changes), the end
//! kept in view as lines come (and not once scrolled up), the links, the selection copied,
//! the side buttons.

use render_api::Scene;

use super::*;

const SCALE: f32 = 2.0;

fn frame(view: &mut LogView, text: &mut TextSystem, width: f32, height: f32) {
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, width, height),
        scale: SCALE,
        text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

fn line(n: usize) -> LogLine {
    LogLine::new("10:00:00.000", LogKind::Info, format!("line {n}"))
}

#[test]
fn the_oldest_lines_leave_past_the_limit() {
    let log = Log::with_limit(3);
    for n in 0..5 {
        log.push(line(n));
    }
    let texts: Vec<String> = log.lines().into_iter().map(|l| l.text).collect();
    assert_eq!(texts, ["line 2", "line 3", "line 4"]);
    assert_eq!(Log::new().limit(), LIMIT);
    // A view drawn meanwhile keeps only what's still there, numbered on.
    let mut text = TextSystem::new();
    let mut view = LogView::new(log.clone());
    frame(&mut view, &mut text, 1600.0, 600.0);
    log.push(line(5));
    frame(&mut view, &mut text, 1600.0, 600.0);
    let rows = view.rows();
    assert_eq!(rows.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [3, 4, 5]);
    assert!(log.memory() > 0);
    log.clear();
    assert!(log.is_empty() && log.memory() == 0);
    frame(&mut view, &mut text, 1600.0, 600.0);
    assert!(view.rows().is_empty());
}

/// Large statements (a dump's, sent one by one) keep the log within [`BYTE_LIMIT`]: the
/// oldest leave first, the last one stays whatever its size.
#[test]
fn the_oldest_lines_leave_past_the_byte_limit() {
    let log = Log::new();
    let big = "x".repeat(BYTE_LIMIT / 4);
    for _ in 0..6 {
        log.push(LogLine::new("", LogKind::Statement, big.clone()));
    }
    assert!(log.memory() <= BYTE_LIMIT);
    assert_eq!(log.len(), 3);
    let huge = "y".repeat(BYTE_LIMIT + 1);
    log.push(LogLine::new("", LogKind::Statement, huge));
    assert_eq!(log.len(), 1, "alone, but kept");
}

/// A wrapped statement's rows are drawn from one tokenizing, kept for its line: the pieces
/// cover it whole.
#[test]
fn a_wrapped_statement_is_tokenized_once() {
    let log = Log::new();
    let sql = "SELECT id, name FROM users WHERE id = 1 AND name = 'x' ".repeat(40);
    log.push(LogLine::new("10:00:00.000", LogKind::Statement, sql).as_code("sql"));
    let mut text = TextSystem::new();
    let mut view = LogView::new(log.clone());
    view.set_wrap(true);
    frame(&mut view, &mut text, 600.0, 600.0);
    assert!(view.rows().len() > 3, "wrapped over several rows");
    assert_eq!(view.code_runs.key, Some((0, 0)));
    let total: usize = view.code_runs.runs.iter().map(|(len, _)| len).sum();
    assert_eq!(total, log.lines()[0].text.len(), "the pieces cover the statement");
}

#[test]
fn soft_wrap_breaks_long_lines_at_the_width() {
    let log = Log::new();
    let long = "word ".repeat(80);
    log.push(LogLine::new("10:00:00.000", LogKind::Result, long.trim_end()));
    log.push(LogLine::new("", LogKind::Info, "first\nsecond"));
    let mut text = TextSystem::new();
    let mut view = LogView::new(log);
    frame(&mut view, &mut text, 1200.0, 600.0);
    let rows = view.rows();
    assert_eq!(rows.len(), 3, "not wrapped: a row per line of text");
    assert!(view.scroll_range().1 >= 0.0);
    view.set_wrap(true);
    frame(&mut view, &mut text, 1200.0, 600.0);
    let wrapped = view.rows();
    let first: Vec<&String> = wrapped.iter().filter(|(n, _)| *n == 0).map(|(_, t)| t).collect();
    assert!(first.len() > 2, "the long line on several rows: {}", first.len());
    // The rows give the line back, broken after a word.
    assert_eq!(first.iter().map(|s| s.as_str()).collect::<String>(), long.trim_end());
    assert!(first[..first.len() - 1].iter().all(|r| r.ends_with(' ')));
    // Narrower: more rows.
    frame(&mut view, &mut text, 700.0, 600.0);
    assert!(view.rows().iter().filter(|(n, _)| *n == 0).count() > first.len());
    // The wrapping function itself: every piece within the room, nothing lost.
    let pieces = wrap("abcdefghij klmnop", 5);
    assert!(pieces.iter().all(|(a, b)| "abcdefghij klmnop"[*a..*b].chars().count() <= 5));
    assert_eq!(pieces.last().map(|p| p.1), Some(17));
}

#[test]
fn the_end_is_kept_in_view_until_scrolled_up() {
    let log = Log::new();
    for n in 0..200 {
        log.push(line(n));
    }
    let mut text = TextSystem::new();
    let mut view = LogView::new(log.clone());
    frame(&mut view, &mut text, 1600.0, 400.0);
    let (scroll, max) = view.scroll_range();
    assert!(max > 0.0 && (scroll - max).abs() < 0.5, "at the end: {scroll} / {max}");
    // More lines: still at the end, only them laid out.
    for n in 200..210 {
        log.push(line(n));
    }
    assert!(view.poll(), "new lines to draw");
    frame(&mut view, &mut text, 1600.0, 400.0);
    assert!(!view.poll());
    let (scroll, max) = view.scroll_range();
    assert!((scroll - max).abs() < 0.5);
    assert_eq!(view.rows().len(), 210);
    // Scrolled up: it stays where it is as lines come.
    assert!(view.scroll(0.0, -300.0));
    assert!(!view.follows());
    let before = view.scroll_range().0;
    log.push(line(210));
    frame(&mut view, &mut text, 1600.0, 400.0);
    assert_eq!(view.scroll_range().0, before);
    // Its button: back to the end, followed again.
    let button = view.button_rect("log.follow").unwrap();
    view.press(Point::new(button.min_x() + 2.0, button.min_y() + 2.0), 1);
    frame(&mut view, &mut text, 1600.0, 400.0);
    assert!(view.follows());
    let (scroll, max) = view.scroll_range();
    assert!((scroll - max).abs() < 0.5);
}

#[test]
fn a_link_is_followed_from_its_text_only() {
    let log = Log::new();
    log.push(line(0));
    let target = ViewerAction::OpenFile {
        path: std::path::PathBuf::from("console.sql"),
        line: Some(1),
        column: Some(15),
    };
    log.push(
        LogLine::new("10:00:01.000", LogKind::Error, "no such table: t").with_link(target.clone()),
    );
    let mut text = TextSystem::new();
    let mut view = LogView::new(log);
    frame(&mut view, &mut text, 1600.0, 600.0);
    let row = view.line_rect(1).unwrap();
    let y = row.min_y() + row.size.height / 2.0;
    // On the time: nothing opens.
    assert_eq!(view.press(Point::new(row.min_x() + 30.0, y), 1), None);
    // On the text: its place.
    let on_text = Point::new(row.min_x() + 330.0, y);
    assert!(view.hover(on_text));
    assert_eq!(view.cursor_at(on_text), Some(ViewerCursor::Pointer));
    assert_eq!(view.press(on_text, 1), Some(target.clone()));
    // A line without a link: nothing.
    let plain = view.line_rect(0).unwrap();
    assert_eq!(view.press(Point::new(plain.min_x() + 330.0, plain.min_y() + 4.0), 1), None);
    // ↵ on the selected error line follows it too.
    view.press(on_text, 2);
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(view.take_action(), Some(target));
}

#[test]
fn the_selection_and_the_buttons_copy_wrap_and_clear() {
    let log = Log::new();
    log.push(
        LogLine::new("10:00:00.000", LogKind::Statement, "SELECT 1")
            .with_prefix("main>")
            .as_code("sql"),
    );
    log.push(LogLine::new("10:00:00.010", LogKind::Result, "1 row retrieved"));
    let mut text = TextSystem::new();
    let mut view = LogView::new(log.clone());
    frame(&mut view, &mut text, 1600.0, 600.0);
    assert_eq!(view.selected_text(), None);
    assert!(view.edit_key(ViewerKey::Down));
    assert!(view.edit_key(ViewerKey::Down));
    assert_eq!(view.selected_text().as_deref(), Some("[10:00:00.010] 1 row retrieved"));
    assert!(view.select_all());
    assert_eq!(
        view.selected_text().as_deref(),
        Some("[10:00:00.000] main> SELECT 1\n[10:00:00.010] 1 row retrieved")
    );
    // Copy: the selection (all, without one), through the app.
    let copy = view.button_rect("log.copy").unwrap();
    let action = view.press(Point::new(copy.min_x() + 2.0, copy.min_y() + 2.0), 1);
    assert!(
        matches!(action, Some(ViewerAction::Request(AppRequest::CopyText(t))) if t.contains("SELECT 1"))
    );
    // Wrap toggled by its button, then by its menu.
    let wrap = view.button_rect("log.wrap").unwrap();
    view.press(Point::new(wrap.min_x() + 2.0, wrap.min_y() + 2.0), 1);
    assert!(view.wraps());
    assert!(view.command("log.wrap"));
    assert!(!view.wraps());
    let menu = view.context_menu(Point::new(800.0, 40.0)).unwrap();
    assert!(menu.iter().any(|e| matches!(e, ViewMenuEntry::Item { id, .. } if id == "log.clear")));
    // Clear: nothing left, in the log too.
    let clear = view.button_rect("log.clear").unwrap();
    view.press(Point::new(clear.min_x() + 2.0, clear.min_y() + 2.0), 1);
    assert!(log.is_empty());
    frame(&mut view, &mut text, 1600.0, 600.0);
    assert!(view.rows().is_empty() && view.selected_text().is_none());
    // Tabs and line ends written as they show.
    log.push(LogLine::new("", LogKind::Info, "a\tb\r\nc"));
    assert_eq!(log.lines()[0].text, "a    b\nc");
}
