//! The lexer: SQL text as tokens, in one pass, each a byte range of the text (nothing is
//! allocated per token). It never fails: a string, a quoted name, a body or a comment left
//! open runs to the end and is marked [`Token::unterminated`] — what's typed in the editor is
//! often unfinished.

use super::Flavor;

/// A piece of SQL text: what it is and where (`start..end`, bytes, always on char
/// boundaries). The tokens of a text cover it whole, in order, without overlapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    /// A string, quoted name, body or block comment the text ends inside of.
    pub unterminated: bool,
}

impl Token {
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    /// Its text in `text` (the text it was read from).
    pub fn text<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }

    /// Whitespace or a comment: what doesn't change what a statement says.
    pub fn is_trivia(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Whitespace | TokenKind::LineComment | TokenKind::BlockComment
        )
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.kind, TokenKind::LineComment | TokenKind::BlockComment)
    }

    /// A word (keyword or name) equal to `keyword`, whatever its case.
    pub fn is_word(&self, text: &str, keyword: &str) -> bool {
        self.kind == TokenKind::Word && self.text(text).eq_ignore_ascii_case(keyword)
    }

    /// A word or a quoted name: something naming an object.
    pub fn is_name(&self) -> bool {
        matches!(self.kind, TokenKind::Word | TokenKind::QuotedIdent)
    }
}

/// What a token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A keyword or an unquoted name (`SELECT`, `users`, `a$b`).
    Word,
    /// A quoted name: `"x"` (`""` inside), `` `x` `` (MySQL, SQLite), `[x]` (SQLite),
    /// `U&"x"` (PostgreSQL).
    QuotedIdent,
    /// A string literal.
    String(StringKind),
    /// A PostgreSQL dollar-quoted body: `$$ … $$`, `$tag$ … $tag$`.
    DollarBody,
    /// `42`, `1.5`, `.5`, `1e-3`, `0x1F`, `1_000`.
    Number,
    /// An operator (`=`, `<>`, `||`, `->>`, `@>`…).
    Operator,
    /// PostgreSQL's cast, `::`.
    Cast,
    OpenParen,
    CloseParen,
    Comma,
    Dot,
    /// Any other punctuation (`[`, `]`, `{`, `}`, a character SQL doesn't use).
    Punct,
    Semicolon,
    /// `-- …` (and `# …` in MySQL), up to the end of its line (the line break not in it).
    LineComment,
    /// `/* … */` (nested in PostgreSQL).
    BlockComment,
    /// A placeholder for a value given when the statement runs.
    Parameter(ParamKind),
    /// Spaces, tabs and line breaks (`\n`, `\r\n`, `\r`).
    Whitespace,
}

/// How a string literal is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringKind {
    /// `'…'`, a quote doubled inside (MySQL also reads backslashes, and `"…"`).
    Plain,
    /// PostgreSQL's `E'…'`: backslash escapes.
    Escape,
    /// `X'…'`: bytes in hexadecimal.
    Binary,
    /// `B'…'`: bits.
    Bit,
    /// `N'…'`: national characters.
    National,
    /// PostgreSQL's `U&'…'`: Unicode escapes.
    Unicode,
}

/// How a parameter is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// `?`: the next one in order.
    Positional,
    /// `?1`, `$1`.
    Numbered,
    /// `:name`, `@name` (SQLite, MySQL), `$name` (SQLite).
    Named,
}

/// `text` as tokens, read as `flavor` writes SQL.
pub fn lex(text: &str, flavor: Flavor) -> Vec<Token> {
    let mut lexer = Lexer { text, bytes: text.as_bytes(), flavor, pos: 0 };
    // Roughly a token every 4 bytes: one allocation for most texts.
    let mut tokens = Vec::with_capacity(text.len() / 4 + 1);
    let mut prev_significant: Option<TokenKind> = None;
    while lexer.pos < text.len() {
        let start = lexer.pos;
        let (kind, unterminated) = lexer.next(prev_significant);
        debug_assert!(lexer.pos > start, "the lexer always moves");
        let token = Token { kind, start, end: lexer.pos, unterminated };
        if !token.is_trivia() {
            prev_significant = Some(kind);
        }
        tokens.push(token);
    }
    tokens
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    flavor: Flavor,
    pos: usize,
}

fn is_word_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_word_char(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}

