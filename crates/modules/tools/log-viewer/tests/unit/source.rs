use super::*;

fn entry(n: u64) -> Entry {
    let mut e = Entry::default();
    e.cells[crate::entry::MSG] = format!("line {n}");
    e
}

fn fake() -> (LogSource, Receiver<Ask>, Sender<Answer>) {
    let (asks, asked) = mpsc::channel();
    let (answer, answers) = mpsc::channel();
    (LogSource::with(asks, answers), asked, answer)
}

/// The count told is shown; a block is asked once, only what's missing; its rows come.
#[test]
fn what_s_shown_is_asked_once_and_comes() {
    let (mut source, asked, answer) = fake();
    answer.send(Answer::Count { count: 450, generation: 0, scanning: false }).unwrap();
    assert!(source.poll());
    assert_eq!(source.total(), Total::Known(450));
    source.request(&GridQuery::default(), 0..30);
    source.request(&GridQuery::default(), 0..30);
    let asks: Vec<Ask> = asked.try_iter().collect();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(source.row_state(3), RowState::Pending);
    assert_eq!(source.status(), SourceStatus::Loading);
    let entries = (0..BLOCK).map(entry).collect();
    answer.send(Answer::Block { block: 0, count: 450, entries, generation: 0 }).unwrap();
    source.poll();
    assert_eq!(source.row_state(3), RowState::Ready);
    assert_eq!(source.cell(3, 4), CellRef::Text("line 3"));
    assert_eq!(source.row_state(450), RowState::Missing);
}

/// Paused: the count shown stays (the rows don't move), the new ones counted beside;
/// resumed, it's what's there now. A block read for a count no longer shown is let go.
#[test]
fn a_pause_holds_what_s_shown_while_the_reader_goes_on() {
    let (mut source, _asked, answer) = fake();
    answer.send(Answer::Count { count: 100, generation: 0, scanning: false }).unwrap();
    source.poll();
    source.pause(true);
    answer.send(Answer::Count { count: 130, generation: 0, scanning: false }).unwrap();
    source.poll();
    assert_eq!(source.counts(), (100, 130));
    assert_eq!(source.total(), Total::Known(100));
    source.pause(false);
    assert_eq!(source.counts(), (130, 130));
    // An answer for the 100 shown before: other rows now.
    answer
        .send(Answer::Block { block: 0, count: 100, entries: vec![entry(0)], generation: 0 })
        .unwrap();
    source.poll();
    assert_eq!(source.row_state(0), RowState::Pending);
}

/// Another filter: a new generation, what was kept let go, the old answers ignored.
#[test]
fn another_filter_starts_anew() {
    use ui_components::data_grid::filter::GridFilter;
    let (mut source, asked, answer) = fake();
    let query =
        GridQuery { filter: Some(GridFilter::NotNull("msg".into())), ..GridQuery::default() };
    source.request(&query, 0..10);
    assert!(matches!(asked.try_recv(), Ok(Ask::Filter { generation: 1, .. })));
    answer.send(Answer::Count { count: 9, generation: 0, scanning: false }).unwrap();
    source.poll();
    assert_eq!(source.total(), Total::Known(0), "the old filter's count ignored");
    answer.send(Answer::Count { count: 9, generation: 1, scanning: true }).unwrap();
    source.poll();
    assert_eq!((source.total(), source.status()), (Total::Known(9), SourceStatus::Loading));
}

/// At most KEPT blocks are kept, the least lately wanted let go first.
#[test]
fn only_so_many_blocks_are_kept() {
    let (mut source, _asked, answer) = fake();
    let count = BLOCK * 50;
    answer.send(Answer::Count { count, generation: 0, scanning: false }).unwrap();
    source.poll();
    for block in 0..40 {
        source.request(&GridQuery::default(), block * BLOCK..block * BLOCK + 1);
        let entries = (0..BLOCK).map(entry).collect();
        answer.send(Answer::Block { block, count, entries, generation: 0 }).unwrap();
        source.poll();
    }
    assert_eq!(source.blocks.len(), KEPT);
    assert!(!source.blocks.contains_key(&0) && source.blocks.contains_key(&39));
}

/// Each level its icon, in its tone; only the level's cell has one; the WHERE field's
/// example is the log's own.
#[test]
fn a_level_has_its_icon_and_the_filter_its_example() {
    use ui_components::ds::Tone;
    assert_eq!(level_icon("ERROR"), Some(("octagon-alert", Tone::Danger)));
    assert_eq!(level_icon("WARN"), Some(("alert", Tone::Warning)));
    assert_eq!(level_icon("INFO"), Some(("info", Tone::Accent)));
    assert_eq!(level_icon("DEBUG"), level_icon("TRACE"));
    assert_eq!(level_icon("other"), None);
    for (icon, _) in ["ERROR", "WARN", "INFO", "DEBUG"].iter().filter_map(|l| level_icon(l)) {
        assert!(icons::Icons::builtin().id(icon).is_some(), "{icon} is in the app's set");
    }
    let (mut source, _asked, answer) = fake();
    answer.send(Answer::Count { count: 1, generation: 0, scanning: false }).unwrap();
    source.poll();
    let mut e = entry(0);
    e.cells[LEVEL] = "ERROR".into();
    answer.send(Answer::Block { block: 0, count: 1, entries: vec![e], generation: 0 }).unwrap();
    source.request(&GridQuery::default(), 0..1);
    source.poll();
    assert_eq!(source.cell_icon(0, LEVEL), Some(("octagon-alert", Tone::Danger)));
    assert_eq!(source.cell_icon(0, crate::entry::MSG), None, "the message has none");
    assert!(source.filter_hint().is_some_and(|h| h.contains("level = 'ERROR'")));
}

/// New lines shown while the grid's rows in view stay the same (a page of 10): the source
/// asks for them again itself (the grid doesn't: its range didn't change), so they never
/// stay pending.
#[test]
fn rows_in_view_are_asked_again_when_new_lines_are_shown() {
    let (mut source, asked, answer) = fake();
    // Asked before the count is known: asked once it is.
    source.request(&GridQuery::default(), 0..10);
    assert_eq!(asked.try_iter().count(), 0);
    answer.send(Answer::Count { count: 29, generation: 0, scanning: false }).unwrap();
    source.poll();
    assert!(matches!(asked.try_recv(), Ok(Ask::Block { block: 0, count: 29, .. })));
    let entries = (0..29).map(entry).collect();
    answer.send(Answer::Block { block: 0, count: 29, entries, generation: 0 }).unwrap();
    source.poll();
    assert_eq!(source.row_state(0), RowState::Ready);
    // A new line: the same rows in view, asked again for the new count.
    answer.send(Answer::Count { count: 30, generation: 0, scanning: false }).unwrap();
    source.poll();
    assert_eq!(source.row_state(0), RowState::Pending);
    assert!(matches!(asked.try_recv(), Ok(Ask::Block { block: 0, count: 30, .. })));
}
