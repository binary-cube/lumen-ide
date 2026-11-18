use std::path::Path;

use code_intelligence::{LanguageIntelligence, SymbolIndex, Types};

use super::*;
use crate::PhpIntelligence;

const MODEL: &str = r#"<?php
namespace App\Model;

class User
{
    public function save(User $user, bool $flush = false): void {}

    /** @deprecated use save() */
    public function store(): void {}

    #[\Deprecated(message: 'gone')]
    public function keep(): void {}

    public function many(string ...$names): void {}
}

/** @method void anything() */
class Magic { public function __call($n, $a) {} }

class Open { public function __call($name, $args) {} }

/** @mixin User */
class Mixed {}

/** @deprecated */
class Old {}

function helper(int $a, int $b = 0): int { return $a; }
"#;

fn index_with(text: &str) -> SymbolIndex {
    let php = PhpIntelligence::default();
    let mut index = SymbolIndex::default();
    for (path, src) in [("/p/Model.php", MODEL), ("/p/probe.php", text)] {
        let mut symbols = php.extract(src);
        symbols.language = "php".into();
        index.set_file(Path::new(path), symbols);
    }
    index
}

/// What the check says of `body` (inside a function of a file using the model), each as
/// its kind and what it's about.
fn checked(body: &str) -> Vec<(String, String)> {
    let text = format!(
        "<?php\nnamespace App;\n\nuse App\\Model\\User;\nuse App\\Model\\{{Magic, Open, Mixed, Old}};\nuse function App\\Model\\helper;\n\nfunction probe(User $user, Magic $magic, Open $open, Mixed $mixed): void\n{{\n{body}\n}}\n"
    );
    let index = index_with(&text);
    let found = PhpIntelligence::default()
        .check(&text, &Types { index: &index, path: Path::new("/p/probe.php") });
    let mut out: Vec<(String, String)> =
        found.problems.iter().map(|p| (p.kind.to_owned(), p.name.clone())).collect();
    out.extend(found.deprecated.iter().map(|d| (format!("deprecated {}", d.kind), d.name.clone())));
    out
}

fn pair(kind: &str, name: &str) -> (String, String) {
    (kind.to_owned(), name.to_owned())
}

#[test]
fn unknown_classes() {
    assert_eq!(checked("    $x = new MissingClass();"), [pair("undefined_class", "MissingClass")]);
    assert_eq!(
        checked("    Gone::make();\n    $user instanceof Nope;"),
        [pair("undefined_class", "Gone"), pair("undefined_class", "Nope")]
    );
    // Known: the project's, PHP's, an import, `self` / `static`, a name the project has.
    assert!(
        checked("    new User(); new \\DateTimeImmutable(); new \\ArrayObject([]);").is_empty()
    );
    assert!(checked("    $o = new Old();").iter().all(|(k, _)| k == "deprecated class"));
}

#[test]
fn argument_counts() {
    assert_eq!(checked("    $user->save();"), [pair("too_few_arguments", "save()")]);
    assert!(checked("    $user->save($user);").is_empty(), "the optional one may be left out");
    assert_eq!(checked("    explode(',');"), [pair("too_few_arguments", "explode()")]);
    assert_eq!(
        checked("    strlen('a', 'b');"),
        [pair("too_many_arguments", "strlen()")],
        "PHP's own"
    );
    assert!(
        checked("    helper(1, 2, 3);").is_empty(),
        "a project function's extra arguments are allowed"
    );
    assert_eq!(checked("    helper();"), [pair("too_few_arguments", "helper()")]);
    // Not counted: spread, named arguments, first-class callables, variadics.
    assert!(checked("    $user->save(...$args); $user->save(flush: true, user: $user); $f = strlen(...); $user->many();").is_empty());
    // Magic methods' documented parameters aren't held against a call.
    assert!(checked("    $magic->anything(1, 2);").is_empty());
}

#[test]
fn unknown_methods_only_on_closed_classes() {
    assert_eq!(checked("    $user->missing();"), [pair("undefined_method", "User::missing()")]);
    assert!(
        checked("    $open->whatever(); $magic->other(); $mixed->save($user);").is_empty(),
        "__call, @method, @mixin"
    );
    assert!(checked("    $unknown->whatever();").is_empty(), "an unknown receiver");
    assert!(checked("    $d = new \\DateTimeImmutable(); $d->format('Y');").is_empty());
}

