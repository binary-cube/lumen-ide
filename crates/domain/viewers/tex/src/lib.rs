//! TeX math as Markdown documents write it (`$…$`, `$$…$$`, ```` ```math ````): parsed into
//! a tree of atoms — symbols with their class (ordinary, operator, binary, relation,
//! opening, closing, punctuation) and font, scripts, fractions, roots, big operators,
//! `\left … \right`, matrices and `cases` / `aligned`, accents, text, spaces. What isn't
//! known is kept as an [`Node::Error`] (shown in red) rather than failing the formula;
//! only unbalanced braces and environments fail it.

mod symbols;

pub use symbols::symbol;

/// How an atom spaces itself from its neighbors (TeX's classes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Ord,
    /// A big operator or a function's name (`\sum`, `\sin`).
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
}

/// The alphabet a letter is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Font {
    /// Letters in italics (the default for variables).
    #[default]
    Italic,
    Roman,
    Bold,
    BoldItalic,
    DoubleStruck,
    Script,
    Fraktur,
    SansSerif,
    Monospace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Accent {
    Hat,
    WideHat,
    Bar,
    Overline,
    Underline,
    Vec,
    Dot,
    Ddot,
    Tilde,
    WideTilde,
    Acute,
    Grave,
    Breve,
    Check,
    OverBrace,
    UnderBrace,
}

/// An environment's kind: its delimiters and how its columns align.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Env {
    Matrix,
    PMatrix,
    BMatrix,
    BraceMatrix,
    VMatrix,
    DoubleVMatrix,
    /// A brace on the left, the columns left-aligned.
    Cases,
    /// Columns right / left in turn (`aligned`, `align`, `split`, `gather`).
    Aligned,
    /// `array` (its columns as `{lcr}` says).
    Array,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Symbol {
        text: String,
        class: Class,
        font: Font,
    },
    Group(Vec<Node>),
    Scripts {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
    },
    /// `\frac`, `\binom` (no bar, in parentheses), `\dfrac` / `\tfrac` (`display`).
    Frac {
        num: Box<Node>,
        den: Box<Node>,
        bar: bool,
        parens: bool,
        display: Option<bool>,
    },
    Sqrt {
        index: Option<Box<Node>>,
        body: Box<Node>,
    },
    /// A big operator (`∑`, `∫`) or a function name with limits (`lim`): whether its
    /// scripts go above and below in display style.
    BigOp {
        text: String,
        limits: bool,
        symbol: bool,
    },
    Delimited {
        left: String,
        body: Vec<Node>,
        right: String,
    },
    Matrix {
        env: Env,
        rows: Vec<Vec<Vec<Node>>>,
        align: Vec<char>,
    },
    Accent {
        accent: Accent,
        body: Box<Node>,
    },
    Text {
        text: String,
        font: Font,
    },
    /// Space, in ems.
    Space(f32),
    /// `\color{red}{…}` / `\textcolor{red}{…}`.
    Colored {
        color: String,
        body: Box<Node>,
    },
    /// What wasn't understood (a command's name).
    Error(String),
}

/// Why a formula couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Where (a byte offset).
    pub at: usize,
    pub kind: ParseErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// A `{` never closed.
    Unclosed,
    /// A `}` with nothing open.
    Unopened,
    /// `\begin{x}` without its `\end{x}` (or the other way).
    Environment(String),
    /// `\left` without `\right`.
    Right,
}

/// Reads a formula.
pub fn parse(source: &str) -> Result<Vec<Node>, ParseError> {
    let mut p = Parser { src: source, pos: 0 };
    let nodes = p.expr(Stop::End)?;
    Ok(nodes)
}

/// What ends a run of atoms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    End,
    Brace,
    Bracket,
    Right,
    /// Inside an environment: `&`, `\\` and `\end` end a cell.
    Cell,
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

