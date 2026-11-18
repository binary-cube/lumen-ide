use std::path::Path;

use code_intelligence::{HoverInfo, LanguageIntelligence, Project, SymbolIndex};

use super::*;
use crate::PhpIntelligence;

/// Where the cursor is in a snippet (not a character PHP uses).
const CURSOR: char = '§';

const USER: &str = r#"<?php
namespace App\Model;

use App\Support\Entity;
use App\Support\HasTimestamps;

/**
 * @property string $nickname
 * @method bool isAdmin()
 */
class User extends Entity
{
    use HasTimestamps;

    public string $email = '';

    public function __construct(
        public readonly string $first,
        public Status $status = Status::Active,
        public ?Address $address = null,
    ) {}

    public function describe(): string { return $this->first; }

    private function secret(): string { return ''; }
}
"#;

const ADDRESS: &str = r#"<?php
namespace App\Model;

final class Address
{
    public function __construct(public string $street, public string $city) {}

    public function withCity(string $city): static { return new static($this->street, $city); }
}
"#;

const STATUS: &str = r#"<?php
namespace App\Model;

enum Status: string
{
    case Active = 'active';
    case Banned = 'banned';

    public function label(): string { return ucfirst($this->value); }

    public static function fromLabel(string $label): self { return self::from($label); }
}
"#;

const ENTITY: &str = r#"<?php
namespace App\Support;

abstract class Entity
{
    public static function create(mixed ...$args): static { return new static(...$args); }

    public function getId(): int { return 0; }
}
"#;

const TIMESTAMPS: &str = r#"<?php
namespace App\Support;

trait HasTimestamps
{
    public function touch(): static { return $this; }

    public function touchedAt(): ?\DateTimeImmutable { return null; }
}
"#;

const COLLECTION: &str = r#"<?php
namespace App\Repo;

/**
 * @template TKey of array-key
 * @template TValue
 * @implements \IteratorAggregate<TKey, TValue>
 */
class Collection implements \IteratorAggregate
{
    /** @return TValue|null */
    public function first(): mixed { return null; }

    /**
     * @template TMapped
     * @param callable(TValue): TMapped $callback
     * @return Collection<TKey, TMapped>
     */
    public function map(callable $callback): self { return $this; }

    /**
     * @param callable(TValue, TKey): bool $keep
     * @return static
     */
    public function filter(callable $keep) { return $this; }

    /** @return \ArrayIterator<TKey, TValue> */
    public function getIterator(): \ArrayIterator { return new \ArrayIterator([]); }
}
"#;

const REPOSITORY: &str = r#"<?php
namespace App\Repo;

use App\Model\User;

class UserRepository
{
    /** @return Collection<int, User> */
    public function all(): Collection { return new Collection(); }

    public function find(int $id): ?User { return null; }

    /**
     * @return array
     * @psalm-return list<User>
     */
    public function active(): array { return []; }

    /**
     * @template T
     * @param class-string<T> $class
     * @return T
     */
    public function make(string $class): object { return new $class(); }
}

/** @extends Collection<int, User> */
class Users extends Collection {}
"#;

const SERVICE_HEAD: &str = r#"<?php
namespace App\Service;

use App\Model\Address;
use App\Model\Status;
use App\Model\User;
use App\Repo\UserRepository;
use App\Repo\Users;

final class UserService
{
    public function __construct(private UserRepository $repo, private Users $users) {}

"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("/p/src/Model/User.php", USER),
        ("/p/src/Model/Address.php", ADDRESS),
        ("/p/src/Model/Status.php", STATUS),
        ("/p/src/Support/Entity.php", ENTITY),
        ("/p/src/Support/HasTimestamps.php", TIMESTAMPS),
        ("/p/src/Repo/Collection.php", COLLECTION),
        ("/p/src/Repo/UserRepository.php", REPOSITORY),
    ]
}

const PATH: &str = "/p/src/Service/UserService.php";

fn index_of(files: &[(&str, &str)]) -> SymbolIndex {
    let php = PhpIntelligence::default();
    let mut index = SymbolIndex::default();
    for (path, text) in files {
        let mut symbols = php.extract(text);
        symbols.language = "php".into();
        index.set_file(Path::new(path), symbols);
    }
    index
}

/// A method body of the service (`body`, with the cursor), the whole file, where the cursor
/// is, and the index with it.
fn service(body: &str) -> (String, usize, SymbolIndex) {
    let src = format!(
        "{SERVICE_HEAD}    public function probe(User $user, array $row): mixed\n    {{\n{body}\n    }}\n}}\n"
    );
    let offset = src.find(CURSOR).unwrap_or(0);
    let text = src.replacen(CURSOR, "", 1);
    let mut all = files();
    all.push((PATH, &text));
    let index = index_of(&all);
    (text, offset, index)
}

