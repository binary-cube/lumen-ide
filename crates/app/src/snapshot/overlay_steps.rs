//! The menus, dialogs, side panels and the modules' own screens.

use super::*;

impl Steps<'_> {
    pub(super) fn level_picker(&mut self) {
        // The status bar must be painted first (the picker anchors to it).
        self.wb.sync_syntax();
        self.frame();
        self.wb.open_level_picker();
        self.frame();
        settle(200);
    }

    /// The "Copy" popover for the active file.
    pub(super) fn copy_menu(&mut self) {
        if let Some(i) = self.wb.active_tab()
            && let Some(path) = self.wb.tab_path(i)
        {
            self.wb.open_copy_menu(&path, Some(i), None);
        }
    }

    /// The active tab's context menu (our own).
    pub(super) fn tab_menu(&mut self) {
        let Some(i) = self.wb.active_tab() else { return };
        let state = crate::commands::TabMenuState {
            count: self.wb.tab_count(),
            has_path: true,
            in_project: true,
            pinned: false,
            can_reopen: true,
            locked: self.wb.tab_lock(i).map(|l| l != crate::workbench::LockState::Writable),
            renamable: false,
        };
        let items = crate::commands::tab_context_menu(
            (i, self.wb.tab_id(i).get()),
            state,
            &crate::commands::file_manager_label(platform_api::FileManager::Finder),
            Vec::new(),
        );
        self.wb.open_context_menu(items, kernel::Point::new(760.0, 130.0));
    }

    /// The menu button in the window's bar (the setting on), and the drawer as asked.
    pub(super) fn menu_drawer(&mut self, state: &str) {
        use ui_workbench::NavKey;
        crate::window_menu::set_asked(true);
        self.frame();
        if state == "closed" {
            return;
        }
        let keys: Vec<NavKey> = state
            .strip_prefix("keys:")
            .unwrap_or_default()
            .chars()
            .filter_map(|c| match c {
                'u' => Some(NavKey::Up),
                'd' => Some(NavKey::Down),
                'l' => Some(NavKey::Left),
                'r' => Some(NavKey::Right),
                _ => None,
            })
            .collect();
        self.wb.show_menu_drawer(&keys, state.strip_prefix("filter:"));
        self.frame();
        settle(200);
        self.frame();
    }

    pub(super) fn task_manager(&mut self, scenario: &str) {
        // The resources the status bar shows (the Task Manager reads them too).
        let sample = crate::resources::measure_now();
        let gpu = render_api::GpuMemory { shared: 20 * 1024 * 1024, window: 28 * 1024 * 1024 };
        self.wb.set_resources(sample, Some(gpu));
        // The whole tab as wide as the window lets it.
        if matches!(scenario, "wide" | "all" | "menu" | "stop" | "servers") {
            self.wb.close_left_panel();
        }
        self.wb.run_command(command_model::CommandId("tasks.open"));
        self.frame();
        if scenario != "1" {
            // Made up, the same every run: its parts, history and state, its clock stopped.
            self.wb.viewer_command(&format!("demo:{scenario}"));
            self.frame();
            // Stopping asked first: its question's modal opened, as the app runs what the
            // view asks for.
            if scenario == "stop" {
                self.wb.take_viewer_action(self.text);
                settle(220);
            }
            self.frame();
            // A row's right-click menu (the terminal's: it started programs).
            if scenario == "menu" {
                let at = Point::new(TASKS_MENU_AT.0, TASKS_MENU_AT.1);
                if let Some(entries) = self.wb.viewer_menu_at(at) {
                    self.wb.open_context_menu(entries, at);
                }
                self.frame();
                settle(220);
            }
            return;
        }
        // Two looks a second apart: the parts' CPU over that second.
        let deadline = Instant::now() + Duration::from_millis(2300);
        while Instant::now() < deadline {
            self.wb.poll_viewers();
            std::thread::sleep(Duration::from_millis(50));
        }
        self.frame();
    }

    /// A remote project's connection edited: made up in a scratch folder (no server is
    /// reached, no secret read), its form prefilled; `invalid`: a port out of range tried.
    pub(super) fn remote_edit(&mut self, state: &str) {
        let dir = std::env::temp_dir().join(format!("snapshot-remote-edit-{}", std::process::id()));
        let data = dir.join(kernel::brand::PROJECT_DIR);
        let toml = format!(
            "[[connection]]\nid = \"snapshot\"\nname = \"Shop (production)\"\nprotocol = \"ftps\"\n\
             host = \"ftp.example.com\"\nport = 21\nuser = \"deploy\"\nauth = \"password\"\n\
             root = \"/www/shop\"\nmode = \"mirror\"\nexcluded = [\".git\", \"vendor\"]\n\
             secrets = \"keychain\"\nlocal = {:?}\n",
            dir.display().to_string()
        );
        let written = std::fs::create_dir_all(&data)
            .and_then(|()| std::fs::write(data.join("remote.toml"), toml));
        if let Err(e) = written {
            tracing::warn!(error = %e, "the remote connection for the snapshot wasn't written");
            return;
        }
        remote_projects::edit_connection(self.wb, &dir);
        // Read: the scratch folder isn't needed any more.
        let _ = std::fs::remove_dir_all(&dir);
        self.frame();
        if state == "invalid" {
            let key = |this: &mut Self, key: EditKey| {
                this.wb.edit_key(this.text, key);
            };
            key(self, EditKey::Tab { shift: false });
            self.wb.viewer_command("select_all");
            self.wb.type_text(self.text, "99999");
            key(self, EditKey::Enter { shift: false });
        }
        self.frame();
    }

    /// The remote project wizard driven by its keys: the connection typed (1), the server's
    /// folders (2), the project's tree with `vendor` opened and left out (3).
    pub(super) fn remote_wizard(&mut self, step: u8) {
        let settle = |this: &mut Self| {
            this.frame();
            this.wait(Duration::from_secs(15), |wb| {
                wb.poll_viewers();
                !wb.viewer_busy()
            });
            this.frame();
        };
        let key = |this: &mut Self, key: EditKey| {
            this.wb.edit_key(this.text, key);
        };
        self.wb.run_command(command_model::CommandId("remote.open"));
        self.frame();
        for (i, typed) in ["127.0.0.1", "2222", "demo", "demo123"].into_iter().enumerate() {
            if i > 0 {
                key(self, EditKey::Tab { shift: false });
            }
            self.wb.type_text(self.text, typed);
        }
        if step < 2 {
            return;
        }
        // Connected; an unknown server trusted from its fingerprint (↵ again).
        key(self, EditKey::Enter { shift: false });
        settle(self);
        key(self, EditKey::Enter { shift: false });
        settle(self);
        if step < 3 {
            return;
        }
        // The project's folder typed, chosen.
        key(self, EditKey::Tab { shift: false });
        self.wb.viewer_command("select_all");
        self.wb.type_text(self.text, "/www/shop");
        key(self, EditKey::Enter { shift: false });
        settle(self);
        key(self, EditKey::Enter { shift: false });
        settle(self);
        // `vendor` (the 4th folder) opened and left out.
        let down = || EditKey::Move { motion: document::Motion::Down, extend: false };
        for _ in 0..4 {
            key(self, down());
        }
        key(self, EditKey::Move { motion: document::Motion::Right, extend: false });
        settle(self);
        self.wb.type_text(self.text, " ");
        self.frame();
    }

    /// `src/cart.ts` opened, answered by the fake language server: its diagnostics shown,
    /// and (`completion`) its suggestions after `console.`.
    pub(super) fn language_server(&mut self, case: &str) {
        let Some(root) = self.wb.project_root_path() else { return };
        // A file a real server answers (`REAL_LSP=1`): a key begun on its last line.
        if let Some((name, source)) = match case {
            "composer" => Some(("composer.json", "{\n  \"name\": \"a/b\",\n  \"desc\"\n}\n")),
            "package" => Some(("package.json", "{\n  \"name\": \"a\",\n  \"scr\"\n}\n")),
            "compose" => Some(("compose.yaml", "services:\n  web:\n    ima\n")),
            "dockerfile" => Some(("Dockerfile", "FROM alpine\nWORK\n")),
            "compose-env" => Some(("compose.yaml", "services:\n  web:\n    image: ${AP}\n")),
            "bundle" => Some(("config/packages/twig.yaml", "twig:\n    default\n")),
            "services" => Some(("config/services.yaml", "services:\n  App\\Mailer:\n    autow\n")),
            _ => None,
        } {
            self.real_server_completion(&root.join(name), source);
            return;
        }
        if let Some((name, source)) = match case {
            "twig" => Some(("page.html.twig", "<p>{{ name|up\n")),
            "blade" => Some(("page.blade.php", "<p>\n@fore\n")),
            _ => None,
        } {
            // A template's own suggestions, typed unfinished: no server answers them.
            let file = root.join(name);
            if std::fs::write(&file, source).is_err() || self.wb.open_file(&file).is_err() {
                tracing::error!("the template's snapshot file can't be made");
                return;
            }
            self.wb.sync_syntax();
            self.frame();
            let last = source.lines().count();
            self.wb.place_cursor_at_line_end(self.text, Some(last));
            self.wb.update_completion(self.text);
            self.frame();
            return;
        }
        let (file, source, last) = if case == "css" {
            let source = "/* The card, answered by a language server (the fake one). */\n.card {\n  color: rebeccapurple;\n  column-count: 2;\n  /* TODO: dark theme */\n  clip: rect(0 0 0 0);\n  col\n}\n";
            (root.join("src/card.css"), source, 7)
        } else if case == "cpp" {
            let source = "// Shapes, answered by a language server (the fake one).\nclass Shape {\npublic:\n  virtual double area() const = 0;\n};\n\nclass Circle : public Shape {\n  double r;\npublic:\n  double area() const override { return 3.14159 * r * r; }\n};\n";
            (root.join("src/shapes.cpp"), source, 4)
        } else if case == "usages" {
            let source = "// The cart's total, answered by a language server (the fake one).\nexport function cartTotal(prices: number[]): number {\n  return prices.reduce((sum, price) => sum + price, 0);\n}\n\nexport const amount = cartTotal([12, 30]);\n";
            (root.join("src/cart.ts"), source, 6)
        } else {
            let source = "// The cart's total, answered by a language server (the fake one).\nexport function total(prices: number[]): number {\n  // TODO: discounts\n  return prices.reduce((sum, price) => sum + price, 0);\n}\n\nconst amount = total([12, 30]);\n// FIXME: rounding\nconsole.\n";
            (root.join("src/cart.ts"), source, 9)
        };
        if case == "usages" {
            // A page calling the script's function: the built-in engine's find (the server
            // knows the scripts), joined with the server's places by Find Usages.
            let page = "<button id=\"pay\">Pay</button>\n<script>\n  document.querySelector('#pay').textContent = cartTotal([12, 30]);\n</script>\n";
            if std::fs::write(root.join("src/cart.html"), page).is_err() {
                tracing::error!("the language server's snapshot page can't be made");
            }
        }
        if std::fs::write(&file, source).is_err() || self.wb.open_file(&file).is_err() {
            tracing::error!("the language server's snapshot file can't be made");
            return;
        }
        self.frame();
        let ready = |wb: &mut Workbench| {
            wb.poll_servers();
            wb.served_at_cursor()
        };
        self.wait(Duration::from_secs(10), ready);
        self.frame();
        // Its diagnostics (published after it opened the file).
        let deadline = Instant::now() + Duration::from_secs(5);
        // (CSS: the fake's `TODO` warning and the built-in engine's deprecated `clip`.)
        while self.wb.mark_count() < 2 && Instant::now() < deadline {
            self.wb.poll_servers();
            self.wb.poll_diagnostics();
            self.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
        if case == "completion" || case == "css" {
            self.wb.place_cursor_at_line_end(self.text, Some(last));
            self.wb.update_completion(self.text);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.wb.completion_open() && Instant::now() < deadline {
                self.wb.poll_servers();
                self.frame();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.frame();
    }

    /// `file` made and opened, a real server's suggestions asked at its last key.
    fn real_server_completion(&mut self, file: &std::path::Path, source: &str) {
        let made = file.parent().is_some_and(|dir| std::fs::create_dir_all(dir).is_ok());
        if !made || std::fs::write(file, source).is_err() || self.wb.open_file(file).is_err() {
            tracing::error!("the server's snapshot file can't be made");
            return;
        }
        self.wb.sync_syntax();
        self.frame();
        let ready = |wb: &mut Workbench| {
            wb.poll_servers();
            wb.served_at_cursor()
        };
        self.wait(Duration::from_secs(30), ready);
        // The line holding the key: the last with text, before a closing bracket.
        let lines: Vec<&str> = source.lines().collect();
        let key = lines.iter().rposition(|l| !l.trim_start().starts_with('}')).unwrap_or(0);
        self.wb.place_cursor_at_line_end(self.text, Some(key + 1));
        if lines.get(key).is_some_and(|l| l.ends_with(['"', '}'])) {
            // Inside the key's quotes or braces.
            self.wb.edit_key(
                self.text,
                EditKey::Move { motion: document::Motion::Left, extend: false },
            );
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.wb.completion_open() && Instant::now() < deadline {
            self.wb.update_completion(self.text);
            self.wb.poll_servers();
            self.frame();
            std::thread::sleep(Duration::from_millis(100));
        }
        self.frame();
    }

    pub(super) fn color_picker(&mut self, line: u32) {
        // Shown as on macOS / Windows (no window here to ask the platform).
        ui_workbench::ColorPicker::enable_eyedropper(true);
        self.wb.sync_syntax();
        self.frame();
        self.wb.open_color_picker_line(line);
        self.frame();
        settle(200);
    }

    /// The app ending: made-up programs being stopped, some ended, one made to, the rest
    /// still asked (`many`: more than the list shows, so it scrolls).
    pub(super) fn ending(&mut self, many: bool) {
        use workbench_model::running::{
            Closing, ProgramState, RunningPrompt, Stopping, StoppingProgram,
        };
        let names: &[&str] = if many {
            &[
                "zsh",
                "claude",
                "node",
                "node",
                "caffeinate",
                "intelephense",
                "zsh",
                "npm",
                "node",
                "php",
                "zsh",
                "git",
            ]
        } else {
            &["zsh", "claude", "node", "node", "caffeinate"]
        };
        let programs = names
            .iter()
            .enumerate()
            .map(|(i, name)| StoppingProgram {
                name: (*name).to_owned(),
                pid: 20146 + u32::try_from(i * 11).unwrap_or(0),
                state: match i {
                    0 | 2 => ProgramState::Ended,
                    4 => ProgramState::Forced,
                    _ => ProgramState::Asked,
                },
            })
            .collect();
        let prompt = RunningPrompt {
            closing: Closing::App,
            groups: Vec::new(),
            projects: Vec::new(),
            modules: Vec::new(),
            waiting: false,
            stopping: Some(Stopping { programs }),
        };
        if !self.wb.open_ending_prompt(&prompt) {
            tracing::error!("the ending's window didn't open");
        }
        self.frame();
        settle(300);
        self.frame();
    }

    pub(super) fn side_panel(&mut self, panel: &str) {
        // The side panel (e.g. `structure`), with usages searched in the background.
        self.wb.toggle_side_panel(panel);
        let timeout = Duration::from_secs(5);
        self.wait(timeout, Workbench::poll_index);
        self.wb.sync_syntax();
        self.frame();
        // The document's symbols come from their thread.
        self.wait(timeout, Workbench::poll_symbols);
        self.frame();
        // The panel's usages, from its own thread.
        self.wait(timeout, Workbench::poll_side_panel);
        self.frame();
        // A file's declarations asked of its server (a language without a structure of
        // ours), answered.
        let deadline = Instant::now() + timeout;
        while self.wb.outline_asked() && Instant::now() < deadline {
            self.wb.poll_servers();
            self.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
        self.frame();
    }

    /// The Services window at the bottom: empty, or with the sample contributor (its console
    /// shown, its Output's log at a stopped clock).
    pub(super) fn services(&mut self, kind: &str) {
        use services_panel::sample;
        if kind == "fake" {
            ui_workbench::services::register(std::sync::Arc::new(sample::SampleProvider));
        }
        self.frame();
        self.wb.toggle_tool(services_panel::TOOL);
        self.frame();
        if kind == "fake" {
            let reveal = services_panel::panel::Reveal {
                provider: sample::ID,
                node: sample::CONSOLE,
                tab: Some(sample::OUTPUT),
            };
            <Workbench as workbench_model::ViewHost>::tell_panel(
                self.wb,
                services_panel::TOOL,
                &reveal,
                false,
            );
        }
        self.frame();
        self.frame();
    }

    pub(super) fn dialog(&mut self, rename: bool) {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        self.wb.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "file-pen",
            title: i18n::t("dialog.unsaved_title"),
            message: i18n::tr!("dialog.unsaved_message", name = "**index.php**"),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("discard", i18n::t("dialog.discard"), ButtonKind::Destructive),
                DialogButton::new("save", i18n::t("dialog.save"), ButtonKind::Primary),
            ],
            cancel: "cancel",
            input: rename.then(|| ("index.php".to_owned(), 0..5)),
            check: None,
            list: Vec::new(),
        });
        self.frame();
        settle(200);
    }

    /// The question before quitting while work still runs (made-up work: a program in the
    /// terminal, an upload half done, a query, a push, a server's download); `waiting`: the
    /// user chose to wait.
    pub(super) fn running_prompt(&mut self, waiting: bool) {
        use workbench_model::StopImpact::{Interrupts, LosesWork};
        use workbench_model::running::{Closing, StopVia, WorkKind, WorkProgress};
        use workbench_model::{ActionTarget, ActivityKind, ActivityUnit, PartRef, RunningWork};
        let project = self.wb.project_identity().map(|(id, _)| id);
        let other = kernel::ProjectId::new();
        let stop = |owner: &str| {
            Some(StopVia {
                part: PartRef { owner: owner.into(), name: String::new(), project: None },
                action: "stop",
                target: ActionTarget::Part,
            })
        };
        let started = |secs| std::time::Instant::now().checked_sub(Duration::from_secs(secs));
        let mut program =
            RunningWork::new(WorkKind::Program, "npm", "terminal", project, LosesWork);
        program.detail = "npm run dev".into();
        program.since = started(1260);
        program.stop = stop("terminal");
        let mut upload = RunningWork::new(
            WorkKind::Operation(ActivityKind::Upload),
            "public/build/app.js",
            "remote.projects",
            project,
            LosesWork,
        );
        upload.detail = "Remote: shop".into();
        upload.progress = Some(WorkProgress {
            done: 3_100_000,
            total: Some(4_800_000),
            unit: ActivityUnit::Bytes,
        });
        upload.since = started(14);
        upload.stop = stop("remote.projects");
        let mut query = RunningWork::new(
            WorkKind::Query,
            "reports.sql",
            "database.console",
            project,
            LosesWork,
        );
        query.detail = "shop_production".into();
        query.since = started(48);
        query.stop = stop("database.console");
        let mut push =
            RunningWork::new(WorkKind::Network, "Pushing…", "vcs.git", Some(other), LosesWork);
        push.detail = "/work/clients/blog".into();
        push.since = started(6);
        let mut download = RunningWork::new(
            WorkKind::Operation(ActivityKind::Download),
            "phpactor",
            "lsp",
            None,
            Interrupts,
        );
        download.progress = Some(WorkProgress {
            done: 9_000_000,
            total: Some(21_000_000),
            unit: ActivityUnit::Bytes,
        });
        download.since = started(20);
        download.stop = stop("lsp");
        let scope = workbench_model::CloseScope::quit();
        let items =
            workbench_model::running::to_ask([program, upload, query, push, download], &scope);
        let others = [(other, "blog".to_owned())];
        let mut prompt = self.wb.running_prompt(Closing::App, items, false, &others);
        self.wb.open_running_prompt(&prompt);
        if waiting {
            prompt.waiting = true;
            self.wb.update_running_prompt(&prompt);
        }
        self.frame();
        settle(200);
    }

    /// A recent project's "Delete…" dialog, its checkbox checked (the danger look).
    pub(super) fn delete_dialog(&mut self) {
        let path = Path::new("/work/clients/shop");
        self.wb.open_dialog(crate::workbench::delete_project_dialog(path, false));
        self.wb.toggle_dialog_check();
        self.frame();
        settle(200);
    }

    /// The question before deleting many files on the server: their common folder once,
    /// then each in a list that scrolls.
    pub(super) fn delete_many_dialog(&mut self) {
        let folder = "admin/control/api/components/ai/analyze/image-studio";
        let names =
            ["README.md", "api.php", "bootstrap.php", "config/config.php", "config/defaults.php"];
        let list: Vec<String> = (0..33)
            .map(|i| match names.get(i) {
                Some(name) => format!("{folder}/{name}"),
                None => format!("{folder}/src/Api/Handler{i}.php"),
            })
            .collect();
        let question = workbench_model::Question {
            title: i18n::t("remote.remove_title"),
            message: i18n::tr!("remote.remove_many", count = list.len()),
            icon: "trash",
            yes: i18n::t("remote.remove_yes"),
            danger: true,
            list,
        };
        <Workbench as workbench_model::WorkbenchHost>::confirm(self.wb, question, Box::new(|_| {}));
        self.frame();
        self.wb.scroll_dialog_list(3.0);
        self.frame();
        settle(200);
    }

    /// The projects list with a sample history in groups (one collapsed, one nested) and,
    /// `menu`, a project's right-click menu, or, `drag`, a project dragged above another
    /// (the line where it would go).
    pub(super) fn project_groups(&mut self, kind: &str) {
        use workspace::recent::{Place, RecentProjects};
        let paths = ["/work/clients/shop", "/work/clients/blog", "/work/tools/cli", "/work/site"];
        let mut model = RecentProjects::of(paths.iter().map(PathBuf::from).collect());
        let clients = model.new_group("Clients", None);
        let northwind = model.new_group("Northwind", Some(clients));
        let tools = model.new_group("Tools", None);
        model.move_to(Path::new(paths[0]), northwind, Place::Anywhere);
        model.move_to(Path::new(paths[1]), clients, Place::Anywhere);
        model.move_to(Path::new(paths[2]), tools, Place::Anywhere);
        model.set_collapsed(tools, true);
        let projects = paths.iter().map(|p| crate::recent::describe(Path::new(p))).collect();
        self.wb.show_recent(ui_workbench::RecentList { model, projects, sessions: Vec::new() });
        self.frame();
        settle(220);
        self.frame();
        // The history scrolled into view (below the actions and the current project).
        let middle = Point::new(SIZE.width as f32 * 0.5, SIZE.height as f32 * 0.6);
        self.wb.scroll(middle, 0.0, 600.0);
        self.frame();
        if kind == "drag"
            && let (Some(from), Some(to)) = (
                self.wb.projects_row(Path::new(paths[1])),
                self.wb.projects_row(Path::new(paths[0])),
            )
        {
            let press = Point::new(from.min_x() + 80.0, from.min_y() + from.size.height / 2.0);
            self.wb.mouse_down(self.text, press, false, false, false, 1);
            let over = Point::new(to.min_x() + 160.0, to.min_y() + to.size.height * 0.3);
            self.wb.mouse_drag(self.text, Point::new(press.x, press.y - 20.0));
            self.wb.mouse_drag(self.text, over);
            self.frame();
        }
        if kind == "menu"
            && let Some(row) = self.wb.projects_row(Path::new(paths[1]))
        {
            let at =
                Point::new(row.min_x() + row.size.width * 0.4, row.min_y() + row.size.height / 2.0);
            if let Some(entries) = self.wb.projects_menu_at(at) {
                self.wb.open_context_menu(entries, at);
            }
            self.frame();
            settle(200);
        }
    }

    /// Notifications shown (see `TOASTS` in the module's doc), as the modules tell them;
    /// their time stopped a third of the way (the same picture every run).
    pub(super) fn toasts(&mut self, kind: &str) {
        use workbench_model::{Notification, WorkbenchHost};
        let shown = match kind {
            "modal" => vec![
                Notification::success(i18n::t("datasources.applied")),
                Notification::success(i18n::t("datasources.applied")),
                Notification::info(i18n::tr!("datasources.moved_keychain", name = "prod")),
            ],
            _ => vec![
                Notification::info(i18n::tr!("files.saved_as_utf8", name = "Shop.php", from = "Windows-1252")),
                Notification::warning(i18n::tr!("files.deleted", count = 3))
                    .with_action(crate::commands::label(crate::commands::UNDO), crate::commands::UNDO)
                    .with_action(
                        crate::commands::label(crate::commands::REVEAL_IN_PROJECT),
                        crate::commands::REVEAL_IN_PROJECT,
                    ),
                Notification::error(i18n::tr!(
                    "database.export_failed",
                    why = "the server closed the connection while the rows were being written, after three attempts"
                ))
                .with_detail("orders.csv · 12 480 rows"),
                Notification::progress(i18n::t("git.pushing")),
            ],
        };
        for notification in shown {
            self.wb.notify(notification);
        }
        self.wb.settle_notifications();
        self.frame();
    }

    /// A navigation's list at the cursor (`implementations`, `hierarchy`), or the code
    /// actions' (`actions`: the bulb worked out first, the index built).
    pub(super) fn related(&mut self, kind: &str) {
        use crate::workbench::Related;
        if kind == "actions" {
            self.wait(Duration::from_secs(20), |wb| !wb.indexing());
            self.frame();
            settle(400);
            self.frame();
            self.wb.show_context_actions(self.text);
            self.frame();
            settle(250);
            return;
        }
        if kind == "definition" {
            self.frame();
            self.wb.goto_definition(self.text);
            self.frame();
            settle(250);
            return;
        }
        let kind = match kind {
            "implementations" => Related::Implementations,
            "types" => Related::TypeDefinition,
            _ => Related::Hierarchy,
        };
        self.frame();
        self.wb.go_to_related(self.text, kind);
        self.frame();
        settle(250);
    }

    /// The extractors' templates (`EXTRACTOR=template`): "Go to Extractors Directory" makes
    /// the project's folder with the examples (the script removes it after), the Python one
    /// shown, its tags colored.
    pub(super) fn extractor_template(&mut self, kind: &str) {
        let _ = kind;
        self.wb.run_command(data_extractors::OPEN_DIRECTORY);
        self.settle_console();
        let Some(root) = self.wb.project_root() else { return };
        let path = data_extractors::templates::own_dir(&root).join("Python-DataFrame.extractor");
        if let Err(e) = self.wb.open_file(&path) {
            tracing::error!(error = %e, "the sample template didn't open");
            return;
        }
        self.wb.sync_syntax();
        self.settle_console();
    }
}
