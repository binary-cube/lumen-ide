use super::*;
use crate::TermCell;

fn grid(rows: &[&str], offset: usize) -> GridSnapshot {
    let cols = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as u16;
    let mut cells = Vec::new();
    for row in rows {
        let mut chars: Vec<char> = row.chars().collect();
        chars.resize(cols as usize, ' ');
        cells.extend(chars.into_iter().map(|ch| TermCell { ch, ..TermCell::default() }));
    }
    GridSnapshot {
        cols,
        rows: rows.len() as u16,
        cells,
        display_offset: offset,
        ..Default::default()
    }
}

#[test]
fn selection_survives_scrolling_and_copies_visible_text() {
    let g = grid(&["hello world   ", "second line"], 0);
    let sel = TermSelection { anchor: g.point(0, 6), head: g.point(1, 5) };
    assert_eq!(sel.text(&g), "world\nsecond");
    // Backwards drag gives the same text.
    let back = TermSelection { anchor: sel.head, head: sel.anchor };
    assert_eq!(back.text(&g), "world\nsecond");
    // Scrolled up one line: the same lines now sit one row lower.
    let scrolled = grid(&["above", "hello world", "second line"], 1);
    assert_eq!(scrolled.visible_row(sel.anchor.line), Some(1));
    assert_eq!(sel.text(&scrolled), "world\nsecond");
}

#[test]
fn words_and_links() {
    let row = "  at src/Controller/Home.php:42 see https://example.com/a.php.";
    assert_eq!(word_at(row, 10), Some(5..31));
    assert_eq!(word_at(row, 0), None);
    let found = links(row);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(
        found[0].target,
        LinkTarget::File { path: "src/Controller/Home.php".into(), line: Some(42), column: None }
    );
    assert_eq!(found[0].cols, 5..31);
    assert_eq!(found[1].target, LinkTarget::Url("https://example.com/a.php".into()));
    let php = links("PHP Fatal error: oops in /var/www/a.php on line 7");
    assert_eq!(
        php[0].target,
        LinkTarget::File { path: "/var/www/a.php".into(), line: Some(7), column: None }
    );
    let win = links(r"C:\work\b.js:3:9 and c.php(12)");
    assert_eq!(
        win[0].target,
        LinkTarget::File { path: r"C:\work\b.js".into(), line: Some(3), column: Some(9) }
    );
    assert_eq!(
        win[1].target,
        LinkTarget::File { path: "c.php".into(), line: Some(12), column: None }
    );
    assert!(
        links("version 1.2 and v2.0")
            .iter()
            .all(|l| !matches!(&l.target, LinkTarget::File { path, .. } if path == "1.2"))
    );
}
