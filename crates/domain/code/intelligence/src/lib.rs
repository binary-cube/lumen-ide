//! The Intelligence context: symbols in code, the project index, completion while
//! typing and "go to definition".
//!
//! The domain can't parse: each language module contributes a
//! [`LanguageIntelligence`] (symbol extraction, the context at the cursor). Here are
//! the shared rules: name resolution, index lookup, ranking of suggestions.

mod assist;
pub mod calls;
pub mod code_style;
pub mod comments;
mod css;
mod diagram;
mod docs;
mod engine;
pub mod env;
mod framework;
mod fuzzy;
mod graph;
pub mod hierarchy;
mod index;
pub mod inlay;
pub mod levels;
pub mod links;
mod outline;
mod query;
pub mod rename;
pub mod servers;
mod signature;
mod symbols;
pub mod template;
pub mod usages;
mod vocabulary;

pub use assist::{AssistContext, AssistPlace, AssistTarget, DocumentAssist};
pub use css::{CssReferences, PlaceMatch, css_match, is_css_kind, place_match};
pub use diagram::{
    ClassDiagram, DiagramEdge, DiagramMember, DiagramNode, DiagramRole, Relation, class_diagram,
};
pub use docs::{DocBlock, DocParam, DocRules, doc_before};
pub use engine::{
    Project, complete, definition, definition_of, hover, hover_of, signature_plainly,
    split_param_plainly,
};
pub use framework::{
    Framework, FrameworkCompletion, FrameworkContext, FrameworkName, FrameworkProblem,
    FrameworkSupport, ProblemLevel, active_frameworks, attribute_name_at, before_string,
    choose_framework, component_tag_at, has_dependency, line_of, pascal_case, path_in_quotes,
    quoted_at, suggest, watched,
};
pub use fuzzy::fuzzy_score;
pub use graph::{
    CatalogEntry, CommentReader, DepGraph, EdgeKind, GraphEdge, GraphFile, GraphSymbol,
    SourceRules, build_graph, catalog,
};
pub use index::{Location, SymbolIndex, Unpack, library_files_held};
pub use outline::{OutlineItem, label, outline, scope_at};
pub use query::{SymbolHit, SymbolQuery, search_symbols};
pub use signature::{
    Call, SignatureHelp, active_parameter, enclosing_call, enclosing_call_in, multiline_signature,
    parameters, signature_help,
};
pub use symbols::{
    FileSymbols, Symbol, SymbolKind, UseImport, Visibility, is_name_char, short_name,
};
pub use vocabulary::Vocabulary;

use std::fmt::Debug;
use std::ops::Range;

use kernel::LanguageId;

/// What the user is typing at the cursor (for completion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    /// The text to be replaced by the chosen suggestion (the typed prefix).
    pub replace: Range<usize>,
    pub prefix: String,
    pub kind: ContextKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextKind {
    /// `$object->|`: the object's members.
    Member(Receiver),
    /// `Class::|`: static members and constants (the class name, as in the code).
    Static(String),
    /// A method's name written in a string (PHP's callables, `[$this, '|']`): the
    /// receiver's methods, static ones too, inserted as bare names.
    Methods(Receiver),
    /// `$|`: local variables.
    Variable,
    /// Any name: classes, functions, keywords.
    Name,
    /// A type name (after `new`, `extends`, `implements`, `instanceof`): classes only.
    TypeName,
    /// Symbols of certain kinds from the whole project, in any language (e.g. in HTML,
    /// `class="…"` looks up CSS classes).
    Symbols(Vec<SymbolKind>),
    /// A file path (e.g. in `include '…'`): the entries of directory `dir` (as in the code,
    /// relative to the current file).
    Path(String),
    /// Only the language's vocabulary (e.g. CSS properties, HTML tags), with a label for
    /// the position that the language uses in [`LanguageIntelligence::vocabulary`].
    Vocabulary(&'static str),
    /// In comments, strings or without a prefix: no suggestions.
    None,
}

/// The object to the left of `->`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receiver {
    This,
    /// The inferred type (the class name, as in the code).
    Class(String),
    Unknown,
}

/// What is under the cursor, for "go to definition".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Class(String),
    Function(String),
    Member {
        receiver: Receiver,
        name: String,
    },
    Static {
        class: String,
        name: String,
    },
    /// A symbol of the given kinds, looked up by name across the whole project.
    Named {
        name: String,
        kinds: Vec<SymbolKind>,
    },
    /// An included file (`include 'x.php'`), with the path as in the code (relative to the
    /// current file; `/…` after `__DIR__ .`).
    File(String),
}

