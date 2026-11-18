use super::*;

#[test]
fn knows_what_a_string_names() {
    let at = |src: &str, word: &str| {
        let content = quoted_at(src, src.find(word).expect("the word") + 1)?;
        slot(src, &content)
    };
    let template = "return $this->render('user/show.html.twig')";
    assert_eq!(at(template, "user"), Some(Slot::Template));
    assert_eq!(at("{% extends 'base.html.twig' %}", "base"), Some(Slot::Template));
    assert_eq!(at("<a href=\"{{ path('blog_show') }}\">", "blog"), Some(Slot::Route));
    assert_eq!(at("$this->redirectToRoute('home')", "home"), Some(Slot::Route));
    assert_eq!(at("$app->find('app:auth:flush')", "app:auth"), Some(Slot::Command));
    let tester = "$tester->execute(['command' => 'app:x', 'arg' => 'y'])";
    assert_eq!(at(tester, "app:x"), Some(Slot::Command));
    assert_eq!(at(tester, "'y'"), None);
    assert_eq!(at("echo 'hello'", "hello"), None);
}

/// A command run by the console, in a script: its name's range (typed so far too), not an
/// option, not after a word ending in `console`.
#[test]
fn console_commands_in_scripts() {
    fn at<'a>(src: &'a str, word: &str) -> Option<&'a str> {
        let i = src.find(word).unwrap() + 2;
        console_command_at(src, i).map(|r| &src[r])
    }
    let json = "\"scripts\": { \"flush\": \"bin/console app:auth:flush --force\" }";
    assert_eq!(at(json, "app:auth"), Some("app:auth:flush"));
    assert_eq!(at(json, "--force"), None);
    assert_eq!(at("php bin/console cache:clear && echo ok", "cache"), Some("cache:clear"));
    assert_eq!(at("myconsole app:x", "app:x"), None);
    // The word in prose, a comment, a description: not the console run.
    assert_eq!(at("// open the console and run it", "and"), None);
    assert_eq!(at("\"description\": \"A console app\"", "app"), None);
    let typed = "bin/console app:au";
    assert_eq!(console_command_at(typed, typed.len()).map(|r| &typed[r]), Some("app:au"));
}

/// A project held in memory (`/p`): its files' text.
fn with_project<R>(files: &[(&str, &str)], f: impl FnOnce(&Project<'_>) -> R) -> R {
    let root = std::path::PathBuf::from("/p");
    let read = |path: &std::path::Path| {
        let relative = path.strip_prefix("/p").ok()?.to_string_lossy().replace('\\', "/");
        files.iter().find(|(p, _)| *p == relative).map(|(_, t)| (*t).to_owned())
    };
    let list = |dir: &std::path::Path| {
        let dir = dir.strip_prefix("/p").map(|d| d.to_string_lossy().replace('\\', "/"));
        let dir = dir.unwrap_or_default();
        let mut entries: Vec<(String, bool)> = files
            .iter()
            .filter_map(|(p, _)| p.strip_prefix(&format!("{dir}/")))
            .map(|rest| match rest.split_once('/') {
                Some((folder, _)) => (folder.to_owned(), true),
                None => (rest.to_owned(), false),
            })
            .collect();
        entries.dedup();
        entries
    };
    let project = Project {
        root: Some(&root),
        read: &read,
        list: &list,
        modified: &|_| None,
        levels: &[],
        languages: &[],
    };
    f(&project)
}

fn context<'a>(
    project: &'a Project<'a>,
    path: &'a std::path::Path,
    text: &'a str,
    offset: usize,
) -> FrameworkContext<'a> {
    FrameworkContext {
        project,
        path,
        language: "php",
        text,
        offset,
        generation: 0,
        structure: 0,
        own: 0,
    }
}

const BLOG: &str = "<?php\nclass Blog\n{\n    #[Route('/blog', name: 'blog_list')]\n    public function list() {}\n}\n";

/// A route named at a use or its declaration is a name to rename; its places are its uses
/// and its declaration; one used that isn't declared is said.
#[test]
fn routes_are_found_renamed_and_checked() {
    let use_text = "<?php\nreturn $this->redirectToRoute('blog_list') . path('missing');\n";
    let files = [("src/Blog.php", BLOG), ("src/Use.php", use_text)];
    with_project(&files, |project| {
        let support = Support::default();
        let used = std::path::Path::new("/p/src/Use.php");
        let at = context(project, used, use_text, use_text.find("blog_list").unwrap() + 2);
        let name = support.name_at(&at).expect("a route at a use");
        assert_eq!((name.kind, name.name.as_str()), (ROUTE, "blog_list"));
        let declared = std::path::Path::new("/p/src/Blog.php");
        let at = context(project, declared, BLOG, BLOG.find("blog_list").unwrap() + 1);
        assert_eq!(
            support.name_at(&at).map(|n| n.name),
            Some("blog_list".into()),
            "at its declaration"
        );
        assert_eq!(support.name_places(ROUTE, "blog_list", "src/Blog.php", BLOG).len(), 1);
        assert_eq!(support.name_places(ROUTE, "blog_list", "src/Use.php", use_text).len(), 1);
        assert!(support.names_in(ROUTE, "templates/a.html.twig"));
        assert!(!support.names_in(ROUTE, "public/app.css"));
        assert_eq!(support.name_taken(ROUTE, "blog_list").len(), 1, "a new name taken");
        let at = context(project, used, use_text, 0);
        let problems = support.diagnostics(&at);
        assert_eq!(problems.len(), 1);
        assert_eq!(&use_text[problems[0].range.clone()], "missing");
    });
}

