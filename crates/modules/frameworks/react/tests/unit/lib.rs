use super::*;

#[test]
fn hooks() {
    let src = "const [n, setN] = React.useState(0); obj.useState(1); us";
    assert_eq!(identifier_before(src, src.len()).map(|r| &src[r]), Some("us"));
    let at = src.find("useState").unwrap();
    assert_eq!(identifier_before(src, at + 3).map(|r| &src[r]), Some("use"));
    let member = src.find("obj.useState").unwrap() + 7;
    assert!(identifier_before(src, member).is_none());
    assert!(API.iter().all(|(_, sig)| sig.starts_with('(')));
}

/// The hooks are offered once `us…` is typed; any other word (one with `ă` too) is left
/// alone.
#[test]
fn hooks_once_us_is_typed() {
    assert!(typing_a_hook("us") && typing_a_hook("useSt"));
    assert!(!typing_a_hook("u") && !typing_a_hook("x"));
    assert!(!typing_a_hook("ăă") && !typing_a_hook("usă") && !typing_a_hook("uă"));
}
