use super::*;

/// No inlay hints shown.
fn no_hints() -> std::sync::Arc<[super::super::Inlay]> {
    std::sync::Arc::from(Vec::new())
}

fn doc(text: &str) -> Document {
    Document::new(text)
}

/// Lines longer than the columns take more rows; a row's line and its place in it, a
/// line's first row; a collapsed region's lines take none.
#[test]
fn long_lines_take_more_rows() {
    let d = doc("short\nthis line is long enough to wrap twice here\nend");
    let folds = Folds::default();
    let mut rows = WrapRows::default();
    rows.refresh(&d, &folds, 16, &no_hints());
    assert_eq!(rows.total(), 1 + 3 + 1);
    assert_eq!(rows.at(0), (0, 0));
    assert_eq!(rows.at(2), (1, 1));
    assert_eq!(rows.at(4), (2, 0));
    assert_eq!(rows.first_row(&folds, 2), 4);
    // The same again: nothing made.
    let before = rows.firsts.as_ptr();
    rows.refresh(&d, &folds, 16, &no_hints());
    assert_eq!(rows.firsts.as_ptr(), before);
}

/// Measured (not a check): the rows of a big file made again (each edit, a resize).
#[test]
#[ignore = "a measurement: cargo test -p ui-workbench --release measure_wrap_rows -- --ignored --nocapture"]
fn measure_wrap_rows() {
    let line =
        "    $result = $this->repository->findBy(['status' => $status], ['id' => 'DESC']);\n";
    let text = line.repeat(200_000);
    let d = doc(&text);
    let folds = Folds::default();
    let mut rows = WrapRows::default();
    let started = std::time::Instant::now();
    rows.refresh(&d, &folds, 60, &no_hints());
    println!(
        "{} lines ({} MB) → {} rows in {:?}",
        d.buffer().line_count(),
        text.len() / 1_000_000,
        rows.total(),
        started.elapsed()
    );
    let short = "    return $x;\n".repeat(200_000);
    let d = doc(&short);
    let started = std::time::Instant::now();
    rows.refresh(&d, &folds, 60, &no_hints());
    println!("short lines: {} rows in {:?}", rows.total(), started.elapsed());
    // A resize, then a keystroke, in the big file of long lines.
    let d = doc(&text);
    rows.refresh(&d, &folds, 60, &no_hints());
    let started = std::time::Instant::now();
    rows.refresh(&d, &folds, 61, &no_hints());
    println!("a resize: {:?}", started.elapsed());
    let mut d = d;
    let at = d.buffer().line_start(100_000);
    d.apply(document::Transaction::single(at..at, "x"), None, document::EditKind::Typing);
    let started = std::time::Instant::now();
    rows.refresh(&d, &folds, 61, &no_hints());
    println!("a keystroke: {:?}", started.elapsed());
    let short_doc = doc(&short);
    rows.refresh(&short_doc, &folds, 60, &no_hints());
    let started = std::time::Instant::now();
    rows.refresh(&short_doc, &folds, 61, &no_hints());
    println!("a resize of short lines: {:?}", started.elapsed());
}

/// An edit measures only the lines it touched; the rows come out as a whole measure's.
#[test]
fn an_edit_measures_only_its_lines() {
    let text = "short\n".repeat(1000) + "this line is long enough to wrap twice here\n";
    let mut d = doc(&text);
    let folds = Folds::default();
    let mut rows = WrapRows::default();
    rows.refresh(&d, &folds, 16, &no_hints());
    assert_eq!(rows.total(), 1000 + 3 + 1);
    rows.measured = 0;
    // A long line typed in the middle, a line joined at the top.
    let at = d.buffer().line_start(500);
    let edit = |d: &mut Document, range: Range<usize>, text: &str| {
        d.apply(document::Transaction::single(range, text), None, document::EditKind::Command);
    };
    edit(&mut d, at..at, "a long line typed in the middle of it all\n");
    edit(&mut d, 5..6, "");
    rows.refresh(&d, &folds, 16, &no_hints());
    assert!(rows.measured < 10, "{} lines measured", rows.measured);
    let mut whole = WrapRows::default();
    whole.refresh(&d, &folds, 16, &no_hints());
    assert_eq!(rows.firsts, whole.firsts);
    assert_eq!(rows.total(), whole.total());
}

/// A line's inlay hints count as room in it: a line that fits wraps once hints make it
/// wider; a new list of hints counts the rows again.
#[test]
fn hints_count_in_a_line_s_rows() {
    let d = doc("call(alpha, beta)\nend");
    let folds = Folds::default();
    let mut rows = WrapRows::default();
    rows.refresh(&d, &folds, 20, &no_hints());
    assert_eq!(rows.total(), 2, "it fits");
    let hint = |offset: usize, label: &str| super::super::Inlay { offset, label: label.into() };
    let hints: std::sync::Arc<[super::super::Inlay]> =
        std::sync::Arc::from(vec![hint(5, "first:"), hint(12, "second:")]);
    rows.refresh(&d, &folds, 20, &hints);
    assert!(rows.total() > 2, "wider with its hints: {}", rows.total());
}