/// What a place inside a comment is, as its language reads its comments (plan 75): only
/// what's written there as code leads somewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentPlace {
    /// Plain text: no underline, no card, no definition.
    Text,
    /// A name written as in code (a type, a member, a function), resolved as code's are.
    Code(Target),
    /// A name the comment or its declaration declares, at this offset of the text: a
    /// template's `T`, a type alias, the parameter a `@param $x` documents.
    Declared(usize),
}

/// What "Move Statement" (⌘⇧↑ / ⌘⇧↓) moves, by the language's structure. Line ranges are
/// 0-based, end exclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementMove {
    /// The whole statement / declaration / element the lines are in (`unit`, its doc
    /// comment included) trades places with its sibling `past`.
    Past { unit: std::ops::Range<usize>, past: std::ops::Range<usize> },
    /// It's the first / last of its block: it stays.
    Stay,
    /// The language doesn't say: the lines move alone ("Move Line").
    Lines,
}

/// How a completed keyword is inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordInsert {
    /// Followed by a space (`function `, `return `).
    Spaced,
    /// As written (`true`, `null`).
    Exact,
    /// As a call: `isset()`, the cursor between the parentheses.
    Call,
}

/// How a language writes the names a refactoring renames (the rules beyond the index's
/// declarations). The default: plain identifiers, nothing implicit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Naming {
    /// What variables and properties are written after (`$` in PHP): a declaration's name
    /// may follow it at its offset.
    pub sigil: Option<char>,
    /// The object a method runs on (`$this`): never renamed.
    pub receiver: Option<&'static str>,
    /// A name written with the sigil that nothing declares is a local variable, scoped to
    /// its function (PHP declares variables by assigning them).
    pub implicit_locals: bool,
    /// A constructor's name (`__construct`, `constructor`).
    pub constructor: Option<&'static str>,
    /// Method names ignore case (PHP).
    pub methods_ignore_case: bool,
    /// Declared names clash whatever their case (PHP's types, functions and methods): a new
    /// name differing only in case is taken.
    pub names_ignore_case: bool,
    /// What separates a qualified name's namespace from its name (`\` in PHP's
    /// `App\Models\User`).
    pub namespace_separator: Option<char>,
    /// The extension of files holding one type named after them (PSR-4's `php`): renaming
    /// the type renames the file, and the other way round.
    pub type_per_file: Option<&'static str>,
    /// Names for the type being declared, written inside it (`self`, `static`).
    pub own_type: &'static [&'static str],
    /// The name for its parent type, written inside it (`parent`).
    pub parent_type: Option<&'static str>,
    /// A type's own name through its static access (`User::class` in PHP), offered among
    /// its static members.
    pub type_name_member: Option<&'static str>,
    /// What makes an object of a type (`new` in `new User(`): its hover shows the
    /// constructor.
    pub construct: Option<&'static str>,
    /// A parameter's modifiers not part of its type (PHP's constructor promotion: `public`,
    /// `readonly`…): left out of the signature shown.
    pub parameter_modifiers: &'static [&'static str],
}

impl Naming {
    /// Plain identifiers, nothing implicit (the default, usable in constants).
    pub const PLAIN: Self = Self {
        sigil: None,
        receiver: None,
        implicit_locals: false,
        constructor: None,
        methods_ignore_case: false,
        names_ignore_case: false,
        namespace_separator: None,
        type_per_file: None,
        own_type: &[],
        parent_type: None,
        type_name_member: None,
        construct: None,
        parameter_modifiers: &[],
    };

    /// `name` is the constructor's.
    pub fn is_constructor(&self, name: &str) -> bool {
        self.constructor.is_some_and(|c| {
            if self.methods_ignore_case { c.eq_ignore_ascii_case(name) } else { c == name }
        })
    }

    /// A qualified name's own part (`User` of `App\Models\User`).
    pub fn short_name<'a>(&self, fqn: &'a str) -> &'a str {
        self.namespace_separator.and_then(|s| fqn.rsplit_once(s)).map_or(fqn, |(_, name)| name)
    }

    /// A qualified name's namespace (`App\Models` of `App\Models\User`), if it has one.
    pub fn namespace_of<'a>(&self, fqn: &'a str) -> Option<&'a str> {
        fqn.rsplit_once(self.namespace_separator?).map(|(namespace, _)| namespace)
    }

    /// `name` written fully qualified (`\Exception`): without its leading separator.
    pub fn absolute<'a>(&self, name: &'a str) -> Option<&'a str> {
        name.strip_prefix(self.namespace_separator?)
    }
}

