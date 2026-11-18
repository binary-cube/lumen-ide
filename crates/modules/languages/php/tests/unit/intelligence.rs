use super::*;

const SRC: &str = r#"<?php
namespace App\Http;

use App\Models\User;
use Foo\{Bar, Baz as Q};

abstract class Base extends Model implements Shape, \JsonSerializable {
    use HasName;
    public const MAX = 10;
    protected static int $count = 0;
    private ?User $user = null;
    public function __construct(private string $title) {}
    public static function make(int $a, string ...$rest): static { return new static(); }
}

if (!function_exists('helper')) {
    function helper($x) { return $x; }
}
"#;

#[test]
fn extracts_declarations_with_names_resolved() {
    let file = PhpIntelligence::default().extract(SRC);
    assert_eq!(file.namespace.as_deref(), Some("App\\Http"));
    let uses: Vec<(&str, &str)> = file.uses.iter().map(|u| (&*u.alias, &*u.fqn)).collect();
    assert_eq!(uses, [("User", "App\\Models\\User"), ("Bar", "Foo\\Bar"), ("Q", "Foo\\Baz")]);
    let find = |name: &str| {
        file.symbols.iter().find(|s| s.name == name.into()).unwrap_or_else(|| panic!("{name}"))
    };
    let base = find("Base");
    assert_eq!(base.fqn, ("App\\Http\\Base").into());
    let parents: Vec<&str> = base.parents.iter().map(|p| &**p).collect();
    assert_eq!(
        parents,
        ["App\\Http\\Model", "App\\Http\\Shape", "JsonSerializable", "App\\Http\\HasName"]
    );
    assert_eq!(find("make").detail.as_deref(), Some("(int $a, string ...$rest): static"));
    assert!(find("make").is_static && find("count").is_static);
    assert_eq!(find("title").kind, SymbolKind::Property, "a promoted property");
    assert_eq!(find("MAX").kind, SymbolKind::ClassConstant);
    assert_eq!(find("helper").fqn, ("App\\Http\\helper").into());
}

fn ctx(src: &str) -> CompletionContext {
    let offset = src.find('|').unwrap();
    context(&src.replace('|', ""), offset)
}

#[test]
fn completion_contexts() {
    assert_eq!(ctx("<?php $this->na|").kind, ContextKind::Member(Receiver::This));
    assert_eq!(
        ctx("<?php $u = new User(); $u->|").kind,
        ContextKind::Member(Receiver::Class("User".into()))
    );
    assert_eq!(
        ctx("<?php function f(Request $r) { $r->in|").kind,
        ContextKind::Member(Receiver::Class("Request".into()))
    );
    // `@var`, the last one written winning; a longer name alike isn't this variable.
    assert_eq!(
        ctx("<?php /** @var Mailer $m */ $m = new Old(); /** @var \\App\\Mail $m */ $m->|").kind,
        ContextKind::Member(Receiver::Class("\\App\\Mail".into()))
    );
    assert_eq!(
        ctx("<?php $item = new Item(); $items = new Bag(); $item->|").kind,
        ContextKind::Member(Receiver::Class("Item".into()))
    );
    assert_eq!(
        ctx("<?php function f(int $n, ?Clock ...$c) { $c->|").kind,
        ContextKind::Member(Receiver::Class("Clock".into()))
    );
    assert_eq!(ctx("<?php User::fi|").kind, ContextKind::Static("User".into()));
    assert_eq!(ctx("<?php echo $us|").kind, ContextKind::Variable);
    assert_eq!(ctx("<?php echo $us|").replace, 11..14);
    assert_eq!(ctx("<?php Arr|").kind, ContextKind::Name);
    assert_eq!(ctx("<?php $x = new Us|").kind, ContextKind::TypeName);
    assert_eq!(ctx("<?php class A extends Mo|").kind, ContextKind::TypeName);
    assert_eq!(ctx("<?php // Arr|").kind, ContextKind::None);
    assert_eq!(ctx("<?php echo 'Arr|").kind, ContextKind::None);
    // A method being named in a class: the magic ones; a function's name: nothing.
    let method = ContextKind::Vocabulary(METHOD_NAME);
    assert_eq!(ctx("<?php class A { public function __con|").kind, method);
    assert_eq!(ctx("<?php class A { function f() { if (1) {} }\n function __|").kind, method);
    assert_eq!(ctx("<?php final class A extends B implements C {\n function _|").kind, method);
    assert_eq!(ctx("<?php function __con|").kind, ContextKind::None);
    assert_eq!(ctx("<?php class A { function f() { $g = function _|").kind, ContextKind::None);
}

