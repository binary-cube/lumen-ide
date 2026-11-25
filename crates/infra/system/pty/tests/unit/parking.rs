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

/// A store keeping what it's given in memory (the terminal's keeps it on disk, sealed);
/// `refuse`: every write fails.
#[derive(Debug, Default)]
struct Kept {
    files: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
    refuse: AtomicBool,
    /// How many times something was read back.
    takes: std::sync::atomic::AtomicUsize,
    /// While set, a write waits until it's cleared (a slow disk: rows stay on their way).
    hold: AtomicBool,
    /// A read takes this long (a slow disk).
    slow_take: AtomicBool,
    /// While set, a read waits until it's cleared.
    hold_take: AtomicBool,
    /// How many writes were asked; whether it says it's ready.
    writes: std::sync::atomic::AtomicUsize,
    not_ready: AtomicBool,
}

impl Kept {
    fn count(&self) -> usize {
        self.files.lock().unwrap().len()
    }
}

impl SpillStore for Kept {
    fn ready(&self) -> bool {
        !self.not_ready.load(Ordering::SeqCst)
    }
    fn write(&self, bytes: &[u8]) -> Option<SpillToken> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        while self.hold.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.refuse.load(Ordering::SeqCst) {
            return None;
        }
        let name = format!("{:032x}", kernel::id::new_uuid());
        self.files.lock().unwrap().push((name.clone(), bytes.to_vec()));
        Some(SpillToken(name))
    }
    fn take(&self, token: &SpillToken) -> Option<Vec<u8>> {
        self.takes.fetch_add(1, Ordering::SeqCst);
        while self.hold_take.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.slow_take.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(400));
        }
        let mut files = self.files.lock().unwrap();
        let at = files.iter().position(|(n, _)| *n == token.0)?;
        Some(files.remove(at).1)
    }
    fn remove(&self, token: &SpillToken) {
        self.files.lock().unwrap().retain(|(n, _)| *n != token.0);
    }
}

/// A session whose parked rows leave memory as soon as they're parked.
fn spilling(cols: usize, limit: usize) -> (Session, Arc<Kept>) {
    let session = shared(cols, limit);
    let store = Arc::new(Kept::default());
    session.1.set_spill(Arc::clone(&store) as Arc<dyn SpillStore>);
    session.1.spill_after.store(0, Ordering::Relaxed);
    (session, store)
}

/// Waits until `done` (a worker's work), at most a few seconds.
fn until(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "never done");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Unseen long enough, the parked rows leave memory (only their count stays); a frame never
/// reads them back itself (a worker does), and they come back the same.
#[test]
fn parked_rows_leave_memory_and_come_back_the_same() {
    let ((term, parking, _, told), store) = spilling(40, 10_000);
    feed(&mut term.lock(), &styled_output(800));
    let before = state(&term.lock());
    parking.spill_after.store(3_600_000, Ordering::Relaxed);
    Parkable::park(&*parking);
    let parked = parking.bytes();
    parking.spill_after.store(0, Ordering::Relaxed);
    parking.spill_if_due();
    assert_eq!(store.count(), 1, "out of memory");
    assert_eq!(parking.rows(), before.1, "still counted in the history's length");
    // What stays: the rare cells with more (this output has a link or a mark every 8 lines).
    assert!(parking.bytes() * 3 < parked, "{} of {parked} bytes", parking.bytes());
    assert!(!parking.restore(false), "a frame doesn't read it");
    assert!(parking.shown(false));
    until(|| told.load(Ordering::SeqCst));
    assert_eq!((parking.rows(), store.count()), (0, 0), "read back and let go");
    assert!(state(&term.lock()) == before);
}

/// Rows parked after others went out of memory go out on their own and come back after
/// them (newer), in order.
#[test]
fn later_rows_leave_memory_apart_and_come_back_in_order() {
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    feed(&mut term.lock(), &numbered(0, 600));
    feed(&mut kept, &numbered(0, 600));
    Parkable::park(&*parking);
    feed(&mut term.lock(), &numbered(600, 1300));
    feed(&mut kept, &numbered(600, 1300));
    Parkable::park(&*parking);
    assert_eq!(store.count(), 2);
    assert!(parking.restore(true));
    assert!(state(&term.lock()) == state(&kept));
}

