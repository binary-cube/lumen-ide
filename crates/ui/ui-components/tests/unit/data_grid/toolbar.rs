//! The toolbar's layout at many widths: nothing overlaps, what goes into the "…" menu goes
//! in its order, the range and the pages' arrows stay the longest.

use super::*;

/// A toolbar from 0 to `width` at scale 1, the texts' widths as a table's (`1–500`,
/// `of 12,345`, the `500 rows` pill, `Submit (3)`).
fn spec(width: f32) -> ToolbarSpec {
    ToolbarSpec {
        min_x: 0.0,
        max_x: width,
        metrics: Metrics::new(|v| v),
        paged: true,
        editable: true,
        pending: false,
        cancel: false,
        loading: false,
        range_w: 52.0,
        of_w: Some(64.0),
        size_w: 96.0,
        submit_w: 84.0,
        extractor_w: 70.0,
    }
}

/// Every state the toolbar can be in.
fn states() -> Vec<ToolbarSpec> {
    let mut out = Vec::new();
    for paged in [true, false] {
        for editable in [true, false] {
            for pending in [true, false] {
                for busy in [true, false] {
                    out.push(ToolbarSpec {
                        paged,
                        editable,
                        pending,
                        cancel: busy,
                        loading: busy,
                        ..spec(0.0)
                    });
                }
            }
        }
    }
    out
}

#[test]
fn everything_fits_when_wide() {
    let laid = layout(&spec(1200.0));
    assert!(laid.collapsed.is_empty());
    assert!(!laid.shows(Tool::More), "no menu when nothing is in it");
    for tool in [
        Tool::First,
        Tool::Back,
        Tool::Range,
        Tool::Of,
        Tool::Forward,
        Tool::Last,
        Tool::PageSize,
        Tool::Add,
        Tool::Delete,
        Tool::Refresh,
        Tool::Find,
        Tool::Transpose,
        Tool::Extractor,
    ] {
        assert!(laid.shows(tool), "{tool:?}");
    }
    assert_eq!(laid.range_room, 52.0);
}

#[test]
fn nothing_overlaps_at_any_width() {
    for state in states() {
        for width in (60..=1400).step_by(7) {
            let s = ToolbarSpec { max_x: width as f32, ..state };
            let laid = layout(&s);
            let rects: Vec<(Tool, f32, f32)> = laid
                .placed
                .iter()
                .map(|&(t, x, w)| (t, x, if t == Tool::Range { laid.range_room } else { w }))
                .collect();
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    let apart = a.1 + a.2 <= b.1 + 0.5 || b.1 + b.2 <= a.1 + 0.5;
                    assert!(apart, "{a:?} and {b:?} overlap at {width}: {state:?}");
                }
            }
            // The left side stays clear of the right one.
            let right = laid
                .placed
                .iter()
                .filter(|(t, ..)| {
                    !matches!(
                        t,
                        Tool::First
                            | Tool::Back
                            | Tool::Range
                            | Tool::Of
                            | Tool::Forward
                            | Tool::Last
                            | Tool::PageSize
                    )
                })
                .map(|&(_, x, _)| x)
                .fold(f32::MAX, f32::min);
            let left = rects
                .iter()
                .filter(|(t, ..)| {
                    matches!(
                        t,
                        Tool::First
                            | Tool::Back
                            | Tool::Range
                            | Tool::Of
                            | Tool::Forward
                            | Tool::Last
                            | Tool::PageSize
                    )
                })
                .map(|&(_, x, w)| x + w)
                .fold(0.0, f32::max);
            if laid.shows(Tool::Range) {
                assert!(left <= right + 0.5, "{left} past {right} at {width}");
            }
        }
    }
}

/// How much room a tool takes beside the others (its width and the gap after it).
fn room_of(spec: &ToolbarSpec, tool: Tool) -> f32 {
    let m = spec.metrics;
    match tool {
        Tool::Of => spec.of_w.unwrap_or(0.0) + m.after_of,
        Tool::PageSize => spec.size_w + m.before_size,
        Tool::Extractor => spec.extractor_w + m.gap + m.divider,
        _ => m.button + m.gap,
    }
}

