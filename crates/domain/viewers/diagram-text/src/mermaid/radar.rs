//! `radar-beta`: axes (`axis m["Math"], s["Science"]`), curves of values on them
//! (`curve a["Alice"]{85, 90}` or `{m: 85, s: 90}`), `max` / `min`.

use super::{Error, Line, Meta, unquote};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Radar {
    pub axes: Vec<(String, String)>,
    pub curves: Vec<(String, Vec<f64>)>,
    pub max: Option<f64>,
    pub min: f64,
}

/// `id["Label"]` (or a bare id): the id and what's shown.
fn named(s: &str) -> (String, String) {
    let s = s.trim();
    match s.split_once('[') {
        Some((id, rest)) => {
            (id.trim().to_owned(), unquote(rest.trim_end_matches(']').trim()).to_owned())
        }
        None => (s.to_owned(), s.to_owned()),
    }
}

pub(super) fn parse(lines: &[Line<'_>], meta: &mut Meta) -> Result<Radar, Error> {
    let mut r = Radar::default();
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "title" => meta.title = Some(rest.to_owned()),
            "axis" => {
                for part in rest.split(',') {
                    r.axes.push(named(part));
                }
            }
            "curve" => {
                for part in split_curves(rest) {
                    let (head, values) =
                        part.split_once('{').ok_or_else(|| Error::unexpected(line))?;
                    let (_, label) = named(head);
                    let values = values.trim_end_matches('}');
                    let mut out = vec![0.0; r.axes.len()];
                    for (i, v) in values.split(',').enumerate() {
                        let v = v.trim();
                        if let Some((axis, value)) = v.split_once(':') {
                            let a = r
                                .axes
                                .iter()
                                .position(|(id, _)| id == axis.trim())
                                .ok_or_else(|| Error::unexpected(line))?;
                            out[a] = value.trim().parse().map_err(|_| Error::unexpected(line))?;
                        } else if let Some(slot) = out.get_mut(i) {
                            *slot = v.parse().map_err(|_| Error::unexpected(line))?;
                        }
                    }
                    r.curves.push((label, out));
                }
            }
            "max" => r.max = rest.parse().ok(),
            "min" => r.min = rest.parse().unwrap_or(0.0),
            "showLegend" | "graticule" | "ticks" => {}
            _ => return Err(Error::unexpected(line)),
        }
    }
    Ok(r)
}

/// `a{1, 2}, b{3, 4}` as its curves.
fn split_curves(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim());
    out.into_iter().filter(|p| !p.is_empty()).collect()
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/radar.rs"]
mod tests;
