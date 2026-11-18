//! The grid's `WHERE` and `ORDER BY` fields: what's typed read into a structured filter
//! and order (columns by name, values as values — never passed on as text), and written
//! back as text (a quick filter from a cell's menu, a header clicked).
//!
//! `WHERE` takes comparisons (`=`, `<>` / `!=`, `<`, `<=`, `>`, `>=`) of a column with a
//! value (`'text'`, a number, `TRUE` / `FALSE`), `IS [NOT] NULL`, `[NOT] LIKE '%text%'`
//! (a column holding the text, any case), joined by `AND` / `OR`, `NOT`, in parentheses.
//! Names are bare or quoted (`"order"`, `` `order` ``, `[order]`), any case.

use super::source::{CellValue, GridColumn, SortKey};

/// How a filter compares a column with a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

impl Compare {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Less => "<",
            Self::LessEq => "<=",
            Self::Greater => ">",
            Self::GreaterEq => ">=",
        }
    }
}

/// Which rows the grid shows.
#[derive(Debug, Clone, PartialEq)]
pub enum GridFilter {
    Compare {
        column: String,
        op: Compare,
        value: CellValue,
    },
    IsNull(String),
    NotNull(String),
    /// The column's text holds `text` (any case).
    Contains {
        column: String,
        text: String,
    },
    And(Vec<GridFilter>),
    Or(Vec<GridFilter>),
    Not(Box<GridFilter>),
}

impl GridFilter {
    /// Both `self` and `other` (an `AND` flattened).
    pub fn and(self, other: GridFilter) -> GridFilter {
        let mut all = match self {
            Self::And(parts) => parts,
            one => vec![one],
        };
        match other {
            Self::And(parts) => all.extend(parts),
            one => all.push(one),
        }
        Self::And(all)
    }
}

/// Why a field's text wasn't understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Something else was expected where `found` is (`None`: the end).
    Unexpected {
        found: Option<String>,
    },
    /// A text not closed by its quote.
    Unterminated,
    UnknownColumn(String),
    /// `= NULL` (always false in SQL): `IS NULL` is meant.
    NullCompare,
    /// A `LIKE` pattern other than `'%text%'` or a plain text.
    Pattern(String),
}

