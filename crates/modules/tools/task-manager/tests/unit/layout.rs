//! The page's layout on every width 200–2400 (1 px apart) and height 200–1600 (20 px
//! apart), at scales 1, 1.5 and 2: nothing past the bounds, no two regions overlapping, the
//! class monotonic in the width (and steady with the hysteresis), a wider table never
//! showing fewer columns, the sorted column always shown and the name at least its least,
//! nothing lost (shown + gone + hidden = present), nothing in the toolbar's "…" that would
//! have fit, rows at least 24 logical pixels; the breakpoints exactly, the detail's place by
//! the height, the separator's ratio, the memo.

use super::*;

fn spec(width: f32, height: f32, scale: f32) -> LayoutSpec {
    LayoutSpec {
        bounds: Rect::new(30.0, 50.0, width * scale, height * scale),
        scale,
        tools: ToolWidths {
            scope: (196.0 * scale).round(),
            grouping: (188.0 * scale).round(),
            interval: (104.0 * scale).round(),
            sort: (96.0 * scale).round(),
        },
        text_widths: {
            let mut w = [0.0; COLUMNS.len()];
            w[ColumnId::Cpu.index()] = (40.0 * scale).round();
            w[ColumnId::Memory.index()] = (56.0 * scale).round();
            w
        },
        ..LayoutSpec::default()
    }
}

fn within(inner: Rect, outer: Rect) -> bool {
    inner.min_x() >= outer.min_x() - 0.01
        && inner.min_y() >= outer.min_y() - 0.01
        && inner.max_x() <= outer.max_x() + 0.01
        && inner.max_y() <= outer.max_y() + 0.01
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.intersect(&b).is_some_and(|r| r.size.width > 0.01 && r.size.height > 0.01)
}

/// The invariants of one layout (`why` says which).
fn check(spec: &LayoutSpec, page: &PageLayout, why: &str) {
    let b = spec.bounds;
    let px = |v: f32| (v * spec.scale).round();
    let mut regions = vec![("toolbar", page.toolbar)];
    regions.extend(page.status.map(|s| ("status", s)));
    if page.summary.area.size.height > 0.0 {
        regions.push(("summary", page.summary.area));
    }
    regions.extend(page.table.map(|t| ("table", t)));
    regions.extend(page.detail.map(|d| ("detail", d)));
    regions.extend(page.splitter.map(|(s, _)| ("splitter", s)));
    for (name, r) in &regions {
        assert!(within(*r, b), "{why}: {name} {r:?} out of {b:?}");
    }
    for (i, (a, ra)) in regions.iter().enumerate() {
        for (c, rc) in &regions[i + 1..] {
            assert!(!overlap(*ra, *rc), "{why}: {a} {ra:?} over {c} {rc:?}");
        }
    }
    for (i, card) in page.summary.cards.iter().enumerate() {
        assert!(within(*card, page.summary.area), "{why}: card {i}");
        for other in &page.summary.cards[i + 1..] {
            assert!(!overlap(*card, *other), "{why}: cards overlap");
        }
    }
    // The toolbar: inside, apart, nothing lost; what's in its "…" wouldn't fit (the
    // overflow's own guarantee, checked through it).
    let mut spans: Vec<(f32, f32)> =
        page.tools.placed.iter().map(|&(_, x, w)| (x, x + w)).collect();
    spans.extend(page.tools.more.map(|(x, w)| (x, x + w)));
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0 + 0.01), "{why}: tools overlap {spans:?}");
    if page.tools.fits {
        assert!(
            spans
                .iter()
                .all(|s| s.0 >= page.toolbar.min_x() - 0.01 && s.1 <= page.toolbar.max_x() + 0.01),
            "{why}: tools out"
        );
    }
    let tools = page.tools.placed.len() + page.tools.collapsed.len();
    let list = page.list_mode != ListMode::Table;
    assert_eq!(tools, if list { 6 } else { 5 }, "{why}: tools lost");
    assert!(page.row_height >= (MIN_ROW * spec.scale).floor(), "{why}: rows too low");
    if let Some(table) = page.table {
        assert!(within(page.rows, table), "{why}: rows");
        if let Some(header) = page.header {
            assert!(within(header, table) && !overlap(header, page.rows), "{why}: header");
            let chooser = page.chooser.expect("a header has its chooser");
            assert!(within(chooser, header), "{why}: chooser");
            let cols = &page.columns;
            let present: Vec<ColumnId> = present(spec, page.class).collect();
            assert_eq!(
                cols.shown.len() + cols.dropped.len() + cols.hidden.len(),
                present.len(),
                "{why}: columns lost"
            );
            let mut end = table.min_x();
            for &(id, x, w) in &cols.shown {
                assert!(x >= end - 0.01, "{why}: {id:?} overlaps");
                end = x + w;
            }
            assert!(end <= chooser.min_x() + 0.01, "{why}: columns under the chooser");
            if let Some(sorted) = spec.sorted_by {
                assert!(cols.shows(sorted), "{why}: sorted {sorted:?} hidden");
            }
            assert!(cols.get(ColumnId::Name).unwrap().1 >= px(150.0), "{why}: the name too narrow");
        }
    }
}

