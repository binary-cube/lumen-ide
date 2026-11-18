//! A made-up contributor (the snapshots', the tests'): two data sources, their consoles
//! (one with a transaction open, one whose last statement failed), a console's tabs — its
//! Output, a pinned result, another one — and the Output's log at a stopped clock.

use ui_components::{
    Log, LogKind, LogLine, LogView, ProjectServices, ServiceId, ServiceNode, ServiceProvider,
    ServiceState, ServiceTab, ServicesContext, ViewerAction, ViewerPaint,
};

/// The sample contributor's id.
pub const ID: &str = "sample";

#[derive(Debug, Default)]
pub struct SampleProvider;

impl ServiceProvider for SampleProvider {
    fn id(&self) -> &'static str {
        ID
    }

    fn order(&self) -> u16 {
        10
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        i18n::t("services.sample_group")
    }

    fn open(&self, _ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        Some(Box::new(SampleServices::new()))
    }
}

/// The nodes' identities (fixed: the same tree every run).
pub const SHOP: ServiceId = ServiceId(0x0001);
pub const CONSOLE: ServiceId = ServiceId(0x0002);
pub const CONSOLE_1: ServiceId = ServiceId(0x0003);
pub const ANALYTICS: ServiceId = ServiceId(0x0004);
pub const REPORTS: ServiceId = ServiceId(0x0005);
pub const OUTPUT: ServiceId = ServiceId(0x0101);
pub const CUSTOMERS: ServiceId = ServiceId(0x0102);
pub const RESULT_2: ServiceId = ServiceId(0x0103);

/// The sample's services in a project.
#[derive(Debug)]
pub struct SampleServices {
    nodes: Vec<ServiceNode>,
    tabs: Vec<ServiceTab>,
    log: Log,
    generation: u64,
}

impl Default for SampleServices {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleServices {
    pub fn new() -> Self {
        let nodes = vec![
            ServiceNode::new(SHOP, "database", "shop.sqlite")
                .with_detail("SQLite")
                .with_color("green"),
            ServiceNode::new(CONSOLE, "file-code", "console")
                .under(SHOP)
                .with_detail("543 ms")
                .with_state(ServiceState::TxOpen),
            ServiceNode::new(CONSOLE_1, "file-code", "console_1")
                .under(SHOP)
                .with_detail("12 ms")
                .with_state(ServiceState::Error),
            ServiceNode::new(ANALYTICS, "server", "analytics@localhost")
                .with_detail("PostgreSQL 17")
                .with_color("red"),
            ServiceNode::new(REPORTS, "file-code", "reports").under(ANALYTICS).with_detail("1.2 s"),
        ];
        let tabs = vec![
            ServiceTab::new(OUTPUT, "file-text", "Output").fixed(),
            ServiceTab::new(CUSTOMERS, "table", "main.customers").pinned(true),
            ServiceTab::new(RESULT_2, "table", "Result 2"),
        ];
        Self { nodes, tabs, log: sample_log(), generation: 1 }
    }

    pub fn log(&self) -> &Log {
        &self.log
    }
}

/// The Output's lines, as a console session writes them.
pub fn sample_log() -> Log {
    let log = Log::new();
    let at = |s: &str| format!("2026-10-31 10:00:{s}");
    log.push(LogLine::new(at("23.698"), LogKind::Info, "Connected to shop.sqlite"));
    log.push(
        LogLine::new(
            at("24.102"),
            LogKind::Statement,
            "SELECT c.name, COUNT(o.id) AS orders FROM customers c LEFT JOIN orders o ON o.customer_id = c.id GROUP BY c.name",
        )
        .with_prefix("main>")
        .as_code("sql"),
    );
    log.push(LogLine::new(
        at("24.513"),
        LogKind::Result,
        "4 rows retrieved starting from 1 in 411 ms (execution: 43 ms, fetching: 368 ms)",
    ));
    log.push(
        LogLine::new(
            at("31.020"),
            LogKind::Statement,
            "UPDATE orders SET status = 'paid' WHERE id = 7",
        )
        .with_prefix("main>")
        .as_code("sql"),
    );
    log.push(LogLine::new(at("31.026"), LogKind::Result, "1 row affected in 6 ms"));
    log.push(LogLine::new(
        at("31.027"),
        LogKind::Warning,
        "A transaction is open: 1 statement waits for Commit or Rollback",
    ));
    log.push(
        LogLine::new(at("40.311"), LogKind::Statement, "SELECT * FROM order_lines")
            .with_prefix("main>")
            .as_code("sql"),
    );
    log.push(
        LogLine::new(
            at("40.312"),
            LogKind::Error,
            "[SQLITE_ERROR] no such table: order_lines (line 1, column 15)",
        )
        .with_link(ViewerAction::OpenFile {
            path: std::path::PathBuf::from("console.sql"),
            line: Some(1),
            column: Some(15),
        }),
    );
    log.push(LogLine::new(
        at("41.900"),
        LogKind::Notice,
        "NOTICE: table \"order_lines_old\" does not exist, skipping",
    ));
    log
}

impl ProjectServices for SampleServices {
    fn nodes(&self) -> &[ServiceNode] {
        &self.nodes
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn tabs(&self, node: ServiceId) -> Vec<ServiceTab> {
        if node == CONSOLE { self.tabs.clone() } else { Vec::new() }
    }

    fn open_tab(&mut self, tab: ServiceId) -> Option<Box<dyn ui_components::Viewer>> {
        match tab {
            OUTPUT => Some(Box::new(LogView::new(self.log.clone()))),
            CUSTOMERS | RESULT_2 => {
                let title = self.tabs.iter().find(|t| t.id == tab)?.title.clone();
                Some(Box::new(Note(title)))
            }
            _ => None,
        }
    }

    fn close_tab(&mut self, tab: ServiceId) {
        self.tabs.retain(|t| t.id != tab || !t.closable);
        self.generation += 1;
    }

    fn pin_tab(&mut self, tab: ServiceId, pinned: bool) {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab) {
            t.pinned = pinned;
        }
        self.generation += 1;
    }
}

/// A result's tab, in the sample: its name said in the middle.
struct Note(String);

impl ui_components::Viewer for Note {
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ds = ui_components::ds::Ctx::new(ctx.palette, ctx.scale);
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        layer.quad(render_api::Quad::filled(ctx.bounds, ctx.palette.editor_background));
        ui_components::ds::empty_state(layer, ctx.text, &ds, ctx.bounds, &self.0, None);
    }

    /// Its actions' keys bound to a view of its own (a result's, in the sample).
    fn actions_view(&self) -> Option<&'static str> {
        Some(NOTE_VIEW)
    }

    /// `*` and the sample's own command taken (what it was given, kept as its text).
    fn key(&mut self, key: char) -> bool {
        self.0.push(key);
        true
    }

    fn command(&mut self, id: &str) -> bool {
        if id == "note.mark" {
            self.0.push('!');
            return true;
        }
        false
    }

    fn selected_text(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

/// The sample's results' view kind (their actions' keys).
pub const NOTE_VIEW: &str = "sample-note";
