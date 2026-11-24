//! One layer as one line of text (the user's own layers, in the settings):
//! `kind=garland;strand=wire;lights=0.5;colors=#f2c66d,#ffffff`.

use kernel::Rgba;

use super::{FieldError, FieldKind, FieldValue, LayerKind, LayerSpec, hex, number_text};

/// A layer as one line: its kind, then every option.
pub fn encode(layer: &LayerSpec) -> String {
    let mut out = format!("kind={}", layer.kind().id());
    for f in layer.kind().fields() {
        let Some(value) = layer.get(f.key) else { continue };
        let text = match value {
            FieldValue::Number(v) => number_text(v),
            FieldValue::Text(s) => s,
            FieldValue::Color(c) => hex(c),
            FieldValue::Colors(list) => list.iter().map(|c| hex(*c)).collect::<Vec<_>>().join(","),
        };
        out.push_str(&format!(";{}={text}", f.key));
    }
    out
}

/// A layer from its line (an unknown kind or option, a wrong value: why).
pub fn decode(line: &str) -> Result<LayerSpec, String> {
    let mut parts = line.split(';').map(str::trim).filter(|p| !p.is_empty());
    let kind = parts
        .next()
        .and_then(|p| p.strip_prefix("kind="))
        .and_then(LayerKind::parse)
        .ok_or_else(|| format!("no kind in `{line}`"))?;
    let mut spec = LayerSpec::new(kind);
    for part in parts {
        let (key, text) = part.split_once('=').ok_or_else(|| format!("`{part}`: no value"))?;
        let field =
            kind.field(key).ok_or_else(|| format!("{}.{key}: no such option", kind.id()))?;
        let value = match field.kind {
            FieldKind::Choice(_) | FieldKind::Text => FieldValue::Text(text.to_owned()),
            FieldKind::Amount(_) => FieldValue::Number(
                text.parse().map_err(|_| format!("{}.{key} = {text}: not a number", kind.id()))?,
            ),
            FieldKind::Color => FieldValue::Color(
                Rgba::from_hex(text).ok_or_else(|| format!("{}.{key} = {text}", kind.id()))?,
            ),
            FieldKind::Colors { .. } => FieldValue::Colors(
                text.split(',')
                    .filter(|c| !c.is_empty())
                    .map(|c| {
                        Rgba::from_hex(c.trim()).ok_or_else(|| format!("{}.{key} = {c}", kind.id()))
                    })
                    .collect::<Result<_, _>>()?,
            ),
        };
        spec.set(key, &value).map_err(|e| match e {
            FieldError::Unknown => format!("{}.{key}: no such option", kind.id()),
            FieldError::Invalid(why) => format!("{}.{key} = {text}: {why}", kind.id()),
        })?;
    }
    Ok(spec)
}
