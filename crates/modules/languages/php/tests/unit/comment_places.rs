use super::*;

/// What the place at `§` in `src` is (the marker taken out).
fn at(src: &str) -> Option<CommentPlace> {
    let offset = src.find('§').expect("a § in the snippet");
    place(&src.replacen('§', "", 1), offset)
}

fn class(name: &str) -> Option<CommentPlace> {
    Some(CommentPlace::Code(Target::Class(name.into())))
}

const TEXT: Option<CommentPlace> = Some(CommentPlace::Text);

/// Where `needle` starts in `src` once the marker is out.
fn offset_of(src: &str, needle: &str) -> usize {
    src.replacen('§', "", 1).find(needle).unwrap()
}

#[test]
fn outside_a_comment_nothing_is_said() {
    assert_eq!(at("<?php $user = new Us§er();"), None);
    assert_eq!(at("<?php $glob = 'src/*.php'; new Us§er();"), None, "`/*` in a string");
    assert_eq!(at("<?php /** @var User */ $u = new Us§er();"), None, "after a closed one");
    assert_eq!(at("<?php $u = '#'; new Us§er();"), None, "`#` in a string");
    assert_eq!(at("<?php #[Rou§te('/')]\nfunction f() {}"), None, "an attribute");
}

#[test]
fn plain_comments_and_doc_text_are_text() {
    assert_eq!(at("<?php // the run§time\n"), TEXT);
    assert_eq!(at("<?php # the run§time\n"), TEXT);
    assert_eq!(at("<?php /* the Run§time class */"), TEXT);
    assert_eq!(at("<?php\n/**\n * Starts the run§time.\n */\nfunction f() {}"), TEXT);
    assert_eq!(at("<?php\n/**\n * @param User $u the us§er to save\n */"), TEXT, "a description");
    assert_eq!(at("<?php\n/**\n * @deprecated use Ot§her\n */"), TEXT, "a text-only tag");
    assert_eq!(at("<?php\n/**\n * @para§m User $u\n */"), TEXT, "the tag's own name");
}

#[test]
fn the_types_after_the_tags_are_code() {
    assert_eq!(at("<?php\n/**\n * @param Us§er $u\n */"), class("User"));
    assert_eq!(
        at("<?php\n/**\n * @return \\App\\Mo§dels\\User\n */"),
        class("\\App\\Models\\User")
    );
    assert_eq!(at("<?php\n/** @var Us§er $u */"), class("User"));
    assert_eq!(at("<?php\n/**\n * @throws Runtime§Exception when\n */"), class("RuntimeException"));
    assert_eq!(at("<?php\n/**\n * @property-read Addr§ess $address\n */"), class("Address"));
    assert_eq!(at("<?php\n/**\n * @mixin Buil§der\n */"), class("Builder"));
    assert_eq!(at("<?php\n/**\n * @extends Fact§ory<User>\n */"), class("Factory"));
    assert_eq!(
        at("<?php\n/**\n * @phpstan-return Coll§ection<int, User>\n */"),
        class("Collection")
    );
    assert_eq!(at("<?php\n/**\n * @psalm-param list<Us§er> $u\n */"), class("User"));
    assert_eq!(at("<?php\n/**\n * @phpstan-assert-if-true Us§er $u\n */"), class("User"));
}

#[test]
fn every_name_of_a_type_s_form_is_code_and_the_rest_text() {
    let ty = |t: &str| at(&format!("<?php\n/**\n * @param {t} $x\n */"));
    assert_eq!(ty("?Us§er"), class("User"));
    assert_eq!(ty("User|Gu§est"), class("Guest"));
    assert_eq!(ty("A&Countab§le"), class("Countable"));
    assert_eq!(ty("array<int, Us§er>"), class("User"));
    assert_eq!(ty("Collection<int, array{id: int, owner?: Us§er}>"), class("User"));
    assert_eq!(ty("array{i§d: int}"), TEXT, "a shape's key");
    assert_eq!(ty("array{own§er?: User}"), TEXT, "an optional key");
    assert_eq!(ty("callable(Us§er): bool"), class("User"));
    assert_eq!(ty("Closure(User): Res§ult"), class("Result"));
    assert_eq!(ty("class-string<Mod§el>"), class("Model"));
    assert_eq!(ty("cla§ss-string<Model>"), TEXT, "a pseudo-type");
    assert_eq!(ty("i§nt"), TEXT, "built in");
    assert_eq!(ty("int<0, m§ax>"), TEXT);
    assert_eq!(ty("non-empty-li§st<User>"), TEXT);
    assert_eq!(ty("'act§ive'|'banned'"), TEXT, "a literal");
    assert_eq!(
        ty("Status::Act§ive"),
        Some(CommentPlace::Code(Target::Static { class: "Status".into(), name: "Active".into() }))
    );
    assert_eq!(ty("Sta§tus::ACTIVE"), class("Status"));
    assert_eq!(ty("Status::ST§ATE_*"), TEXT, "a wildcard");
}

