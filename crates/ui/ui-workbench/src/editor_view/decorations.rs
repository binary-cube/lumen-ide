//! What the editor is given to draw beside its text, from outside it: the colors, the
//! matches, the problems, the gutter's marks, the hints and lenses, the bulb.

use super::*;

/// A colored span of text (e.g. from syntax highlighting).
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSpan {
    pub range: Range<usize>,
    pub color: Rgba,
    /// The theme's face for it (drawn in the code font's bold / italic, as wide).
    pub bold: bool,
    pub italic: bool,
}

/// Everything drawn over the text, coming from outside the view.
#[derive(Debug, Clone, Copy)]
pub struct Decorations<'a> {
    /// In order of application: on overlap, the last one wins.
    pub colors: &'a [ColorSpan],
    /// Search matches, sorted.
    pub matches: &'a [Range<usize>],
    pub current_match: Option<&'a Range<usize>>,
    pub folds: &'a Folds,
    /// Error ranges (sorted): squiggly underline and marks on the scroll strip.
    pub errors: &'a [Range<usize>],
    /// Deprecated names (sorted): struck through, marks on the scroll strip.
    pub deprecated: &'a [Range<usize>],
    /// Other warnings (sorted, e.g. an extension missing from `composer.json`): a yellow
    /// squiggle, marks on the scroll strip.
    pub warnings: &'a [Range<usize>],
    /// Unused code (sorted): dimmed.
    pub unused: &'a [Range<usize>],
    /// The symbol under the mouse with ⌘ held: can be clicked to go to the declaration.
    pub link: Option<&'a Range<usize>>,
    /// Occurrences of the selected word (sorted): subtle background and marks on the scroll strip.
    pub occurrences: &'a [Range<usize>],
    /// Colors written in the code: (line, color), sorted; swatch in the gutter.
    pub swatches: &'a [(u32, Rgba)],
    /// The marks of the lines a module decorates (sorted; the lines changed since the last
    /// commit): bars in the gutter.
    pub vcs: &'a [workbench_model::LineMark],
    /// Per line, its annotation (git blame) when shown; empty otherwise.
    pub annotations: &'a [Option<workbench_model::LineAnnotation>],
    /// Changes whenever the errors, the warnings or the occurrences are recomputed (also
    /// at the same document version): the scroll strip's lines are kept for it.
    pub marks_generation: u64,
    /// The statements a module marks (a query console's: running, done, failed), sorted:
    /// a dot in the gutter on each one's first line.
    pub statements: &'a [StatementLines],
    /// Lines drawn highlighted (a console's statement Run would run): the first and the one
    /// after the last.
    pub highlight: Option<(usize, usize)>,
    /// The lines of the statement the cursor is in (a console's: what Run runs), the first
    /// and the one after the last: a bar in the gutter beside them.
    pub current: Option<(usize, usize)>,
    /// A place pointed at (bytes) and its color: a profile's finding in a console — the
    /// text tinted and outlined, its lines lightly tinted.
    pub focus: Option<(usize, usize, Rgba)>,
    /// The code actions' bulb, beside the cursor's line.
    pub bulb: Option<Bulb>,
    /// The lenses after their lines' text (sorted by line).
    pub lenses: &'a [LineLens],
    /// What a module hides (a `.env` file's secrets), sorted: drawn as marks of the same
    /// length (`workbench_model::concealment::mask`), the text unchanged.
    pub concealed: &'a [Range<usize>],
}

/// The bulb beside a line: code actions are there at the cursor (`fix`: a quick fix among
/// them, drawn brighter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bulb {
    pub line: u32,
    pub fix: bool,
}

/// A note written after a line's text (a code lens: `2 usages · 1 implementation`): its
/// parts, each clicked on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineLens {
    pub line: u32,
    pub parts: Vec<String>,
}

/// A hint shown inside the text, before byte `offset` (an inlay: a parameter's name): room
/// is made for it in its row, the text itself is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inlay {
    pub offset: usize,
    pub label: String,
}

impl Inlay {
    /// The columns it takes in its row (its label and a space after it).
    pub(super) fn columns(&self) -> usize {
        self.label.chars().count() + 1
    }
}

/// A statement's lines as a module marks them in the gutter (0-based, end exclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementLines {
    pub lines: Range<usize>,
    pub state: ui_components::editor_toolbar::MarkState,
}

/// What an error's preview shows: its range, the header (message, code, line), the colors.
#[derive(Debug, Clone)]
pub struct ErrorPreviewSpec<'a> {
    pub error: Range<usize>,
    pub message: &'a str,
    pub code: Option<&'a str>,
    pub line_label: &'a str,
    pub colors: &'a [ColorSpan],
}

/// What a problem tooltip shows.
#[derive(Debug, Clone, Copy)]
pub struct ErrorTip<'a> {
    pub message: &'a str,
    /// The code it is about, in a pill.
    pub code: Option<&'a str>,
    /// A warning (e.g. deprecated): the warning color instead of the error one.
    pub warning: bool,
    /// Unused code: a neutral tone.
    pub weak: bool,
}
