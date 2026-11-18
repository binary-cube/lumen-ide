//! A JavaScript / DOM API under the mouse, explained from our data: a member of a
//! well-known object (`document.querySelector` → `Document.querySelector`) or a global,
//! with its signature and its deprecated / experimental / non-standard status.

use code_intelligence::{HoverInfo, HoverNote};

pub fn hover(text: &str, offset: usize) -> Option<HoverInfo> {
    let range = code_intelligence::word_at(text, offset, "$")?;
    let word = &text[range.clone()];
    let js = crate::data::get();
    let before = &text[..range.start];
    if let Some(object) = before.strip_suffix('.') {
        let start = object
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '$'))
            .last()
            .map_or(object.len(), |(i, _)| i);
        let Some(owner) = js.owner(&object[start..]) else {
            // An object of unknown type: the member if every interface agrees on it.
            let (owner, sig) = js.any_signature(word)?;
            return Some(HoverInfo {
                range,
                code: format!("{owner}.{word}{sig}"),
                notes: vec![],
                doc: None,
                ..Default::default()
            });
        };
        let (member, flags) = js.members(owner).iter().find(|(m, _)| *m == word)?;
        let sig = js.signature(owner, member).unwrap_or("");
        return Some(HoverInfo {
            range,
            code: format!("{owner}.{member}{sig}"),
            notes: HoverNote::from_flags(flags),
            doc: None,
            ..Default::default()
        });
    }
    // A global: only worth a card when there is something to say about it.
    let (name, flags) = js.globals.iter().find(|(g, _)| *g == word)?;
    let notes = HoverNote::from_flags(flags);
    let sig = js.signature("", name);
    (!notes.is_empty() || sig.is_some()).then(|| HoverInfo {
        range,
        code: format!("{name}{}", sig.unwrap_or("")),
        notes,
        doc: None,
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "../tests/unit/hover.rs"]
mod tests;