#[test]
fn a_name_the_comments_declare_leads_to_its_declaration() {
    let src = "<?php\n/**\n * @template TModel of Model\n * @param class-string<TMo§del> $c\n */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "TModel of"))));
    // The class's template, used in its method's comment.
    let src = "<?php\n/**\n * @template TValue\n */\nclass Box {\n    /** @return TVal§ue */\n    public function get() {}\n}";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "TValue\n"))));
    // On the declaration itself, and its bound.
    let src = "<?php\n/**\n * @template TMo§del of Model\n */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "TModel"))));
    assert_eq!(at("<?php\n/**\n * @template T of Mod§el\n */"), class("Model"));
    // A type alias.
    let src = "<?php\n/**\n * @phpstan-type UserRow array{id: int}\n * @param UserR§ow $row\n */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "UserRow array"))));
    assert_eq!(at("<?php\n/**\n * @psalm-type Row = array{user: Us§er}\n */"), class("User"));
    let src = "<?php\n/**\n * @phpstan-import-type UserRow from Rep§ository\n */";
    assert_eq!(at(src), class("Repository"));
}

#[test]
fn a_parameter_s_name_leads_to_the_function_s_parameter() {
    let src = "<?php\n/**\n * @param User $us§er the one\n */\npublic function save(User $user, bool $flush) {}";
    let declared = offset_of(src, "$user,");
    assert_eq!(at(src), Some(CommentPlace::Declared(declared)));
    let src = "<?php\n/**\n * @param $fl§ush\n */\nfunction save(User $user, bool $flush) {}";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "$flush)"))));
    let src = "<?php\n/**\n * @param int $mis§sing\n */\nfunction save(User $user) {}";
    assert_eq!(at(src), TEXT, "not a parameter of it");
    assert_eq!(at("<?php\n/** @var User $us§er */\n$user = x();"), TEXT, "`@var`'s variable");
}

