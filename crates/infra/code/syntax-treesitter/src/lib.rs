//! The shared syntax engine, based on tree-sitter: language layers, injections,
//! highlighting and folding. Language modules contribute [`Grammar`]s and, optionally,
//! [`InjectionProvider`]s and [`FoldProvider`]s; the engine knows no specific language.

mod children;
mod doc;
mod grammar;
mod highlight;
mod indent;
mod injection;
mod parse;
mod regions;
mod selection;
mod statements;
mod text;
mod worker;

pub use children::{children, named_children, push_children};
pub use doc::{DocComments, DocPart};
pub use grammar::{Grammar, Grammars};
pub use highlight::{HighlightSpan, Highlighter};
pub use indent::IndentAnchor;
pub use injection::{Injection, InjectionContext, InjectionProvider, InjectionSource};
pub use parse::{
    ScopeKey, SyntaxConfig, SyntaxError, SyntaxErrorKind, SyntaxSnapshot, parse_document,
};
pub use regions::FoldProvider;
pub use selection::selection_ranges;
pub use statements::{Moved, statement_move};
pub use worker::{DocumentSyntax, ParseJob, ParseResult, SyntaxWorker, Waker};

/// Re-exported for modules that write their own injection rules.
pub use tree_sitter;
