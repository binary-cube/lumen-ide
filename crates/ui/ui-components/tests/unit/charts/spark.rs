//! The charts: buckets on empty, single, constant, spiky series and on ones longer than the
//! room (a spike kept by `Max`, the level by `Last`), two series, the quads per pixel
//! column, the reading under the mouse on every column, the nice scale, the cache, the
//! series' colors against both themes.

use super::*;
use render_api::Scene;

fn light() -> Palette {
    Palette { editor_background: Rgba::WHITE, ..Palette::dark() }
}

fn quads(f: impl FnOnce(&mut Layer)) -> usize {
    let mut scene = Scene::default();
    let layer = scene.push_layer(None);
    f(layer);
    scene.quad_count()
}

#[test]
fn an_empty_series_draws_nothing_and_reads_nothing() {
    let plot = Plot::build(Samples::default(), 60, 120, Reduce::Max);
    assert!(plot.is_empty());
    assert_eq!(plot.buckets.len(), 60, "one bucket a place (fewer places than columns)");
    assert!(plot.buckets.iter().all(Option::is_none));
    let palette = Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let rect = Rect::new(0.0, 0.0, 120.0, 40.0);
    for style in [Style::Bars, Style::Steps] {
        assert_eq!(quads(|l| sparkline(l, &ds, rect, &plot, palette.accent, style, 10.0)), 0);
    }
    assert!((0..120).all(|x| plot.reading(rect, x as f32).is_none()));
}

#[test]
fn a_single_value_sits_at_the_right() {
    let plot = Plot::build(Samples::new(&[3.0]), 60, 60, Reduce::Max);
    assert_eq!(plot.len, 1);
    assert_eq!(plot.buckets[59], Some(3.0));
    assert!(plot.buckets[..59].iter().all(Option::is_none));
    let rect = Rect::new(0.0, 0.0, 60.0, 20.0);
    let reading = plot.reading(rect, 59.5).unwrap();
    assert_eq!((reading.value, reading.ago), (3.0, 0));
    assert!(plot.reading(rect, 10.0).is_none(), "no value there yet");
}

#[test]
fn a_constant_series_is_one_step() {
    let values = [5.0; 60];
    let plot = Plot::build(Samples::new(&values), 60, 60, Reduce::Max);
    let palette = Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let rect = Rect::new(0.0, 0.0, 120.0, 40.0);
    // One fill and one step for the whole run.
    assert_eq!(quads(|l| sparkline(l, &ds, rect, &plot, palette.accent, Style::Steps, 10.0)), 2);
    assert_eq!(plot.peak, 5.0);
}

#[test]
fn a_longer_series_keeps_its_spike_or_its_level() {
    let mut values = vec![1.0; 900];
    values[437] = 80.0;
    values[899] = 2.0;
    let max = Plot::build(Samples::new(&values), 900, 100, Reduce::Max);
    assert_eq!(max.buckets.len(), 100);
    assert_eq!(max.peak, 80.0, "the spike survives the aggregation");
    assert_eq!(max.buckets.iter().flatten().filter(|v| **v == 80.0).count(), 1);
    let last = Plot::build(Samples::new(&values), 900, 100, Reduce::Last);
    assert_eq!(last.last(), Some(2.0), "the newest level");
    assert_eq!(last.peak, 2.0, "a level: each bucket's newest value");
}

#[test]
fn values_split_in_two_slices_read_as_one() {
    let whole: Vec<f32> = (0..50).map(|i| i as f32).collect();
    let (a, b) = whole.split_at(17);
    let joined = Plot::build(Samples::from_slices((a, b)), 60, 30, Reduce::Max);
    let single = Plot::build(Samples::new(&whole), 60, 30, Reduce::Max);
    assert_eq!(joined, single);
    assert_eq!(Samples::from_slices((a, b)).last(), Some(49.0));
    assert_eq!(Samples::from_slices((a, b)).get(20), Some(20.0));
}

#[test]
fn never_more_than_two_quads_a_pixel_column() {
    let palette = Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    // Spiky: every value differs from the one before.
    let values: Vec<f32> = (0..900).map(|i| ((i * 37) % 23) as f32).collect();
    for width in [8usize, 20, 33, 60, 61, 120, 300, 899, 900, 1400] {
        let rect = Rect::new(3.0, 0.0, width as f32, 40.0);
        for slots in [60usize, 300, 900] {
            let samples = Samples::new(&values[900 - slots.min(900)..]);
            let plot = Plot::build(samples, slots, width, Reduce::Max);
            for style in [Style::Bars, Style::Steps] {
                let n = quads(|l| sparkline(l, &ds, rect, &plot, palette.accent, style, 25.0));
                assert!(n <= 2 * width, "{n} quads in {width} px ({slots} places, {style:?})");
            }
        }
    }
}

