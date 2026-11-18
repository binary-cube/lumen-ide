//! Doc comments (`/** … */`, PHPDoc and JSDoc): the summary and the `@param`
//! descriptions of the declaration they precede. A parameter's name is written as its
//! language writes variables (with its sigil, `$a` in PHP; `...` before a variadic one).

/// How a language writes doc comments (given by its module, see
/// [`crate::LanguageIntelligence::doc_rules`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocRules {
    /// What opens and closes one (`/**`, `*/`).
    pub opener: &'static str,
    pub closer: &'static str,
    /// What opens an attribute between it and the declaration (`#[` in PHP), skipped.
    pub attribute: Option<&'static str>,
    /// What starts a decorator line between them (`@` in TypeScript), skipped.
    pub decorator: Option<char>,
    /// What the language writes variables after (`$`), for the `@param` names.
    pub sigil: Option<char>,
}

/// A declaration's doc comment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocBlock {
    /// The text before the first tag, paragraphs separated by `\n`.
    pub summary: String,
    pub params: Vec<DocParam>,
    /// It says `@deprecated`.
    pub deprecated: bool,
}

/// A `@param`: the name (without the sigil), the type if written, the description.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocParam {
    pub name: String,
    pub ty: Option<String>,
    pub desc: String,
}

impl DocBlock {
    fn find(&self, name: &str) -> Option<&DocParam> {
        let name = bare(name);
        self.params.iter().find(|p| p.name == name)
    }

    /// The description of parameter `name` (with or without its sigil).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.find(name).map(|p| p.desc.as_str()).filter(|d| !d.is_empty())
    }

    /// The type written for parameter `name` (`@param {number} a`, `@param int $a`).
    pub fn param_type(&self, name: &str) -> Option<&str> {
        self.find(name)?.ty.as_deref()
    }
}

/// `name` without what may lead it: `...`, its sigil (any non-name character).
fn bare(name: &str) -> &str {
    name.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '_')
}

/// The doc comment right before the declaration on the line of `offset`, as its language
/// writes them (`rules`): attributes (`#[Route(…)]`) or decorators between them are skipped.
pub fn doc_before(text: &str, offset: usize, rules: &DocRules) -> Option<DocBlock> {
    let offset = offset.min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let mut before = text[..line_start].trim_end();
    // Attributes (on one line or several: `#[AsCommand(\n name: …,\n)]`) / decorators on
    // their own lines.
    loop {
        if let Some(start) = rules.attribute.and_then(|open| attribute_start(before, open)) {
            before = before[..start].trim_end();
            continue;
        }
        let last_start = before.rfind('\n').map_or(0, |i| i + 1);
        let last = before[last_start..].trim();
        let decorator =
            rules.decorator.is_some_and(|d| last.starts_with(d) && !last.starts_with("@param"));
        // A one-line attribute the bracket matching can't read (`#[Route('/x')] // note`,
        // a `[` in one of its strings): its line.
        let attribute = rules.attribute.is_some_and(|open| last.starts_with(open));
        if decorator || attribute {
            before = before[..last_start].trim_end();
        } else {
            break;
        }
    }
    let body = before.strip_suffix(rules.closer)?;
    let open = body.rfind(rules.opener)?;
    // Not a doc comment if code sits between it and the declaration.
    Some(parse(&body[open + rules.opener.len()..], rules.sigil))
}

/// Where the attribute `text` ends with starts (its `open`, e.g. `#[`, matched across lines
/// and the brackets inside it), if it ends with one.
fn attribute_start(text: &str, open: &str) -> Option<usize> {
    let marker = open.strip_suffix('[')?;
    if !text.ends_with(']') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in text.char_indices().rev() {
        match c {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    return text[..i].ends_with(marker).then(|| i - marker.len());
                }
            }
            _ => {}
        }
    }
    None
}

fn parse(body: &str, sigil: Option<char>) -> DocBlock {
    let lines: Vec<&str> = body
        .lines()
        .map(|l| {
            let l = l.trim();
            let l = l.strip_prefix('*').unwrap_or(l);
            l.strip_prefix(' ').unwrap_or(l).trim_end()
        })
        .collect();
    let mut doc = DocBlock::default();
    let mut summary: Vec<String> = Vec::new();
    let mut paragraph = String::new();
    let mut in_tags = false;
    // The tag being read (continuation lines are appended to it).
    let mut current: Option<usize> = None;
    for line in lines {
        if let Some(tag) = line.strip_prefix('@') {
            in_tags = true;
            current = None;
            if tag.split_whitespace().next() == Some("deprecated") {
                doc.deprecated = true;
            }
            if let Some(rest) = tag.strip_prefix("param")
                && let Some(param) = param(rest, sigil)
            {
                doc.params.push(param);
                current = Some(doc.params.len() - 1);
            }
            continue;
        }
        if in_tags {
            if let (Some(i), false) = (current, line.is_empty()) {
                let desc = &mut doc.params[i].desc;
                if !desc.is_empty() {
                    desc.push(' ');
                }
                desc.push_str(line.trim());
            }
            continue;
        }
        if line.is_empty() {
            if !paragraph.is_empty() {
                summary.push(std::mem::take(&mut paragraph));
            }
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(line);
        }
    }
    if !paragraph.is_empty() {
        summary.push(paragraph);
    }
    doc.summary = summary.join("\n").replace('`', "");
    doc
}

/// `@param` after the tag: with a sigil `Type $name desc` / `$name desc` (PHPDoc), JSDoc
/// `{Type} name desc`.
fn param(rest: &str, sigil: Option<char>) -> Option<DocParam> {
    let mut rest = rest.trim();
    let mut ty = None;
    if rest.starts_with('{') {
        let close = rest.find('}')?;
        ty = Some(rest[1..close].trim().to_owned());
        rest = rest[close + 1..].trim_start();
    }
    let words: Vec<&str> = rest.split_whitespace().collect();
    // The name is the first word starting with the sigil, else the first word (JSDoc).
    let sigiled = sigil.and_then(|sigil| {
        words.iter().position(|w| w.trim_start_matches("...").starts_with(sigil))
    });
    let name_at = sigiled.unwrap_or(0);
    let name = *words.get(name_at)?;
    if let (Some(at), None) = (sigiled, &ty) {
        ty = (at > 0).then(|| words[..at].join(" "));
    }
    let desc = words[name_at + 1..].join(" ");
    let desc = desc.trim_start_matches(['-', ' ']).replace('`', "");
    let name = bare(name);
    let name = name.trim_matches(['[', ']']).split('=').next().unwrap_or(name);
    Some(DocParam { name: name.to_owned(), ty, desc })
}

#[cfg(test)]
#[path = "../tests/unit/docs.rs"]
mod tests;
