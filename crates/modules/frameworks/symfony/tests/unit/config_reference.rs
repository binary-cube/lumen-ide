use super::*;

const DUMP: &str = r#"# Default configuration for extension with alias: "security"
security:
    access_denied_url:    null # Example: /foo/error403
    firewalls:            # Required
        # Prototype
        name:
            pattern:              ~
            form_login:
                login_path:           /login
    access_control:
        # Prototype
        -
            path:                 null

# Default configuration for extension with alias: "twig"
twig:
    default_path:         '%kernel.project_dir%/templates'
    date:
        format:               'F j, Y H:i'
"#;

fn labels(reference: &Reference, path: &[&str], query: &str) -> Vec<String> {
    let path: Vec<String> = path.iter().map(|p| (*p).to_owned()).collect();
    reference.suggestions(&path, query).into_iter().map(|i| i.label).collect()
}

#[test]
fn the_bundles_and_their_options_are_read() {
    let r = Reference::parse(DUMP);
    assert_eq!(labels(&r, &[], ""), ["twig", "security"], "the shorter first");
    assert!(labels(&r, &["twig"], "def").contains(&"default_path".to_owned()));
    assert_eq!(labels(&r, &["twig", "date"], "form"), ["format"]);
}

#[test]
fn any_name_stands_for_a_prototype() {
    let r = Reference::parse(DUMP);
    assert!(labels(&r, &["security", "firewalls", "main"], "pat").contains(&"pattern".to_owned()));
    let deep = labels(&r, &["security", "firewalls", "api", "form_login"], "login");
    assert_eq!(deep, ["login_path"]);
    assert!(labels(&r, &["security", "firewalls"], "").is_empty(), "the name is the user's");
}

#[test]
fn an_unknown_path_has_nothing() {
    assert!(labels(&Reference::parse(DUMP), &["twig", "nope"], "").is_empty());
}

fn path(text: &str) -> Option<Vec<String>> {
    path_at(text, text.len())
}

#[test]
fn the_path_is_what_holds_the_key() {
    let start = "security:\n    firewalls:\n        main:\n            ";
    assert_eq!(path(start).unwrap(), ["security", "firewalls", "main"]);
    let sibling = "twig:\n    debug: true\n    ";
    assert_eq!(path(sibling).unwrap(), ["twig"]);
    assert_eq!(path("").unwrap(), Vec::<String>::new());
}

#[test]
fn when_is_not_part_of_it_and_a_list_ends_it() {
    assert_eq!(path("when@dev:\n    twig:\n        ").unwrap(), ["twig"]);
    assert_eq!(path("a:\n    - b: 1\n      "), None);
}

#[test]
fn the_bundles_are_read_by_their_short_names() {
    let php = "<?php\nreturn [\n    Symfony\\Bundle\\TwigBundle\\TwigBundle::class => ['all' => true],\n    Doctrine\\Bundle\\DoctrineBundle\\DoctrineBundle::class => ['dev' => true],\n];\n";
    assert_eq!(bundles(php), ["TwigBundle", "DoctrineBundle"]);
}

/// The console run as the project's, each bundle's options kept together; the one failing
/// doesn't lose the others.
#[cfg(unix)]
#[test]
fn what_the_console_prints_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("symfony-reference-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(root.join(kernel::brand::PROJECT_DIR)).unwrap();
    std::fs::write(
        root.join(BUNDLES),
        "A\\TwigBundle::class => [],\nB\\NoOptionsBundle::class => [],\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join(CONSOLE), "").unwrap();
    let fake = root.join("fake-php");
    let script =
        "#!/bin/sh\n[ \"$3\" = TwigBundle ] || exit 1\necho 'twig:'\necho '    debug: false'\n";
    std::fs::write(&fake, script).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(read_all(&root, &fake), Ok(1));
    let kept = std::fs::read_to_string(file_of(&root)).unwrap();
    assert_eq!(labels(&Reference::parse(&kept), &["twig"], "deb"), ["debug"]);
    let _ = std::fs::remove_dir_all(root);
}
