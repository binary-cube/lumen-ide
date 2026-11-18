//! Tree-sitter queries more than one language builds on (a grammar extending another's), kept
//! here so that none reads another's module.

pub const HTML_INJECTIONS: &str = include_str!("../queries/html-injections.scm");
pub const HTML_FOLDS: &str = include_str!("../queries/html-folds.scm");
pub const HTML_INDENTS: &str = include_str!("../queries/html-indents.scm");
pub const PHP_HIGHLIGHTS: &str = include_str!("../queries/php-highlights.scm");
