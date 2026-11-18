//! `pie`: a title (on the first line or its own), `showData`, and `"Label" : value` slices.

use super::{Error, Line, Meta, label};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pie {
    pub show_data: bool,
    pub slices: Vec<(String, f64)>,
}

pub(super) fn parse(head: Line<'_>, lines: &[Line<'_>], meta: &mut Meta) -> Result<Pie, Error> {
    let mut pie = Pie::default();
    let mut rest = head.text.trim().strip_prefix("pie").unwrap_or_default().trim();
    if let Some(r) = rest.strip_prefix("showData") {
        pie.show_data = true;
        rest = r.trim();
    }
    if let Some(t) = rest.strip_prefix("title") {
        meta.title = Some(t.trim().to_owned());
    }
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if t == "showData" {
            pie.show_data = true;
            continue;
        }
        if let Some(title) = t.strip_prefix("title") {
            meta.title = Some(title.trim().to_owned());
            continue;
        }
        let (name, value) = t.rsplit_once(':').ok_or_else(|| Error::unexpected(line))?;
        let value: f64 = value.trim().parse().map_err(|_| Error::unexpected(line))?;
        if value < 0.0 || !value.is_finite() {
            return Err(Error::unexpected(line));
        }
        pie.slices.push((label(name.trim()), value));
    }
    Ok(pie)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/pie.rs"]
mod tests;
