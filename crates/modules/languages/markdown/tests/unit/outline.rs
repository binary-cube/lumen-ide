use super::*;

fn names(text: &str) -> Vec<(u8, String)> {
    headings(text).into_iter().map(|h| (h.level, h.text)).collect()
}

/// The headings nest as the document does: a `##` inside the `#` before it, to the next
/// heading of its level or higher; the outline follows.
#[test]
fn headings_nest_by_their_level() {
    let text = "# Guide\nintro\n## Install\nsteps\n### macOS\nbrew\n## Use\nrun\n# Notes\nend\n";
    let file = symbols(text);
    let outline = code_intelligence::outline(&file);
    let rows: Vec<(usize, &str)> =
        outline.iter().map(|o| (o.depth, file.symbols[o.index].name.as_ref())).collect();
    assert_eq!(rows, [(0, "Guide"), (1, "Install"), (2, "macOS"), (1, "Use"), (0, "Notes")]);
    let install = &file.symbols[1];
    assert_eq!(&text[install.range.clone()], "## Install\nsteps\n### macOS\nbrew");
    assert_eq!(&text[install.offset..install.offset + 7], "Install");
    assert_eq!(install.line, 2);
    assert_eq!(file.symbols[4].range.end, text.len(), "the last to the end");
    // The breadcrumb's chain at the cursor in "brew".
    let at = text.find("brew").unwrap();
    let chain: Vec<&str> = code_intelligence::scope_at(&file, at)
        .into_iter()
        .map(|i| file.symbols[i].name.as_ref())
        .collect();
    assert_eq!(chain, ["Guide", "Install", "macOS"]);
}

/// Underlined headings count; a `---` after a blank line or a list item is a rule.
#[test]
fn setext_headings_and_rules() {
    let text = "Title\n=====\n\nPart\n---\ntext\n\n---\n- item\n---\n";
    assert_eq!(names(text), [(1, "Title".to_owned()), (2, "Part".to_owned())]);
}

/// Nothing in a code block (fenced or indented), the front matter, or what only looks like
/// a heading (`#tag`, seven `#`s).
#[test]
fn no_headings_where_markdown_has_none() {
    let text = "---\ntitle: # not\n---\n# Real\n```sh\n# a comment\n```\n~~~~\n# no\n~~~~\n    # indented code\n#tag\n####### seven\n## Also\n";
    assert_eq!(names(text), [(1, "Real".to_owned()), (2, "Also".to_owned())]);
}

/// The closing `#`s go, a `#` of the title stays; the spaces are tidied; a long title is cut
/// on a character, not a byte.
#[test]
fn titles_are_tidied() {
    assert_eq!(
        names("## Closed ##\n# C#\n#   Spaced   out  \n"),
        [(2, "Closed".to_owned()), (1, "C#".to_owned()), (1, "Spaced out".to_owned())]
    );
    let long = format!("# {}\n", "ș".repeat(300));
    let title = &names(&long)[0].1;
    assert_eq!(title.chars().count(), MAX_NAME + 1, "cut, with …");
    assert!(title.ends_with('…'));
}

/// A heading with nothing after its `#` isn't listed (no name to show).
#[test]
fn an_empty_heading_isn_t_listed() {
    let file = symbols("#\ntext\n## Next\n");
    let outline = code_intelligence::outline(&file);
    let rows: Vec<&str> = outline.iter().map(|o| file.symbols[o.index].name.as_ref()).collect();
    assert_eq!(rows, ["Next"]);
}
