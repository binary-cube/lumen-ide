//! Copying through the data extractors, and the list choosing one.

use super::*;

impl<S: GridSource> DataGrid<S> {
    /// The extractor ⌘C writes with (the project's choice).
    pub fn extractor(&self) -> std::sync::Arc<dyn Extractor> {
        extractors::catalog().chosen(self.project)
    }

    /// The chosen extractor's name (read again only when the catalog changed).
    pub(in crate::data_grid) fn extractor_label(&mut self) -> String {
        let generation = extractors::catalog().generation();
        match &self.extract_label {
            Some((g, label)) if *g == generation => label.clone(),
            _ => {
                let label = self.extractor().name();
                self.extract_label = Some((generation, label.clone()));
                label
            }
        }
    }

    /// Whether the extractors' popover is open.
    pub fn extractors_open(&self) -> bool {
        self.extract_open
    }

    /// The popover's rows, grouped: the SQL ones and their skips, the
    /// separated formats and their configuration, the others, the user's templates.
    pub(in crate::data_grid) fn extractor_rows(&self) -> Vec<PopRow> {
        let catalog = extractors::catalog();
        let all = catalog.all(self.project);
        let mut rows = Vec::new();
        for group in extractors::Group::ALL {
            let members: Vec<_> = all.iter().filter(|e| e.group() == group).collect();
            // The separated formats and the templates show their command even when empty.
            if members.is_empty()
                && !matches!(group, extractors::Group::Delimited | extractors::Group::Scripted)
            {
                continue;
            }
            rows.push(PopRow::Heading(group.label()));
            rows.extend(members.iter().map(|e| PopRow::Choice {
                id: e.id().to_owned(),
                name: e.name(),
                problem: e.problem(),
            }));
            match group {
                extractors::Group::Sql => {
                    rows.push(PopRow::Toggle {
                        computed: true,
                        label: i18n::t("extract.skip_computed"),
                    });
                    rows.push(PopRow::Toggle {
                        computed: false,
                        label: i18n::t("extract.skip_generated"),
                    });
                }
                extractors::Group::Delimited => rows.push(PopRow::Action {
                    command: extractors::CONFIGURE_CSV_COMMAND,
                    label: i18n::t("extract.configure_csv"),
                }),
                extractors::Group::Scripted => rows.push(PopRow::Action {
                    command: extractors::OPEN_DIRECTORY_COMMAND,
                    label: i18n::t("extract.open_directory"),
                }),
                extractors::Group::Other => {}
            }
        }
        rows.push(PopRow::Note(i18n::t("extract.xlsx_note")));
        rows
    }

    /// Opens the extractors' popover (closes the others).
    pub fn open_extractors(&mut self) -> bool {
        self.extract_rows = self.extractor_rows();
        self.extract_scroll = 0.0;
        self.extract_open = true;
        self.sizes_open = false;
        self.more_open = false;
        true
    }

    /// Extractor `id` chosen for the project (kept: the extractors' module saves it).
    pub fn choose_extractor(&mut self, id: &str) -> bool {
        let catalog = extractors::catalog();
        if catalog.find(self.project, id).is_none_or(|e| e.problem().is_some()) {
            return false;
        }
        catalog.choose(self.project, id);
        self.extract_label = None;
        self.action = Some(GridAction::Command(extractors::SAVE_COMMAND.to_owned()));
        true
    }

    /// "Skip Computed Columns" (`computed`) or "Skip Generated Columns" switched.
    pub fn toggle_skip(&mut self, computed: bool) {
        let catalog = extractors::catalog();
        let mut options = catalog.options();
        if computed {
            options.skip_computed = !options.skip_computed;
        } else {
            options.skip_generated = !options.skip_generated;
        }
        catalog.set_options(options);
        self.action = Some(GridAction::Command(extractors::SAVE_COMMAND.to_owned()));
    }

    /// A row of the popover used: an extractor chosen (closing it), a skip switched (it
    /// stays), a command run (closing it).
    pub(super) fn use_extractor_row(&mut self, at: usize) {
        match self.extract_rows.get(at).cloned() {
            Some(PopRow::Choice { id, problem: None, .. }) => {
                self.extract_open = false;
                self.choose_extractor(&id);
            }
            Some(PopRow::Toggle { computed, .. }) => self.toggle_skip(computed),
            Some(PopRow::Action { command, .. }) => {
                self.extract_open = false;
                self.action = Some(GridAction::Command(command.to_owned()));
            }
            _ => {}
        }
    }
}