/// What a language's type inference works with beyond the text: the project's declarations
/// and where the document is among them.
#[derive(Debug, Clone, Copy)]
pub struct Types<'a> {
    pub index: &'a SymbolIndex,
    pub path: &'a std::path::Path,
}

/// What [`LanguageIntelligence::check`] finds in a document knowing the project: problems
/// (an unknown class, a call's arguments) and uses of deprecated declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    pub problems: Vec<Problem>,
    pub deprecated: Vec<Deprecated>,
}

/// A problem found knowing the project (shown as a warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub range: std::ops::Range<usize>,
    /// What it is (`undefined_class`, `too_few_arguments`…), translated by the app.
    pub kind: &'static str,
    /// What it's about, shown as code next to the message (`MissingClass`, `save()`).
    pub name: String,
    /// The arguments a call expects and those it was given, for the argument counts.
    pub arguments: Option<(usize, usize)>,
}

/// What [`LanguageIntelligence::analyse`] finds in a document's version.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentAnalysis {
    pub symbols: FileSymbols,
    pub issues: Vec<LevelIssue>,
    pub deprecated: Vec<Deprecated>,
    pub unused: Vec<Unused>,
    pub requirements: Vec<Requirement>,
}

/// What a language's intelligence offers the editor beyond completion: the editor's
/// right-click menu shows a row only where its language offers it (no Rename in a SQL
/// console, no Change Signature in a stylesheet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageFeature {
    /// Going to a name's declaration and finding its usages.
    Navigation,
    /// Renaming a name everywhere it's used.
    Rename,
    /// Changing a function's parameters with its calls.
    ChangeSignature,
    /// Moving its files as a refactoring (what refers to them following).
    Move,
    /// A type's diagram (its parents, members and the types it uses).
    TypeDiagram,
}

impl LanguageFeature {
    /// Every feature (a language with the whole set).
    pub const ALL: &'static [Self] =
        &[Self::Navigation, Self::Rename, Self::ChangeSignature, Self::Move, Self::TypeDiagram];
    /// Names resolved and renamed, nothing more (the default).
    pub const NAMES: &'static [Self] = &[Self::Navigation, Self::Rename];
}