/// The labels completion offers at the cursor of `body`.
fn complete(body: &str) -> Vec<String> {
    let (text, offset, index) = service(body);
    let php = PhpIntelligence::default();
    let (_, items) =
        code_intelligence::complete(&php, &index, Path::new(PATH), &text, offset, &Project::none());
    items.into_iter().map(|i| i.label).collect()
}

/// The type of the receiver before the `->` at the cursor.
fn receiver(body: &str) -> String {
    let (text, offset, index) = service(body);
    api::receiver_at(&text, offset, &index).map(|t| t.show()).unwrap_or_default()
}

fn has(labels: &[String], wanted: &[&str]) -> bool {
    wanted.iter().all(|w| labels.iter().any(|l| l == w))
}

#[test]
fn properties_returns_and_chains() {
    assert_eq!(receiver("        $this->repo->§"), "UserRepository", "a promoted property");
    assert_eq!(receiver("        $this->repo->all()->§"), "Collection<int, User>");
    assert_eq!(receiver("        $u = $this->repo->find(1);\n        $u->§"), "User", "?User");
    assert_eq!(receiver("        $user->address?->§"), "Address", "nullsafe");
    assert_eq!(receiver("        $user->status->§"), "Status", "an enum-typed property");
    assert_eq!(
        receiver("        $x = (clone $user)->touch();\n        $x->§"),
        "User",
        "static from a trait"
    );
}

#[test]
fn generics_through_templates() {
    assert_eq!(
        receiver("        $first = $this->repo->all()->first();\n        $first?->§"),
        "User"
    );
    assert_eq!(
        receiver("        $this->users->first()->§"),
        "User",
        "@extends Collection<int, User>"
    );
    assert_eq!(
        receiver("        $this->repo->all()->filter(fn ($u) => true)->first()->§"),
        "User",
        "static keeps the arguments"
    );
    assert_eq!(
        receiver(
            "        $this->repo->all()->map(fn (User $u): Address => $u->address)->first()->§"
        ),
        "Address",
        "a method template from the callable's return type"
    );
    assert_eq!(
        receiver("        $this->repo->make(Address::class)->§"),
        "Address",
        "class-string<T>"
    );
    // An untyped arrow function's parameter: what the callable expects.
    assert_eq!(receiver("        $this->repo->all()->filter(fn ($u) => $u->§)"), "User");
}

#[test]
fn foreach_lists_and_shapes() {
    assert_eq!(
        receiver(
            "        foreach ($this->repo->active() as $member) {\n            $member->§\n        }"
        ),
        "User"
    );
    assert_eq!(
        receiver(
            "        foreach ($this->repo->all() as $i => $member) {\n            $member->§\n        }"
        ),
        "User",
        "an IteratorAggregate's"
    );
    let shape = "    /** @param array{id: int, address: Address} $row */\n";
    let (text, offset, index) = {
        let src = format!(
            "{SERVICE_HEAD}{shape}    public function shapes(array $row): string\n    {{\n        return $row['address']->§;\n    }}\n}}\n"
        );
        let offset = src.find(CURSOR).unwrap();
        let text = src.replacen(CURSOR, "", 1);
        let mut all = files();
        all.push((PATH, &text));
        let index = index_of(&all);
        (text, offset, index)
    };
    let ty = api::receiver_at(&text, offset, &index).map(|t| t.show());
    assert_eq!(ty.as_deref(), Some("Address"), "a shape's field");
    assert_eq!(
        receiver("        [$a, $b] = [$user, new Address('a', 'b')];\n        $a->§"),
        "User",
        "destructured"
    );
}

