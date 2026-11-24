use super::*;

fn source(path: &str, text: &str) -> Source {
    Source { path: path.into(), text: text.into() }
}

#[test]
fn inline_test_modules_are_found_and_path_ones_are_not() {
    let inline =
        source("a/src/lib.rs", "#[cfg(test)]\nmod tests {\n    #[test]\n    fn x() {}\n}\n");
    assert_eq!(inline_tests(&inline).len(), 1);
    let attributed =
        source("a/src/lib.rs", "#[cfg(test)]\n#[allow(dead_code)]\npub(crate) mod t {}\n");
    assert_eq!(inline_tests(&attributed).len(), 1, "after other attributes too");
    let file =
        source("a/src/lib.rs", "#[cfg(test)]\n#[path = \"../tests/unit/lib.rs\"]\nmod tests;\n");
    assert!(inline_tests(&file).is_empty());
    let gated_item = source("a/src/lib.rs", "#[cfg(test)]\nfn helper() {}\nmod real {\n}\n");
    assert!(inline_tests(&gated_item).is_empty(), "the gate ends at the item");
}

#[test]
fn the_brand_in_a_literal_needs_an_alias_comment() {
    let plain = source("a/src/lib.rs", "let thread = \"lumen-index\";\n");
    assert_eq!(brand_literals(&plain, "lumen").len(), 1);
    let shouting = source("a/src/lib.rs", "let title = \"Lumen\";\n");
    assert_eq!(brand_literals(&shouting, "lumen").len(), 1, "any case");
    let alias = source(
        "a/src/lib.rs",
        "// `lumen`: the id saved before it was renamed.\nlet id = if id == \"lumen\" { \"x\" } else { id };\n",
    );
    assert!(brand_literals(&alias, "lumen").is_empty());
    let comment =
        source("a/src/lib.rs", "//! `lumen --snapshot out.png`\nlet a = 1; // \"lumen\"\n");
    assert!(brand_literals(&comment, "lumen").is_empty(), "comments aren't code");
    let far = source("a/src/lib.rs", "// an alias\n\n\n\n\nlet a = \"lumen\";\n");
    assert_eq!(brand_literals(&far, "lumen").len(), 1, "the comment just above, not anywhere");
}

#[test]
fn settings_are_declared_only_in_settings_rs() {
    let text = "pub const A: Setting<bool> = Setting::new(\"a\", \"b\", || true);\n";
    assert!(settings_outside(&source("m/src/settings.rs", text)).is_empty());
    assert_eq!(settings_outside(&source("m/src/view.rs", text)).len(), 1);
}

#[test]
fn locale_keys_named_in_the_code_must_exist() {
    let keys = locale_keys(
        "[tasks]\ntitle = \"Task Manager\"\n# note = \"x\"\n[console]\nerror_at = \"{message}\"\n",
    );
    assert!(keys.contains("tasks.title") && keys.contains("console.error_at"));
    assert!(!keys.contains("tasks.note"), "comments aren't keys");
    let code = source(
        "m/src/view.rs",
        "let a = t(\"tasks.title\");\nlet b = i18n::tr!(\"console.error_at\", x = 1);\nlet c = t(\"tasks.gone\");\nlet d = set(\"tasks.other\");\nlet e = t(\"Plain text\");\n",
    );
    let found = unknown_keys(&code, &keys);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("tasks.gone"));
}

/// A function let take a long list of arguments is refused; a mention in a comment isn't
/// one.
#[test]
fn long_argument_lists_are_refused() {
    let allowed = "#[allow(clippy::too_many_arguments)]\nfn f() {}\n";
    assert_eq!(long_arguments(&source("modules/x/src/a.rs", allowed)).len(), 1);
    let said = "// never clippy::too_many_arguments here\nfn f() {}\n";
    assert!(long_arguments(&source("modules/x/src/a.rs", said)).is_empty());
}
