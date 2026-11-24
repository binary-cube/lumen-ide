//! The small steps the others are made of: frames and waits, typing, searches, a drag, a
//! split, the canvas, a hover.

use super::*;

impl Steps<'_> {
    /// Polls the views until none is busy, or `timeout` passes.
    pub(super) fn wait_for_viewers(&mut self, timeout: Duration) {
        self.wait(timeout, |wb| {
            wb.poll_viewers();
            !wb.viewer_busy() && !wb.viewer_working()
        });
    }

    /// The active file revealed in the tree, then a tree entry dragged (and dropped).
    pub(super) fn tree_drag(&mut self, drag: TabDrag) {
        if let Some(i) = self.wb.active_tab() {
            self.wb.reveal_in_project(i);
        }
        self.tab_drag(drag);
        if drag.drop {
            self.wait_for_viewers(Duration::from_secs(20));
        }
    }

    /// The first tab pinned, then the tab bar scrolled by `dx` pixels.
    pub(super) fn pin(&mut self, dx: f32) {
        self.wb.toggle_pin(0);
        self.frame();
        self.wb.scroll(Point::new(1400.0, 118.0), dx, 0.0);
    }

    /// The rename preview for the symbol at the cursor, `name` typed (empty: the old name).
    pub(super) fn rename(&mut self, name: &str) {
        self.wait(Duration::from_secs(20), |wb| !wb.indexing());
        self.wb.run_command(command_model::CommandId("refactor.rename"));
        self.wait_for_viewers(Duration::from_secs(20));
        if !name.is_empty() {
            self.wb.type_text(self.text, name);
        }
    }

    /// The Move preview: `folder[,typed][!]`.
    pub(super) fn move_entry(&mut self, spec: &str) {
        self.wait(Duration::from_secs(20), |wb| !wb.indexing());
        let apply = spec.ends_with('!');
        let spec = spec.trim_end_matches('!');
        let (folder, typed) = spec.split_once(',').unwrap_or((spec, ""));
        let (Some(root), Some(from)) =
            (self.wb.project_root(), self.wb.active_tab().and_then(|i| self.wb.tab_path(i)))
        else {
            return;
        };
        self.wb.move_entry(&from, &root.join(folder));
        self.wait_for_viewers(Duration::from_secs(20));
        if !typed.is_empty() {
            self.wb.viewer_command("select_all");
            self.wb.type_text(self.text, typed);
        }
        if apply {
            self.wb.edit_key(self.text, EditKey::Enter { shift: false });
        }
    }

    /// The active file's local history, its versions read from `dir` (a fixture: the view
    /// opened with its own data, the module's type — the only one named here).
    pub(super) fn local_history(&mut self, dir: PathBuf) {
        let Some(i) = self.wb.active_tab() else { return };
        let (Some(file), Some(path)) = (self.wb.tab_path(i), self.wb.tab_relative_path(i)) else {
            return;
        };
        let data = local_history::HistoryData {
            store: std::sync::Arc::new(history_fs::DiskHistory::new(dir)),
            path: path.to_string_lossy().replace('\\', "/"),
            file,
        };
        workbench_model::ViewHost::show_in_panel(
            self.wb,
            local_history::TOOL,
            "History",
            local_history::EXTENSION,
            &data,
        );
    }

    /// A view that needs the index (a diagram, the usages, Change Signature), once ready.
    pub(super) fn indexed_view(&mut self, command: command_model::CommandId) {
        self.wait(Duration::from_secs(20), |wb| !wb.indexing());
        self.wb.run_command(command);
        self.wait_for_viewers(Duration::from_secs(20));
    }

    /// The window's own buttons (`1`, `max`), or full screen (`full`, `full-shown`).
    pub(super) fn window_controls(&mut self, controls: &str) {
        self.wb.set_window_controls(Some(controls == "max"));
        // Full screen: the bar hidden, or come down (the mouse at the top edge).
        self.wb.set_full_screen(controls.starts_with("full"));
        self.frame();
        if controls == "full-shown" {
            self.wb.hover(Point::new(SIZE.width as f32 / 2.0, 0.0));
            settle(300);
            self.frame();
        }
    }

    /// A paint into a throwaway scene (layouts, hit areas and animations update).
    pub(super) fn frame(&mut self) {
        self.wb.paint(self.text, &mut Scene::default());
    }

    /// Polls `ready` until it says yes or `timeout` passes.
    pub(super) fn wait(
        &mut self,
        timeout: Duration,
        mut ready: impl FnMut(&mut Workbench) -> bool,
    ) {
        let deadline = Instant::now() + timeout;
        while !ready(self.wb) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `keys` keys typed at the end of the active file, about 12 a second (a fast typist),
    /// each followed by its frame; between keys, what the servers answered is polled and
    /// painted as the app does. Printed: what each key and each answer cost the UI thread
    /// (the edit, the servers' answers used, the frame's scene — the GPU's drawing aside,
    /// the same with or without a server), and how often the suggestions were shown.
    pub(super) fn typing(&mut self, keys: usize) {
        // The file's server ready (none: the built-in engine), its project loaded. (The
        // document is announced to the servers by a frame.)
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.wb.served_at_cursor() && Instant::now() < deadline {
            self.wb.poll_servers();
            self.frame();
            settle(20);
        }
        let served = self.wb.served_at_cursor();
        let warm = Instant::now() + Duration::from_secs(if served { 5 } else { 1 });
        while Instant::now() < warm {
            self.wb.poll_servers();
            self.wb.poll_diagnostics();
            self.frame();
            settle(20);
        }
        self.wb.place_cursor_at_line_end(self.text, None);
        const TEXT: &str = "\nconst names = items.map((item) => item.name.trim());\nconsole.log(names.length, names.join(', '));\n";
        let (mut typed, mut answers, mut listed) = (Vec::new(), Vec::new(), 0usize);
        let started = Instant::now();
        for c in TEXT.chars().cycle().take(keys) {
            let key = Instant::now();
            match c {
                '\n' => {
                    // Enter would accept a suggestion: the list closed first.
                    self.wb.edit_key(self.text, EditKey::Escape);
                    self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                }
                _ => {
                    self.wb.type_text(self.text, c.encode_utf8(&mut [0; 4]));
                }
            }
            self.wb.poll_servers();
            self.wb.poll_diagnostics();
            self.wb.paint(self.text, &mut Scene::default());
            typed.push(key.elapsed());
            let next = key + Duration::from_millis(83);
            while Instant::now() < next {
                settle(4);
                let at = Instant::now();
                if self.wb.poll_servers() | self.wb.poll_diagnostics() {
                    self.wb.paint(self.text, &mut Scene::default());
                    answers.push(at.elapsed());
                }
            }
            listed += usize::from(self.wb.completion_open());
        }
        let stats = |mut v: Vec<Duration>| {
            v.sort();
            let at = |q: f64| v.get(((v.len() as f64 - 1.0) * q) as usize).copied();
            let ms = |d: Option<Duration>| d.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            format!(
                "n {} p50 {:.2} p95 {:.2} p99 {:.2} max {:.2} ms",
                v.len(),
                ms(at(0.5)),
                ms(at(0.95)),
                ms(at(0.99)),
                ms(v.last().copied())
            )
        };
        eprintln!(
            "typing: served {served}, {keys} keys in {:.1} s, list shown after {listed}",
            started.elapsed().as_secs_f64()
        );
        eprintln!("typing: keys: {}", stats(typed));
        eprintln!("typing: answers: {}", stats(answers));
    }

    /// Every tab closed, then `seconds` + 25 s of frames (10 a second): the servers left
    /// without documents stop meanwhile. The process's memory printed before and after.
    pub(super) fn idle(&mut self, seconds: u64) {
        let mb = || crate::resources::measure_now().ram_bytes / (1024 * 1024);
        eprintln!("idle: app {} MB with the tabs open", mb());
        while self.wb.tab_count() > 0 {
            self.wb.close_tab(0);
        }
        self.frame();
        let closed = Instant::now();
        eprintln!("idle: tabs closed");
        while closed.elapsed() < Duration::from_secs(seconds + 25) {
            self.wb.poll_servers();
            self.frame();
            settle(100);
        }
        kernel::memory::release_free();
        eprintln!("idle: app {} MB after {} s", mb(), closed.elapsed().as_secs());
    }

    /// `seconds` of frames, 30 a second, nothing else done; the process's memory and the
    /// heap's live bytes printed every 10 s.
    pub(super) fn soak(&mut self, seconds: u64) {
        let mb = |b: u64| b / (1024 * 1024);
        let started = Instant::now();
        let mut next = started;
        let mut frames = 0u64;
        while started.elapsed() < Duration::from_secs(seconds) {
            if Instant::now() >= next {
                let ram = mb(crate::resources::measure_now().ram_bytes);
                let heap = kernel::memory::live_heap().map_or(0, |b| mb(b as u64));
                eprintln!(
                    "soak: {:>4} s, {frames} frames, app {ram} MB, heap {heap} MB",
                    started.elapsed().as_secs()
                );
                next += Duration::from_secs(10);
            }
            self.frame();
            frames += 1;
            settle(33);
        }
    }

    pub(super) fn wait_for_indexing(&mut self, timeout: Duration) {
        settle(50);
        self.wait(timeout, |wb| !wb.indexing());
    }

    pub(super) fn file_search(&mut self, query: &str) {
        self.wb.focus_file_search();
        self.wb.type_text(self.text, query);
        // The file index is built in the background.
        self.wait(Duration::from_secs(5), Workbench::poll_index);
        // The panel's results, from its search's thread.
        self.frame();
        self.wait(Duration::from_secs(5), Workbench::poll_side_panel);
    }

    pub(super) fn terminal(&mut self, command: &str) {
        self.wb.toggle_tool("terminal");
        // A first paint starts the shell (its size and the waker known then).
        self.frame();
        settle(300);
        self.wb.type_text(self.text, &format!("{command}\r"));
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            self.wb.poll_viewers();
            settle(20);
        }
    }

    pub(super) fn type_at_line(&mut self, typed: &str, line: Option<usize>) {
        self.wait_for_indexing(Duration::from_secs(20));
        self.wb.place_cursor_at_line_end(self.text, line);
        // The language at the cursor (HTML in PHP) comes from parsing.
        self.wb.sync_syntax();
        for c in typed.chars() {
            match c {
                '\t' => {
                    self.wb.edit_key(self.text, EditKey::Tab { shift: false });
                }
                '\n' => {
                    self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                }
                _ => {
                    self.wb.type_text(self.text, c.encode_utf8(&mut [0; 4]));
                }
            }
        }
    }

    pub(super) fn project_search(&mut self, query: &str) {
        self.wait_for_indexing(Duration::from_secs(20));
        // The semantic model, if installed (vectors are computed in the background).
        wait_for_semantic_model(Duration::from_secs(20));
        if self.wb.poll_indexer() || semantic::model().is_some() {
            settle(500);
        }
        // The folders section uses the file index.
        self.wb.start_file_index();
        self.wait(Duration::from_secs(10), Workbench::poll_index);
        self.wb.open_project_search();
        // `⇥`: Tab (from the search's field to the folder's, and back).
        for (i, part) in query.split('⇥').enumerate() {
            if i > 0 {
                self.wb.edit_key(self.text, EditKey::Tab { shift: false });
            }
            if !part.is_empty() {
                self.wb.type_text(self.text, part);
            }
        }
        settle(200);
    }

    /// Other windows with these projects (made up; this window's is the first), numbered
    /// as the app numbers them.
    pub(super) fn project_windows(&mut self, names: &[String]) {
        let windows = names
            .iter()
            .enumerate()
            .map(|(i, name)| workbench_model::ProjectWindow {
                window: 100 + i as u64,
                name: name.clone(),
                path: PathBuf::from(format!("/Users/me/work/{name}")),
                number: u8::try_from(i + 1).ok().filter(|n| *n <= 9),
                current: i == 0,
            })
            .collect();
        self.wb.set_project_windows(windows);
        self.frame();
    }

    pub(super) fn split(&mut self, splits: &[(Split, bool)]) {
        for &(direction, move_tab) in splits {
            let active = self.wb.active_tab().unwrap_or(0);
            self.wb.split_tab(active, direction, move_tab);
        }
        tracing::info!(groups = self.wb.group_count(), "editor split");
        self.wb.sync_syntax();
        self.frame();
    }

    pub(super) fn canvas(&mut self, mode: char, o: &Options) {
        self.wait(Duration::from_secs(10), |wb| !wb.indexing());
        self.wb.start_file_index();
        self.wait(Duration::from_secs(10), Workbench::poll_index);
        if let Some(line) = o.line {
            self.wb.place_cursor_at_line_end(self.text, Some(line));
        }
        self.wb.run_command(command_model::CommandId("view.canvas"));
        // Its graph, built by the module in the background.
        self.wait(Duration::from_secs(10), Workbench::poll_viewers);
        // Its mode, as its keys (`2`, `3`) send it.
        match mode {
            '2' => self.wb.viewer_command("files"),
            '3' => self.wb.viewer_command("code"),
            _ => false,
        };
        // The selected declaration edited on its card, the text typed at the start.
        if let Some(typed) = &o.canvas_edit {
            self.wb.viewer_command("edit");
            self.wb.type_text(self.text, typed);
        }
        // The ⌘K search open, with the text.
        if let Some(query) = &o.canvas_query {
            self.wb.viewer_command("search");
            self.wb.type_text(self.text, query);
        }
        self.frame();
    }

    pub(super) fn tab_drag(&mut self, drag: TabDrag) {
        let TabDrag { from, to, drop } = drag;
        self.frame();
        self.wb.mouse_down(self.text, from, false, false, false, 1);
        let middle = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
        self.wb.mouse_drag(self.text, middle);
        self.wb.mouse_drag(self.text, to);
        if drop {
            self.wb.mouse_up(self.text);
            tracing::info!(
                groups = self.wb.group_count(),
                tabs = self.wb.tab_count(),
                "tab dropped"
            );
        }
    }

    pub(super) fn compare(&mut self, left: &Path, right: &Path, merge: bool) {
        compare::compare_files(self.wb, left, right);
        // Its differences, found on a thread.
        self.wait_for_viewers(Duration::from_secs(10));
        if merge {
            // Compose mode, the first difference taken from the right.
            self.wb.viewer_command("merge");
            self.wb.viewer_command("take_right");
        }
    }

    /// The mouse over the first place of `word` in the active file (its second character:
    /// inside the word, past a `$`).
    pub(super) fn hover_word(&mut self, word: &str) {
        use workbench_model::WorkbenchHost;
        let Some(text) = self.wb.active_file().map(|f| f.text.to_string()) else { return };
        let Some(at) = text.find(word).map(|i| i + 1) else {
            tracing::error!("`{word}` isn't in the active file");
            return;
        };
        workbench_model::EditorHost::select_active(self.wb, at..at);
        self.frame();
        let Some(caret) = self.wb.caret_rect(self.text) else { return };
        let p = Point::new(caret.min_x() + 1.0, caret.min_y() + caret.size.height / 2.0);
        // The caret elsewhere (its line not lit under the card), the view where it is.
        let end = text.len();
        workbench_model::EditorHost::select_active(self.wb, end..end);
        self.hover(p);
    }

    pub(super) fn hover(&mut self, p: Point) {
        // A paint first, so the hover areas (tabs etc.) exist.
        self.frame();
        self.wb.sync_syntax();
        self.frame();
        self.wb.hover(p);
        self.wb.update_error_tip(self.text, p);
        self.wb.update_code_hover(self.text, p);
        // Transitions (e.g. a preview appearing) finish before the capture.
        settle(600);
        self.frame();
        settle(200);
    }
}
