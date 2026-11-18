//! The bar above a `.http` file's editor: Run All (every request, in
//! order), Stop, "Run with:" its environment (the files beside it listed; one added to the
//! public or the private file); each request's ▶ in the gutter (its latest run's state after
//! it ran: running, done, failed), the request at the cursor marked beside its lines.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, radius};
use ui_components::editor_toolbar::{
    EditorState, EditorToolbar, StatementMark, ToolbarAction, ToolbarPaint,
};
use ui_components::{Tip, ViewMenuEntry, ViewerAction, ViewerCursor, WithHost};
use ui_kit::{draw_icon, draw_text};

use crate::runs::Runs;

/// The bar's height (logical pixels).
const HEIGHT: f32 = 34.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    RunAll,
    Stop,
    Environment,
    Graph,
}

/// The environments' names of the file's folder, as last read (in the background).
type Names = Arc<Mutex<Option<Vec<String>>>>;

/// A text up to this size is read on the UI thread at each edit (microseconds); a bigger one
/// (a body of megabytes written inline) on a worker.
const PARSED_HERE: usize = 128 * 1024;

pub struct HttpBar {
    runs: Arc<Runs>,
    file: PathBuf,
    root: Option<PathBuf>,
    wake: Arc<dyn Fn() + Send + Sync>,
    editor: Option<EditorState>,
    /// The editor's text read (once per version: a cursor moved reads nothing again), the
    /// text and version it was read from (a big file's is read on a worker: the one before
    /// serves meanwhile).
    parsed: Arc<http_requests::HttpFile>,
    parsed_text: Arc<str>,
    parsed_version: u64,
    parsing: crate::pending::Pending<(u64, Arc<str>, Arc<http_requests::HttpFile>)>,
    marks: Vec<StatementMark>,
    /// What [`Self::marks`] were made for: the runs' generation, the text's version.
    marks_for: (u64, u64),
    marks_generation: u64,
    current: Option<Range<usize>>,
    names: Names,
    hits: Vec<(Rect, Tool)>,
    hover: Option<Tool>,
    action: Option<ToolbarAction>,
}

impl std::fmt::Debug for HttpBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpBar").field("file", &self.file).finish_non_exhaustive()
    }
}

impl HttpBar {
    pub fn new(
        runs: Arc<Runs>,
        file: &Path,
        root: Option<&Path>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        runs.follow(&wake);
        let bar = Self {
            runs,
            file: file.to_path_buf(),
            root: root.map(Path::to_path_buf),
            wake,
            editor: None,
            parsed: Arc::default(),
            parsed_text: Arc::from(""),
            parsed_version: u64::MAX,
            parsing: crate::pending::Pending::default(),
            marks: Vec::new(),
            marks_for: (u64::MAX, u64::MAX),
            marks_generation: 0,
            current: None,
            names: Arc::default(),
            hits: Vec::new(),
            hover: None,
            action: None,
        };
        bar.read_names();
        bar
    }

    /// The environments' names read again, in the background (the app woken after).
    fn read_names(&self) {
        let (names, file, root, wake) =
            (Arc::clone(&self.names), self.file.clone(), self.root.clone(), Arc::clone(&self.wake));
        kernel::work::run(crate::job::THREAD, move || {
            let environments = http_environments::of(&file, root.as_deref());
            let read: Vec<String> = environments.names().map(str::to_owned).collect();
            *names.lock().unwrap_or_else(PoisonError::into_inner) = Some(read);
            wake();
        });
    }

    /// What was read from `text` (version `version`), taken.
    fn take_parsed(&mut self, version: u64, text: Arc<str>, parsed: http_requests::HttpFile) {
        self.parsed = Arc::new(parsed);
        self.parsed_text = text;
        self.parsed_version = version;
    }

    /// The request at the editor's cursor, from what was read (of its text, or of the one
    /// before while a big file's is read: never cut where the text doesn't have it).
    fn find_current(&mut self) {
        let Some(state) = &self.editor else { return };
        self.current = self.parsed.request_at(state.cursor).and_then(|r| {
            let start = r.line_range.start;
            let body = state.text.get(start..r.range.end.min(state.text.len()))?;
            Some(start..start + body.trim_end().len())
        });
    }

