//! Doc comments' parts (plan 75h): a language says where a doc comment's tags, types and
//! documented names are ([`DocComments`], contributed by its module); highlighting colors
//! them over the comment's own color, as scopes of their own (`comment.doc.tag`,
//! `comment.doc.type`, `comment.doc.variable`: a theme that doesn't name them gets them
//! from its code colors). Asked only when highlighting is made again (a new version, a
//! parse, a theme), for the comments in the area colored.

use std::ops::Range;

use languages::LanguageId;

/// A part of a doc comment colored as code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocPart {
    /// `@param`, `@return`, `{@link`.
    Tag,
    /// A type written for a tag (`int|null`, `{Promise<User>}`).
    Type,
    /// The name a tag documents (`$user`, `user`).
    Variable,
}

impl DocPart {
    const ALL: [DocPart; 3] = [Self::Tag, Self::Type, Self::Variable];

    fn scope(self) -> &'static str {
        match self {
            Self::Tag => "comment.doc.tag",
            Self::Type => "comment.doc.type",
            Self::Variable => "comment.doc.variable",
        }
    }
}

/// A language's doc comments, read for their parts.
pub trait DocComments: Send + Sync + std::fmt::Debug {
    /// The language whose comments it reads (its grammar's).
    fn language(&self) -> LanguageId;
    /// The grammar's nodes that are comments (`comment`).
    fn nodes(&self) -> &'static [&'static str];
    /// The parts of `comment` (a comment node's text; ranges within it); none when it isn't
    /// a doc comment.
    fn parts(&self, comment: &str) -> Vec<(Range<usize>, DocPart)>;
}

/// The longest comment read for its parts (bytes): a longer one keeps the comment's color.
pub(crate) const LONGEST: usize = 64 * 1024;

/// The scope keys' captures past the queries' own: one per part.
const FIRST: u16 = u16::MAX - DocPart::ALL.len() as u16;

/// The capture a part is highlighted as.
pub(crate) fn capture(part: DocPart) -> u16 {
    FIRST + DocPart::ALL.iter().position(|p| *p == part).unwrap_or(0) as u16
}

/// The scope of a part's capture (`None`: one of the queries' own).
pub(crate) fn scope(capture: u16) -> Option<&'static str> {
    let index = capture.checked_sub(FIRST)?;
    DocPart::ALL.get(index as usize).map(|p| p.scope())
}
