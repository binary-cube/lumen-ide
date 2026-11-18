use super::*;
use std::path::PathBuf;
use std::sync::Arc;

use refactor::application::Sources;
use refactor::moving::{
    ClassFile, MoveContext, MoveFacts, MoveTarget, ScannedFile, Types, plan_move,
};

fn names(scan: &FileScan) -> Vec<String> {
    scan.blocks.iter().flat_map(|b| &b.names).map(|n| n.name.clone()).collect()
}

#[test]
fn finds_the_namespace_imports_and_the_classes_used() {
    let text = r#"<?php
declare(strict_types=1);

namespace App\Http;

use App\Models\User;
use App\Models\{Post, Tag as Label};
use function strlen;

/**
 * @param User|Post $item the item (see me@example.com)
 * @author Someone
 */
#[Route('/x')]
final class Controller extends Base implements \Countable
{
    use Helpers;
    private ?Repo $repo;

    public function show(User $user): Response
    {
        $x = new Label();
        $y = Post::find(1)->tag;
        strlen('App\Models\User');
        return self::render(FOO);
    }
}
?>
<p>new Html()</p>
"#;
    let scan = PhpNamespaces.scan(text);
    assert_eq!(scan.blocks.len(), 1);
    let block = &scan.blocks[0];
    assert_eq!(block.namespace, "App\\Http");
    assert_eq!(&text[block.name_range.clone().unwrap()], "App\\Http");
    let imports: Vec<_> =
        block.imports.iter().map(|i| (i.fqn.as_str(), i.alias.as_str())).collect();
    assert_eq!(
        imports,
        vec![
            ("App\\Models\\User", "User"),
            ("App\\Models\\Post", "Post"),
            ("App\\Models\\Tag", "Label")
        ]
    );
    assert_eq!(block.groups.len(), 1);
    assert_eq!(&text[block.imports[2].item.clone()], "Tag as Label");
    assert!(text[block.import_at..].starts_with("\n\n/**"), "after the last import");
    assert_eq!(block.declared, vec!["Controller"]);
    assert_eq!(
        names(&scan),
        vec![
            "User",
            "Post",
            "Route",
            "Base",
            "Countable",
            "Helpers",
            "Repo",
            "User",
            "Response",
            "Label",
            "Post",
            "FOO"
        ]
    );
    assert!(block.names.iter().any(|n| n.fully_qualified && n.name == "Countable"));
    assert!(scan.strings.iter().any(|s| &text[s.clone()] == "App\\Models\\User"));
}

#[test]
fn a_file_without_a_namespace_gets_one_after_declare() {
    let text = "<?php\ndeclare(strict_types=1);\n\nclass A {}\n";
    let scan = PhpNamespaces.scan(text);
    let block = &scan.blocks[0];
    assert_eq!(block.namespace, "");
    assert_eq!(&text[..block.declare_at], "<?php\ndeclare(strict_types=1);");
}

struct Known;

impl Types for Known {
    fn is_type(&self, fqn: &str) -> bool {
        matches!(fqn, "App\\Models\\Post" | "App\\Models\\User")
    }
}

struct NoSources;

impl Sources for NoSources {
    fn candidates(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn read(&self, _path: &Path) -> Option<String> {
        None
    }
}

fn file(path: &str, text: &str) -> ScannedFile {
    ScannedFile { path: PathBuf::from(path), text: text.into(), scan: PhpNamespaces.scan(text) }
}

/// Moves `App\Models\User` to `App\Domain`; each file as it reads after.
fn moved(files: &[ScannedFile]) -> Vec<String> {
    let roots = refactor::moving::NamespaceRoots::new(
        SEPARATOR,
        [("App\\".to_owned(), PathBuf::from("/p/app"))],
    );
    let class = ClassFile {
        fqn: "App\\Models\\User".into(),
        path: PathBuf::from("/p/app/Models/User.php"),
        subdirs: Vec::new(),
    };
    let target = MoveTarget::new(
        class.path.clone(),
        false,
        PathBuf::from("/p/app/Domain"),
        vec![class],
        &roots,
    );
    let context = MoveContext {
        target,
        roots,
        root: PathBuf::from("/p"),
        sources: Arc::new(NoSources),
        rules: Some(Arc::new(PhpNamespaces)),
        types: Arc::new(Known),
    };
    let facts = MoveFacts { files: files.to_vec() };
    let plan = plan_move(&context, &facts, "App\\Domain", Path::new("/p/app/Domain/User.php"));
    let edits = plan.edits();
    files
        .iter()
        .map(|f| {
            let e = edits.iter().find(|e| e.path == f.path).map(|e| e.edits.clone());
            refactor::apply_edits(&f.text, &e.unwrap_or_default()).unwrap()
        })
        .collect()
}

#[test]
fn moving_a_class_updates_it_and_its_users() {
    let user = file(
        "/p/app/Models/User.php",
        "<?php\n\nnamespace App\\Models;\n\n/** @var Post[] */\nclass User extends Post\n{\n}\n",
    );
    let sibling = file(
        "/p/app/Models/Post.php",
        "<?php\n\nnamespace App\\Models;\n\nclass Post\n{\n    public function author(): User {}\n}\n",
    );
    let grouped = file(
        "/p/app/Http/C.php",
        "<?php\n\nnamespace App\\Http;\n\nuse App\\Models\\{Post, User};\n\nnew User(); new \\App\\Models\\User();\n",
    );
    let after = moved(&[user, sibling, grouped]);
    assert_eq!(
        after[0],
        "<?php\n\nnamespace App\\Domain;\n\nuse App\\Models\\Post;\n\n/** @var Post[] */\nclass User extends Post\n{\n}\n"
    );
    assert_eq!(
        after[1],
        "<?php\n\nnamespace App\\Models;\n\nuse App\\Domain\\User;\n\nclass Post\n{\n    public function author(): User {}\n}\n"
    );
    assert_eq!(
        after[2],
        "<?php\n\nnamespace App\\Http;\n\nuse App\\Models\\{Post};\nuse App\\Domain\\User;\n\nnew User(); new \\App\\Domain\\User();\n"
    );
}

/// Every attribute of a list is a class used (`#[A, B(…)]`, several lines too); a function
/// called in an argument isn't, nor an argument's name.
#[test]
fn every_attribute_of_a_list_is_a_name() {
    let text = "<?php\nnamespace App;\n#[Locked, AsCommand(\n    name: 'x',\n    other: strlen('a'),\n)]\nclass A {}\n";
    let found = names(&PhpNamespaces.scan(text));
    assert!(found.contains(&"Locked".to_owned()), "{found:?}");
    assert!(!found.contains(&"name".to_owned()), "a named argument: {found:?}");
    assert!(found.contains(&"AsCommand".to_owned()), "{found:?}");
    assert!(!found.contains(&"strlen".to_owned()), "{found:?}");
}