    fn text(&self) -> Option<Arc<str>> {
        self.editor.as_ref().map(|e| Arc::clone(&e.text))
    }

    /// The marks made again when the runs or the text changed.
    fn refresh_marks(&mut self) -> bool {
        if self.editor.is_none() {
            return false;
        }
        // Made from the text that was read (a big file's newer text: once it's read).
        let key = (self.runs.generation(), self.parsed_version);
        if key == self.marks_for {
            return false;
        }
        self.marks_for = key;
        let marks = self.runs.marks(&self.file, &self.parsed, &self.parsed_text);
        if marks == self.marks {
            return false;
        }
        self.marks = marks;
        self.marks_generation += 1;
        true
    }

    /// A run started: the Services window shows it.
    fn started(&mut self) {
        self.action = Some(ToolbarAction::View(ViewerAction::WithHost(WithHost::once(|host| {
            if workbench_model::presence::command(workbench_model::TOGGLE_SERVICES) {
                host.show_side_panel(workbench_model::SERVICES_TOOL);
            }
        }))));
        self.refresh_marks();
    }

    fn run_at(&mut self, offset: usize) -> bool {
        let Some(text) = self.text() else { return false };
        let ran = crate::job::run_at(&self.runs, &self.file, &text, offset);
        if ran {
            self.started();
        }
        ran
    }

    fn run_all(&mut self) -> bool {
        let Some(text) = self.text() else { return false };
        let ran = crate::job::run_all(&self.runs, &self.file, &text);
        if ran {
            self.started();
        }
        ran
    }

    fn environment_label(&self) -> String {
        self.runs.environment(&self.file).unwrap_or_else(|| t("http.no_environment"))
    }

    fn environment_menu(&self) -> Vec<ViewMenuEntry> {
        let chosen = self.runs.environment(&self.file);
        let check =
            |entry: ViewMenuEntry, on: bool| if on { entry.with_icon("check") } else { entry };
        let mut entries =
            vec![check(ViewMenuEntry::item("env:", t("http.no_environment")), chosen.is_none())];
        let names =
            self.names.lock().unwrap_or_else(PoisonError::into_inner).clone().unwrap_or_default();
        for name in names {
            let on = chosen.as_deref() == Some(name.as_str());
            entries.push(check(ViewMenuEntry::item(format!("env:{name}"), name), on));
        }
        entries.push(ViewMenuEntry::Separator);
        entries.push(ViewMenuEntry::item("env-add:public", t("http.add_public_env")));
        entries.push(ViewMenuEntry::item("env-add:private", t("http.add_private_env")));
        entries.push(
            ViewMenuEntry::item("env-edit", t("http.edit_environments")).with_icon("file-json"),
        );
        entries
    }

    /// The environment file opened in the editor (made with an example first if it isn't
    /// there).
    fn add_environment(&mut self, private: bool) {
        let name = if private { http_requests::env::PRIVATE } else { http_requests::env::PUBLIC };
        let wake = Arc::clone(&self.wake);
        let (file, root) = (self.file.clone(), self.root.clone());
        self.action =
            Some(ToolbarAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
                host.spawn(Box::new(move || {
                    // Where it goes is looked for here, in the background (it reads the disk).
                    let folder = http_environments::folder_for(&file, root.as_deref());
                    let made = folder.join(name);
                    let opened = made.clone();
                    if !made.exists() {
                        let example = if private {
                            "{\n  \"dev\": {\n    \"token\": \"\"\n  }\n}\n"
                        } else {
                            "{\n  \"dev\": {\n    \"host\": \"localhost\"\n  }\n}\n"
                        };
                        let _ = files::write_atomic(&made, example.as_bytes());
                    }
                    // The user's own file never committed.
                    let ignored = private && root.as_deref().is_some_and(crate::gitignore::ensure);
                    wake();
                    Box::new(move |host: &mut dyn workbench_model::WorkbenchHost| {
                        host.open_file(&opened);
                        if ignored {
                            host.notify(workbench_model::Notification::info(t("http.ignored")));
                        }
                    })
                }));
            }))));
    }

    fn press(&mut self, tool: Tool, anchor: Rect) -> Option<ToolbarAction> {
        match tool {
            Tool::RunAll => {
                self.run_all();
            }
            Tool::Stop => {
                self.runs.stop(&self.file);
            }
            Tool::Environment => {
                self.read_names();
                return Some(ToolbarAction::Menu { anchor, entries: self.environment_menu() });
            }
            Tool::Graph => self.show_graph(),
        }
        self.action.take()
    }

    /// The file's requests as a graph, in a tab.
    fn show_graph(&mut self) {
        let Some(text) = self.text() else { return };
        let data = crate::chain_view::ChainData {
            runs: Arc::clone(&self.runs),
            file: self.file.clone(),
            text,
        };
        let name =
            self.file.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.action =
            Some(ToolbarAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
                let title = i18n::tr!("http.chain_title", file = name.as_str());
                host.open_view(&title, crate::chain_view::EXTENSION, &data);
            }))));
    }

    fn enabled(&self, tool: Tool) -> bool {
        match tool {
            Tool::Stop => self.runs.running(&self.file),
            _ => true,
        }
    }
}

