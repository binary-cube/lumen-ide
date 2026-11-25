use std::path::PathBuf;

use super::Options;

fn parsed(args: &[&str]) -> Option<Options> {
    Options::parse(args.iter().map(|a| (*a).to_owned()))
}

/// The host's own switches read, CEF's beside them left alone.
#[test]
fn the_hosts_switches_are_read_among_cefs() {
    let options = parsed(&[
        "--type=renderer",
        "--channel-dir",
        "/tmp/ch",
        "--channel-name",
        "web-host-2",
        "--profile",
        "/tmp/profile",
        "--hold",
        "/tmp/v/.in-use",
        "--adapter-luid",
        "1,2",
    ])
    .expect("complete");
    assert_eq!(options.channel.dir, PathBuf::from("/tmp/ch"));
    assert_eq!(options.channel.name, "web-host-2");
    assert_eq!(options.profile, PathBuf::from("/tmp/profile"));
    assert_eq!(options.hold, Some(PathBuf::from("/tmp/v/.in-use")));
    assert_eq!(options.adapter, Some(web_engine_protocol::Adapter { high: 1, low: 2 }));
}

/// Without its channel or its profile the host has nothing to do; a GPU it can't read is
/// none.
#[test]
fn what_the_host_needs_must_be_given() {
    assert!(parsed(&["--channel-dir", "/tmp/ch", "--profile", "/p"]).is_none(), "no name");
    assert!(parsed(&["--channel-name", "n", "--profile", "/p"]).is_none(), "no folder");
    assert!(parsed(&["--channel-dir", "/tmp/ch", "--channel-name", "n"]).is_none(), "no profile");
    assert!(
        parsed(&["--channel-dir", "/d", "--channel-name"]).is_none(),
        "a switch without its value"
    );
    let options = parsed(&[
        "--channel-dir",
        "/d",
        "--channel-name",
        "n",
        "--profile",
        "/p",
        "--adapter-luid",
        "x",
    ]);
    assert_eq!(options.and_then(|o| o.adapter), None);
}

/// A channel's name that would reach outside its folder is refused.
#[test]
fn a_channel_name_stays_in_its_folder() {
    for name in ["../up", "a/b", "a\\b", "..", ""] {
        let args = ["--channel-dir", "/d", "--channel-name", name, "--profile", "/p"];
        assert!(parsed(&args).is_none(), "{name:?}");
    }
}