#[test]
fn a_magic_method_s_types_are_code_its_name_its_declaration() {
    let src = "<?php\n/**\n * @method static Buil§der where(string $column, mixed $value = null) the query\n */";
    assert_eq!(at(src), class("Builder"));
    let src = "<?php\n/**\n * @method static Builder wh§ere(string $column)\n */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "where("))));
    assert_eq!(at("<?php\n/**\n * @method void save(Us§er $u)\n */"), class("User"));
    assert_eq!(at("<?php\n/**\n * @method void save(User $§u)\n */"), TEXT);
    assert_eq!(at("<?php\n/**\n * @method void save(User $u = nu§ll)\n */"), TEXT);
    assert_eq!(at("<?php\n/**\n * @method save(User $u): Res§ult\n */"), class("Result"));
    assert_eq!(at("<?php\n/**\n * @method void save() and the de§scription\n */"), TEXT);
}

#[test]
fn references_name_classes_members_and_functions() {
    let member = |class: &str, name: &str| {
        Some(CommentPlace::Code(Target::Static { class: class.into(), name: name.into() }))
    };
    assert_eq!(at("<?php\n/**\n * @see Us§er\n */"), class("User"));
    assert_eq!(at("<?php\n/**\n * @see User::sa§ve()\n */"), member("User", "save"));
    assert_eq!(at("<?php\n/**\n * @see Us§er::save()\n */"), class("User"));
    assert_eq!(at("<?php\n/**\n * @see User::$na§me\n */"), member("User", "name"));
    assert_eq!(at("<?php\n/**\n * @uses Status::ACT§IVE\n */"), member("Status", "ACTIVE"));
    assert_eq!(
        at("<?php\n/**\n * @see hel§per()\n */"),
        Some(CommentPlace::Code(Target::Function("helper".into())))
    );
    assert_eq!(at("<?php\n/**\n * @covers \\App\\Sh§op\n */"), class("\\App\\Shop"));
    assert_eq!(at("<?php\n/**\n * @link https://exam§ple.com/doc\n */"), TEXT, "a link's own");
    assert_eq!(at("<?php\n/**\n * @see User and the te§xt\n */"), TEXT);
    // Inline.
    assert_eq!(at("<?php\n/**\n * Saves it, see {@see Us§er::save()} too.\n */"), class("User"));
    assert_eq!(at("<?php\n/**\n * Like {@link Repo§sitory}.\n */"), class("Repository"));
    assert_eq!(at("<?php\n/**\n * {@inheri§tDoc}\n */"), TEXT);
}

#[test]
fn a_test_s_provider_and_dependency_are_its_class_methods() {
    let this = |name: &str| {
        Some(CommentPlace::Code(Target::Member { receiver: Receiver::This, name: name.into() }))
    };
    assert_eq!(at("<?php\n/**\n * @dataProvider us§ers\n */"), this("users"));
    assert_eq!(at("<?php\n/**\n * @depends testCre§ate\n */"), this("testCreate"));
}

#[test]
fn an_annotation_names_its_class() {
    assert_eq!(at("<?php\n/**\n * @ORM\\Ent§ity\n */"), class("ORM\\Entity"));
    assert_eq!(at("<?php\n/**\n * @Rou§te(\"/home\", name=\"home\")\n */"), class("Route"));
    assert_eq!(at("<?php\n/**\n * @Route(\"/home\", na§me=\"home\")\n */"), TEXT);
}

#[test]
fn the_class_itself_and_its_parent() {
    // Inside the class's body: the words, which the language resolves to the class around.
    let src = "<?php\nclass Box {\n    /** @return sta§tic */\n    public function make() {}\n}";
    assert_eq!(at(src), class("self"));
    assert_eq!(
        at("<?php\nclass Box {\n    /** @return $th§is */\n    public function f() {}\n}"),
        class("self")
    );
    assert_eq!(
        at("<?php\nclass Box {\n    /** @return par§ent */\n    public function f() {}\n}"),
        class("parent")
    );
    // On the class's own comment: its name, the one it extends.
    let src = "<?php\n/**\n * @method static se§lf make()\n */\nfinal class Box extends Base {}";
    assert_eq!(at(src), class("Box"));
    let src = "<?php\n/**\n * @mixin pare§nt\n */\nclass Box extends Base {}";
    assert_eq!(at(src), class("Base"));
}

#[test]
fn inherit_doc_leads_to_the_parent_s_method() {
    let parent = |name: &str| {
        Some(CommentPlace::Code(Target::Static { class: "parent".into(), name: name.into() }))
    };
    let src = "<?php\nclass A extends B {\n    /**\n     * @inherit§Doc\n     */\n    public function save(): void {}\n}";
    assert_eq!(at(src), parent("save"));
    let src = "<?php\nclass A extends B {\n    /** {@inheri§tdoc} */\n    public static function make() {}\n}";
    assert_eq!(at(src), parent("make"));
    assert_eq!(at("<?php\n/** @inherit§Doc */\nclass A {}"), TEXT, "not on a method");
}

#[test]
fn covers_a_method_of_the_default_class() {
    let src = "<?php\n/**\n * @coversDefaultClass \\App\\Shop\n */\nclass ShopTest {\n    /** @covers ::to§tal */\n    public function testTotal() {}\n}";
    assert_eq!(
        at(src),
        Some(CommentPlace::Code(Target::Static {
            class: "\\App\\Shop".into(),
            name: "total".into()
        }))
    );
}

#[test]
fn an_annotation_s_arguments_name_classes() {
    let line = |args: &str| at(&format!("<?php\n/**\n * @ORM\\ManyToOne({args})\n */"));
    assert_eq!(line("targetEntity=\"App\\Entity\\Us§er\""), class("App\\Entity\\User"));
    assert_eq!(line("targetEntity=Us§er::class"), class("User"));
    assert_eq!(line("repositoryClass=\"Repo§sitory\""), class("Repository"));
    assert_eq!(line("inversedBy=\"ord§ers\""), TEXT);
    assert_eq!(line("targetEntity=\"User\", invers§edBy=\"orders\""), TEXT);
}

#[test]
fn code_between_backticks_is_code_in_any_comment() {
    let class = |n: &str| Some(CommentPlace::Code(Target::Class(n.to_owned())));
    let at = |text: &str| {
        let offset = text.find('|').unwrap();
        place(&text.replacen('|', "", 1), offset)
    };
    assert_eq!(at("<?php // the `Ca|rt` total"), class("Cart"));
    assert_eq!(at("<?php // the Ca|rt total"), Some(CommentPlace::Text), "no backticks: text");
    assert_eq!(
        at("<?php # see `Cart::to|tal()`"),
        Some(CommentPlace::Code(Target::Static { class: "Cart".into(), name: "total".into() }))
    );
    assert_eq!(
        at("<?php /* `for|mat($x)` */"),
        Some(CommentPlace::Code(Target::Function("format".into())))
    );
    assert_eq!(at("<?php /** Made by `new \\App\\Ca|rt` */"), class("\\App\\Cart"));
    assert_eq!(at("<?php // `$us|er`"), Some(CommentPlace::Text), "a variable");
    assert_eq!(at("<?php $a = '`Ca|rt`';"), None, "not a comment");
}
