//! A template read into its parts (see [`super`]'s doc for the language): its header, the
//! text before the rows, the row's body, the text after — or the first error, with its
//! line and column.

use super::{Expr, Filter, Node, Section, Template, TemplateError};

/// At most this much text in a template.
pub const MAX_LEN: usize = 256 << 10;
/// At most this many sections one in another.
pub const MAX_DEPTH: usize = 16;

/// Where a byte is, 1-based (a column counted in characters).
pub fn place(text: &str, at: usize) -> (u32, u32) {
    let at = at.min(text.len());
    let before = &text[..at];
    let line = before.matches('\n').count() as u32 + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = text[start..at].chars().count() as u32 + 1;
    (line, column)
}

fn error(text: &str, at: usize, message: String) -> TemplateError {
    let (line, column) = place(text, at);
    TemplateError { line, column, message }
}

/// The header (`---` lines with `name:` and `extension:`), if the text starts with one: the
/// name, the extension, where the body starts.
fn header(text: &str) -> Result<(Option<String>, Option<String>, usize), TemplateError> {
    let first_end = text.find('\n');
    let first = text[..first_end.unwrap_or(text.len())].trim_end_matches('\r');
    if first.trim() != "---" {
        return Ok((None, None, 0));
    }
    let (mut name, mut extension) = (None, None);
    let mut at = first_end.map_or(text.len() + 1, |e| e + 1);
    loop {
        if at > text.len() {
            return Err(error(text, 0, i18n::t("extract.template_header_open")));
        }
        let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
        let line = text[at..end].trim_end_matches('\r').trim();
        if line == "---" {
            return Ok((name, extension, (end + 1).min(text.len())));
        }
        if !line.is_empty() && !line.starts_with('#') {
            let Some((key, value)) = line.split_once(':') else {
                return Err(error(text, at, i18n::t("extract.template_header_line")));
            };
            let (key, value) = (key.trim(), value.trim().to_owned());
            let fine = !value.is_empty()
                && (key != "extension" || value.chars().all(|c| c.is_ascii_alphanumeric()));
            match key {
                "name" | "extension" if !fine => {
                    return Err(error(
                        text,
                        at,
                        i18n::tr!("extract.template_header_value", key = key),
                    ));
                }
                "name" => name = Some(value),
                "extension" => extension = Some(value.to_ascii_lowercase()),
                other => {
                    return Err(error(
                        text,
                        at,
                        i18n::tr!("extract.template_header_key", key = other),
                    ));
                }
            }
        }
        at = end + 1;
    }
}

/// A tag between `{{` and `}}`, where it is.
#[derive(Debug, Clone)]
struct Tag<'a> {
    /// Its content, trimmed.
    inner: &'a str,
    /// Where `{{` starts.
    start: usize,
    /// Where its content starts (for the errors' places).
    inner_at: usize,
}

/// The text cut into texts and tags (`\{{` is a literal `{{`).
enum Piece<'a> {
    Text(String),
    Tag(Tag<'a>),
}

fn pieces(text: &str, from: usize) -> Result<Vec<Piece<'_>>, TemplateError> {
    let mut out = Vec::new();
    let mut literal = String::new();
    let mut at = from;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with("\\{{") || rest.starts_with("\\{#") {
            literal.push_str(&rest[1..3]);
            at += 3;
            continue;
        }
        // `{# … #}`: a comment (as `{{! … }}`).
        if let Some(after) = rest.strip_prefix("{#") {
            let Some(close) = after.find("#}") else {
                return Err(error(text, at, i18n::t("extract.template_unclosed_comment")));
            };
            if !literal.is_empty() {
                out.push(Piece::Text(std::mem::take(&mut literal)));
            }
            out.push(Piece::Tag(Tag { inner: "!", start: at, inner_at: at + 2 }));
            at += 2 + close + 2;
            continue;
        }
        if let Some(after) = rest.strip_prefix("{{") {
            let Some(close) = after.find("}}") else {
                return Err(error(text, at, i18n::t("extract.template_unclosed_tag")));
            };
            let raw = &after[..close];
            if let Some(nested) = raw.find("{{") {
                return Err(error(text, at + 2 + nested, i18n::t("extract.template_unclosed_tag")));
            }
            if !literal.is_empty() {
                out.push(Piece::Text(std::mem::take(&mut literal)));
            }
            let lead = raw.len() - raw.trim_start().len();
            out.push(Piece::Tag(Tag { inner: raw.trim(), start: at, inner_at: at + 2 + lead }));
            at += 2 + close + 2;
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        literal.push(c);
        at += c.len_utf8();
    }
    if !literal.is_empty() {
        out.push(Piece::Text(literal));
    }
    Ok(out)
}

