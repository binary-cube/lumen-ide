//! The columns on every width: none overlapping or past the room, nothing lost (shown +
//! dropped + hidden = all), a wider table never showing fewer, the sorted one always shown,
//! the first at least its least; the order they leave in, pinned and hidden ones, the
//! numbers' reserved width, the flexible ones growing.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum C {
    Name,
    Cpu,
    Memory,
    Network,
    Disk,
    Gpu,
    Threads,
    Detail,
}

fn columns() -> Vec<ColumnSpec<C>> {
    vec![
        ColumnSpec::new(C::Name, 0, 150.0).flexible(f32::INFINITY),
        ColumnSpec::new(C::Cpu, 1, 64.0),
        ColumnSpec::new(C::Memory, 2, 76.0),
        ColumnSpec::new(C::Network, 3, 80.0),
        ColumnSpec::new(C::Disk, 4, 80.0),
        ColumnSpec::new(C::Gpu, 5, 72.0),
        ColumnSpec::new(C::Threads, 7, 48.0),
        ColumnSpec::new(C::Detail, 8, 160.0).flexible(360.0),
    ]
}

fn check(columns: &[ColumnSpec<C>], width: f32, sorted: Option<C>, before: &mut usize) {
    let fit = fit(columns, 0.0, width, 8.0, sorted);
    assert!(!fit.cramped, "{width}");
    let spans: Vec<(f32, f32)> = fit.shown.iter().map(|&(_, x, w)| (x, x + w)).collect();
    assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "overlap at {width}");
    assert!(spans.last().unwrap().1 <= width + 0.01, "past the room at {width}");
    assert_eq!(fit.shown.len() + fit.dropped.len() + fit.hidden.len(), columns.len());
    if let Some(sorted) = sorted {
        assert!(fit.shows(sorted), "{sorted:?} sorted, gone at {width}");
    }
    assert!(fit.get(C::Name).unwrap().1 >= 150.0, "the name keeps its least at {width}");
    // None left for nothing: each one gone wouldn't fit beside the others.
    let used: f32 = fit
        .shown
        .iter()
        .map(|&(id, ..)| columns.iter().find(|c| c.id == id).unwrap().least() + 8.0)
        .sum();
    for id in &fit.dropped {
        let least = columns.iter().find(|c| c.id == *id).unwrap().least();
        assert!(used + least - 8.0 + 8.0 > width + 0.01, "{id:?} would fit at {width}");
    }
    assert!(fit.shown.len() >= *before, "fewer at {width}");
    *before = fit.shown.len();
}

#[test]
fn every_width_fits_by_priority() {
    let columns = columns();
    for sorted in [None, Some(C::Cpu), Some(C::Threads), Some(C::Detail), Some(C::Name)] {
        let least = 150.0
            + sorted
                .filter(|s| *s != C::Name)
                .map_or(0.0, |s| columns.iter().find(|c| c.id == s).unwrap().least() + 8.0);
        let mut before = 0;
        for width in (least as usize)..=1600 {
            check(&columns, width as f32, sorted, &mut before);
        }
    }
}

#[test]
fn the_highest_priority_leaves_first_and_the_sorted_one_stays() {
    let columns = columns();
    let all: f32 = columns.iter().map(ColumnSpec::least).sum::<f32>() + 7.0 * 8.0;
    let everything = fit(&columns, 0.0, all, 8.0, None);
    assert!(everything.dropped.is_empty());
    let less = fit(&columns, 0.0, all - 1.0, 8.0, None);
    assert_eq!(less.dropped, [C::Detail]);
    let fewer = fit(&columns, 0.0, all - 300.0, 8.0, None);
    assert_eq!(fewer.dropped, [C::Detail, C::Threads, C::Gpu]);
    // Sorted by threads: it stays, the next ones go in its place.
    let sorted = fit(&columns, 0.0, all - 300.0, 8.0, Some(C::Threads));
    assert!(sorted.shows(C::Threads));
    assert_eq!(sorted.dropped, [C::Detail, C::Gpu, C::Disk]);
}

