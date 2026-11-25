//! The status bar's windows: an item's window from the `…` list when the item didn't fit,
//! and the resources pinned (pills) never keeping another item's window from opening.

use super::*;

/// A project in a scratch folder with `a.txt` open, laid out at `width`; the folder goes at
/// the end (`Drop`).
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn opened(width: u32) -> (Workbench, Scratch) {
    let dir = std::env::temp_dir().join(format!("status-bar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(width, 700), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    paint(&mut wb);
    (wb, Scratch(dir))
}

fn paint(wb: &mut Workbench) {
    wb.paint(&mut TextSystem::new(), &mut render_api::Scene::default());
}

/// The file's item under the `…` (a narrow window): its row there opens its window, over
/// the `…`, and the window stays drawn.
#[test]
fn a_hidden_item_s_window_opens_from_the_more_list() {
    let (mut wb, _dir) = opened(900);
    let hidden = |wb: &Workbench| wb.status.hidden().iter().any(|i| i.action == Some(FILE_DETAILS));
    // Narrowed until it's under the `…`.
    let mut width = 900;
    while !hidden(&wb) && width > 200 {
        width -= 20;
        wb.resize(PhysicalSize::new(width, 700), 1.0);
        paint(&mut wb);
    }
    assert!(hidden(&wb), "the file's item under the `…`");
    let ids: Vec<String> = wb
        .status_more_entries()
        .into_iter()
        .filter_map(|e| match e {
            ui_workbench::MenuEntry::Item { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    let row = format!("{STATUS_MENU}{FILE_DETAILS}");
    assert!(ids.contains(&row), "{ids:?}");
    assert_eq!(wb.status_more_chosen(FILE_DETAILS), None, "no command: its window");
    paint(&mut wb);
    assert_eq!(wb.status_popover.owner(), Some(FILE_DETAILS), "open over the `…`, kept");
    let more = wb.status.action_rect(ui_workbench::STATUS_MORE).unwrap();
    let window = wb.status_popover.bounds().expect("drawn");
    assert!(window.max_y() <= more.min_y(), "above the bar");
}

/// A window open over the pages is the app's to click, whether or not the window can cut
/// holes in the system's pages (only those are cut).
#[test]
fn what_floats_over_the_pages_is_known_without_holes() {
    let (mut wb, _dir) = opened(900);
    let anchor = wb.status.action_rect(FILE_DETAILS).expect("the file's item shown");
    wb.status_popover.open(FILE_DETAILS, anchor);
    paint(&mut wb);
    let window = wb.status_popover.bounds().expect("drawn");
    wb.set_page_holes(false);
    assert!(wb.page_holes().is_empty(), "nothing cut");
    assert!(wb.floating_over_pages().iter().any(|(r, _)| *r == window), "the app's to click");
    wb.set_page_holes(true);
    assert!(wb.page_holes().iter().any(|(r, _)| *r == window), "cut where it can be");
}

/// The resources pinned (their pills in the editor's corner): hovering the file's item
/// still opens its window, and the pills stay.
#[test]
fn pinned_resources_leave_the_other_windows_free() {
    let (mut wb, _dir) = opened(1400);
    wb.resource_pills.open(RESOURCES, Rect::default());
    wb.resource_pills.set_pinned(true);
    let item = wb.status.action_rect(FILE_DETAILS).expect("the file's item shown");
    assert!(wb.hover_status_popover(item.center()));
    assert_eq!(wb.status_popover.owner(), Some(FILE_DETAILS));
    assert!(wb.resource_pills.is_open() && wb.resource_pills.pinned());
}

/// The mouse going onto a page the app draws: the bar's window open by hover closes (it
/// would stay over the page and take its mouse there), the item's hover goes out, the
/// tooltip goes; a pinned window stays.
#[test]
fn entering_a_page_ends_the_apps_hover() {
    let (mut wb, _dir) = opened(1400);
    let item = wb.status.action_rect(FILE_DETAILS).expect("the file's item shown");
    let _ = wb.hover(item.center());
    assert_eq!(wb.status_popover.owner(), Some(FILE_DETAILS), "open by hover");
    assert!(wb.hover_cleared(), "a redraw");
    assert!(!wb.status_popover.is_open(), "closed");
    assert!(wb.tooltips.areas().is_empty());
    let _ = wb.hover(item.center());
    wb.status_popover.set_pinned(true);
    let _ = wb.hover_cleared();
    assert_eq!(wb.status_popover.owner(), Some(FILE_DETAILS), "pinned: it stays");
}

/// The resources' trail is a minute by time: values told within a second replace each
/// other (a busy terminal waking the app), seconds without a value hold the one before.
#[test]
fn the_resources_trail_keeps_one_value_a_second() {
    let start = std::time::Instant::now();
    let at = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut trail = ResourceTrail::default();
    for i in 0..100 {
        trail.push(at(i), i as f32, 1.0, None);
    }
    assert_eq!(trail.cpu.len(), 1, "a hundred wakes within a second: one value");
    assert_eq!(trail.cpu.back(), Some(&99.0));
    trail.push(at(5_000), 7.0, 1.0, None);
    assert_eq!(trail.cpu.iter().copied().collect::<Vec<_>>(), [99.0, 99.0, 99.0, 99.0, 99.0, 7.0]);
    trail.push(at(500_000), 1.0, 1.0, None);
    assert_eq!(trail.cpu.len(), TRAIL, "never more than a minute");
}

/// The resources' meter has no tooltip (its figures and its window say it all; one at every
/// pass got in the way).
#[test]
fn the_resources_meter_has_no_tooltip() {
    let (mut wb, _dir) = opened(1400);
    wb.set_resources(
        crate::resources::Sample { cpu: 3.0, ram_bytes: 1 << 28, ..Default::default() },
        None,
    );
    paint(&mut wb);
    let meter = wb.status.action_rect(RESOURCES).expect("the meter shown");
    assert!(wb.tooltip_at(meter.center()).is_none());
}

/// The resources pinned (their pills shown): the mouse over their meter opens no window,
/// the pills say it already; unpinned, it opens as before.
#[test]
fn pinned_resources_open_no_window_over_their_meter() {
    let (mut wb, _dir) = opened(1400);
    wb.set_resources(
        crate::resources::Sample { cpu: 3.0, ram_bytes: 1 << 28, ..Default::default() },
        Some(render_api::GpuMemory { shared: 0, window: 1024 }),
    );
    paint(&mut wb);
    let meter = wb.status.action_rect(RESOURCES).expect("the meter shown");
    assert!(wb.hover_status_popover(meter.center()));
    assert_eq!(wb.status_popover.owner(), Some(RESOURCES), "unpinned: its window");
    wb.status_popover.close();
    wb.resource_pills.open(RESOURCES, Rect::default());
    wb.resource_pills.set_pinned(true);
    wb.hover_status_popover(meter.center());
    assert!(!wb.status_popover.is_open(), "pinned: no window");
}

/// The resources' pills and their window stand in the cards' corner, by their meter: as far
/// from the cards' right edge as from their bottom, above the bar.
#[test]
fn the_resources_stand_over_their_meter() {
    let (mut wb, _dir) = opened(1400);
    wb.set_resources(
        crate::resources::Sample { cpu: 3.0, ram_bytes: 1 << 28, ..Default::default() },
        Some(render_api::GpuMemory { shared: 0, window: 1024 }),
    );
    paint(&mut wb);
    let meter = wb.status.action_rect(RESOURCES).expect("the meter shown");
    let bar = wb.status.bounds();
    assert!(wb.hover_status_popover(meter.center()));
    paint(&mut wb);
    let window = wb.status_popover.bounds().expect("its window");
    let corner = wb.cards_corner();
    let cards = wb.card.max_x();
    assert!(corner.x < cards, "{corner:?} in the cards");
    let right = corner.x.min(meter.max_x());
    assert!((window.max_x() - right).abs() <= 1.0, "{window:?} at {right}");
    assert!((window.max_y() - corner.y).abs() <= 1.0 && window.max_y() <= bar.min_y());
    wb.status_popover.close();
    wb.toggle_resources_pin();
    paint(&mut wb);
    let pills = wb.resource_pills.bounds().expect("the pills");
    assert!((pills.max_x() - right).abs() <= 1.0, "{pills:?} at {right}");
    assert!((pills.max_y() - corner.y).abs() <= 1.0);
    // As far from the cards' right edge as from their bottom.
    let open = |r: Rect| (r.size.height > 0.0).then_some(r);
    let low = std::iter::once(wb.card)
        .chain(open(wb.tools.bottom_bounds))
        .map(|r| r.max_y())
        .fold(0.0_f32, f32::max);
    assert!(((cards - corner.x) - (low - corner.y)).abs() <= 1.0, "{corner:?}");
}

/// Memory said in MB under a GB, in GB with one decimal from there on.
#[test]
fn memory_reads_in_mb_then_gb() {
    let mb = 1024 * 1024;
    assert_eq!(memory_text(250 * mb), "250 MB");
    assert_eq!(memory_text(1023 * mb), "1023 MB");
    assert_eq!(memory_text(1216 * mb), "1,2 GB");
    assert_eq!(memory_text(2048 * mb), "2,0 GB");
}

/// The resources' window says the app's own memory, its programs' and their sum (what the
/// system's monitor shows for the app); no programs: neither row.
#[test]
fn the_resources_say_the_app_its_programs_and_the_total() {
    let (mut wb, _dir) = opened(1200);
    let mb = 1024 * 1024;
    let sample = crate::resources::Sample {
        ram_bytes: 240 * mb,
        programs: 35,
        programs_bytes: 1000 * mb,
        ..Default::default()
    };
    wb.resources = Some((sample, None));
    let values: Vec<(String, String)> = wb
        .resource_rows()
        .into_iter()
        .filter_map(|r| match r {
            PopoverRow::Value { label, value, .. } => Some((label, value)),
            PopoverRow::Series { .. } => None,
        })
        .collect();
    assert_eq!(
        values,
        [
            (tr!("status.programs"), "35 · 1000 MB".to_owned()),
            (tr!("status.total"), "1,2 GB".to_owned())
        ]
    );
    // None running: the rows stay (the window keeps its shape), saying 0.
    let none = crate::resources::Sample { programs: 0, programs_bytes: 0, ..sample };
    wb.resources = Some((none, None));
    let values: Vec<String> = wb
        .resource_rows()
        .into_iter()
        .filter_map(|r| match r {
            PopoverRow::Value { value, .. } => Some(value),
            PopoverRow::Series { .. } => None,
        })
        .collect();
    assert_eq!(values, ["0 · 0 MB", "240 MB"]);
}

/// Pinned, the RAM's pill says the app's memory, then (after a bar) the total the system
/// shows for it; the others say one value.
#[test]
fn the_ram_pill_says_the_app_and_the_total() {
    let (mut wb, _dir) = opened(1200);
    let mb = 1024 * 1024;
    let sample = crate::resources::Sample {
        ram_bytes: 198 * mb,
        programs: 4,
        programs_bytes: 169 * mb,
        ..Default::default()
    };
    wb.resources = Some((sample, None));
    let pills = wb.resource_pills();
    let ram = pills.iter().find(|p| p.label == tr!("status.ram")).expect("a RAM pill");
    assert_eq!((ram.value.as_str(), ram.more.as_deref()), ("198 MB", Some("367 MB")));
    assert!(pills.iter().filter(|p| p.label != tr!("status.ram")).all(|p| p.more.is_none()));
}

/// Word wrap's item: shown beside the file's details, a click runs its command; it says
/// whether it's on (its tooltip; its label always, drawn on as a switch only while on).
#[test]
fn word_wrap_s_item_says_whether_it_is_on_and_turns_it() {
    let (mut wb, _dir) = opened(1600);
    let command = crate::commands::WORD_WRAP.0;
    for on in [true, false] {
        let item = wb.word_wrap_item(on);
        let want = if on { tr!("status.wrap_on_detail") } else { tr!("status.wrap_off_detail") };
        assert_eq!(item.tooltip, Some(want));
        assert_eq!(item.text, tr!("status.wrap"), "its label always");
        assert_eq!(item.switch, Some(on), "drawn on as a switch only while on");
        assert_eq!(item.action, Some(command));
    }
    let item = wb.status.action_rect(command).expect("the item shown");
    let clicked = wb.mouse_down(&mut TextSystem::new(), item.center(), false, false, false, 1);
    assert!(matches!(clicked, MouseResult::Command(c) if c == command), "{clicked:?}");
}
