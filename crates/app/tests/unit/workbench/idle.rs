//! A module's command started after a while without input: once a spell, only in the
//! frontmost window (focused or not), shown, with nothing open over it; input starts the
//! wait again. Relax mode
//! is the one: off by default, started after the minutes its settings say.

use std::time::Duration;

use super::*;

fn window() -> Workbench {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb
}

#[test]
fn relax_mode_starts_after_the_minutes_without_input_once() {
    let relax = relax::settings::config();
    let mut wb = window();
    // Off by default: nothing is waited for.
    relax::settings::set_config(relax::settings::Config::DEFAULT);
    assert_eq!(wb.idle_deadline(), None);
    relax::settings::set_config(relax::settings::Config {
        idle_minutes: Some(5),
        ..relax::settings::Config::DEFAULT
    });
    let start = Instant::now();
    wb.idle_input(start);
    assert_eq!(wb.idle_deadline(), Some(start + Duration::from_secs(300)));
    assert!(!wb.run_idle_actions(start + Duration::from_secs(299), true), "not yet");
    assert!(!wb.has_modal_view());
    // Another window is in front when it's due: skipped this spell (one window starts it).
    assert!(!wb.run_idle_actions(start + Duration::from_secs(300), false));
    assert!(!wb.has_modal_view());
    assert_eq!(wb.idle_deadline(), None, "until input comes");
    // Hidden (minimized): skipped too.
    wb.set_occluded(true);
    wb.idle_input(start);
    assert!(!wb.run_idle_actions(start + Duration::from_secs(300), true));
    wb.set_occluded(false);
    // Input, then five minutes, in front (focused or not): it starts, once.
    let back = start + Duration::from_secs(400);
    wb.idle_input(back);
    assert!(wb.run_idle_actions(back + Duration::from_secs(300), true));
    assert!(wb.has_modal_view() && wb.modal_open());
    assert!(!wb.run_idle_actions(back + Duration::from_secs(900), true), "once a spell");
    // Something already over the window: not again on top of it.
    wb.idle_input(back);
    assert!(!wb.run_idle_actions(back + Duration::from_secs(1300), true));
    relax::settings::set_config(relax);
}
