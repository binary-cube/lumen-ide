//! Profiles of a statement (protocol 3): the plan's nodes, their figures, the options.

use database as d;

use crate::dto;

macro_rules! same_fields {
    ($from:ty => $to:ty { $($field:ident),+ $(,)? }) => {
        impl From<$from> for $to {
            fn from(v: $from) -> Self {
                Self { $($field: v.$field),+ }
            }
        }
    };
}

same_fields!(d::ProfileCaps => dto::ProfileCaps {
    estimated, actual, node_timing, buffers, io_timing, wal, memory, spill, parallel, settings,
    verbose_output, jit, triggers, engine_counters, node_actuals_by_scan, statement_history,
    table_stats, undo_writes
});
same_fields!(dto::ProfileCaps => d::ProfileCaps {
    estimated, actual, node_timing, buffers, io_timing, wal, memory, spill, parallel, settings,
    verbose_output, jit, triggers, engine_counters, node_actuals_by_scan, statement_history,
    table_stats, undo_writes
});

use d::profile as p;

same_fields!(p::Io => dto::Io {
    shared_hit, shared_read, shared_dirtied, shared_written, local_hit, local_read,
    local_dirtied, local_written, temp_read, temp_written, read_ms, write_ms, local_read_ms,
    local_write_ms, temp_read_ms, temp_write_ms, block_size
});
same_fields!(dto::Io => p::Io {
    shared_hit, shared_read, shared_dirtied, shared_written, local_hit, local_read,
    local_dirtied, local_written, temp_read, temp_written, read_ms, write_ms, local_read_ms,
    local_write_ms, temp_read_ms, temp_write_ms, block_size
});
same_fields!(p::HashStats => dto::HashStats { buckets, original_buckets, batches, original_batches });
same_fields!(dto::HashStats => p::HashStats { buckets, original_buckets, batches, original_batches });
same_fields!(p::CacheStats => dto::CacheStats { hits, misses, evictions, overflows });
same_fields!(dto::CacheStats => p::CacheStats { hits, misses, evictions, overflows });
same_fields!(p::Wal => dto::Wal { records, full_page_images, bytes, buffers_full });
same_fields!(dto::Wal => p::Wal { records, full_page_images, bytes, buffers_full });
same_fields!(p::NodeFlags => dto::Flags { disabled, async_capable, partial });
same_fields!(dto::Flags => p::NodeFlags { disabled, async_capable, partial });
same_fields!(p::PlanningMemory => dto::PlanningMemory { used_bytes, allocated_bytes });
same_fields!(dto::PlanningMemory => p::PlanningMemory { used_bytes, allocated_bytes });
same_fields!(p::Serialize => dto::Serialization { ms, bytes, format });
same_fields!(dto::Serialization => p::Serialize { ms, bytes, format });
same_fields!(p::Setting => dto::Setting { name, value });
same_fields!(dto::Setting => p::Setting { name, value });
same_fields!(p::Wait => dto::Wait { event, ms, count });
same_fields!(dto::Wait => p::Wait { event, ms, count });
same_fields!(p::TriggerTime => dto::Trigger { name, relation, calls, ms });
same_fields!(dto::Trigger => p::TriggerTime { name, relation, calls, ms });
same_fields!(p::Jit => dto::Jit {
    functions, generation_ms, inlining_ms, optimization_ms, emission_ms, deform_ms, total_ms
});
same_fields!(dto::Jit => p::Jit {
    functions, generation_ms, inlining_ms, optimization_ms, emission_ms, deform_ms, total_ms
});
same_fields!(p::EngineCounters => dto::Counters {
    fullscan_steps, sorts, autoindex, vm_steps, reprepare, runs, filter_hits, filter_misses,
    mem_used, rows_examined, select_scan, sort_merge_passes, tmp_disk_tables
});
same_fields!(dto::Counters => p::EngineCounters {
    fullscan_steps, sorts, autoindex, vm_steps, reprepare, runs, filter_hits, filter_misses,
    mem_used, rows_examined, select_scan, sort_merge_passes, tmp_disk_tables
});
same_fields!(p::StatementStats => dto::History {
    calls, mean_ms, stddev_ms, min_ms, max_ms, rows, hit_ratio, temp_blocks, wal_bytes
});
same_fields!(dto::History => p::StatementStats {
    calls, mean_ms, stddev_ms, min_ms, max_ms, rows, hit_ratio, temp_blocks, wal_bytes
});
same_fields!(p::Estimates => dto::Estimates { startup_cost, total_cost, rows, width });
same_fields!(dto::Estimates => p::Estimates { startup_cost, total_cost, rows, width });
same_fields!(p::Actuals => dto::Actuals {
    rows, loops, startup_ms, total_ms, never_executed, rows_removed_by_filter,
    rows_removed_by_join_filter, rows_removed_by_recheck, heap_fetches, index_searches,
    exact_blocks, lossy_blocks
});
same_fields!(dto::Actuals => p::Actuals {
    rows, loops, startup_ms, total_ms, never_executed, rows_removed_by_filter,
    rows_removed_by_join_filter, rows_removed_by_recheck, heap_fetches, index_searches,
    exact_blocks, lossy_blocks
});

