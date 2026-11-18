use super::*;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::Config;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::Processor;

use crate::parked::tests::{history_rows, styled_output};

fn term(cols: usize, history: usize) -> Term<VoidListener> {
    let config = Config { scrolling_history: history, ..Config::default() };
    Term::new(config, &TermSize::new(cols, 10), VoidListener)
}

fn feed(term: &mut Term<VoidListener>, bytes: &[u8]) {
    Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new().advance(term, bytes);
}

fn numbered(from: usize, to: usize) -> Vec<u8> {
    (from..to)
        .map(|i| format!("\x1b[3{}mline {i}\x1b[0m\r\n", i % 8))
        .collect::<String>()
        .into_bytes()
}

/// Everything a terminal holds that parking may touch: every row (history and screen), the
/// history's length, the scroll, the cursors.
fn state(term: &Term<VoidListener>) -> (Vec<Row<Cell>>, usize, usize, Point, Point) {
    let grid = term.grid();
    let mut rows = history_rows(term);
    rows.extend((0..grid.screen_lines()).map(|l| grid[Line(l as i32)].clone()));
    (rows, grid.history_size(), grid.display_offset(), grid.cursor.point, grid.saved_cursor.point)
}

/// Parks `term`'s history as the session does: taken out, packed, then put back.
fn park(term: &mut Term<VoidListener>) -> (Parked, usize) {
    let columns = term.columns();
    let offset = term.grid().display_offset();
    let rows = take_history(term.grid_mut());
    let mut parked = Parked::default();
    parked.push(&rows, columns);
    (parked, offset)
}

fn put(term: &mut Term<VoidListener>, mut parked: Parked, limit: usize, scrolled: usize) {
    let rows: Vec<Row<Cell>> = parked.take().into_iter().flat_map(|(_, rows)| rows).collect();
    put_back(term.grid_mut(), rows, limit, scrolled);
}

/// Each logical line (rows joined by their wraps), trimmed.
fn lines(rows: &[Row<Cell>]) -> Vec<String> {
    let mut out = vec![String::new()];
    for row in rows {
        let last = out.last_mut().unwrap();
        last.extend(
            row[..]
                .iter()
                .filter(|c| {
                    !c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                })
                .map(|c| c.c),
        );
        if !row.last().is_some_and(|c| c.flags.contains(Flags::WRAPLINE)) {
            *last = last.trim_end().to_owned();
            out.push(String::new());
        }
    }
    out.pop();
    out
}

#[test]
fn parked_and_put_back_the_terminal_is_the_same() {
    let mut term = term(40, 10_000);
    feed(&mut term, &styled_output(600));
    let before = state(&term);
    let (parked, offset) = park(&mut term);
    // Only the history left: the screen and the cursor stayed.
    assert_eq!(term.grid().history_size(), 0);
    let screen = state(&term);
    assert_eq!(&screen.0[..], &before.0[before.1..]);
    assert_eq!((screen.3, screen.4), (before.3, before.4));
    put(&mut term, parked, 10_000, offset);
    assert!(state(&term) == before, "not the same after coming back");
}

/// The output while it's parked goes into the grid; back, the parked rows are above it, in
/// order, as if they had never left — scrolled back or not, at the history's limit or not.
#[test]
fn output_while_parked_goes_below_the_parked_rows() {
    for (limit, scrolled) in [(10_000, 0), (10_000, 37), (1_000, 0), (1_000, 25)] {
        let mut kept = term(40, limit);
        let mut parked_one = term(40, limit);
        let mut both = |bytes: &[u8]| {
            feed(&mut kept, bytes);
            feed(&mut parked_one, bytes);
        };
        both(&styled_output(900));
        kept.scroll_display(Scroll::Delta(scrolled as i32));
        parked_one.scroll_display(Scroll::Delta(scrolled as i32));
        let (parked, offset) = park(&mut parked_one);
        assert_eq!(offset, scrolled);
        feed(&mut kept, &numbered(0, 400));
        feed(&mut parked_one, &numbered(0, 400));
        put(&mut parked_one, parked, limit, offset);
        assert!(state(&kept) == state(&parked_one), "limit {limit}, scrolled {scrolled}");
        assert!(kept.grid().history_size() <= limit);
    }
}

