//! A cell's text as it's shown: draw.io keeps HTML in it when the style says `html=1`
//! (`<b>`, `<br>`, `<div>`, entities): its lines kept, its tags left out, its entities
//! read; a plain value as it is.

/// `value`'s text (`html`: written as HTML).
pub fn plain(value: &str, html: bool) -> String {
    if !html {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(open) = rest.find('<') {
        out.push_str(&entities(&rest[..open]));
        let Some(close) = rest[open..].find('>') else {
            // A `<` that opens nothing: text.
            out.push_str(&entities(&rest[open..]));
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        let name = tag.trim_start_matches('/').split([' ', '/']).next().unwrap_or_default();
        // A line ends at a break and between blocks.
        let block = matches!(name, "br" | "div" | "p" | "li" | "tr" | "h1" | "h2" | "h3" | "h4");
        if block && !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(&entities(rest));
    out.trim_end_matches('\n').to_owned()
}

/// HTML entities read (`&amp;`, `&nbsp;`, `&#39;`, `&#x2713;`); an unknown one left as it is.
fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let Some(end) = after.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let name = &after[1..end];
        let read = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match read {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "../tests/unit/label.rs"]
mod tests;
