//! `timeline`: a title, sections, and periods with their events (`2004 : Facebook :
//! Google`, more on following lines starting with `:`).

use super::{Error, Line, Meta, label};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Period {
    pub name: String,
    pub section: Option<usize>,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeline {
    pub sections: Vec<String>,
    pub periods: Vec<Period>,
}

pub(super) fn parse(lines: &[Line<'_>], meta: &mut Meta) -> Result<Timeline, Error> {
    let mut t = Timeline::default();
    for line in lines {
        let text = line.text.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(title) = text.strip_prefix("title ") {
            meta.title = Some(title.trim().to_owned());
            continue;
        }
        if let Some(section) = text.strip_prefix("section ") {
            t.sections.push(label(section.trim()));
            continue;
        }
        let mut parts = text.split(':').map(str::trim);
        let first = parts.next().unwrap_or_default();
        let events: Vec<String> = parts.filter(|e| !e.is_empty()).map(label).collect();
        if first.is_empty() {
            // `: more` continues the period before.
            let Some(last) = t.periods.last_mut() else { return Err(Error::unexpected(line)) };
            last.events.extend(events);
            continue;
        }
        let section = t.sections.len().checked_sub(1);
        t.periods.push(Period { name: label(first), section, events });
    }
    Ok(t)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/timeline.rs"]
mod tests;