impl ParseError {
    /// For the user.
    pub fn message(&self) -> String {
        match self {
            Self::Unexpected { found: Some(found) } => {
                i18n::tr!("grid.parse_unexpected", found = found.as_str())
            }
            Self::Unexpected { found: None } => i18n::t("grid.parse_end"),
            Self::Unterminated => i18n::t("grid.parse_unterminated"),
            Self::UnknownColumn(name) => i18n::tr!("grid.parse_column", name = name.as_str()),
            Self::NullCompare => i18n::t("grid.parse_null"),
            Self::Pattern(p) => i18n::tr!("grid.parse_pattern", pattern = p.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A name (bare, or quoted: then never a keyword).
    Name(String, bool),
    Text(String),
    Number(String),
    Op(Compare),
    Open,
    Close,
    Comma,
}

impl Token {
    fn shown(&self) -> String {
        match self {
            Self::Name(n, _) => n.clone(),
            Self::Text(t) => format!("'{t}'"),
            Self::Number(n) => n.clone(),
            Self::Op(op) => op.symbol().to_owned(),
            Self::Open => "(".to_owned(),
            Self::Close => ")".to_owned(),
            Self::Comma => ",".to_owned(),
        }
    }

    fn keyword(&self, word: &str) -> bool {
        matches!(self, Self::Name(n, false) if n.eq_ignore_ascii_case(word))
    }
}

fn tokens(text: &str) -> Result<Vec<Token>, ParseError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Token::Open);
                i += 1;
            }
            ')' => {
                out.push(Token::Close);
                i += 1;
            }
            ',' => {
                out.push(Token::Comma);
                i += 1;
            }
            '=' => {
                out.push(Token::Op(Compare::Eq));
                i += if chars.get(i + 1) == Some(&'=') { 2 } else { 1 };
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token::Op(Compare::NotEq));
                i += 2;
            }
            '<' => match chars.get(i + 1) {
                Some('=') => {
                    out.push(Token::Op(Compare::LessEq));
                    i += 2;
                }
                Some('>') => {
                    out.push(Token::Op(Compare::NotEq));
                    i += 2;
                }
                _ => {
                    out.push(Token::Op(Compare::Less));
                    i += 1;
                }
            },
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Token::Op(Compare::GreaterEq));
                    i += 2;
                } else {
                    out.push(Token::Op(Compare::Greater));
                    i += 1;
                }
            }
            '\'' | '"' | '`' | '[' => {
                let close = if c == '[' { ']' } else { c };
                let mut value = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err(ParseError::Unterminated),
                        // Doubled: the quote itself.
                        Some(&q) if q == close && chars.get(i + 1) == Some(&close) && c != '[' => {
                            value.push(q);
                            i += 2;
                        }
                        Some(&q) if q == close => {
                            i += 1;
                            break;
                        }
                        Some(&ch) => {
                            value.push(ch);
                            i += 1;
                        }
                    }
                }
                out.push(if c == '\'' { Token::Text(value) } else { Token::Name(value, true) });
            }
            c if c.is_ascii_digit()
                || ((c == '-' || c == '.')
                    && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit() || *n == '.')) =>
            {
                let start = i;
                i += 1;
                while let Some(&n) = chars.get(i) {
                    let exponent_sign = (n == '-' || n == '+') && matches!(chars[i - 1], 'e' | 'E');
                    if n.is_ascii_digit() || n == '.' || n == 'e' || n == 'E' || exponent_sign {
                        i += 1;
                    } else {
                        break;
                    }
                }
                out.push(Token::Number(chars[start..i].iter().collect()));
            }
            c if c.is_alphanumeric() || c == '_' || c == '$' => {
                let start = i;
                while chars.get(i).is_some_and(|n| n.is_alphanumeric() || *n == '_' || *n == '$') {
                    i += 1;
                }
                out.push(Token::Name(chars[start..i].iter().collect(), false));
            }
            other => return Err(ParseError::Unexpected { found: Some(other.to_string()) }),
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    columns: &'a [GridColumn],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    fn unexpected(&self) -> ParseError {
        ParseError::Unexpected { found: self.peek().map(Token::shown) }
    }

    fn eat_keyword(&mut self, word: &str) -> bool {
        if self.peek().is_some_and(|t| t.keyword(word)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn column(&mut self) -> Result<String, ParseError> {
        match self.next() {
            Some(Token::Name(name, _)) => column_named(self.columns, &name),
            _ => {
                self.at -= 1;
                Err(self.unexpected())
            }
        }
    }

    fn or(&mut self) -> Result<GridFilter, ParseError> {
        let mut parts = vec![self.and()?];
        while self.eat_keyword("or") {
            parts.push(self.and()?);
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { GridFilter::Or(parts) })
    }

    fn and(&mut self) -> Result<GridFilter, ParseError> {
        let mut parts = vec![self.not()?];
        while self.eat_keyword("and") {
            parts.push(self.not()?);
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { GridFilter::And(parts) })
    }

    fn not(&mut self) -> Result<GridFilter, ParseError> {
        if self.eat_keyword("not") {
            return Ok(GridFilter::Not(Box::new(self.not()?)));
        }
        if self.peek() == Some(&Token::Open) {
            self.at += 1;
            let inner = self.or()?;
            if self.next() != Some(Token::Close) {
                self.at -= 1;
                return Err(self.unexpected());
            }
            return Ok(inner);
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<GridFilter, ParseError> {
        let column = self.column()?;
        if self.eat_keyword("is") {
            let negated = self.eat_keyword("not");
            if !self.eat_keyword("null") {
                return Err(self.unexpected());
            }
            return Ok(if negated {
                GridFilter::NotNull(column)
            } else {
                GridFilter::IsNull(column)
            });
        }
        let negated = self.eat_keyword("not");
        if self.eat_keyword("like") {
            let Some(Token::Text(pattern)) = self.next() else {
                self.at -= 1;
                return Err(self.unexpected());
            };
            let filter = like(column, &pattern)?;
            return Ok(if negated { GridFilter::Not(Box::new(filter)) } else { filter });
        }
        if negated {
            return Err(self.unexpected());
        }
        let Some(Token::Op(op)) = self.next() else {
            self.at -= 1;
            return Err(self.unexpected());
        };
        let value = self.value()?;
        Ok(GridFilter::Compare { column, op, value })
    }

    fn value(&mut self) -> Result<CellValue, ParseError> {
        match self.next() {
            Some(Token::Text(t)) => Ok(CellValue::Text(t)),
            Some(Token::Number(n)) => number(&n).ok_or(ParseError::Unexpected { found: Some(n) }),
            Some(t) if t.keyword("true") => Ok(CellValue::Bool(true)),
            Some(t) if t.keyword("false") => Ok(CellValue::Bool(false)),
            Some(t) if t.keyword("null") => Err(ParseError::NullCompare),
            _ => {
                self.at -= 1;
                Err(self.unexpected())
            }
        }
    }
}

fn number(text: &str) -> Option<CellValue> {
    if let Ok(i) = text.parse::<i64>() {
        return Some(CellValue::Int(i));
    }
    text.parse::<f64>().ok().filter(|f| f.is_finite()).map(CellValue::Float)
}

/// `LIKE '%text%'` holds `text`; a pattern without wildcards is the text itself.
fn like(column: String, pattern: &str) -> Result<GridFilter, ParseError> {
    let inner = pattern.strip_prefix('%').and_then(|p| p.strip_suffix('%'));
    match inner {
        Some(text) if !text.contains(['%', '_']) => {
            Ok(GridFilter::Contains { column, text: text.to_owned() })
        }
        None if !pattern.contains(['%', '_']) => Ok(GridFilter::Compare {
            column,
            op: Compare::Eq,
            value: CellValue::Text(pattern.to_owned()),
        }),
        _ => Err(ParseError::Pattern(pattern.to_owned())),
    }
}

/// The column named `name` (any case; the exact case first), as the source names it.
fn column_named(columns: &[GridColumn], name: &str) -> Result<String, ParseError> {
    columns
        .iter()
        .find(|c| c.name == name)
        .or_else(|| columns.iter().find(|c| c.name.eq_ignore_ascii_case(name)))
        .map(|c| c.name.clone())
        .ok_or_else(|| ParseError::UnknownColumn(name.to_owned()))
}

/// The `WHERE` field's text as a filter (`None`: empty).
pub fn parse_where(text: &str, columns: &[GridColumn]) -> Result<Option<GridFilter>, ParseError> {
    let tokens = tokens(text)?;
    if tokens.is_empty() {
        return Ok(None);
    }
    let mut parser = Parser { tokens, at: 0, columns };
    let filter = parser.or()?;
    if parser.at < parser.tokens.len() {
        return Err(parser.unexpected());
    }
    Ok(Some(filter))
}

/// The `ORDER BY` field's text as an order (`a, b DESC`; empty: none).
pub fn parse_order(text: &str, columns: &[GridColumn]) -> Result<Vec<SortKey>, ParseError> {
    let tokens = tokens(text)?;
    let mut parser = Parser { tokens, at: 0, columns };
    let mut keys = Vec::new();
    while parser.peek().is_some() {
        let column = parser.column()?;
        let descending = if parser.eat_keyword("desc") {
            true
        } else {
            parser.eat_keyword("asc");
            false
        };
        // A column twice: its first place counts.
        if !keys.iter().any(|k: &SortKey| k.column == column) {
            keys.push(SortKey { column, descending });
        }
        match parser.next() {
            None => break,
            Some(Token::Comma) => {}
            Some(_) => {
                parser.at -= 1;
                return Err(parser.unexpected());
            }
        }
    }
    Ok(keys)
}

const KEYWORDS: [&str; 10] =
    ["and", "or", "not", "is", "null", "like", "true", "false", "asc", "desc"];

/// `name` as written in a field: bare when it can be, else in double quotes.
pub fn quote_name(name: &str) -> String {
    let bare = !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name));
    if bare { name.to_owned() } else { format!("\"{}\"", name.replace('"', "\"\"")) }
}

fn value_text(value: &CellValue) -> String {
    match value {
        CellValue::Null => "NULL".to_owned(),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::Int(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Text(t) | CellValue::Json(t) | CellValue::Other(t) => {
            format!("'{}'", t.replace('\'', "''"))
        }
        CellValue::Bytes(b) => format!("'{}'", String::from_utf8_lossy(b).replace('\'', "''")),
    }
}

/// `filter` as the `WHERE` field writes it (read back by [`parse_where`] as the same).
pub fn where_text(filter: &GridFilter) -> String {
    fn part(filter: &GridFilter, nested: bool) -> String {
        match filter {
            GridFilter::Compare { column, op, value } => {
                format!("{} {} {}", quote_name(column), op.symbol(), value_text(value))
            }
            GridFilter::IsNull(column) => format!("{} IS NULL", quote_name(column)),
            GridFilter::NotNull(column) => format!("{} IS NOT NULL", quote_name(column)),
            GridFilter::Contains { column, text } => {
                format!("{} LIKE '%{}%'", quote_name(column), text.replace('\'', "''"))
            }
            GridFilter::And(parts) | GridFilter::Or(parts) => {
                let joint = if matches!(filter, GridFilter::And(_)) { " AND " } else { " OR " };
                let inner: Vec<String> = parts.iter().map(|p| part(p, true)).collect();
                let text = inner.join(joint);
                if nested && parts.len() > 1 { format!("({text})") } else { text }
            }
            GridFilter::Not(inner) => format!("NOT {}", part(inner, true)),
        }
    }
    part(filter, false)
}

/// `keys` as the `ORDER BY` field writes them.
pub fn order_text(keys: &[SortKey]) -> String {
    keys.iter()
        .map(|k| {
            let name = quote_name(&k.column);
            if k.descending { format!("{name} DESC") } else { name }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The order after a click on `column`'s header: alone, ascending → descending → none;
/// with `multi` (⇧), the column is added last, turned descending, then taken out — the
/// others staying as they were.
pub fn cycle_sort(keys: &mut Vec<SortKey>, column: &str, multi: bool) {
    let at = keys.iter().position(|k| k.column == column);
    if multi {
        match at {
            None => keys.push(SortKey { column: column.to_owned(), descending: false }),
            Some(i) if !keys[i].descending => keys[i].descending = true,
            Some(i) => {
                keys.remove(i);
            }
        }
        return;
    }
    let next = match (keys.len(), at) {
        (1, Some(_)) if !keys[0].descending => {
            Some(SortKey { column: column.to_owned(), descending: true })
        }
        (1, Some(_)) => None,
        _ => Some(SortKey { column: column.to_owned(), descending: false }),
    };
    keys.clear();
    keys.extend(next);
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/filter.rs"]
mod tests;