/// After `function` in a class: `__construct` and the other magic methods, `()` added.
#[test]
fn a_class_offers_the_magic_methods() {
    let php = PhpIntelligence::default();
    let context = ctx("<?php class A { public function __con|");
    let words = php.vocabulary(&context, "<?php class A { public function __con");
    let construct = words.iter().find(|w| w.label == "__construct").expect("__construct");
    assert_eq!((construct.insert.as_str(), construct.cursor_back), ("__construct()", 1));
    assert!(words.iter().any(|w| w.label == "__toString"));
}

/// An attribute's name is a class made through its constructor (`#[AsCommand(…)]`, the
/// second of a list, a qualified one, on several lines) — not a function called; its
/// arguments' parentheses, an array or a call around a `,` are not an attribute list.
#[test]
fn attributes_name_classes() {
    let class = |name: &str| Some(Target::Class(name.into()));
    assert_eq!(tgt("<?php\n#[AsCo|mmand(\n    name: 'app:x',\n)]\nclass A {}"), class("AsCommand"));
    assert_eq!(tgt("<?php\n#[AsCo|mmand]\nclass A {}"), class("AsCommand"));
    assert_eq!(tgt("<?php\n#[Final, AsCo|mmand('x')]\nclass A {}"), class("AsCommand"));
    assert_eq!(tgt("<?php\n#[ORM\\Col|umn(type: 'string')]"), class("ORM\\Column"));
    assert_eq!(
        tgt("<?php\n#[A(['a', 'b']), B(c: 1)]\n#[Route('/x', methods: ['GET'])]\nf(1, ma|x(2));"),
        Some(Target::Function("max".into()))
    );
    assert_eq!(tgt("<?php $x = [1, ma|x(2)];"), Some(Target::Function("max".into())));
    // Completion after `#[`: classes only.
    assert_eq!(ctx("<?php\n#[AsC|").kind, ContextKind::TypeName);
    assert_eq!(ctx("<?php\n#[A(1), ORM\\Col|").kind, ContextKind::TypeName);
    assert_eq!(ctx("<?php\nf(1, ma|").kind, ContextKind::Name);
    // The constructor runs for an attribute and for `new`.
    let php = PhpIntelligence::default();
    let text = "<?php\n#[ORM\\Column(type: 'string')]\n$x = new \\App\\User(1);";
    let at = |word: &str| text.find(word).unwrap();
    assert!(php.constructs_at(text, at("Column")));
    assert!(php.constructs_at(text, at("User")));
    assert!(!php.constructs_at(text, at("type")));
}

fn tgt(src: &str) -> Option<Target> {
    let offset = src.find('|').unwrap();
    target(&src.replace('|', ""), offset)
}

#[test]
fn import_goes_after_existing_uses() {
    let intel = PhpIntelligence::default();
    let src = "<?php\nnamespace App;\n\nuse A\\B;\n\nclass C {\n    use T;\n}\n";
    let (at, text) = intel.import_edit(src, "App\\Models\\User").unwrap();
    assert_eq!(&src[..at], "<?php\nnamespace App;\n\nuse A\\B;");
    assert_eq!(text, "\nuse App\\Models\\User;");
    let (at, _) = intel.import_edit("<?php\nnamespace App;\nclass C {}", "X\\Y").unwrap();
    assert_eq!(at, "<?php\nnamespace App;".len());
}

