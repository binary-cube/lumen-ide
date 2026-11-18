//! Relations found from names, where the engine keeps no foreign key: `orders.user_id`
//! (or `userId`, `userid`, `UserID`, `user_fk`, `fk_user`, `id_user`) points to the `id` of
//! `user` (or `users`, `User`, `tbl_user`, `t_user`) when that table exists and the types
//! agree. Pure and deterministic, one pass over the columns (a map of the tables' names),
//! fast for thousands of them.
//!
//! The rules:
//! - **The column** is split into words (snake_case, camelCase, `ID` in capitals) and its
//!   key word taken off: `id` at its end or start, `fk` at its end or start; `userid` (one
//!   word ending in `id`) counts only when a table is named so (`paid`, `valid`, `guid`
//!   never are a table's key: nothing named `pa`, `val`, `gu`). What remains is its stem.
//! - **The table** is named the same, its words compared without case or separators,
//!   singular or plural (`category` / `categories`, `box` / `boxes`, `status` / `statuses`,
//!   `person` / `people` and the other irregular ones), with a usual prefix or not (`tbl_`,
//!   `tb_`, `t_`).
//! - **The key** it points to is the table's `id`, else its primary key of one column; the
//!   types agree (integers with integers, text with text, a type unknown allowed but less
//!   sure), never text with an integer.
//! - **A tree:** `parent_id` (and the like: `ancestor`, `root`, `manager`…) points to its own
//!   table when no table bears that name.
//! - **Never twice:** a column already in a foreign key (the engine's, or one accepted) isn't
//!   looked at; a table's own key never points to itself.
//! - **How sure:** [`Confidence::High`] for `<table>_id` / `<table>Id` and the table named
//!   exactly or in the plural; [`Confidence::Medium`] for the other forms (`fk_`, `_fk`,
//!   `id_`, `userid`, a prefixed table, a tree other than `parent`); one step less when the
//!   key isn't `id` or a type is unknown.

use std::collections::{HashMap, HashSet};

use crate::diagram::SchemaGraph;
use crate::{ColumnInfo, TableKind, TableRef, TypeClass};

/// How sure a relation found from names is (ordered: the surest last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    fn lower(self) -> Self {
        match self {
            Self::High => Self::Medium,
            _ => Self::Low,
        }
    }
}

/// A key not kept by the engine: `columns` of `table` point to `references` of `target` —
/// one accepted (a "virtual" foreign key, kept with the data source) or one set aside.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VirtualKey {
    pub table: TableRef,
    pub columns: Vec<String>,
    pub target: TableRef,
    pub references: Vec<String>,
}

impl VirtualKey {
    /// The same key (names compared without case: as the engines compare them).
    pub fn same(&self, other: &Self) -> bool {
        let names = |a: &[String], b: &[String]| {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
        };
        let tables = |a: &TableRef, b: &TableRef| {
            a.name.eq_ignore_ascii_case(&b.name)
                && a.schema.as_deref().map(str::to_lowercase)
                    == b.schema.as_deref().map(str::to_lowercase)
        };
        tables(&self.table, &other.table)
            && tables(&self.target, &other.target)
            && names(&self.columns, &other.columns)
            && names(&self.references, &other.references)
    }
}

/// Which relations a diagram shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RelationKinds {
    /// The engine's keys and those accepted.
    Real,
    /// Those found from names, and those accepted.
    Inferred,
    #[default]
    Both,
}

impl RelationKinds {
    /// As saved.
    pub fn name(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Inferred => "inferred",
            Self::Both => "both",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "real" => Some(Self::Real),
            "inferred" => Some(Self::Inferred),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

/// What the user decided about a source's relations (kept with it, never a secret): the
/// keys accepted, those set aside, which a diagram shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationNotes {
    pub virtual_keys: Vec<VirtualKey>,
    pub ignored: Vec<VirtualKey>,
    pub show: RelationKinds,
}

impl RelationNotes {
    pub fn is_empty(&self) -> bool {
        self.virtual_keys.is_empty() && self.ignored.is_empty() && self.show == RelationKinds::Both
    }

    /// `key` accepted (no longer set aside).
    pub fn accept(&mut self, key: VirtualKey) {
        self.ignored.retain(|k| !k.same(&key));
        if !self.virtual_keys.iter().any(|k| k.same(&key)) {
            self.virtual_keys.push(key);
        }
    }