#[test]
fn scopes_and_narrowing() {
    assert_eq!(
        receiver("        $x = $user;\n        $f = function () use ($x) { return $x->§; };"),
        "User",
        "a closure's use"
    );
    assert_eq!(
        receiver("        $x = $user;\n        $f = function () { return $x->§; };"),
        "",
        "not used: not seen"
    );
    assert_eq!(
        receiver("        $x = $user;\n        $f = fn () => $x->§;"),
        "User",
        "an arrow function sees its scope"
    );
    assert_eq!(
        receiver(
            "        $o = $row[0];\n        if ($o instanceof Address) {\n            $o->§\n        }"
        ),
        "Address"
    );
    assert_eq!(
        receiver("        $o = $row[0];\n        if ($o instanceof Address && $o->§)"),
        "Address"
    );
    assert_eq!(
        receiver("        $o = $row[0];\n        assert($o instanceof Address);\n        $o->§"),
        "Address"
    );
    assert_eq!(
        receiver(
            "        $o = $row[0];\n        if (!$o instanceof Address) {\n            return null;\n        }\n        $o->§"
        ),
        "Address"
    );
    assert_eq!(
        receiver("        $u = $user;\n        $u = new Address('a', 'b');\n        $u->§"),
        "Address",
        "the last binding"
    );
    assert_eq!(
        receiver("        /** @var Address $o */\n        $o = $row[0];\n        $o->§"),
        "Address",
        "@var"
    );
    assert_eq!(receiver("        $o = $row[0]; /** @var Address $o */\n        $o->§"), "Address");
    assert_eq!(
        receiver("        try {} catch (\\RuntimeException $e) {\n            $e->§\n        }"),
        "RuntimeException"
    );
    assert_eq!(
        receiver("        $u = $u->touch();\n        $u->§"),
        "",
        "itself before it: unknown"
    );
}

#[test]
fn unions_and_operators() {
    assert_eq!(
        receiver("        $a = $user->address ?? new Address('a', 'b');\n        $a->§"),
        "Address"
    );
    assert_eq!(receiver("        $a = $row ? $user : null;\n        $a->§"), "User");
    assert_eq!(
        receiver("        $a = match (1) { 1 => $user, default => $user };\n        $a->§"),
        "User"
    );
    assert_eq!(
        receiver("        $a = User::create();\n        $a->§"),
        "User",
        "static through the parent"
    );
    assert_eq!(receiver("        $s = Status::fromLabel('a');\n        $s->§"), "Status", "self");
    assert_eq!(receiver("        Status::Active->§"), "Status", "an enum's case");
}

#[test]
fn completion_offers_what_the_types_have() {
    let members = complete("        $this->repo->all()->first()->§");
    assert!(
        has(&members, &["describe", "touch", "getId", "email", "nickname", "isAdmin"]),
        "{members:?}"
    );
    assert!(!members.iter().any(|m| m == "secret"), "private stays hidden");
    let status = complete("        $user->status->§");
    assert!(has(&status, &["label", "name", "value", "from", "tryFrom", "cases"]), "{status:?}");
    let statics = complete("        Status::§");
    assert!(
        has(&statics, &["Active", "Banned", "fromLabel", "from", "tryFrom", "cases"]),
        "{statics:?}"
    );
    assert!(!statics.iter().any(|m| m == "value"), "not a static");
    let date = complete("        $d = new \\DateTimeImmutable();\n        $d->§");
    assert!(has(&date, &["format", "modify", "getTimestamp"]), "{date:?}");
    let chained =
        complete("        $d = new \\DateTimeImmutable();\n        $d->modify('+1 day')->§");
    assert!(has(&chained, &["format"]), "a built-in returning static: {chained:?}");
    let exception = complete("        $e = new \\RuntimeException();\n        $e->§");
    assert!(
        has(&exception, &["getMessage", "getPrevious"]),
        "inherited from Exception: {exception:?}"
    );
}

#[test]
fn navigation_follows_the_inference() {
    let (text, _, index) =
        service("        $this->repo->all()->first()->describe();\n        $user->nickname;");
    let php = PhpIntelligence::default();
    let at = text.find("describe()").unwrap() + 2;
    let found =
        code_intelligence::definition(&php, &index, Path::new(PATH), &text, at, &Project::none());
    let lines: Vec<(String, u32)> =
        found.iter().map(|l| (l.path.display().to_string(), l.line)).collect();
    assert_eq!(lines, [("/p/src/Model/User.php".to_owned(), 22)]);
    let at = text.find("nickname;").unwrap() + 2;
    let found =
        code_intelligence::definition(&php, &index, Path::new(PATH), &text, at, &Project::none());
    assert_eq!(found.first().map(|l| l.line), Some(7), "@property's line");
}

fn hover(body: &str) -> Option<HoverInfo> {
    let (text, offset, index) = service(body);
    let php = PhpIntelligence::default();
    code_intelligence::hover(&php, &index, Path::new(PATH), &text, offset, &Project::none())
}

