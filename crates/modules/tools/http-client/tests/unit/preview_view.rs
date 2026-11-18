//! A response's page in a tab: a page for the web engine once it's been laid out, another
//! identity for another page, Open in Browser writing it and asking the app to open it.

use super::*;
use ui_components::AppRequest;

fn data() -> PreviewData {
    let folder = std::env::temp_dir().join(format!("http-preview-{:x}", kernel::id::new_uuid()));
    PreviewData { html: "<p>hi</p>".into(), url: "https://e.com/".into(), folder }
}

#[test]
fn its_page_is_given_once_it_has_a_place() {
    let mut view = PreviewView::default();
    assert_eq!(view.web_page(), None, "nothing yet");
    view.set_data(&data());
    assert_eq!(view.web_page(), None, "not painted yet: no place");
    view.area = Rect::new(0.0, 40.0, 300.0, 200.0);
    let page = view.web_page().unwrap();
    assert_eq!(page.source, ui_components::WebSource::Html("<p>hi</p>".into()));
    view.set_data(&data());
    assert_ne!(view.web_page().unwrap().id, page.id, "another page: another identity");
}

#[test]
fn open_in_browser_writes_the_page_and_asks_for_it() {
    let mut view = PreviewView::default();
    let data = data();
    view.set_data(&data);
    view.browser = Rect::new(0.0, 0.0, 50.0, 20.0);
    assert!(view.pointer_down(Point::new(5.0, 5.0), 1, Default::default()).is_none());
    let started = std::time::Instant::now();
    while !view.poll() && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let Some(ViewerAction::Request(AppRequest::OpenInBrowser(path))) = view.take_action() else {
        panic!("asked")
    };
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "<p>hi</p>");
    let _ = std::fs::remove_dir_all(&data.folder);
}