    /// `key` set aside (no longer accepted).
    pub fn ignore(&mut self, key: VirtualKey) {
        self.virtual_keys.retain(|k| !k.same(&key));
        if !self.ignored.iter().any(|k| k.same(&key)) {
            self.ignored.push(key);
        }
    }

    /// `key` neither accepted nor set aside (found again from names, if it is).
    pub fn forget(&mut self, key: &VirtualKey) {
        self.virtual_keys.retain(|k| !k.same(key));
        self.ignored.retain(|k| !k.same(key));
    }

    pub fn is_ignored(&self, key: &VirtualKey) -> bool {
        self.ignored.iter().any(|k| k.same(key))
    }

    pub fn is_accepted(&self, key: &VirtualKey) -> bool {
        self.virtual_keys.iter().any(|k| k.same(key))
    }

    /// The key (accepted) column `column` of `table` is in, if any: where it points.
    pub fn key_of(&self, table: &TableRef, column: &str) -> Option<&VirtualKey> {
        self.virtual_keys.iter().find(|k| {
            k.table.name.eq_ignore_ascii_case(&table.name)
                && k.table.schema == table.schema
                && k.columns.iter().any(|c| c.eq_ignore_ascii_case(column))
        })
    }
}

/// A relation found from names: the key it would be, its tables in the graph, how sure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub key: VirtualKey,
    pub from: usize,
    pub to: usize,
    pub confidence: Confidence,
}

/// How a column names what it points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stem {
    /// Its words without the key word, lowercase (`["order", "item"]`).
    pub words: Vec<String>,
    /// `<stem>_id` or `<stem>Id`: the usual way (else `fk_`, `_fk`, `id_`).
    pub usual: bool,
    /// One word ending in `id` (`userid`): only a table so named makes it one.
    pub fused: bool,
}

/// `column` split into words: at `_`, `-`, spaces and digits' edges, and where its case
/// changes (`userId` → user, id; `UserID` → user, id; `HTTPServer` → http, server).
pub fn words(column: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let chars: Vec<char> = column.chars().collect();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            continue;
        }
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1).copied();
        let boundary = match prev {
            Some(p) if p.is_alphanumeric() => {
                (p.is_lowercase() && c.is_uppercase())
                    || (p.is_alphabetic() && c.is_ascii_digit())
                    || (p.is_ascii_digit() && c.is_alphabetic())
                    // The last capital of a run starting a word: `HTTPServer` → HTTP, Server.
                    || (p.is_uppercase() && c.is_uppercase() && next.is_some_and(char::is_lowercase))
            }
            _ => false,
        };
        if boundary && !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        word.extend(c.to_lowercase());
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// What `column` points to by its name (see the module's doc); `None`: no key word.
pub fn stem(column: &str) -> Option<Stem> {
    let mut w = words(column);
    if w.len() >= 2 {
        let (first, last) = (w[0].clone(), w[w.len() - 1].clone());
        let usual = last == "id";
        if usual || last == "fk" {
            w.pop();
            // `fk_user_id`: both.
            if w.len() >= 2 && w[0] == "fk" {
                w.remove(0);
            }
            return Some(Stem { words: w, usual: usual && first != "fk", fused: false });
        }
        if first == "id" || first == "fk" {
            w.remove(0);
            return Some(Stem { words: w, usual: false, fused: false });
        }
        return None;
    }
    let one = w.pop()?;
    let base = one.strip_suffix("id").filter(|b| b.len() >= 2)?;
    Some(Stem { words: vec![base.to_owned()], usual: false, fused: true })
}

/// The irregular plurals, as plural → singular.
const IRREGULAR: &[(&str, &str)] = &[
    ("people", "person"),
    ("men", "man"),
    ("women", "woman"),
    ("children", "child"),
    ("mice", "mouse"),
    ("geese", "goose"),
    ("teeth", "tooth"),
    ("feet", "foot"),
    ("oxen", "ox"),
    ("indices", "index"),
    ("matrices", "matrix"),
    ("vertices", "vertex"),
    ("analyses", "analysis"),
    ("theses", "thesis"),
    ("crises", "crisis"),
    ("criteria", "criterion"),
    ("phenomena", "phenomenon"),
    ("leaves", "leaf"),
    ("lives", "life"),
    ("wives", "wife"),
    ("knives", "knife"),
    ("halves", "half"),
    ("shelves", "shelf"),
    ("quizzes", "quiz"),
    ("movies", "movie"),
    ("cookies", "cookie"),
    ("ties", "tie"),
    ("pies", "pie"),
    ("zombies", "zombie"),
];

