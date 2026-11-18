use super::super::{Diagram, parse as parse_any};

#[test]
fn requirements() {
    let p = parse_any("requirementDiagram\n\n    requirement test_req {\n    id: 1\n    text: the test text.\n    risk: high\n    verifymethod: test\n    }\n\n    element test_entity {\n    type: simulation\n    }\n\n    test_entity - satisfies -> test_req").expect("parsed");
    let Diagram::Requirement(g) = p.diagram else { panic!() };
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.nodes[0].lines.len(), 4);
    assert_eq!(g.edges[0].label.as_deref(), Some("«satisfies»"));
    assert_eq!((g.edges[0].from, g.edges[0].to), (1, 0));
}

#[test]
fn c4_context() {
    let p = parse_any("C4Context\n  title System Context diagram for Internet Banking System\n  Enterprise_Boundary(b0, \"BankBoundary0\") {\n    Person(customerA, \"Banking Customer A\", \"A customer of the bank, with personal bank accounts.\")\n    System(SystemAA, \"Internet Banking System\", \"Allows customers to view information.\")\n    System_Ext(SystemE, \"Mainframe Banking System\", \"Stores all of the core banking information.\")\n  }\n  Rel(customerA, SystemAA, \"Uses\")\n  BiRel(SystemAA, SystemE, \"Uses\", \"HTTPS\")\n  UpdateRelStyle(customerA, SystemAA, $textColor=\"blue\")").expect("parsed");
    assert!(p.meta.title.is_some());
    let Diagram::C4(g) = p.diagram else { panic!() };
    assert_eq!(g.groups.len(), 1);
    assert_eq!(g.nodes.len(), 3);
    assert!(g.nodes.iter().all(|n| n.group == Some(0)));
    assert_eq!(g.nodes[0].kind, "Person");
    assert!(g.edges[1].tail && g.edges[1].detail.as_deref() == Some("[HTTPS]"));
}

#[test]
fn architecture() {
    let p = parse_any("architecture-beta\n    group api(cloud)[API]\n\n    service db(database)[Database] in api\n    service disk1(disk)[Storage] in api\n    service server(server)[Server] in api\n\n    db:L -- R:server\n    disk1:T -- B:server\n    junction center").expect("parsed");
    let Diagram::Architecture(g) = p.diagram else { panic!() };
    assert_eq!(g.groups[0].title, "API");
    assert_eq!(g.nodes.len(), 4);
    assert_eq!((g.nodes[0].kind.as_str(), g.nodes[0].group), ("database", Some(0)));
    assert_eq!(g.edges.len(), 2);
}
