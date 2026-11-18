use super::*;

#[test]
fn only_diagrams_and_its_own_tabs() {
    assert!(is_diagram(Path::new("/p/flow.drawio")));
    assert!(is_diagram(Path::new("/p/FLOW.DRAWIO")));
    assert!(!is_diagram(Path::new("/p/flow.xml")));
    assert!(!is_diagram(Path::new("/p/drawio")));
    let provider = Provider;
    assert_eq!(
        provider.kind(Path::new("/p/flow.drawio.view-diagram-web")),
        Some(ViewerKind::Replace)
    );
    // The file itself too: chosen only when no viewer before it opens it (our own editor).
    assert_eq!(provider.kind(Path::new("/p/flow.drawio")), Some(ViewerKind::Replace));
    assert_eq!(provider.kind(Path::new("/p/flow.xml")), None);
    assert!(EXTENSION.starts_with("view-"));
}

/// A `.drawio` file opened directly (draw.io its default) is a view of that very file, as
/// its own tab's path gives its file.
#[test]
fn a_diagram_opened_directly_is_a_view_of_it() {
    let provider = Provider;
    assert!(provider.open(Path::new("/p/flow.drawio")).is_ok());
    assert!(provider.open(Path::new("/p/flow.drawio.view-diagram-web")).is_ok());
}

/// "Edit in draw.io" is offered beside another opener only: while draw.io opens `.drawio`
/// files itself, it would open the same.
#[test]
fn edit_in_draw_io_is_redundant_while_it_is_the_opener() {
    workbench_model::presence::set_opener(|_| Some("drawio"));
    assert!(!opens_diagrams(), "our editor opens them: the row stays");
    workbench_model::presence::set_opener(|e| (e == FILE_EXTENSION).then_some(PROVIDER_ID));
    assert!(opens_diagrams(), "draw.io opens them: the row goes");
}

/// Two files of the same name in different folders are two tabs: each tab's name is its
/// file's path in the project (the whole path outside it), and the tab's path gives its file
/// back (a tab made again from a session).
#[test]
fn each_file_has_its_own_tab_known_again_from_its_path() {
    let root = Path::new("/work/project");
    let (a, b) = (root.join("a/flow.drawio"), root.join("b/flow.drawio"));
    let (na, nb) = (view_name(Some(root), &a), view_name(Some(root), &b));
    assert_ne!(na, nb);
    assert_eq!(Path::new(&na), Path::new("a/flow.drawio"));
    let tab = root.join(format!("{na}.{EXTENSION}"));
    assert_eq!(file_of_view(&tab), Some(a));
    // Outside the project: its whole path.
    let outside = Path::new("/elsewhere/x.drawio");
    let tab = root.join(format!("{}.{EXTENSION}", view_name(Some(root), outside)));
    assert_eq!(file_of_view(&tab).as_deref(), Some(outside));
    assert_eq!(file_of_view(Path::new("/work/project/notes.txt.view-other")), None);
}