/// The words that are the same in the plural.
const SAME: [&str; 6] = ["series", "species", "news", "data", "media", "info"];

/// `word` (lowercase) in the singular, as it most likely is: see [`singulars`].
pub fn singular(word: &str) -> String {
    singulars(word).into_iter().next().unwrap_or_else(|| word.to_owned())
}

/// What `word` (lowercase) may be in the singular, the likeliest first (none: it's already
/// singular, or the same in both): the irregular ones; `-ies` → `-y`; `-es` taken off after
/// `ss`, `us`, `x`, `z`, `ch`, `sh`, `o` (`statuses`, `boxes`), and `-s` alone too
/// (`houses`); `-s` taken off, never after `ss`, `us`, `is` (`address`, `status`,
/// `analysis` are singular).
pub fn singulars(word: &str) -> Vec<String> {
    if let Some((_, one)) = IRREGULAR.iter().find(|(many, _)| *many == word) {
        return vec![(*one).to_owned()];
    }
    if SAME.contains(&word) || IRREGULAR.iter().any(|(_, one)| *one == word) {
        return Vec::new();
    }
    if let Some(base) = word.strip_suffix("ies").filter(|b| b.len() >= 2) {
        return vec![format!("{base}y")];
    }
    let mut out = Vec::new();
    if let Some(base) = word.strip_suffix("es")
        && ["ss", "us", "x", "z", "ch", "sh", "o"].iter().any(|end| base.ends_with(end))
    {
        out.push(base.to_owned());
    }
    if let Some(base) = word.strip_suffix('s')
        && !word.ends_with("ss")
        && !word.ends_with("us")
        && !word.ends_with("is")
        && base.len() >= 2
    {
        out.push(base.to_owned());
    }
    out
}

/// The prefixes a table's name may bear.
const PREFIXES: [&str; 3] = ["tbl", "tb", "t"];

/// The names pointing to their own table (a tree, a chain), when no table bears them.
const SELF: [&str; 12] = [
    "parent",
    "ancestor",
    "root",
    "predecessor",
    "successor",
    "previous",
    "prev",
    "next",
    "superior",
    "manager",
    "supervisor",
    "original",
];

/// How a table was named by a stem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Named {
    /// Through a prefix (`tbl_user`).
    Prefixed,
    /// In the plural (or the singular of a plural stem).
    Plural,
    Exact,
}

/// A name's words joined (lowercase, no separators), and joined with the last one in each
/// of its possible singulars.
fn keys(words: &[String]) -> (String, Vec<String>) {
    let exact = words.concat();
    let Some((last, before)) = words.split_last() else { return (exact, Vec::new()) };
    let start = before.concat();
    let ones = singulars(last).into_iter().map(|one| format!("{start}{one}")).collect();
    (exact, ones)
}

/// The class two columns must share; `None`: unknown (allowed, less sure).
fn family(c: &ColumnInfo) -> Option<TypeClass> {
    match c.class {
        TypeClass::Integer => Some(TypeClass::Integer),
        TypeClass::Text => Some(TypeClass::Text),
        TypeClass::Binary => Some(TypeClass::Binary),
        TypeClass::Decimal => Some(TypeClass::Decimal),
        TypeClass::Other => None,
        other => Some(other),
    }
}

/// Whether `from` can hold `to`'s values: `Some(true)` sure, `Some(false)` never, `None`
/// unknown.
fn compatible(from: &ColumnInfo, to: &ColumnInfo) -> Option<bool> {
    let (a, b) = (family(from)?, family(to)?);
    Some(
        a == b
            // A key kept as a decimal (`NUMERIC(10)`) holds integers.
            || matches!((a, b), (TypeClass::Integer, TypeClass::Decimal) | (TypeClass::Decimal, TypeClass::Integer)),
    )
}