#[test]
fn includes_paths_and_psr4() {
    let intel = PhpIntelligence::default();
    let src = "<?php\ninclude __DIR__ . '/config/db.php';\nrequire_once('lib/helpers.php');\n";
    assert_eq!(intel.includes(src), ["/config/db.php", "lib/helpers.php"]);
    let typed = "<?php include __DIR__ . '/config/d";
    let c = context(typed, typed.len());
    assert_eq!((c.kind, c.prefix.as_str()), (ContextKind::Path("/config/".into()), "d"));
    let src = "<?php include 'lib/hel|pers.php';";
    let at = src.find('|').unwrap();
    assert_eq!(target(&src.replace('|', ""), at), Some(Target::File("lib/helpers.php".into())));

    let composer =
        r#"{"autoload": {"psr-4": {"App\\": "app/", "App\\Domain\\": ["src/Domain/"]}}}"#;
    let read = |p: &std::path::Path| p.ends_with("composer.json").then(|| composer.to_owned());
    let project = Project {
        root: Some(std::path::Path::new("/p")),
        read: &read,
        list: &|_| Vec::new(),
        modified: &|_| None,
        levels: &[],
        languages: &[],
    };
    assert_eq!(
        intel.locate(&project, "App\\Models\\User"),
        [std::path::PathBuf::from("/p/app/Models/User.php")]
    );
    assert_eq!(
        intel.locate(&project, "App\\Domain\\Order")[0],
        std::path::PathBuf::from("/p/src/Domain/Order.php")
    );
}

#[test]
fn definition_targets() {
    assert_eq!(tgt("<?php new Us|er();"), Some(Target::Class("User".into())));
    assert_eq!(
        tgt("<?php $this->sa|ve();"),
        Some(Target::Member { receiver: Receiver::This, name: "save".into() })
    );
    assert_eq!(
        tgt("<?php User::fi|nd(1);"),
        Some(Target::Static { class: "User".into(), name: "find".into() })
    );
    assert_eq!(tgt("<?php array_m|ap($f, $a);"), Some(Target::Function("array_map".into())));
    assert_eq!(
        tgt("<?php\nuse App\\Models\\Us|er;"),
        Some(Target::Class("\\App\\Models\\User".into()))
    );
    // A trait used in a class's body: as the file resolves it, not a full name.
    assert_eq!(
        tgt("<?php\nclass A\n{\n    use HasTi|mestamps;\n}"),
        Some(Target::Class("HasTimestamps".into()))
    );
    assert_eq!(tgt("<?php echo $va|r;"), None);
}

#[test]
fn a_method_moves_whole_with_its_docblock_and_a_statement_stays_in_its_function() {
    use code_intelligence::StatementMove;
    let text = "<?php\nclass A\n{\n    /** One. */\n    public function one()\n    {\n        $a = 1;\n        $b = 2;\n    }\n\n    public function two() {}\n}\n";
    let php = PhpIntelligence::default();
    // On `two`: above `one` (with its docblock).
    assert_eq!(
        php.statement_move(text, 10..11, true),
        StatementMove::Past { unit: 10..11, past: 3..9 }
    );
    // On `one`'s name line: the whole method (docblock included) goes down.
    assert_eq!(
        php.statement_move(text, 4..5, false),
        StatementMove::Past { unit: 3..9, past: 10..11 }
    );
    // `$b = 2;` goes above `$a = 1;`, not out of the function.
    assert_eq!(
        php.statement_move(text, 7..8, true),
        StatementMove::Past { unit: 7..8, past: 6..7 }
    );
    assert_eq!(php.statement_move(text, 7..8, false), StatementMove::Stay);
    // `class A` is the first after `<?php`: it stays.
    assert_eq!(php.statement_move(text, 1..2, true), StatementMove::Stay);
}

/// PHP's naming for a rename: `$` variables, `$this`, `__construct` in any case, PSR-4.
#[test]
fn naming_rules() {
    let naming = PhpIntelligence::default().naming();
    assert_eq!((naming.sigil, naming.receiver), (Some('$'), Some("$this")));
    assert!(naming.implicit_locals);
    assert!(naming.is_constructor("__construct") && naming.is_constructor("__CONSTRUCT"));
    assert!(!naming.is_constructor("construct"));
    assert_eq!(naming.type_per_file, Some("php"));
}

