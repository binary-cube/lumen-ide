//! The literals offered where an enum column's value is written: the column found among
//! the tables in scope, its values quoted.

use crate::sql::value::ValueOf;

use super::{Completer, EnumColumn, Suggestion, SuggestionKind};

impl<'a> Completer<'a> {
    /// The literals of the enum column whose value is written at the cursor, in their
    /// declared order, quoted (inside an open literal, it's replaced whole).
    pub(super) fn literals(&mut self) {
        let Some(value_of) = self.scope.value_of.clone() else { return };
        let Some((owner, column, values)) = self.value_column(&value_of) else { return };
        let detail = format!("{owner}.{column}");
        for value in values.iter() {
            let item = Suggestion::new(value.to_string(), SuggestionKind::Value)
                .insert(crate::sql::value::literal(value))
                .detail(detail.clone());
            self.push_by(0, &value.to_lowercase(), item);
        }
    }

    /// The column `value_of` names among the tables in scope (the innermost first): its
    /// table's name, its name, its values (`None`: not an enum, or not known — its columns
    /// asked for).
    fn value_column(&mut self, value_of: &ValueOf) -> Option<EnumColumn> {
        let index = self.index?;
        let (tables, wanted): (Vec<usize>, Result<&str, usize>) = match value_of {
            ValueOf::Column { qualifier, name } if qualifier.is_empty() => {
                let mut all: Vec<usize> = (0..self.scope.tables.len()).collect();
                all.sort_by_key(|&i| self.scope.tables[i].level);
                (all, Ok(name.as_str()))
            }
            ValueOf::Column { qualifier, name } => {
                (self.scope.qualified(qualifier), Ok(name.as_str()))
            }
            ValueOf::Position { table, position } => (self.scope.qualified(table), Err(*position)),
        };
        for t in tables {
            let scope_table = self.scope.tables[t].clone();
            let Some(i) = self.resolve(&scope_table) else { continue };
            let table = index.table(i);
            let Some(columns) = &table.columns else {
                self.missing.push(table.table.clone());
                continue;
            };
            let column = match wanted {
                Ok(name) => columns.iter().find(|c| c.name.eq_ignore_ascii_case(name)),
                Err(position) => columns.get(position),
            };
            if let Some(c) = column {
                let values = c.values.clone()?;
                return Some((table.table.name.clone(), c.name.to_string(), values));
            }
        }
        None
    }
}
