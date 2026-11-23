use super::*;

/// A view saying it moves, forever (a part that would spin frames).
struct Moving;

impl ui_workbench::Viewer for Moving {
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn is_animating(&self) -> bool {
        true
    }
}

fn workbench() -> Workbench {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb
}

fn tab(wb: &Workbench, name: &str, viewer: Option<Box<dyn ui_workbench::Viewer>>) -> Tab {
    let mut doc = Document::new("");
    doc.mark_saved(PathBuf::from(format!("/virtual/{name}")));
    let mut tab = Tab::new(doc, wb.code_font.clone(), None, LanguageId::PLAIN_TEXT);
    tab.viewer = viewer.map(|v| {
        TabViewer::new(v, "moving-view", ui_workbench::ViewerKind::Replace, ViewMode::Preview)
    });
    tab
}

/// Only what's on screen is asked whether it moves: a tab behind another moves for nobody;
/// brought to the front, it's asked, and named among the parts moving.
#[test]
fn only_what_is_shown_is_asked_whether_it_moves() {
    let mut wb = workbench();
    let moving = tab(&wb, "moving", Some(Box::new(Moving)));
    wb.push_tab(moving);
    let plain = tab(&wb, "plain", None);
    wb.push_tab(plain);
    assert_eq!(wb.tabs.len(), 2);
    wb.active = 1;
    assert!(!wb.moving_parts().contains(&"moving-view"), "behind another tab: not asked");
    wb.active = 0;
    assert!(wb.animating());
    assert!(wb.moving_parts().contains(&"moving-view"), "in front: asked, by its name");
}