/// The relations `graph`'s columns name (see the module's doc), without those its links
/// already are (the engine's, the accepted): by table, then column.
pub fn infer(graph: &SchemaGraph) -> Vec<Candidate> {
    // Each table by its name's keys: exact, singular, and without a prefix.
    let mut by_name: HashMap<String, Vec<(usize, Named)>> = HashMap::new();
    for (i, t) in graph.tables.iter().enumerate() {
        if t.external || t.kind == TableKind::View {
            continue;
        }
        let w = words(&t.table().name);
        let (exact, ones) = keys(&w);
        by_name.entry(exact).or_default().push((i, Named::Exact));
        for one in ones {
            by_name.entry(one).or_default().push((i, Named::Plural));
        }
        if w.len() >= 2 && PREFIXES.contains(&w[0].as_str()) {
            let (exact, ones) = keys(&w[1..]);
            for key in std::iter::once(exact).chain(ones) {
                by_name.entry(key).or_default().push((i, Named::Prefixed));
            }
        }
    }
    // The columns already in a link (by table, lowercased): found at once for each column.
    let in_links: HashSet<(usize, String)> = graph
        .links
        .iter()
        .flat_map(|l| l.columns.iter().map(move |c| (l.from, c.to_ascii_lowercase())))
        .collect();
    let linked = |t: usize, column: &str| in_links.contains(&(t, column.to_ascii_lowercase()));
    let mut out = Vec::new();
    for (from, table) in graph.tables.iter().enumerate() {
        if table.external || table.kind == TableKind::View {
            continue;
        }
        let own_key = table.structure.primary_key();
        for column in &table.structure.columns {
            if linked(from, &column.name) {
                continue;
            }
            let Some(stem) = stem(&column.name) else { continue };
            if stem.words.is_empty() {
                continue;
            }
            let (exact, ones) = keys(&stem.words);
            // The best table so named: exact before plural before prefixed, then the
            // same schema, then the first. (A plural stem, `users_id`, names `user` too.)
            let mut found: Vec<(usize, Named)> = Vec::new();
            for &(t, how) in by_name.get(&exact).into_iter().flatten() {
                found.push((t, how));
            }
            for one in &ones {
                for &(t, how) in by_name.get(one).into_iter().flatten() {
                    found.push((t, how.min(Named::Plural)));
                }
            }
            let schema = &table.table().schema;
            found.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| {
                        let same = |t: usize| graph.tables[t].table().schema == *schema;
                        same(b.0).cmp(&same(a.0))
                    })
                    .then(a.0.cmp(&b.0))
            });
            let (to, named, tree) = match found.first() {
                Some(&(to, named)) => (to, named, false),
                None if stem.words.len() == 1 && SELF.contains(&stem.words[0].as_str()) => {
                    (from, Named::Exact, true)
                }
                None => continue,
            };
            // A table's own key never points to itself.
            let alone = own_key.len() == 1 && own_key[0].eq_ignore_ascii_case(&column.name);
            if to == from && (alone || !tree) {
                continue;
            }
            let target = &graph.tables[to];
            let key = target
                .structure
                .columns
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case("id"))
                .or_else(|| match target.structure.primary_key().as_slice() {
                    [one] => target.structure.column(one),
                    _ => None,
                });
            let Some(key) = key else { continue };
            if key.name.eq_ignore_ascii_case(&column.name) && to == from {
                continue;
            }
            let fits = compatible(column, key);
            if fits == Some(false) {
                continue;
            }
            // The usual form naming a table exactly or in the plural, or `parent_id`.
            let usual = stem.usual
                && ((named >= Named::Plural && !tree) || (tree && stem.words[0] == "parent"));
            let mut confidence = if usual { Confidence::High } else { Confidence::Medium };
            if !key.name.eq_ignore_ascii_case("id") {
                confidence = confidence.lower();
            }
            if fits.is_none() {
                confidence = confidence.lower();
            }
            out.push(Candidate {
                key: VirtualKey {
                    table: table.table().clone(),
                    columns: vec![column.name.clone()],
                    target: target.table().clone(),
                    references: vec![key.name.clone()],
                },
                from,
                to,
                confidence,
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/relations.rs"]
mod tests;