#[test]
fn parameters_split_before_the_variable() {
    assert_eq!(split_param("?Device $device = null"), (Some("?Device".into()), "$device".into()));
    assert_eq!(split_param("int ...$rest"), (Some("int".into()), "...$rest".into()));
    assert_eq!(split_param("array &$out"), (Some("array".into()), "$out".into()));
    assert_eq!(split_param("$x"), (None, "$x".into()));
}

#[test]
fn signatures_carry_the_modifiers() {
    let php = PhpIntelligence::default();
    let file = php.extract(SRC);
    let find = |name: &str| file.symbols.iter().find(|s| s.name == name.into()).unwrap();
    assert_eq!(php.signature(find("count"), Some(": int")), "protected static $count: int");
    assert_eq!(php.signature(find("Base"), None), "class Base");
}

#[test]
fn places_fit_the_symbol_kind() {
    let text = "$this->name; $name; self::$name; $x->save(); save(); A::LIMIT; new User;";
    let range = |needle: &str| {
        let start = text.find(needle).unwrap();
        start..start + needle.len()
    };
    let name = |nth: usize| {
        let start = text.match_indices("name").nth(nth).unwrap().0;
        start..start + 4
    };
    assert!(fits_kind(text, &name(0), SymbolKind::Property), "$this->name");
    assert!(!fits_kind(text, &name(1), SymbolKind::Property), "a $name variable");
    assert!(fits_kind(text, &name(2), SymbolKind::Property), "self::$name");
    let method = text.find("save").unwrap();
    assert!(fits_kind(text, &(method..method + 4), SymbolKind::Method));
    // A method and a property by the same name: called, the method; not, the property.
    assert!(!fits_kind(text, &(method..method + 4), SymbolKind::Property));
    assert!(!fits_kind(text, &name(0), SymbolKind::Method), "->name is no call");
    let callable = "$f = $shop->cart(...); $c = $this->cart;";
    let at = callable.find("cart").unwrap();
    assert!(fits_kind(callable, &(at..at + 4), SymbolKind::Method), "a first-class callable");
    let at = callable.rfind("cart").unwrap();
    assert!(fits_kind(callable, &(at..at + 4), SymbolKind::Property));
    assert!(!fits_kind(callable, &(at..at + 4), SymbolKind::Method));
    let function = text.rfind("save").unwrap();
    assert!(!fits_kind(text, &(function..function + 4), SymbolKind::Method));
    assert!(fits_kind(text, &(function..function + 4), SymbolKind::Function));
    assert!(fits_kind(text, &range("LIMIT"), SymbolKind::ClassConstant));
    assert!(fits_kind(text, &range("User"), SymbolKind::Class));
}

/// A declaration's attributes, short, as its note (the outline shows it): each one's name
/// and its first argument when it's a string; none without attributes.
#[test]
fn attributes_become_the_declaration_s_note() {
    let src = "<?php\nnamespace App;\n\n#[AsCommand(\n    name: 'app:auth:flush',\n    description: 'Sign out',\n)]\n#[\\Final]\nfinal class Flush {\n    #[Route(\"/blog\", name: 'blog')]\n    public function index() {}\n    public function plain() {}\n}\n";
    let file = PhpIntelligence::default().extract(src);
    let note = |name: &str| {
        file.symbols.iter().find(|s| s.name == name.into()).and_then(|s| s.note.clone())
    };
    assert_eq!(note("Flush").as_deref(), Some("AsCommand('app:auth:flush') Final"));
    assert_eq!(note("index").as_deref(), Some("Route(\"/blog\")"));
    assert_eq!(note("plain"), None);
}