/// Rows parked at another width (a full-screen program had the screen through a resize)
/// come back flowed as the resize flowed the rest: the same lines.
#[test]
fn rows_of_another_width_are_flowed_again() {
    let mut out = styled_output(200);
    out.extend(numbered(0, 50));
    for (from, to) in [(80, 33), (33, 80), (60, 61)] {
        let mut wide = term(from, 10_000);
        feed(&mut wide, &out);
        let rows = history_rows(&wide);
        let flowed = reflow(rows.clone(), from, to);
        assert!(flowed.iter().all(|r| r.len() == to));
        assert_eq!(lines(&flowed), lines(&rows), "{from} → {to}");
    }
}

/// A session's terminal, its parking (parking at once), the erases its reader counts and
/// whether it was told its rows came back.
type Session = (
    Arc<FairMutex<Term<VoidListener>>>,
    Arc<Parking<VoidListener>>,
    Arc<AtomicU64>,
    Arc<AtomicBool>,
);

fn shared(cols: usize, limit: usize) -> Session {
    let term = Arc::new(FairMutex::new(term(cols, limit)));
    let erased = Arc::new(AtomicU64::new(0));
    let told = Arc::new(AtomicBool::new(false));
    let toldc = Arc::clone(&told);
    let parking = Arc::new(Parking::new(
        &term,
        Arc::clone(&erased),
        limit,
        Box::new(move || toldc.store(true, Ordering::SeqCst)),
    ));
    parking.park_after.store(0, Ordering::Relaxed);
    (term, parking, erased, told)
}

#[test]
fn a_session_parks_counts_and_comes_back() {
    let (term, parking, _, _) = shared(40, 10_000);
    feed(&mut term.lock(), &styled_output(800));
    let before = state(&term.lock());
    parking.park();
    let history = before.1;
    assert_eq!(term.lock().grid().history_size(), 0);
    // The history's length shown counts the parked rows; their memory is a fraction.
    assert_eq!(parking.rows(), history);
    assert_eq!(parking.history(0), history);
    let cells = history * 40 * std::mem::size_of::<Cell>();
    assert!(parking.bytes() > 0 && parking.bytes() * 5 < cells, "{} of {cells}", parking.bytes());
    assert!(parking.restore(true));
    assert_eq!(parking.rows(), 0);
    assert_eq!(parking.bytes(), 0);
    assert!(state(&term.lock()) == before);
}

#[test]
fn not_parked_while_drawn_nor_with_little_history() {
    let (term, parking, _, _) = shared(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 100));
    parking.park();
    assert_eq!(parking.rows(), 0, "too little");
    feed(&mut term.lock(), &numbered(0, 1000));
    parking.park_after.store(3_600_000, Ordering::Relaxed);
    parking.restore(true);
    parking.park();
    assert_eq!(parking.rows(), 0, "drawn just now");
}

/// Drawn again, the rows come back on a worker (the history's length right meanwhile), and
/// the view is told.
#[test]
fn drawn_again_the_rows_come_back_on_a_worker() {
    let (term, parking, _, told) = shared(40, 10_000);
    feed(&mut term.lock(), &styled_output(800));
    let before = state(&term.lock());
    parking.park();
    assert!(parking.rows() > 0);
    assert!(parking.shown(false));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !told.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline, "never came back");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(parking.rows(), 0);
    assert!(state(&term.lock()) == before);
}

/// Parked scrolled back, it comes back at once when drawn: what's shown is in those rows.
#[test]
fn parked_scrolled_back_comes_back_at_once() {
    let (term, parking, _, _) = shared(40, 10_000);
    feed(&mut term.lock(), &styled_output(800));
    term.lock().scroll_display(Scroll::Delta(120));
    let before = state(&term.lock());
    parking.park();
    assert!(parking.must_restore());
    assert!(parking.shown(true));
    assert!(!parking.must_restore());
    assert!(state(&term.lock()) == before);
}

