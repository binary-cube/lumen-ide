use super::*;

/// Every agent has an id, a name and a drawing of its own (none twice).
#[test]
fn every_planned_agent_is_distinct_and_drawn() {
    for (i, (id, name, icon, svg)) in AGENTS.iter().enumerate() {
        assert!(!id.is_empty() && !name.is_empty(), "{id}");
        assert!(icon.starts_with("agent-"), "{icon}");
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{id}");
        assert!(AGENTS[i + 1..].iter().all(|(other, _, o_icon, _)| other != id && o_icon != icon));
    }
}
