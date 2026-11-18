use super::super::{Diagram, parse as parse_any};

#[test]
fn bit_ranges() {
    let p = parse_any("packet-beta\ntitle UDP Packet\n0-15: \"Source Port\"\n16-31: \"Destination Port\"\n32-47: \"Length\"\n48-63: \"Checksum\"\n64-95: \"Data (variable length)\"\n+8: \"Next\"").expect("parsed");
    assert_eq!(p.meta.title.as_deref(), Some("UDP Packet"));
    let Diagram::Packet(k) = p.diagram else { panic!() };
    assert_eq!(k.fields[0], (0, 15, "Source Port".into()));
    assert_eq!(k.fields[5], (96, 103, "Next".into()));
}