/// A section tag alone on its line (with only blanks around it) takes its line with it, as
/// Mustache does: the blanks before it and the line break after it are dropped.
fn standalone(pieces: &mut [Piece<'_>]) {
    let is_block =
        |p: &Piece<'_>| matches!(p, Piece::Tag(t) if t.inner.starts_with(['#', '^', '/', '!']));
    let blanks = |s: &str| s.chars().all(|c| c == ' ' || c == '\t');
    // Which ones are alone, read on the text as written (before anything is dropped).
    let alone: Vec<bool> = (0..pieces.len())
        .map(|i| {
            if !is_block(&pieces[i]) {
                return false;
            }
            let before = match i.checked_sub(1).map(|j| &pieces[j]) {
                None => true,
                Some(Piece::Text(t)) => {
                    let tail = t.rsplit('\n').next().unwrap_or("");
                    blanks(tail) && (t.contains('\n') || i == 1)
                }
                Some(Piece::Tag(_)) => false,
            };
            let after = match pieces.get(i + 1) {
                None => true,
                Some(Piece::Text(t)) => match t.find('\n') {
                    Some(n) => blanks(t[..n].trim_end_matches('\r')),
                    None => i + 2 == pieces.len() && blanks(t),
                },
                Some(Piece::Tag(_)) => false,
            };
            before && after
        })
        .collect();
    for (i, alone) in alone.into_iter().enumerate() {
        if !alone {
            continue;
        }
        if let Some(Piece::Text(t)) = i.checked_sub(1).map(|j| &mut pieces[j]) {
            let keep = t.rfind('\n').map_or(0, |n| n + 1);
            t.truncate(keep);
        }
        if let Some(Piece::Text(t)) = pieces.get_mut(i + 1) {
            let cut = t.find('\n').map_or(t.len(), |n| n + 1);
            t.drain(..cut);
        }
    }
}

/// What a section is about, and the loops it may be in.
fn section_of(name: &str) -> Option<Section> {
    Some(match name {
        "rows" => Section::Rows,
        "columns" => Section::Columns,
        "first" => Section::First,
        "last" => Section::Last,
        "null" => Section::Null,
        "key" => Section::Key,
        "number" => Section::Number,
        _ => return None,
    })
}

fn expr_of(name: &str) -> Option<Expr> {
    Some(match name {
        "value" => Expr::Value,
        "table" => Expr::Table,
        "column.name" => Expr::ColumnName,
        "column.type" => Expr::ColumnType,
        "column.kind" => Expr::ColumnKind,
        "column.index" => Expr::ColumnIndex,
        "row.index" => Expr::RowIndex,
        "row.number" => Expr::RowNumber,
        _ => return None,
    })
}

fn filter_of(text: &str) -> Result<Filter, String> {
    let (name, arg) = match text.split_once(':') {
        Some((n, a)) => (n.trim(), Some(a.trim())),
        None => (text.trim(), None),
    };
    let plain = |f: Filter| match arg {
        None => Ok(f),
        Some(_) => Err(i18n::tr!("extract.template_filter_arg", name = name)),
    };
    match name {
        "sql" => plain(Filter::Sql),
        "json" => plain(Filter::Json),
        "csv" => plain(Filter::Csv),
        "html" => plain(Filter::Html),
        "xml" => plain(Filter::Xml),
        "md" => plain(Filter::Md),
        "php" => plain(Filter::Php),
        "py" => plain(Filter::Py),
        "upper" => plain(Filter::Upper),
        "lower" => plain(Filter::Lower),
        "trim" => plain(Filter::Trim),
        "pad" => match arg.and_then(|a| a.parse::<i32>().ok()).filter(|n| n.unsigned_abs() <= 1000)
        {
            Some(n) => Ok(Filter::Pad(n)),
            None => Err(i18n::t("extract.template_pad")),
        },
        "" => Err(i18n::t("extract.template_filter_empty")),
        other => Err(i18n::tr!("extract.template_filter_unknown", name = other)),
    }
}

/// Where the parser is: the sections open (each with its kind and where it opened).
struct Open {
    section: Section,
    inverted: bool,
    at: usize,
    body: Vec<Node>,
}

/// What an expression or a section needs around it, said when it's missing.
fn check_place(
    text: &str,
    at: usize,
    needs_row: bool,
    needs_column: bool,
    open: &[Open],
) -> Result<(), TemplateError> {
    let in_rows = open.iter().any(|o| o.section == Section::Rows && !o.inverted);
    let in_columns = open.iter().any(|o| o.section == Section::Columns && !o.inverted);
    if needs_column && !in_columns {
        return Err(error(text, at, i18n::t("extract.template_needs_columns")));
    }
    if needs_row && !in_rows {
        return Err(error(text, at, i18n::t("extract.template_needs_rows")));
    }
    Ok(())
}

/// `text` read as a template named `stem` (when its header names none).
pub fn parse(text: &str, stem: &str) -> Result<Template, TemplateError> {
    if text.len() > MAX_LEN {
        return Err(TemplateError {
            line: 1,
            column: 1,
            message: i18n::t("extract.template_too_long"),
        });
    }
    let (name, extension, body_at) = header(text)?;
    let mut pieces = pieces(text, body_at)?;
    standalone(&mut pieces);
    let mut open: Vec<Open> = Vec::new();
    let mut root: Vec<Node> = Vec::new();
    // The rows' section, once: where it is in the top level.
    let mut rows_at: Option<usize> = None;
    for piece in pieces {
        let node = match piece {
            Piece::Text(t) => {
                if t.is_empty() {
                    continue;
                }
                Node::Text(t)
            }
            Piece::Tag(tag) => {
                let inner = tag.inner;
                if inner.starts_with('!') {
                    continue;
                }
                if let Some(name) = inner.strip_prefix('#').or_else(|| inner.strip_prefix('^')) {
                    let inverted = inner.starts_with('^');
                    let name_at = tag.inner_at + 1 + (name.len() - name.trim_start().len());
                    let name = name.trim();
                    let Some(section) = section_of(name) else {
                        return Err(error(
                            text,
                            name_at,
                            i18n::tr!("extract.template_section_unknown", name = name),
                        ));
                    };
                    let looped = matches!(section, Section::Rows | Section::Columns);
                    if inverted && looped {
                        return Err(error(
                            text,
                            tag.inner_at,
                            i18n::tr!("extract.template_not_inverted", name = name),
                        ));
                    }
                    if open.len() >= MAX_DEPTH {
                        return Err(error(
                            text,
                            tag.inner_at,
                            i18n::t("extract.template_too_deep"),
                        ));
                    }
                    match section {
                        Section::Rows => {
                            if !open.is_empty() {
                                return Err(error(
                                    text,
                                    tag.inner_at,
                                    i18n::t("extract.template_rows_top"),
                                ));
                            }
                            if rows_at.is_some() {
                                return Err(error(
                                    text,
                                    tag.inner_at,
                                    i18n::t("extract.template_rows_once"),
                                ));
                            }
                        }
                        Section::Columns => {
                            if open.iter().any(|o| o.section == Section::Columns) {
                                return Err(error(
                                    text,
                                    tag.inner_at,
                                    i18n::t("extract.template_columns_nested"),
                                ));
                            }
                        }
                        Section::First | Section::Last => {
                            if !open
                                .iter()
                                .any(|o| matches!(o.section, Section::Rows | Section::Columns))
                            {
                                return Err(error(
                                    text,
                                    tag.inner_at,
                                    i18n::t("extract.template_needs_loop"),
                                ));
                            }
                        }
                        Section::Null | Section::Number => {
                            check_place(text, tag.inner_at, true, true, &open)?;
                        }
                        Section::Key => check_place(text, tag.inner_at, false, true, &open)?,
                    }
                    open.push(Open { section, inverted, at: tag.start, body: Vec::new() });
                    continue;
                }
                if let Some(name) = inner.strip_prefix('/') {
                    let name = name.trim();
                    let Some(last) = open.pop() else {
                        return Err(error(
                            text,
                            tag.inner_at,
                            i18n::tr!("extract.template_close_unopened", name = name),
                        ));
                    };
                    if section_of(name) != Some(last.section) {
                        let (line, column) = place(text, last.at);
                        return Err(error(
                            text,
                            tag.inner_at,
                            i18n::tr!(
                                "extract.template_close_other",
                                name = name,
                                open = last.section.name(),
                                line = line,
                                column = column
                            ),
                        ));
                    }
                    let node = Node::Section {
                        section: last.section,
                        inverted: last.inverted,
                        body: last.body,
                    };
                    if last.section == Section::Rows {
                        rows_at = Some(root.len());
                    }
                    match open.last_mut() {
                        Some(parent) => parent.body.push(node),
                        None => root.push(node),
                    }
                    continue;
                }
                // An output: an expression and its filters.
                let mut parts = inner.split('|');
                let head = parts.next().unwrap_or("").trim();
                let Some(expr) = expr_of(head) else {
                    let message = if head.is_empty() {
                        i18n::t("extract.template_empty_tag")
                    } else {
                        i18n::tr!("extract.template_field_unknown", name = head)
                    };
                    return Err(error(text, tag.inner_at, message));
                };
                match expr {
                    Expr::Value => check_place(text, tag.inner_at, true, true, &open)?,
                    Expr::ColumnName | Expr::ColumnType | Expr::ColumnKind | Expr::ColumnIndex => {
                        check_place(text, tag.inner_at, false, true, &open)?;
                    }
                    Expr::RowIndex | Expr::RowNumber => {
                        check_place(text, tag.inner_at, true, false, &open)?;
                    }
                    Expr::Table => {}
                }
                let mut filters = Vec::new();
                let mut offset = tag.inner_at + inner.find('|').unwrap_or(inner.len());
                for part in parts {
                    let part_at = offset + 1 + (part.len() - part.trim_start().len());
                    match filter_of(part) {
                        Ok(f) => filters.push(f),
                        Err(message) => return Err(error(text, part_at, message)),
                    }
                    offset += 1 + part.len();
                }
                Node::Output { expr, filters }
            }
        };
        match open.last_mut() {
            Some(parent) => parent.body.push(node),
            None => root.push(node),
        }
    }
    if let Some(last) = open.last() {
        return Err(error(
            text,
            last.at,
            i18n::tr!("extract.template_unclosed_section", name = last.section.name()),
        ));
    }
    let (before, row, after) = match rows_at {
        Some(at) => {
            let mut before = root;
            let mut after = before.split_off(at);
            let rows = after.remove(0);
            let Node::Section { body, .. } = rows else {
                unreachable!("the rows' section was put here")
            };
            (before, Some(body), after)
        }
        None => (root, None, Vec::new()),
    };
    Ok(Template {
        name: name.unwrap_or_else(|| stem.to_owned()),
        extension: extension.unwrap_or_else(|| "txt".to_owned()),
        before,
        row,
        after,
    })
}