/// Erased or cleared while out of memory: let go unread; the terminal closed: the same;
/// a store that can't keep them: they stay in memory.
#[test]
fn rows_out_of_memory_are_let_go_with_the_history() {
    let ((term, parking, erased, _), store) = spilling(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 800));
    Parkable::park(&*parking);
    erased.fetch_add(1, Ordering::SeqCst);
    assert!(parking.restore(true));
    until(|| store.count() == 0);
    feed(&mut term.lock(), &numbered(800, 1600));
    Parkable::park(&*parking);
    assert_eq!(store.count(), 1);
    parking.forget();
    until(|| store.count() == 0);
    feed(&mut term.lock(), &numbered(1600, 2400));
    Parkable::park(&*parking);
    assert_eq!(store.count(), 1);
    drop(parking);
    until(|| store.count() == 0);
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    store.refuse.store(true, Ordering::SeqCst);
    feed(&mut term.lock(), &numbered(0, 800));
    let before = state(&term.lock());
    Parkable::park(&*parking);
    assert!(parking.rows() > 0 && parking.bytes() > 4_096, "kept in memory");
    assert!(parking.restore(true));
    assert!(state(&term.lock()) == before);
}

/// Not while it's drawn, nor scrolled back (what's shown is in those rows).
#[test]
fn rows_stay_in_memory_while_drawn_or_scrolled_back() {
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 800));
    parking.spill_after.store(3_600_000, Ordering::Relaxed);
    Parkable::park(&*parking);
    assert_eq!(store.count(), 0, "drawn lately");
    parking.restore(true);
    parking.spill_after.store(0, Ordering::Relaxed);
    term.lock().scroll_display(Scroll::Delta(50));
    Parkable::park(&*parking);
    assert_eq!(store.count(), 0, "scrolled back");
}

/// What leaving memory saves, a busy terminal's worth (`--ignored`, in release: the numbers
/// in the plan): 5000 history rows at 200 columns as in `measure_park_and_restore`.
#[test]
#[ignore = "a measurement"]
fn measure_spill() {
    let ((term, parking, _, _), store) = spilling(200, 5_000);
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
    parking.spill_after.store(3_600_000, Ordering::Relaxed);
    Parkable::park(&*parking);
    let parked = parking.bytes();
    parking.spill_after.store(0, Ordering::Relaxed);
    let at = Instant::now();
    parking.spill_if_due();
    let spilled = at.elapsed();
    let left = parking.bytes();
    let encoded = store.files.lock().unwrap()[0].1.clone();
    let at = Instant::now();
    let packed = miniz_oxide::deflate::compress_to_vec(&encoded, 1);
    let compressed = at.elapsed();
    let at = Instant::now();
    assert!(parking.restore(true));
    let back = at.elapsed();
    eprintln!(
        "{} rows × 200: cells {:.1} MB → parked {:.2} MB → out of memory {} B left; \
         encoded {:.2} MB, compressed {:.0} KB (+28 B sealed); encoded in {spilled:?}, \
         compressed in {compressed:?}, read back and put back {back:?}",
        before.1,
        cells as f64 / 1048576.0,
        parked as f64 / 1048576.0,
        left,
        encoded.len() as f64 / 1048576.0,
        packed.len() as f64 / 1024.0,
    );
    assert!(state(&term.lock()) == before);
}

/// A busy hidden terminal at its limit: the oldest rows out of memory go by their counts —
/// whole files let go, the oldest one left cut when read — never read back for it; what
/// comes back is the newest rows, in order.
#[test]
fn over_the_limit_the_oldest_rows_go_unread() {
    let ((term, parking, _, _), store) = spilling(40, 1_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    kept.set_options(Config { scrolling_history: 1_000, ..Config::default() });
    let mut fed = 0;
    for _ in 0..4 {
        feed(&mut term.lock(), &numbered(fed, fed + 600));
        feed(&mut kept, &numbered(fed, fed + 600));
        fed += 600;
        Parkable::park(&*parking);
    }
    assert_eq!(store.takes.load(Ordering::SeqCst), 0, "nothing read back to trim");
    assert_eq!(parking.rows(), 1_000);
    until(|| store.count() <= 2); // whole files let go (on a worker)
    assert!(parking.restore(true));
    assert!(state(&term.lock()) == state(&kept), "the newest 1000, in order");
}

/// The UI thread's calls (a limit told as a frame is drawn, Clear Terminal, a frame's
/// restore) never read the rows out of memory back.
#[test]
fn the_ui_thread_never_reads_rows_out_of_memory() {
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 3000));
    Parkable::park(&*parking);
    assert_eq!(store.count(), 1);
    parking.set_limit(1_000);
    assert_eq!(parking.rows(), 1_000, "trimmed by its count");
    assert!(!parking.restore(false), "a frame waits for the worker");
    parking.forget();
    assert_eq!(parking.rows(), 0);
    assert_eq!(store.takes.load(Ordering::SeqCst), 0);
    until(|| store.count() == 0);
}

