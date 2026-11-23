//! Charts of a result's rows (plan 94): a pane under any tabular result whose host offers
//! panes (`ui_components::result_pane`: a query's result in the database console…), with
//! its series settings beside it — the chart's kind, which column goes on X, which splits
//! the series in groups, which are the values and how they're summed, its options — drawn
//! with the app's one chart (`ui_components::charts`).
//!
//! It's contributed as a [`ResultPaneProvider`]; no other module knows it, and it knows none:
//! the rows come lent as `charts_model::Tabular`, what it asks goes back as a request, its
//! state as text its host keeps.

mod export;
pub mod file_view;
mod pane;
mod panel;
pub mod settings;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::IconSvg;
use ui_components::result_pane::{ResultPane, ResultPaneProvider};

pub use pane::ChartPane;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("viewer.charts"),
    name: "Charts",
    description: "Draws a result's rows as a chart (bars, lines, areas, points, bubbles, a pie, a histogram) under the result, its series set beside it",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "chart", ..extensions::ModuleInfo::BUILT_IN },
};

/// The chart's kinds' icons and its own (in the app's set's style).
const ICONS: &[IconSvg] = &[
    IconSvg { name: "chart", svg: include_str!("../svg/chart.svg") },
    IconSvg { name: "chart-bar", svg: include_str!("../svg/chart-bar.svg") },
    IconSvg { name: "chart-pie", svg: include_str!("../svg/chart-pie.svg") },
    IconSvg { name: "chart-area", svg: include_str!("../svg/chart-area.svg") },
    IconSvg { name: "chart-line", svg: include_str!("../svg/chart-line.svg") },
    IconSvg { name: "chart-scatter", svg: include_str!("../svg/chart-scatter.svg") },
    IconSvg { name: "chart-bubble", svg: include_str!("../svg/chart-bubble.svg") },
    IconSvg { name: "chart-stacked-bar", svg: include_str!("../svg/chart-stacked-bar.svg") },
    IconSvg { name: "chart-stacked-area", svg: include_str!("../svg/chart-stacked-area.svg") },
    IconSvg { name: "chart-histogram", svg: include_str!("../svg/chart-histogram.svg") },
];

/// The threads it prepares charts on (with the shared chart's rasterizing).
pub(crate) const PREPARE_THREAD: &str = "chart-prepare";

#[derive(Debug, Default)]
pub struct ChartsModule;

impl Module for ChartsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(workbench_model::ModuleThreads(&[PREPARE_THREAD, "chart-raster"]));
        for icon in ICONS {
            ctx.contribute(*icon);
        }
        ctx.contribute::<Arc<dyn ResultPaneProvider>>(Arc::new(ChartPaneProvider));
        // Any data as a chart: "Open as Chart" and its tab.
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            file_view::ChartFileProvider,
        ));
        ctx.contribute(workbench_model::CommandContribution {
            spec: command_model::CommandSpec {
                id: command_model::CommandId(file_view::OPEN_COMMAND),
                label: "Open as Chart",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("chart"),
            handler: Arc::new(file_view::OpenAsChart),
        });
        // In the editor's and its tab's right-click menus (the file as the editor has it).
        ctx.contribute(workbench_model::ContextMenuItem::new(
            command_model::CommandId(file_view::OPEN_COMMAND),
            &[workbench_model::MenuPlace::Editor, workbench_model::MenuPlace::Tab],
            workbench_model::MenuSection::Compare,
            50,
        ));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::ChartSettings,
        ));
        Ok(())
    }
}

/// The pane's kind, as offered to the hosts.
#[derive(Debug)]
struct ChartPaneProvider;

impl ResultPaneProvider for ChartPaneProvider {
    fn id(&self) -> &'static str {
        "chart"
    }

    fn icon(&self) -> &'static str {
        "chart"
    }

    fn title(&self) -> String {
        i18n::t("charts.title")
    }

    fn make(&self) -> Box<dyn ResultPane> {
        Box::new(ChartPane::new())
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
