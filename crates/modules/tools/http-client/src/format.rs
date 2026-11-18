//! A response's body as it's shown: its kind from its media type (JSON, XML, HTML, YAML,
//! text, an image, binary), its text made readable — JSON indented, XML indented by its
//! tags —, the extension it's saved with (the editor colors it by that).

/// What a body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Json,
    Xml,
    Html,
    Yaml,
    Text,
    Image,
    Binary,
}

/// `media_type`'s kind (`None`: unsaid — text).
pub fn kind(media_type: Option<&str>) -> Kind {
    let Some(media) = media_type else { return Kind::Text };
    if media.ends_with("json") || media.ends_with("+json") {
        Kind::Json
    } else if media == "text/html" || media.ends_with("xhtml+xml") {
        Kind::Html
    } else if media.ends_with("xml") {
        Kind::Xml
    } else if media.contains("yaml") {
        Kind::Yaml
    } else if media.starts_with("image/") {
        Kind::Image
    } else if media.starts_with("text/")
        || media.ends_with("javascript")
        || media.contains("x-www-form-urlencoded")
        || media.ends_with("graphql")
    {
        Kind::Text
    } else {
        Kind::Binary
    }
}

/// The extension a body of `media_type` is saved with.
pub fn extension(media_type: Option<&str>) -> &'static str {
    match kind(media_type) {
        Kind::Json => "json",
        Kind::Xml => "xml",
        Kind::Html => "html",
        Kind::Yaml => "yaml",
        Kind::Image => match media_type.unwrap_or_default() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/svg+xml" => "svg",
            "image/webp" => "webp",
            _ => "img",
        },
        Kind::Text => "txt",
        Kind::Binary => "bin",
    }
}

/// `bytes` (a body of `kind`) as text to read: JSON indented (as it was if it isn't
/// JSON), XML indented by its tags, the rest as it is (invalid UTF-8 replaced).
pub fn readable(kind: Kind, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    match kind {
        Kind::Json => serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| text.into_owned()),
        Kind::Xml => indent_xml(&text),
        _ => text.into_owned(),
    }
}

/// XML with each tag on its line, indented by its depth (text between tags kept on the
/// tag's line). A document already laid out over lines is left as it is.
pub fn indent_xml(text: &str) -> String {
    if text.trim().lines().count() > 2 {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut depth: usize = 0;
    let mut rest = text.trim();
    while let Some(open) = rest.find('<') {
        let between = rest[..open].trim();
        if !between.is_empty() {
            out.push_str(between);
        }
        let Some(close) = rest[open..].find('>').map(|n| open + n) else { break };
        let tag = &rest[open..=close];
        let closing = tag.starts_with("</");
        let alone = tag.ends_with("/>") || tag.starts_with("<?") || tag.starts_with("<!");
        if closing {
            depth = depth.saturating_sub(1);
        }
        // A closing tag right after its text stays on the text's line.
        if !(closing && !between.is_empty()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
        }
        out.push_str(tag);
        if !closing && !alone {
            depth += 1;
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest.trim());
    out
}

/// A size as people read it (`88.02 kB`).
pub fn size(bytes: u64) -> String {
    match bytes {
        0..1000 => format!("{bytes} B"),
        1000..1_000_000 => format!("{:.2} kB", bytes as f64 / 1000.0),
        _ => format!("{:.2} MB", bytes as f64 / 1_000_000.0),
    }
}

/// A request method's color (as the gutter's, the log's: a read green, a write amber, a
/// removal red), for the graph's cards and the HTTP window's rows.
pub fn method_color(method: &str, p: &ui_components::Palette) -> kernel::Rgba {
    match method {
        "GET" | "HEAD" | "OPTIONS" => p.vcs_added,
        "DELETE" => p.danger,
        "POST" | "PUT" | "PATCH" => p.warning,
        _ => p.accent,
    }
}

#[cfg(test)]
#[path = "../tests/unit/format.rs"]
mod tests;
