//! `quadrantChart`: the axes' two ends (`x-axis Low --> High`), the four quadrants' names
//! and points at `[x, y]` (0–1).

use super::{Error, Line, Meta, unquote};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quadrant {
    pub x: (String, String),
    pub y: (String, String),
    /// Quadrants 1 (top right), 2 (top left), 3 (bottom left), 4 (bottom right).
    pub quadrants: [String; 4],
    pub points: Vec<(String, f64, f64)>,
}

fn ends(s: &str) -> (String, String) {
    match s.split_once("-->") {
        Some((a, b)) => (unquote(a.trim()).to_owned(), unquote(b.trim()).to_owned()),
        None => (unquote(s.trim()).to_owned(), String::new()),
    }
}

pub(super) fn parse(lines: &[Line<'_>], meta: &mut Meta) -> Result<Quadrant, Error> {
    let mut q = Quadrant::default();
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "title" => meta.title = Some(rest.to_owned()),
            "x-axis" => q.x = ends(rest),
            "y-axis" => q.y = ends(rest),
            "quadrant-1" | "quadrant-2" | "quadrant-3" | "quadrant-4" => {
                let i = word.as_bytes()[9] - b'1';
                q.quadrants[i as usize] = unquote(rest).to_owned();
            }
            _ => {
                let (name, at) = t.split_once(':').ok_or_else(|| Error::unexpected(line))?;
                let at = at.split(":::").next().unwrap_or(at).trim();
                let at = at
                    .strip_prefix('[')
                    .and_then(|a| a.split(']').next())
                    .ok_or_else(|| Error::unexpected(line))?;
                let (x, y) = at.split_once(',').ok_or_else(|| Error::unexpected(line))?;
                let (x, y): (f64, f64) = (
                    x.trim().parse().map_err(|_| Error::unexpected(line))?,
                    y.trim().parse().map_err(|_| Error::unexpected(line))?,
                );
                q.points.push((
                    unquote(name.trim()).to_owned(),
                    x.clamp(0.0, 1.0),
                    y.clamp(0.0, 1.0),
                ));
            }
        }
    }
    Ok(q)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/quadrant.rs"]
mod tests;