enum Token<'a> {
    Command(&'a str),
    Char(char),
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn skip_space(&mut self) {
        let rest = self.rest();
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn peek(&mut self) -> Option<Token<'a>> {
        self.skip_space();
        let rest = self.rest();
        let mut chars = rest.chars();
        let c = chars.next()?;
        if c == '\\' {
            let after = &rest[1..];
            let len = after.chars().take_while(char::is_ascii_alphabetic).count();
            if len > 0 {
                return Some(Token::Command(&after[..len]));
            }
            let next = after.chars().next()?;
            return Some(Token::Command(&after[..next.len_utf8()]));
        }
        Some(Token::Char(c))
    }

    fn bump(&mut self, token: &Token<'_>) {
        self.pos += match token {
            Token::Command(name) => 1 + name.len(),
            Token::Char(c) => c.len_utf8(),
        };
    }

    fn expr(&mut self, stop: Stop) -> Result<Vec<Node>, ParseError> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            let Some(token) = self.peek() else {
                return match stop {
                    Stop::End | Stop::Cell => Ok(out),
                    Stop::Right => Err(ParseError { at: self.pos, kind: ParseErrorKind::Right }),
                    _ => Err(ParseError { at: self.pos, kind: ParseErrorKind::Unclosed }),
                };
            };
            match (&token, stop) {
                (Token::Char('}'), Stop::Brace) => {
                    self.bump(&token);
                    return Ok(out);
                }
                (Token::Char('}'), _) => {
                    return Err(ParseError { at: self.pos, kind: ParseErrorKind::Unopened });
                }
                (Token::Char(']'), Stop::Bracket) => {
                    self.bump(&token);
                    return Ok(out);
                }
                (Token::Command("right"), Stop::Right) => return Ok(out),
                (Token::Char('&') | Token::Command("\\" | "end" | "cr"), Stop::Cell) => {
                    return Ok(out);
                }
                (Token::Char('^' | '_'), _) => {
                    self.bump(&token);
                    let sup = matches!(token, Token::Char('^'));
                    let script = self.argument()?;
                    let base = out.pop().unwrap_or(Node::Group(Vec::new()));
                    out.push(attach(base, script, sup));
                    continue;
                }
                (Token::Char('\''), _) => {
                    self.bump(&token);
                    let base = out.pop().unwrap_or(Node::Group(Vec::new()));
                    let prime =
                        Node::Symbol { text: "′".into(), class: Class::Ord, font: Font::Roman };
                    out.push(attach(base, prime, true));
                    continue;
                }
                _ => {}
            }
            let node = self.atom(token)?;
            out.extend(node);
        }
    }

    /// A script's or a command's argument: a group, a command, or one character.
    fn argument(&mut self) -> Result<Node, ParseError> {
        let Some(token) = self.peek() else { return Ok(Node::Group(Vec::new())) };
        if let Token::Char('{') = token {
            self.bump(&token);
            return Ok(Node::Group(self.expr(Stop::Brace)?));
        }
        let mut nodes = self.atom(token)?;
        Ok(if nodes.len() == 1 { nodes.remove(0) } else { Node::Group(nodes) })
    }

    /// A `{…}` argument's raw text (`\text{…}`, `\begin{…}`).
    fn raw_argument(&mut self) -> Result<String, ParseError> {
        self.skip_space();
        if !self.rest().starts_with('{') {
            // One character (`\text a`).
            let c = self.rest().chars().next().map(String::from).unwrap_or_default();
            self.pos += c.len();
            return Ok(c);
        }
        let start = self.pos + 1;
        let mut depth = 0;
        for (i, c) in self.rest().char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let text = self.src[start..self.pos + i].to_owned();
                        self.pos += i + 1;
                        return Ok(text);
                    }
                }
                _ => {}
            }
        }
        Err(ParseError { at: self.pos, kind: ParseErrorKind::Unclosed })
    }

    fn optional(&mut self) -> Result<Option<Node>, ParseError> {
        self.skip_space();
        if self.rest().starts_with('[') {
            self.pos += 1;
            return Ok(Some(Node::Group(self.expr(Stop::Bracket)?)));
        }
        Ok(None)
    }

    /// A delimiter after `\left` / `\right` (`(`, `\{`, `\langle`, `.`).
    fn delimiter(&mut self) -> String {
        match self.peek() {
            Some(token @ Token::Char(c)) => {
                self.bump(&token);
                if c == '.' { String::new() } else { c.to_string() }
            }
            Some(token @ Token::Command(name)) => {
                self.bump(&token);
                match name {
                    "{" | "lbrace" => "{".into(),
                    "}" | "rbrace" => "}".into(),
                    "|" | "Vert" => "‖".into(),
                    "vert" => "|".into(),
                    "langle" => "⟨".into(),
                    "rangle" => "⟩".into(),
                    "lfloor" => "⌊".into(),
                    "rfloor" => "⌋".into(),
                    "lceil" => "⌈".into(),
                    "rceil" => "⌉".into(),
                    "lbrack" => "[".into(),
                    "rbrack" => "]".into(),
                    other => other.into(),
                }
            }
            None => String::new(),
        }
    }

    fn atom(&mut self, token: Token<'a>) -> Result<Vec<Node>, ParseError> {
        self.bump(&token);
        let name = match token {
            Token::Char('{') => return Ok(vec![Node::Group(self.expr(Stop::Brace)?)]),
            Token::Char('~') => return Ok(vec![Node::Space(0.33)]),
            Token::Char('&') => return Ok(Vec::new()),
            Token::Char(c) => return Ok(vec![char_node(c)]),
            Token::Command(name) => name,
        };
        let one = |n: Node| Ok(vec![n]);
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" | "binom" | "dbinom" | "tbinom" => {
                let num = self.argument()?;
                let den = self.argument()?;
                let binom = name.ends_with("binom");
                let display = match name {
                    "dfrac" | "cfrac" | "dbinom" => Some(true),
                    "tfrac" | "tbinom" => Some(false),
                    _ => None,
                };
                one(Node::Frac {
                    num: Box::new(num),
                    den: Box::new(den),
                    bar: !binom,
                    parens: binom,
                    display,
                })
            }
            "sqrt" => {
                let index = self.optional()?.map(Box::new);
                let body = Box::new(self.argument()?);
                one(Node::Sqrt { index, body })
            }
            "left" => {
                let left = self.delimiter();
                let body = self.expr(Stop::Right)?;
                // `\right` itself.
                if let Some(t @ Token::Command("right")) = self.peek() {
                    self.bump(&t);
                }
                let right = self.delimiter();
                one(Node::Delimited { left, body, right })
            }
            "right" => Err(ParseError { at: self.pos, kind: ParseErrorKind::Right }),
            "begin" => self.environment(),
            "end" => {
                let env = self.raw_argument()?;
                Err(ParseError { at: self.pos, kind: ParseErrorKind::Environment(env) })
            }
            "text" | "textrm" | "textnormal" | "mbox" | "hbox" | "textit" | "textbf" | "texttt"
            | "textsf" => {
                let text = self.raw_argument()?;
                let font = match name {
                    "textit" => Font::Italic,
                    "textbf" => Font::Bold,
                    "texttt" => Font::Monospace,
                    "textsf" => Font::SansSerif,
                    _ => Font::Roman,
                };
                one(Node::Text { text, font })
            }
            "operatorname" | "operatorname*" => {
                let text = self.raw_argument()?;
                one(Node::BigOp { text, limits: name.ends_with('*'), symbol: false })
            }
            "mathbb" | "mathcal" | "mathscr" | "mathfrak" | "mathbf" | "mathrm" | "mathit"
            | "mathsf" | "mathtt" | "boldsymbol" | "bm" | "mathbold" => {
                let font = match name {
                    "mathbb" => Font::DoubleStruck,
                    "mathcal" | "mathscr" => Font::Script,
                    "mathfrak" => Font::Fraktur,
                    "mathbf" | "mathbold" => Font::Bold,
                    "boldsymbol" | "bm" => Font::BoldItalic,
                    "mathrm" => Font::Roman,
                    "mathsf" => Font::SansSerif,
                    "mathtt" => Font::Monospace,
                    _ => Font::Italic,
                };
                let body = match self.argument()? {
                    Node::Group(mut nodes) if nodes.len() == 1 => nodes.remove(0),
                    body => body,
                };
                one(with_font(body, font))
            }
            "color" | "textcolor" => {
                let color = self.raw_argument()?;
                let body = if name == "textcolor" {
                    self.argument()?
                } else {
                    Node::Group(self.expr_until_group_end()?)
                };
                one(Node::Colored { color, body: Box::new(body) })
            }
            "hat" | "widehat" | "bar" | "overline" | "underline" | "vec" | "overrightarrow"
            | "dot" | "ddot" | "tilde" | "widetilde" | "acute" | "grave" | "breve" | "check"
            | "overbrace" | "underbrace" => {
                let accent = match name {
                    "hat" => Accent::Hat,
                    "widehat" => Accent::WideHat,
                    "bar" => Accent::Bar,
                    "overline" => Accent::Overline,
                    "underline" => Accent::Underline,
                    "vec" | "overrightarrow" => Accent::Vec,
                    "dot" => Accent::Dot,
                    "ddot" => Accent::Ddot,
                    "tilde" => Accent::Tilde,
                    "widetilde" => Accent::WideTilde,
                    "acute" => Accent::Acute,
                    "grave" => Accent::Grave,
                    "breve" => Accent::Breve,
                    "check" => Accent::Check,
                    "overbrace" => Accent::OverBrace,
                    _ => Accent::UnderBrace,
                };
                let body = Box::new(self.argument()?);
                one(Node::Accent { accent, body })
            }
            "," | "thinspace" => one(Node::Space(0.17)),
            ":" | ">" | "medspace" => one(Node::Space(0.22)),
            ";" | "thickspace" => one(Node::Space(0.28)),
            "!" | "negthinspace" => one(Node::Space(-0.17)),
            " " => one(Node::Space(0.33)),
            "quad" => one(Node::Space(1.0)),
            "qquad" => one(Node::Space(2.0)),
            "displaystyle" | "textstyle" | "scriptstyle" | "limits" | "nolimits" | "nonumber"
            | "notag" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr"
            | "biggl" | "biggr" => Ok(Vec::new()),
            other => match symbol(other) {
                Some(node) => one(node),
                None => one(Node::Error(format!("\\{other}"))),
            },
        }
    }

    /// The rest of the current group (`\color{red} x y` colors to the group's end).
    fn expr_until_group_end(&mut self) -> Result<Vec<Node>, ParseError> {
        let mut out = Vec::new();
        while let Some(token) = self.peek() {
            if matches!(
                token,
                Token::Char('}') | Token::Char('&') | Token::Command("\\" | "end" | "right")
            ) {
                break;
            }
            if matches!(token, Token::Char('^' | '_')) {
                self.bump(&token);
                let sup = matches!(token, Token::Char('^'));
                let script = self.argument()?;
                let base = out.pop().unwrap_or(Node::Group(Vec::new()));
                out.push(attach(base, script, sup));
                continue;
            }
            out.extend(self.atom(token)?);
        }
        Ok(out)
    }

    /// `\begin{name}` … `\end{name}`: its rows of cells.
    fn environment(&mut self) -> Result<Vec<Node>, ParseError> {
        let start = self.pos;
        let name = self.raw_argument()?;
        let env = match name.trim_end_matches('*') {
            "matrix" | "smallmatrix" => Env::Matrix,
            "pmatrix" => Env::PMatrix,
            "bmatrix" => Env::BMatrix,
            "Bmatrix" => Env::BraceMatrix,
            "vmatrix" => Env::VMatrix,
            "Vmatrix" => Env::DoubleVMatrix,
            "cases" | "dcases" => Env::Cases,
            "aligned" | "align" | "alignat" | "split" | "gather" | "gathered" | "eqnarray"
            | "equation" => Env::Aligned,
            "array" => Env::Array,
            _ => return Err(ParseError { at: start, kind: ParseErrorKind::Environment(name) }),
        };
        let align: Vec<char> = if env == Env::Array {
            self.raw_argument()?.chars().filter(|c| matches!(c, 'l' | 'c' | 'r')).collect()
        } else {
            Vec::new()
        };
        let mut rows: Vec<Vec<Vec<Node>>> = vec![Vec::new()];
        loop {
            let cell = self.expr(Stop::Cell)?;
            if let Some(r) = rows.last_mut() {
                r.push(cell);
            }
            match self.peek() {
                Some(t @ Token::Char('&')) => self.bump(&t),
                Some(t @ Token::Command("\\" | "cr")) => {
                    self.bump(&t);
                    // `\\[2pt]`: the spacing left out.
                    self.skip_space();
                    if self.rest().starts_with('[')
                        && let Some(end) = self.rest().find(']')
                    {
                        self.pos += end + 1;
                    }
                    rows.push(Vec::new());
                }
                Some(t @ Token::Command("end")) => {
                    self.bump(&t);
                    let closing = self.raw_argument()?;
                    if closing != name {
                        return Err(ParseError {
                            at: self.pos,
                            kind: ParseErrorKind::Environment(name),
                        });
                    }
                    break;
                }
                _ => return Err(ParseError { at: start, kind: ParseErrorKind::Environment(name) }),
            }
        }
        // A last empty row (after a final `\\`) left out.
        if rows.last().is_some_and(|r| r.iter().all(Vec::is_empty)) && rows.len() > 1 {
            rows.pop();
        }
        Ok(vec![Node::Matrix { env, rows, align }])
    }
}

