//! Emmet abbreviation parser: elements (`tag.class#id[attributes]{text}*N`), the operators
//! `>` (child), `+` (sibling), `^` (climb one level), `( … )` groups and `$` numbering.

/// An element in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    /// Tag name, as in the abbreviation (empty: inferred from the parent).
    pub name: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    /// Attributes in writing order; `None` = attribute without a value (e.g. `required`).
    pub attrs: Vec<(String, Option<String>)>,
    pub text: Option<String>,
    pub children: Vec<Node>,
}

pub fn parse(abbreviation: &str) -> Option<Vec<Node>> {
    let mut p = Parser { chars: abbreviation.chars().collect(), pos: 0 };
    let (nodes, climb) = p.seq()?;
    // Leftover `$` (no multiplier above) become 1.
    let nodes: Vec<Node> = nodes.iter().map(|n| number(n, 1, 1)).collect();
    (p.pos == p.chars.len() && climb == 0 && !nodes.is_empty()).then_some(nodes)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.pos += 1;
        }
        found
    }

    /// Siblings on one level; also returns how many levels to climb (`^`).
    fn seq(&mut self) -> Option<(Vec<Node>, usize)> {
        let mut out = Vec::new();
        loop {
            let (mut template, count, group) = self.item()?;
            if self.eat('>') {
                let (children, climb) = self.seq()?;
                // Children go to the last element (of the group, or the element itself).
                template.last_mut()?.children.extend(children);
                let climb = climb.saturating_sub(1);
                out.extend(replicate(&template, count)?);
                if climb > 0 {
                    return Some((out, climb));
                }
                let _ = group;
                // After `^`, the abbreviation continues with a sibling on this level (no `+`).
                if self.at_item_start() {
                    continue;
                }
            } else {
                out.extend(replicate(&template, count)?);
                template.clear();
            }
            if self.eat('+') {
                continue;
            }
            if self.peek() == Some('^') {
                let mut climb = 0;
                while self.eat('^') {
                    climb += 1;
                }
                return Some((out, climb));
            }
            return Some((out, 0));
        }
    }

    fn at_item_start(&self) -> bool {
        self.peek()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '#' | '[' | '{' | '(' | '!'))
    }

    /// An element or a group, with its multiplier: (template, how many times, is group).
    fn item(&mut self) -> Option<(Vec<Node>, usize, bool)> {
        if self.eat('(') {
            let (nodes, climb) = self.seq()?;
            if climb != 0 || !self.eat(')') {
                return None;
            }
            let count = self.multiplier()?;
            return Some((nodes, count, true));
        }
        let node = self.element()?;
        let count = self.multiplier()?;
        Some((vec![node], count, false))
    }

    fn multiplier(&mut self) -> Option<usize> {
        if !self.eat('*') {
            return Some(1);
        }
        let digits = self.take_while(|c| c.is_ascii_digit());
        let n: usize = digits.parse().ok()?;
        (1..=1000).contains(&n).then_some(n)
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> String {
        let start = self.pos;
        while self.peek().is_some_and(&f) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn element(&mut self) -> Option<Node> {
        let name_char =
            |c: char| c.is_alphanumeric() || matches!(c, ':' | '-' | '_' | '!' | '@' | '$');
        let mut node = Node { name: self.take_while(name_char), ..Node::default() };
        let mut any = !node.name.is_empty();
        loop {
            match self.peek() {
                Some('.') => {
                    self.pos += 1;
                    let class = self.take_while(|c| {
                        c.is_alphanumeric() || matches!(c, '-' | '_' | '$' | ':' | '@')
                    });
                    if class.is_empty() {
                        return None;
                    }
                    node.classes.push(class);
                }
                Some('#') => {
                    self.pos += 1;
                    let id =
                        self.take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '$'));
                    if id.is_empty() {
                        return None;
                    }
                    node.id = Some(id);
                }
                Some('[') => {
                    self.pos += 1;
                    self.attributes(&mut node)?;
                }
                Some('{') => {
                    self.pos += 1;
                    node.text = Some(self.braced()?);
                }
                _ => break,
            }
            any = true;
        }
        any.then_some(node)
    }

    /// `[a=1 b="two words" required]` (after `[`).
    fn attributes(&mut self, node: &mut Node) -> Option<()> {
        loop {
            self.take_while(char::is_whitespace);
            if self.eat(']') {
                return Some(());
            }
            let name = self.take_while(|c| !c.is_whitespace() && !matches!(c, '=' | ']'));
            if name.is_empty() {
                return None;
            }
            let value = if self.eat('=') {
                Some(match self.peek() {
                    Some(q @ ('"' | '\'')) => {
                        self.pos += 1;
                        let v = self.take_while(|c| c != q);
                        if !self.eat(q) {
                            return None;
                        }
                        v
                    }
                    _ => self.take_while(|c| !c.is_whitespace() && c != ']'),
                })
            } else {
                None
            };
            node.attrs.push((name, value));
        }
    }

    /// Text between braces (after `{`), with balanced braces.
    fn braced(&mut self) -> Option<String> {
        let mut depth = 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(out);
                    }
                }
                _ => {}
            }
            out.push(c);
        }
        None
    }
}

/// The most elements an abbreviation expands to (`div*1000>div*1000>div*1000` would be a
/// billion, made on the UI thread at a Tab): past it, it isn't expanded.
pub const MOST_NODES: usize = 20_000;

/// How many elements `nodes` hold, theirs included.
fn size(nodes: &[Node]) -> usize {
    nodes.iter().map(|n| 1usize.saturating_add(size(&n.children))).fold(0, usize::saturating_add)
}

/// Copies of the template, with `$` replaced by the copy number (`$$` = zero-padded);
/// `None` past [`MOST_NODES`].
fn replicate(template: &[Node], count: usize) -> Option<Vec<Node>> {
    if size(template).saturating_mul(count) > MOST_NODES {
        return None;
    }
    // Without multiplication, `$` stays for the multiplier above (`li*3>a{$}`).
    if count == 1 {
        return Some(template.to_vec());
    }
    Some((1..=count).flat_map(|i| template.iter().map(move |n| number(n, i, count))).collect())
}

fn number(node: &Node, i: usize, count: usize) -> Node {
    let n = |s: &str| numbered(s, i, count);
    Node {
        name: n(&node.name),
        id: node.id.as_deref().map(n),
        classes: node.classes.iter().map(|c| n(c)).collect(),
        attrs: node.attrs.iter().map(|(k, v)| (n(k), v.as_deref().map(n))).collect(),
        text: node.text.as_deref().map(n),
        children: node.children.iter().map(|c| number(c, i, count)).collect(),
    }
}

/// `item$` → `item3`; `$$$` → `003`. Without multiplication (a single copy), `$` stays `1`.
fn numbered(s: &str, i: usize, _count: usize) -> String {
    if !s.contains('$') {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let mut width = 1;
        while chars.peek() == Some(&'$') {
            chars.next();
            width += 1;
        }
        out.push_str(&format!("{i:0width$}"));
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/parse.rs"]
mod tests;
