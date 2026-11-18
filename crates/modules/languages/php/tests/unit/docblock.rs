use super::*;

const CODE: &str = "<?php\nclass A {\n    /**\n     * @param string $email\n     * @param |\n     */\n    public function __construct(\n        public string $email,\n        #[Sensitive] private ?int $age = null,\n        array ...$tags,\n    ) {}\n}";

fn at(marker: &str) -> (String, usize) {
    let i = CODE.find(marker).unwrap();
    (CODE.replacen(marker, "", 1), i)
}

#[test]
fn tags_types_and_params() {
    let text = "/**\n * @pa";
    let ctx = context(text, text.len()).unwrap();
    assert_eq!(ctx.kind, ContextKind::Vocabulary(TAG));
    assert_eq!(ctx.prefix, "@pa");

    let text = "/**\n * @param stri";
    let ctx = context(text, text.len()).unwrap();
    assert_eq!((ctx.kind, ctx.prefix.as_str()), (ContextKind::TypeName, "stri"));

    let text = "/**\n * @return string|Us";
    assert_eq!(context(text, text.len()).unwrap().prefix, "Us");

    let (text, offset) = at("|");
    let ctx = context(&text, offset).unwrap();
    assert_eq!(ctx.kind, ContextKind::TypeName, "the first word is the type");
    let text2 = format!("{}int $", &text[..offset]) + &text[offset..];
    let ctx = context(&text2, offset + 5).unwrap();
    assert_eq!(ctx.kind, ContextKind::Vocabulary(PARAM));
    let labels: Vec<String> = vocabulary(&ctx, &text2).into_iter().map(|i| i.label).collect();
    assert_eq!(labels, ["$age", "$tags"], "no $email, already documented");
}

#[test]
fn stub_from_signature() {
    let text = "/**\n * \n */\nfunction send(User $to, ?string $subject = null, int &$n, ...$rest): bool {}";
    assert_eq!(
        stub(text, 5),
        [
            "@param User        $to",
            "@param string|null $subject",
            "@param int         $n",
            "@param mixed[]     $rest",
            "@return bool",
        ]
    );
    let text = "/**\n * \n */\nprivate ?Foo $foo = null;";
    assert_eq!(stub(text, 5), ["@var Foo|null"]);
    let text = "/**\n * \n */\nclass A {}";
    assert!(stub(text, 5).is_empty());
}
