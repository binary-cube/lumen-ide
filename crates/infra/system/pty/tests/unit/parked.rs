use super::*;

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::Config;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::Processor;

/// A terminal `cols` wide after `bytes`.
pub(crate) fn term_after(cols: usize, bytes: &[u8]) -> Term<VoidListener> {
    let config = Config { scrolling_history: 10_000, ..Config::default() };
    let mut term = Term::new(config, &TermSize::new(cols, 10), VoidListener);
    let mut parser: Processor = Processor::new();
    parser.advance(&mut term, bytes);
    term
}

/// Output with every kind of cell: colors named, indexed and RGB, every attribute, wide
/// characters (one cut by the edge), combining marks, an underline's color, a link, long
/// lines wrapped, blank lines, a colored background to the edge.
pub(crate) fn styled_output(lines: usize) -> Vec<u8> {
    // A wide character cut by the edge (a leading spacer, then on the next row).
    let mut out = format!("{}中x\r\n", "a".repeat(39));
    for i in 0..lines {
        match i % 8 {
            0 => out.push_str(&format!("\x1b[31mred {i}\x1b[0m \x1b[1;4mbold under\x1b[0m\r\n")),
            1 => out.push_str(&format!(
                "\x1b[38;5;208mindexed\x1b[48;2;10;20;30m rgb bg {i}\x1b[0m\x1b[3;7;2;9mmixed\x1b[0m\r\n"
            )),
            2 => out.push_str(&format!("wide 中文字 🎉 {i}\x1b[8mhidden\x1b[0m\r\n")),
            3 => out.push_str(&format!("e\u{301}combining a\u{308} {i}\x1b[4:3m\x1b[58;5;196mcurl\x1b[0m\r\n")),
            4 => out.push_str(&format!(
                "\x1b]8;id=x{i};https://example.com/{i}\x1b\\link {i}\x1b]8;;\x1b\\ after\r\n"
            )),
            5 => out.push_str(&format!("{}{i}\r\n", "long line that wraps ".repeat(6))),
            6 => out.push_str("\r\n"),
            _ => out.push_str(&format!("\x1b[44m\x1b[Kblue to the edge {i}\x1b[0m\r\n")),
        }
    }
    out.into_bytes()
}

/// The history's rows, oldest first (copies).
pub(crate) fn history_rows(term: &Term<VoidListener>) -> Vec<Row<Cell>> {
    let history = term.grid().history_size();
    (1..=history).rev().map(|i| term.grid()[Line(-(i as i32))].clone()).collect()
}

#[test]
fn rows_come_back_as_they_went() {
    let term = term_after(40, &styled_output(400));
    let rows = history_rows(&term);
    assert!(rows.len() > 400, "{}", rows.len());
    // Every kind of cell is there.
    let cells = || rows.iter().flat_map(|r| r[..].iter());
    assert!(cells().any(|c| c.flags.contains(Flags::WIDE_CHAR)));
    assert!(cells().any(|c| c.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)));
    assert!(cells().any(|c| c.flags.contains(Flags::WRAPLINE)));
    assert!(cells().any(|c| c.hyperlink().is_some()));
    assert!(cells().any(|c| c.zerowidth().is_some_and(|z| !z.is_empty())));
    assert!(cells().any(|c| c.underline_color().is_some()));
    let mut parked = Parked::default();
    parked.push(&rows, 40);
    assert_eq!(parked.len(), rows.len());
    let cells_bytes = rows.len() * 40 * std::mem::size_of::<Cell>();
    assert!(parked.bytes() * 5 < cells_bytes, "{} of {cells_bytes}", parked.bytes());
    let back = parked.take();
    assert!(parked.is_empty());
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].0, 40);
    assert_eq!(back[0].1.len(), rows.len());
    for (i, (a, b)) in rows.iter().zip(&back[0].1).enumerate() {
        assert_eq!(a, b, "row {i}");
        assert_eq!(a.len(), b.len());
    }
}

#[test]
fn the_oldest_go_past_the_limit_and_widths_stay_apart() {
    let narrow = history_rows(&term_after(40, &styled_output(300)));
    let wide = history_rows(&term_after(60, &styled_output(300)));
    let mut parked = Parked::default();
    parked.push(&narrow, 40);
    parked.push(&wide[..100], 60);
    parked.push(&wide[100..], 60);
    assert_eq!(parked.len(), narrow.len() + wide.len());
    // Into the first park, then all of it.
    parked.keep(wide.len() + 10);
    assert_eq!(parked.len(), wide.len() + 10);
    let back = parked.take();
    assert_eq!(back.len(), 2, "one run per width");
    assert_eq!(back[0].1, narrow[narrow.len() - 10..].to_vec());
    assert_eq!(back[1].1, wide);
    let mut parked = Parked::default();
    parked.push(&narrow, 40);
    parked.keep(0);
    assert!(parked.is_empty());
}

/// Encoded (what goes to disk) and decoded with what stayed in memory: every kind of cell
/// comes back the same; anything changed or cut short is refused, never misread.
#[test]
fn encoded_rows_come_back_the_same() {
    let term = term_after(40, &styled_output(300));
    let rows = history_rows(&term);
    let mut parked = Parked::default();
    parked.push(&rows[..100], 40);
    parked.push(&rows[100..], 40);
    parked.keep(rows.len() - 7);
    let len = parked.len();
    let (bytes, kept) = parked.encode();
    assert!(parked.is_empty(), "emptied");
    assert!(bytes.windows(4).any(|w| w == b"wide"), "plain bytes (the store seals them)");
    let mut back = Parked::decode(&bytes, kept).unwrap();
    assert_eq!(back.len(), len);
    let back: Vec<Row<Cell>> = back.take().into_iter().flat_map(|(_, r)| r).collect();
    assert!(back == rows[7..], "the same rows, the 7 let go still gone");
    // Changed or short: refused.
    let mut parked = Parked::default();
    parked.push(&rows, 40);
    let (bytes, kept) = parked.encode();
    assert!(Parked::decode(&bytes[..bytes.len() - 1], Vec::new()).is_none());
    let mut wrong = bytes;
    wrong[0] = b'X';
    assert!(Parked::decode(&wrong, kept).is_none());
}
