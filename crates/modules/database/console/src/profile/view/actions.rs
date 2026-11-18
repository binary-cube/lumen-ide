//! What the profile's bar and menus do.

use super::*;

impl ProfileView {
    pub(super) fn copy(&mut self, text: String, said: &str) {
        self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
        self.notifications.push(Notification::success(i18n::t(said)));
    }

    /// Finding `i`'s SQL put at the console's end (selected, never run).
    pub(super) fn insert(&mut self, i: usize) {
        let Some(sql) = self
            .shown_findings()
            .get(i)
            .and_then(|f| f.suggestion.as_ref()?.sql().map(str::to_owned))
        else {
            return;
        };
        let Some(file) = self.runner.as_ref().and_then(Runner::file) else { return };
        self.action = Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            crate::profile::insert_into(host, &file, &sql);
        })));
    }

    pub(super) fn export(&mut self, kind: ExportKind) {
        let Some(report) = self.report().cloned() else { return };
        match kind {
            ExportKind::Text => self.copy(export::text(&report), "profile.view.copied_text"),
            ExportKind::EngineJson => match export::engine_json(&report) {
                Some(json) => self.copy(json, "profile.view.copied_engine"),
                None => self.notifications.push(Notification::info(i18n::t("profile.view.no_raw"))),
            },
            ExportKind::Markdown => {
                let statement = self.set.as_ref().map(|s| s.statement.clone()).unwrap_or_default();
                let findings = self.shown_findings();
                let md = export::markdown(&report, &statement, &findings);
                self.copy(md, "profile.view.copied_markdown");
            }
            ExportKind::Json => {
                let name =
                    export::file_name(self.set.as_ref().map_or("profile", |s| s.title.as_str()));
                let start = self
                    .runner
                    .as_ref()
                    .and_then(Runner::file)
                    .and_then(|f| f.parent().map(PathBuf::from));
                self.action = Some(ViewerAction::Request(AppRequest::PickSavePath { start, name }));
            }
        }
    }

    /// Our JSON written to `path` in the background (told when done).
    pub(super) fn write_json(&mut self, path: PathBuf) {
        let Some(report) = self.report().cloned() else { return };
        let pending: Pending = Arc::default();
        let slot = Arc::clone(&pending);
        let job = move || {
            let json = export::json(&report);
            let result = std::fs::write(&path, json).map(|()| path).map_err(|e| e.to_string());
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        };
        match &self.runner {
            Some(runner) => runner.runtime().spawn(job),
            None => job(),
        }
        self.export = Some(pending);
    }

    pub(super) fn open_in_tab(&mut self) {
        let (Some(runner), Some(set)) = (self.runner.clone(), self.set.clone()) else { return };
        let title = self.tab_title();
        let data = ProfileTabData { runner, set };
        self.action = Some(ViewerAction::OpenView(ui_components::OpenView::new(
            title,
            crate::profile::VIEW,
            data,
        )));
    }

    pub(super) fn set_filter(&mut self, text: String) {
        self.filter = text;
        self.table.set_filter(&self.filter);
    }

    pub(super) fn open_dropdown(
        &mut self,
        anchor: Rect,
        items: Vec<(String, String, &'static str, bool)>,
    ) {
        self.dropdown = Some(Dropdown { anchor, items, rect: Rect::default() });
    }

    pub(super) fn export_items(&self) -> Vec<(String, String, &'static str, bool)> {
        let report = self.report();
        ExportKind::ALL
            .into_iter()
            .map(|k| {
                let icon = match k {
                    ExportKind::Json => "download",
                    _ => "copy",
                };
                (
                    format!("profile.export:{}", k.id()),
                    k.label(),
                    icon,
                    report.is_some_and(|r| k.available(r)),
                )
            })
            .collect()
    }

    pub(super) fn mode_items(&self) -> Vec<(String, String, &'static str, bool)> {
        ProfileMode::ALL
            .into_iter()
            .map(|m| (format!("profile.mode:{}", m.id()), m.label(), m.icon(), m.available()))
            .collect()
    }

    pub(super) fn more_items(&self) -> Vec<(String, String, &'static str, bool)> {
        let mut items = vec![
            (
                "profile.expand_all".to_owned(),
                i18n::t("console.plan_expand_all"),
                "chevrons-expand",
                true,
            ),
            (
                "profile.collapse_all".to_owned(),
                i18n::t("console.plan_collapse_all"),
                "chevrons-collapse",
                true,
            ),
        ];
        if matches!(self.source, Source::Services(_)) {
            items.push((
                "profile.open_in_tab".to_owned(),
                i18n::t("profile.view.open_in_tab"),
                "arrow-up-right",
                true,
            ));
        }
        items
    }

    pub(super) fn press_hit(&mut self, hit: Hit) {
        match hit {
            Hit::Mode(i) => {
                if let Some(&m) = ProfileMode::ALL.get(i) {
                    self.set_mode(m);
                }
            }
            Hit::ModeMenu | Hit::Export | Hit::More | Hit::Compare => {
                let anchor = self
                    .hits
                    .iter()
                    .find(|(_, h, _)| *h == hit)
                    .map(|(r, ..)| *r)
                    .unwrap_or_default();
                let items = match hit {
                    Hit::ModeMenu => self.mode_items(),
                    Hit::Export => self.export_items(),
                    Hit::Compare => self.compare_items(),
                    _ => self.more_items(),
                };
                self.open_dropdown(anchor, items);
            }
            Hit::Filter => self.typing = true,
            Hit::FilterClear => {
                self.set_filter(String::new());
                self.typing = false;
            }
            Hit::ExpandAll => self.table.set_all_open(true),
            Hit::CollapseAll => self.table.set_all_open(false),
            Hit::OpenInTab => self.open_in_tab(),
            Hit::Drawer => self.drawer_open = !self.drawer_open,
            Hit::DrawerClose => self.drawer_open = false,
            Hit::Severity(l) => {
                self.severity = if self.severity == Some(l) { None } else { Some(l) };
                self.shown_generation += 1;
                if !self.laid.mode_wide {
                    self.drawer_open = true;
                }
            }
            Hit::Compact(i) => self.choose_finding(i),
            Hit::MenuItem(i) => {
                if let Some(id) = self
                    .dropdown
                    .take()
                    .and_then(|d| d.items.get(i).filter(|it| it.3).map(|it| it.0.clone()))
                {
                    self.command(&id);
                }
            }
            Hit::Statement
            | Hit::Depth
            | Hit::RolledBack
            | Hit::Engine
            | Hit::Pill(_)
            | Hit::TablesNote => {}
        }
    }

    pub(super) fn hit_at(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, ..)| r.contains(p)).map(|(_, h, _)| h.clone())
    }
}
