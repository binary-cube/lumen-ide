use super::*;

const WIDTHS: TopWidths =
    TopWidths { method: (104.0, 76.0), url: 80.0, send: (96.0, 32.0), gap: 8.0 };

fn ends(row: &TopRow) -> [(f32, f32); 3] {
    [row.method, row.url, row.send].map(|(x, w)| (x, x + w))
}

#[test]
fn wide_every_part_has_its_width_and_the_url_the_rest() {
    let row = top_row((0.0, 600.0), WIDTHS);
    assert!(row.send_labeled);
    assert_eq!(row.method, (0.0, 104.0));
    assert_eq!(row.send, (504.0, 96.0));
    assert_eq!(row.url, (112.0, 384.0));
}

/// At every width: the parts in order, none over another, all within the row.
#[test]
fn at_every_width_nothing_is_drawn_over_another() {
    for width in 0..=700 {
        let max = width as f32;
        let row = top_row((10.0, 10.0 + max), WIDTHS);
        let [method, url, send] = ends(&row);
        assert!(method.0 >= 10.0 && send.1 <= 10.0 + max + 0.01, "{width}: {row:?}");
        assert!(method.1 <= send.0 + 0.01, "{width}: method under Send {row:?}");
        if row.url.1 > 0.0 {
            assert!(method.1 <= url.0 && url.1 <= send.0 + 0.01, "{width}: {row:?}");
        }
    }
}

/// Narrower: Send shows its icon alone before the method narrows, the URL keeping its least.
#[test]
fn narrow_send_goes_to_its_icon_then_the_method_narrows() {
    let row = top_row((0.0, 280.0), WIDTHS);
    assert!(!row.send_labeled);
    assert_eq!(row.send.1, 32.0);
    assert_eq!(row.method.1, 104.0);
    let row = top_row((0.0, 220.0), WIDTHS);
    assert_eq!(row.method.1, 220.0 - 16.0 - 80.0 - 32.0);
    assert_eq!(row.url.1, 80.0);
    // Past the method's least, the URL gives what's left.
    let row = top_row((0.0, 180.0), WIDTHS);
    assert_eq!(row.method.1, 76.0);
    assert_eq!(row.url.1, 180.0 - 16.0 - 76.0 - 32.0);
}