/// A language's intelligence, contributed by its module.
pub trait LanguageIntelligence: Send + Sync + Debug {
    fn language(&self) -> LanguageId;
    /// What it offers the editor (its menu's rows): by default its names resolved and
    /// renamed ([`LanguageFeature::NAMES`]).
    fn features(&self) -> &'static [LanguageFeature] {
        LanguageFeature::NAMES
    }
    /// Extensions of the files to index.
    fn extensions(&self) -> &[&'static str];
    /// The declarations in the file (namespace, imports, classes, members, functions).
    fn extract(&self, text: &str) -> FileSymbols;
    fn context(&self, text: &str, offset: usize) -> CompletionContext;
    fn target(&self, text: &str, offset: usize) -> Option<Target>;
    /// What `offset` is in one of the language's comments (its doc tags' types, a name the
    /// comment declares, or text); `None` outside a comment, or when the language doesn't
    /// tell (then the comment is read as code, as [`Self::target`] reads it).
    fn comment_place(&self, _text: &str, _offset: usize) -> Option<CommentPlace> {
        None
    }
    /// The variables visible at `offset` (without `$`).
    fn locals(&self, text: &str, offset: usize) -> Vec<String>;
    fn keywords(&self) -> &[&'static str];
    /// The keywords offered in file `path`: a language with dialects tells them by the
    /// file's name (SQL's `.pgsql`, `x.sqlite.sql`); by default [`keywords`](Self::keywords).
    fn keywords_for(&self, path: &std::path::Path) -> std::borrow::Cow<'_, [&'static str]> {
        let _ = path;
        std::borrow::Cow::Borrowed(self.keywords())
    }
    /// Where the name at `offset` is declared in this same text, by the language's scope
    /// rules (locals, parameters, nested functions); `None` leaves it to the project index.
    /// Checked first: the nearest declaration shadows the project's.
    fn local_definition(&self, _text: &str, _offset: usize) -> Option<usize> {
        None
    }

    /// "Move Statement": what moves when lines `lines` go up / down — the language knows
    /// what a statement, a declaration, an element is, and that one doesn't leave its block.
    fn statement_move(
        &self,
        _text: &str,
        _lines: std::ops::Range<usize>,
        _up: bool,
    ) -> StatementMove {
        StatementMove::Lines
    }

    /// The extensions of the files a symbol of this language (of `kind`) can be used in, for
    /// "Find Usages" (the language's own and those embedding it); none: not searched.
    fn usage_extensions(&self, _kind: SymbolKind) -> &'static [&'static str] {
        &[]
    }

    /// The characters, besides a name's, that may start a suggestion as they're typed (`$`,
    /// `->`'s `>`, `::`'s `:`, `.`, `<`, quotes…); others close the list.
    fn completion_triggers(&self) -> &'static str {
        ""
    }

    /// How its files refer to CSS classes and ids (attributes, calls, a stylesheet's
    /// selectors), for telling a CSS name's usages apart; `None`: they don't.
    fn css_references(&self) -> Option<crate::CssReferences> {
        None
    }

    /// Whether the name at `range` in `text` (one of its files) is the `kind` symbol searched
    /// for, by its place alone (CSS's classes, ids and custom properties, as
    /// [`css_references`](Self::css_references) says); `None`: `kind` is resolved through
    /// its definitions.
    fn place_match(
        &self,
        text: &str,
        range: &std::ops::Range<usize>,
        kind: SymbolKind,
    ) -> Option<crate::PlaceMatch> {
        crate::place_match(text, range, kind, self.css_references().as_ref())
    }

    /// What a name holds besides letters, digits and `_` (`-` in CSS, `$` in JavaScript,
    /// `$` and `\\` in PHP's qualified names): the word under the mouse.
    fn word_chars(&self) -> &'static str {
        ""
    }

    /// How the language writes the names a rename changes.
    fn naming(&self) -> Naming {
        Naming::default()
    }

    /// What follows a parameter's name in a named argument (`name:` in PHP 8), if the
    /// language has them: completion offers them in a call's arguments, the signature help
    /// follows them.
    fn named_argument(&self) -> Option<&'static str> {
        None
    }

    /// Whether the type named at `word_start` is being made here (its constructor runs):
    /// after the language's `new` by default; PHP's attributes (`#[AsCommand(`) too.
    fn constructs_at(&self, text: &str, word_start: usize) -> bool {
        let before = text.get(..word_start).unwrap_or("").trim_end();
        self.naming().construct.is_some_and(|new| before.ends_with(new))
    }

    /// How keyword `keyword` is inserted when completed: followed by a space (by default),
    /// exactly (`true`), or as a call (`isset()`, the cursor between the parentheses).
    fn keyword_insert(&self, keyword: &str) -> KeywordInsert {
        let _ = keyword;
        KeywordInsert::Spaced
    }

    /// The name at `range` in `text` can be a symbol of `kind` by its surroundings (a method
    /// after `->` / `::`, a property not a `$variable`…): `false` rules it out before asking
    /// the navigation. The default: a type can't follow a member access nor the sigil (its
    /// [`Self::source_rules`]).
    fn fits_kind(&self, text: &str, range: &Range<usize>, kind: SymbolKind) -> bool {
        fits_kind_plainly(text, range, kind, self.source_rules().as_ref())
    }

    /// A declaration's signature on one line, for the hover card (`function name(…)`,
    /// `class Name`); `detail` is its parameters / type. The default: [`signature_plainly`].
    fn signature(&self, symbol: &Symbol, detail: Option<&str>) -> String {
        signature_plainly(symbol, detail)
    }

    /// A parameter of a signature as its type and name. The default: [`split_param_plainly`]
    /// (`name: Type = 1` → `Type`, `name`).
    fn split_param(&self, param: &str) -> (Option<String>, String) {
        split_param_plainly(param)
    }

    /// How its code points at other files and declarations, for the dependency graph (the
    /// canvas). The default: none (its files show only their declarations).
    fn source_rules(&self) -> Option<SourceRules> {
        None
    }

    /// The type names in a signature's parameters (`detail`: `(Db $db, ?Clock $c = null)` →
    /// `Db`, `Clock`), as written (resolved by the caller); scalars and pseudo-types left
    /// out. The default: none.
    fn parameter_types(&self, _detail: &str) -> Vec<String> {
        Vec::new()
    }

    /// The type written for the variable at `offset` (`$cart` after `$cart = new Cart`, a
    /// parameter's hint, a `@var`), as written; `None` if the language doesn't say.
    fn type_at(&self, _text: &str, _offset: usize) -> Option<String> {
        None
    }

    /// The files included by the text (`include` / `require`), with paths as in [`Target::File`].
    fn includes(&self, _text: &str) -> Vec<String> {
        Vec::new()
    }

    /// The files in which type `fqn` should be declared according to the project's
    /// conventions (e.g. PSR-4 from `composer.json`), when the index doesn't have it.
    fn locate(&self, _project: &Project<'_>, _fqn: &str) -> Vec<std::path::PathBuf> {
        Vec::new()
    }

    /// Where and what to insert to import type `fqn` in `text` (e.g. a `use` line),
    /// or `None` if the language has no imports.
    fn import_edit(&self, _text: &str, _fqn: &str) -> Option<(usize, String)> {
        None
    }

    /// Text inserted automatically at the cursor when `typed` is typed at `offset` (already in
    /// the text): `</div>` on `>` after `<div>` (the cursor before it), `php` and a new line on
    /// `?` after `<` (the cursor after it). Asked of the language at the cursor, then of the
    /// file's own (outside its code, a PHP file's text is HTML).
    fn on_type(&self, _text: &str, _offset: usize, _typed: char) -> Option<TypedInsert> {
        None
    }

    /// The characters [`on_type`](Self::on_type) answers to (the others never reach it, so
    /// the document isn't copied for them).
    fn on_type_triggers(&self) -> &'static [char] {
        &[]
    }

    /// The language's fixed suggestions for the context (CSS properties, HTML tags…).
    fn vocabulary(&self, _context: &CompletionContext, _text: &str) -> Vec<CompletionItem> {
        Vec::new()
    }
    /// The language levels (versions), ascending, e.g. `["7.4", "8.0", …]`; empty = the language
    /// has no versions. The project picks one (see [`Project::level`]), and suggestions
    /// introduced later ([`LanguageIntelligence::since`]) are no longer shown.
    fn levels(&self) -> &[&'static str] {
        &[]
    }

    /// The level used by the project, inferred from its configuration (e.g. `require.php` in
    /// `composer.json`) or from its environment (e.g. the installed interpreter) as
    /// [`Self::warm`] found it. Called on the UI thread: it never waits for a program.
    fn detect_level(&self, _project: &Project<'_>) -> Option<DetectedLevel> {
        None
    }

    /// The slow look at the environment (running the installed interpreter for its
    /// version), done once on a background thread at startup; [`Self::detect_level`]
    /// answers from it afterwards.
    fn warm(&self) {}

    /// The features in `text` newer than `level` (the language's own version string).
    fn level_issues(&self, _text: &str, _level: &str) -> Vec<LevelIssue> {
        Vec::new()
    }

    /// What the text uses that is deprecated (an HTML element or attribute, a CSS
    /// property, a DOM API), from the language's data. Runs off the UI thread.
    fn deprecated(&self, _text: &str) -> Vec<Deprecated> {
        Vec::new()
    }

    /// What the text needs the project to declare in its manifest (see
    /// [`LanguageIntelligence::manifest`]), e.g. the PHP extensions it uses. Runs off the
    /// UI thread; the app checks them against the manifest.
    fn requirements(&self, _text: &str) -> Vec<Requirement> {
        Vec::new()
    }

    /// The project file declaring what the code needs (`composer.json`), if any.
    fn manifest(&self) -> Option<&'static str> {
        None
    }

    /// What the text declares but never uses (imports, private members). Runs off the UI
    /// thread.
    fn unused(&self, _text: &str) -> Vec<Unused> {
        Vec::new()
    }

    /// Everything asked of an open document's version at once — its declarations and its
    /// analyses (`level`: the features newer than it) — off the UI thread. A language that
    /// parses answers it from one parse; by default, each asked on its own.
    fn analyse(&self, text: &str, level: Option<&str>) -> DocumentAnalysis {
        DocumentAnalysis {
            symbols: self.extract(text),
            issues: level.map(|level| self.level_issues(text, level)).unwrap_or_default(),
            deprecated: self.deprecated(text),
            unused: self.unused(text),
            requirements: self.requirements(text),
        }
    }

    /// The type (its full name) of the object a member access is made on — `offset` in or
    /// right after the member's name, after the language's member access (`->`) — worked
    /// out knowing the project (`types`): a variable's assignments, the return and property
    /// types of the declarations, generics… `None`: the context's own [`Receiver`] is used.
    fn receiver_type(&self, _text: &str, _offset: usize, _types: &Types<'_>) -> Option<String> {
        None
    }

    /// Every type (full names) the object of a member access can be — a union's
    /// (`$flag ? new A() : new B()`), the first one the most likely —; by default the one
    /// [`Self::receiver_type`] gives.
    fn receiver_types(&self, text: &str, offset: usize, types: &Types<'_>) -> Vec<String> {
        self.receiver_type(text, offset, types).into_iter().collect()
    }

    /// The type (its full name) declaring the member named at `offset` (after the language's
    /// member or static access), knowing the project: an ancestor's, one the language
    /// forwards to (PHP's `@mixin`, `__callStatic`). `None`: looked up among the receiver's
    /// members as the index has them.
    fn member_owner(&self, _text: &str, _offset: usize, _types: &Types<'_>) -> Option<String> {
        None
    }

    /// The members of type `class` (a full name) that the language provides itself — no
    /// project file declares them: a language's built-in classes, what every enum has —
    /// for it and its ancestors, as symbols without a file.
    fn builtin_members(&self, _class: &str, _types: &Types<'_>) -> Vec<Symbol> {
        Vec::new()
    }

    /// What's under the mouse when it isn't a project declaration, knowing the project: a
    /// variable (its type and where it comes from), a built-in type's member reached through
    /// an inferred type. `None`: [`hover`](Self::hover).
    fn typed_hover(&self, _text: &str, _offset: usize, _types: &Types<'_>) -> Option<HoverInfo> {
        None
    }

    /// The problems of the document knowing the project (`types`: its index complete):
    /// unknown classes, calls with too few or too many arguments, uses of deprecated
    /// declarations. Runs off the UI thread, after the document's own symbols are indexed.
    fn check(&self, _text: &str, _types: &Types<'_>) -> Checked {
        Checked::default()
    }

    /// [`check`](Self::check) looks at the rest of the project: an open document is checked
    /// again when other files' declarations change.
    fn checks_project(&self) -> bool {
        false
    }

    /// Whether the declaration is deprecated by what its language writes on it besides
    /// its doc comment's `@deprecated` (PHP's `#[\Deprecated]`).
    fn is_deprecated(&self, _symbol: &Symbol) -> bool {
        false
    }

    /// What the language itself provides at `offset` (a PHP built-in function, a CSS
    /// property, an HTML element, a DOM API), explained from its data. Project symbols are
    /// handled by [`hover`].
    fn hover(&self, _text: &str, _offset: usize) -> Option<HoverInfo> {
        None
    }

    /// The signature of a function the language itself provides, by its name, for a call's
    /// parameter help where the language has no declarations to read it from (SQL's
    /// built-in functions, by the file's dialect): `substr(text, start, length) → text`.
    /// Unlike [`Self::hover`], nothing else (hover cards, inlay hints) uses it.
    fn call_signature(&self, _path: &std::path::Path, _name: &str) -> Option<String> {
        None
    }

    /// The level from which a language keyword, function or constant exists.
    fn since(&self, _word: &str) -> Option<&'static str> {
        None
    }

    /// How it writes doc comments (`/** … */`, the attributes or decorators between one and
    /// its declaration): read for hover, and Enter right after its opener fills a new one in
    /// from the declaration below ([`doc_comment`](Self::doc_comment)). `None`: none.
    fn doc_rules(&self) -> Option<crate::DocRules> {
        None
    }

    /// How its code is colored where there's no parser (the canvas, a comparison, a hover
    /// card): registered at startup ([`code_style::register`](crate::code_style::register)).
    fn code_style(&self) -> Option<crate::code_style::CodeStyle> {
        None
    }

    /// The declaration at `offset` of `text` (one of its files) shown whole in a hover, when
    /// its name alone says little: a CSS selector's rule. `None`: its signature.
    fn declaration_code(&self, _text: &str, _offset: usize, _kind: SymbolKind) -> Option<String> {
        None
    }

    /// The lines of a doc block generated for the declaration after `offset` (which is
    /// inside a new `/** */`), without `*`, e.g. `["@param string $email", "@return bool"]`.
    fn doc_comment(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }

    /// The extraction version: when it changes, the language's symbols in the on-disk
    /// cache are recomputed.
    fn version(&self) -> u32 {
        1
    }
}

