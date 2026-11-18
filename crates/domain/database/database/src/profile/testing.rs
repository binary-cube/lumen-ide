//! Profiles built by hand, for tests (the domain's, the drivers', the views'): a node at a
//! time, each under the last one opened ([`ProfileBuilder::child`] / [`ProfileBuilder::up`]).

use crate::capabilities::ProfileCaps;
use crate::profile::node::{NodeId, OpKind, ProfileNode};
use crate::profile::{Engine, Profile, ProfileDepth};

/// Builds a profile: `node` adds under the current parent, `child` adds and goes into it,
/// `up` goes back to its parent.
#[derive(Debug)]
pub struct ProfileBuilder {
    profile: Profile,
    stack: Vec<NodeId>,
}

impl ProfileBuilder {
    pub fn new(driver: &str, depth: ProfileDepth) -> Self {
        let engine =
            Engine { driver: driver.to_owned(), name: driver.to_owned(), ..Engine::default() };
        let mut profile = Profile::new(engine, depth);
        profile.collected = ProfileCaps {
            estimated: true,
            actual: depth.runs(),
            node_timing: depth.runs(),
            ..ProfileCaps::NONE
        };
        Self { profile, stack: Vec::new() }
    }

    /// The profile's id fixed (exports compared with a golden file).
    pub fn id(mut self, id: u128) -> Self {
        self.profile.id = crate::profile::ProfileId::from_u128(id);
        self
    }

    /// The engine's name and version.
    pub fn engine(mut self, name: &str, version: Option<&str>) -> Self {
        self.profile.engine.name = name.to_owned();
        self.profile.engine.version = version.map(str::to_owned);
        self
    }

    /// Adds a node under the current parent (not entering it); `edit` fills it in.
    pub fn node(mut self, op: OpKind, name: &str, edit: impl FnOnce(&mut ProfileNode)) -> Self {
        let mut node = ProfileNode::new(op, name);
        edit(&mut node);
        self.profile.push(self.stack.last().copied(), node);
        self
    }

    /// Adds a node under the current parent and enters it.
    pub fn child(mut self, op: OpKind, name: &str, edit: impl FnOnce(&mut ProfileNode)) -> Self {
        let mut node = ProfileNode::new(op, name);
        edit(&mut node);
        let id = self.profile.push(self.stack.last().copied(), node);
        self.stack.push(id);
        self
    }

    /// Back to the current parent's parent.
    pub fn up(mut self) -> Self {
        self.stack.pop();
        self
    }

    /// The statement's figures, the collected capabilities, the notes.
    pub fn with(mut self, edit: impl FnOnce(&mut Profile)) -> Self {
        edit(&mut self.profile);
        self
    }

    pub fn build(self) -> Profile {
        self.profile
    }
}

/// A node's actual figures, per loop: `(startup, total, rows, loops)`.
pub fn ran(node: &mut ProfileNode, startup_ms: f64, total_ms: f64, rows: f64, loops: f64) {
    node.actuals.startup_ms = Some(startup_ms);
    node.actuals.total_ms = Some(total_ms);
    node.actuals.rows = Some(rows);
    node.actuals.loops = Some(loops);
}

/// A node's estimates: `(startup cost, total cost, rows)`.
pub fn planned(node: &mut ProfileNode, startup: f64, total: f64, rows: f64) {
    node.estimates.startup_cost = Some(startup);
    node.estimates.total_cost = Some(total);
    node.estimates.rows = Some(rows);
}

/// What a rule is checked against, for tests: the statement, the facts, the session's runs.
#[derive(Debug, Clone)]
pub struct Check {
    pub text: String,
    pub flavor: crate::sql::Flavor,
    pub facts: crate::profile::rules::Facts,
    pub recent: crate::profile::rules::RecentRuns,
    pub production: bool,
    pub thresholds: crate::profile::rules::Thresholds,
}

impl Check {
    /// `text` as a PostgreSQL statement, nothing known of its tables.
    pub fn postgres(text: &str) -> Self {
        Self::new(text, crate::sql::Flavor::Postgres { version: Some(170_000) })
    }

