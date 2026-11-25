//! Files dragged over the pages the app draws.

use std::sync::Arc;

use platform_api::WebView;
use ui_workbench::WebSource;

use super::super::tests::{Calls, Taking, page};
use super::super::{MadePage, WebViews};
use super::*;

/// Files dragged from the system: over a page that takes them they're its (told as they
/// come, move, leave, drop, in its own pixels); off it, the window's; dropped off it, not
/// its.
#[test]
fn files_dragged_go_to_the_page_under_them() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let made = Arc::clone(&calls);
    views.sync_at(vec![(page(1, &html, 10.0), true)], false, move |_: &WebSource, _, _| {
        let view: Box<dyn WebView> = Box::new(Taking(Arc::clone(&made)));
        Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
    });
    let at = |x: f32, y: f32| kernel::Point::new(x, y);
    let taken = || std::mem::take(&mut *calls.0.lock().unwrap());
    views.files_entered(&[std::path::PathBuf::from("/a.png")]);
    assert_eq!(views.files_at(at(500.0, 5.0), true), FilesOver::Window);
    assert_eq!(views.files_at(at(30.0, 5.0), true), FilesOver::Page);
    assert_eq!(views.files_at(at(31.0, 5.0), true), FilesOver::Page);
    assert_eq!(views.files_at(at(500.0, 5.0), true), FilesOver::Window);
    assert_eq!(views.files_at(at(30.0, 6.0), true), FilesOver::Page);
    assert!(views.files_ended(Some(at(30.0, 6.0))), "dropped on it");
    assert_eq!(taken(), ["enter 1 20 5", "over 21 5", "leave", "enter 1 20 6", "drop 20 6"]);
    views.files_entered(&[std::path::PathBuf::from("/a.png")]);
    assert_eq!(views.files_at(at(500.0, 5.0), true), FilesOver::Window);
    assert!(!views.files_ended(Some(at(500.0, 5.0))), "dropped off it: the window's");
    assert!(!views.files_dragged());
}

/// A page let go while files are dragged over it: dropped there, they're the window's.
#[test]
fn files_over_a_page_gone_are_the_windows() {
    let calls = Arc::new(Calls::default());
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let made = Arc::clone(&calls);
    views.sync_at(vec![(page(1, &html, 10.0), true)], false, move |_: &WebSource, _, _| {
        let view: Box<dyn WebView> = Box::new(Taking(Arc::clone(&made)));
        Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
    });
    views.files_entered(&[std::path::PathBuf::from("/a.png")]);
    let at = kernel::Point::new(30.0, 5.0);
    assert_eq!(views.files_at(at, true), FilesOver::Page);
    views.sync_at(Vec::new(), false, |_: &WebSource, _, _| None::<MadePage>);
    assert!(!views.files_ended(Some(at)), "the page went: the window's");
}
