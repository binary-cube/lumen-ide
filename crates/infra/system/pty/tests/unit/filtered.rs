use std::collections::VecDeque;
use std::time::Duration;

use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::test::TermSize as GridSize;
use alacritty_terminal::vte::ansi::Processor;
use terminal_session::history::Foreground;

use std::sync::Mutex;

use super::*;

/// A PTY whose output is the chunks given, then "nothing more for now".
struct FakePty {
    reader: Chunks,
    writer: Vec<u8>,
}

struct Chunks(VecDeque<Vec<u8>>);

impl Read for Chunks {
    /// An empty chunk: "nothing for now" once (the reader drained between two writes).
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(mut chunk) = self.0.pop_front() else {
            return Err(io::ErrorKind::WouldBlock.into());
        };
        if chunk.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = chunk.len().min(buf.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        if n < chunk.len() {
            self.0.push_front(chunk.split_off(n));
        }
        Ok(n)
    }
}

#[allow(unsafe_code)]
impl EventedReadWrite for FakePty {
    type Reader = Chunks;
    type Writer = Vec<u8>;

    unsafe fn register(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()> {
        Ok(())
    }

    fn reregister(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()> {
        Ok(())
    }

    fn deregister(&mut self, _: &Arc<Poller>) -> io::Result<()> {
        Ok(())
    }

    fn reader(&mut self) -> &mut Chunks {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut Vec<u8> {
        &mut self.writer
    }
}

struct Quiet;
impl EventListener for Quiet {}

/// What a terminal shows after the PTY's `chunks` went through the filter (read as
/// alacritty's loop does: until "nothing more"), with `foreground` in it under `policy`.
fn history_after(chunks: Vec<Vec<u8>>, policy: HistoryClear, foreground: Foreground) -> usize {
    let pty = FakePty { reader: Chunks(chunks.into()), writer: Vec::new() };
    let mut filtered = Filtered::new(
        pty,
        Arc::new(AtomicU8::new(policy.to_u8())),
        Arc::new(Mutex::new(Probe::new(move || foreground))),
        Redrawing::default(),
        Arc::default(),
    );
    let config =
        alacritty_terminal::term::Config { scrolling_history: 10_000, ..Default::default() };
    let mut term = Term::new(config, &GridSize::new(80, 24), Quiet);
    let mut parser: Processor = Processor::new();
    let mut buf = vec![0u8; 4096];
    loop {
        match filtered.reader().read(&mut buf) {
            Ok(0) => break,
            Ok(n) => parser.advance(&mut term, &buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => panic!("{e}"),
        }
    }
    term.grid().history_size()
}

/// 500 lines, then a program's full-screen redraws erasing the history each time (split
/// anywhere across reads), as Ink-based programs send them.
fn output() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    for i in 0..500 {
        all.extend(format!("line {i}\r\n").into_bytes());
    }
    for _ in 0..20 {
        all.extend(b"\x1b[2J\x1b[3J\x1b[Hframe\r\nframe\r\n");
    }
    // Uneven reads: a sequence cut in two now and then.
    all.chunks(7).map(<[u8]>::to_vec).collect()
}

const AGENT: Foreground = Foreground::Program(Duration::from_secs(120));
const CLEAR: Foreground = Foreground::Program(Duration::from_millis(20));

/// What scrolled up before the redraws: 500 lines and the empty one under them, less a
/// screen (24).
const BEFORE: usize = 501 - 24;

#[test]
fn a_long_running_program_redrawing_keeps_the_history() {
    let history = history_after(output(), HistoryClear::KeepForRunningPrograms, AGENT);
    // Exactly: its redraws' screen erases push nothing (plan 55h).
    assert_eq!(history, BEFORE);
}

#[test]
fn clear_from_the_shell_empties_it() {
    assert_eq!(history_after(output(), HistoryClear::KeepForRunningPrograms, Foreground::Shell), 0);
    assert_eq!(history_after(output(), HistoryClear::KeepForRunningPrograms, CLEAR), 0);
}

#[test]
fn each_policy() {
    assert_eq!(history_after(output(), HistoryClear::Standard, AGENT), 0);
    assert_eq!(history_after(output(), HistoryClear::KeepAlways, Foreground::Shell), BEFORE);
    assert_eq!(history_after(output(), HistoryClear::KeepAlways, CLEAR), BEFORE);
}

#[test]
fn output_without_the_sequence_passes_unchanged() {
    let text = b"plain \x1b[1;31mred\x1b[0m \x1b]0;title\x07 \xe2\x9c\x93 done\r\n".to_vec();
    let chunks: Vec<Vec<u8>> = text.chunks(3).map(<[u8]>::to_vec).collect();
    let pty = FakePty { reader: Chunks(chunks.into()), writer: Vec::new() };
    let mut filtered = Filtered::new(
        pty,
        Arc::new(AtomicU8::new(HistoryClear::KeepAlways.to_u8())),
        Arc::new(Mutex::new(Probe::new(|| unreachable!("asked only for an ESC[3J")))),
        Redrawing::default(),
        Arc::default(),
    );
    let mut read = Vec::new();
    let mut buf = [0u8; 2];
    while let Ok(n) = filtered.reader().read(&mut buf) {
        read.extend_from_slice(&buf[..n]);
    }
    assert_eq!(read, text);
}

/// What reaches the terminal: output, or the panel resized (the program redraws after).
enum Step {
    Output(Vec<u8>),
    Resize(usize, usize),
}

/// The history's lines (oldest first) after `steps` went through the filter with a
/// long-running program in the foreground.
fn history_lines(steps: Vec<Step>, policy: HistoryClear, foreground: Foreground) -> Vec<String> {
    use alacritty_terminal::index::{Column, Line};
    let config =
        alacritty_terminal::term::Config { scrolling_history: 10_000, ..Default::default() };
    let mut term = Term::new(config, &GridSize::new(80, 24), Quiet);
    let mut parser: Processor = Processor::new();
    let policy = Arc::new(AtomicU8::new(policy.to_u8()));
    let probe = Arc::new(Mutex::new(Probe::new(move || foreground)));
    let redrawing = Redrawing::default();
    for step in steps {
        match step {
            // As the session resizes: keeping the history while a program redraws.
            Step::Resize(columns, lines) => {
                let redrawn = redrawing.lock().unwrap().is_some();
                crate::resize_keeping_history(&mut term, GridSize::new(columns, lines), redrawn);
            }
            Step::Output(bytes) => {
                // Read as alacritty does: in uneven pieces, until "nothing more".
                let chunks = bytes.chunks(7).map(<[u8]>::to_vec).collect::<Vec<_>>();
                let pty = FakePty { reader: Chunks(chunks.into()), writer: Vec::new() };
                let mut filtered = Filtered::new(
                    pty,
                    Arc::clone(&policy),
                    Arc::clone(&probe),
                    Arc::clone(&redrawing),
                    Arc::default(),
                );
                let mut buf = vec![0u8; 4096];
                loop {
                    match filtered.reader().read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => parser.advance(&mut term, &buf[..n]),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => panic!("{e}"),
                    }
                }
            }
        }
    }
    let grid = term.grid();
    let history = grid.history_size();
    (1..=history)
        .rev()
        .map(|i| {
            let row = &grid[Line(-(i as i32))];
            (0..grid.columns()).map(|c| row[Column(c)].c).collect::<String>().trim_end().to_owned()
        })
        .collect()
}

/// A program's frame at `columns`: ten lines, the input box a full-width inverse bar.
fn frame(n: usize, columns: usize) -> Vec<u8> {
    let mut out = b"\x1b[2J\x1b[3J\x1b[H".to_vec();
    out.extend(format!("frame {n} header\r\n").into_bytes());
    for i in 0..8 {
        out.extend(format!("frame {n} line {i}\r\n").into_bytes());
    }
    out.extend(b"\x1b[7m");
    out.extend(format!("> input {n}").into_bytes());
    out.extend(std::iter::repeat_n(b' ', columns.saturating_sub(9)));
    out.extend(b"\x1b[0m");
    out
}

/// The panel resized 20 times while a long-running program redraws (2J 3J H and a frame
/// with a full-width inverse line, after each resize): the history holds what ran before it
/// once, no copy of its frames (plan 55h: each redraw's ED 2 used to push the previous frame
/// into it once its ED 3 was kept).
#[test]
fn a_programs_redraws_across_resizes_leave_no_frames_in_the_history() {
    let mut earlier = Vec::new();
    for i in 0..100 {
        earlier.extend(format!("earlier {i}\r\n").into_bytes());
    }
    let mut steps = vec![Step::Output(earlier), Step::Output(frame(0, 80))];
    for n in 1..=20 {
        let (columns, lines) = if n % 2 == 0 { (80, 24) } else { (60, 18) };
        steps.push(Step::Resize(columns, lines));
        steps.push(Step::Output(frame(n, columns)));
    }
    let history = history_lines(steps, HistoryClear::KeepForRunningPrograms, AGENT);
    assert!(history.iter().all(|l| !l.contains("frame") && !l.contains("input")), "{history:#?}");
    let earlier: Vec<&String> = history.iter().filter(|l| l.starts_with("earlier")).collect();
    assert_eq!(earlier.len(), history.len(), "only the earlier output: {history:#?}");
    for (i, line) in earlier.iter().enumerate() {
        assert_eq!(**line, format!("earlier {i}"), "once each, in order");
    }
    assert!(earlier.len() >= 100 - 24, "{}", earlier.len());
}

/// The shell's `clear` still empties the history; a program's lone ED 2 (no ED 3) still
/// pushes the screen into it, as before.
#[test]
fn clear_still_empties_it_and_a_lone_screen_erase_is_unchanged() {
    let mut earlier = Vec::new();
    for i in 0..100 {
        earlier.extend(format!("earlier {i}\r\n").into_bytes());
    }
    let clear = b"\x1b[H\x1b[2J\x1b[3J".to_vec();
    let steps = vec![Step::Output(earlier.clone()), Step::Output(clear)];
    let history = history_lines(steps, HistoryClear::KeepForRunningPrograms, Foreground::Shell);
    assert!(history.is_empty(), "{history:?}");
    let lone = b"\x1b[2J\x1b[Hdrawn".to_vec();
    let steps = vec![Step::Output(earlier), Step::Output(lone)];
    let history = history_lines(steps, HistoryClear::KeepForRunningPrograms, AGENT);
    // As before: the screen pushed up into the history (all 100 lines there).
    assert_eq!(history.iter().filter(|l| l.starts_with("earlier")).count(), 100, "pushed up");
}

/// A program writing `ESC[2J` and `ESC[3J` apart, the reader drained between them: the
/// screen still pushes nothing (the `2J` waited a moment for the `3J`).
#[test]
fn an_erase_written_apart_from_the_history_erase_pushes_nothing() {
    let mut chunks = Vec::new();
    for i in 0..500 {
        chunks.push(format!("line {i}\r\n").into_bytes());
    }
    for _ in 0..20 {
        chunks.push(b"\x1b[2J".to_vec());
        chunks.push(Vec::new());
        chunks.push(b"\x1b[3J\x1b[Hframe\r\nframe\r\n".to_vec());
    }
    let history = history_after(chunks, HistoryClear::KeepForRunningPrograms, AGENT);
    assert_eq!(history, BEFORE);
}

/// Nothing after it: the `2J` goes out as it is after a moment (the screen pushed, as any
/// `ESC[2J`); from the shell it isn't held at all.
#[test]
fn a_lone_erase_goes_out_after_a_moment() {
    let lone = || vec![b"one\r\ntwo\r\n".to_vec(), b"\x1b[2J".to_vec()];
    let started = std::time::Instant::now();
    let pushed = history_after(lone(), HistoryClear::KeepForRunningPrograms, AGENT);
    let waited = started.elapsed();
    assert!(pushed > 0, "the screen pushed, as a plain ESC[2J does");
    assert!(waited >= HOLD && waited < HOLD * 5, "{waited:?}");
    let started = std::time::Instant::now();
    assert!(history_after(lone(), HistoryClear::KeepForRunningPrograms, Foreground::Shell) > 0);
    assert!(started.elapsed() < HOLD, "the shell's: not held");
}

/// The erases of the history let through are counted once handed to the emulator (a small
/// buffer: the bytes before them handed first), never the ones dropped.
#[test]
fn the_erases_let_through_are_counted_as_handed_out() {
    for (policy, bytes, counted) in [
        (HistoryClear::Standard, &b"some text \x1b[2J\x1b[3J\x1b[H more"[..], 1),
        (HistoryClear::KeepAlways, b"some text \x1b[2J\x1b[3J\x1b[H more", 0),
        (HistoryClear::KeepAlways, b"a full reset \x1bc after", 1),
        (HistoryClear::Standard, b"plain text, colors \x1b[31mred\x1b[0m", 0),
    ] {
        let chunks = bytes.chunks(5).map(<[u8]>::to_vec).collect::<Vec<_>>();
        let pty = FakePty { reader: Chunks(chunks.into()), writer: Vec::new() };
        let erased = Arc::new(AtomicU64::new(0));
        let mut filtered = Filtered::new(
            pty,
            Arc::new(AtomicU8::new(policy.to_u8())),
            Arc::new(Mutex::new(Probe::new(|| Foreground::Shell))),
            Redrawing::default(),
            Arc::clone(&erased),
        );
        // Handed out a few bytes at a time: counted only once the erase itself is.
        let mut handed = Vec::new();
        let mut buf = [0u8; 3];
        loop {
            match filtered.reader().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    handed.extend_from_slice(&buf[..n]);
                    let whole = [&b"\x1b[3J"[..], b"\x1bc"]
                        .iter()
                        .any(|e| handed.windows(e.len()).any(|w| w == *e));
                    assert!(erased.load(Ordering::SeqCst) <= u64::from(whole), "{bytes:?}");
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(erased.load(Ordering::SeqCst), counted, "{bytes:?}");
    }
}