/// Expanding an abbreviation on Tab (e.g. Emmet: `div.card#main` → `<div class="card"
/// id="main"></div>`), contributed by a module.
pub trait Expander: Send + Sync + Debug {
    /// The languages (at the cursor) in which it applies, e.g. `html`, `twig`.
    fn languages(&self) -> &[&'static str];
    /// The expansion of the abbreviation at the end of `line_before` (the line's text up to the
    /// cursor), or `None` if there is no abbreviation there. `indent` is the line's indent, and
    /// `unit` one indentation step.
    fn expand(&self, line_before: &str, indent: &str, unit: &str) -> Option<Expansion>;
}

/// What a language inserts as a character is typed ([`LanguageIntelligence::on_type`]): its
/// own undo step (one Undo leaves what was typed by hand).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedInsert {
    pub text: String,
    /// Where the cursor ends up in `text` (bytes; 0: before it, as before `</div>`).
    pub cursor: usize,
    /// Characters that, typed right after (nothing else done since), take it back first:
    /// `=` after `<?` gives `<?=`, not `<?php` and a `=`.
    pub retract_on: &'static [char],
}

impl TypedInsert {
    /// `text` after the cursor, which stays where it is (a closing tag).
    pub fn after_cursor(text: impl Into<String>) -> Self {
        Self { text: text.into(), cursor: 0, retract_on: &[] }
    }

