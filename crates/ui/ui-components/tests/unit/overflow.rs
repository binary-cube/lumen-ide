//! The bar with "…" on every width: nothing overlaps or leaves the bar, nothing is lost
//! (placed + in the menu = all), nothing is in the menu that would have fit, a wider bar
//! never trades an item for a less wanted one; the menu's order; a flexible item shrinking first.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum T {
    Scope,
    Grouping,
    Filter,
    Pause,
    Interval,
    Sort,
}

fn items() -> Vec<OverflowItem<T>> {
    vec![
        OverflowItem::new(T::Scope, 190.0, Side::Left, Some(3), 3),
        OverflowItem::new(T::Grouping, 150.0, Side::Left, Some(1), 2),
        OverflowItem::new(T::Filter, 260.0, Side::Left, None, 9).flexible(120.0),
        OverflowItem::new(T::Sort, 40.0, Side::Right, None, 8),
        OverflowItem::new(T::Pause, 32.0, Side::Right, Some(2), 1),
        OverflowItem::new(T::Interval, 96.0, Side::Right, Some(0), 0),
    ]
}

fn spec(items: &[OverflowItem<T>], width: f32) -> OverflowSpec<'_, T> {
    OverflowSpec { min_x: 0.0, max_x: width, pad: 8.0, gap: 6.0, more_width: 32.0, items }
}

fn spans(layout: &OverflowLayout<T>) -> Vec<(f32, f32)> {
    let mut spans: Vec<(f32, f32)> = layout.placed.iter().map(|&(_, x, w)| (x, x + w)).collect();
    spans.extend(layout.more.map(|(x, w)| (x, x + w)));
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    spans
}

#[test]
fn every_width_fits_without_overlap_or_loss() {
    let items = items();
    let mut before: Option<OverflowLayout<T>> = None;
    for width in 380..=1400 {
        let width = width as f32;
        let layout = fit(&spec(&items, width));
        assert!(layout.fits, "{width}");
        let spans = spans(&layout);
        assert!(spans.first().unwrap().0 >= 0.0 && spans.last().unwrap().1 <= width, "{width}");
        assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "overlap at {width}: {spans:?}");
        // Nothing lost.
        assert_eq!(layout.placed.len() + layout.collapsed.len(), items.len(), "{width}");
        assert_eq!(layout.more.is_some(), !layout.collapsed.is_empty());
        // Nothing hidden for nothing: any one of them back wouldn't fit.
        for id in &layout.collapsed {
            let i = items.iter().position(|it| it.id == *id).unwrap();
            let hidden: Vec<usize> = layout
                .collapsed
                .iter()
                .filter(|c| *c != id)
                .map(|c| items.iter().position(|it| it.id == *c).unwrap())
                .collect();
            assert!(needed(&spec(&items, width), &hidden) > width, "{id:?} would fit at {width}");
            assert!(items[i].collapse.is_some());
        }
        // Wider never trades an item for a less wanted one: one shown before is shown
        // still, or one more wanted that wasn't took its room.
        if let Some(before) = &before {
            let rank = |id: &T| items.iter().find(|i| i.id == *id).unwrap().collapse;
            for (id, ..) in &before.placed {
                let kept = layout.shows(*id);
                let better = layout
                    .placed
                    .iter()
                    .any(|(other, ..)| !before.shows(*other) && rank(other) > rank(id));
                assert!(kept || better, "{id:?} went at {width} for nothing more wanted");
            }
        }
        before = Some(layout);
    }
}

#[test]
fn they_go_in_their_order_and_the_menu_has_its_own() {
    let items = items();
    let all = fit(&spec(&items, 2000.0));
    assert!(all.collapsed.is_empty() && all.more.is_none());
    // The filter at its widest when there's room.
    assert_eq!(all.get(T::Filter).unwrap().1, 260.0);
    // Narrower: the filter shrinks before anything goes.
    let full: f32 = 16.0 + 190.0 + 150.0 + 260.0 + 40.0 + 32.0 + 96.0 + 5.0 * 6.0;
    let shrunk = fit(&spec(&items, full - 100.0));
    assert!(shrunk.collapsed.is_empty());
    assert_eq!(shrunk.get(T::Filter).unwrap().1, 160.0);
    // The interval first, then the grouping.
    let less = fit(&spec(&items, full - 160.0));
    assert_eq!(less.collapsed, [T::Interval]);
    let tight = fit(&spec(&items, 420.0));
    assert!(tight.shows(T::Filter) && tight.shows(T::Sort), "the ones that never go stay");
    let mut menu = tight.collapsed.clone();
    menu.sort_by_key(|t| items.iter().find(|i| i.id == *t).unwrap().menu);
    assert_eq!(tight.collapsed, menu, "the menu's own order");
    // Right items end at the bar's end, the "…" after them.
    let (more, more_w) = tight.more.unwrap();
    assert_eq!(more + more_w, 420.0 - 8.0);
    assert!(tight.get(T::Sort).unwrap().0 < more);
}

#[test]
fn a_narrow_item_comes_back_where_a_wide_one_did_not_fit() {
    let items = vec![
        OverflowItem::new(T::Interval, 200.0, Side::Right, Some(1), 0),
        OverflowItem::new(T::Pause, 30.0, Side::Right, Some(0), 1),
        OverflowItem::new(T::Filter, 100.0, Side::Left, None, 2),
    ];
    // The pause goes first, but that isn't enough: the interval goes too; then the pause
    // fits again.
    let width = 16.0 + 100.0 + 30.0 + 32.0 + 12.0 + 10.0;
    let layout = fit(&spec(&items, width));
    assert_eq!(layout.collapsed, [T::Interval]);
    assert!(layout.shows(T::Pause));
}

#[test]
fn too_narrow_even_for_what_stays_says_so() {
    let items = items();
    let layout = fit(&spec(&items, 100.0));
    assert!(!layout.fits);
    assert_eq!(layout.placed.len() + layout.collapsed.len(), items.len());
}