#[test]
fn pinned_and_hidden_columns() {
    let mut columns = columns();
    columns[5] = columns[5].chosen(Choice::Pinned); // GPU
    columns[2] = columns[2].chosen(Choice::Hidden); // memory
    let wide = fit(&columns, 0.0, 2000.0, 8.0, None);
    assert!(!wide.shows(C::Memory) && wide.hidden == [C::Memory]);
    let narrow = fit(&columns, 0.0, 320.0, 8.0, None);
    assert!(narrow.shows(C::Gpu), "pinned: others leave for it");
    assert!(!narrow.shows(C::Network));
    // Hidden but sorted by it: shown anyway.
    assert!(fit(&columns, 0.0, 2000.0, 8.0, Some(C::Memory)).shows(C::Memory));
    // Only once nothing else can leave does a pinned one go.
    let tiny = fit(&columns, 0.0, 230.0, 8.0, Some(C::Cpu));
    assert!(!tiny.shows(C::Gpu) && tiny.shows(C::Cpu));
}

#[test]
fn numbers_keep_their_widest_text_and_flexible_ones_grow() {
    let mut columns = columns();
    columns[1] = columns[1].reserving(90.0);
    let fit_wide = fit(&columns, 10.0, 2000.0, 8.0, None);
    assert_eq!(fit_wide.get(C::Cpu).unwrap().1, 90.0);
    assert_eq!(fit_wide.get(C::Detail).unwrap().1, 360.0, "the detail at its most");
    let name = fit_wide.get(C::Name).unwrap();
    assert_eq!(name.0, 10.0);
    assert!((fit_wide.end() - 2010.0).abs() <= 8.0, "the name took the rest");
}

#[test]
fn too_narrow_for_what_stays_cuts_the_first() {
    let fit = fit(&columns(), 0.0, 180.0, 8.0, Some(C::Cpu));
    assert!(fit.cramped);
    assert!(fit.end() <= 180.0);
    assert!(fit.shows(C::Cpu));
}

#[test]
fn a_sized_first_column_keeps_its_width_and_the_last_takes_the_rest() {
    let mut columns = columns();
    columns[0] = columns[0].min(80.0).sized(Some(240.0));
    let fit = fit(&columns, 0.0, 2000.0, 8.0, None);
    assert_eq!(fit.get(C::Name).unwrap().1, 240.0);
    assert!(fit.dropped.is_empty());
    let (detail_x, detail_w) = fit.get(C::Detail).unwrap();
    assert!(detail_w > 360.0, "the detail takes what's left, past its most");
    assert!((detail_x + detail_w - 2000.0).abs() <= 1.0, "up to the end");
    // Narrower than its spec's least: the user's least is the floor.
    columns[0] = columns[0].sized(Some(40.0));
    assert_eq!(super::fit(&columns, 0.0, 2000.0, 8.0, None).get(C::Name).unwrap().1, 80.0);
}

#[test]
fn a_sized_column_keeps_its_width_and_gives_way_only_when_nothing_can_leave() {
    let mut columns = columns();
    columns[0] = columns[0].min(80.0).sized(Some(600.0));
    let all: f32 = columns.iter().skip(1).map(ColumnSpec::least).sum::<f32>() + 7.0 * 8.0;
    // Room for every other column and a name of 300: the others leave by their priority,
    // the name keeps its width.
    let tight = fit(&columns, 0.0, all + 300.0, 8.0, None);
    assert_eq!(tight.get(C::Name).unwrap().1, 600.0);
    assert_eq!(tight.dropped[0], C::Detail);
    assert!(tight.end() <= all + 300.0);
    // Room again: they come back.
    assert!(fit(&columns, 0.0, all + 900.0, 8.0, None).dropped.is_empty());
    // Narrower than the name and the sorted column: the name gives way, never under its
    // floor, before being cut.
    let narrow = fit(&columns, 0.0, 300.0, 8.0, Some(C::Cpu));
    assert!(!narrow.cramped);
    assert_eq!(narrow.get(C::Name).unwrap().1, 300.0 - 64.0 - 8.0);
    assert!(narrow.shows(C::Cpu));
    let tiny = fit(&columns, 0.0, 100.0, 8.0, Some(C::Cpu));
    assert!(tiny.cramped && tiny.end() <= 100.0);
}

