//! `@var` in the templates' comments: Twig's `{# @var user \App\Entity\User #}` and Blade's
//! `{{-- @var \App\Models\User $user --}}`. The class names written there lead to the class
//! (⌘-click, the hover card) as in a PHPDoc `@var`: this assist only says which PHP name it
//! is; the engine finds it in the project's index. Read from the comment around the cursor
//! (a bounded look back, no parsing): nothing else of the templates is touched.

use code_intelligence::{AssistContext, AssistTarget, CommentPlace, DocumentAssist, Target};

/// How far back the comment's opening is looked for (bytes): a comment longer than this
/// isn't a `@var` one.
const LOOK_BACK: usize = 4096;

/// The `@var` comments of Twig and Blade, read by the PHP module.
#[derive(Debug, Default)]
pub struct TemplateVars;

impl DocumentAssist for TemplateVars {
    fn id(&self) -> &'static str {
        "php-template-vars"
    }

    fn languages(&self) -> &'static [&'static str] {
        &["twig", "blade"]
    }

    fn definition(&self, at: &AssistContext<'_>) -> Option<AssistTarget> {
        let target = class_at(at.text, at.offset)?;
        Some(AssistTarget::Code { language: "php", target })
    }
}

/// The template comments' delimiters: Twig's and Blade's.
const COMMENTS: &[(&str, &str)] = &[("{#", "#}"), ("{{--", "--}}")];

/// The PHP name at `offset` of a template's `@var` comment, if it's a type's name there.
pub fn class_at(text: &str, offset: usize) -> Option<Target> {
    let (body, base) = comment_around(text, offset)?;
    let at = offset - base;
    // Each `@var` of the comment: its two words, one the variable, the other the type.
    let mut search = 0;
    while let Some(found) = body[search..].find("@var") {
        let tag = search + found;
        search = tag + 4;
        let mut words = words_after(body, search).take(2);
        let (first, second) = (words.next(), words.next());
        for (start, word) in [first, second].into_iter().flatten() {
            if at < start || at > start + word.len() || is_variable(word, first, second) {
                continue;
            }
            // Read within the comment alone (a template has no class around it: `self`,
            // `static`, `parent` name nothing there).
            return match crate::comment_places::type_place(body, word, at - start, 0) {
                CommentPlace::Code(Target::Class(name))
                    if ["self", "static", "parent", "this"]
                        .iter()
                        .any(|w| w.eq_ignore_ascii_case(name.trim_start_matches('$'))) =>
                {
                    None
                }
                CommentPlace::Code(target) => Some(target),
                _ => None,
            };
        }
    }
    None
}

/// The body of the template comment around `offset` and where it starts in `text`.
fn comment_around(text: &str, offset: usize) -> Option<(&str, usize)> {
    let offset = offset.min(text.len());
    let floor = offset.saturating_sub(LOOK_BACK);
    let floor = (floor..=offset).find(|&i| text.is_char_boundary(i))?;
    let before = text.get(floor..offset)?;
    COMMENTS.iter().find_map(|(open, close)| {
        let start = before.rfind(open)?;
        // Closed before the cursor: not around it.
        if before[start..].contains(close) {
            return None;
        }
        let body = floor + start + open.len();
        let end = text[offset..].find(close).map_or(text.len(), |e| offset + e);
        Some((&text[body..end], body))
    })
}

/// The words after `from` in `body` (at their offsets), stopping at a newline.
fn words_after(body: &str, from: usize) -> impl Iterator<Item = (usize, &str)> {
    let line = body[from..].split('\n').next().unwrap_or("");
    line.split_whitespace().map(move |word| {
        // `split_whitespace` hands out slices of `line`: their place in it.
        let start = word.as_ptr() as usize - line.as_ptr() as usize;
        (from + start, word)
    })
}

/// `word` is the variable of the `@var` (`first` and `second` its two words): written with
/// `$`, or the other one names a class (`\App\User`, `User[]`), else Twig's order (the
/// variable first).
fn is_variable(word: &str, first: Option<(usize, &str)>, second: Option<(usize, &str)>) -> bool {
    if word.starts_with('$') {
        return true;
    }
    let other = match (first, second) {
        (Some((_, f)), Some((_, s))) if f == word => s,
        (Some((_, f)), Some(_)) => f,
        _ => return false,
    };
    if other.starts_with('$') {
        return false;
    }
    let typed = |w: &str| w.contains(['\\', '[', '|', '<', '?']);
    match (typed(word), typed(other)) {
        (true, _) => false,
        (false, true) => true,
        (false, false) => first.is_some_and(|(_, f)| f == word),
    }
}

#[cfg(test)]
#[path = "../tests/unit/template_vars.rs"]
mod tests;