/// A script attached to `base` (a second one of the same kind stacks on a group).
fn attach(base: Node, script: Node, sup: bool) -> Node {
    match base {
        Node::Scripts { base, sub, sup: s } if (sup && s.is_none()) || (!sup && sub.is_none()) => {
            if sup {
                Node::Scripts { base, sub, sup: Some(Box::new(script)) }
            } else {
                Node::Scripts { base, sub: Some(Box::new(script)), sup: s }
            }
        }
        base => {
            let (sub, sup_node) =
                if sup { (None, Some(Box::new(script))) } else { (Some(Box::new(script)), None) };
            Node::Scripts { base: Box::new(base), sub, sup: sup_node }
        }
    }
}

/// A character as an atom.
fn char_node(c: char) -> Node {
    let (text, class, font) = match c {
        'a'..='z' | 'A'..='Z' => (c.to_string(), Class::Ord, Font::Italic),
        '0'..='9' | '.' => (c.to_string(), Class::Ord, Font::Roman),
        '+' => ("+".into(), Class::Bin, Font::Roman),
        '-' => ("−".into(), Class::Bin, Font::Roman),
        '*' => ("∗".into(), Class::Bin, Font::Roman),
        '/' => ("/".into(), Class::Ord, Font::Roman),
        '=' | '<' | '>' => (c.to_string(), Class::Rel, Font::Roman),
        ':' => (":".into(), Class::Rel, Font::Roman),
        '(' | '[' => (c.to_string(), Class::Open, Font::Roman),
        ')' | ']' => (c.to_string(), Class::Close, Font::Roman),
        ',' | ';' => (c.to_string(), Class::Punct, Font::Roman),
        '|' => ("|".into(), Class::Ord, Font::Roman),
        c if c.is_alphabetic() => (c.to_string(), Class::Ord, Font::Italic),
        c => (c.to_string(), Class::Ord, Font::Roman),
    };
    Node::Symbol { text, class, font }
}

/// `node` (and what it holds) in `font`.
fn with_font(node: Node, font: Font) -> Node {
    match node {
        Node::Symbol { text, class, font: f } => {
            // Digits and operators keep their upright shape, except in bold.
            let keep = f == Font::Roman
                && !matches!(
                    font,
                    Font::Bold
                        | Font::BoldItalic
                        | Font::DoubleStruck
                        | Font::Monospace
                        | Font::SansSerif
                )
                && !text.chars().all(char::is_alphabetic);
            Node::Symbol { text, class, font: if keep { f } else { font } }
        }
        Node::Group(nodes) => Node::Group(nodes.into_iter().map(|n| with_font(n, font)).collect()),
        Node::Scripts { base, sub, sup } => Node::Scripts {
            base: Box::new(with_font(*base, font)),
            sub: sub.map(|s| Box::new(with_font(*s, font))),
            sup: sup.map(|s| Box::new(with_font(*s, font))),
        },
        other => other,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