/// Everything asked of a document at once (one parse) is what each question asks alone.
#[test]
fn one_analysis_is_each_one_alone() {
    let intel = PhpIntelligence::default();
    let text = "<?php\nnamespace App;\nuse Foo\\Unused;\nclass A {\n    private function gone() {}\n    public function m(): ?int { return mb_strlen('x') ?? match(1) { 1 => 2 }; }\n}\n";
    let all = intel.analyse(text, Some("7.4"));
    assert_eq!(all.symbols, intel.extract(text));
    assert_eq!(all.issues, intel.level_issues(text, "7.4"));
    assert!(!all.issues.is_empty(), "`match` is newer than 7.4");
    assert_eq!(all.unused, intel.unused(text));
    assert!(!all.unused.is_empty());
    assert_eq!(all.requirements, intel.requirements(text));
    assert!(intel.analyse(text, None).issues.is_empty(), "no level: nothing checked");
}

/// A variable's type as written for it: `new`, a parameter's hint, `@var` — the cursor on
/// its name or on its `$`, its own declaration included.
#[test]
fn the_type_written_for_a_variable() {
    let php = PhpIntelligence::default();
    let src = "<?php\nfunction f(Cart $cart) {\n    $shop = new Shop();\n    /** @var Item $item */\n    return [$cart, $shop, $item, $this, $n];\n}\n";
    let at = |needle: &str| src.rfind(needle).unwrap();
    assert_eq!(php.type_at(src, at("$cart") + 2).as_deref(), Some("Cart"));
    assert_eq!(php.type_at(src, at("$shop")).as_deref(), Some("Shop"), "on its `$`");
    assert_eq!(php.type_at(src, at("$item") + 1).as_deref(), Some("Item"));
    assert_eq!(
        php.type_at(src, src.find("$cart)").unwrap() + 1).as_deref(),
        Some("Cart"),
        "its parameter"
    );
    assert_eq!(php.type_at(src, at("$this") + 1), None);
    assert_eq!(php.type_at(src, at("$n") + 1), None);
    assert_eq!(php.type_at(src, at("return")), None, "not a variable");
}

#[test]
fn doc_comments_give_the_index_their_types_and_magic() {
    let src = r#"<?php
namespace App;

/**
 * @template TValue
 * @extends Base<int, TValue>
 * @mixin \App\Builder
 * @property string $nickname
 * @method static self make(int $a)
 */
class Box extends Base
{
    /** @var list<User> */
    private array $users = [];

    /** @return Collection<int, User> */
    public function all(): Collection {}

    /** @return string */
    public function name(): string {}

    /** @deprecated use all() */
    public function old() {}

    #[\Deprecated]
    public function older() {}

    /** @param array{user: User} $row */
    public function __construct(public array $row, int $plain) {}
}

enum Suit: string { case Hearts = 'h'; }
"#;
    let file = PhpIntelligence::default().extract(src);
    let find = |name: &str| {
        file.symbols.iter().find(|s| s.name == name.into()).unwrap_or_else(|| panic!("{name}"))
    };
    let facts = |name: &str| crate::phpdoc::Facts::decode(find(name).types.as_deref());
    let class = facts("Box");
    assert_eq!(class.templates, [("TValue".to_owned(), None)]);
    assert_eq!(class.extends, ["Base<int, TValue>"]);
    assert_eq!(class.mixins, ["\\App\\Builder"]);
    assert_eq!(facts("users").var.as_deref(), Some("list<User>"));
    assert_eq!(facts("all").returns.as_deref(), Some("Collection<int, User>"));
    assert_eq!(find("name").types, None, "nothing the code doesn't say");
    assert!(facts("old").deprecated && facts("older").deprecated);
    assert_eq!(
        facts("row").var.as_deref(),
        Some("array{user: User}"),
        "a promoted parameter's @param"
    );
    assert_eq!(facts("Suit").backed.as_deref(), Some("string"));
    // The magic members: the class's, at their tags, its range taking its doc comment in.
    let nickname = find("nickname");
    assert_eq!((nickname.kind, nickname.detail.as_deref()), (SymbolKind::Property, Some("string")));
    assert_eq!(nickname.container.as_deref(), Some("App\\Box"));
    assert!(src[nickname.offset..].starts_with("$nickname"));
    assert_eq!(nickname.line, 7);
    let make = find("make");
    assert!(make.is_static && make.kind == SymbolKind::Method);
    assert_eq!(make.detail.as_deref(), Some("(int $a): self"));
    assert_eq!(make.note.as_deref(), Some("@method"));
    let class = find("Box");
    assert!(class.range.start < nickname.offset && class.range.contains(&make.offset));
    assert!(PhpIntelligence::default().is_deprecated(find("older")));
}

