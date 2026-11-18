use super::*;
use crate::collector::{Line, key};
use std::collections::VecDeque;

#[test]
fn a_ring_keeps_its_capacity_and_its_order() {
    let mut ring = Ring::new(3);
    assert!(ring.is_empty() && ring.capacity() == 3);
    for v in 1..=5 {
        ring.push(v as f32);
    }
    assert_eq!(ring.len(), 3);
    assert_eq!(ring.iter().collect::<Vec<_>>(), [3.0, 4.0, 5.0], "the oldest gone, in order");
    assert_eq!(ring.last(), Some(5.0));
    let (a, b) = ring.as_slices();
    assert_eq!([a, b].concat(), [3.0, 4.0, 5.0], "two slices, nothing copied");
    let mut out = vec![9.0; 10];
    ring.copy_into(&mut out);
    assert_eq!(out, [3.0, 4.0, 5.0]);
    // Smaller: the oldest dropped; larger: room kept for more.
    ring.set_capacity(2);
    assert_eq!(ring.iter().collect::<Vec<_>>(), [4.0, 5.0]);
    ring.set_capacity(4);
    ring.push(6.0);
    ring.push(7.0);
    assert_eq!(ring.iter().collect::<Vec<_>>(), [4.0, 5.0, 6.0, 7.0]);
    assert_eq!(Ring::new(0).capacity(), 1, "never empty of room");
}

fn line(name: &str, cpu: f32, memory: u64) -> Line {
    let mut line = Line::new(name, "app", workbench_model::resources::APP, None);
    line.cpu = cpu;
    line.memory = Some(memory);
    line.threads = 1;
    line
}

fn frame(cpu: f32, lines: Vec<Line>) -> Frame {
    Frame { cpu, memory: 100, net: 1.0, lines, ..Frame::default() }
}

/// The looks added one by one give the values the old whole table carried: the last
/// minute, the newest last; a series not known (GPU) stays empty.
#[test]
fn looks_added_one_by_one_give_the_last_minute() {
    let mut history = History::default();
    let start = Instant::now();
    // What the old table kept: the last 60 of each.
    let (mut total, mut index) = (VecDeque::new(), VecDeque::new());
    for i in 0..75 {
        let at = start + Duration::from_secs(i);
        history.record(&frame(i as f32, vec![line("Index", i as f32 / 2.0, 10 + i)]), at);
        for (kept, value) in [(&mut total, i as f32), (&mut index, i as f32 / 2.0)] {
            if kept.len() == 60 {
                kept.pop_front();
            }
            kept.push_back(value);
        }
    }
    let cpu = |trace: &Trace| trace.get(Metric::Cpu).iter().collect::<Vec<_>>();
    assert_eq!(cpu(history.total()), total.iter().copied().collect::<Vec<_>>());
    let part = history.part(&key("app", "Index", None)).unwrap();
    assert_eq!(cpu(part), index.iter().copied().collect::<Vec<_>>());
    assert_eq!(part.get(Metric::Memory).last(), Some(84.0));
    assert!(part.get(Metric::Gpu).is_empty(), "no GPU known: none drawn");
    assert_eq!(history.generation(), 75);
}

/// A part gone keeps its values a while (its graphs still drawn), then is forgotten.
#[test]
fn a_part_gone_is_forgotten_after_a_while() {
    let mut history = History::default();
    let start = Instant::now();
    history.record(&frame(1.0, vec![line("Git", 1.0, 1), line("Index", 1.0, 1)]), start);
    let later = |s: u64| start + Duration::from_secs(s);
    history.record(&frame(1.0, vec![line("Index", 1.0, 1)]), later(10));
    assert!(history.part(&key("app", "Git", None)).is_some(), "kept a while");
    history.record(&frame(1.0, vec![line("Index", 1.0, 1)]), later(31));
    assert!(history.part(&key("app", "Git", None)).is_none(), "forgotten");
    assert_eq!(history.parts(), 1);
}

/// Many parts over fifteen minutes stay under the budget: a value kept every few looks.
#[test]
fn the_history_stays_under_its_budget() {
    let mut history = History::new(Duration::from_secs(900), Duration::from_secs(1));
    let start = Instant::now();
    let lines: Vec<Line> = (0..40).map(|i| line(&format!("part {i}"), 1.0, 1)).collect();
    history.record(&frame(1.0, lines.clone()), start);
    assert_eq!(history.step(), 1, "40 parts × 900 values fit");
    assert!(history.bytes() <= BUDGET);
    // 200 parts don't: fewer values, every few looks.
    let many: Vec<Line> = (0..200).map(|i| line(&format!("part {i}"), 1.0, 1)).collect();
    for i in 0..10 {
        history.record(&frame(i as f32, many.clone()), start + Duration::from_secs(i + 1));
    }
    assert!(history.step() > 1, "a step of {}", history.step());
    assert!(history.bytes() <= BUDGET, "{} bytes", history.bytes());
    assert!(history.capacity() < 900);
    // Fewer parts again (the others forgotten): every look kept again.
    let later = start + Duration::from_secs(60);
    history.record(&frame(1.0, lines), later);
    assert_eq!(history.step(), 1);
    assert_eq!(history.capacity(), 900);
}

/// Another window or interval: the rings resized (their newest values kept).
#[test]
fn another_window_resizes_the_rings() {
    let mut history = History::default();
    let start = Instant::now();
    for i in 0..60 {
        history.record(&frame(i as f32, Vec::new()), start + Duration::from_secs(i));
    }
    history.set_window(Duration::from_secs(60), Duration::from_secs(2));
    assert_eq!(history.capacity(), 30);
    assert_eq!(history.total().get(Metric::Cpu).last(), Some(59.0), "the newest kept");
    history.set_window(Duration::from_secs(300), Duration::from_secs(1));
    assert_eq!(history.capacity(), 300);
}

/// The collector's budget for its values over time (plan 40: at most 2 ms a look): 1 000
/// parts recorded into their rings in well under it (a release build).
#[test]
#[cfg_attr(debug_assertions, ignore = "a budget: cargo test -p task-manager --release budget")]
fn budget_a_look_is_recorded_in_time() {
    use std::time::{Duration, Instant};
    let lines: Vec<Line> = (0..1000)
        .map(|i| {
            let mut l =
                Line::new(format!("part {i}"), "owner", workbench_model::resources::APP, None);
            l.cpu = i as f32 % 13.0;
            l.memory = Some(i as u64 * 1024);
            l
        })
        .collect();
    let frame = Frame { lines, ..Frame::default() };
    let mut history = History::new(Duration::from_secs(60), Duration::from_secs(1));
    let start = Instant::now();
    // Warmed up: every ring made.
    history.record(&frame, start);
    let mut times: Vec<Duration> = (1..=31)
        .map(|i| {
            let at = start + Duration::from_secs(i);
            let t = Instant::now();
            history.record(&frame, at);
            t.elapsed()
        })
        .collect();
    times.sort();
    let took = times[15];
    eprintln!("budget: a look of 1000 parts recorded in {took:?}");
    assert!(took <= Duration::from_millis(1), "{took:?}");
}
