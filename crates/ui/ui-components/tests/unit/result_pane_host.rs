use super::*;

/// A pane telling what it was lent (how the host says the rows are held).
#[derive(Debug)]
struct Telling(Arc<std::sync::Mutex<Vec<Held>>>);

impl crate::control::Control for Telling {}

impl ResultPane for Telling {
    fn set_rows(&mut self, _rows: Lent, _epoch: u64, held: Held) {
        self.0.lock().unwrap().push(held);
    }
    fn paint(&mut self, _paint: crate::viewer::ViewerPaint<'_>) {}
}

#[derive(Debug)]
struct Offered(Arc<std::sync::Mutex<Vec<Held>>>);

impl ResultPaneProvider for Offered {
    fn id(&self) -> &'static str {
        "telling"
    }
    fn icon(&self) -> &'static str {
        "chart"
    }
    fn title(&self) -> String {
        "Telling".into()
    }
    fn make(&self) -> Box<dyn ResultPane> {
        Box::new(Telling(Arc::clone(&self.0)))
    }
}

/// A host offering one pane (no folder: nothing kept), and what it's lent.
fn host_with_pane() -> (PaneHost, Arc<std::sync::Mutex<Vec<Held>>>) {
    let told: Arc<std::sync::Mutex<Vec<Held>>> = Arc::default();
    let host =
        PaneHost { offered: vec![Arc::new(Offered(Arc::clone(&told)))], ..PaneHost::default() };
    (host, told)
}

/// A shown thing's key is the same for the same text (its blanks at the ends left out), and
/// another for another.
#[test]
fn a_key_is_stable() {
    assert_eq!(key_of("select 1"), key_of("  select 1\n"));
    assert_ne!(key_of("select 1"), key_of("select 2"));
    assert_eq!(key_of("select 1").len(), 16);
    assert_eq!(file_name("chart", "00ff"), "00ff-chart.txt");
}

/// With no pane open the grid has all the room and nothing is the pane's.
#[test]
fn the_room_is_split_only_while_a_pane_is_open() {
    let mut host = PaneHost::default();
    let area = Rect::new(0.0, 30.0, 800.0, 600.0);
    assert_eq!(host.split(area, 1.0), (area, None));
    assert!(!host.over(Point::new(10.0, 500.0)));
    assert!(!host.captures());
}

/// A state is written whole: a temporary file renamed over the old one.
#[test]
fn a_state_is_written_whole() {
    let dir = std::env::temp_dir().join(format!("pane-host-{:032x}", kernel::id::new_uuid()));
    let path = dir.join("k-chart.txt");
    write(&dir, &path, "one").expect("written");
    write(&dir, &path, "two").expect("written again");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "two");
    assert_eq!(std::fs::read_dir(&dir).expect("listed").count(), 1, "no temporary file left");
    let _ = std::fs::remove_dir_all(&dir);
}

#[derive(Debug, Default)]
struct Still;

impl crate::control::Control for Still {}

impl ResultPane for Still {
    fn set_rows(&mut self, _rows: Lent, _epoch: u64, _held: Held) {}
    fn paint(&mut self, _paint: crate::viewer::ViewerPaint<'_>) {}
}

#[derive(Debug)]
struct StillProvider;

impl ResultPaneProvider for StillProvider {
    fn id(&self) -> &'static str {
        "still-pane"
    }
    fn icon(&self) -> &'static str {
        "chart"
    }
    fn title(&self) -> String {
        String::new()
    }
    fn make(&self) -> Box<dyn ResultPane> {
        Box::new(Still)
    }
}

/// The wheel over the pane is the pane's even when it scrolls nothing: never the grid's
/// under it; elsewhere it goes on.
#[test]
fn the_wheel_over_a_pane_never_reaches_the_grid() {
    let mut host =
        PaneHost { offered: vec![std::sync::Arc::new(StillProvider)], ..PaneHost::default() };
    host.toggle(0);
    let (_, pane) = host.split(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    let pane = pane.expect("a pane");
    host.hover(pane.center());
    assert!(host.scroll(0.0, 3.0), "taken over the pane");
    host.hover(Point::new(10.0, 10.0));
    assert!(!host.scroll(0.0, 3.0), "over the grid: the grid's");
}

/// States changed quickly are written by one writer, one at a time, the newest of each file
/// last: never a second writer while one is on its way, never an older state over a newer.
#[test]
fn quick_states_are_written_one_at_a_time_the_newest_last() {
    let dir = std::env::temp_dir().join(format!("pane-writes-{:032x}", kernel::id::new_uuid()));
    let path = dir.join("state.txt");
    let writes = Arc::new(std::sync::Mutex::new(Writes::default()));
    let jobs: std::cell::RefCell<Vec<Box<dyn FnOnce() + Send>>> = Default::default();
    let spawn = |job: Box<dyn FnOnce() + Send>| jobs.borrow_mut().push(job);
    for text in ["kind", "options", "series"] {
        queue_write(&writes, (dir.clone(), path.clone(), text.to_owned()), &spawn);
    }
    assert_eq!(jobs.borrow().len(), 1, "one writer");
    for job in jobs.take() {
        job();
    }
    assert_eq!(std::fs::read_to_string(&path).ok().as_deref(), Some("series"));
    queue_write(&writes, (dir.clone(), path.clone(), "again".to_owned()), &spawn);
    assert_eq!(jobs.borrow().len(), 1, "drained: the next state starts one again");
    for job in jobs.take() {
        job();
    }
    assert_eq!(std::fs::read_to_string(&path).ok().as_deref(), Some("again"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A right-click's rows name every pane offered by its title (none by a name of the
/// host's); one chosen opens it drawn from the selection (the selection lent, said as
/// such); all the rows again, from the menu or the pane's own request; closed.
#[test]
fn a_pane_is_drawn_from_the_selection_and_back() {
    let (mut host, told) = host_with_pane();
    let menu = host.menu(true);
    let pick = menu
        .iter()
        .find_map(|e| match e {
            crate::ViewMenuEntry::Item { id, .. } if id.contains("pick:") => Some(id.clone()),
            _ => None,
        })
        .expect("a row per pane");
    let selection: Lent = Arc::new(charts_model::data::Rows::new(Vec::new()));
    assert!(host.menu_command(&pick, || Some(Arc::clone(&selection))));
    assert!(host.is_picked());
    host.lend(1, || Arc::new(charts_model::data::Rows::new(Vec::new())) as Lent, Held::default());
    assert_eq!(told.lock().unwrap().last().map(|h| h.picked), Some(true), "the selection lent");
    assert!(host.menu(true).iter().any(|e| matches!(e,
        crate::ViewMenuEntry::Item { id, .. } if id.ends_with("whole"))));
    assert!(host.menu_command(&format!("{PANE_MENU}whole"), || None));
    host.lend(2, || Arc::new(charts_model::data::Rows::new(Vec::new())) as Lent, Held::default());
    assert_eq!(told.lock().unwrap().last().map(|h| h.picked), Some(false), "all the rows");
    assert!(host.menu_command(&format!("{PANE_MENU}close"), || None));
    assert!(host.pane_ref().is_none());
    assert!(!host.menu_command("grid:copy", || None), "not the host's");
}