#[test]
fn tools_collapse_in_order_and_come_back_where_they_fit() {
    for state in states() {
        for width in (60..=1400).rev().step_by(5) {
            let s = ToolbarSpec { max_x: width as f32, ..state };
            let laid = layout(&s);
            // The menu has them as the toolbar does, and only when there's one.
            let order: Vec<usize> = laid
                .collapsed
                .iter()
                .map(|t| MENU_ORDER.iter().position(|m| m == t).unwrap())
                .collect();
            assert!(order.windows(2).all(|w| w[0] < w[1]), "{:?}", laid.collapsed);
            assert_eq!(laid.shows(Tool::More), !laid.collapsed.is_empty());
            assert!(!laid.collapsed.contains(&Tool::Of), "`of N` is only hidden");
            // A step goes whole (‹‹ with ››, ‹ with ›).
            for step in COLLAPSE {
                let shown: Vec<bool> = step
                    .iter()
                    .filter(|t| !matches!(t, Tool::Of))
                    .filter(|t| {
                        state.paged
                            || !matches!(t, Tool::First | Tool::Last | Tool::Back | Tool::Forward)
                    })
                    .filter(|t| state.editable || !matches!(t, Tool::Add | Tool::Delete))
                    .map(|t| laid.shows(*t))
                    .collect();
                assert!(shown.windows(2).all(|w| w[0] == w[1]), "{step:?} split at {width}");
            }
            // Nothing put away would have fit in the room left (when the range is whole).
            if laid.range_room >= s.range_w {
                let left_end = laid
                    .placed
                    .iter()
                    .filter(|(t, ..)| {
                        matches!(
                            t,
                            Tool::First
                                | Tool::Back
                                | Tool::Range
                                | Tool::Of
                                | Tool::Forward
                                | Tool::Last
                                | Tool::PageSize
                        )
                    })
                    .map(|&(_, x, w)| x + w)
                    .fold(0.0, f32::max);
                let right = laid
                    .placed
                    .iter()
                    .filter(|(t, ..)| {
                        !matches!(
                            t,
                            Tool::First
                                | Tool::Back
                                | Tool::Range
                                | Tool::Of
                                | Tool::Forward
                                | Tool::Last
                                | Tool::PageSize
                        )
                    })
                    .map(|&(_, x, _)| x)
                    .fold(s.max_x, f32::min);
                let free = right - s.metrics.gap - s.metrics.pad - left_end;
                for tool in laid
                    .collapsed
                    .iter()
                    .filter(|t| !matches!(t, Tool::First | Tool::Last | Tool::Back | Tool::Forward))
                {
                    assert!(
                        room_of(&s, *tool) > free + 0.5,
                        "{tool:?} would fit in {free} at {width}: {laid:?}"
                    );
                }
            }
            // What's under way never goes.
            assert_eq!(laid.shows(Tool::Submit), state.pending);
            assert_eq!(laid.shows(Tool::Revert), state.pending);
            assert_eq!(laid.shows(Tool::Cancel), state.cancel);
            assert!(laid.shows(Tool::Range) || laid.range_room <= 0.0);
        }
    }
}

#[test]
fn the_page_size_goes_first_and_the_arrows_last() {
    let full = layout(&spec(1200.0));
    let (_, size) = full.get(Tool::PageSize).unwrap();
    // A little narrower than everything needs: only the page size is put away.
    let mut width = 1200.0;
    while layout(&spec(width)).collapsed.is_empty() {
        width -= 1.0;
    }
    let laid = layout(&spec(width));
    assert_eq!(laid.collapsed, vec![Tool::PageSize], "at {width} ({size})");
    // Narrow (the Database window open beside): the range and ‹ › stay, the rest is in "…".
    let laid = layout(&spec(250.0));
    for tool in [Tool::Back, Tool::Range, Tool::Forward, Tool::Delete, Tool::More] {
        assert!(laid.shows(tool), "{tool:?}: {laid:?}");
    }
    assert_eq!(
        laid.collapsed,
        vec![
            Tool::First,
            Tool::Last,
            Tool::PageSize,
            Tool::Add,
            Tool::Refresh,
            Tool::Find,
            Tool::Transpose,
            Tool::Extractor
        ]
    );
    // Narrower still: the arrows too, then the range is cut (never under the buttons).
    let laid = layout(&spec(120.0));
    assert!(!laid.shows(Tool::Back) && !laid.shows(Tool::Forward));
    assert!(laid.collapsed.contains(&Tool::Back));
    let laid = layout(&spec(70.0));
    assert!(laid.range_room < 52.0);
    let (x, _) = laid.get(Tool::Range).unwrap();
    let (more, _) = laid.get(Tool::More).unwrap();
    assert!(x + laid.range_room <= more);
}

#[test]
fn every_row_shows_when_unpaged() {
    let laid = layout(&ToolbarSpec { paged: false, ..spec(200.0) });
    for tool in [Tool::First, Tool::Back, Tool::Forward, Tool::Last] {
        assert!(!laid.shows(tool) && !laid.collapsed.contains(&tool), "{tool:?}");
    }
}

#[test]
fn the_extractor_s_pill_sits_left_of_the_view_s_buttons() {
    let laid = layout(&spec(1200.0));
    let (x, w) = laid.get(Tool::Extractor).unwrap();
    let (refresh, _) = laid.get(Tool::Refresh).unwrap();
    assert!(x + w < refresh, "{x} + {w} before {refresh}");
    assert_eq!(w, 70.0);
    // Without one (a source not copied from), nothing is there.
    let laid = layout(&ToolbarSpec { extractor_w: 0.0, ..spec(1200.0) });
    assert!(!laid.shows(Tool::Extractor));
    // It goes into "…" before Refresh (narrower buttons may come back beside it).
    let mut width = 1200.0;
    loop {
        let laid = layout(&spec(width));
        if laid.collapsed.contains(&Tool::Extractor) {
            assert!(laid.shows(Tool::Refresh), "at {width}");
            break;
        }
        width -= 1.0;
    }
}
