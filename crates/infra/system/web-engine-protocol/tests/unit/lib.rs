use super::*;

/// The channel is named after the protocol: hosts of other versions never meet.
#[test]
fn the_channel_names_the_protocol() {
    assert!(channel_name().ends_with(&PROTOCOL.to_string()));
}
