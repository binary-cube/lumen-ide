use super::*;

/// What the place at `§` in `src` is (the marker taken out).
fn at(src: &str) -> Option<CommentPlace> {
    let offset = src.find('§').expect("a § in the snippet");
    place(&src.replacen('§', "", 1), offset)
}

fn class(name: &str) -> Option<CommentPlace> {
    Some(CommentPlace::Code(Target::Class(name.into())))
}

fn member(class: &str, name: &str) -> Option<CommentPlace> {
    Some(CommentPlace::Code(Target::Static { class: class.into(), name: name.into() }))
}

/// Where `needle` starts in `src` once the marker is out.
fn offset_of(src: &str, needle: &str) -> usize {
    src.replacen('§', "", 1).find(needle).unwrap()
}

const TEXT: Option<CommentPlace> = Some(CommentPlace::Text);

#[test]
fn outside_a_comment_nothing_is_said() {
    assert_eq!(at("const u = new Us§er();"), None);
    assert_eq!(at("const glob = 'src/*.js'; new Us§er();"), None, "`/*` in a string");
    assert_eq!(at("const t = `a // b`; new Us§er();"), None, "`//` in a template");
    assert_eq!(at("/** @type {User} */ const u = new Us§er();"), None);
}

#[test]
fn plain_comments_and_doc_text_are_text() {
    assert_eq!(at("// the run§time\n"), TEXT);
    assert_eq!(at("/* the Run§time */"), TEXT);
    assert_eq!(at("/**\n * Starts the run§time.\n */"), TEXT);
    assert_eq!(at("/**\n * @param {User} u the us§er\n */\nfunction f(u) {}"), TEXT);
    assert_eq!(at("/**\n * @depre§cated\n */"), TEXT);
    assert_eq!(at("/**\n * @fires Sa§ved\n */"), TEXT);
}

#[test]
fn the_types_in_braces_are_code() {
    let ty = |t: &str| at(&format!("/**\n * @param {{{t}}} x\n */\nfunction f(x) {{}}"));
    assert_eq!(ty("Us§er"), class("User"));
    assert_eq!(ty("?Us§er"), class("User"));
    assert_eq!(ty("User=§"), class("User"), "optional, right after the name");
    assert_eq!(ty("...Us§er"), class("User"));
    assert_eq!(ty("Array<Us§er>"), class("User"));
    assert_eq!(ty("Us§er[]"), class("User"));
    assert_eq!(ty("Promise<Res§ult>"), class("Result"));
    assert_eq!(ty("Record<string, Us§er>"), class("User"));
    assert_eq!(ty("{id: number, owner?: Us§er}"), class("User"));
    assert_eq!(ty("{i§d: number}"), TEXT, "a key");
    assert_eq!(ty("str§ing"), TEXT, "built in");
    assert_eq!(ty("'act§ive'|'banned'"), TEXT, "a literal");
    assert_eq!(ty("function(Us§er): boolean"), class("User"));
    assert_eq!(ty("import('./models').Us§er"), class("User"));
    assert_eq!(ty("import('./mod§els').User"), TEXT, "the path");
    let returns = at("/**\n * @returns {Promise<Us§er>} the user\n */");
    assert_eq!(returns, class("User"));
    assert_eq!(at("/** @type {Us§er} */"), class("User"));
    assert_eq!(at("/**\n * @throws {Http§Error}\n */"), class("HttpError"));
}

#[test]
fn a_name_the_comments_declare_leads_to_its_declaration() {
    let src = "/**\n * @typedef {Object} Point\n * @property {number} x\n */\n\n/** @param {Poi§nt} p */\nfunction draw(p) {}";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "Point\n"))));
    let src = "/**\n * @typedef {Object} Po§int\n */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "Point"))));
    let src = "/**\n * @callback Done\n */\n/** @param {Do§ne} cb */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "Done\n"))));
    let src = "/**\n * @template T, U\n * @param {§U} u\n */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "U\n"))));
    let src = "/**\n * @template {Model} T§M\n */";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "TM"))));
    assert_eq!(at("/**\n * @template {Mo§del} TM\n */"), class("Model"));
}

#[test]
fn a_parameter_s_name_leads_to_the_function_s_parameter() {
    let src = "/**\n * @param {User} us§er\n */\nfunction save(user, flush) {}";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "user, flush"))));
    let src = "/**\n * @param {boolean} [flu§sh=false]\n */\nconst save = (user, flush) => {};";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "flush) =>"))));
    let src = "/**\n * @param {Object} opt§ions.size\n */\nfunction open(options) {}";
    assert_eq!(at(src), Some(CommentPlace::Declared(offset_of(src, "options) {"))));
    let src = "/**\n * @param {Object} options.si§ze\n */\nfunction open(options) {}";
    assert_eq!(at(src), TEXT, "a property of it");
    assert_eq!(at("/**\n * @param {User} mis§sing\n */\nfunction save(user) {}"), TEXT);
}

#[test]
fn references_and_links_name_classes_and_members() {
    assert_eq!(at("/**\n * @see Us§er\n */"), class("User"));
    assert_eq!(at("/**\n * @see User#sa§ve\n */"), member("User", "save"));
    assert_eq!(at("/**\n * @see Us§er.create\n */"), class("User"));
    assert_eq!(at("/**\n * @see User.cre§ate()\n */"), member("User", "create"));
    assert_eq!(at("/**\n * @see https://exam§ple.com\n */"), TEXT);
    assert_eq!(at("/**\n * @see module:foo~Ba§r\n */"), TEXT);
    assert_eq!(at("/**\n * Like {@link Repo§sitory}.\n */"), class("Repository"));
    assert_eq!(at("/**\n * See {@linkcode User#sa§ve}.\n */"), member("User", "save"));
    assert_eq!(at("/**\n * {@li§nk User}\n */"), TEXT, "the tag itself");
    assert_eq!(at("/**\n * @extends Ba§se\n */"), class("Base"));
    assert_eq!(at("/**\n * @augments {Ba§se}\n */"), class("Base"));
    assert_eq!(at("/**\n * @implements {Sha§pe}\n */"), class("Shape"));
}

#[test]
fn code_between_backticks_is_code_in_any_comment() {
    let at = |text: &str| {
        let offset = text.find('|').unwrap();
        place(&text.replacen('|', "", 1), offset)
    };
    assert_eq!(at("// uses `Ca|rt`"), Some(CommentPlace::Code(Target::Class("Cart".into()))));
    assert_eq!(
        at("/* `for|mat(x)` */"),
        Some(CommentPlace::Code(Target::Function("format".into())))
    );
    assert_eq!(
        at("/** See `Cart#to|tal` */"),
        Some(CommentPlace::Code(Target::Static { class: "Cart".into(), name: "total".into() }))
    );
    assert_eq!(at("// `this.it|ems`"), Some(CommentPlace::Text));
    assert_eq!(at("// uses Ca|rt"), Some(CommentPlace::Text));
}