#[test]
fn a_variable_s_hover_says_its_type_and_where_it_comes_from() {
    let info = hover("        $fi§rst = $this->repo->all()->first();").expect("a card");
    assert_eq!(info.code, "?User $first");
    assert!(
        info.doc.as_deref().is_some_and(|d| d.contains("$this->repo->all()->first()")),
        "{:?}",
        info.doc
    );
    let info =
        hover("        $s = Status::fromLabel('a');\n        echo $§s->label();").expect("a card");
    assert_eq!(info.code, "Status $s");
    let info = hover("        echo $us§er->email;").expect("a parameter");
    assert_eq!(info.code, "User $user");
    assert_eq!(hover("        $x§ = $unknown;"), None, "nothing known: no card");
    let builtin = hover("        $d = new \\DateTimeImmutable();\n        $d->for§mat('Y');")
        .expect("a built-in method");
    assert!(builtin.code.contains("format(string $format): string"), "{}", builtin.code);
}

#[test]
fn magic_and_facades() {
    let facade = r#"<?php
namespace Illuminate\Support\Facades;

/**
 * @method static mixed get(string $key, mixed $default = null)
 * @method static \Illuminate\Database\Query\Builder table(string $table)
 */
class DB {}
"#;
    let builder = "<?php\nnamespace Illuminate\\Database\\Query;\nclass Builder { public function where($a): static { return $this; } public function get(): array { return []; } }\n";
    let src = "<?php\nnamespace App;\nuse Illuminate\\Support\\Facades\\DB;\nDB::§";
    let offset = src.find(CURSOR).unwrap();
    let text = src.replacen(CURSOR, "", 1);
    let index =
        index_of(&[("/v/DB.php", facade), ("/v/Builder.php", builder), ("/p/a.php", &text)]);
    let php = PhpIntelligence::default();
    let (_, items) = code_intelligence::complete(
        &php,
        &index,
        Path::new("/p/a.php"),
        &text,
        offset,
        &Project::none(),
    );
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"get") && labels.contains(&"table"), "{labels:?}");
    let text = "<?php\nnamespace App;\nuse Illuminate\\Support\\Facades\\DB;\n$q = DB::table('users')->where(1);\n$q->";
    let ty = api::receiver_at(text, text.len(), &index).map(|t| t.show());
    assert_eq!(ty.as_deref(), Some("Builder"));
}

#[test]
fn functions_and_mixins() {
    let helpers = r#"<?php
namespace App\Support;

use App\Repo\Collection;

/**
 * @template TValue
 * @param array<TValue> $items
 * @return Collection<int, TValue>
 */
function collect(array $items = []): Collection { return new Collection(); }
"#;
    let mixed =
        "<?php\nnamespace App;\n/** @mixin \\App\\Repo\\UserRepository */\nclass Proxy {}\n";
    let mut all = files();
    all.push(("/p/helpers.php", helpers));
    all.push(("/p/Proxy.php", mixed));
    let text = "<?php\nnamespace App;\nuse function App\\Support\\collect;\n$c = collect([new \\App\\Model\\User('a')]);\n$c->first()->";
    all.push(("/p/t.php", text));
    let index = index_of(&all);
    let ty = api::receiver_at(text, text.len(), &index).map(|t| t.show());
    assert_eq!(ty.as_deref(), Some("User"), "a function's template from its argument");
    let text = "<?php\nnamespace App;\nfunction p(Proxy $p) { $p->find(1)->";
    let ty = api::receiver_at(text, text.len(), &index).map(|t| t.show());
    assert_eq!(ty.as_deref(), Some("User"), "through @mixin");
}

#[test]
fn unfinished_code_still_has_types() {
    assert_eq!(receiver("        $user->§\n        $x = 1;"), "User");
    let (text, _, index) = service("");
    let text = text.replace("    }\n}\n", "        $user->address->");
    let ty = api::receiver_at(&text, text.len(), &index).map(|t| t.show());
    assert_eq!(ty.as_deref(), Some("Address"), "at the end of an unfinished file");
}

#[test]
fn loops_and_depth_stay_bounded() {
    // Each variable defined from the previous one, far beyond the depth limit.
    let mut body = String::from("        $v0 = $user;\n");
    for i in 1..60 {
        body.push_str(&format!("        $v{i} = $v{};\n", i - 1));
    }
    body.push_str("        $v59->§");
    assert_eq!(receiver(&body), "", "too deep: unknown, not a hang");
    assert_eq!(receiver("        $a = $b;\n        $b = $a;\n        $a = $b;\n        $a->§"), "");
}

const ELOQUENT: &str = r#"<?php
namespace Illuminate\Database\Eloquent;

use Illuminate\Database\Concerns\BuildsQueries;

abstract class Model
{
    public function __callStatic($method, $parameters) {}