    /// `text` with the cursor at its end, taken back if one of `retract_on` follows.
    pub fn before_cursor(text: impl Into<String>, retract_on: &'static [char]) -> Self {
        let text = text.into();
        Self { cursor: text.len(), text, retract_on }
    }
}

/// The result of an expansion: how many bytes before the cursor are replaced, the new text
/// and where the cursor ends up in it (offset in bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion {
    pub replace: usize,
    pub text: String,
    pub cursor: usize,
}

/// How a color is written in code (preserved when editing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorFormat {
    /// `#rgb`, `#rrggbb`, `#rrggbbaa`.
    Hex,
    /// `rgb(…)` / `rgba(…)`.
    Rgb,
    /// `hsl(…)` / `hsla(…)`.
    Hsl,
    /// A CSS name (`red`): becomes hex when edited.
    Named,
}

/// A color written in code: where it is (bytes, relative to the text), its value and form.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorLiteral {
    pub range: std::ops::Range<usize>,
    pub color: kernel::Rgba,
    pub format: ColorFormat,
}

/// Colors in code (the swatch in the gutter and the color picker), contributed by a module.
pub trait ColorProvider: Send + Sync + Debug {
    /// The colors on a line, in language `language` (from the start of each).
    fn find(&self, language: &str, line: &str) -> Vec<ColorLiteral>;
    /// A color typed by hand (e.g. in the picker's field).
    fn parse(&self, text: &str) -> Option<ColorLiteral>;
    /// The color's text in the requested form (`Named` → hex).
    fn format(&self, color: kernel::Rgba, format: ColorFormat) -> String;
}

