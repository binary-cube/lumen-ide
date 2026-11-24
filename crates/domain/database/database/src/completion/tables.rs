//! The schemas, tables and views offered, the tables related to those in scope after
//! `JOIN`, and the whole `ON` conditions made from their keys.

use crate::TableKind;
use crate::schema_index::{IndexRelation, RelationOrigin};
use crate::sql::scope::ScopeTable;

use super::{Completer, Suggestion, SuggestionKind, alias_for, relation_detail};

impl<'a> Completer<'a> {
    pub(super) fn schemas(&mut self, group: u8) {
        let Some(index) = self.index else { return };
        for s in index.schemas() {
            let item = Suggestion::new(s.clone(), SuggestionKind::Schema).insert(self.name(s));
            self.push(group, item);
        }
    }

    pub(super) fn tables(&mut self, schema: Option<&str>, join: bool) {
        if schema.is_none() {
            // The statement's CTEs first.
            for cte in self.scope.ctes.clone() {
                let item = Suggestion::new(cte.name.clone(), SuggestionKind::Cte)
                    .insert(self.name(&cte.name));
                self.push(if join { 3 } else { 0 }, item);
            }
            if join {
                self.related();
            }
        }
        let Some(index) = self.index else { return };
        let limit = self.req.limit;
        let base = if join { 4 } else { 1 };
        for i in index.candidates(&self.query, limit * 4) {
            let t = index.table(i);
            let current = index.in_schema(i, self.req.schema);
            if let Some(schema) = schema
                && !index.schema_of(i).is_some_and(|s| s.eq_ignore_ascii_case(schema))
            {
                continue;
            }
            let explicit = schema.is_some();
            let label = if current || explicit || t.table.schema.is_none() {
                t.table.name.clone()
            } else {
                format!("{}.{}", t.table.schema.as_deref().unwrap_or_default(), t.table.name)
            };
            let insert = if explicit { self.name(&t.table.name) } else { self.table_text(i) };
            let kind = if t.kind == TableKind::View {
                SuggestionKind::View
            } else {
                SuggestionKind::Table
            };
            let mut item = Suggestion::new(label, kind).insert(insert);
            item.detail = index.schema_of(i).map(str::to_owned);
            item.current = current;
            let group = if current || explicit { base } else { base + 2 };
            let key = t.table.name.clone();
            self.push_by(group, &key, item);
        }
        if schema.is_none() {
            // The other schemas (to qualify a table with), after the current one's tables.
            for s in index.schemas().to_vec() {
                if self
                    .req
                    .schema
                    .or(index.default_schema())
                    .is_some_and(|c| c.eq_ignore_ascii_case(&s))
                {
                    continue;
                }
                let item = Suggestion::new(s.clone(), SuggestionKind::Schema)
                    .insert(format!("{}.", self.name(&s)));
                self.push(base + 1, item);
            }
        }
    }

    /// After `JOIN`: the tables related to those in scope, each with its alias and its
    /// whole condition (`orders o ON o.user_id = u.id`): real and accepted keys first,
    /// those found from names last.
    pub(super) fn related(&mut self) {
        let Some(index) = self.index else { return };
        let own: Vec<ScopeTable> =
            self.scope.tables.iter().filter(|t| t.level == 0).cloned().collect();
        let mut taken: Vec<String> =
            self.scope.tables.iter().map(|t| t.name().to_lowercase()).collect();
        for left in &own {
            let Some(li) = self.resolve(left) else { continue };
            for r in index.relations_of(li).cloned().collect::<Vec<_>>() {
                let other = if r.from as usize == li { r.to } else { r.from } as usize;
                let alias = alias_for(&index.table(other).table.name, &taken);
                let right_name = alias.clone();
                let condition = self.condition_text(&r, other, &right_name, li, left.name());
                let text =
                    format!("{} {} ON {}", self.table_text(other), self.name(&alias), condition);
                let mut item = Suggestion::new(text.clone(), SuggestionKind::Join);
                item.detail = Some(relation_detail(index, &r));
                item.inferred = r.origin == RelationOrigin::Inferred;
                // The tables it points to first, then those pointing to it, then those
                // found from names.
                let group = if item.inferred {
                    2
                } else if r.from as usize == li {
                    0
                } else {
                    1
                };
                let key = index.table(other).table.name.clone();
                self.push_by(group, &key, item);
            }
            taken.push(left.name().to_lowercase());
        }
    }

    /// `relation`'s condition between table `right` (written `right_name`) and table
    /// `left` (written `left_name`), the right side first: `u.id = o.user_id`.
    fn condition_text(
        &self,
        r: &IndexRelation,
        right: usize,
        right_name: &str,
        left: usize,
        left_name: &str,
    ) -> String {
        let _ = left;
        // The table joined is the key's own (its columns point to the other's), or the one
        // pointed to (a tree's parent: `p.id = c.parent_id`).
        let right_is_from = r.from as usize == right && r.from != r.to;
        let (rc, lc) =
            if right_is_from { (&r.columns, &r.references) } else { (&r.references, &r.columns) };
        rc.iter()
            .zip(lc)
            .map(|(a, b)| {
                format!(
                    "{}.{} = {}.{}",
                    self.name(right_name),
                    self.name(a),
                    self.name(left_name),
                    self.name(b)
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// After `JOIN t ON`: the conditions between `t` and the tables before it.
    pub(super) fn conditions(&mut self, right: usize, left: &[usize]) {
        let Some(index) = self.index else { return };
        let Some(rt) = self.scope.tables.get(right).cloned() else { return };
        let Some(ri) = self.resolve(&rt) else { return };
        for &l in left {
            let Some(lt) = self.scope.tables.get(l).cloned() else { continue };
            let Some(li) = self.resolve(&lt) else { continue };
            for r in index.relations_of(ri).cloned().collect::<Vec<_>>() {
                let other = if r.from as usize == ri { r.to } else { r.from } as usize;
                if other != li {
                    continue;
                }
                let text = self.condition_text(&r, ri, rt.name(), li, lt.name());
                let mut item = Suggestion::new(text, SuggestionKind::Join);
                item.detail = Some(relation_detail(index, &r));
                item.inferred = r.origin == RelationOrigin::Inferred;
                let group = if item.inferred { 2 } else { 0 };
                self.push_by(group, &self.query.clone(), item);
            }
        }
    }
}