    /** @return \Illuminate\Database\Eloquent\Builder<static> */
    public static function query() {}
}

/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 * @mixin \Illuminate\Database\Query\Builder
 */
class Builder
{
    /** @use \Illuminate\Database\Concerns\BuildsQueries<TModel> */
    use BuildsQueries;

    /** @return $this */
    public function where($column, $operator = null) {}
}
"#;

const BUILDS_QUERIES: &str = r#"<?php
namespace Illuminate\Database\Concerns;

/** @template TValue */
trait BuildsQueries
{
    /** @return TValue|null */
    public function first($columns = ['*']) {}
}
"#;

const QUERY_BUILDER: &str = "<?php\nnamespace Illuminate\\Database\\Query;\nclass Builder { /** @return $this */ public function orderBy($c) {} }\n";

#[test]
fn eloquent_models_forward_their_static_calls_to_a_builder() {
    let model = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass User extends Model { public string $email; }\n";
    let probe = "<?php\nnamespace App;\nuse App\\Models\\User;\n$user = User::where('email', 1)->first();\n$q = User::query()->orderBy('name');\n";
    let index = index_of(&[
        ("/v/Model.php", ELOQUENT),
        ("/v/BuildsQueries.php", BUILDS_QUERIES),
        ("/v/QueryBuilder.php", QUERY_BUILDER),
        ("/p/User.php", model),
        ("/p/probe.php", probe),
    ]);
    let php = PhpIntelligence::default();
    let at = probe.find("$user").unwrap() + 2;
    let info = code_intelligence::hover(
        &php,
        &index,
        Path::new("/p/probe.php"),
        probe,
        at,
        &Project::none(),
    );
    assert_eq!(info.map(|i| i.code).as_deref(), Some("?User $user"));
    let at = probe.find("orderBy").unwrap();
    let ty = api::receiver_at(probe, at, &index).map(|t| t.show());
    assert_eq!(ty.as_deref(), Some("Builder<User>"));
    let found = code_intelligence::definition(
        &php,
        &index,
        Path::new("/p/probe.php"),
        probe,
        at + 2,
        &Project::none(),
    );
    assert_eq!(
        found.first().map(|l| l.path.display().to_string()).as_deref(),
        Some("/v/QueryBuilder.php"),
        "through the mixin"
    );
    let at = probe.find("where").unwrap() + 2;
    let found = code_intelligence::definition(
        &php,
        &index,
        Path::new("/p/probe.php"),
        probe,
        at,
        &Project::none(),
    );
    assert_eq!(
        found.first().map(|l| l.path.display().to_string()).as_deref(),
        Some("/v/Model.php"),
        "forwarded"
    );
    // Completion: the builder's own members, its trait's and its mixin's.
    let at = probe.find("orderBy").unwrap();
    let (_, items) = code_intelligence::complete(
        &php,
        &index,
        Path::new("/p/probe.php"),
        probe,
        at,
        &Project::none(),
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(["where", "first", "orderBy"].iter().all(|l| labels.contains(l)), "{labels:?}");
    let own = labels.iter().position(|l| *l == "where");
    let mixed = labels.iter().position(|l| *l == "orderBy");
    assert!(own < mixed, "a mixin's members after the class's own");
}

const SHOP: &str = "<?php\nnamespace App;\n\nconst DEFAULT_CITY = 'Cluj';\ndefine('App\\LIMIT', 10);\n\nenum Kind: string\n{\n    case Big = 'big';\n}\n\n/** A shop. */\nfinal class Shop\n{\n    public const MAX = 50;\n\n    public function __construct(private Cart $cart, int $size = 1)\n    {\n    }\n\n    public function cart(): Cart\n    {\n        return $this->cart;\n    }\n}\n\nfinal class Cart\n{\n    public function total(): float\n    {\n        return 0.0;\n    }\n}\n";

/// `SHOP` and a file using it (`body`, the cursor at `§`): the text, the cursor, the index.
fn shop(body: &str) -> (String, usize, SymbolIndex) {
    let src =
        format!("<?php\nnamespace App;\n\nfunction probe(bool $flag): void\n{{\n{body}\n}}\n");
    let offset = src.find(CURSOR).unwrap_or(0);
    let text = src.replacen(CURSOR, "", 1);
    let index = index_of(&[("/p/Shop.php", SHOP), (PATH, &text)]);
    (text, offset, index)
}

fn shop_definition(body: &str) -> Option<(String, u32)> {
    let (text, offset, index) = shop(body);
    let php = PhpIntelligence::default();
    let found = code_intelligence::definition(
        &php,
        &index,
        Path::new(PATH),
        &text,
        offset,
        &Project::none(),
    );
    found.first().map(|l| (l.path.display().to_string(), l.line))
}

fn shop_hover(body: &str) -> HoverInfo {
    let (text, offset, index) = shop(body);
    let php = PhpIntelligence::default();
    code_intelligence::hover(&php, &index, Path::new(PATH), &text, offset, &Project::none())
        .expect("a card")
}

/// A ternary's object can be either branch's: completion offers both classes' members.
#[test]
fn a_union_offers_every_branch_s_members() {
    let (text, offset, index) =
        shop("    $x = $flag ? new Cart() : new Shop(new Cart());\n    $x->§");
    let php = PhpIntelligence::default();
    let (_, items) =
        code_intelligence::complete(&php, &index, Path::new(PATH), &text, offset, &Project::none());
    let labels: Vec<String> = items.into_iter().map(|i| i.label).collect();
    assert!(has(&labels, &["total", "cart"]), "{labels:?}");
}

/// A method and a property by the same name: `->cart()` is the method, `->cart` the property.
#[test]
fn a_call_goes_to_the_method_and_an_access_to_the_property() {
    let shop_file = "/p/Shop.php".to_owned();
    assert_eq!(shop_definition("    (new Shop(new Cart()))->ca§rt();"), Some((shop_file, 20)));
    let (_, _, index) = shop("");
    let php = PhpIntelligence::default();
    let at = SHOP.find("$this->cart;").unwrap() + "$this->ca".len();
    let found = code_intelligence::definition(
        &php,
        &index,
        Path::new("/p/Shop.php"),
        SHOP,
        at,
        &Project::none(),
    );
    assert_eq!(found.first().map(|l| l.line), Some(16), "the promoted property");
}

/// Constants in a namespace (`const`, `define()`): where they're declared, their value.
#[test]
fn namespaced_constants_are_found_and_show_their_value() {
    let shop_file = "/p/Shop.php".to_owned();
    assert_eq!(shop_definition("    echo DEFAULT_CI§TY;"), Some((shop_file.clone(), 3)));
    assert_eq!(shop_definition("    echo \\App\\LIM§IT;"), Some((shop_file, 4)));
    assert_eq!(shop_hover("    echo DEFAULT_CI§TY;").code, "const DEFAULT_CITY = 'Cluj'");
    assert_eq!(shop_hover("    echo Shop::MA§X;").code, "const MAX = 50");
    assert_eq!(shop_hover("    echo Kind::B§ig;").code, "case Big = 'big'");
}

/// `new Shop(`: the class's card, its constructor's signature in it; the parameters' help
/// is the constructor's alone.
#[test]
fn a_new_shows_the_class_with_its_constructor() {
    let card = shop_hover("    $s = new Sh§op(new Cart());");
    let lines: Vec<&str> = card.code.lines().collect();
    assert_eq!(lines[0], "class Shop", "{card:?}");
    assert!(lines[1].contains("__construct(") && lines[1].contains("Cart $cart"), "{card:?}");
    assert_eq!(card.params.len(), 2);
    let (text, offset, index) = shop("    $s = new Shop(new Cart(), §);");
    let php = PhpIntelligence::default();
    let help = code_intelligence::signature_help(
        &php,
        &index,
        Path::new(PATH),
        &text,
        offset,
        &Project::none(),
    )
    .expect("the constructor's parameters");
    assert!(help.code.contains("__construct(") && !help.code.contains("class Shop"), "{help:?}");
    assert_eq!(help.active, Some(1));
}

/// A method named in a callable's string: navigation, hover and completion take it, when
/// the class is certain and has the method.
#[test]
fn callables_written_as_strings_name_their_method() {
    let shop_file = "/p/Shop.php".to_owned();
    let cart = Some((shop_file, 20));
    let body = |code: &str| format!("    $shop = new Shop(new Cart());\n    $f = {code};");
    assert_eq!(shop_definition(&body("[$shop, 'ca§rt']")), cart);
    assert_eq!(shop_definition(&body("[Shop::class, 'ca§rt']")), cart);
    assert_eq!(shop_definition(&body("['App\\Shop', 'ca§rt']")), cart);
    assert_eq!(shop_definition(&body("'App\\Shop::ca§rt'")), cart);
    assert_eq!(shop_definition(&body("\\Closure::fromCallable([$shop, 'ca§rt'])")), cart);
    assert_eq!(shop_definition(&body("method_exists($shop, 'ca§rt')")), cart);
    // An object of unknown type, a method the class doesn't have, a plain string: nothing.
    assert_eq!(shop_definition(&body("[$unknown, 'ca§rt']")), None);
    assert_eq!(shop_definition(&body("[$shop, 'mis§sing']")), None);
    assert_eq!(shop_definition(&body("strlen('to§tal')")), None);
    // A string is a class only where PHP takes it as one, and by its case when global.
    let cart_class = Some(("/p/Shop.php".to_owned(), 26));
    assert_eq!(shop_definition(&body("strlen('ca§rt')")), None, "not the class Cart");
    assert_eq!(shop_definition(&body("echo 'Ca§rt'")), None);
    assert_eq!(shop_definition(&body("$a = ['Ca§rt' => 1]")), None);
    assert_eq!(shop_definition(&body("class_exists('App\\Ca§rt')")), cart_class);
    assert_eq!(shop_definition(&body("$c = ['cart' => 'App\\Ca§rt']")), cart_class);
    assert_eq!(shop_definition(&body("$c = 'app\\ca§rt'")), cart_class, "PHP's case");
    assert_eq!(shop_definition(&body("class_exists('Ca§rt')")), None, "a global Cart");
    let card = shop_hover(&body("[$shop, 'ca§rt']"));
    assert!(card.code.contains("function cart("), "{card:?}");
    // Completion in the string: the class's methods, as bare names.
    let (text, offset, index) = shop(&body("[$shop, 'c§']"));
    let php = PhpIntelligence::default();
    let (context, items) =
        code_intelligence::complete(&php, &index, Path::new(PATH), &text, offset, &Project::none());
    assert_eq!(context.prefix, "c");
    let cart = items.iter().find(|i| i.label == "cart").expect("cart offered");
    assert_eq!(cart.insert, "cart");
    assert!(
        items.iter().all(|i| i.kind == code_intelligence::CompletionKind::Method),
        "methods only"
    );
}

/// A framework's call (registered: `Locator::get` gives the class its argument names) —
/// where the method's own type says only `mixed`; also through a class extending it.
#[test]
fn a_frameworks_call_gives_the_class_its_argument_names() {
    code_intelligence::calls::register(
        code_intelligence::calls::ClassArgumentCall::new("App\\Di\\Locator", "get", 0)
            .names("test.locator"),
    );
    const LOCATOR: &str = "<?php\nnamespace App\\Di;\ninterface Locator { public function get(string $id): mixed; }\ninterface Container extends Locator {}\n";
    let code = format!(
        "<?php\nnamespace App;\nuse App\\Di\\Container;\nuse App\\Model\\User;\nfunction f(Container $c) {{ $c->get(User::class)->{CURSOR} }}\n"
    );
    let offset = code.find(CURSOR).unwrap();
    let text = code.replacen(CURSOR, "", 1);
    let mut all = files();
    all.push(("/p/src/Di/Locator.php", LOCATOR));
    all.push(("/p/src/f.php", &text));
    let index = index_of(&all);
    let ty = api::receiver_at(&text, offset, &index).map(|t| t.show()).unwrap_or_default();
    assert_eq!(ty, "User");
    let plain = text.replace("User::class", "'user'");
    let at = offset - "User::class".len() + "'user'".len();
    let ty = api::receiver_at(&plain, at, &index);
    assert!(ty.is_none_or(|t| t.show() != "User"), "a name nobody knows: its declared type");
    // The project's framework knows that name's class (its table in the index).
    let mut index = index;
    let names =
        std::collections::HashMap::from([("user".to_owned(), "App\\Model\\User".to_owned())]);
    index.set_named("test.locator", names);
    let ty = api::receiver_at(&plain, at, &index).map(|t| t.show()).unwrap_or_default();
    assert_eq!(ty, "User", "a name its project's table has");
    let interpolated = text.replace("User::class", "\"$x\"");
    let at = offset - "User::class".len() + "\"$x\"".len();
    let ty = api::receiver_at(&interpolated, at, &index);
    assert!(ty.is_none_or(|t| t.show() != "User"), "nothing interpolated");
}

/// A packed file's codec for the tests: each file's symbols kept aside by path (the bytes
/// are the path).
#[derive(Debug, Default)]
struct ByPath(std::collections::HashMap<String, code_intelligence::FileSymbols>);

impl code_intelligence::Unpack for ByPath {
    fn unpack(&self, bytes: &[u8]) -> Option<code_intelligence::FileSymbols> {
        self.0.get(std::str::from_utf8(bytes).ok()?).cloned()
    }
}

/// The index of `files`, those `packed` says kept packed as a library's are (plan 74).
fn index_packed(files: &[(&str, &str)], packed: impl Fn(&str) -> bool) -> SymbolIndex {
    let php = PhpIntelligence::default();
    let extracted: Vec<(&str, code_intelligence::FileSymbols)> = files
        .iter()
        .map(|(path, text)| {
            let mut symbols = php.extract(text);
            symbols.language = "php".into();
            (*path, symbols)
        })
        .collect();
    let codec =
        Arc::new(ByPath(extracted.iter().map(|(p, s)| ((*p).to_owned(), s.clone())).collect()));
    let mut index = SymbolIndex::default();
    for (path, symbols) in extracted {
        if packed(path) {
            let bytes: Arc<[u8]> = Arc::from(path.as_bytes());
            index.set_file_packed(Path::new(path), &symbols, bytes, Arc::clone(&codec) as _);
        } else {
            index.set_file(Path::new(path), symbols);
        }
    }
    index
}

/// Everything asked at `offset` of `text` (file `path`): completion, hover, the definition,
/// what a rename would rename and the file's checks — as text, to compare.
fn answers(index: &SymbolIndex, path: &str, text: &str, offset: usize) -> Vec<String> {
    let php = PhpIntelligence::default();
    let (path, project) = (Path::new(path), Project::none());
    let (_, items) = code_intelligence::complete(&php, index, path, text, offset, &project);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    let hover = code_intelligence::hover(&php, index, path, text, offset, &project);
    let found = code_intelligence::definition(&php, index, path, text, offset, &project);
    let subject = code_intelligence::rename::subject_at(
        index,
        path,
        text,
        offset,
        "php".into(),
        found.first().cloned(),
        &|_| php.naming(),
    );
    let checked = php.check(text, &code_intelligence::Types { index, path });
    vec![
        format!("{labels:?}"),
        format!("{hover:?}"),
        format!("{found:?}"),
        format!("{:?}", subject.map(|s| (s.name, s.declared.fqn, s.declared.at))),
        format!("{:?} {:?}", checked.problems, checked.deprecated),
    ]
}

/// A library's files kept packed (plan 74) answer as when kept whole: completion, hover,
/// definitions, rename and checks give the same, on members, inherited ones, a trait's, an
/// enum's, `@property` / `@method`, Eloquent's forwarding and a mixin.
#[test]
fn a_library_kept_packed_answers_as_when_kept_whole() {
    let bodies = [
        "        $this->repo->all()->first()->§",
        "        $this->repo->all()->first()->desc§ribe();",
        "        $user->nick§name;",
        "        $user->isAd§min();",
        "        $user->status->§",
        "        Status::§",
        "        $user->tou§ch();",
        "        $a = new Address('a', 'b');\n        $a->missing§Method();\n        $a->withCity();",
        "        $a = User::create();\n        $a->§",
    ];
    for body in bodies {
        let (text, offset, _) = service(body);
        let mut all = files();
        all.push((PATH, &text));
        let whole = index_packed(&all, |_| false);
        let packed = index_packed(&all, |p| p != PATH);
        let expected = answers(&whole, PATH, &text, offset);
        let nothing = ["[]", "None", "[]", "None", "[] []"];
        assert_ne!(expected, nothing, "something is answered: {body}");
        if body.contains("missing") {
            assert!(expected[4].contains("missingMethod"), "the check says it: {}", expected[4]);
        }
        assert_eq!(answers(&packed, PATH, &text, offset), expected, "{body}");
    }
    let model = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass User extends Model { public string $email; }\n";
    let probe = "<?php\nnamespace App;\nuse App\\Models\\User;\n$user = User::where('email', 1)->first();\n$q = User::query()->orderBy('name');\n";
    let all = [
        ("/v/Model.php", ELOQUENT),
        ("/v/BuildsQueries.php", BUILDS_QUERIES),
        ("/v/QueryBuilder.php", QUERY_BUILDER),
        ("/p/User.php", model),
        ("/p/probe.php", probe),
    ];
    let whole = index_packed(&all, |_| false);
    let packed = index_packed(&all, |p| p.starts_with("/v/"));
    for at in [
        probe.find("$user").unwrap() + 2,
        probe.find("where").unwrap() + 2,
        probe.find("orderBy").unwrap(),
        probe.find("orderBy").unwrap() + 2,
    ] {
        let expected = answers(&whole, "/p/probe.php", probe, at);
        assert_eq!(answers(&packed, "/p/probe.php", probe, at), expected, "at {at}");
    }
}
