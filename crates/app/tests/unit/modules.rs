use super::*;
use document::Document;
use kernel::LanguageId;
use std::path::Path;
use syntax_ts::{Highlighter, parse_document};

const SAMPLE: &str = r#"<?php $config = json_decode('{"debug": true}'); ?>
<!doctype html>
<html>
  <head>
    <style>
      body { color: red; }
    </style>
    <script type="application/ld+json">
      {"@type": "Organization", "name": "Example"}
    </script>
    <script>
      const data = JSON.parse('[1, 2, 3]');
      // language=json
      const raw = '{"a": 1}';
    </script>
  </head>
  <body data-config='{"theme": "dark", "size": 2}'>
    <?php
    $x = <<<JSON
    {"items": [1, 2]}
    JSON;
    $list = [
        'a' => 1,
        'b' => 2,
    ];
    ?>
  </body>
</html>
"#;

fn support() -> LanguageSupport {
    let (support, reports) = load(&ModuleSelection::default());
    assert!(reports.iter().all(|r| r.state == ModuleState::Active), "{reports:?}");
    assert!(support.syntax.grammars().validate().is_empty());
    support
}

fn language_of(snapshot: &syntax_ts::SyntaxSnapshot, needle: &str) -> String {
    let offset = SAMPLE.find(needle).unwrap_or_else(|| panic!("{needle}"));
    snapshot.language_at(offset).to_string()
}

#[test]
fn detects_file_languages() {
    let s = support();
    let detect = |p: &str, head: &str| s.languages.detect(Path::new(p), head).to_string();
    assert_eq!(detect("a.php", ""), "php");
    assert_eq!(detect("composer.lock", ""), "json");
    assert_eq!(detect("docker-compose.yml", ""), "yaml");
    assert_eq!(detect("style.scss", ""), "scss");
    assert_eq!(detect("bin/console", "#!/usr/bin/env php\n"), "php");
    assert_eq!(detect("data", "{\"a\": [1, 2]}"), "json");
}

