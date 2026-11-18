use std::path::Path;

use code_intelligence::Project;

use super::*;

const CONTROLLER: &str = "$this->render('user/show.html.twig', ['user' => $u, 'orders' => $o]);";

fn project() -> Project<'static> {
    Project {
        root: Some(Path::new("/p")),
        read: &|path| (path == Path::new("/p/src/C.php")).then(|| CONTROLLER.to_owned()),
        ..Project::none()
    }
}

fn labels(twig: &Twig, project: &Project<'_>, file: &str, marked: &str) -> Option<Vec<String>> {
    let offset = marked.find('¦').unwrap();
    let text = marked.replace('¦', "");
    let path = Path::new("/p/templates").join(file);
    let answer = twig.complete(&context(project, &path, &text, offset))?;
    Some(answer.items.into_iter().map(|i| i.label).collect())
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
        language: "twig",
        text,
        offset,
        generation: 0,
        structure: 0,
        own: 0,
    }
}

fn prepared(project: &Project<'_>) -> Twig {
    let twig = Twig::default();
    let at = context(project, Path::new(""), "", 0);
    twig.prepare(&at, &["C.php".to_owned()]);
    twig
}

#[test]
fn the_variables_its_controller_passes() {
    let project = project();
    let twig = prepared(&project);
    let found = labels(&twig, &project, "user/show.html.twig", "{{ ord¦ }}").unwrap();
    assert!(found.contains(&"orders".to_owned()));
    let other = labels(&twig, &project, "other.html.twig", "{{ ord¦ }}").unwrap_or_default();
    assert!(!other.contains(&"orders".to_owned()), "another template's");
}

#[test]
fn symfony_s_own_words() {
    let project = project();
    let twig = Twig::default();
    let has = |marked: &str, word: &str| {
        labels(&twig, &project, "a.twig", marked).is_some_and(|l| l.iter().any(|x| x == word))
    };
    assert!(has("{{ form_ro¦ }}", "form_row"));
    assert!(has("{{ a|tra¦ }}", "trans"));
    assert!(has("{% form_th¦ %}", "form_theme"));
    assert!(has("{{ app.us¦ }}", "user"));
    assert!(has("{{ ap¦ }}", "app"));
}

#[test]
fn a_string_stays_for_the_routes() {
    let project = project();
    assert_eq!(labels(&Twig::default(), &project, "a.twig", "{{ path('ho¦') }}"), None);
}

#[test]
fn only_templates() {
    let project = project();
    let at = context(&project, Path::new("/p/src/C.php"), "{{ fo", 5);
    assert!(Twig::default().complete(&at).is_none());
}