impl EditorToolbar for HttpBar {
    fn height(&self) -> f32 {
        HEIGHT
    }

    fn paint(&mut self, ctx: ToolbarPaint<'_>) {
        let ToolbarPaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        ds::divider(layer, &ds, b.min_x(), b.max_x(), b.max_y() - ds.line());
        self.hits.clear();
        let mid = b.min_y() + b.size.height / 2.0;
        let size = px(26.0);
        let mut x = b.min_x() + px(8.0);
        for (tool, icon, tint) in
            [(Tool::RunAll, "play", Some(p.vcs_added)), (Tool::Stop, "square", Some(p.danger))]
        {
            let r = Rect::new(x, (mid - size / 2.0).round(), size, size);
            let enabled = self.enabled(tool);
            if enabled && self.hover == Some(tool) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(px(radius::SM)));
            }
            if let Some(id) = icons.id(icon) {
                let s = px(15.0);
                let ir = Rect::new(
                    (r.min_x() + (size - s) / 2.0).round(),
                    (r.min_y() + (size - s) / 2.0).round(),
                    s,
                    s,
                );
                let color =
                    if enabled { tint.unwrap_or(p.text) } else { p.text_muted.with_alpha(0.4) };
                draw_icon(layer, icons, id, ir, color);
            }
            self.hits.push((r, tool));
            x = r.max_x() + px(2.0);
        }
        ds::divider_vertical(layer, &ds, (x + px(6.0)).round(), mid - px(9.0), mid + px(9.0));
        x += px(14.0);
        let font = ds.ui_font(-1.0);
        let label = text.shape_line(&t("http.run_with"), &font);
        draw_text(
            layer,
            &label,
            x.round(),
            ds::optical_baseline(b.min_y(), b.size.height, &font),
            p.text_muted,
        );
        x += label.width + px(8.0);
        let value = self.environment_label();
        let value_w = text.shape_line(&value, &font.clone().with_weight(Font::NORMAL)).width;
        let field = Rect::new(x, (mid - px(13.0)).round(), (value_w + px(44.0)).ceil(), px(26.0));
        let spec = ds::DropdownSpec {
            value: &value,
            open: false,
            hovered: self.hover == Some(Tool::Environment),
            enabled: true,
            note: None,
            placeholder: None,
        };
        ds::dropdown(layer, text, icons, &ds, field, spec);
        self.hits.push((field, Tool::Environment));
        // The chain as a graph, at the bar's right end.
        let graph = Rect::new(b.max_x() - px(8.0) - size, (mid - size / 2.0).round(), size, size);
        if self.hover == Some(Tool::Graph) {
            layer.quad(Quad::filled(graph, p.text.with_alpha(0.08)).rounded(px(radius::SM)));
        }
        if let Some(id) = icons.id("graph") {
            let s = px(15.0);
            let ir = Rect::new(
                (graph.min_x() + (size - s) / 2.0).round(),
                (graph.min_y() + (size - s) / 2.0).round(),
                s,
                s,
            );
            draw_icon(layer, icons, id, ir, p.text.with_alpha(0.86));
        }
        self.hits.push((graph, Tool::Graph));
    }

    fn pointer_down(&mut self, p: Point) -> Option<ToolbarAction> {
        let (rect, tool) = self.hits.iter().find(|(r, _)| r.contains(p)).copied()?;
        if !self.enabled(tool) {
            return None;
        }
        self.press(tool, rect)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let tool = p.and_then(|p| self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t));
        let changed = tool != self.hover;
        self.hover = tool;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let tool = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t)?;
        self.enabled(tool).then_some(ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (rect, tool) = self.hits.iter().find(|(r, _)| r.contains(p))?;
        let label = match tool {
            Tool::RunAll => t("http.run_all"),
            Tool::Stop => t("http.stop"),
            Tool::Environment => t("http.environment_tip"),
            Tool::Graph => t("http.show_graph"),
        };
        Some(Tip::new(*rect, label))
    }

    fn poll(&mut self) -> bool {
        // A big file's text read on its worker.
        let mut changed = false;
        if let Some((version, text, parsed)) = self.parsing.take() {
            self.parsed = parsed;
            self.parsed_text = text;
            self.parsed_version = version;
            let before = self.current.clone();
            self.find_current();
            changed = self.current != before;
        }
        self.refresh_marks() | changed
    }

    fn icon(&self) -> Option<&'static str> {
        Some(crate::services::ICON)
    }

    fn command(&mut self, id: &str) -> bool {
        if let Some(name) = id.strip_prefix("env:") {
            let chosen = (!name.is_empty()).then(|| name.to_owned());
            self.runs.choose_environment(&self.file, chosen);
            return true;
        }
        match id {
            "env-add:public" => self.add_environment(false),
            "env-add:private" => self.add_environment(true),
            "env-edit" => {
                let data =
                    crate::env_view::EnvData { file: self.file.clone(), root: self.root.clone() };
                self.action = Some(ToolbarAction::View(ViewerAction::OpenView(
                    ui_components::OpenView::new(
                        t("http.edit_environments"),
                        crate::env_view::EXTENSION,
                        data,
                    ),
                )));
            }
            "run" => {
                let at = self.editor.as_ref().map_or(0, |e| e.cursor);
                return self.run_at(at);
            }
            "run-all" => return self.run_all(),
            "stop" => return self.runs.stop(&self.file),
            "graph" => self.show_graph(),
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ToolbarAction> {
        // An OAuth sign-in's page, asked for by a run.
        if let Some(asked) = self.runs.take_asked() {
            return Some(ToolbarAction::View(asked));
        }
        self.action.take()
    }

    fn follows_editor(&self) -> bool {
        // The request at the cursor is marked beside its lines; the marks follow the text.
        true
    }

    fn sync_editor(&mut self, state: &EditorState) {
        if self.editor.as_ref().is_none_or(|e| e.version != state.version) {
            if state.text.len() <= PARSED_HERE {
                self.parsing = crate::pending::Pending::default();
                self.take_parsed(
                    state.version,
                    Arc::clone(&state.text),
                    http_requests::parse(&state.text),
                );
            } else {
                let (version, text) = (state.version, Arc::clone(&state.text));
                self.parsing.set_waker(&Some(Arc::clone(&self.wake)));
                self.parsing.start(crate::response_view::THREAD, move || {
                    let parsed = Arc::new(http_requests::parse(&text));
                    (version, text, parsed)
                });
            }
        }
        self.editor = Some(state.clone());
        self.find_current();
        self.refresh_marks();
    }

    fn marks(&self) -> (&[StatementMark], u64) {
        (&self.marks, self.marks_generation)
    }

    fn mark_clicked(&mut self, at: usize) -> bool {
        self.run_at(at)
    }

    fn current(&self) -> Option<Range<usize>> {
        self.current.clone()
    }
}

#[cfg(test)]
#[path = "../tests/unit/bar.rs"]
mod tests;