#[test]
fn imported_functions_and_constants_are_their_kind() {
    assert_eq!(
        tgt("<?php\nuse function App\\Support\\format_na|me;"),
        Some(Target::Function("\\App\\Support\\format_name".into()))
    );
    assert_eq!(
        tgt("<?php\nuse const App\\LIM|IT;"),
        Some(Target::Named { name: "LIMIT".into(), kinds: vec![SymbolKind::Constant] })
    );
    let text = "<?php\nuse function App\\Support\\format_name;\nuse const App\\LIMIT;\n";
    let range = |needle: &str| {
        let start = text.find(needle).unwrap();
        start..start + needle.len()
    };
    assert!(fits_kind(text, &range("format_name"), SymbolKind::Function));
    assert!(!fits_kind(text, &range("format_name"), SymbolKind::Class));
    assert!(fits_kind(text, &range("LIMIT"), SymbolKind::Constant));
    // A method being declared: itself, as its class's member.
    assert_eq!(
        tgt("<?php class A { abstract public function descr|ibe(): string; }"),
        Some(Target::Member { receiver: Receiver::This, name: "describe".into() })
    );
    assert_eq!(tgt("<?php function hel|per() {}"), Some(Target::Function("helper".into())));
}

#[test]
fn strings_and_comments_are_not_uses() {
    let text = "<?php\n// User here\n/* User there */\n/** @param User $u */\n$a = 'Test User'; $b = \"Hi {$user->name} User\"; new User;\n";
    let at = |nth: usize| {
        let start = text.match_indices("User").nth(nth).unwrap().0;
        start..start + 4
    };
    assert!(!fits_kind(text, &at(0), SymbolKind::Class), "a line comment");
    assert!(!fits_kind(text, &at(1), SymbolKind::Class), "a block comment");
    assert!(fits_kind(text, &at(2), SymbolKind::Class), "a doc comment's type");
    assert!(!fits_kind(text, &at(3), SymbolKind::Class), "a string");
    assert!(!fits_kind(text, &at(4), SymbolKind::Class), "an interpolated string's text");
    assert!(fits_kind(text, &at(5), SymbolKind::Class));
    let name = text.find("name}").unwrap();
    assert!(
        fits_kind(text, &(name..name + 4), SymbolKind::Property),
        "what a string interpolates is code"
    );
}

#[test]
fn namespaces_are_offered_after_a_separator() {
    let php = PhpIntelligence::default();
    let mut index = code_intelligence::SymbolIndex::default();
    for (path, src) in [
        ("/p/User.php", "<?php\nnamespace App\\Model;\nclass User {}\n"),
        ("/p/Repo.php", "<?php\nnamespace App\\Repo;\nclass Users {}\n"),
        ("/p/Mod.php", "<?php\nnamespace App\\Modules\\Billing;\nclass Invoice {}\n"),
    ] {
        let mut symbols = php.extract(src);
        symbols.language = "php".into();
        index.set_file(std::path::Path::new(path), symbols);
    }
    let complete = |src: &str| {
        let offset = src.find('|').unwrap();
        let text = src.replace('|', "");
        let (_, items) = code_intelligence::complete(
            &php,
            &index,
            std::path::Path::new("/p/t.php"),
            &text,
            offset,
            &Project::none(),
        );
        items
            .into_iter()
            .filter(|i| i.kind == code_intelligence::CompletionKind::Namespace)
            .map(|i| (i.label, i.insert))
            .collect::<Vec<_>>()
    };
    let found = complete("<?php\nuse App\\Mo|");
    assert_eq!(found, [("Model".into(), "Model\\".into()), ("Modules".into(), "Modules\\".into())]);
    assert_eq!(complete("<?php\nuse App\\|").len(), 3, "all of them before a name is typed");
    assert!(complete("<?php\nnew \\App\\Modules\\B|").iter().any(|(l, _)| l == "Billing"));
    assert!(complete("<?php\n$x = Mo|").is_empty(), "no namespace before it");
}