#[test]
fn a_sized_number_column_never_cuts_its_widest_text_nor_grows() {
    let mut columns = columns();
    columns[1] = columns[1].reserving(90.0).min(40.0).sized(Some(50.0));
    columns[2] = columns[2].sized(Some(120.0));
    let fit = fit(&columns, 0.0, 2000.0, 8.0, None);
    assert_eq!(fit.get(C::Cpu).unwrap().1, 90.0);
    assert_eq!(fit.get(C::Memory).unwrap().1, 120.0);
}

#[test]
fn dragging_an_edge_stays_between_the_floor_and_what_the_others_leave() {
    let mut columns = columns();
    columns[0] = columns[0].min(80.0);
    columns[1] = columns[1].min(40.0);
    let width = 1200.0;
    let fit = fit(&columns, 0.0, width, 8.0, None);
    // Each edge but the last, in the gap's middle.
    let edges: Vec<(C, f32)> = fit.edges().collect();
    assert_eq!(edges.len(), fit.shown.len() - 1);
    let (name_x, name_w) = fit.get(C::Name).unwrap();
    assert_eq!(edges[0], (C::Name, name_x + name_w + 4.0));
    assert_eq!(fit.edge_at(name_x + name_w + 6.0, 4.0), Some(C::Name));
    assert_eq!(fit.edge_at(name_x + name_w - 20.0, 4.0), None);
    let (_, detail_x, detail_w) = *fit.shown.last().unwrap();
    assert_eq!(fit.edge_at(detail_x + detail_w + 4.0, 4.0), None, "the last has no edge");
    // The name narrowed past its floor stops there; widened, it stops where the others
    // are at their least.
    assert_eq!(fit.dragged(C::Name, name_w, -10_000.0), Some(80.0));
    let others: f32 = columns.iter().skip(1).map(ColumnSpec::least).sum::<f32>() + 7.0 * 8.0;
    assert_eq!(fit.dragged(C::Name, name_w, 10_000.0), Some((width - others).floor()));
    assert_eq!(fit.dragged(C::Name, name_w, 25.0), Some(name_w + 25.0));
    // The CPU widened as far as it goes: the name back at its least, nothing dropped.
    let most = fit.dragged(C::Cpu, 64.0, 10_000.0).unwrap();
    columns[1] = columns[1].sized(Some(most));
    let after = super::fit(&columns, 0.0, width, 8.0, None);
    assert!(after.dropped.is_empty());
    assert_eq!(after.get(C::Cpu).unwrap().1, most);
    assert_eq!(after.get(C::Name).unwrap().1, 150.0);
    assert!(after.end() <= width + 0.01);
}

#[test]
fn every_width_fits_with_sized_columns() {
    let mut columns = columns();
    columns[0] = columns[0].min(80.0).sized(Some(400.0));
    columns[3] = columns[3].min(40.0).sized(Some(140.0));
    for width in 150..=1600 {
        let fit = fit(&columns, 0.0, width as f32, 8.0, Some(C::Cpu));
        let spans: Vec<(f32, f32)> = fit.shown.iter().map(|&(_, x, w)| (x, x + w)).collect();
        assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "overlap at {width}");
        if !fit.cramped {
            assert!(fit.end() <= width as f32 + 0.01, "past the room at {width}");
        }
        assert!(fit.shows(C::Cpu), "the sorted one at {width}");
        assert!(fit.cramped || fit.get(C::Name).unwrap().1 >= 80.0, "the name's floor at {width}");
        // Once there's room for it, the name has its width again.
        if width >= 400 + 64 + 8 {
            assert_eq!(fit.get(C::Name).unwrap().1, 400.0, "at {width}");
        }
    }
}
