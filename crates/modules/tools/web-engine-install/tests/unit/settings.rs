use super::*;

/// The settings page stops a download under way.
#[test]
fn the_settings_page_has_stop() {
    assert_eq!(WebEngineSettings::default().command("stop"), Some(crate::STOP));
}

/// The settings page is drawn again while it's open whenever the install changes.
#[test]
fn the_settings_page_follows_the_install() {
    let page = WebEngineSettings::default();
    page.drawn.store(service::generation(), Ordering::Relaxed);
    assert!(!page.poll(), "nothing changed");
    page.drawn.store(service::generation().wrapping_add(1), Ordering::Relaxed);
    assert!(page.poll(), "changed since it was drawn");
}
