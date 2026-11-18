use std::path::Path;

use code_intelligence::Project;

use super::*;

const CONTROLLER: &str = "return view('users.show', ['user' => $u])->with('orders', $o);";
const ALERT: &str = "@props(['type' => 'info', 'message'])<div></div>";

fn read(path: &Path) -> Option<String> {
    // Joined on Windows with `\\`: the same file either way.
    match path.to_str()?.replace('\\', "/").as_str() {
        "/p/app/C.php" => Some(CONTROLLER.to_owned()),
        "/p/resources/views/components/alert.blade.php" => Some(ALERT.to_owned()),
        _ => None,
    }
}

fn context<'a>(
    project: &'a Project<'a>,
    path: &'a Path,
    text: &'a str,
    offset: usize,
) -> FrameworkContext<'a> {
    FrameworkContext {
        project,
        path,
        language: "blade",
        text,
        offset,
        generation: 0,
        structure: 0,
        own: 0,
    }
}

fn prepared(project: &Project<'_>) -> Blade {
    let views: Views = Arc::new(vec![
        ("users.show".to_owned(), "users/show.blade.php".to_owned()),
        ("components.alert".to_owned(), "components/alert.blade.php".to_owned()),
        ("components.forms.input".to_owned(), "components/forms/input.blade.php".to_owned()),
        ("components.card.index".to_owned(), "components/card/index.blade.php".to_owned()),
    ]);
    let blade = Blade::default();
    blade.prepare(&context(project, Path::new(""), "", 0), &["app/C.php".to_owned()], &views);
    blade
}

fn labels(blade: &Blade, project: &Project<'_>, view: &str, marked: &str) -> Vec<String> {
    let offset = marked.find('¦').unwrap();
    let text = marked.replace('¦', "");
    let path = Path::new("/p/resources/views").join(view);
    blade
        .complete(&context(project, &path, &text, offset))
        .map(|a| a.items.into_iter().map(|i| i.label).collect())
        .unwrap_or_default()
}

fn project() -> Project<'static> {
    Project { root: Some(Path::new("/p")), read: &read, ..Project::none() }
}

#[test]
fn the_variables_passed_to_this_view() {
    let p = project();
    let blade = prepared(&p);
    let found = labels(&blade, &p, "users/show.blade.php", "{{ $¦");
    assert!(found.contains(&"$user".to_owned()) && found.contains(&"$orders".to_owned()));
    assert!(found.contains(&"$errors".to_owned()));
    assert!(!found.contains(&"$slot".to_owned()), "not a component");
    assert!(!labels(&blade, &p, "other.blade.php", "{{ $¦").contains(&"$user".to_owned()));
}

#[test]
fn a_component_has_its_own() {
    let p = project();
    let found = labels(&prepared(&p), &p, "components/alert.blade.php", "{{ $sl¦");
    assert_eq!(found, ["$slot"]);
}

#[test]
fn the_components_by_their_tags() {
    let p = project();
    let blade = prepared(&p);
    assert_eq!(labels(&blade, &p, "a.blade.php", "<x-al¦"), ["x-alert"]);
    assert_eq!(labels(&blade, &p, "a.blade.php", "<x-forms¦"), ["x-forms.input"]);
    assert_eq!(labels(&blade, &p, "a.blade.php", "<x-car¦"), ["x-card"], "index is the folder");
}

#[test]
fn a_component_s_attributes_are_its_props() {
    let p = project();
    let blade = prepared(&p);
    assert_eq!(labels(&blade, &p, "a.blade.php", "<x-alert ty¦"), ["type"]);
    assert!(labels(&blade, &p, "a.blade.php", "<div ty¦").is_empty(), "not a component");
}

#[test]
fn only_blade_files() {
    let p = project();
    let at = context(&p, Path::new("/p/app/C.php"), "{{ $a", 5);
    assert!(prepared(&p).complete(&at).is_none());
}
