//! `packet-beta` (or `packet`): fields over bit ranges (`0-15: "Source Port"`, `16: "Flag"`,
//! `+8: "Next"` continuing from the last).

use super::{Error, Line, Meta, unquote};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Packet {
    /// First bit, last bit, name.
    pub fields: Vec<(u32, u32, String)>,
}

pub(super) fn parse(lines: &[Line<'_>], meta: &mut Meta) -> Result<Packet, Error> {
    let mut p = Packet::default();
    let mut next = 0u32;
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(title) = t.strip_prefix("title ") {
            meta.title = Some(title.trim().to_owned());
            continue;
        }
        let (bits, name) = t.split_once(':').ok_or_else(|| Error::unexpected(line))?;
        let bits = bits.trim();
        let parse = |s: &str| s.trim().parse::<u32>().map_err(|_| Error::unexpected(line));
        let (a, b) = if let Some(n) = bits.strip_prefix('+') {
            let n = parse(n)?.max(1);
            (next, next + n - 1)
        } else if let Some((a, b)) = bits.split_once('-') {
            (parse(a)?, parse(b)?)
        } else {
            let a = parse(bits)?;
            (a, a)
        };
        if b < a {
            return Err(Error::unexpected(line));
        }
        p.fields.push((a, b, unquote(name.trim()).to_owned()));
        next = b + 1;
    }
    Ok(p)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/packet.rs"]
mod tests;
