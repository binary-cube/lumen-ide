//! `xychart-beta` (or `xychart`): a title, an x axis (categories `[a, b]` or a range
//! `0 --> 10`), a y axis (a title, a range), and series of bars and lines; `horizontal`
//! turns it.

use super::{Error, Line, Meta, unquote};

#[derive(Debug, Clone, PartialEq)]
pub enum Series {
    Bar(Vec<f64>),
    Line(Vec<f64>),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XyChart {
    pub horizontal: bool,
    pub x_title: Option<String>,
    pub categories: Vec<String>,
    pub x_range: Option<(f64, f64)>,
    pub y_title: Option<String>,
    pub y_range: Option<(f64, f64)>,
    pub series: Vec<Series>,
}

/// `[a, "b c", 3]`.
fn list(s: &str) -> Option<Vec<String>> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?;
    Some(inner.split(',').map(|p| unquote(p.trim()).to_owned()).filter(|p| !p.is_empty()).collect())
}

/// An axis' title, categories and range.
type Axis = (Option<String>, Option<Vec<String>>, Option<(f64, f64)>);

/// An axis: `"Title" [a, b]`, `"Title" 0 --> 10`, `[a, b]`, `0 --> 10` or a title alone.
fn axis(s: &str) -> Axis {
    let s = s.trim();
    let (title, rest) = if let Some(q) = s.strip_prefix('"') {
        let end = q.find('"').unwrap_or(q.len());
        (Some(q[..end].to_owned()), q.get(end + 1..).unwrap_or("").trim())
    } else if s.starts_with('[') || s.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-')
    {
        (None, s)
    } else {
        // A bare title, up to the list or the range.
        let end = s.find(['[']).or_else(|| s.find(|c: char| c.is_ascii_digit())).unwrap_or(s.len());
        (Some(s[..end].trim().to_owned()).filter(|t| !t.is_empty()), s[end..].trim())
    };
    if let Some(l) = list(rest) {
        return (title, Some(l), None);
    }
    if let Some((a, b)) = rest.split_once("-->")
        && let (Ok(a), Ok(b)) = (a.trim().parse(), b.trim().parse())
    {
        return (title, None, Some((a, b)));
    }
    (title, None, None)
}

pub(super) fn parse(head: Line<'_>, lines: &[Line<'_>], meta: &mut Meta) -> Result<XyChart, Error> {
    let mut c = XyChart { horizontal: head.text.contains("horizontal"), ..XyChart::default() };
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "title" => meta.title = Some(unquote(rest).to_owned()),
            "x-axis" => {
                let (title, cats, range) = axis(rest);
                c.x_title = title;
                c.categories = cats.unwrap_or_default();
                c.x_range = range;
            }
            "y-axis" => {
                let (title, _, range) = axis(rest);
                c.y_title = title;
                c.y_range = range;
            }
            "bar" | "line" => {
                // A series may be named first: `bar "Sales" [1, 2]`.
                let start = rest.find('[').ok_or_else(|| Error::unexpected(line))?;
                let values: Vec<f64> = list(&rest[start..])
                    .ok_or_else(|| Error::unexpected(line))?
                    .iter()
                    .map(|v| v.parse::<f64>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| Error::unexpected(line))?;
                c.series.push(if word == "bar" {
                    Series::Bar(values)
                } else {
                    Series::Line(values)
                });
            }
            _ => return Err(Error::unexpected(line)),
        }
    }
    Ok(c)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/xychart.rs"]
mod tests;