    pub fn sqlite(text: &str) -> Self {
        Self::new(text, crate::sql::Flavor::Sqlite)
    }

    pub fn new(text: &str, flavor: crate::sql::Flavor) -> Self {
        Self {
            text: text.to_owned(),
            flavor,
            facts: crate::profile::rules::Facts::default(),
            recent: crate::profile::rules::RecentRuns::default(),
            production: false,
            thresholds: crate::profile::rules::Thresholds::DEFAULT,
        }
    }

    /// A table known (its columns, its indexes by name and columns).
    pub fn table(mut self, facts: crate::profile::rules::TableFacts) -> Self {
        self.facts.tables.push(facts);
        self
    }

    pub fn production(mut self) -> Self {
        self.production = true;
        self
    }

    /// The statement, planned as the console plans it (sent as written).
    pub fn statement(&self) -> crate::console::PlannedStatement {
        let tokens = crate::sql::lex(&self.text, self.flavor);
        crate::console::PlannedStatement {
            text: self.text.clone(),
            range: 0..self.text.len(),
            kind: crate::sql::classify(&self.text, &tokens, self.flavor),
            bound: None,
        }
    }

    /// What rule `id` finds in `profile` (nothing when it doesn't apply).
    pub fn rule(&self, id: &str, profile: Profile) -> Vec<crate::profile::rules::Finding> {
        let report = crate::profile::derive(profile);
        let statement = self.statement();
        let cx = self.context(&report, &statement);
        let rule = crate::profile::rules::rules()
            .iter()
            .find(|r| r.id() == id)
            .unwrap_or_else(|| panic!("no rule {id}"));
        if rule.applies(&report) { rule.check(&cx) } else { Vec::new() }
    }

    /// Every finding of `profile`, as [`crate::profile::rules::analyze`] sorts them.
    pub fn analyze(&self, profile: Profile) -> Vec<crate::profile::rules::Finding> {
        let report = crate::profile::derive(profile);
        let statement = self.statement();
        crate::profile::rules::analyze(&self.context(&report, &statement))
    }

    pub fn context<'a>(
        &'a self,
        report: &'a crate::profile::ProfileReport,
        statement: &'a crate::console::PlannedStatement,
    ) -> crate::profile::rules::RuleContext<'a> {
        crate::profile::rules::RuleContext::new(
            report,
            &self.facts,
            statement,
            self.flavor,
            &self.recent,
            &self.thresholds,
            self.production,
        )
    }
}

/// A table's facts: its columns (all `text` unless written `name:integer`) and its indexes
/// (`(name, [columns])`).
pub fn table(
    name: &str,
    columns: &[&str],
    indexes: &[(&str, &[&str])],
) -> crate::profile::rules::TableFacts {
    let table = match name.split_once('.') {
        Some((schema, name)) => crate::TableRef::in_schema(schema, name),
        None => crate::TableRef::new(name),
    };
    let columns = columns
        .iter()
        .map(|c| {
            let (name, ty) = c.split_once(':').unwrap_or((c, "text"));
            let mut column = crate::ColumnInfo::new(name, ty);
            column.class = crate::TypeClass::of(ty);
            column
        })
        .collect();
    let indexes = indexes
        .iter()
        .map(|(n, cols)| {
            crate::IndexInfo::new(*n, cols.iter().map(|c| (*c).to_owned()).collect(), false)
        })
        .collect();
    crate::profile::rules::TableFacts {
        structure: Some(crate::Structure {
            table: table.clone(),
            columns,
            indexes,
            ..crate::Structure::default()
        }),
        ..crate::profile::rules::TableFacts::new(table)
    }
}

/// A node's table: `name`, aliased `alias`.
pub fn on(node: &mut ProfileNode, name: &str, alias: Option<&str>) {
    node.object = Some(crate::profile::node::ObjectRef {
        kind: crate::profile::node::ObjectKind::Table,
        schema: None,
        name: name.to_owned(),
        alias: alias.map(str::to_owned),
    });
}

/// A node's condition.
pub fn cond(node: &mut ProfileNode, kind: crate::profile::node::CondKind, text: &str) {
    node.conditions.push(crate::profile::node::Condition::new(kind, text));
}