impl From<&p::Extra> for dto::Extra {
    fn from(e: &p::Extra) -> Self {
        Self {
            key: e.key.clone(),
            label: e.label.clone(),
            group: e.group.clone(),
            value: (&e.value).into(),
        }
    }
}

impl From<&p::ExtraValue> for dto::ExtraValue {
    fn from(v: &p::ExtraValue) -> Self {
        use serde_json::Value as J;
        let (value, unit) = match v {
            p::ExtraValue::Bool(b) => (J::Bool(*b), None),
            p::ExtraValue::Int(n) => (J::from(*n), None),
            p::ExtraValue::Float(n) => (J::from(*n), None),
            p::ExtraValue::Text(t) => (J::String(t.clone()), None),
            p::ExtraValue::Quantity(q) => (J::from(q.value), Some(q.unit.id().to_owned())),
            p::ExtraValue::List(items) => (
                J::Array(
                    items
                        .iter()
                        .map(|i| serde_json::to_value(dto::ExtraValue::from(i)).unwrap_or(J::Null))
                        .collect(),
                ),
                None,
            ),
            p::ExtraValue::Group(items) => (
                J::Array(
                    items
                        .iter()
                        .map(|i| serde_json::to_value(dto::Extra::from(i)).unwrap_or(J::Null))
                        .collect(),
                ),
                None,
            ),
        };
        Self { kind: v.type_id().to_owned(), value, unit }
    }
}

impl From<dto::Extra> for p::Extra {
    fn from(e: dto::Extra) -> Self {
        Self { key: e.key, label: e.label, group: e.group, value: e.value.into() }
    }
}

/// A value as far as it's understood: an unknown type is kept as its JSON's text.
impl From<dto::ExtraValue> for p::ExtraValue {
    fn from(v: dto::ExtraValue) -> Self {
        use serde_json::Value as J;
        match (v.kind.as_str(), v.value) {
            ("bool", J::Bool(b)) => p::ExtraValue::Bool(b),
            ("int", J::Number(n)) if n.as_i64().is_some() => {
                p::ExtraValue::Int(n.as_i64().unwrap_or_default())
            }
            ("float" | "int", J::Number(n)) => p::ExtraValue::Float(n.as_f64().unwrap_or_default()),
            ("text", J::String(t)) => p::ExtraValue::Text(t),
            ("quantity", J::Number(n)) => p::ExtraValue::Quantity(p::Quantity::new(
                n.as_f64().unwrap_or_default(),
                v.unit.as_deref().and_then(p::Unit::from_id).unwrap_or(p::Unit::None),
            )),
            ("list", J::Array(items)) => p::ExtraValue::List(
                items
                    .into_iter()
                    .filter_map(|i| serde_json::from_value::<dto::ExtraValue>(i).ok())
                    .map(Into::into)
                    .collect(),
            ),
            ("group", J::Array(items)) => p::ExtraValue::Group(
                items
                    .into_iter()
                    .filter_map(|i| serde_json::from_value::<dto::Extra>(i).ok())
                    .map(Into::into)
                    .collect(),
            ),
            (_, J::String(t)) => p::ExtraValue::Text(t),
            (_, J::Bool(b)) => p::ExtraValue::Bool(b),
            (_, other) => p::ExtraValue::Text(other.to_string()),
        }
    }
}

fn extras(list: &[p::Extra]) -> Vec<dto::Extra> {
    list.iter().map(Into::into).collect()
}

fn extras_back(list: Vec<dto::Extra>) -> Vec<p::Extra> {
    list.into_iter().map(Into::into).collect()
}