// Every width and height (200–1600 by 20) at every scale, in six tests run side by side.
#[test]
fn every_width_and_low_height_at_scale_1() {
    every_width_and_height(1.0, 200..=880);
}

#[test]
fn every_width_and_high_height_at_scale_1() {
    every_width_and_height(1.0, 900..=1600);
}

#[test]
fn every_width_and_low_height_at_scale_1_5() {
    every_width_and_height(1.5, 200..=880);
}

#[test]
fn every_width_and_high_height_at_scale_1_5() {
    every_width_and_height(1.5, 900..=1600);
}

#[test]
fn every_width_and_low_height_at_scale_2() {
    every_width_and_height(2.0, 200..=880);
}

#[test]
fn every_width_and_high_height_at_scale_2() {
    every_width_and_height(2.0, 900..=1600);
}

fn every_width_and_height(scale: f32, heights: std::ops::RangeInclusive<u32>) {
    for height in heights.step_by(20) {
        let mut before: Option<PageLayout> = None;
        for width in 200..=2400 {
            let mut s = spec(width as f32, height as f32, scale);
            let page = layout(&s, None);
            let why = format!("{width}×{height} @{scale}");
            check(&s, &page, &why);
            if let Some(before) = &before {
                assert!(page.class >= before.class, "{why}: the class went down");
                assert!(
                    page.columns.shown.len() >= before.columns.shown.len(),
                    "{why}: fewer columns"
                );
            }
            // With a part's detail open, and every project's view.
            if width % 7 == 0 {
                s.detail_open = true;
                check(&s, &layout(&s, None), &format!("{why} detail"));
                s.scope_all = true;
                s.grouping = Grouping::Kind;
                s.sorted_by = Some(ColumnId::Memory);
                check(&s, &layout(&s, None), &format!("{why} all"));
            }
            before = Some(page);
        }
    }
}

#[test]
fn the_breakpoints_exactly_with_and_without_hysteresis() {
    use WidthClass::*;
    for (below, at, low, high) in [
        (319.0, 320.0, Minimal, Compact),
        (479.0, 480.0, Compact, Narrow),
        (759.0, 760.0, Narrow, Medium),
        (1079.0, 1080.0, Medium, Wide),
    ] {
        assert_eq!(class_of(below), low);
        assert_eq!(class_of(at), high);
        // From below, the next class needs 8 px past it; from above, 8 px under.
        assert_eq!(width_class(at, Some(low)), low);
        assert_eq!(width_class(at + 7.0, Some(low)), low);
        assert_eq!(width_class(at + 8.0, Some(low)), high);
        assert_eq!(width_class(below, Some(high)), high);
        assert_eq!(width_class(at - 8.0, Some(high)), high);
        assert_eq!(width_class(at - 9.0, Some(high)), low);
        // Dragged back and forth by 7 px around the breakpoint: no change either way.
        for start in [low, high] {
            let mut class = width_class(if start == low { below } else { at }, Some(start));
            for dx in [-7.0, 7.0, -7.0, 0.0, 7.0, -3.0, 5.0] {
                let next = width_class(at + dx, Some(class));
                assert_eq!(next, class, "{start:?} at {}", at + dx);
                class = next;
            }
        }
    }
    // A big jump crosses every class.
    assert_eq!(width_class(250.0, Some(Wide)), Minimal);
    assert_eq!(width_class(2000.0, Some(Minimal)), Wide);
}

