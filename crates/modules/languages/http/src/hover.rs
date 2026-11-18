//! The card of a `{{variable}}` under the mouse: its value in the file (`@name = …`) and in
//! each environment, a private one's hidden (it's a secret: only that it's there is said);
//! a dynamic one's meaning; "defined nowhere" when nothing has it.

use std::ops::Range;

use code_intelligence::{CompletionKind, HoverInfo, HoverParam};
use http_requests::env::{Environments, SHARED};

/// The `{{name}}` at `offset` of `text`: the name's bytes and the name.
pub fn variable_at(text: &str, offset: usize) -> Option<(Range<usize>, &str)> {
    http_requests::vars::references(text).into_iter().find_map(|(open, name)| {
        let close = open + text[open..].find("}}")? + 2;
        (open..close).contains(&offset).then(|| {
            let start = open + 2 + text[open + 2..close].find(name).unwrap_or(0);
            (start..start + name.len(), name)
        })
    })
}

/// The card of variable `name` (at `range`) in a file reading `text`, its environments
/// `environments`.
pub fn card(text: &str, name: &str, range: Range<usize>, environments: &Environments) -> HoverInfo {
    let title = Some((i18n::t("http.hover_title"), Some(CompletionKind::Variable)));
    if name.starts_with('$') {
        let base = name.split('(').next().unwrap_or(name);
        let meaning = crate::complete::DYNAMIC
            .iter()
            .find(|(n, _)| n.split('(').next() == Some(base))
            .map(|(_, what)| (*what).to_owned());
        return HoverInfo {
            range,
            code: name.to_owned(),
            title,
            doc: meaning,
            ..HoverInfo::default()
        };
    }
    let mut params = Vec::new();
    let file = http_requests::parse(text);
    if let Some(variable) = file.variables.iter().rev().find(|v| v.name == name) {
        params.push(HoverParam {
            ty: Some(i18n::t("http.this_file")),
            name: variable.value.clone(),
            doc: None,
        });
    }
    let shown = |secret: bool, value: &str| {
        if secret { "••••••".to_owned() } else { value.to_owned() }
    };
    for environment in environments.names().map(Some).chain([None]) {
        let Some(variable) = environments.resolve(environment, name) else { continue };
        // `$shared` once: under its own name, not under each environment it fills.
        if environment.is_some() && environments.resolve(None, name).is_some_and(|v| v == variable)
        {
            continue;
        }
        params.push(HoverParam {
            ty: Some(environment.unwrap_or(SHARED).to_owned()),
            name: shown(variable.source.is_secret(), &variable.value),
            doc: variable.source.is_secret().then(|| i18n::t("http.private_value")),
        });
    }
    HoverInfo {
        range,
        code: format!("{{{{{name}}}}}"),
        title,
        doc: params.is_empty().then(|| i18n::t("http.undefined")),
        params_label: Some(i18n::t("http.hover_values")),
        params,
        ..HoverInfo::default()
    }
}

#[cfg(test)]
#[path = "../tests/unit/hover.rs"]
mod tests;