/// What makes up an operator, `?`, `@`, `:` and `#` aside (read by their own rules).
fn is_operator_byte(b: u8) -> bool {
    matches!(
        b,
        b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'=' | b'~' | b'!' | b'%' | b'^' | b'&' | b'|'
    )
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    fn char_at(&self, pos: usize) -> Option<char> {
        self.text.get(pos..).and_then(|rest| rest.chars().next())
    }

    fn postgres(&self) -> bool {
        matches!(self.flavor, Flavor::Postgres { .. })
    }

    fn mysql(&self) -> bool {
        self.flavor == Flavor::MySql
    }

    fn sqlite(&self) -> bool {
        self.flavor == Flavor::Sqlite
    }

    fn next(&mut self, prev: Option<TokenKind>) -> (TokenKind, bool) {
        let b = self.bytes[self.pos];
        match b {
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c' => {
                self.whitespace();
                (TokenKind::Whitespace, false)
            }
            b'-' if self.peek(1) == Some(b'-') && self.line_comment_dashes() => {
                self.skip_line();
                (TokenKind::LineComment, false)
            }
            b'#' if self.mysql() => {
                self.skip_line();
                (TokenKind::LineComment, false)
            }
            b'/' if self.peek(1) == Some(b'*') => {
                let closed = self.block_comment();
                (TokenKind::BlockComment, !closed)
            }
            b'\'' => {
                let closed = self.quoted(b'\'', self.mysql());
                (TokenKind::String(StringKind::Plain), !closed)
            }
            b'"' if self.mysql() => {
                let closed = self.quoted(b'"', true);
                (TokenKind::String(StringKind::Plain), !closed)
            }
            b'"' => {
                let closed = self.quoted(b'"', false);
                (TokenKind::QuotedIdent, !closed)
            }
            b'`' if self.mysql() || self.sqlite() => {
                let closed = self.quoted(b'`', false);
                (TokenKind::QuotedIdent, !closed)
            }
            b'[' if self.sqlite() => {
                self.pos += 1;
                let closed = self.until(b']');
                (TokenKind::QuotedIdent, !closed)
            }
            b'$' => self.dollar(),
            b'?' => self.question(),
            b':' => self.colon(),
            b'@' if !self.postgres() => self.named_or_operator(1),
            b'0'..=b'9' => {
                self.number();
                (TokenKind::Number, false)
            }
            b'.' if self.peek(1).is_some_and(|d| d.is_ascii_digit())
                && !matches!(
                    prev,
                    Some(TokenKind::Word | TokenKind::QuotedIdent | TokenKind::CloseParen)
                ) =>
            {
                self.number();
                (TokenKind::Number, false)
            }
            b'(' => self.one(TokenKind::OpenParen),
            b')' => self.one(TokenKind::CloseParen),
            b',' => self.one(TokenKind::Comma),
            b'.' => self.one(TokenKind::Dot),
            b';' => self.one(TokenKind::Semicolon),
            b'[' | b']' | b'{' | b'}' => self.one(TokenKind::Punct),
            b'@' | b'#' => {
                self.operator();
                (TokenKind::Operator, false)
            }
            _ if is_operator_byte(b) => {
                self.operator();
                (TokenKind::Operator, false)
            }
            _ => {
                let c = self.char_at(self.pos).unwrap_or('\u{fffd}');
                if c.is_whitespace() {
                    self.whitespace();
                    (TokenKind::Whitespace, false)
                } else if is_word_start(c) {
                    self.word_or_prefixed()
                } else {
                    self.pos += c.len_utf8();
                    (TokenKind::Punct, false)
                }
            }
        }
    }

    fn one(&mut self, kind: TokenKind) -> (TokenKind, bool) {
        self.pos += 1;
        (kind, false)
    }

    fn whitespace(&mut self) {
        while let Some(c) = self.char_at(self.pos) {
            if !c.is_whitespace() {
                break;
            }
            self.pos += c.len_utf8();
        }
    }

    /// Whether the `--` under the cursor starts a comment: in MySQL only when a space or a
    /// control character follows it.
    fn line_comment_dashes(&self) -> bool {
        self.dashes_at(self.pos)
    }

    fn dashes_at(&self, pos: usize) -> bool {
        !self.mysql() || self.bytes.get(pos + 2).is_some_and(|&b| b <= b' ')
    }

    fn skip_line(&mut self) {
        let rest = &self.bytes[self.pos..];
        let len = rest.iter().position(|&b| b == b'\n' || b == b'\r').unwrap_or(rest.len());
        self.pos += len;
    }

    /// Past `/* … */`; whether it was closed. Nested in PostgreSQL.
    fn block_comment(&mut self) -> bool {
        self.pos += 2;
        let nested = self.postgres();
        let mut depth = 1usize;
        while self.pos < self.bytes.len() {
            match (self.bytes[self.pos], self.peek(1)) {
                (b'*', Some(b'/')) => {
                    self.pos += 2;
                    depth -= 1;
                    if depth == 0 {
                        return true;
                    }
                }
                (b'/', Some(b'*')) if nested => {
                    self.pos += 2;
                    depth += 1;
                }
                _ => self.pos += 1,
            }
        }
        false
    }

    /// Past a text quoted by `quote` (the opening one under the cursor), where the quote
    /// doubled stands for itself and, with `backslash`, `\` escapes the next character.
    fn quoted(&mut self, quote: u8, backslash: bool) -> bool {
        self.pos += 1;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if backslash && b == b'\\' {
                self.pos += 2;
                continue;
            }
            self.pos += 1;
            if b == quote {
                if self.peek(0) == Some(quote) {
                    self.pos += 1;
                } else {
                    return true;
                }
            }
        }
        self.pos = self.bytes.len();
        false
    }

    /// Past the next `close` (included); whether there was one.
    fn until(&mut self, close: u8) -> bool {
        let rest = &self.bytes[self.pos..];
        match rest.iter().position(|&b| b == close) {
            Some(i) => {
                self.pos += i + 1;
                true
            }
            None => {
                self.pos = self.bytes.len();
                false
            }
        }
    }

    fn operator(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while let Some(b) = self.peek(0) {
            let continues = is_operator_byte(b) || matches!(b, b'@' | b'#' | b'?');
            // A comment's start ends an operator (`a=--x` is `a =` then a comment).
            let comment = (b == b'-' && self.peek(1) == Some(b'-') && self.dashes_at(self.pos))
                || (b == b'/' && self.peek(1) == Some(b'*'));
            let mysql_comment = b == b'#' && self.mysql();
            if !continues || comment || mysql_comment || (b == b'?' && !self.postgres()) {
                break;
            }
            self.pos += 1;
        }
        debug_assert!(self.pos > start);
    }

    /// The digits of a number after a sign, with its fraction, exponent, hexadecimal or
    /// `_` separators; letters stuck to it (`12abc`) are read with it.
    fn number(&mut self) {
        let b = &self.bytes;
        if b[self.pos] == b'0'
            && matches!(self.peek(1), Some(b'x' | b'X' | b'o' | b'O' | b'b' | b'B'))
        {
            self.pos += 2;
        }
        let mut seen_dot = false;
        let mut seen_exp = false;
        while self.pos < b.len() {
            let c = b[self.pos];
            if c.is_ascii_alphanumeric() || c == b'_' {
                if (c == b'e' || c == b'E')
                    && !seen_exp
                    && matches!(self.peek(1), Some(b'+' | b'-'))
                    && self.peek(2).is_some_and(|d| d.is_ascii_digit())
                {
                    seen_exp = true;
                    self.pos += 2;
                } else if c == b'e' || c == b'E' {
                    seen_exp = true;
                }
                self.pos += 1;
            } else if c == b'.'
                && !seen_dot
                && !seen_exp
                // `1..2` (a range somewhere) keeps its dots apart.
                && self.peek(1) != Some(b'.')
            {
                seen_dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn word(&mut self) {
        while let Some(c) = self.char_at(self.pos) {
            if !is_word_char(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
    }

    /// A word, or the prefixed literal it starts (`E'…'`, `X'…'`, `U&"…"`).
    fn word_or_prefixed(&mut self) -> (TokenKind, bool) {
        let first = self.bytes[self.pos].to_ascii_uppercase();
        let next = self.peek(1);
        let prefix = match (first, next) {
            (b'E', Some(b'\'')) if self.postgres() => Some(StringKind::Escape),
            (b'X', Some(b'\'')) => Some(StringKind::Binary),
            (b'B', Some(b'\'')) if !self.sqlite() => Some(StringKind::Bit),
            (b'N', Some(b'\'')) => Some(StringKind::National),
            _ => None,
        };
        if let Some(kind) = prefix {
            self.pos += 1;
            let closed = self.quoted(b'\'', kind == StringKind::Escape || self.mysql());
            return (TokenKind::String(kind), !closed);
        }
        if first == b'U' && next == Some(b'&') && self.postgres() {
            match self.peek(2) {
                Some(b'\'') => {
                    self.pos += 2;
                    let closed = self.quoted(b'\'', false);
                    return (TokenKind::String(StringKind::Unicode), !closed);
                }
                Some(b'"') => {
                    self.pos += 2;
                    let closed = self.quoted(b'"', false);
                    return (TokenKind::QuotedIdent, !closed);
                }
                _ => {}
            }
        }
        self.word();
        (TokenKind::Word, false)
    }

    /// `$1`, `$name` (SQLite), `$$ … $$` / `$tag$ … $tag$` (PostgreSQL), else punctuation.
    fn dollar(&mut self) -> (TokenKind, bool) {
        let after = self.peek(1);
        if after.is_some_and(|d| d.is_ascii_digit()) && !self.mysql() {
            self.pos += 1;
            while self.peek(0).is_some_and(|d| d.is_ascii_digit()) {
                self.pos += 1;
            }
            return (TokenKind::Parameter(ParamKind::Numbered), false);
        }
        if self.postgres() {
            if let Some(tag_len) = self.dollar_tag() {
                let tag = &self.bytes[self.pos..self.pos + tag_len];
                self.pos += tag_len;
                let rest = &self.text[self.pos..];
                // The tag is ASCII-delimited (`$`), so searching bytes stays on char bounds.
                let tag = std::str::from_utf8(tag).unwrap_or("$$");
                return match rest.find(tag) {
                    Some(i) => {
                        self.pos += i + tag_len;
                        (TokenKind::DollarBody, false)
                    }
                    None => {
                        self.pos = self.bytes.len();
                        (TokenKind::DollarBody, true)
                    }
                };
            }
        } else if self.sqlite() && self.char_at(self.pos + 1).is_some_and(is_word_start) {
            return self.named_or_operator(1);
        }
        self.pos += 1;
        (TokenKind::Punct, false)
    }

    /// The length of a dollar-quote's tag under the cursor (`$$`, `$tag$`), if it is one.
    fn dollar_tag(&self) -> Option<usize> {
        let mut i = self.pos + 1;
        let first = self.char_at(i)?;
        if first == '$' {
            return Some(2);
        }
        if !(first == '_' || first.is_alphabetic()) {
            return None;
        }
        while let Some(c) = self.char_at(i) {
            if c == '$' {
                return Some(i + 1 - self.pos);
            }
            if !(c == '_' || c.is_alphanumeric()) {
                return None;
            }
            i += c.len_utf8();
        }
        None
    }

    /// `?`, `?1`; in PostgreSQL `?|`, `?&`, `?-`, `?#` are the JSON and geometry operators.
    fn question(&mut self) -> (TokenKind, bool) {
        if self.postgres() && matches!(self.peek(1), Some(b'|' | b'&' | b'-' | b'#')) {
            self.operator();
            return (TokenKind::Operator, false);
        }
        self.pos += 1;
        if self.peek(0).is_some_and(|d| d.is_ascii_digit()) {
            while self.peek(0).is_some_and(|d| d.is_ascii_digit()) {
                self.pos += 1;
            }
            return (TokenKind::Parameter(ParamKind::Numbered), false);
        }
        (TokenKind::Parameter(ParamKind::Positional), false)
    }

    /// `::` (a cast), `:=`, `:name`, else a lone `:` (an array slice's).
    fn colon(&mut self) -> (TokenKind, bool) {
        match self.peek(1) {
            Some(b':') => {
                self.pos += 2;
                (TokenKind::Cast, false)
            }
            Some(b'=') => {
                self.pos += 2;
                (TokenKind::Operator, false)
            }
            _ => self.named_or_operator(1),
        }
    }

    /// A named parameter whose sigil is `sigil_len` bytes long, if a name follows it; else
    /// the sigil alone, as an operator.
    fn named_or_operator(&mut self, sigil_len: usize) -> (TokenKind, bool) {
        let name_start = self.pos + sigil_len;
        if self.char_at(name_start).is_some_and(is_word_start) {
            self.pos = name_start;
            self.word();
            return (TokenKind::Parameter(ParamKind::Named), false);
        }
        self.pos += sigil_len;
        (TokenKind::Operator, false)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/sql/lex.rs"]
mod tests;