/// A word of a comment's text leads nowhere (plan 75: „runtime” isn't `Runtime`); the type
/// after a tag does, and its card shows.
#[test]
fn only_what_a_comment_writes_as_code_leads_somewhere() {
    use code_intelligence::Project;
    let runtime = "<?php\nnamespace PHPUnit\\Event;\nfinal class Runtime { public function run(): void {} }\n";
    let text = "<?php\nnamespace App;\n\nuse PHPUnit\\Event\\Runtime;\n\n/**\n * Starts the runtime.\n *\n * @param Runtime $runtime the one to start\n */\nfunction start(Runtime $runtime): void {}\n";
    let php = PhpIntelligence::default();
    let mut index = code_intelligence::SymbolIndex::default();
    for (path, src) in [("/v/Runtime.php", runtime), ("/p/start.php", text)] {
        let mut symbols = php.extract(src);
        symbols.language = "php".into();
        index.set_file(std::path::Path::new(path), symbols);
    }
    let path = std::path::Path::new("/p/start.php");
    let ask = |at: usize| {
        let found = code_intelligence::definition(&php, &index, path, text, at, &Project::none());
        let hover = code_intelligence::hover(&php, &index, path, text, at, &Project::none());
        (found.first().map(|l| (l.path.display().to_string(), l.offset)), hover.map(|h| h.code))
    };
    let word = text.find("runtime.").unwrap() + 2;
    assert_eq!(ask(word), (None, None), "a word of the text");
    let described = text.find("start\n").map(|_| text.find("the one").unwrap() + 2).unwrap();
    assert_eq!(ask(described), (None, None), "the description");
    let ty = text.find("@param Runtime").unwrap() + "@param R".len();
    let (found, card) = ask(ty);
    assert_eq!(found.map(|f| f.0).as_deref(), Some("/v/Runtime.php"), "the type");
    assert!(card.is_some_and(|c| c.contains("Runtime")), "its card");
    let parameter = text.find("$runtime the").unwrap() + 3;
    let declared = text.find("Runtime $runtime)").unwrap() + "Runtime ".len();
    assert_eq!(ask(parameter).0, Some(("/p/start.php".to_owned(), declared)), "the parameter");
    // In code, as before.
    let code = text.find("(Runtime").unwrap() + 3;
    assert_eq!(ask(code).0.map(|f| f.0).as_deref(), Some("/v/Runtime.php"));
}

/// `@inheritDoc` leads to the parent's method, `static` in a method's comment to its class
/// (plan 75b).
#[test]
fn a_comment_s_own_class_words_lead_to_it() {
    use code_intelligence::Project;
    let text = "<?php\nnamespace App;\n\nclass Base\n{\n    public function save(): void {}\n}\n\nclass Box extends Base\n{\n    /** @inheritDoc */\n    public function save(): void {}\n\n    /** @return static */\n    public function make(): static { return $this; }\n}\n";
    let php = PhpIntelligence::default();
    let mut index = code_intelligence::SymbolIndex::default();
    let mut symbols = php.extract(text);
    symbols.language = "php".into();
    let path = std::path::Path::new("/p/box.php");
    index.set_file(path, symbols);
    let line = |at: usize| {
        let found = code_intelligence::definition(&php, &index, path, text, at, &Project::none());
        found.first().map(|l| l.line)
    };
    assert_eq!(line(text.find("inheritDoc").unwrap() + 3), Some(5), "Base::save");
    assert_eq!(line(text.find("static */").unwrap() + 2), Some(8), "Box");
}
