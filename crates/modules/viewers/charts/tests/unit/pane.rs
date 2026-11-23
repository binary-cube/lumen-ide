use super::*;
use charts_model::{Aggregate, ChartKind, Column, Rows};

fn rows() -> Lent {
    let mut rows = Rows::new(vec![
        Column::new("country", ColumnKind::Text),
        Column::new("amount", ColumnKind::Number),
    ]);
    for (c, a) in [("RO", 10), ("DE", 30), ("RO", 5)] {
        rows.push([Some(c.to_string()), Some(a.to_string())]);
    }
    Arc::new(rows)
}

/// A pane over the rows with nothing chosen (the first chart proposed put aside).
fn blank() -> ChartPane {
    let mut pane = ChartPane::new();
    pane.set_rows(rows(), 1, Held::default());
    pane.spec = ChartSpec { series: vec![SeriesSpec::default()], ..ChartSpec::default() };
    pane
}

/// Waits for the worker's preparing (a test only).
fn settled(pane: &mut ChartPane) {
    let started = std::time::Instant::now();
    while pane.busy() {
        pane.poll();
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "prepared");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Nothing chosen: a first chart is proposed from the columns and prepared from the rows on
/// a worker.
#[test]
fn a_chart_is_prepared_once_its_series_is_chosen() {
    let mut pane = ChartPane::new();
    let held = Held { rows: 3, ..Held::default() };
    pane.set_rows(rows(), 1, held);
    settled(&mut pane);
    // A first chart proposed: the text along X, the number as values, bars.
    assert_eq!(pane.spec().series[0].x.as_deref(), Some("country"));
    assert_eq!(pane.spec().series[0].y, ["amount"]);
    assert_eq!(pane.spec().kind, ChartKind::Bar);
    assert_eq!(pane.unready, None);
    // A kind chosen: prepared again for it.
    pane.press_panel(PanelHit::Kind(ChartKind::Pie));
    settled(&mut pane);
    assert_eq!(pane.spec().kind, ChartKind::Pie);
    assert!(pane.unready.is_none());
}

/// The list under a field chooses a column (or none: the rows in order), the values'
/// list ticks several, the aggregate's one.
#[test]
fn the_fields_lists_choose_columns() {
    let mut pane = blank();
    pane.open_picker(Field::X);
    assert_eq!(pane.choosing, Some(Field::X));
    pane.picked(PickerEvent::Chosen(1));
    assert_eq!(pane.spec().series[0].x.as_deref(), Some("country"));
    pane.open_picker(Field::Group);
    pane.picked(PickerEvent::Chosen(0));
    assert_eq!(pane.spec().series[0].group, None, "no groups");
    pane.open_picker(Field::Y);
    pane.picked(PickerEvent::Toggled(0));
    assert_eq!(pane.spec().series[0].y, ["amount"]);
    pane.picked(PickerEvent::Toggled(0));
    assert!(pane.spec().series[0].y.is_empty(), "unticked");
    pane.picked(PickerEvent::Closed);
    pane.open_picker(Field::Aggregate);
    pane.picked(PickerEvent::Chosen(1));
    assert_eq!(pane.spec().series[0].aggregate, Aggregate::Count);
}

/// Its settings kept as text come back whole; set already, the chart shows first.
#[test]
fn its_settings_are_kept_and_restored() {
    let mut pane = ChartPane::new();
    assert_eq!(pane.saved(), None, "nothing chosen: nothing kept");
    pane.series().x = Some("country".into());
    pane.series().y = vec!["amount".into()];
    pane.spec.options.points = true;
    let saved = pane.saved().expect("kept");
    let mut again = ChartPane::new();
    again.restore(&saved);
    assert_eq!(again.spec(), pane.spec());
    assert!(!again.settings, "the chart first");
    again.restore("not json");
    assert_eq!(again.spec(), pane.spec(), "a text it can't read is left");
}

/// "Fetch all rows" asks the host, once.
#[test]
fn fetch_all_asks_the_host_once() {
    let mut pane = ChartPane::new();
    pane.press_panel(PanelHit::FetchAll);
    assert_eq!(
        pane.take_request(),
        Some(PaneRequest::AllRows { limit: crate::settings::fetch_rows() })
    );
    assert_eq!(pane.take_request(), None);
}

/// While a field's list is open it takes the keys: ↓ ↵ choose, Esc closes; closed, the keys
/// go on to the host.
#[test]
fn an_open_list_takes_the_keys() {
    use ui_components::{KeyReach, ViewerKey};
    let mut pane = blank();
    assert!(!pane.captures_keys());
    pane.open_picker(Field::X);
    assert!(pane.captures_keys());
    assert!(pane.key(ViewerKey::Down, KeyReach::default()));
    assert!(pane.key(ViewerKey::Enter, KeyReach::default()));
    assert_eq!(pane.spec().series[0].x.as_deref(), Some("country"));
    assert!(!pane.captures_keys(), "chosen: closed");
    pane.open_picker(Field::X);
    pane.key(ViewerKey::Escape, KeyReach::default());
    assert!(!pane.captures_keys());
    assert!(!pane.key(ViewerKey::Down, KeyReach::default()), "closed: the host's");
}

/// Series added as the one before them (their X, their aggregate), each set on its own; the
/// one chosen removed, never the last.
#[test]
fn series_are_added_set_and_removed() {
    let mut pane = blank();
    pane.series().x = Some("country".into());
    pane.press_panel(PanelHit::AddSeries);
    assert_eq!((pane.spec().series.len(), pane.current), (2, 1));
    assert_eq!(pane.spec().series[1].x.as_deref(), Some("country"));
    pane.series().y = vec!["amount".into()];
    assert!(pane.spec().series[0].y.is_empty(), "the first is its own");
    pane.press_panel(PanelHit::RemoveSeries);
    assert_eq!((pane.spec().series.len(), pane.current), (1, 0));
    pane.press_panel(PanelHit::RemoveSeries);
    assert_eq!(pane.spec().series.len(), 1, "never the last");
}

/// While rows are still coming only their count changes; the chart is prepared once they've
/// all come.
#[test]
fn rows_still_coming_are_prepared_once_at_the_end() {
    let mut pane = ChartPane::new();
    pane.set_rows(rows(), 1, Held { rows: 3, more: true, fetching: true, ..Held::default() });
    assert!(!pane.busy(), "nothing prepared while they come");
    assert_eq!(pane.held.rows, 3);
    pane.set_rows(rows(), 2, Held { rows: 3, ..Held::default() });
    assert!(pane.busy(), "prepared now");
    settled(&mut pane);
}

/// A chart saved before the ring existed reads as it was (a full pie, no hole).
#[test]
fn a_spec_saved_before_the_ring_reads_as_a_pie() {
    let saved = r#"{"kind":"pie","series":[],"options":{"stepped":false,"points":false,"percent":false,"log":false}}"#;
    let spec: charts_model::spec::ChartSpec = serde_json::from_str(saved).expect("it reads");
    assert_eq!(spec.options.hole, 0);
}

/// Drawn from a selection: the chart set for all the rows drawn from it when its columns
/// are there (else one proposed for the selection's), said in the bar, kept as the state;
/// all the rows again: the chart set before back, unless it was changed meanwhile.
#[test]
fn a_selection_is_drawn_and_the_chart_set_before_comes_back() {
    let mut pane = ChartPane::new();
    pane.set_rows(rows(), 1, Held { rows: 3, ..Held::default() });
    pane.press_panel(PanelHit::Kind(ChartKind::Pie));
    let set = pane.spec().clone();
    let picked = Held { rows: 2, picked: true, ..Held::default() };
    pane.set_rows(rows(), 2, picked);
    assert_eq!(pane.spec(), &set, "its columns are there: drawn as set");
    assert_eq!(pane.note(), i18n::tr!("charts.from_selection", count = 2));
    // Only the amounts selected: a chart proposed for them, the one set kept as the state.
    let mut amounts = Rows::new(vec![Column::new("amount", ColumnKind::Number)]);
    amounts.push([Some("4")]);
    amounts.push([Some("9")]);
    pane.set_rows(Arc::new(amounts), 3, picked);
    assert_ne!(pane.spec(), &set);
    assert_eq!(pane.saved(), serde_json::to_string(&set).ok());
    pane.set_rows(rows(), 4, Held { rows: 3, ..Held::default() });
    assert_eq!(pane.spec(), &set, "all the rows: back as set");
    // Changed while on a selection: kept.
    pane.set_rows(rows(), 5, picked);
    pane.press_panel(PanelHit::Kind(ChartKind::Line));
    pane.set_rows(rows(), 6, Held { rows: 3, ..Held::default() });
    assert_eq!(pane.spec().kind, ChartKind::Line);
    // The bar's button asks the host for all the rows.
    pane.set_rows(rows(), 7, picked);
    pane.press_panel(PanelHit::WholeRows);
    assert_eq!(pane.take_request(), Some(PaneRequest::WholeRows));
}
