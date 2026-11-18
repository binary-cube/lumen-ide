//! An HTML element or attribute under the mouse, explained from our data (deprecated /
//! experimental / non-standard).

use code_intelligence::{HoverInfo, HoverNote};

pub fn hover(text: &str, offset: usize) -> Option<HoverInfo> {
    let range = code_intelligence::word_at(text, offset, "-")?;
    let html = crate::data::get();
    let word = text[range.clone()].to_ascii_lowercase();
    let before = &text[..range.start];
    // `<tag` or `</tag`: the element.
    if before.ends_with('<') || before.ends_with("</") {
        let (_, flags) = html.elements.iter().find(|(t, _)| *t == word)?;
        return Some(HoverInfo {
            range,
            code: format!("<{word}>"),
            notes: HoverNote::from_flags(flags),
            doc: None,
            ..Default::default()
        });
    }
    // Inside a tag, after a space: an attribute of the tag's element.
    let open = before.rfind('<')?;
    if before[open..].contains('>') || !before.ends_with(char::is_whitespace) {
        return None;
    }
    let tag: String = before[open + 1..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    let flags = html
        .attributes(&tag)
        .iter()
        .chain(html.global_attributes())
        .find(|(a, _)| *a == word)
        .map(|(_, f)| *f)?;
    Some(HoverInfo {
        range,
        code: format!("<{tag} {word}>"),
        notes: HoverNote::from_flags(flags),
        doc: None,
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "../tests/unit/hover.rs"]
mod tests;
