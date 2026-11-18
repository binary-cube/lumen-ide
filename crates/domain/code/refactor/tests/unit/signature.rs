use super::*;

/// PHP-like parameters, enough for the tests: `Type $name = default`, named arguments.
#[derive(Debug)]
struct Php;

impl SignatureSyntax for Php {
    fn calls(&self) -> CallRules {
        CallRules {
            member_access: &["->", "::"],
            declares: &["function"],
            constructs: Some("new"),
            sigil: Some('$'),
        }
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["php"]
    }
    fn parse(&self, text: &str) -> Param {
        let (head, default) = match text.split_once('=') {
            Some((h, d)) => (h.trim(), d.trim().to_owned()),
            None => (text.trim(), String::new()),
        };
        let (type_hint, name) = match head.rsplit_once(' ') {
            Some((t, n)) => (t.to_owned(), n),
            None => (String::new(), head),
        };
        let variadic = name.starts_with("...");
        let name = name.trim_start_matches("...").trim_start_matches('$').to_owned();
        Param { prefix: String::new(), type_hint, name, default, variadic, by_ref: false }
    }
    fn render(&self, p: &Param) -> String {
        let mut out = String::new();
        if !p.type_hint.is_empty() {
            out.push_str(&p.type_hint);
            out.push(' ');
        }
        if p.variadic {
            out.push_str("...");
        }
        out.push('$');
        out.push_str(&p.name);
        if !p.default.is_empty() {
            out.push_str(" = ");
            out.push_str(&p.default);
        }
        out
    }
    fn named_argument<'a>(&self, arg: &'a str) -> Option<(&'a str, &'a str)> {
        let (name, value) = arg.split_once(':')?;
        (!value.starts_with(':') && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .then(|| (name.trim(), value.trim()))
    }
    fn render_named(&self, name: &str, value: &str) -> Option<String> {
        Some(format!("{name}: {value}"))
    }
}

fn params(list: &str) -> Vec<Param> {
    list.split(", ").filter(|s| !s.is_empty()).map(|p| Php.parse(p)).collect()
}

fn kept(old: &[Param], from: usize) -> NewParam {
    NewParam::Kept { from, param: old[from].clone() }
}

#[test]
fn lists_split_on_their_own_commas() {
    let text = r#"f($a, g($b, $c), [1, 2], "x, y", 'it\'s', /* , */ $d)"#;
    let (inside, items) = split_list(text, 1).unwrap();
    assert_eq!(&text[inside], &text[2..text.len() - 1]);
    let items: Vec<&str> = items.iter().map(|r| &text[r.clone()]).collect();
    assert_eq!(items, ["$a", "g($b, $c)", "[1, 2]", r#""x, y""#, r"'it\'s'", "/* , */ $d"]);
    assert_eq!(split_list("f()", 1).unwrap().1.len(), 0);
}

#[test]
fn calls_follow_reordered_removed_and_added_parameters() {
    let old = params("int $a, string $b, bool $c = false");
    // (b, a, new $d), c removed.
    let new = vec![
        kept(&old, 1),
        kept(&old, 0),
        NewParam::Added { param: Php.parse("?int $d = null"), value: "42".into() },
    ];
    assert_eq!(rewrite_args(&["1", "'x'", "true"], &old, &new, &Php), "'x', 1, 42");
    // No value for the calls: the new parameter's default — left out at the end.
    let new = vec![
        kept(&old, 0),
        NewParam::Added { param: Php.parse("int $d = 0"), value: String::new() },
    ];
    assert_eq!(rewrite_args(&["1", "'x'"], &old, &new, &Php), "1");
}

#[test]
fn gaps_get_defaults_or_names() {
    let old = params("int $a, int $b = 5, int $c = 7");
    // `c` moved first; a call passing only `a` keeps `b`'s default where it's needed.
    let new = vec![kept(&old, 0), kept(&old, 1), kept(&old, 2)];
    assert_eq!(rewrite_args(&["1", "c: 9"], &old, &new, &Php), "1, 5, 9");
    let old = params("int $a, int $b");
    let new = vec![
        kept(&old, 0),
        NewParam::Added { param: Php.parse("int $x"), value: String::new() },
        kept(&old, 1),
    ];
    // `x` has neither a value nor a default: `b` is passed by name.
    assert_eq!(rewrite_args(&["1", "2"], &old, &new, &Php), "1, b: 2");
}

#[test]
fn variadic_extras_stay_at_the_end() {
    let old = params("string $f, ...$rest");
    let new = vec![
        NewParam::Added { param: Php.parse("bool $strict"), value: "true".into() },
        kept(&old, 0),
        kept(&old, 1),
    ];
    assert_eq!(rewrite_args(&["'%s'", "$a", "$b"], &old, &new, &Php), "true, '%s', $a, $b");
}

#[test]
fn declarations_keep_their_layout() {
    let old = params("int $a, int $b");
    let new = vec![kept(&old, 1), kept(&old, 0)];
    assert_eq!(render_params(&new, "int $a, int $b", &Php), "int $b, int $a");
    let multi = "\n        int $a,\n        int $b,\n    ";
    assert_eq!(render_params(&new, multi, &Php), "\n        int $b,\n        int $a,\n    ");
}

#[test]
fn finds_calls_and_plans_their_new_arguments() {
    use std::collections::HashMap;
    struct Files(HashMap<PathBuf, String>);
    impl Sources for Files {
        fn candidates(&self) -> Vec<PathBuf> {
            let mut p: Vec<PathBuf> = self.0.keys().cloned().collect();
            p.sort();
            p
        }
        fn read(&self, path: &Path) -> Option<String> {
            self.0.get(path).cloned()
        }
    }
    struct All;
    impl Resolver for All {
        fn resolve(&self, _: &Path, _: &str, _: Range<usize>) -> Resolution {
            Resolution::Target
        }
    }
    let decl = "<?php function send(string $to, string $body) {}\nsend('a', 'b');";
    let other = "<?php send($x, $y); $o->send(1); function send2() {}";
    let files = Files(HashMap::from([
        ("/p/a.php".into(), decl.to_owned()),
        ("/p/b.php".into(), other.to_owned()),
    ]));
    let open = decl.find("(string").unwrap();
    let (inside, items) = split_list(decl, open).unwrap();
    let old: Vec<Param> = items.iter().map(|r| Php.parse(&decl[r.clone()])).collect();
    let target = SignatureTarget {
        name: "send".into(),
        kind: CallKind::Function,
        path: "/p/a.php".into(),
        params: inside,
        old: old.clone(),
    };
    let calls = find_calls(&target, &Php, &files, &All, &|| false);
    let counts: Vec<usize> = calls.files.iter().map(|(_, c)| c.len()).collect();
    assert_eq!(counts, [1, 1], "the declaration and `->send` aren't function calls");
    let new = vec![kept(&old, 1), kept(&old, 0)];
    let plan = calls.plan(&target, &new, &Php, &|_, _| None);
    let edits = plan.edits();
    let a = crate::apply_edits(decl, &edits[0].edits).unwrap();
    assert_eq!(a, "<?php function send(string $body, string $to) {}\nsend('b', 'a');");
    let b = crate::apply_edits(other, &edits[1].edits).unwrap();
    assert_eq!(b, "<?php send($y, $x); $o->send(1); function send2() {}");
}
