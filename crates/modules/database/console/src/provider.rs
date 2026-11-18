//! The bar above a console's editor: offered for the files of a project's consoles'
//! folder (`consoles/<source>/<name>.sql` in its own folder), and above the project's own
//! SQL files (39n: "No data source — Attach…", then the console's bar on the source it's
//! attached to — [`crate::file_bar`]); nothing else. Opening it connects nothing.

use std::sync::Arc;

use ui_components::editor_toolbar::{EditorToolbar, EditorToolbarProvider, ToolbarContext};

use crate::bar::ConsoleBar;
use crate::file_bar::SqlFileBar;

#[derive(Debug, Default)]
pub struct ConsoleBarProvider;

impl EditorToolbarProvider for ConsoleBarProvider {
    fn id(&self) -> &'static str {
        "database-console"
    }

    fn open(&self, context: &ToolbarContext<'_>) -> Option<Box<dyn EditorToolbar>> {
        let (root, project) = (context.root?, context.project?);
        // A look at the path first: most files are neither a console's nor SQL.
        if database::console::identify(root, context.path).is_none() {
            database::console::attachable(root, context.path)?;
            let consoles = crate::consoles::consoles(project)?;
            return Some(Box::new(SqlFileBar::new(
                consoles,
                context.path,
                Arc::clone(&context.wake),
            )));
        }
        let consoles = crate::consoles::consoles(project)?;
        let spec = consoles.spec_of_path(context.path)?;
        let runner = consoles.runner(spec.id, spec.source);
        runner.set_wake(Arc::clone(&context.wake));
        Some(Box::new(ConsoleBar::new(consoles, spec, runner)))
    }
}