#[test]
fn php_file_has_all_embedded_layers() {
    let s = support();
    let doc = Document::new(SAMPLE);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("php"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    assert_eq!(language_of(&snapshot, "<!doctype"), "html");
    assert_eq!(language_of(&snapshot, "color: red"), "css");
    assert_eq!(language_of(&snapshot, "\"@type\""), "json"); // <script type="application/ld+json">
    assert_eq!(language_of(&snapshot, "const data"), "javascript");
    assert_eq!(language_of(&snapshot, "1, 2, 3]"), "json"); // JSON.parse('...')
    assert_eq!(language_of(&snapshot, "\"a\": 1}'"), "json"); // // language=json
    assert_eq!(language_of(&snapshot, "\"theme\""), "json"); // data-config attribute
    assert_eq!(language_of(&snapshot, "\"debug\""), "json"); // json_decode('...')
    assert_eq!(language_of(&snapshot, "\"items\""), "json"); // heredoc <<<JSON
    assert_eq!(language_of(&snapshot, "$list"), "php");
}

#[test]
fn folds_cover_all_languages_with_labels() {
    let s = support();
    let doc = Document::new(SAMPLE);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("php"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    let line = |needle: &str| SAMPLE[..SAMPLE.find(needle).unwrap()].matches('\n').count() as u32;
    let fold_at = |l: u32| snapshot.folds().iter().find(|f| f.start_line == l).cloned();
    assert!(fold_at(line("<html>")).is_some(), "element HTML");
    assert!(fold_at(line("<style>")).is_some(), "style");
    assert!(
        fold_at(line("$list"))
            .is_some_and(|f| f.placeholder == Some(languages::FoldCount::Items(2)))
    );
    assert!(fold_at(line("{\"@type\"")).is_none(), "an object on a single line");
}

const TWIG: &str = r#"{% extends 'base.html.twig' %}

{# Pagina de profil #}
{% block body %}
  <style>
    .card { color: red; }
  </style>
  {% if user %}
    <h1 class="title">{{ user.name|upper }}</h1>
    {% for post in user.posts %}
      <p>{{ post.title }}</p>
    {% endfor %}
  {% endif %}
  <script>
    const id = {{ user.id }};
  </script>
{% endblock %}
"#;

#[test]
fn twig_templates_embed_html_css_js_and_fold_blocks() {
    let s = support();
    assert_eq!(s.languages.detect(Path::new("templates/base.html.twig"), "").to_string(), "twig");
    let doc = Document::new(TWIG);
    let twig = LanguageId::new_static("twig");
    let snapshot =
        parse_document(&s.syntax, &twig, &doc.buffer().snapshot(), None, &[], 0).unwrap();
    let lang = |needle: &str| snapshot.language_at(TWIG.find(needle).unwrap()).to_string();
    assert_eq!(lang("user.name"), "twig");
    assert_eq!(lang("class=\"title\""), "html");
    assert_eq!(lang("color: red"), "css");
    assert_eq!(lang("const id"), "javascript");
    let line = |needle: &str| TWIG[..TWIG.find(needle).unwrap()].matches('\n').count() as u32;
    let fold = |needle: &str| {
        snapshot.folds().iter().find(|f| f.start_line == line(needle)).map(|f| f.end_line)
    };
    assert_eq!(fold("{% block body"), Some(line("{% endblock")));
    assert_eq!(fold("{% if user"), Some(line("{% endif")));
    assert_eq!(fold("{% for post"), Some(line("{% endfor")));
    assert!(fold("<style>").is_some(), "HTML folding in Twig");
}

#[test]
fn php_completion_and_definition_across_files() {
    use code_intelligence::{Project, SymbolIndex, complete, definition};
    let s = support();
    let php = s.intelligence.iter().find(|i| i.language().as_str() == "php").expect("php");
    let user = r#"<?php
namespace App\Models;

class Model { public function save(): bool { return true; } }

class User extends Model {
    public string $name;
    public const ADMIN = 'admin';
    public static function find(int $id): ?static { return null; }
    public function posts(): array { return []; }
    protected function cache(): void {}
    private function secret(): void {}
    public function own() { $this-> }
}
"#;
    let controller = "<?php\nnamespace App\\Http;\n\nuse App\\Models\\User;\n\nclass UserController {\n    public function show(User $user) {\n        $user->|\n    }\n}\n";
    let mut index = SymbolIndex::default();
    index.set_file(Path::new("/p/app/Models/User.php"), php.extract(user));
    let at = controller.find('|').unwrap();
    let text = controller.replace('|', "");
    let ctl = Path::new("/p/app/Http/UserController.php");
    index.set_file(ctl, php.extract(&text));

    let labels = |text: &str, at: usize| -> Vec<String> {
        complete(php.as_ref(), &index, ctl, text, at, &Project::none())
            .1
            .into_iter()
            .map(|i| i.label)
            .collect()
    };
    let members = labels(&text, at);
    assert!(
        members.contains(&"posts".to_owned()) && members.contains(&"save".to_owned()),
        "{members:?}"
    );
    assert!(members.contains(&"name".to_owned()));

    let typed = text.replace("$user->", "User::");
    let statics = labels(&typed, at - 1);
    assert!(
        statics.contains(&"find".to_owned()) && statics.contains(&"ADMIN".to_owned()),
        "{statics:?}"
    );

    let typed = text.replace("$user->", "new Us");
    assert!(labels(&typed, at - 1).first().is_some_and(|l| l == "User"));

    // Visibility: from the controller (outside the class) only public members; through
    // `$this` in User, also protected / private ones.
    assert!(
        !members.contains(&"secret".to_owned()) && !members.contains(&"cache".to_owned()),
        "{members:?}"
    );
    let user_path = Path::new("/p/app/Models/User.php");
    let inside = user.find("$this-> ").unwrap() + "$this->".len();
    let own: Vec<String> =
        complete(php.as_ref(), &index, user_path, user, inside, &Project::none())
            .1
            .into_iter()
            .map(|i| i.label)
            .collect();
    assert!(own.contains(&"secret".to_owned()) && own.contains(&"cache".to_owned()), "{own:?}");

    // Auto-import: `User` is already imported in the controller; a class from another
    // namespace isn't.
    let typed = text.replace("$user->", "new Mod");
    let items = complete(php.as_ref(), &index, ctl, &typed, at, &Project::none()).1;
    let model = items.iter().find(|i| i.label == "Model").expect("Model");
    assert_eq!(model.import.as_deref(), Some("App\\Models\\Model"));
    let typed = text.replace("$user->", "new Use");
    let items = complete(php.as_ref(), &index, ctl, &typed, at, &Project::none()).1;
    assert_eq!(items.iter().find(|i| i.label == "User").and_then(|i| i.import.clone()), None);

    // Go to definition: from the controller to class User and to the inherited method `save`.
    let call = text.replace("$user->", "$user->save();");
    let defs = definition(
        php.as_ref(),
        &index,
        ctl,
        &call,
        call.find("save").unwrap() + 1,
        &Project::none(),
    );
    assert_eq!(defs.first().map(|d| d.path.as_path()), Some(Path::new("/p/app/Models/User.php")));
    let class_at = text.find("User $user").unwrap() + 1;
    let defs = definition(php.as_ref(), &index, ctl, &text, class_at, &Project::none());
    assert_eq!(defs.first().map(|d| d.line), Some(5));
}

/// A PHP attribute names its class (through `use`), on one line or several: go to
/// definition goes there, the hover and the signature help show its constructor, completion
/// after `#[` offers classes (imported when needed).
#[test]
fn php_attributes_are_classes_made_through_their_constructor() {
    use code_intelligence::{Project, SymbolIndex, complete, definition, hover, signature_help};
    let s = support();
    let php = s.intelligence.iter().find(|i| i.language().as_str() == "php").expect("php");
    let attribute = r#"<?php
namespace Symfony\Component\Console\Attribute;

#[\Attribute(\Attribute::TARGET_CLASS)]
class AsCommand
{
    public function __construct(
        public string $name,
        public ?string $description = null,
        array $aliases = [],
        bool $hidden = false,
    ) {}
}
"#;
    let command = r#"<?php
namespace App\Command;

use Symfony\Component\Console\Attribute\AsCommand;

#[AsCommand(
    name:        'app:auth:flush',
    description: 'Sign every admin out',
)]
final class FlushAuthCommand {}
"#;
    let mut index = SymbolIndex::default();
    let vendor = Path::new("/p/vendor/symfony/console/Attribute/AsCommand.php");
    index.set_file(vendor, php.extract(attribute));
    let path = Path::new("/p/src/Command/FlushAuthCommand.php");
    index.set_file(path, php.extract(command));
    let on = command.find("AsCommand(").unwrap() + 2;
    let defs = definition(php.as_ref(), &index, path, command, on, &Project::none());
    assert_eq!(defs.first().map(|d| d.path.as_path()), Some(vendor), "go to definition");
    let card = hover(php.as_ref(), &index, path, command, on, &Project::none()).expect("hover");
    assert!(card.code.contains("$name") && card.code.contains("$description"), "{card:?}");
    // Writing the description: its parameter.
    let at = command.find("'Sign").unwrap();
    let help = signature_help(php.as_ref(), &index, path, command, at, &Project::none())
        .expect("signature help");
    assert!(help.code.contains("$description"), "{help:?}");
    // `#[AsC` offers the class, imported.
    let typed = "<?php\nnamespace App\\Command;\n\n#[AsC";
    let other = Path::new("/p/src/Command/Other.php");
    let items = complete(php.as_ref(), &index, other, typed, typed.len(), &Project::none()).1;
    let item = items.iter().find(|i| i.label == "AsCommand").expect("AsCommand offered");
    assert_eq!(item.import.as_deref(), Some("Symfony\\Component\\Console\\Attribute\\AsCommand"));
    assert!(items.iter().all(|i| i.label != "strlen"), "classes only");
    // Its arguments by name: those not written yet, first, in the signature's order.
    let typed = command.replace("description: 'Sign every admin out',", "de");
    let at = typed.find("de\n").unwrap() + 2;
    let items = complete(php.as_ref(), &index, path, &typed, at, &Project::none()).1;
    assert_eq!(items.first().map(|i| i.insert.as_str()), Some("description: "), "{items:?}");
    assert!(items.iter().all(|i| i.label != "name:"), "already named");
    let typed = command.replace("    name:        'app:auth:flush',\n", "    hi,\n");
    let at = typed.find("hi,").unwrap() + 2;
    let items = complete(php.as_ref(), &index, path, &typed, at, &Project::none()).1;
    assert_eq!(items.first().map(|i| i.label.as_str()), Some("hidden:"), "{items:?}");
    // The signature help follows a named argument (not its position).
    let named = command.replace("name:        'app:auth:flush',\n    ", "");
    let at = named.find("'Sign").unwrap();
    let help = signature_help(php.as_ref(), &index, path, &named, at, &Project::none()).unwrap();
    let active = help.active.map(|i| &help.code[help.params[i].clone()]);
    assert!(active.is_some_and(|p| p.contains("$description")), "{active:?}");
    // A name another one starts with (`name` / `names`) isn't the argument being written.
    let longer = command.replace("description: 'Sign every admin out',", "names: [],");
    let at = longer.find("[]").unwrap();
    let help = signature_help(php.as_ref(), &index, path, &longer, at, &Project::none()).unwrap();
    let active = help.active.map(|i| &help.code[help.params[i].clone()]);
    assert!(!active.is_some_and(|p| p.contains("$name")), "{active:?}");
    // A declaration's own parameters take no named arguments.
    let declaring = "<?php\nfunction save(string $user, ";
    let other = Path::new("/p/src/save.php");
    index.set_file(other, php.extract(declaring));
    let items =
        complete(php.as_ref(), &index, other, declaring, declaring.len(), &Project::none()).1;
    assert!(items.iter().all(|i| !i.insert.ends_with(": ")), "{items:?}");
}

#[test]
fn syntax_errors_in_file_and_embedded_layers() {
    let s = support();
    let src = "<?php\n$a = 1\necho $a;\n?>\n<div class=\"<?= $a ?>\">ok</div>\n<style>\n.x { color: red; }\n</style>\n";
    let doc = Document::new(src);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("php"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    let errors = snapshot.errors();
    assert!(!errors.is_empty(), "`;` missing after `$a = 1`");
    let line_of = |o: usize| src[..o].matches('\n').count();
    assert!(errors.iter().all(|e| line_of(e.range.start) <= 2), "{errors:?}");
    // A correct file has no errors (neither in the HTML with PHP in attributes, nor in CSS).
    let ok = src.replace("$a = 1\n", "$a = 1;\n");
    let doc = Document::new(&ok);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("php"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    assert_eq!(snapshot.errors(), Vec::new());
}

/// PHP mixed with HTML, scripts and styles is valid as written: a `<script>` holding a
/// whole PHP block, PHP printed into JavaScript and CSS, the alternative syntax wrapping
/// HTML (`if (…): ?> … <?php endif; ?>`). No error is said where PHP is cut out of them.
#[test]
fn php_mixed_with_html_scripts_and_styles_has_no_errors() {
    let s = support();
    let src = r#"<script>
<?php
    if (!isset($_GET["tip"])) {
        exit();
    }
    $tip = $_GET["tip"];
?>
    var tip = <?= json_encode($tip) ?>;
    var items = [<?php foreach ($list as $i): ?>"<?= $i ?>",<?php endforeach; ?>];
    if (tip) { console.log(tip); }
</script>
<style>
    .box { color: <?= $color ?>; width: <?php echo $w; ?>px; }
</style>
<?php if ($user): ?>
    <div class="user <?= $user->active ? 'on' : '' ?>">
        <?php foreach ($user->roles as $role): ?>
            <span><?= htmlspecialchars($role) ?></span>
        <?php endforeach; ?>
    </div>
<?php elseif ($guest): ?>
    <a href="/login">Login</a>
<?php else: ?>
    <p>Nobody</p>
<?php endif; ?>
<?php while ($row = next($rows)): ?><tr><td><?= $row ?></td></tr><?php endwhile; ?>
"#;
    let doc = Document::new(src);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("php"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    let errors: Vec<_> =
        snapshot.errors().into_iter().map(|e| (src[e.range.clone()].to_owned(), e)).collect();
    assert!(errors.is_empty(), "{errors:#?}");
    // The PHP inside the script is PHP (not JavaScript), the script around it JavaScript.
    let at = |needle: &str| src.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert_eq!(snapshot.language_at(at("isset")).as_str(), "php");
    assert_eq!(snapshot.language_at(at("console")).as_str(), "javascript");
    assert_eq!(snapshot.language_at(at("color:")).as_str(), "css");
    // A real mistake in the script is still said.
    let broken = src.replace("if (tip) {", "if (tip {");
    let doc = Document::new(&broken);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("php"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    assert!(!snapshot.errors().is_empty(), "the script's own mistake");
}

#[test]
fn highlights_use_embedded_scopes() {
    let s = support();
    let doc = Document::new(SAMPLE);
    let text = doc.buffer().snapshot();
    let snapshot =
        parse_document(&s.syntax, &LanguageId::new_static("php"), &text, None, &[], 0).unwrap();
    let mut spans = Vec::new();
    Highlighter::new().highlight(&snapshot, &text, 0..SAMPLE.len(), &mut spans);
    let key = SAMPLE.find("\"theme\"").unwrap();
    let scope = spans
        .iter()
        .rev()
        .find(|sp| sp.range.contains(&key))
        .and_then(|sp| s.syntax.scope(sp.scope))
        .map(|(lang, name)| format!("{name}.{lang}"));
    assert_eq!(scope.as_deref(), Some("property.json"));
}

/// A PHP attribute's name is colored as a type, a named argument's name as a parameter
/// (the last span applied wins).
#[test]
fn php_attributes_and_named_arguments_are_highlighted() {
    let s = support();
    let src =
        "<?php\n#[AsCommand(\n    name: 'app:x',\n)]\n#[ORM\\Column(type: 'string')]\nclass A {}\n";
    let doc = Document::new(src);
    let text = doc.buffer().snapshot();
    let snapshot =
        parse_document(&s.syntax, &LanguageId::new_static("php"), &text, None, &[], 0).unwrap();
    let mut spans = Vec::new();
    Highlighter::new().highlight(&snapshot, &text, 0..src.len(), &mut spans);
    let scope_at = |needle: &str| {
        let at = src.find(needle).unwrap();
        spans
            .iter()
            .rev()
            .find(|sp| sp.range.contains(&at))
            .and_then(|sp| s.syntax.scope(sp.scope))
            .map(|(_, name)| name)
    };
    assert_eq!(scope_at("AsCommand").as_deref(), Some("type"));
    assert_eq!(scope_at("Column").as_deref(), Some("type"));
    assert_eq!(scope_at("name:").as_deref(), Some("variable.parameter"));
    assert_eq!(scope_at("type:").as_deref(), Some("variable.parameter"));
}

/// Incremental parsing must give exactly the result of a parse from scratch.
#[test]
fn incremental_parse_matches_full_parse() {
    use document::{EditKind, Transaction};
    let s = support();
    let php = LanguageId::new_static("php");
    let edits: [(&str, &str); 5] = [
        ("\"debug\": true", "\"debug\": false, \"x\": [1]"),
        ("<body", "<section>\n    <p>nou</p>\n  </section>\n  <body"),
        ("$list = [", "$extra = json_decode('{\"k\": 1}');\n    $list = ["),
        ("const raw", "// language=json\n      const more = '[2]';\n      const raw"),
        ("    <style>\n      body { color: red; }\n    </style>\n", ""),
    ];
    let mut doc = Document::new(SAMPLE);
    let mut previous =
        parse_document(&s.syntax, &php, &doc.buffer().snapshot(), None, &[], doc.version())
            .unwrap();
    for (needle, replacement) in edits {
        let text = doc.buffer().to_string();
        let at = text.find(needle).unwrap_or_else(|| panic!("{needle}"));
        let version = doc.version();
        doc.apply(Transaction::single(at..at + needle.len(), replacement), None, EditKind::Command);
        let changes = doc.changes_since(version).unwrap();
        previous.apply_changes(&changes, doc.version());
        let snapshot = doc.buffer().snapshot();
        let incremental =
            parse_document(&s.syntax, &php, &snapshot, Some(&previous), &changes, doc.version())
                .unwrap();
        let full = parse_document(&s.syntax, &php, &snapshot, None, &[], doc.version()).unwrap();

        let text = doc.buffer().to_string();
        let languages = |snap: &syntax_ts::SyntaxSnapshot| {
            (0..text.len()).step_by(7).map(|o| snap.language_at(o).to_string()).collect::<Vec<_>>()
        };
        let mut a = incremental.describe_layers();
        let mut b = full.describe_layers();
        a.sort();
        b.sort();
        assert_eq!(a, b, "layers after `{needle}`");
        assert_eq!(languages(&incremental), languages(&full), "languages after `{needle}`");
        let folds = |snap: &syntax_ts::SyntaxSnapshot| {
            snap.folds()
                .iter()
                .map(|f| (f.start_line, f.end_line, f.placeholder))
                .collect::<Vec<_>>()
        };
        assert_eq!(folds(&incremental), folds(&full), "folding after `{needle}`");
        assert_eq!(incremental.layer_count(), full.layer_count(), "layers after `{needle}`");
        previous = incremental;
    }
}

/// Benchmark:
/// `EDITOR_BENCH_FILE=<file> cargo test --release -p app bench -- --ignored --nocapture`.
#[test]
#[ignore = "a manual measurement on a given file"]
fn bench_incremental_parse() {
    use document::{EditKind, Transaction};
    let Some(path) = std::env::var_os("EDITOR_BENCH_FILE") else { return };
    let s = support();
    let text = std::fs::read_to_string(&path).unwrap();
    let language = s.languages.detect(Path::new(&path), &text[..text.len().min(4096)]);
    let mut doc = Document::new(&text);
    let started = std::time::Instant::now();
    let mut previous =
        parse_document(&s.syntax, &language, &doc.buffer().snapshot(), None, &[], doc.version())
            .unwrap();
    println!("full: {:?} ({} layers)", started.elapsed(), previous.layer_count());
    for i in 0..5 {
        let at = doc.buffer().line_start(doc.buffer().line_count() / 2 + i * 10);
        let version = doc.version();
        doc.apply(Transaction::single(at..at, "x"), None, EditKind::Typing);
        let changes = doc.changes_since(version).unwrap();
        let started = std::time::Instant::now();
        previous.apply_changes(&changes, doc.version());
        previous = parse_document(
            &s.syntax,
            &language,
            &doc.buffer().snapshot(),
            Some(&previous),
            &changes,
            doc.version(),
        )
        .unwrap();
        println!("incremental #{i}: {:?}", started.elapsed());
    }
}

#[test]
fn a_disabled_module_takes_nothing_else_along() {
    let (support, reports) =
        load(&ModuleSelection { disabled: vec!["lang.html".into()], ..Default::default() });
    assert!(support.languages.get(&LanguageId::new_static("html")).is_none());
    // Modules do not depend on each other: PHP starts without HTML.
    assert!(
        reports.iter().any(|r| r.id.as_str() == "lang.php" && r.state == ModuleState::Active),
        "PHP works without HTML (no HTML colors in it, nothing more)"
    );
}

#[test]
#[ignore = "a measurement: cargo test -p app --release measure_activation -- --ignored --nocapture"]
fn measure_activation() {
    let (_, reports) = builtin().activate(&ModuleSelection::default());
    let mut total = std::time::Duration::ZERO;
    for r in &reports {
        total += r.took;
        println!("{:>10.3} ms  {}", r.took.as_secs_f64() * 1000.0, r.id);
    }
    println!("{:>10.3} ms  total", total.as_secs_f64() * 1000.0);
}

/// The process's resident memory (KB), from `ps` (measurements only).
fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok();
    out.and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).unwrap_or(0)
}

#[test]
#[ignore = "a measurement: cargo test -p app --release measure_preload -- --ignored --nocapture"]
fn measure_preload() {
    let registry = builtin();
    let (_, _) = registry.activate(&ModuleSelection::default());
    for id in ["lang.php", "lang.css", "lang.html", "lang.javascript"] {
        let before = rss_kb();
        let took = registry.preload(|m| m.as_str() == id);
        let after = rss_kb();
        println!(
            "{id:>16}: {:?}  +{} KB",
            took[0].1.unwrap_or_default(),
            after.saturating_sub(before)
        );
    }
    let released = registry.release_idle(std::time::Duration::ZERO, |_| false);
    println!("released by {released} modules; now {} KB", rss_kb());
}

#[test]
#[ignore = "a measurement: cargo test -p app --release measure_grammars -- --ignored --nocapture"]
fn measure_grammars() {
    let (support, _) = load(&ModuleSelection::default());
    for language in [
        "php",
        "html",
        "javascript",
        "typescript",
        "css",
        "scss",
        "json",
        "yaml",
        "twig",
        "blade",
        "markdown",
        "c",
        "cpp",
        "lua",
        "zig",
        "bash",
        "toml",
        "dockerfile",
    ] {
        let before = rss_kb();
        let started = std::time::Instant::now();
        support.syntax.grammars().warm_up(&languages::LanguageId::new(language));
        println!(
            "{language:>12}: {:?}  +{} KB",
            started.elapsed(),
            rss_kb().saturating_sub(before)
        );
    }
}

/// What modules contribute shows in one order, the same at every start: the tool windows by
/// side then their order (no two share a place: the order never falls to chance), the
/// menus and commands as the modules are registered.
#[test]
fn contributions_keep_one_order() {
    let names = |s: &LanguageSupport| {
        let tools: Vec<_> = s.tools.iter().map(|t| (t.dock(), t.order(), t.id())).collect();
        let menus: Vec<_> = s.menus.iter().map(|m| m.id).collect();
        let commands: Vec<_> = s.commands.iter().map(|c| c.spec.id).collect();
        (tools, menus, commands)
    };
    let (first, second) = (support(), support());
    assert_eq!(names(&first), names(&second));
    let (tools, _, _) = names(&first);
    for pair in tools.windows(2) {
        assert!(
            (pair[0].0, pair[0].1) != (pair[1].0, pair[1].1),
            "{} and {} share a place",
            pair[0].2,
            pair[1].2
        );
    }
}

/// A Vue component: its `lang="ts"` script is TypeScript (and so are its template's
/// expressions: the directives' values, the `{{ }}`), its `lang="scss"` style SCSS; an
/// htmx handler (`hx-on:click`) is JavaScript in any HTML.
#[test]
fn vue_and_htmx_embed_their_languages() {
    let s = support();
    const VUE: &str = "<template>\n  <div :class=\"{ active: isOn }\" @click=\"count++\">{{ count * 2 }}</div>\n</template>\n\n<script setup lang=\"ts\">\nconst count = ref<number>(0);\n</script>\n\n<style lang=\"scss\">\n.card { .title { color: red; } }\n</style>\n";
    let doc = Document::new(VUE);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("html"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    let at = |needle: &str| snapshot.language_at(VUE.find(needle).unwrap()).to_string();
    assert_eq!(at("ref<number>"), "typescript");
    assert_eq!(at(".title"), "scss");
    assert_eq!(at("active: isOn"), "typescript", ":class");
    assert_eq!(at("count++"), "typescript", "@click");
    assert_eq!(at("count * 2"), "typescript", "{{ }}");
    assert_eq!(at("<div"), "html");

    const PAGE: &str = "<p>{{ not code here }}</p><button hx-post=\"/save\" hx-on:click=\"alert('saved')\">Save</button>";
    let doc = Document::new(PAGE);
    let snapshot = parse_document(
        &s.syntax,
        &LanguageId::new_static("html"),
        &doc.buffer().snapshot(),
        None,
        &[],
        0,
    )
    .unwrap();
    let at = |needle: &str| snapshot.language_at(PAGE.find(needle).unwrap()).to_string();
    assert_eq!(at("alert("), "javascript");
    assert_eq!(at("/save"), "html", "hx-post's value isn't code");
    assert_eq!(at("not code"), "html", "not a component: {{ }} stays text");
}

/// Every block in the settings (the app's and the modules') has the words a search finds it
/// by in both languages (`settings.keywords_<block>`).
#[test]
fn every_settings_block_has_its_search_words_in_both_languages() {
    let (support, _) = load(&extensions::ModuleSelection::default());
    let locales = Path::new(env!("CARGO_MANIFEST_DIR")).join("../domain/core/i18n/locales");
    for lang in ["ro", "en"] {
        let catalog = std::fs::read_to_string(locales.join(format!("{lang}.toml"))).unwrap();
        for block in &support.settings {
            let key = format!("keywords_{} = ", block.id().replace('-', "_"));
            assert!(catalog.lines().any(|l| l.starts_with(&key)), "{lang}: {key}");
        }
    }
}

/// The languages' and frameworks' modules bring their folders: dependencies, what's not
/// listed, what's generated.
#[test]
fn modules_register_their_project_folders() {
    let _support = support();
    assert!(kernel::paths::is_dependency_dir("vendor"), "Composer's, from PHP");
    assert!(kernel::paths::is_unlisted_dir("node_modules"), "npm's, from JavaScript");
    let skipped = kernel::paths::skipped_from_search();
    assert!(skipped.contains(&"var/cache") && skipped.contains(&"storage"), "the frameworks'");
}

#[test]
fn the_modules_drivers_are_registered_for_the_data_sources() {
    let _support = support();
    let sqlite = database::drivers::find("sqlite").expect("the SQLite module's driver");
    assert_eq!(sqlite.kind, database::Kind::Relational);
    let files = database::drivers::files();
    assert!(files.iter().any(|f| f.driver == "sqlite" && f.has_extension(Path::new("a.db"))));
}

/// A PHP heredoc named `SQL` (or after its database) is SQL: highlighted with SQL's colors,
/// commented with `--`, its grammar's errors never reported (incomplete SQL is normal there).
#[test]
fn php_heredoc_sql_lights_up() {
    let s = support();
    assert_eq!(s.languages.detect(Path::new("schema.sql"), "").to_string(), "sql");
    assert_eq!(s.languages.detect(Path::new("report.pgsql"), "").to_string(), "sql");
    let src = "<?php\n$sql = <<<SQL\n    SELECT id, count(*) FROM users WHERE name = 'ana'\n    SQL;\n\
               $pg = <<<PGSQL\n    DELETE FROM t WHERE\n    PGSQL;\n$n = 1;\n";
    let doc = Document::new(src);
    let text = doc.buffer().snapshot();
    let snapshot =
        parse_document(&s.syntax, &LanguageId::new_static("php"), &text, None, &[], 0).unwrap();
    let at = |needle: &str| src.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert_eq!(snapshot.language_at(at("SELECT")).as_str(), "sql");
    assert_eq!(snapshot.language_at(at("DELETE")).as_str(), "sql");
    assert_eq!(snapshot.language_at(at("$n")).as_str(), "php");
    let mut spans = Vec::new();
    Highlighter::new().highlight(&snapshot, &text, 0..src.len(), &mut spans);
    let scope_at = |needle: &str| {
        let at = at(needle);
        spans
            .iter()
            .rev()
            .find(|sp| sp.range.contains(&at))
            .and_then(|sp| s.syntax.scope(sp.scope))
            .map(|(lang, name)| format!("{name}.{lang}"))
    };
    assert_eq!(scope_at("SELECT").as_deref(), Some("keyword.sql"));
    assert_eq!(scope_at("FROM").as_deref(), Some("keyword.sql"));
    assert_eq!(scope_at("count").as_deref(), Some("function.call.sql"));
    assert_eq!(scope_at("users").as_deref(), Some("type.sql"));
    assert_eq!(scope_at("'ana'").as_deref(), Some("string.sql"));
    // The unfinished `DELETE … WHERE` raises nothing.
    assert_eq!(snapshot.errors(), Vec::new());
    // ⌘/ in the heredoc writes SQL's comment.
    let sql = s.languages.get(snapshot.language_at(at("SELECT"))).unwrap();
    assert_eq!(sql.line_comment, Some("--"));
}

/// Every module's name and description are in the locales (`[module] <id>_name` / `<id>`,
/// the id's `.` and `-` as `_`): the settings' Module section and the Task Manager show
/// them in the interface's language, never the manifest's English.
#[test]
fn every_module_is_named_in_the_locales() {
    let (_, reports) = load(&ModuleSelection::default());
    let mut missing = Vec::new();
    for report in &reports {
        let id = report.id.as_str().replace(['.', '-'], "_");
        for key in [format!("module.{id}_name"), format!("module.{id}")] {
            if i18n::t(&key) == key {
                missing.push(key);
            }
        }
    }
    assert!(missing.is_empty(), "missing from the locales: {missing:?}");
}

/// What the languages and the frameworks tell the `.env` module reaches it: Symfony's Flex
/// blocks, Laravel's key prefixes, the ways code reads a variable, the load orders.
#[test]
fn what_modules_know_of_env_files_is_registered() {
    let _ = load(&ModuleSelection::default());
    let sections = code_intelligence::env::sections();
    assert!(
        sections.iter().any(|s| matches!(
            s,
            code_intelligence::env::EnvSections::Fenced { open: "###> ", .. }
        ))
    );
    assert!(sections.contains(&code_intelligence::env::EnvSections::KeyPrefix));
    // How each language reads a variable, and the frameworks' load orders.
    let reads = code_intelligence::env::reads();
    let prefixes = |language: &str| -> Vec<&str> {
        reads
            .iter()
            .filter(|r| r.languages.contains(&language))
            .flat_map(|r| r.accesses.iter().map(|a| a.prefix))
            .collect()
    };
    for (language, prefix) in [
        ("php", "getenv"),
        ("php", "env"),
        ("yaml", "%env("),
        ("javascript", "process.env"),
        ("bash", "$"),
    ] {
        assert!(prefixes(language).contains(&prefix), "{language}: {prefix}");
    }
    let orders: Vec<&str> = code_intelligence::env::orders().iter().map(|o| o.name).collect();
    assert!(orders.contains(&"Symfony") && orders.contains(&"Laravel"));
}

/// A condition nobody evaluates is dropped silently and its pattern matches everywhere: each
/// one a loaded query uses is known to the engine.
#[test]
fn every_condition_of_a_query_is_evaluated() {
    let (support, _) = load(&ModuleSelection::default());
    let unknown = support.syntax.grammars().unevaluated_conditions();
    assert!(unknown.is_empty(), "{unknown:?}");
}

/// An embedded language starts inside its template literal, not on the backtick.
#[test]
fn a_template_literal_s_backticks_are_the_script_s() {
    let (support, _) = load(&ModuleSelection::default());
    let source = "const a = html`<p>x</p>`;\n";
    let text = document::Document::new(source).buffer().snapshot();
    let language = kernel::LanguageId::new_static("javascript");
    let parsed =
        syntax_ts::parse_document(&support.syntax, &language, &text, None, &[], 0).unwrap();
    let inside = source.find("<p").unwrap();
    assert_eq!(parsed.language_at(inside + 1).as_str(), "html");
    assert_eq!(parsed.language_at(inside - 1).as_str(), "javascript");
}

/// A rule that holds only inside an Envoy block doesn't make every template's text a script.
#[test]
fn a_blade_template_s_text_is_not_a_script() {
    let (support, _) = load(&ModuleSelection::default());
    let source = "<p>\n@fore\nhello\n";
    let text = document::Document::new(source).buffer().snapshot();
    let language = kernel::LanguageId::new_static("blade");
    let parsed =
        syntax_ts::parse_document(&support.syntax, &language, &text, None, &[], 0).unwrap();
    assert_ne!(parsed.language_at(source.find("hello").unwrap()).as_str(), "bash");
}

/// Every module taken out, one at a time: the others all start, the workbench opens a project
/// and a file and takes what its background work finds. In a process of its own: loading
/// modules sets what the whole process knows (the other tests' menus, who answers what).
#[test]
fn the_app_works_without_any_one_module() {
    let exe = std::env::current_exe().unwrap();
    let test = "modules::tests::without_each_module_in_turn";
    let output = std::process::Command::new(exe)
        .args(["--ignored", "--exact", test, "--nocapture", "--test-threads=1"])
        .output()
        .unwrap();
    let told = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{told}\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(told.contains("1 passed"), "it ran: {told}");
}

#[test]
#[ignore = "run by the_app_works_without_any_one_module, in its own process"]
fn without_each_module_in_turn() {
    let (_, all) = load(&ModuleSelection::default());
    let project = std::env::temp_dir().join(format!("without-a-module-{}", std::process::id()));
    std::fs::create_dir_all(&project).unwrap();
    let file = project.join("index.php");
    std::fs::write(&file, "<?php\n\necho 'hello';\n").unwrap();
    assert!(all.len() > 40, "{}", all.len());
    for out in all.iter().map(|r| r.id.as_str().to_owned()) {
        let (support, reports) =
            load(&ModuleSelection { disabled: vec![out.clone()], ..Default::default() });
        for report in &reports {
            let expected =
                if report.id.as_str() == out { ModuleState::Disabled } else { ModuleState::Active };
            assert_eq!(report.state, expected, "{} without {out}", report.id);
        }
        let mut workbench =
            crate::workbench::Workbench::new(Arc::new(support), crate::workbench::Ports::local());
        workbench.open_path(&project).unwrap();
        workbench.open_path(&file).unwrap();
        for _ in 0..3 {
            workbench.poll_syntax();
            workbench.poll_index();
            workbench.poll_symbols();
            workbench.poll_fs();
            workbench.poll_viewers();
            workbench.poll_jobs();
            workbench.poll_diagnostics();
            workbench.poll_frameworks();
        }
        assert_eq!(workbench.tab_count(), 1, "the file opened without {out}");
    }
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn a_template_s_var_comment_leads_to_the_php_class() {
    use code_intelligence::{AssistContext, AssistTarget, Project, SymbolIndex, definition_of};
    let s = support();
    let php = s.intelligence.iter().find(|i| i.language().as_str() == "php").expect("php");
    let mut index = SymbolIndex::default();
    let user = "<?php\nnamespace App\\Entity;\n\nclass User {}\n";
    let user_path = Path::new("/p/src/Entity/User.php");
    index.set_file(user_path, php.extract(user));
    for (language, template) in [
        ("twig", "{# @var user \\App\\Entity\\User #}\n{{ user.name }}"),
        ("blade", "{{-- @var \\App\\Entity\\User $user --}}\n{{ $user->name }}"),
    ] {
        let offset = template.find("User").unwrap() + 2;
        let path = Path::new("/p/templates/show");
        let at = AssistContext {
            project: None,
            root: None,
            path,
            language,
            text: template,
            offset,
            version: 1,
        };
        let target = s
            .assists
            .iter()
            .filter(|a| a.asked_for(language, language))
            .find_map(|a| a.definition(&at));
        let Some(AssistTarget::Code { language: "php", target }) = target else {
            panic!("{language}: {target:?}");
        };
        let found =
            definition_of(php.as_ref(), &index, path, template, offset, &target, &Project::none());
        assert_eq!(found.first().map(|l| l.path.as_path()), Some(user_path), "{language}");
    }
}

/// A doc comment's tags, types and names are colored as their own scopes (PHPDoc, JSDoc,
/// SassDoc), the text as the comment; a plain comment keeps its one color.
#[test]
fn doc_comments_parts_are_highlighted() {
    let s = support();
    let scope_at = |language: &'static str, src: &str, needle: &str| {
        let doc = Document::new(src);
        let text = doc.buffer().snapshot();
        let snapshot =
            parse_document(&s.syntax, &LanguageId::new_static(language), &text, None, &[], 0)
                .unwrap();
        let mut spans = Vec::new();
        Highlighter::new().highlight(&snapshot, &text, 0..src.len(), &mut spans);
        let at = src.find(needle).unwrap() + 1;
        spans
            .iter()
            .rev()
            .find(|sp| sp.range.contains(&at))
            .and_then(|sp| s.syntax.scope(sp.scope))
            .map(|(_, name)| name)
    };
    let php = "<?php\n/**\n * Finds one.\n * @param int|null $id the id\n */\nfunction f($id) {}\n// @param int $x\n";
    assert_eq!(scope_at("php", php, "@param").as_deref(), Some("comment.doc.tag"));
    assert_eq!(scope_at("php", php, "int|null").as_deref(), Some("comment.doc.type"));
    assert_eq!(scope_at("php", php, "$id the").as_deref(), Some("comment.doc.variable"));
    assert_eq!(scope_at("php", php, "Finds").as_deref(), Some("comment"));
    assert_eq!(scope_at("php", php, "@param int $x").as_deref(), Some("comment"), "not a doc one");
    let js = "/**\n * @returns {Promise<User>} it\n */\nfunction f() {}\n";
    assert_eq!(scope_at("javascript", js, "@returns").as_deref(), Some("comment.doc.tag"));
    assert_eq!(scope_at("javascript", js, "Promise").as_deref(), Some("comment.doc.type"));
    let scss = "/// @param {Number} $size the size\n@mixin m($size) {}\n";
    assert_eq!(scope_at("scss", scss, "@param").as_deref(), Some("comment.doc.tag"));
    assert_eq!(scope_at("scss", scss, "$size the").as_deref(), Some("comment.doc.variable"));
}

/// The viewer a file opens in: the first that shows it, in the order the app asks them.
fn opener_of(support: &LanguageSupport, path: &str) -> Option<&'static str> {
    support.viewers.iter().find(|v| v.kind(Path::new(path)).is_some()).map(|v| v.id())
}

/// Shown, our diagram editor opens `.drawio` files, "Edit in draw.io" beside it. Hidden, it
/// still runs (its module is active) but shows nothing of its own and draw.io opens the
/// files; its viewer is asked last (only for what nothing else opens). The Diagrams window
/// and New Diagram (their own module) stay either way.
#[test]
fn a_hidden_module_shows_nothing_and_another_opens_its_files() {
    let editor = "tool.diagram-editor";
    let has_tool = |s: &LanguageSupport, id: &str| s.tools.iter().any(|t| t.id() == id);
    let has_command = |s: &LanguageSupport, id: &str| s.commands.iter().any(|c| c.spec.id.0 == id);
    let (shown, reports) = load(&ModuleSelection::default());
    assert!(reports.iter().any(|r| r.id.as_str() == editor && !r.hidden));
    assert_eq!(opener_of(&shown, "/p/flow.drawio"), Some("drawio"));
    assert!(has_tool(&shown, "diagram-files") && has_command(&shown, "diagram.new"));
    assert!(has_command(&shown, "diagram_web.edit"));
    drop(shown);

    let (hidden, reports) =
        load(&ModuleSelection { hidden: vec![editor.into()], ..Default::default() });
    let report = reports.iter().find(|r| r.id.as_str() == editor).unwrap();
    assert!(report.state == ModuleState::Active && report.hidden, "it runs, hidden");
    assert_eq!(opener_of(&hidden, "/p/flow.drawio"), Some("diagram-web"), "draw.io opens it");
    assert!(has_tool(&hidden, "diagram-files"), "the Diagrams window stays");
    assert!(has_command(&hidden, "diagram.new"), "New Diagram stays");
    assert!(hidden.viewers.iter().any(|v| v.id() == "drawio"), "its viewer stays, asked last");
    assert_eq!(hidden.viewers.last().map(|v| v.id()), Some("drawio"));
    // Nothing else of the others changes.
    assert!(has_tool(&hidden, "terminal") && has_command(&hidden, "diagram_web.edit"));
    assert_eq!(opener_of(&hidden, "/p/logo.png"), Some("image"));
}

/// A hidden module still answers what the others ask (the terminal running an agent's
/// session), while its window's button, its commands and its menu rows are gone.
#[test]
fn a_hidden_module_still_answers_requests() {
    let (support, _) =
        load(&ModuleSelection { hidden: vec!["terminal".into()], ..Default::default() });
    assert!(support.answers.has::<contracts::terminal::RunInTerminal>());
    assert!(support.answers.has::<contracts::terminal::RenameTerminal>());
    assert!(!support.tools.iter().any(|t| t.id() == "terminal"));
    assert!(!support.commands.iter().any(|c| c.spec.id == terminal_panel::NEW));
    let (shown, _) = load(&ModuleSelection::default());
    assert!(shown.tools.iter().any(|t| t.id() == "terminal"));
    assert!(shown.commands.iter().any(|c| c.spec.id == terminal_panel::NEW));
}
