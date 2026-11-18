//! The editor's split tree: each split divides only that group (to the right or below), so
//! they can be combined freely (e.g. left | (top / bottom)). Branches share their space by
//! ratios, changed by dragging the separators.

use kernel::{Point, Rect};

/// The direction of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// Groups side by side (Split Right).
    Right,
    /// Groups one below the other (Split Down).
    Down,
}

/// How far the smallest group can shrink, as a ratio of the branch.
const MIN_RATIO: f32 = 0.1;

#[derive(Debug, Clone, PartialEq)]
enum Node {
    /// An editor group (its index).
    Leaf(usize),
    Branch {
        dir: Split,
        children: Vec<(Node, f32)>,
    },
}

/// A separator between two children of a branch.
#[derive(Debug, Clone, PartialEq)]
pub struct Divider {
    /// The drawn line.
    pub rect: Rect,
    pub dir: Split,
    /// The path to the branch (child indices from the root).
    path: Vec<usize>,
    /// Separates child `index` from `index + 1`.
    index: usize,
    /// The branch's start and length along the split axis.
    span: (f32, f32),
    /// The separators' thickness (the children share what's left).
    line: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SplitTree {
    root: Node,
}

impl Default for SplitTree {
    fn default() -> Self {
        Self { root: Node::Leaf(0) }
    }
}

fn renumber(node: &mut Node, f: &dyn Fn(usize) -> usize) {
    match node {
        Node::Leaf(g) => *g = f(*g),
        Node::Branch { children, .. } => children.iter_mut().for_each(|(c, _)| renumber(c, f)),
    }
}

/// Inserts `new` next to leaf `target` (after it, or `before` it), in direction `dir`,
/// with `share` of its space. `true` if found.
fn insert(
    node: &mut Node,
    target: usize,
    new: usize,
    dir: Split,
    share: f32,
    before: bool,
) -> bool {
    match node {
        Node::Leaf(g) if *g == target => {
            let (old, new) = ((Node::Leaf(target), 1.0 - share), (Node::Leaf(new), share));
            let children = if before { vec![new, old] } else { vec![old, new] };
            *node = Node::Branch { dir, children };
            true
        }
        Node::Leaf(_) => false,
        Node::Branch { dir: branch_dir, children } => {
            // The leaf is a direct child of a branch in the same direction: a new sibling, with
            // `share` of its space.
            if *branch_dir == dir
                && let Some(i) = children.iter().position(|(c, _)| *c == Node::Leaf(target))
            {
                let taken = children[i].1 * share;
                children[i].1 -= taken;
                children.insert(if before { i } else { i + 1 }, (Node::Leaf(new), taken));
                return true;
            }
            children.iter_mut().any(|(c, _)| insert(c, target, new, dir, share, before))
        }
    }
}

/// Removes leaf `g`; branches with a single child are replaced by it. `true` if found.
fn remove(node: &mut Node, g: usize) -> bool {
    let Node::Branch { children, .. } = node else { return false };
    if let Some(i) = children.iter().position(|(c, _)| *c == Node::Leaf(g)) {
        let (_, freed) = children.remove(i);
        // The freed space goes to a neighbor (the previous one, or the first).
        let neighbour = i.saturating_sub(1).min(children.len().saturating_sub(1));
        if let Some(n) = children.get_mut(neighbour) {
            n.1 += freed;
        }
    } else if !children.iter_mut().any(|(c, _)| remove(c, g)) {
        return false;
    }
    if children.len() == 1 {
        let (only, _) = children.remove(0);
        *node = only;
    }
    true
}

fn layout(
    node: &Node,
    rect: Rect,
    line: f32,
    path: &mut Vec<usize>,
    groups: &mut Vec<(usize, Rect)>,
    dividers: &mut Vec<Divider>,
) {
    match node {
        Node::Leaf(g) => groups.push((*g, rect)),
        Node::Branch { dir, children } => {
            let n = children.len();
            let (start, total) = match dir {
                Split::Right => (rect.min_x(), rect.size.width),
                Split::Down => (rect.min_y(), rect.size.height),
            };
            let available = (total - line * (n - 1) as f32).max(0.0);
            let mut at = start;
            for (i, (child, ratio)) in children.iter().enumerate() {
                let size =
                    if i + 1 == n { start + total - at } else { (available * ratio).floor() };
                let r = match dir {
                    Split::Right => Rect::new(at, rect.min_y(), size, rect.size.height),
                    Split::Down => Rect::new(rect.min_x(), at, rect.size.width, size),
                };
                path.push(i);
                layout(child, r, line, path, groups, dividers);
                path.pop();
                at += size;
                if i + 1 < n {
                    let d = match dir {
                        Split::Right => Rect::new(at, rect.min_y(), line, rect.size.height),
                        Split::Down => Rect::new(rect.min_x(), at, rect.size.width, line),
                    };
                    dividers.push(Divider {
                        rect: d,
                        dir: *dir,
                        path: path.clone(),
                        index: i,
                        span: (start, total),
                        line,
                    });
                    at += line;
                }
            }
        }
    }
}

impl SplitTree {
    /// Splits group `target`: the new group gets index `target + 1` (the ones after it are
    /// renumbered), as in the group list.
    pub fn split(&mut self, target: usize, dir: Split) {
        self.split_sharing(target, dir, 0.5);
    }