/// While new rows leave the grid, the length counted takes those out of memory too (the
/// scrollbar never jumps for a frame).
#[test]
fn the_count_while_parking_includes_rows_out_of_memory() {
    let ((term, parking, _, _), _store) = spilling(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 800));
    Parkable::park(&*parking);
    let out = parking.rows();
    assert!(out > 0);
    let store = parking.store.lock().unwrap();
    parking.counting(&store, 300);
    assert_eq!(parking.rows(), out + 300);
}

/// A resize or a scroll (the UI thread) never reads rows out of memory: what's in memory is
/// put back at once, the rest by a worker after, in order.
#[test]
fn the_ui_thread_puts_back_only_what_is_in_memory() {
    let ((term, parking, _, told), store) = spilling(40, 10_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    for (from, to) in [(0, 700), (700, 1400)] {
        feed(&mut term.lock(), &numbered(from, to));
        feed(&mut kept, &numbered(from, to));
        Parkable::park(&*parking);
    }
    assert_eq!(store.count(), 2);
    store.slow_take.store(true, Ordering::SeqCst);
    let at = Instant::now();
    parking.restore_now();
    assert!(at.elapsed() < Duration::from_millis(200), "no file read: {:?}", at.elapsed());
    until(|| told.load(Ordering::SeqCst));
    assert_eq!(parking.rows(), 0);
    assert!(state(&term.lock()) == state(&kept), "all back, in order");
}

/// A restore while newer rows are on their way out waits for them (never puts older ones
/// back first, which the newer would then land above).
#[test]
fn rows_come_back_in_order_around_a_spill_in_flight() {
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    feed(&mut term.lock(), &numbered(0, 700));
    feed(&mut kept, &numbered(0, 700));
    Parkable::park(&*parking);
    feed(&mut term.lock(), &numbered(700, 1400));
    feed(&mut kept, &numbered(700, 1400));
    parking.spill_after.store(3_600_000, Ordering::Relaxed);
    parking.park();
    parking.spill_after.store(0, Ordering::Relaxed);
    store.hold.store(true, Ordering::SeqCst);
    let writer = {
        let parking = Arc::clone(&parking);
        std::thread::spawn(move || parking.spill_if_due())
    };
    until(|| parking.store.lock().unwrap().moving > 0);
    assert!(!parking.restore(false), "on their way: nothing put back");
    store.hold.store(false, Ordering::SeqCst);
    writer.join().unwrap();
    assert!(parking.restore(true));
    assert!(state(&term.lock()) == state(&kept), "in order");
}

/// A lower limit told while rows are on their way out cuts those (the oldest), never the
/// newest parked ones.
#[test]
fn a_lower_limit_meanwhile_cuts_the_rows_on_their_way() {
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    kept.set_options(Config { scrolling_history: 1_000, ..Config::default() });
    feed(&mut term.lock(), &numbered(0, 1500));
    feed(&mut kept, &numbered(0, 1500));
    parking.spill_after.store(3_600_000, Ordering::Relaxed);
    parking.park();
    parking.spill_after.store(0, Ordering::Relaxed);
    store.hold.store(true, Ordering::SeqCst);
    let writer = {
        let parking = Arc::clone(&parking);
        std::thread::spawn(move || parking.spill_if_due())
    };
    until(|| parking.store.lock().unwrap().moving > 0);
    term.lock().set_options(Config { scrolling_history: 1_000, ..Config::default() });
    parking.set_limit(1_000);
    assert_eq!(parking.rows(), 1_000);
    store.hold.store(false, Ordering::SeqCst);
    writer.join().unwrap();
    assert_eq!(parking.rows(), 1_000);
    assert!(parking.restore(true));
    assert!(state(&term.lock()) == state(&kept), "the newest 1000 kept");
}

/// A store that couldn't write isn't given the history again before a while (nothing encoded
/// for nothing); one saying it isn't ready isn't given it at all.
#[test]
fn a_failing_store_is_left_alone_for_a_while() {
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    store.refuse.store(true, Ordering::SeqCst);
    feed(&mut term.lock(), &numbered(0, 800));
    Parkable::park(&*parking);
    assert_eq!(store.writes.load(Ordering::SeqCst), 1);
    feed(&mut term.lock(), &numbered(800, 1600));
    Parkable::park(&*parking);
    assert_eq!(store.writes.load(Ordering::SeqCst), 1, "not asked again so soon");
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    store.not_ready.store(true, Ordering::SeqCst);
    feed(&mut term.lock(), &numbered(0, 800));
    Parkable::park(&*parking);
    assert_eq!(store.writes.load(Ordering::SeqCst), 0, "not ready: not asked");
    assert!(parking.rows() > 0, "kept in memory");
}

/// A read back never starts while newer rows are being written out, and a write never
/// starts while rows are being read back: the history comes back in order either way.
#[test]
fn a_spill_and_a_read_never_cross() {
    // A write under way: a read doesn't start.
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    for (from, to) in [(0, 700), (700, 1400)] {
        feed(&mut term.lock(), &numbered(from, to));
        feed(&mut kept, &numbered(from, to));
        parking.spill_after.store(if from == 0 { 0 } else { 3_600_000 }, Ordering::Relaxed);
        Parkable::park(&*parking);
    }
    parking.spill_after.store(0, Ordering::Relaxed);
    store.hold.store(true, Ordering::SeqCst);
    let writer = {
        let parking = Arc::clone(&parking);
        std::thread::spawn(move || parking.spill_if_due())
    };
    until(|| parking.store.lock().unwrap().moving > 0);
    parking.read_spilled();
    assert_eq!(store.takes.load(Ordering::SeqCst), 0, "no read while a write is under way");
    store.hold.store(false, Ordering::SeqCst);
    writer.join().unwrap();
    assert!(parking.restore(true));
    assert!(state(&term.lock()) == state(&kept), "in order");
    // A read under way: a write doesn't start.
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    let mut kept = super::super::parked::tests::term_after(40, b"");
    feed(&mut term.lock(), &numbered(0, 700));
    feed(&mut kept, &numbered(0, 700));
    Parkable::park(&*parking);
    store.hold_take.store(true, Ordering::SeqCst);
    let reader = {
        let parking = Arc::clone(&parking);
        std::thread::spawn(move || parking.read_spilled())
    };
    until(|| parking.store.lock().unwrap().moving > 0);
    feed(&mut term.lock(), &numbered(700, 1400));
    feed(&mut kept, &numbered(700, 1400));
    parking.park();
    let writes = store.writes.load(Ordering::SeqCst);
    parking.spill_if_due();
    assert_eq!(store.writes.load(Ordering::SeqCst), writes, "no write while a read is under way");
    store.hold_take.store(false, Ordering::SeqCst);
    reader.join().unwrap();
    assert!(parking.restore(true));
    assert!(state(&term.lock()) == state(&kept), "in order");
}

/// Parked scrolled back while older rows are out of memory: where the view was is kept
/// (it comes back at once, drawn as it was).
#[test]
fn a_scrolled_back_park_is_kept_with_rows_out_of_memory() {
    let ((term, parking, _, _), store) = spilling(40, 10_000);
    feed(&mut term.lock(), &numbered(0, 700));
    Parkable::park(&*parking);
    assert_eq!(store.count(), 1);
    feed(&mut term.lock(), &numbered(700, 1400));
    term.lock().scroll_display(Scroll::Delta(50));
    let before = state(&term.lock());
    parking.spill_after.store(3_600_000, Ordering::Relaxed);
    parking.park();
    assert!(parking.must_restore(), "scrolled back: put back at once when drawn");
    assert!(parking.shown(true));
    let after = state(&term.lock());
    assert_eq!(after.2, before.2, "the same place in the history");
}
