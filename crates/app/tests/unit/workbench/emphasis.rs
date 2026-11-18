//! The region emphasis in the window: nothing while no module asks; asked, the content
//! muted but the region with the keyboard (or under the pointer), never the overlays over
//! it, kept as it was while one is open; the editor's popups kept; the theme's effects
//! standing still meanwhile; off, everything as before.

use std::sync::Mutex;
use std::time::Instant;

use super::*;
use workbench_model::{EmphasisRequest, Follow, RegionEmphasis};

/// A contributor the test turns on and off (not the Focus mode module's: its state is the
/// process's).
#[derive(Debug, Default)]
struct Asking(Mutex<Option<EmphasisRequest>>);

impl Asking {
    fn set(&self, follow: Option<Follow>) {
        *self.0.lock().unwrap() = follow.map(|follow| EmphasisRequest {
            gray: 1.0,
            dim: 0.1,
            follow,
            pause_effects: true,
        });
    }
}

impl RegionEmphasis for Asking {
    fn request(&self) -> Option<EmphasisRequest> {
        *self.0.lock().unwrap()
    }
}

fn window(theme: Option<&str>) -> (Workbench, Arc<Asking>, TextSystem, Scene) {
    let (mut support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let asking = Arc::new(Asking::default());
    support.region_emphasis = vec![asking.clone() as Arc<dyn RegionEmphasis>];
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    if let Some(id) = theme {
        let mut registry = theming::ThemeRegistry::default();
        for theme in themes_builtin::builtin_themes() {
            registry.register(theme);
        }
        wb.set_theme(registry.resolve(&theming::ThemeId::parse(id)).unwrap());
        wb.set_decorations_window(true, true, false);
    }
    (wb, asking, TextSystem::new(), Scene::default())
}

fn paint(wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
    scene.reset(Rgba::BLACK);
    wb.paint(text, scene);
}

/// The areas the frame keeps.
fn kept(scene: &Scene) -> Vec<Rect> {
    scene.emphasis().map(|e| e.keeps().iter().map(|k| k.rect).collect()).unwrap_or_default()
}

#[test]
fn nothing_while_no_module_asks_and_off_restores_everything() {
    let (mut wb, asking, mut text, mut scene) = window(None);
    paint(&mut wb, &mut text, &mut scene);
    assert!(scene.emphasis().is_none());
    assert!(wb.emphasis.regions.is_empty(), "no work: no regions listed");
    let layers = scene.layers().len();
    asking.set(Some(Follow::Keyboard));
    paint(&mut wb, &mut text, &mut scene);
    // The editor's card (it has the keyboard) kept; every layer of the content muted.
    assert_eq!(kept(&scene), vec![wb.card]);
    assert_eq!(scene.emphasis().map(|e| e.below()), Some(layers));
    assert!(!wb.emphasis.regions.is_empty());
    asking.set(None);
    paint(&mut wb, &mut text, &mut scene);
    assert!(scene.emphasis().is_none());
    assert!(wb.emphasis.regions.is_empty(), "let go");
}

#[test]
fn the_pointer_colors_the_region_under_it_until_a_key() {
    let (mut wb, asking, mut text, mut scene) = window(None);
    asking.set(Some(Follow::Both));
    paint(&mut wb, &mut text, &mut scene);
    let status = wb.status.bounds();
    let over = Point::new(status.min_x() + 40.0, status.min_y() + 4.0);
    assert!(wb.hover(over).0, "another region: a frame");
    paint(&mut wb, &mut text, &mut scene);
    assert_eq!(kept(&scene), vec![status]);
    assert!(!wb.hover(Point::new(over.x + 30.0, over.y)).0, "the same region: no frame");
    // A key: back to the keyboard's.
    wb.emphasis_key();
    paint(&mut wb, &mut text, &mut scene);
    assert_eq!(kept(&scene), vec![wb.card]);
    // Keyboard only: the pointer changes nothing.
    asking.set(Some(Follow::Keyboard));
    wb.hover(over);
    paint(&mut wb, &mut text, &mut scene);
    assert_eq!(kept(&scene), vec![wb.card]);
    // The pointer gone from the window: the keyboard's.
    asking.set(Some(Follow::Pointer));
    wb.hover(over);
    paint(&mut wb, &mut text, &mut scene);
    assert_eq!(kept(&scene), vec![status]);
    assert!(wb.pointer_left());
    paint(&mut wb, &mut text, &mut scene);
    assert_eq!(kept(&scene), vec![wb.card]);
}

#[test]
fn overlays_are_never_muted_and_what_is_behind_a_modal_stays() {
    let (mut wb, asking, mut text, mut scene) = window(None);
    asking.set(Some(Follow::Both));
    paint(&mut wb, &mut text, &mut scene);
    let content = scene.layers().len();
    wb.open_about();
    paint(&mut wb, &mut text, &mut scene);
    let below = scene.emphasis().map(|e| e.below()).unwrap();
    assert_eq!(below, content, "the content only");
    assert!(scene.layers().len() > below, "About drawn over it, not muted");
    // The pointer over the status bar behind it: nothing changes.
    let status = wb.status.bounds();
    assert!(!wb.hover(Point::new(status.min_x() + 40.0, status.min_y() + 4.0)).0);
    paint(&mut wb, &mut text, &mut scene);
    assert_eq!(kept(&scene), vec![wb.card]);
}

#[test]
fn the_theme_effects_stand_still_while_asked() {
    let (mut wb, asking, mut text, mut scene) = window(Some("christmas-dark"));
    paint(&mut wb, &mut text, &mut scene);
    let now = Instant::now();
    wb.decorations_input(now);
    assert!(wb.decorations_deadline(now).is_some(), "moving");
    asking.set(Some(Follow::Both));
    paint(&mut wb, &mut text, &mut scene);
    assert!(wb.decorations_drawn(), "still drawn: the theme's look stays");
    assert_eq!(wb.decorations_deadline(now), None, "no frame of their own");
    let time = wb.decorations_time();
    std::thread::sleep(std::time::Duration::from_millis(20));
    paint(&mut wb, &mut text, &mut scene);
    assert_eq!(wb.decorations_time(), time, "frozen");
    asking.set(None);
    paint(&mut wb, &mut text, &mut scene);
    assert!(wb.decorations_deadline(Instant::now()).is_some(), "moving again");
}