/// With the compiled container: a service named leads to its class's file, is completed
/// with its class, and one it doesn't have is said.
#[test]
fn services_lead_to_their_class_and_unknown_ones_are_said() {
    let xml = "<container><services>\n<service id=\"App\\Util\\Slugger\" class=\"App\\Util\\Slugger\"/>\n<service id=\"slugger\" alias=\"App\\Util\\Slugger\"/>\n</services></container>\n";
    let code = "<?php\n$container->get('slugger');\n$container->get('nope');\n";
    let files = [
        ("var/cache/dev/App_KernelDevDebugContainer.xml", xml),
        ("src/Util/Slugger.php", "<?php\nclass Slugger {}\n"),
        ("src/Use.php", code),
    ];
    with_project(&files, |project| {
        let support = Support::default();
        let path = std::path::Path::new("/p/src/Use.php");
        let at = context(project, path, code, code.find("slugger").unwrap() + 1);
        let found = support.definition(&at).expect("its class");
        assert_eq!(found.path, std::path::PathBuf::from("/p/src/Util/Slugger.php"));
        let typed = "<?php\n$container->get('slu');\n";
        let at = context(project, path, typed, typed.find("slu").unwrap() + 3);
        let completion = support.complete(&at).expect("suggestions");
        let slugger = completion.items.iter().find(|i| i.label == "slugger").expect("its id");
        assert_eq!(slugger.detail.as_deref(), Some("App\\Util\\Slugger"));
        let problems = support.diagnostics(&context(project, path, code, 0));
        assert_eq!(problems.len(), 1);
        assert_eq!(&code[problems[0].range.clone()], "nope");
        // The services' classes by id, for the PHP inference (`$container->get('slugger')`).
        let named = support.named_types(&context(project, path, code, 0));
        let (table, classes) = &named[0];
        assert_eq!(*table, SERVICES);
        assert_eq!(classes.get("slugger").map(String::as_str), Some("App\\Util\\Slugger"));
    });
}

/// Parameters (from `services.yaml`) and translations: a parameter's value on its card, a
/// translation's text in each locale; a translation key no file has is said.
#[test]
fn parameters_and_translations_are_shown_and_checked() {
    let services = "parameters:\n    app.email: 'a@b.c'\nservices:\n    _defaults: ~\n";
    let code = "<?php\n$this->getParameter('app.email');\n$t->trans('user.hi');\n$t->trans('user.nope');\n";
    let files = [
        ("config/services.yaml", services),
        ("translations/messages.en.yaml", "user:\n    hi: Hi\n"),
        ("translations/messages.ro.yaml", "user:\n    hi: Salut\n"),
        ("src/Use.php", code),
    ];
    with_project(&files, |project| {
        let support = Support::default();
        let path = std::path::Path::new("/p/src/Use.php");
        let card =
            |word: &str| support.hover(&context(project, path, code, code.find(word).unwrap() + 1));
        assert_eq!(card("app.email").map(|h| h.code), Some("parameter app.email = a@b.c".into()));
        let hi = card("user.hi").expect("its card");
        assert_eq!(hi.code, "translation user.hi = Hi");
        assert_eq!(hi.doc.as_deref(), Some("en (messages): Hi\nro (messages): Salut"));
        let found =
            support.definition(&context(project, path, code, code.find("user.hi").unwrap() + 1));
        assert_eq!(found.map(|l| l.path), Some("/p/translations/messages.en.yaml".into()));
        let problems = support.diagnostics(&context(project, path, code, 0));
        let named: Vec<&str> = problems.iter().map(|p| &code[p.range.clone()]).collect();
        assert_eq!(named, ["user.nope"], "parameters without the container: not checked");
    });
}

/// A change Symfony hears that leaves a catalog's files as they were (a controller saved)
/// keeps the catalog: the compiled container isn't read again; its files changing (here: the
/// project's file list) makes it again.
#[test]
fn a_catalog_is_made_again_only_when_its_files_change() {
    let support = Support::default();
    let path = std::path::Path::new("/p/src/Use.php");
    let params = |value: &str| format!("parameters:\n    app.mode: {value}\n");
    let first = params("first");
    with_project(&[("config/services.yaml", first.as_str())], |project| {
        let known = support.catalog(
            &FrameworkContext { own: 0, structure: 0, ..context(project, path, "", 0) },
            catalog::Named::Parameter,
        );
        assert_eq!(known.get("app.mode").and_then(|p| p.detail.as_deref()), Some("first"));
    });
    // Same dates (none here), same list: the one made is kept, not read again.
    let second = params("second");
    with_project(&[("config/services.yaml", second.as_str())], |project| {
        let known = support.catalog(
            &FrameworkContext { own: 1, structure: 0, ..context(project, path, "", 0) },
            catalog::Named::Parameter,
        );
        assert_eq!(known.get("app.mode").and_then(|p| p.detail.as_deref()), Some("first"));
        let known = support.catalog(
            &FrameworkContext { own: 2, structure: 1, ..context(project, path, "", 0) },
            catalog::Named::Parameter,
        );
        assert_eq!(known.get("app.mode").and_then(|p| p.detail.as_deref()), Some("second"));
    });
}
