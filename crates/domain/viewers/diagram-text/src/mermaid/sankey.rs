//! `sankey-beta` (or `sankey`): flows as CSV rows — `source,target,value` (quoted fields
//! allowed).

use super::{Error, Line};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sankey {
    pub nodes: Vec<String>,
    /// From, to, how much.
    pub links: Vec<(usize, usize, f64)>,
}

/// A CSV row's fields (`"a, b",c` keeps its comma).
fn fields(row: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut field).trim().to_owned()),
            c => field.push(c),
        }
    }
    out.push(field.trim().to_owned());
    out
}

pub(super) fn parse(lines: &[Line<'_>]) -> Result<Sankey, Error> {
    let mut s = Sankey::default();
    let node = |s: &mut Sankey, name: &str| -> usize {
        match s.nodes.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                s.nodes.push(name.to_owned());
                s.nodes.len() - 1
            }
        }
    };
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let f = fields(t);
        let [a, b, v] = f.as_slice() else { return Err(Error::unexpected(line)) };
        let value: f64 = v.parse().map_err(|_| Error::unexpected(line))?;
        let (a, b) = (node(&mut s, a), node(&mut s, b));
        s.links.push((a, b, value.max(0.0)));
    }
    Ok(s)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/sankey.rs"]
mod tests;