#[test]
fn each_class_has_its_summary_table_and_detail() {
    let at = |w: f32, h: f32| layout(&spec(w, h, 1.0), None);
    let wide = at(1200.0, 900.0);
    assert_eq!(
        (wide.class, wide.summary.form, wide.summary.cards.len()),
        (WidthClass::Wide, SummaryForm::Cards, 4)
    );
    assert!(
        wide.summary.cards.iter().all(|c| c.min_y() == wide.summary.cards[0].min_y()),
        "one row"
    );
    assert_eq!(wide.summary.cards[0].size.height, 96.0);
    assert_eq!(wide.row_height, 32.0);
    assert!(wide.columns.shows(ColumnId::Trend));
    assert_eq!(wide.place, DetailPlace::Right);
    let medium = at(900.0, 900.0);
    assert_eq!((medium.summary.cards[0].size.height, medium.place), (80.0, DetailPlace::Below));
    assert!(!medium.columns.shows(ColumnId::Trend), "no trend under wide");
    let narrow = at(600.0, 900.0);
    assert_eq!(narrow.summary.cards[0].size.height, 68.0);
    assert_eq!(
        narrow.summary.cards[2].min_y(),
        narrow.summary.cards[0].max_y() + 10.0,
        "two by two"
    );
    assert_eq!((narrow.row_height, narrow.place), (26.0, DetailPlace::Below));
    let mut three = spec(600.0, 900.0, 1.0);
    three.has_gpu = false;
    let three = layout(&three, None);
    assert_eq!(three.summary.cards.len(), 3);
    assert!(
        three.summary.cards.iter().all(|c| c.min_y() == three.summary.cards[0].min_y()),
        "three in a row"
    );
    let compact = at(420.0, 900.0);
    assert_eq!(
        (compact.summary.form, compact.list_mode, compact.place),
        (SummaryForm::Pills, ListMode::TwoLines, DetailPlace::Page)
    );
    assert!(compact.header.is_none(), "a list has no headers");
    assert!(compact.tools.shows(Tool::Sort), "\"Sort by…\" in its bar instead");
    assert_eq!(
        at(390.0, 900.0).summary.cards[2].min_y(),
        at(390.0, 900.0).summary.cards[1].max_y() + 10.0,
        "pills two by two under 400"
    );
    let minimal = at(280.0, 900.0);
    assert_eq!((minimal.summary.form, minimal.list_mode), (SummaryForm::Line, ListMode::OneLine));
    assert!(minimal.filter_icon && minimal.status.is_none());
}

#[test]
fn the_height_decides_the_summary_and_where_the_detail_goes() {
    let at = |w: f32, h: f32| layout(&spec(w, h, 1.0), None);
    assert_eq!(at(1200.0, 419.0).summary.form, SummaryForm::Pills, "short: the band");
    assert_eq!(at(1200.0, 259.0).summary.form, SummaryForm::None, "very short: none");
    assert!(at(1200.0, 259.0).status.is_some(), "the status line stays");
    assert_eq!(at(900.0, 560.0).place, DetailPlace::Below);
    assert_eq!(at(900.0, 559.0).place, DetailPlace::Page);
    assert_eq!(at(600.0, 640.0).place, DetailPlace::Below);
    assert_eq!(at(600.0, 639.0).place, DetailPlace::Page);
    // Open on a page: the table and the summary give it their room.
    let mut s = spec(600.0, 500.0, 1.0);
    s.detail_open = true;
    let page = layout(&s, None);
    assert!(page.table.is_none() && page.summary.form == SummaryForm::None);
    let detail = page.detail.unwrap();
    assert!(detail.min_y() < at(600.0, 500.0).summary.area.max_y());
    // Below: at least 220 px, the table above it.
    s.bounds = Rect::new(0.0, 0.0, 900.0, 900.0);
    let below = layout(&s, None);
    let (d, t) = (below.detail.unwrap(), below.table.unwrap());
    assert!(d.size.height >= 220.0 && t.max_y() <= d.min_y());
    assert_eq!(below.splitter.unwrap().1, Orientation::Horizontal);
}

