use super::*;

fn ro(source: &str, args: &[(&str, Value)]) -> String {
    Message::parse(source).format(Locale::Ro, args)
}

#[test]
fn romanian_plurals() {
    let m = "{count, plural, =0 {nicio eroare} one {# eroare} few {# erori} other {# de erori}}";
    let cases = [
        (0, "nicio eroare"),
        (1, "1 eroare"),
        (2, "2 erori"),
        (19, "19 erori"),
        (20, "20 de erori"),
        (101, "101 erori"),
        (120, "120 de erori"),
    ];
    for (n, expected) in cases {
        assert_eq!(ro(m, &[("count", n.into())]), expected);
    }
    assert_eq!(ro("{n, plural, few {# ore} other {# de ore}}", &[("n", 1.5.into())]), "1,5 ore");
}

#[test]
fn english_args_select_and_escapes() {
    let m = Message::parse("{count, plural, one {# error} other {# errors}} in {file}");
    assert_eq!(
        m.format(Locale::En, &[("count", 1.into()), ("file", "a.php".into())]),
        "1 error in a.php"
    );
    assert_eq!(
        m.format(Locale::En, &[("count", 4.into()), ("file", "a.php".into())]),
        "4 errors in a.php"
    );
    let s = Message::parse("{kind, select, file {Fișier} other {Director}} nou");
    assert_eq!(s.format(Locale::Ro, &[("kind", "file".into())]), "Fișier nou");
    assert_eq!(s.format(Locale::Ro, &[("kind", "x".into())]), "Director nou");
    assert_eq!(ro("It''s '{literal}'", &[]), "It's {literal}");
    // Wrong syntax: the text stays as is.
    assert_eq!(ro("{broken, plural, one {x}", &[]), "{broken, plural, one {x}");
}

/// The tests that change the (global) language take turns.
static LOCALE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn catalogs_fallback_and_macro() {
    let _lock = LOCALE.lock();
    register("ro", "[test]\nhello = \"Salut, {name}!\"\n");
    register("en", "[test]\nhello = \"Hello, {name}!\"\nonly_en = \"English only\"\n");
    set_locale("ro");
    assert_eq!(tr!("test.hello", name = "Ana"), "Salut, Ana!");
    assert_eq!(t("test.only_en"), "English only");
    assert_eq!(t("test.missing"), "test.missing");
    // The bundled catalogs have the same keys in all languages.
    let registry = REGISTRY.read().unwrap();
    let keys = |l: &str| {
        let mut k: Vec<_> =
            registry.catalogs[l].keys().filter(|k| !k.starts_with("test.")).cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys("ro"), keys("en"));
}

/// Sizes in bytes, in the language's decimal separator.
#[test]
fn byte_sizes() {
    let _lock = LOCALE.lock();
    set_locale("en");
    assert_eq!(bytes(812), "812 B");
    assert_eq!(bytes(1025), "2 KB");
    assert_eq!(bytes(3 * 1024 * 1024 + 400 * 1024), "3.4 MB");
    assert_eq!(bytes(24 * 1024 * 1024), "24 MB");
    assert_eq!(bytes(1288 * 1024 * 1024), "1.3 GB");
    set_locale("ro");
    assert_eq!(bytes(3 * 1024 * 1024 + 400 * 1024), "3,4 MB");
}

/// How long ago, from "just now" to years.
#[test]
fn ages_read_naturally() {
    let _lock = LOCALE.lock();
    set_locale("en");
    assert_eq!(ago(30), "just now");
    assert_eq!(ago(-5), "just now", "a clock behind: never negative");
    assert_eq!(ago(3 * 60), "3 minutes ago");
    assert_eq!(ago(3600), "1 hour ago");
    assert_eq!(ago(3 * 86_400), "3 days ago");
    assert_eq!(ago(2 * 86_400 * 30), "2 months ago");
    assert_eq!(ago(3 * 86_400 * 365), "3 years ago");
    set_locale("ro");
}

/// A message in every language at once (a search's words, whatever the interface's).
#[test]
fn a_message_in_every_language() {
    let words = in_every_language("settings.keywords_history");
    assert!(words.contains("istoric") && words.contains("history"), "{words}");
    assert_eq!(in_every_language("settings.no_such_key"), "");
}

/// Each locale is valid TOML for every reader: a key once per section (ours took the last of
/// two quietly), and both languages have the same keys.
#[test]
#[allow(clippy::disallowed_methods)] // The locales' own files, read as text.
fn locales_have_each_key_once_and_the_same_keys() {
    let keys = |name: &str| {
        let path = format!("{}/locales/{name}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).unwrap();
        let mut section = String::new();
        let mut keys = std::collections::BTreeSet::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.to_owned();
            } else if let Some((key, _)) =
                line.split_once('=').filter(|_| !line.starts_with('#') && !line.is_empty())
            {
                let key = format!("{section}.{}", key.trim());
                assert!(keys.insert(key.clone()), "{name}: {key} twice");
            }
        }
        keys
    };
    let (ro, en) = (keys("ro"), keys("en"));
    let only = |a: &std::collections::BTreeSet<String>, b: &std::collections::BTreeSet<String>| {
        a.difference(b).cloned().collect::<Vec<_>>()
    };
    assert!(only(&ro, &en).is_empty(), "only in ro: {:?}", only(&ro, &en));
    assert!(only(&en, &ro).is_empty(), "only in en: {:?}", only(&en, &ro));
}

/// A catalog added moves the generation (texts kept under it are made again); the language
/// set moves it too (the same `bump`, not set here: the other tests share the language).
#[test]
fn the_generation_moves_with_the_messages() {
    let before = generation();
    register("test-generation", "test_generation = \"x\"");
    assert!(generation() > before);
}