/// A language feature used in the text that is newer than the chosen level (e.g. `match`
/// with PHP 7.4): shown as an error, with "requires PHP 8.0". Also what the level removed
/// (`removed_*`: an error) or deprecated (`deprecated_*`: a warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelIssue {
    pub range: std::ops::Range<usize>,
    /// What it is: a feature id (`match`, `return_type`…) whose label the app translates,
    /// `function` with the function's name in `name`, or a `removed_*` / `deprecated_*` id
    /// with its own message.
    pub feature: &'static str,
    pub name: Option<String>,
    /// The first level that supports it.
    pub since: &'static str,
    /// What it's written as, shown as code next to the message (`match`, `??`,
    /// `str_contains()`), if anything.
    pub code: Option<String>,
}

impl LevelIssue {
    /// Still works at the level, but deprecated by it: a warning, not an error.
    pub fn is_warning(&self) -> bool {
        self.feature.starts_with("deprecated_")
    }
}

/// What the mouse rests on, explained: a signature or a syntax, with notes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HoverInfo {
    /// The word it is about.
    pub range: std::ops::Range<usize>,
    /// `strlen(string $string): int`, `display: [ <display-outside> || … ]`, `<center>`.
    pub code: String,
    pub notes: Vec<HoverNote>,
    /// A short explanation (a doc comment's summary, a framework's documentation).
    pub doc: Option<String>,
    /// The card's header: the type declaring it (`DeviceRegistration`) with its kind, or
    /// a type's namespace.
    pub title: Option<(String, Option<CompletionKind>)>,
    /// Shown dimmed after the title (the type's namespace).
    pub subtitle: Option<String>,
    /// The parameters, with their documentation (`@param`).
    pub params: Vec<HoverParam>,
    /// What those rows are called on the card when they aren't parameters (a table's
    /// columns); `None`: "Parameters".
    pub params_label: Option<String>,
    /// Where it is declared (relative to the project).
    pub source: Option<String>,
}