impl From<&p::ProfileNode> for dto::ProfileNode {
    fn from(n: &p::ProfileNode) -> Self {
        let memory = n.memory.as_ref().map(|m| dto::Memory {
            peak_bytes: m.peak_bytes,
            sort_method: m.sort_method.clone(),
            sort_space: m
                .sort_space
                .map(|(bytes, s)| dto::SortSpace { bytes, storage: s.id().to_owned() }),
            hash: m.hash.map(Into::into),
            cache: m.cache.map(Into::into),
            storage: m.storage.map(|s| s.id().to_owned()),
        });
        let parallel = n.parallel.as_ref().map(|par| dto::Parallel {
            planned: par.planned,
            launched: par.launched,
            aware: par.aware,
            per_worker: par
                .per_worker
                .iter()
                .map(|w| dto::Worker {
                    number: w.number,
                    rows: w.rows,
                    loops: w.loops,
                    startup_ms: w.startup_ms,
                    total_ms: w.total_ms,
                    io: w.io.map(Into::into),
                    extra: extras(&w.extra),
                })
                .collect(),
        });
        Self {
            parent: n.parent.map(|p| p.0),
            children: n.children.iter().map(|c| c.0).collect(),
            role: n.role.map(|r| r.id().to_owned()),
            subplan_name: n.subplan_name.clone(),
            op: n.op.id().to_owned(),
            name: n.name.clone(),
            join: n.join.map(|j| j.id().to_owned()),
            object: n.object.as_ref().map(|o| dto::ObjectRef {
                kind: o.kind.id().to_owned(),
                schema: o.schema.clone(),
                name: o.name.clone(),
                alias: o.alias.clone(),
            }),
            index: n.index.clone(),
            scan_direction: n.scan_direction.map(|s| s.id().to_owned()),
            conditions: n
                .conditions
                .iter()
                .map(|c| dto::Condition {
                    kind: c.kind.id().to_owned(),
                    label: match &c.kind {
                        p::CondKind::Other(l) if !l.is_empty() => Some(l.clone()),
                        _ => None,
                    },
                    text: c.text.clone(),
                })
                .collect(),
            output: n.output.clone(),
            estimates: (n.estimates != p::Estimates::default()).then(|| n.estimates.into()),
            actuals: (n.actuals != p::Actuals::default()).then(|| n.actuals.into()),
            io: n.io.map(Into::into),
            memory,
            spill: n.spill.map(|s| dto::Spill {
                kind: s.kind.id().to_owned(),
                disk_bytes: s.disk_bytes,
                batches: s.batches,
            }),
            parallel,
            wal: n.wal.map(Into::into),
            flags: (n.flags != p::NodeFlags::default()).then(|| n.flags.into()),
            extra: extras(&n.extra),
        }
    }
}

fn node_back(n: dto::ProfileNode) -> p::ProfileNode {
    let op = p::OpKind::from_id(&n.op).unwrap_or(p::OpKind::Other);
    let mut node = p::ProfileNode::new(op, n.name);
    node.parent = n.parent.map(p::NodeId);
    node.children = n.children.into_iter().map(p::NodeId).collect();
    node.role = n.role.as_deref().and_then(p::ChildRole::from_id);
    node.subplan_name = n.subplan_name;
    node.join = n.join.as_deref().and_then(p::JoinKind::from_id);
    node.object = n.object.map(|o| p::ObjectRef {
        kind: p::ObjectKind::from_id(&o.kind).unwrap_or(p::ObjectKind::Other),
        schema: o.schema,
        name: o.name,
        alias: o.alias,
    });
    node.index = n.index;
    node.scan_direction = n.scan_direction.as_deref().and_then(p::ScanDirection::from_id);
    node.conditions = n
        .conditions
        .into_iter()
        .map(|c| {
            let kind = p::CondKind::from_id(&c.kind, c.label.as_deref())
                .unwrap_or_else(|| p::CondKind::Other(c.label.unwrap_or(c.kind)));
            p::Condition { kind, text: c.text }
        })
        .collect();
    node.output = n.output;
    node.estimates = n.estimates.map(Into::into).unwrap_or_default();
    node.actuals = n.actuals.map(Into::into).unwrap_or_default();
    node.io = n.io.map(Into::into);
    node.memory = n.memory.map(|m| p::Memory {
        peak_bytes: m.peak_bytes,
        sort_method: m.sort_method,
        sort_space: m
            .sort_space
            .map(|s| (s.bytes, p::Storage::from_id(&s.storage).unwrap_or(p::Storage::Memory))),
        hash: m.hash.map(Into::into),
        cache: m.cache.map(Into::into),
        storage: m.storage.as_deref().and_then(p::Storage::from_id),
    });
    node.spill = n.spill.and_then(|s| {
        Some(p::Spill {
            kind: p::SpillKind::from_id(&s.kind)?,
            disk_bytes: s.disk_bytes,
            batches: s.batches,
        })
    });
    node.parallel = n.parallel.map(|par| p::Parallel {
        planned: par.planned,
        launched: par.launched,
        aware: par.aware,
        per_worker: par
            .per_worker
            .into_iter()
            .map(|w| p::WorkerStats {
                number: w.number,
                rows: w.rows,
                loops: w.loops,
                startup_ms: w.startup_ms,
                total_ms: w.total_ms,
                io: w.io.map(Into::into),
                extra: extras_back(w.extra),
            })
            .collect(),
    });
    node.wal = n.wal.map(Into::into);
    node.flags = n.flags.map(Into::into).unwrap_or_default();
    node.extra = extras_back(n.extra);
    node
}

