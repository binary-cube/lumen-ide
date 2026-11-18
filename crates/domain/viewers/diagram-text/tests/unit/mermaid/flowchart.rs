use super::super::{Diagram, ErrorKind, parse as parse_any};
use super::*;

fn chart(source: &str) -> Flowchart {
    match parse_any(source).expect("parsed").diagram {
        Diagram::Flowchart(f) => f,
        other => panic!("{other:?}"),
    }
}

fn node<'a>(f: &'a Flowchart, id: &str) -> &'a Node {
    f.nodes.iter().find(|n| n.id == id).unwrap_or_else(|| panic!("{id}"))
}

#[test]
fn shapes_and_labels() {
    let f = chart(
        "flowchart TD\n A[Rect] --> B(Round) --> C([Stadium]) --> D[[Sub]] --> E[(DB)]\n\
         F((Circle)) --> G{Decide?} --> H{{Hex}} --> I[/Para/] --> J[\\Alt\\]\n\
         K[/Trap\\] --> L[\\TrapAlt/] --> M>Flag] --> N(((Double)))\n O[\"Quoted [text]\"]",
    );
    let shapes: Vec<Shape> = f.nodes.iter().map(|n| n.shape).collect();
    use Shape::*;
    assert_eq!(
        shapes,
        vec![
            Rect,
            Round,
            Stadium,
            Subroutine,
            Cylinder,
            Circle,
            Diamond,
            Hexagon,
            Parallelogram,
            ParallelogramAlt,
            Trapezoid,
            TrapezoidAlt,
            Asymmetric,
            DoubleCircle,
            Rect
        ]
    );
    assert_eq!(node(&f, "G").label, "Decide?");
    assert_eq!(node(&f, "O").label, "Quoted [text]");
    assert_eq!(f.edges.len(), 11);
}

#[test]
fn links() {
    let f = chart(
        "graph LR\nA-->B\nA --- C\nA-.->D\nA==>E\nA--oF\nA--xG\nA<-->H\nA~~~I\n\
         A-->|label|J\nA-- text -->K\nA-. dotted .->L\nA ---> M\nA == thick ==> N",
    );
    assert_eq!(f.direction, super::super::Direction::Right);
    let e = |to: &str| {
        let n = f.nodes.iter().position(|n| n.id == to).unwrap();
        f.edges.iter().find(|e| e.to == End::Node(n)).unwrap().clone()
    };
    assert_eq!((e("B").stroke, e("B").head), (Stroke::Solid, Head::Arrow));
    assert_eq!((e("C").stroke, e("C").head), (Stroke::Solid, Head::None));
    assert_eq!((e("D").stroke, e("D").head), (Stroke::Dotted, Head::Arrow));
    assert_eq!((e("E").stroke, e("E").head), (Stroke::Thick, Head::Arrow));
    assert_eq!(e("F").head, Head::Circle);
    assert_eq!(e("G").head, Head::Cross);
    assert_eq!((e("H").head, e("H").tail), (Head::Arrow, Head::Arrow));
    assert_eq!(e("I").stroke, Stroke::Invisible);
    assert_eq!(e("J").label.as_deref(), Some("label"));
    assert_eq!(e("K").label.as_deref(), Some("text"));
    assert_eq!((e("L").label.as_deref(), e("L").stroke), (Some("dotted"), Stroke::Dotted));
    assert_eq!(e("M").length, 2);
    assert_eq!((e("N").label.as_deref(), e("N").stroke), (Some("thick"), Stroke::Thick));
}

#[test]
fn chains_and_ampersands() {
    let f = chart("graph TD\nA & B --> C & D --> E; F-->G");
    assert_eq!(f.edges.len(), 4 + 2 + 1);
    assert_eq!(f.nodes.len(), 7);
}

#[test]
fn subgraphs_classes_styles_clicks() {
    let f = chart(
        "flowchart TB\n  c1-->a2\n  subgraph one [The One]\n    direction LR\n    a1-->a2\n  end\n\
         subgraph two\n  b1-->b2\n  subgraph inner\n   x\n  end\n  end\n  one --> two\n\
         classDef hot fill:#f00,color:#fff\n  class a1 hot\n  b1:::hot\n\
         style b2 stroke:#0f0,stroke-width:3px\n  linkStyle 0 stroke:#00f\n\
         click a1 \"https://example.com\" \"Tip\"",
    );
    let one = &f.subgraphs[0];
    assert_eq!((one.id.as_str(), one.title.as_str()), ("one", "The One"));
    assert_eq!(one.direction, Some(super::super::Direction::Right));
    // a2 was named before the subgraph: it stays outside it.
    assert_eq!(one.nodes.iter().map(|&n| f.nodes[n].id.as_str()).collect::<Vec<_>>(), ["a1"]);
    assert_eq!(f.subgraphs[2].parent, Some(1));
    assert!(f.edges.iter().any(|e| e.from == End::Group(0) && e.to == End::Group(1)));
    assert!(node(&f, "a1").style.fill.is_some() && node(&f, "b1").style.color.is_some());
    assert_eq!(node(&f, "b2").style.stroke_width, Some(3.0));
    assert!(f.edges[0].style.stroke.is_some());
    assert_eq!(node(&f, "a1").link.as_deref(), Some("https://example.com"));
}

#[test]
fn errors_name_their_line() {
    let e = parse_any("graph TD\nA-->B\nA-->\n").unwrap_err();
    assert_eq!((e.line, e.kind), (3, ErrorKind::Unexpected("A-->".into())));
    let e = parse_any("graph TD\nsubgraph x\nA\n").unwrap_err();
    assert_eq!((e.line, e.kind), (2, ErrorKind::Unclosed("end")));
    let e = parse_any("graph TD\nA[unclosed\n").unwrap_err();
    assert_eq!(e.line, 2);
}