#[test]
fn deprecated_calls() {
    assert_eq!(checked("    $user->store();"), [pair("deprecated method", "store")]);
    assert_eq!(checked("    $user->keep();"), [pair("deprecated method", "keep")]);
}

#[test]
fn arities() {
    assert_eq!(arity("(int $a, int $b = 1)"), Some((1, Some(2))));
    assert_eq!(
        arity("(int $a = 1, int $b)"),
        Some((2, Some(2))),
        "a required one after an optional"
    );
    assert_eq!(arity("(string ...$names)"), Some((0, None)));
    assert_eq!(arity("()"), Some((0, Some(0))));
}

#[test]
fn stubs_deprecations_and_properties() {
    let stubs = crate::stubs::parse(
        "v\t8.1,8.2\n8.1\t\tc\tclass\tBox\t\t\tcore\n8.1\t\tp\tBox\tsize\tpublic readonly\tint\n8.2\t\tdf\tutf8_encode\n8.2\t\tdm\tBox\topen\n",
    );
    assert_eq!(stubs.deprecated("utf8_encode"), Some("8.2"));
    assert_eq!(stubs.deprecated("\\Box::OPEN"), Some("8.2"));
    assert_eq!(stubs.deprecated("strlen"), None);
    let size = stubs.properties("box").first().copied().expect("a property");
    assert!(size.ty == "int" && size.is_public() && !size.is_static());
}

/// The bundled stubs (regenerated from every PHP version, plan 55o) carry PHP's own
/// deprecations (G07: `utf8_encode` since 8.2) and its classes' properties.
#[test]
fn the_bundled_stubs_know_deprecations_and_properties() {
    let stubs = crate::stubs::get();
    assert_eq!(stubs.deprecated("utf8_encode"), Some("8.2"));
    assert_eq!(stubs.deprecated("strlen"), None);
    let message = stubs.properties("exception").iter().find(|p| p.name == "message");
    assert!(message.is_some_and(|p| !p.is_public()), "Exception::$message, protected");
}

/// G07 (the probes of plan 43): a call to PHP's own deprecated function is said.
#[test]
fn a_deprecated_php_function_is_said() {
    let found = checked("    $s = utf8_encode('x');");
    assert!(
        found.iter().any(|(k, n)| k.starts_with("deprecated") && n.contains("utf8_encode")),
        "{found:?}"
    );
}

#[test]
fn a_trait_s_method_wins_over_the_parent_s() {
    let units = "<?php\nnamespace Lib;\ntrait Units { public function add($unit, $value = 1, $overflow = null): static { return $this; } }\n";
    let date = "<?php\nnamespace Lib;\nclass Date extends \\DateTime { use Units; }\n";
    let probe = "<?php\nnamespace App;\nfunction f(\\Lib\\Date $d) { $d->add('day', 1, true); }\n";
    let php = PhpIntelligence::default();
    let mut index = SymbolIndex::default();
    for (path, src) in [("/v/Units.php", units), ("/v/Date.php", date), ("/p/probe.php", probe)] {
        let mut symbols = php.extract(src);
        symbols.language = "php".into();
        index.set_file(Path::new(path), symbols);
    }
    let found = php.check(probe, &Types { index: &index, path: Path::new("/p/probe.php") });
    assert!(found.problems.is_empty(), "{:?}", found.problems);
}

#[test]
fn mixins_pointing_at_each_other_end() {
    let a = "<?php\nnamespace Lib;\n/** @mixin B */\nclass A {}\n/** @mixin A */\nclass B {}\n/** @mixin A\n * @mixin B */\nclass C extends A {}\n";
    let probe =
        "<?php\nnamespace App;\nfunction f(\\Lib\\C $c) { $c->nothing(); $c->none()->x(); }\n";
    let php = PhpIntelligence::default();
    let mut index = SymbolIndex::default();
    for (path, src) in [("/v/A.php", a), ("/p/probe.php", probe)] {
        let mut symbols = php.extract(src);
        symbols.language = "php".into();
        index.set_file(Path::new(path), symbols);
    }
    let started = std::time::Instant::now();
    let found = php.check(probe, &Types { index: &index, path: Path::new("/p/probe.php") });
    assert!(found.problems.is_empty(), "mixins: never certain");
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}
