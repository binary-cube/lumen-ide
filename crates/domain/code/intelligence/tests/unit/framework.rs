use super::*;

#[derive(Debug)]
struct F(&'static str, Option<&'static str>);
#[derive(Debug)]
struct Nothing;
impl FrameworkSupport for Nothing {}
impl Framework for F {
    fn id(&self) -> &'static str {
        self.0
    }
    fn name(&self) -> &'static str {
        self.0
    }
    fn family(&self) -> Option<&'static str> {
        self.1
    }
    fn detect(&self, _: &Project<'_>) -> Option<String> {
        None
    }
    fn activate(&self, _: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        Arc::new(Nothing)
    }
}

#[test]
fn one_per_family_and_choices_win() {
    let all: Vec<Arc<dyn Framework>> = vec![
        Arc::new(F("symfony", Some("php"))),
        Arc::new(F("laravel", Some("php"))),
        Arc::new(F("htmx", None)),
    ];
    let found = |ids: &[&str]| -> Vec<(String, String)> {
        ids.iter().map(|i| ((*i).to_owned(), String::new())).collect()
    };
    let detected = found(&["symfony", "laravel", "htmx"]);
    assert_eq!(active_frameworks(&all, &detected, &[]), ["symfony", "htmx"]);
    let choices = [("laravel".to_owned(), true), ("htmx".to_owned(), false)];
    assert_eq!(active_frameworks(&all, &detected, &choices), ["laravel"]);
    let forced = [("laravel".to_owned(), true)];
    assert_eq!(active_frameworks(&all, &[], &forced), ["laravel"]);
}

#[test]
fn component_tags() {
    let src = "<div><UserCard :user=\"u\" /></UserCard><user-card/><span>";
    let at = |w: &str| component_tag_at(src, src.find(w).unwrap() + 1).map(|r| &src[r]);
    assert_eq!(at("UserCard :"), Some("UserCard"));
    assert_eq!(at("user-card/"), Some("user-card"));
    assert_eq!(at("span"), None, "an HTML tag");
    assert_eq!(pascal_case("user-card"), "UserCard");
}

#[test]
fn strings_and_dependencies() {
    let src = "$this->render('user/show.html.twig', [";
    let at = src.find("show").unwrap();
    let content = quoted_at(src, at).unwrap();
    assert_eq!(&src[content.clone()], "user/show.html.twig");
    assert_eq!(before_string(src, &content), "$this->render(");
    assert_eq!(quoted_at("view('us", 9).map(|r| r.start), Some(6), "still typed");
    assert!(quoted_at("f(x)", 2).is_none());
    let html = r#"<a href="{{ path('blog_show') }}">"#;
    let inner = quoted_at(html, html.find("blog").unwrap()).unwrap();
    assert_eq!(&html[inner], "blog_show");
    let composer = r#"{"require": {"php": ">=8.2", "symfony/framework-bundle": "7.*"}}"#;
    assert!(has_dependency(composer, "symfony/framework-bundle"));
    assert!(!has_dependency(
        r#"{"name": "symfony/framework-bundle-x"}"#,
        "symfony/framework-bundle"
    ));
}

#[test]
fn quoted_paths_link_as_a_whole() {
    let twig = "{% import '_macros.html.twig' as ui %}";
    let at = twig.find("macros").unwrap();
    let range = path_in_quotes(twig, at, None).unwrap();
    assert_eq!(&twig[range], "_macros.html.twig");
    let php = "require __DIR__ . '/../config/app.php';";
    let range = path_in_quotes(php, php.find("config").unwrap(), Some('$')).unwrap();
    assert_eq!(&php[range], "/../config/app.php");
    // Plain words and unfinished strings keep the word.
    assert!(path_in_quotes("echo 'hello world';", 7, None).is_none());
    assert!(path_in_quotes("$x = 'a.b", 7, Some('$')).is_none());
    // A variable inside (the language's sigil) isn't a path.
    let interpolated = "\"views/$name.php\";";
    assert!(path_in_quotes(interpolated, 3, Some('$')).is_none());
    assert!(path_in_quotes(interpolated, 3, None).is_none(), "a shell's or JS's `$` too");
}

/// A framework's files: its globs; none watch every file.
#[test]
fn a_framework_watches_its_own_files() {
    let patterns = ["src/**/*.php", "config/**"];
    assert!(watched(&patterns, "src/Controller/Blog.php"));
    assert!(watched(&patterns, "config/routes.yaml"));
    assert!(!watched(&patterns, "public/app.css"));
    assert!(watched(&[], "public/app.css"), "none: every file");
}
