//! The relations read and laid out: the graph read on a worker, what's shown worked out,
//! the layout made in the background and taken when it comes.

use super::*;

impl RelationsView {
    pub(super) fn wake(&self) -> Waker {
        let waker = Arc::clone(&self.waker);
        Arc::new(move || {
            if let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                wake();
            }
        })
    }

    /// Reads the source's structure again, in the background (what's shown stays until
    /// it comes).
    pub(super) fn read(&mut self) {
        let Some(request) = self.request.clone() else { return };
        if let Some(old) = self.reading.take() {
            old.cancel.cancel();
        }
        let cancel = Cancel::default();
        let progress = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0)));
        let (tx, rx) = std::sync::mpsc::channel();
        let (stop, counts, wake) = (cancel.clone(), Arc::clone(&progress), self.wake());
        kernel::work::run(THREAD, move || {
            let told = Arc::clone(&wake);
            // The counts kept at each table; the app woken at most every 50 ms (thousands
            // of tables read: not a wake each), and at the last.
            let started = std::time::Instant::now();
            // When it was last woken (0: never; the first table wakes it).
            let last = AtomicU64::new(0);
            let tell = move |done: usize, total: usize| {
                counts.0.store(done, Ordering::Relaxed);
                counts.1.store(total, Ordering::Relaxed);
                let now = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let before = last.load(Ordering::Relaxed);
                let due = before == 0 || now.saturating_sub(before) >= 50;
                if due || done >= total {
                    last.store(now.max(1), Ordering::Relaxed);
                    told();
                }
            };
            let result = request.access.read(request.schema.as_deref(), &stop, &tell).map(|t| {
                let graph = SchemaGraph::new(t);
                let found = infer(&graph);
                Read { graph, found, notes: request.access.notes() }
            });
            if !stop.is_cancelled() && tx.send(result).is_ok() {
                wake();
            }
        });
        self.reading = Some(Reading { rx, cancel, progress });
    }

    /// Shows `graph` (given directly in tests): the relations its names say found here,
    /// the decisions the source keeps.
    pub fn set_graph(&mut self, graph: SchemaGraph) {
        let found = infer(&graph);
        let notes = self.request.as_ref().map(|r| r.access.notes()).unwrap_or_default();
        self.show_read(Read { graph, found, notes });
    }

    /// Shows what was read: the table asked first becomes the one whose neighbors are
    /// shown; the tables kept by their names across a read again.
    pub(super) fn show_read(&mut self, read: Read) {
        let Read { graph, found, notes } = read;
        let name = |g: &Option<Arc<SchemaGraph>>, i: Option<usize>| -> Option<TableRef> {
            g.as_ref().and_then(|g| i.and_then(|i| g.tables.get(i)).map(|t| t.table().clone()))
        };
        let first = self.graph.is_none();
        let (focus, selected) = (name(&self.graph, self.focus), name(&self.graph, self.selected));
        let wanted = self.request.as_ref().and_then(|r| r.focus.clone());
        self.focus = None;
        self.selected = None;
        if first {
            if let Some(table) = wanted.and_then(|t| graph.find(&t)) {
                self.focus = Some(table);
                self.selected = Some(table);
            }
        } else {
            self.focus = focus.and_then(|t| graph.find(&t));
            self.selected = selected.and_then(|t| graph.find(&t));
            // The places before point to the tables by place: gone with the old graph.
            self.history.clear();
        }
        // The lines are known by their tables' places: gone with the old graph.
        self.edge = None;
        self.popover = None;
        self.error = None;
        self.base = Some(Arc::new(graph));
        self.found = Arc::new(found);
        self.notes = notes;
        self.refit = first;
        self.rebuild();
    }

    /// The graph made again from the base, the relations found and the decisions; laid out.
    pub(super) fn rebuild(&mut self) {
        self.undecided = (0..self.found.len())
            .filter(|&i| {
                let key = &self.found[i].key;
                !self.notes.is_accepted(key) && !self.notes.is_ignored(key)
            })
            .collect();
        let Some(base) = &self.base else { return };
        self.graph = Some(Arc::new(base.with_notes(&self.found, &self.notes)));
        self.relayout();
    }

    /// What's to be shown now: every table or a table's neighbors, narrowed by the filter.
    pub fn wanted(&self) -> Shown {
        let Some(graph) = &self.graph else { return Shown::default() };
        let kinds = self.notes.show;
        let mut tables = match self.focus {
            Some(f) => graph.neighborhood(f, self.depth, |l| l.origin.shown(kinds)),
            None => (0..graph.tables.len()).collect(),
        };
        let filter = self.filter.text().trim();
        if !filter.is_empty() {
            let matching = graph.matching(filter);
            tables.retain(|t| matching.binary_search(t).is_ok() || Some(*t) == self.focus);
        }
        let links = graph
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.origin.shown(kinds)
                    && tables.binary_search(&l.from).is_ok()
                    && tables.binary_search(&l.to).is_ok()
            })
            .map(|(i, _)| i)
            .collect();
        Shown { tables, links, focus: self.focus }
    }

    /// Lays out what's to be shown, in the background, unless it's what's shown already.
    pub(super) fn relayout(&mut self) {
        let wanted = self.wanted();
        let Some(graph) = self.graph.clone() else { return };
        if wanted == self.shown
            && self.laying.is_none()
            && Arc::ptr_eq(&graph, &self.drawn)
            && !self.layout.layout.cards.is_empty()
        {
            return;
        }
        if self.laying.as_ref().is_some_and(|(s, g, _)| *s == wanted && Arc::ptr_eq(g, &graph)) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (shown, wake, of) = (wanted.clone(), self.wake(), Arc::clone(&graph));
        kernel::work::run(THREAD, move || {
            if tx.send(lay_out(&of, &shown)).is_ok() {
                wake();
            }
        });
        self.laying = Some((wanted, graph, rx));
    }

    /// Lays out what's to be shown here and now (tests, snapshots: the same function the
    /// background runs).
    pub fn lay_out_now(&mut self) {
        let Some(graph) = self.graph.clone() else { return };
        let wanted = self.wanted();
        self.laying = None;
        let layout = lay_out(&graph, &wanted);
        self.take_layout(wanted, graph, layout);
    }

    pub(super) fn take_layout(
        &mut self,
        shown: Shown,
        graph: Arc<SchemaGraph>,
        layout: DiagramLayout,
    ) {
        self.kinds_drawn = [false; 3];
        // What each frame would otherwise look for in the whole model: the schemas, each
        // row's link (the first shown that starts at it).
        let mut schemas = graph.tables.iter().filter_map(|t| t.table().schema.as_deref());
        let first = schemas.next();
        self.several_schemas = schemas.any(|s| Some(s) != first);
        self.row_links.clear();
        for &i in &shown.links {
            if let Some(l) = graph.links.get(i)
                && let Some(column) = l.columns.first()
            {
                self.row_links.entry(l.from).or_default().entry(column.clone()).or_insert(i);
            }
        }
        for l in shown.links.iter().filter_map(|&l| graph.links.get(l)) {
            let kind = match l.origin {
                database::diagram::Origin::Real => 0,
                database::diagram::Origin::Virtual => 1,
                database::diagram::Origin::Inferred(_) => 2,
            };
            self.kinds_drawn[kind] = true;
        }
        self.shown = shown;
        self.drawn = graph;
        self.layout = Arc::new(layout);
        self.layout_generation += 1;
        if std::mem::take(&mut self.refit) {
            self.view.fit_pending = true;
        } else {
            self.center_selected();
        }
    }
}