/// A parameter on the hover card: `Device` `$device` – the managed row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HoverParam {
    pub ty: Option<String>,
    pub name: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverNote {
    /// Available from this language level on.
    Since(&'static str),
    /// Removed in this language level.
    Removed(&'static str),
    Deprecated,
    Experimental,
    NonStandard,
    /// Provided by a language extension (e.g. PHP's `redis`), not the language itself.
    Extension(&'static str),
}

impl HoverNote {
    /// From the data files' flags (`d` deprecated, `x` experimental, `n` non-standard).
    pub fn from_flags(flags: &str) -> Vec<Self> {
        let mut notes = Vec::new();
        if flags.contains('d') {
            notes.push(Self::Deprecated);
        }
        if flags.contains('x') {
            notes.push(Self::Experimental);
        }
        if flags.contains('n') {
            notes.push(Self::NonStandard);
        }
        notes
    }
}

/// The word around `offset`: letters, digits, `_` and the language's own word characters
/// (`extra`: `-` in CSS, `$` in JavaScript, `$` and `\\` in PHP).
pub fn word_at(text: &str, offset: usize, extra: &str) -> Option<std::ops::Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || extra.contains(c);
    let offset = offset.min(text.len());
    if !text.is_char_boundary(offset) {
        return None;
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = offset + text[offset..].find(|c: char| !is_word(c)).unwrap_or(text.len() - offset);
    (start < end).then_some(start..end)
}

/// Something the text needs the project to declare (e.g. PHP's `ext-http` in
/// `composer.json`, for `new http\Env\Request`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub range: std::ops::Range<usize>,
    /// As written in the manifest (`ext-http`).
    pub name: String,
}

/// Something declared but never used in the text (an import, a private member): shown
/// dimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unused {
    pub range: std::ops::Range<usize>,
    /// What it is (`import`, `method`, `property`), translated by the app.
    pub kind: &'static str,
    pub name: String,
}

/// A deprecated name used in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deprecated {
    pub range: std::ops::Range<usize>,
    /// What it is (`element`, `attribute`, `property`, `api`), translated by the app.
    pub kind: &'static str,
    /// As shown in the message (`<center>`, `clip`, `document.write`).
    pub name: String,
}

/// A detected language level and where it comes from (for the UI: "composer.json", "php 8.3.6").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedLevel {
    pub level: String,
    pub source: String,
}

/// The kind of a suggestion (for the icon).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CompletionKind {
    Class,
    Interface,
    Function,
    Method,
    Property,
    Constant,
    Variable,
    #[default]
    Keyword,
    /// HTML tag.
    Tag,
    /// HTML attribute or CSS property.
    Attribute,
    /// Value (CSS) or language constant.
    Value,
    /// CSS selector (class, id) or CSS variable.
    Selector,
    /// A database's table.
    Table,
    /// A database's view.
    View,
    /// A table's column.
    Column,
    /// A database's schema.
    Schema,
    /// A piece of code made for the place (a whole join condition, a column list).
    Snippet,
    /// A namespace (a part of a qualified name: `Models` in `App\Models\User`).
    Namespace,
}

impl CompletionItem {
    /// A simple suggestion, from a language's vocabulary (the engine assigns the score).
    pub fn word(label: &str, kind: CompletionKind, detail: Option<&str>) -> Self {
        Self {
            label: label.to_owned(),
            kind,
            detail: detail.map(str::to_owned),
            insert: label.to_owned(),
            cursor_back: 0,
            score: 0,
            import: None,
            visibility: None,
            replace: None,
            edits: Vec::new(),
            resolve: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    /// Detail shown on the right (the signature, the namespace, the class).
    pub detail: Option<String>,
    /// The text inserted in place of the prefix.
    pub insert: String,
    /// How many chars back the cursor moves after insertion (e.g. into the parens of `foo()`).
    pub cursor_back: usize,
    pub score: i32,
    /// The type to import on accept (e.g. `use App\Models\User;` in PHP).
    pub import: Option<String>,
    /// The member's visibility (a marker in the list).
    pub visibility: Option<Visibility>,
    /// The text it replaces, when it isn't the typed prefix (a language server's own range,
    /// starting where it says, ending at the cursor). Bytes of the document it was made for.
    pub replace: Option<Range<usize>>,
    /// What it changes elsewhere in the document when it's accepted (a language server's
    /// import): bytes of the document it was made for, before `replace`'s start.
    pub edits: Vec<(Range<usize>, String)>,
    /// A language server works it out in full when it's accepted (its handle: the import
    /// it adds comes then).
    pub resolve: Option<u64>,
}

/// [`LanguageIntelligence::fits_kind`]'s default (also for files no language claims): a type
/// isn't written after a member access (`rules`) nor right after the sigil.
pub fn fits_kind_plainly(
    text: &str,
    range: &Range<usize>,
    kind: SymbolKind,
    rules: Option<&SourceRules>,
) -> bool {
    let Some(rules) = rules else { return true };
    let head = text[..range.start].trim_end_matches([' ', '\t']);
    let member = rules.member_access.iter().any(|a| head.ends_with(a));
    let sigil = rules.naming.sigil.is_some_and(|s| text[..range.start].ends_with(s));
    !kind.is_type() || (!member && !sigil)
}
