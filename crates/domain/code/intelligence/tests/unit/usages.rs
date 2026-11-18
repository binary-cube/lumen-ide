use super::*;
use crate::Naming;

/// PHP's code, as its module writes it.
const PHP: SourceRules = SourceRules {
    extensions: &["php"],
    includes: &[],
    include_suffixes: &[],
    before_type: &["new", "extends", "implements", "instanceof"],
    static_access: Some("::"),
    member_access: &["->", "::"],
    declares_function: &["function"],
    declares_type: &["class", "interface", "trait", "enum"],
    modifiers: &[],
    naming: Naming { sigil: Some('$'), namespace_separator: Some('\\'), ..Naming::PLAIN },
    comments: None,
};

/// JavaScript's: members after `.`, no sigil, no static access of its own.
const JS: SourceRules = SourceRules {
    extensions: &["js"],
    member_access: &["."],
    declares_function: &["function"],
    declares_type: &["class", "interface", "trait", "enum"],
    ..SourceRules::plain(&["js"])
};

fn q(name: &str, kind: SymbolKind) -> UsageQuery {
    UsageQuery {
        name: name.into(),
        kind,
        language: "php".into(),
        declared_in: None,
        declared_line: 0,
    }
}

#[test]
fn rules_per_kind() {
    let src = "$car->stop(2); stop(1); Car::stop(); $stopper->stop; function stop() {}";
    assert_eq!(matches(src, &q("stop", SymbolKind::Method), &PHP).len(), 2);
    assert_eq!(matches(src, &q("stop", SymbolKind::Function), &PHP).len(), 1);
    let src = "class Car {} new Car(); Car::make(); $Car; function f(Car $c) {}";
    assert_eq!(matches(src, &q("Car", SymbolKind::Class), &PHP).len(), 3);
    let src = "<div class=\"btn btn-primary\">";
    let css = SourceRules::plain(&["css"]);
    assert_eq!(matches(src, &q("btn", SymbolKind::CssClass), &css).len(), 1);
}

/// What follows a member access and what a sigil writes come from the language's rules.
#[test]
fn rules_of_the_language() {
    let src = "$a->speed; Car::$speed; Car::SPEED; $speed; speed;";
    assert_eq!(matches(src, &q("speed", SymbolKind::Property), &PHP).len(), 2);
    let src = "car.stop(); this.speed; stop(); function stop() {}";
    assert_eq!(matches(src, &q("stop", SymbolKind::Method), &JS).len(), 1);
    assert_eq!(matches(src, &q("speed", SymbolKind::Property), &JS).len(), 1);
    assert_eq!(matches(src, &q("stop", SymbolKind::Function), &JS).len(), 1);
    // A `$` is part of a JavaScript name only by its rules: `$stop` isn't `stop` in PHP.
    assert_eq!(matches("$stop();", &q("stop", SymbolKind::Function), &PHP).len(), 0);
    assert_eq!(matches("$.stop();", &q("stop", SymbolKind::Method), &JS).len(), 1);
}

/// One per line, the declaration itself left out, the files read through the caller.
#[test]
fn usages_one_per_line_without_the_declaration() {
    let query = UsageQuery {
        declared_in: Some("src/Car.php".into()),
        declared_line: 1,
        ..q("stop", SymbolKind::Function)
    };
    let files = ["src/Car.php".to_owned(), "src/use.php".to_owned(), "gone.php".to_owned()];
    let read = |path: &Path| match path.to_str()? {
        "src/Car.php" => Some("<?php\nfunction stop() {}\nstop();\n".to_owned()),
        "src/use.php" => Some("stop(); stop();\n\nstop();".to_owned()),
        _ => None,
    };
    let found = find_usages(&files, &query, &PHP, &read, &|| false);
    let at = |p: &str, line| UsageAt { path: p.into(), line };
    assert_eq!(found, [at("src/Car.php", 2), at("src/use.php", 0), at("src/use.php", 2)]);
    assert!(find_usages(&files, &query, &PHP, &read, &|| true).is_empty(), "cancelled");
}

/// Where a symbol can be used is its language's module's to say (none: nowhere).
#[test]
fn where_a_symbol_can_be_used() {
    assert!(usage_extensions(None, SymbolKind::CssClass).is_empty());
    assert!(usage_extensions(None, SymbolKind::Method).is_empty());
}

/// Several declarations counted in one pass over the files (each read once), one per line,
/// the declarations' own lines left out.
#[test]
fn usages_counted_for_several_at_once() {
    let a =
        "<?php\nclass Cart { function total() {} }\n$c = new Cart(); $c->total(); $c->total();\n";
    let b = "<?php\n$x->total();\n";
    let reads = std::cell::Cell::new(0);
    let read = |p: &Path| {
        reads.set(reads.get() + 1);
        Some(if p == Path::new("a.php") { a } else { b }.to_owned())
    };
    let cart = UsageQuery {
        declared_in: Some("a.php".into()),
        declared_line: 1,
        ..q("Cart", SymbolKind::Class)
    };
    let total = UsageQuery {
        declared_in: Some("a.php".into()),
        declared_line: 1,
        ..q("total", SymbolKind::Method)
    };
    let files = ["a.php".to_owned(), "b.php".to_owned()];
    let counts = count_usages(&files, &[cart, total], &PHP, &read, &|| false);
    assert_eq!(counts, [1, 2], "one line each in a.php, then b.php's");
    assert_eq!(reads.get(), 2);
}

/// A name in a comment's text isn't a usage; written there as code (a tag's type) it is
/// (plan 75: the language reads its comments).
#[test]
fn a_comment_s_text_is_no_usage() {
    fn place(text: &str, at: usize) -> Option<crate::CommentPlace> {
        let comment = text[..at].rfind("/*").filter(|&o| !text[o..at].contains("*/"))?;
        Some(match text[comment..at].ends_with("@param ") {
            true => crate::CommentPlace::Code(crate::Target::Class("User".into())),
            false => crate::CommentPlace::Text,
        })
    }
    let rules = SourceRules { comments: Some(crate::CommentReader(place)), ..PHP };
    let src = "<?php\n/** Saves a User.\n * @param User $u */\nfunction save(User $u) {}\n";
    let found = matches(src, &q("User", SymbolKind::Class), &rules);
    assert_eq!(found, [src.find("User $u */").unwrap(), src.find("User $u)").unwrap()]);
    assert_eq!(matches(src, &q("User", SymbolKind::Class), &PHP).len(), 3, "without: all");
}
