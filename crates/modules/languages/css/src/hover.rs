//! A CSS property under the mouse, explained from our data: its value syntax and whether
//! it is deprecated / experimental.

use code_intelligence::{HoverInfo, HoverNote};

pub fn hover(text: &str, offset: usize) -> Option<HoverInfo> {
    let range = code_intelligence::word_at(text, offset, "-")?;
    // A property is followed by `:` (and is not a selector like `a:hover {`).
    let after = text[range.end..].trim_start();
    if !after.starts_with(':') {
        return None;
    }
    let declaration = &after[1..];
    let ends = declaration.find([';', '}', '{']).map(|i| declaration.as_bytes()[i]);
    if ends == Some(b'{') {
        return None;
    }
    let name = text[range.clone()].to_ascii_lowercase();
    let css = crate::data::get();
    let property = css.properties.iter().find(|p| p.name == name)?;
    let code =
        if property.syntax.is_empty() { name } else { format!("{name}: {}", property.syntax) };
    Some(HoverInfo {
        range,
        code,
        notes: HoverNote::from_flags(property.flags),
        doc: None,
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "../tests/unit/hover.rs"]
mod tests;