#[test]
fn the_detail_on_the_right_keeps_its_ratio_and_its_limits() {
    let mut s = spec(1600.0, 900.0, 1.0);
    s.detail_open = true;
    let width = |s: &LayoutSpec| layout(s, None).detail.unwrap().size.width;
    // 30% of the page's content, within 300–520.
    let content = 1600.0 - 24.0;
    assert_eq!(width(&s), (content * 0.30_f32).round().clamp(300.0, 520.0));
    s.ratio_right = 0.6;
    assert_eq!(width(&s), 520.0);
    s.ratio_right = 0.15;
    assert_eq!(width(&s), 300.0);
    // The ratio, not the pixels, as the page grows.
    s.ratio_right = 0.25;
    let narrow = width(&s);
    s.bounds.size.width = 2000.0;
    assert!(width(&s) > narrow);
    let page = layout(&s, None);
    let (strip, orientation) = page.splitter.unwrap();
    assert_eq!(orientation, Orientation::Vertical);
    assert_eq!(strip.min_x(), page.table.unwrap().max_x());
    assert_eq!(strip.max_x(), page.detail.unwrap().min_x());
}

#[test]
fn the_toolbar_puts_away_the_interval_first_then_the_grouping() {
    let at = |w: f32| layout(&spec(w, 900.0, 1.0), None);
    assert!(at(1400.0).tools.collapsed.is_empty());
    let mut seen: Vec<Tool> = Vec::new();
    for w in (480..1400).rev() {
        for t in &at(w as f32).tools.collapsed {
            if !seen.contains(t) {
                seen.push(*t);
            }
        }
    }
    assert_eq!(seen.first(), Some(&Tool::Interval));
    assert_eq!(seen.get(1), Some(&Tool::Grouping), "{seen:?}");
    assert!(!seen.contains(&Tool::Filter), "the filter shrinks, it never goes");
    assert!(at(359.0).filter_icon && !at(360.0).filter_icon);
}

#[test]
fn the_memo_works_one_out_only_for_a_new_spec() {
    let mut cache = LayoutCache::default();
    let s = spec(1200.0, 800.0, 2.0);
    let first = (*cache.get(&s)).clone();
    cache.get(&s);
    cache.get(&s);
    assert_eq!(cache.computed, 1);
    assert_eq!(*cache.get(&s), first);
    let mut wider = s.clone();
    wider.bounds.size.width += 2.0;
    cache.get(&wider);
    assert_eq!(cache.computed, 2);
    // The hysteresis through the cache: back and forth by 7 px over a breakpoint.
    let mut cache = LayoutCache::default();
    let at = |w: f32| spec(w, 800.0, 1.0);
    assert_eq!(cache.get(&at(1075.0)).class, WidthClass::Medium);
    assert_eq!(cache.get(&at(1082.0)).class, WidthClass::Medium);
    assert_eq!(cache.get(&at(1088.0)).class, WidthClass::Wide);
    assert_eq!(cache.get(&at(1075.0)).class, WidthClass::Wide);
    assert_eq!(cache.get(&at(1071.0)).class, WidthClass::Medium);
    kernel::Purge::purge(&mut cache);
    assert!(cache.last().is_none());
}

