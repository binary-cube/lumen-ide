use super::*;
use crate::data::{Column, ColumnKind, Rows};
use crate::geometry::{Metrics, View, lay_out};
use crate::prepare::prepare;
use crate::spec::{ChartSpec, Options, SeriesSpec};

fn look() -> SvgLook {
    SvgLook {
        colors: vec![Rgba::rgb(0x5b9bf8), Rgba::rgb(0x4cc38a)],
        text: Rgba::rgb(0x202020),
        muted: Rgba::rgb(0x808080),
        grid: Rgba::rgb(0x202020).with_alpha(0.07),
        zero: Rgba::rgb(0x202020).with_alpha(0.2),
        background: Rgba::rgb(0xffffff),
        font_family: "Inter & co".into(),
        font_size: 12.0,
        line_width: 1.75,
        area_alpha: (0.18, 0.55),
    }
}

fn written(kind: ChartKind) -> String {
    let mut rows =
        Rows::new(vec![Column::new("k", ColumnKind::Text), Column::new("v", ColumnKind::Number)]);
    for (k, v) in [("a<b", 3), ("c", 5)] {
        rows.push([Some(k.to_string()), Some(v.to_string())]);
    }
    let spec = ChartSpec {
        kind,
        series: vec![SeriesSpec {
            x: Some("k".into()),
            y: vec!["v".into()],
            ..SeriesSpec::default()
        }],
        options: Options::default(),
    };
    let p = prepare(&spec, &rows).expect("p");
    let view = View {
        area: kernel::Rect::new(100.0, 50.0, 400.0, 300.0),
        options: Options::default(),
        metrics: Metrics::at(1.0),
        hidden: &[],
        zoom: None,
        axes: true,
        legend: true,
        y_range: None,
        lanes: false,
        y_label: None,
    };
    let g = lay_out(&p, &view, &mut |s: &str| s.len() as f32 * 7.0);
    svg(&p, &g, &look())
}

/// Bars, a line and a pie written as the screen draws them, from the area's corner, their
/// texts escaped.
#[test]
fn a_chart_is_written_as_svg() {
    let bars = written(ChartKind::Bar);
    assert!(bars.starts_with("<svg") && bars.ends_with("</svg>"));
    assert!(bars.contains(r#"width="400" height="300""#));
    assert_eq!(bars.matches("<rect").count(), 3, "the background and two bars");
    assert!(bars.contains("a&lt;b"), "a label escaped");
    assert!(bars.contains("Inter &amp; co"));
    assert!(!bars.contains("x=\"-"), "nothing left of the area");
    let line = written(ChartKind::Line);
    assert!(line.contains("<polyline"));
    let pie = written(ChartKind::Pie);
    assert_eq!(pie.matches("<path").count(), 2, "a slice each");
}
