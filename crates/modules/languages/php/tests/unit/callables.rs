use super::*;

/// The callable at `§` of `src`: its class part (as a navigation target writes it) and its
/// method's name.
fn callable(src: &str) -> Option<(String, String)> {
    let offset = src.find('§').unwrap();
    let text = src.replacen('§', "", 1);
    with_callable(&text, offset, |_, c| {
        Some((written_class(&text, &c.class), text[c.name.clone()].to_owned()))
    })
}

#[test]
fn the_callable_forms_name_their_method() {
    let pair = |s: &str| Some((s.to_owned(), "cart".to_owned()));
    let of = |code: &str| callable(&format!("<?php\n$f = {code};\n"));
    assert_eq!(of("[$obj, 'ca§rt']"), pair(""));
    assert_eq!(of("[$this->shop, \"ca§rt\"]"), pair(""));
    assert_eq!(of("[self::class, 'ca§rt']"), pair("self"));
    assert_eq!(of("[static::class, 'cart§']"), pair("static"));
    assert_eq!(of("[Shop::class, '§cart']"), pair("Shop"));
    assert_eq!(of("['App\\Shop', 'ca§rt']"), pair("\\App\\Shop"));
    assert_eq!(of("'App\\Shop::ca§rt'"), pair("\\App\\Shop"));
    assert_eq!(of("\\Closure::fromCallable([$shop, 'ca§rt'])"), pair(""));
    assert_eq!(of("method_exists($shop, 'ca§rt')"), pair(""));
    // Nothing else: a lone string, the class part of the string form, three elements, keys,
    // an interpolated string, a name that isn't one.
    assert_eq!(of("'ca§rt'"), None);
    assert_eq!(of("'App\\Sh§op::cart'"), None);
    assert_eq!(of("[$a, 'ca§rt', 1]"), None);
    assert_eq!(of("['x' => $a, 'y' => 'ca§rt']"), None);
    assert_eq!(of("[$a, \"ca§rt{$x}\"]"), None);
    assert_eq!(of("[$a, 'ca§rt now']"), None);
    assert_eq!(of("strlen('ca§rt')"), None);
}

/// A string names a class only where PHP takes it as one.
#[test]
fn a_string_is_a_class_only_where_php_takes_it_as_one() {
    let of = |code: &str| {
        let src = format!("<?php\n{code};\n");
        let offset = src.find('§').unwrap();
        class_in_string(&src.replacen('§', "", 1), offset)
    };
    let class = |name: &str| Some(Some(name.to_owned()));
    // Taken as a class: a full name with a namespace, anywhere; a callable's class part; the
    // functions asking for one.
    assert_eq!(of("$c = ['mailer' => 'App\\Service\\Mai§ler']"), class("\\App\\Service\\Mailer"));
    assert_eq!(of("$f = ['Sh§op', 'cart']"), class("\\Shop"));
    assert_eq!(of("$f = 'Sh§op::cart'"), class("\\Shop"));
    assert_eq!(of("class_exists('Ca§rt')"), class("\\Cart"));
    assert_eq!(of("is_a($x, 'Ca§rt')"), class("\\Cart"));
    assert_eq!(of("property_exists('\\App\\Ca§rt', 'lines')"), class("\\App\\Cart"));
    // Not: a word, a sentence, an array's key, a string given elsewhere, the wrong argument.
    assert_eq!(of("strlen('ca§rt')"), Some(None));
    assert_eq!(of("echo 'Us§er'"), Some(None));
    assert_eq!(of("echo 'Hello Us§er'"), Some(None));
    assert_eq!(of("$a = ['Us§er' => 1]"), Some(None));
    assert_eq!(of("is_a('Us§er', $x)"), Some(None));
    // Not in a string: not ours to say.
    assert_eq!(of("new Us§er()"), None);
}