impl From<&p::StatementProfile> for dto::StatementProfile {
    fn from(s: &p::StatementProfile) -> Self {
        Self {
            planning_ms: s.planning_ms,
            execution_ms: s.execution_ms,
            wall_ms: s.wall_ms,
            first_row_ms: s.first_row_ms,
            fetch_ms: s.fetch_ms,
            rows_returned: s.rows_returned,
            rows_affected: s.rows_affected,
            planning_io: s.planning_io.map(Into::into),
            io_total: s.io_total.map(Into::into),
            planning_memory: s.planning_memory.map(Into::into),
            serialize: s.serialize.clone().map(Into::into),
            jit: s.jit.map(Into::into),
            triggers: s.triggers.iter().cloned().map(Into::into).collect(),
            settings: s.settings.iter().cloned().map(Into::into).collect(),
            query_id: s.query_id,
            counters: s.counters.map(Into::into),
            history: s.history.map(Into::into),
            waits: s.waits.iter().cloned().map(Into::into).collect(),
            extra: extras(&s.extra),
        }
    }
}

impl From<dto::StatementProfile> for p::StatementProfile {
    fn from(s: dto::StatementProfile) -> Self {
        Self {
            planning_ms: s.planning_ms,
            execution_ms: s.execution_ms,
            wall_ms: s.wall_ms,
            first_row_ms: s.first_row_ms,
            fetch_ms: s.fetch_ms,
            rows_returned: s.rows_returned,
            rows_affected: s.rows_affected,
            planning_io: s.planning_io.map(Into::into),
            io_total: s.io_total.map(Into::into),
            planning_memory: s.planning_memory.map(Into::into),
            serialize: s.serialize.map(Into::into),
            jit: s.jit.map(Into::into),
            triggers: s.triggers.into_iter().map(Into::into).collect(),
            settings: s.settings.into_iter().map(Into::into).collect(),
            query_id: s.query_id,
            counters: s.counters.map(Into::into),
            history: s.history.map(Into::into),
            waits: s.waits.into_iter().map(Into::into).collect(),
            extra: extras_back(s.extra),
        }
    }
}

impl From<&p::Profile> for dto::Profile {
    fn from(profile: &p::Profile) -> Self {
        Self {
            format: profile.format,
            id: Some(profile.id.to_string()),
            engine: dto::Engine {
                driver: profile.engine.driver.clone(),
                name: profile.engine.name.clone(),
                version: profile.engine.version.clone(),
                version_num: profile.engine.version_num,
            },
            depth: p::export::depth_id(profile.depth).to_owned(),
            statement: (&profile.statement).into(),
            nodes: profile.nodes.iter().map(Into::into).collect(),
            roots: profile.roots.iter().map(|r| r.0).collect(),
            collected: profile.collected.into(),
            notes: profile
                .notes
                .iter()
                .map(|n| dto::ProfileNote { id: n.id().to_owned(), argument: n.argument() })
                .collect(),
            raw: profile.raw.as_ref().map(|r| dto::RawPlan {
                format: r.format.id().to_owned(),
                text: r.text.to_string(),
            }),
        }
    }
}

