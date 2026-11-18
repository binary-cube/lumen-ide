//! XPath over an XML response, as chained requests read it (`{{login.response.body./a/b}}`,
//! as JSONPath for JSON): `/a/b` children by name, `//b` descendants, `*` any element,
//! `[2]` the second (from 1), `[@id='7']` the ones with that attribute, `@name` an
//! attribute's value, `text()` an element's text. What a path selects, as text, in document
//! order.

/// What `path` selects in `xml` (each as text: an element's text, an attribute's value).
pub fn select(xml: &str, path: &str) -> Result<Vec<String>, String> {
    let document = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let mut nodes: Vec<roxmltree::Node<'_, '_>> = vec![document.root()];
    let mut rest = path.trim();
    let mut out: Option<Vec<String>> = None;
    while !rest.is_empty() {
        let descend = rest.starts_with("//");
        rest = rest.trim_start_matches('/');
        let end = step_end(rest);
        let step = &rest[..end];
        rest = &rest[end..];
        if step == "text()" {
            out = Some(
                nodes.iter().map(|n| n.text().unwrap_or_default().trim().to_owned()).collect(),
            );
            break;
        }
        if let Some(attribute) = step.strip_prefix('@') {
            out = Some(
                nodes.iter().filter_map(|n| n.attribute(attribute)).map(str::to_owned).collect(),
            );
            break;
        }
        let (name, predicate) = match step.find('[') {
            Some(at) => (&step[..at], Some(step[at + 1..].trim_end_matches(']'))),
            None => (step, None),
        };
        let mut next = Vec::new();
        for node in &nodes {
            let candidates: Vec<roxmltree::Node<'_, '_>> = if descend {
                node.descendants().filter(|d| d != node).collect()
            } else {
                node.children().collect()
            };
            let mut matched: Vec<roxmltree::Node<'_, '_>> = candidates
                .into_iter()
                .filter(|c| c.is_element() && (name == "*" || c.tag_name().name() == name))
                .collect();
            if let Some(predicate) = predicate {
                matched = filter(matched, predicate)?;
            }
            next.extend(matched);
        }
        // `//` from nested nodes finds the same ones again: each once, in document order.
        if descend {
            next.sort_by_key(|n| n.range().start);
            next.dedup_by_key(|n| n.id());
        }
        nodes = next;
    }
    Ok(out.unwrap_or_else(|| nodes.iter().map(|n| text_of(*n)).collect()))
}

/// Where a step ends (the next `/` outside a predicate).
fn step_end(text: &str) -> usize {
    let mut depth = 0;
    for (i, c) in text.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            '/' if depth == 0 => return i,
            _ => {}
        }
    }
    text.len()
}

fn filter<'a, 'i>(
    nodes: Vec<roxmltree::Node<'a, 'i>>,
    predicate: &str,
) -> Result<Vec<roxmltree::Node<'a, 'i>>, String> {
    let predicate = predicate.trim();
    if let Ok(n) = predicate.parse::<usize>() {
        return Ok(nodes.into_iter().nth(n.saturating_sub(1)).into_iter().collect());
    }
    if predicate == "last()" {
        return Ok(nodes.into_iter().last().into_iter().collect());
    }
    if let Some(rest) = predicate.strip_prefix('@') {
        let (name, value) = match rest.split_once('=') {
            Some((name, value)) => (name.trim(), Some(value.trim().trim_matches(['\'', '"']))),
            None => (rest.trim(), None),
        };
        return Ok(nodes
            .into_iter()
            .filter(|n| match (n.attribute(name), value) {
                (Some(a), Some(v)) => a == v,
                (Some(_), None) => true,
                _ => false,
            })
            .collect());
    }
    Err(format!("an XPath predicate not understood: [{predicate}]"))
}

/// An element's text, all of it (its descendants' too).
fn text_of(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(|d| d.is_text())
        .filter_map(|d| d.text())
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
#[path = "../tests/unit/xpath.rs"]
mod tests;