/// A program erasing the history while it's parked (counted by the reader) erases the
/// parked rows too; Clear Terminal as well.
#[test]
fn an_erase_while_parked_lets_the_rows_go() {
    let (term, parking, erased, _) = shared(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 800));
    parking.park();
    feed(&mut term.lock(), b"\x1b[3J");
    erased.fetch_add(1, Ordering::SeqCst);
    feed(&mut term.lock(), &numbered(800, 820));
    assert!(parking.restore(true));
    assert_eq!(parking.rows(), 0);
    assert_eq!(term.lock().grid().history_size(), 20);
    // Parked again (only the rows since), then cleared by the user.
    feed(&mut term.lock(), &numbered(820, 1500));
    parking.park();
    assert!(parking.rows() > 600);
    parking.forget();
    assert_eq!(parking.rows(), 0);
    assert_eq!(parking.history(0), 0);
}

/// While a full-screen program has its own screen, the rows stay parked; once it leaves,
/// they come back where they were.
#[test]
fn a_full_screen_program_keeps_them_parked() {
    let (term, parking, _, _) = shared(40, 10_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    let both = |t: &mut Term<VoidListener>, k: &mut Term<VoidListener>, bytes: &[u8]| {
        feed(t, bytes);
        feed(k, bytes);
    };
    both(&mut term.lock(), &mut kept, &numbered(0, 700));
    parking.park();
    both(&mut term.lock(), &mut kept, b"\x1b[?1049hfull screen\x1b[2;1Hmore");
    assert!(parking.restore(true));
    assert!(parking.rows() > 0, "still parked");
    // Not parked again from the program's screen.
    parking.park();
    both(&mut term.lock(), &mut kept, b"\x1b[?1049l");
    both(&mut term.lock(), &mut kept, &numbered(700, 720));
    assert!(parking.restore(true));
    assert_eq!(parking.rows(), 0);
    assert!(state(&term.lock()) == state(&kept));
}

/// A lower limit chosen while parked lets the oldest go at once.
#[test]
fn a_lower_limit_lets_the_oldest_parked_rows_go() {
    let (term, parking, _, _) = shared(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 3000));
    parking.park();
    term.lock().set_options(Config { scrolling_history: 1000, ..Config::default() });
    parking.set_limit(1000);
    assert_eq!(parking.rows(), 1000);
    assert!(parking.restore(true));
    let grid_rows = history_rows(&term.lock());
    assert_eq!(grid_rows.len(), 1000);
    assert_eq!(lines(&grid_rows[..1])[0], "line 1991");
}

/// What parking costs, a busy terminal's worth (`--ignored`, in release: the numbers in the
/// plan): 5000 history rows at 200 columns, colored lines as an agent's transcript prints.
#[test]
#[ignore = "a measurement"]
fn measure_park_and_restore() {
    let (term, parking, _, _) = shared(200, 5_000);
    let mut out = Vec::new();
    for i in 0..6_000 {
        out.extend(
            format!(
                "\x1b[38;2;{};120;80m{i:6}\x1b[0m \x1b[1mthe quick brown fox jumps over the lazy dog\x1b[0m {}\r\n",
                i % 200,
                "function return const let ".repeat(i % 4)
            )
            .into_bytes(),
        );
    }
    feed(&mut term.lock(), &out);
    let before = state(&term.lock());
    let cells = before.1 * 200 * std::mem::size_of::<Cell>();
    for _ in 0..3 {
        let at = Instant::now();
        let rows = take_history(term.lock().grid_mut());
        let taken = at.elapsed();
        let at = Instant::now();
        let mut parked = Parked::default();
        parked.push(&rows, 200);
        let packed = at.elapsed();
        let at = Instant::now();
        drop(rows);
        let freed = at.elapsed();
        let bytes = parked.bytes();
        let at = Instant::now();
        let runs = parked.take();
        let unpacked = at.elapsed();
        let at = Instant::now();
        let rows: Vec<Row<Cell>> = runs.into_iter().flat_map(|(_, rows)| rows).collect();
        put_back(term.lock().grid_mut(), rows, 5_000, 0);
        let put = at.elapsed();
        eprintln!(
            "{} rows × 200: cells {:.1} MB → parked {:.2} MB; taken out {taken:?} (locked), \
             packed {packed:?}, freed {freed:?}; unpacked {unpacked:?}, put back {put:?} (locked)",
            before.1,
            cells as f64 / 1048576.0,
            bytes as f64 / 1048576.0,
        );
        assert!(state(&term.lock()) == before);
    }
    let _ = parking;
}