#[test]
fn the_detail_lays_out_its_charts_by_its_width() {
    assert_eq!(detail_layout(500.0, 1.0).chart_columns, 2);
    assert_eq!(detail_layout(420.0, 1.0).chart_columns, 1);
    assert_eq!(detail_layout(800.0, 2.0).chart_columns, 1, "logical pixels");
    assert_eq!(detail_layout(361.0, 1.0).stat_columns, 2);
    assert_eq!(detail_layout(360.0, 1.0).stat_columns, 1);
}

#[test]
fn a_column_hidden_or_pinned_by_the_user() {
    let mut s = spec(1300.0, 900.0, 1.0);
    s.choices[ColumnId::Memory.index()] = Choice::Hidden;
    s.choices[ColumnId::Detail.index()] = Choice::Pinned;
    let page = layout(&s, None);
    assert!(!page.columns.shows(ColumnId::Memory) && page.columns.hidden == [ColumnId::Memory]);
    assert!(page.columns.shows(ColumnId::Detail));
    // Narrow: the pinned detail pushes others out.
    let mut narrow = s.clone();
    narrow.bounds.size.width = 640.0;
    let page = layout(&narrow, None);
    assert!(page.columns.shows(ColumnId::Detail));
    assert!(!page.columns.dropped.is_empty());
    // Every project's view, not by project: its project's column.
    s.scope_all = true;
    assert!(present(&s, WidthClass::Wide).any(|c| c == ColumnId::Project));
    s.grouping = Grouping::Project;
    assert!(!present(&s, WidthClass::Wide).any(|c| c == ColumnId::Project));
}

/// The layout's budget (plan 40: at most 50 µs a page, worked out again while a separator
/// is dragged), at every class (a release build).
#[test]
#[cfg_attr(debug_assertions, ignore = "a budget: cargo test -p task-manager --release budget")]
fn budget_a_page_is_laid_out_in_time() {
    use std::time::{Duration, Instant};
    for width in [280.0, 400.0, 600.0, 900.0, 1300.0, 2400.0] {
        let mut s = spec(width, 900.0, 2.0);
        s.detail_open = true;
        let mut times: Vec<Duration> = (0..101)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(layout(&s, None));
                t.elapsed()
            })
            .collect();
        times.sort();
        let took = times[50];
        eprintln!("budget: a {width} px page laid out in {took:?}");
        assert!(took <= Duration::from_micros(50), "{width}: {took:?}");
    }
}

/// A column the user sized keeps its width (logical, at every scale); once the name is
/// sized, the detail (the last column) takes what's left up to the chooser; on a page too
/// narrow for it, the others leave, then it gives way.
#[test]
fn columns_sized_by_the_user() {
    for scale in [1.0, 2.0] {
        let mut s = spec(1400.0, 900.0, scale);
        s.widths[ColumnId::Name.index()] = 220.0;
        s.widths[ColumnId::Memory.index()] = 110.0;
        let page = layout(&s, None);
        let name = page.columns.get(ColumnId::Name).unwrap();
        assert_eq!(name.1, 220.0 * scale);
        assert_eq!(page.columns.get(ColumnId::Memory).unwrap().1, 110.0 * scale);
        let (last, x, w) = *page.columns.shown.last().unwrap();
        assert_eq!(last, ColumnId::Detail);
        assert!(w > 360.0 * scale, "the detail past its most");
        let chooser = page.chooser.unwrap();
        assert!((chooser.min_x() - (x + w) - 12.0 * scale).abs() <= 1.0, "up to the chooser");
        // A name wider than the page: the others leave by their priority (the sorted one
        // stays), then it gives way rather than be cut.
        let mut wide = spec(900.0, 900.0, scale);
        wide.widths[ColumnId::Name.index()] = 2000.0;
        let page = layout(&wide, None);
        assert!(page.columns.shows(ColumnId::Cpu) && !page.columns.cramped);
        assert!(page.columns.end() <= page.chooser.unwrap().min_x());
    }
}