/// A profile as far as it's understood: of a newer format, what isn't known is left out
/// (said by [`p::ProfileNote::NewerFormat`]); a node listed where it can't be (a parent or
/// child past the list) is dropped from the tree, never a panic.
pub fn profile_from(dto: dto::Profile) -> p::Profile {
    let depth = p::export::depth_from_id(&dto.depth).unwrap_or(p::ProfileDepth::Estimated);
    let engine = p::Engine {
        driver: dto.engine.driver,
        name: dto.engine.name,
        version: dto.engine.version,
        version_num: dto.engine.version_num,
    };
    let mut profile = p::Profile::new(engine, depth);
    if let Some(id) = dto.id.as_deref().and_then(p::ProfileId::parse) {
        profile.id = id;
    }
    let count = dto.nodes.len() as u32;
    profile.nodes = dto.nodes.into_iter().map(node_back).collect();
    for node in &mut profile.nodes {
        node.parent = node.parent.filter(|p| p.0 < count);
        node.children.retain(|c| c.0 < count);
    }
    profile.roots = dto.roots.into_iter().filter(|r| *r < count).map(p::NodeId).collect();
    profile.statement = dto.statement.into();
    profile.collected = dto.collected.into();
    profile.notes =
        dto.notes.into_iter().filter_map(|n| p::ProfileNote::from_id(&n.id, n.argument)).collect();
    profile.raw = dto.raw.and_then(|r| {
        Some(p::RawPlan { format: p::RawFormat::from_id(&r.format)?, text: r.text.into() })
    });
    if dto.format > p::PROFILE_FORMAT {
        profile.note(p::ProfileNote::NewerFormat { format: dto.format });
    }
    profile.mark_size();
    profile
}

impl From<&p::ProfileOptions> for dto::ProfileOptions {
    fn from(o: &p::ProfileOptions) -> Self {
        Self {
            depth: p::export::depth_id(o.depth).to_owned(),
            buffers: o.buffers,
            verbose: o.verbose,
            settings: o.settings,
            wal: o.wal,
            serialize: o.serialize,
            statement_history: o.statement_history,
            timeout_ms: o.timeout.map(|t| t.as_millis().min(u128::from(u64::MAX)) as u64),
            max_raw_bytes: Some(o.max_raw_bytes as u64),
        }
    }
}

impl From<dto::ProfileOptions> for p::ProfileOptions {
    fn from(o: dto::ProfileOptions) -> Self {
        Self {
            depth: p::export::depth_from_id(&o.depth).unwrap_or(p::ProfileDepth::Estimated),
            buffers: o.buffers,
            verbose: o.verbose,
            settings: o.settings,
            wal: o.wal,
            serialize: o.serialize,
            statement_history: o.statement_history,
            timeout: o.timeout_ms.map(std::time::Duration::from_millis),
            max_raw_bytes: o
                .max_raw_bytes
                .map_or(p::MAX_RAW_BYTES, |b| (b as usize).min(p::MAX_RAW_BYTES)),
        }
    }
}

impl From<&p::TableStats> for dto::TableStats {
    fn from(t: &p::TableStats) -> Self {
        Self {
            table: t.table.clone().into(),
            live_rows: t.live_rows,
            dead_rows: t.dead_rows,
            table_bytes: t.table_bytes,
            index_bytes: t.index_bytes,
            total_bytes: t.total_bytes,
            seq_scans: t.seq_scans,
            idx_scans: t.idx_scans,
            last_analyze: t.last_analyze.clone(),
            last_autoanalyze: t.last_autoanalyze.clone(),
            last_vacuum: t.last_vacuum.clone(),
            modified_since_analyze: t.modified_since_analyze,
            stats_present: t.stats_present,
            indexes: t
                .indexes
                .iter()
                .map(|i| dto::IndexStats {
                    name: i.name.clone(),
                    columns: i.columns.clone(),
                    unique: i.unique,
                    bytes: i.bytes,
                    scans: i.scans,
                })
                .collect(),
            extra: extras(&t.extra),
        }
    }
}

impl From<dto::TableStats> for p::TableStats {
    fn from(t: dto::TableStats) -> Self {
        Self {
            table: t.table.into(),
            live_rows: t.live_rows,
            dead_rows: t.dead_rows,
            table_bytes: t.table_bytes,
            index_bytes: t.index_bytes,
            total_bytes: t.total_bytes,
            seq_scans: t.seq_scans,
            idx_scans: t.idx_scans,
            last_analyze: t.last_analyze,
            last_autoanalyze: t.last_autoanalyze,
            last_vacuum: t.last_vacuum,
            modified_since_analyze: t.modified_since_analyze,
            stats_present: t.stats_present,
            indexes: t
                .indexes
                .into_iter()
                .map(|i| p::IndexStats {
                    name: i.name,
                    columns: i.columns,
                    unique: i.unique,
                    bytes: i.bytes,
                    scans: i.scans,
                })
                .collect(),
            extra: extras_back(t.extra),
        }
    }
}
