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
