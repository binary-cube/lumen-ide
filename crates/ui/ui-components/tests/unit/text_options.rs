use super::*;

fn rows(line: &str, columns: usize) -> Vec<&str> {
    let mut out = Vec::new();
    wrap_rows(line, columns, 4, &mut out);
    out.into_iter().map(|r| &line[r]).collect()
}

/// A line that fits is one row; a longer one breaks after a space (or a mark) in its row's
/// second half, else where the row is full; every byte is in exactly one row.
#[test]
fn a_line_wraps_at_words_else_where_it_is_full() {
    assert_eq!(rows("short", 20), ["short"]);
    assert_eq!(rows("", 20), [""]);
    assert_eq!(rows("$total = $price * $count;", 12), ["$total = ", "$price * ", "$count;"]);
    assert_eq!(rows("abcdefghijklmnopqrst", 8), ["abcdefgh", "ijklmnop", "qrst"]);
    let line = "foo(bar, baz) + qux(quux, corge)";
    assert_eq!(rows(line, 12).concat(), line, "nothing lost or doubled");
    // A wide character counts two and is never split; a tab goes to its stop.
    assert_eq!(rows("日本語日本語", 8), ["日本語日", "本語"]);
    assert_eq!(rows("\tabcdefgh", 8), ["\tabcd", "efgh"]);
    // Too narrow: rows of the least columns.
    assert_eq!(rows("abcdefghij", 2), ["abcdefgh", "ij"]);
}

/// The rows after the first hanging in hold fewer columns (never fewer than the least).
#[test]
fn hanging_rows_hold_fewer_columns() {
    let mut out = Vec::new();
    let line = "abcdefghijklmnopqrstuvwxyz";
    wrap_rows_hanging(line, 10, 4, 4, &mut out);
    let rows: Vec<&str> = out.iter().map(|r| &line[r.clone()]).collect();
    assert_eq!(rows[0].len(), 10);
    assert!(rows[1..].iter().all(|r| r.len() <= 8), "{rows:?}");
    assert_eq!(rows.concat(), line);
    wrap_rows_hanging(line, 10, 4, 100, &mut out);
    assert!(out.iter().skip(1).all(|r| r.len() == LEAST_WRAP_COLUMNS || r.end == line.len()));
}

/// Room shown in the line (a hint) counts as the character after it, so a row with hints
/// breaks sooner; nothing of the text lost.
#[test]
fn room_shown_in_the_line_counts() {
    let mut plain = Vec::new();
    let mut hinted = Vec::new();
    let line = "call(alpha, beta, gamma, delta, epsilon)";
    wrap_rows_with(line, (20, 4, 0), &[], &mut plain);
    wrap_rows_with(line, (20, 4, 0), &[(5, 8), (12, 8)], &mut hinted);
    assert!(hinted[0].end < plain[0].end, "breaks sooner: {plain:?} {hinted:?}");
    let rows: Vec<&str> = hinted.iter().map(|r| &line[r.clone()]).collect();
    assert_eq!(rows.concat(), line);
}