    /// Splits group `target` as [`split`](Self::split), the new group taking `share` of its
    /// space.
    pub fn split_sharing(&mut self, target: usize, dir: Split, share: f32) {
        let new = target + 1;
        renumber(&mut self.root, &|g| if g >= new { g + 1 } else { g });
        insert(&mut self.root, target, new, dir, share.clamp(MIN_RATIO, 1.0 - MIN_RATIO), false);
    }

    /// Splits group `target` with the new group before it (to its left, or above): the new
    /// one gets index `target`, `target` and the ones after it move one up.
    pub fn split_before(&mut self, target: usize, dir: Split) {
        renumber(&mut self.root, &|g| if g >= target { g + 1 } else { g });
        insert(&mut self.root, target + 1, target, dir, 0.5, true);
    }

    /// Removes group `g` (the ones after it are renumbered).
    pub fn remove(&mut self, g: usize) {
        if self.root == Node::Leaf(g) {
            return;
        }
        remove(&mut self.root, g);
        renumber(&mut self.root, &|i| if i > g { i - 1 } else { i });
    }

    /// Each group's area and the separators, in `rect`, with lines of thickness `line`.
    pub fn layout(&self, rect: Rect, line: f32) -> (Vec<(usize, Rect)>, Vec<Divider>) {
        let (mut groups, mut dividers) = (Vec::new(), Vec::new());
        layout(&self.root, rect, line, &mut Vec::new(), &mut groups, &mut dividers);
        (groups, dividers)
    }

    /// Moves the separator to point `p` (the ratios of the two neighbors).
    pub fn drag(&mut self, divider: &Divider, p: Point) {
        let mut node = &mut self.root;
        for &i in &divider.path {
            let Node::Branch { children, .. } = node else { return };
            node = &mut children[i].0;
        }
        let Node::Branch { children, .. } = node else { return };
        let i = divider.index;
        if i + 1 >= children.len() {
            return;
        }
        let (start, total) = divider.span;
        let along = match divider.dir {
            Split::Right => p.x,
            Split::Down => p.y,
        };
        // As laid out: the children share the length minus the separators, and `p` is this
        // separator's center (after `i` separators and half of its own).
        let children_count = children.len();
        let available = (total - divider.line * (children_count - 1) as f32).max(1.0);
        let offset = divider.line * i as f32 + divider.line / 2.0;
        let position = ((along - start - offset) / available).clamp(0.0, 1.0);
        let before: f32 = children[..i].iter().map(|(_, r)| r).sum();
        let pair = children[i].1 + children[i + 1].1;
        let first =
            (position - before).clamp(MIN_RATIO.min(pair / 2.0), pair - MIN_RATIO.min(pair / 2.0));
        children[i].1 = first;
        children[i + 1].1 = pair - first;
    }
}

impl SplitTree {
    /// The branch separated by `divider`: its children share its space equally.
    pub fn equalize(&mut self, divider: &Divider) {
        let mut node = &mut self.root;
        for &i in &divider.path {
            let Node::Branch { children, .. } = node else { return };
            node = &mut children[i].0;
        }
        let Node::Branch { children, .. } = node else { return };
        let share = 1.0 / children.len() as f32;
        for (_, ratio) in children.iter_mut() {
            *ratio = share;
        }
    }
}

/// Text form of a node: `L0` (group 0), `R(0.5 L0,0.5 D(…))` (branches).
fn write(node: &Node, out: &mut String) {
    match node {
        Node::Leaf(g) => out.push_str(&format!("L{g}")),
        Node::Branch { dir, children } => {
            out.push(if *dir == Split::Right { 'R' } else { 'D' });
            out.push('(');
            for (i, (child, ratio)) in children.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!("{ratio:.4} "));
                write(child, out);
            }
            out.push(')');
        }
    }
}

fn read(s: &str) -> Option<(Node, &str)> {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('L') {
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        return Some((Node::Leaf(rest[..digits].parse().ok()?), &rest[digits..]));
    }
    let dir = match s.chars().next()? {
        'R' => Split::Right,
        'D' => Split::Down,
        _ => return None,
    };
    let mut rest = s[1..].strip_prefix('(')?;
    let mut children = Vec::new();
    loop {
        let rest_trim = rest.trim_start();
        let space = rest_trim.find(' ')?;
        let ratio: f32 = rest_trim[..space].parse().ok()?;
        let (child, after) = read(&rest_trim[space + 1..])?;
        children.push((child, ratio));
        let after = after.trim_start();
        if let Some(next) = after.strip_prefix(',') {
            rest = next;
        } else {
            rest = after.strip_prefix(')')?;
            break;
        }
    }
    (children.len() >= 2).then_some((Node::Branch { dir, children }, rest))
}

fn leaves(node: &Node, out: &mut Vec<usize>) {
    match node {
        Node::Leaf(g) => out.push(*g),
        Node::Branch { children, .. } => children.iter().for_each(|(c, _)| leaves(c, out)),
    }
}

impl SplitTree {
    /// The text form (project session).
    pub fn to_spec(&self) -> String {
        let mut out = String::new();
        write(&self.root, &mut out);
        out
    }

    /// The tree from its text form, if it has exactly the groups `0..groups`.
    pub fn from_spec(spec: &str, groups: usize) -> Option<Self> {
        let (root, rest) = read(spec)?;
        let mut found = Vec::new();
        leaves(&root, &mut found);
        found.sort_unstable();
        (rest.trim().is_empty() && found == (0..groups).collect::<Vec<_>>())
            .then_some(Self { root })
    }
}

#[cfg(test)]
#[path = "../tests/unit/split_tree.rs"]
mod tests;