#[test]
fn every_column_reads_its_bucket() {
    let values: Vec<f32> = (0..300).map(|i| i as f32).collect();
    let rect = Rect::new(10.0, 0.0, 150.0, 30.0);
    let plot = Plot::build(Samples::new(&values), 300, 150, Reduce::Last);
    let mut last_ago = usize::MAX;
    for x in 10..160 {
        let reading = plot.reading(rect, x as f32 + 0.5).expect("a value under every column");
        assert_eq!(Some(reading.value), plot.buckets[reading.bucket]);
        // Further right: more recent.
        assert!(reading.ago <= last_ago);
        last_ago = reading.ago;
        assert!(rect.min_x() <= reading.x && reading.x < rect.max_x());
    }
    assert_eq!(plot.reading(rect, 159.5).unwrap().ago, 0, "the newest at the right edge");
    assert_eq!(plot.reading(rect, 159.5).unwrap().value, 299.0);
    assert!(plot.reading(rect, 9.0).is_none() && plot.reading(rect, 160.0).is_none());
}

#[test]
fn the_scale_rounds_up_nicely() {
    for (v, nice) in [
        (0.0, 1.0),
        (0.3, 0.5),
        (1.0, 1.0),
        (1.2, 2.0),
        (2.1, 2.5),
        (3.0, 5.0),
        (7.0, 10.0),
        (18.0, 20.0),
        (412.0, 500.0),
    ] {
        assert!((nice_ceiling(v) - nice).abs() < 1e-3, "{v} → {}", nice_ceiling(v));
    }
    assert_eq!(scale_for(0.4, 5.0), 5.0, "an idle series on its floor");
    assert_eq!(nice_ceiling(f32::NAN), 1.0);
}

#[test]
fn the_cache_builds_only_for_new_values_or_room() {
    let values = [1.0, 2.0, 3.0];
    let mut cache = PlotCache::default();
    assert!(cache.is_stale(1, 60, 60, Reduce::Max));
    let first = cache.get(1, Samples::new(&values), 60, 60, Reduce::Max).clone();
    assert!(!cache.is_stale(1, 60, 60, Reduce::Max));
    // The same generation: the values aren't even read.
    let same = cache.get(1, Samples::new(&[9.0]), 60, 60, Reduce::Max).clone();
    assert_eq!(first, same);
    assert!(cache.is_stale(1, 60, 30, Reduce::Max), "another width");
    assert!(cache.is_stale(2, 60, 60, Reduce::Max), "new values");
    kernel::Purge::purge(&mut cache);
    assert!(cache.is_stale(1, 60, 60, Reduce::Max));
}

#[test]
fn the_series_colors_read_on_both_themes_and_differ() {
    for palette in [Palette::dark(), light()] {
        let colors: Vec<Rgba> = Series::ALL.iter().map(|s| series_color(&palette, *s)).collect();
        for (s, c) in Series::ALL.iter().zip(&colors) {
            let on = crate::severity::contrast(*c, palette.editor_background);
            assert!(on >= 3.0, "{s:?} on {:?}: {on}", palette.editor_background);
        }
        for i in 0..colors.len() {
            for j in i + 1..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }
}

/// The aggregation's budget (plan 40: at most 0.2 ms for 900 values, done once per new value
/// or width), on widths from a card's to a wide chart's, both ways of reducing (a release
/// build).
#[test]
#[cfg_attr(debug_assertions, ignore = "a budget: cargo test -p ui-components --release budget")]
fn budget_900_values_are_aggregated_in_time() {
    use std::time::{Duration, Instant};
    let values: Vec<f32> = (0..900).map(|i| ((i * 37) % 101) as f32).collect();
    let mut plot = Plot::default();
    for columns in [28, 200, 400, 900, 1800] {
        for reduce in [Reduce::Max, Reduce::Last] {
            let mut times: Vec<Duration> = (0..51)
                .map(|_| {
                    let t = Instant::now();
                    plot.fill(Samples::new(&values), 900, columns, reduce);
                    t.elapsed()
                })
                .collect();
            times.sort();
            let took = times[25];
            eprintln!("budget: 900 values on {columns} columns ({reduce:?}) in {took:?}");
            assert!(took <= Duration::from_micros(200), "{columns} {reduce:?}: {took:?}");
        }
    }
}
