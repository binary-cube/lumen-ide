//! What the host is started with (`--channel-dir <dir> --channel-name <name> --profile <dir>`,
//! and optionally `--hold <file>`, `--adapter-luid <high>,<low>`); CEF's own switches, given
//! to its subprocesses on the same command line, are left to it.

use std::path::PathBuf;

use web_host_core::channel::Channel;

/// What the host was started with.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub channel: Channel,
    /// Where the pages keep their storage (the app's data: never a folder the system cleans).
    pub profile: PathBuf,
    /// A file held locked while it runs (its version's `.in-use`: the app cleans no version
    /// a host runs from).
    pub hold: Option<PathBuf>,
    /// The GPU it draws on (`--adapter-luid <high>,<low>`: the app's, so its frames open
    /// there).
    pub adapter: Option<web_engine_protocol::Adapter>,
}

impl Options {
    /// From the program's arguments, its name left out (`None`: one it needs is missing, or
    /// the channel's name isn't a plain name).
    pub fn parse(args: impl IntoIterator<Item = String>) -> Option<Self> {
        let mut args = args.into_iter();
        let (mut dir, mut name, mut profile, mut hold) = (None, None, None, None);
        let mut adapter = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--channel-dir" => dir = args.next().map(PathBuf::from),
                "--channel-name" => name = args.next(),
                "--profile" => profile = args.next().map(PathBuf::from),
                "--hold" => hold = args.next().map(PathBuf::from),
                "--adapter-luid" => {
                    adapter = args.next().and_then(|a| web_engine_protocol::Adapter::parse(&a));
                }
                // CEF's own switches (its subprocesses get theirs): not ours.
                _ => {}
            }
        }
        let name = name.filter(|name| plain_name(name))?;
        Some(Self { channel: Channel { name, dir: dir? }, profile: profile?, hold, adapter })
    }
}

/// Whether a channel's name names a file in its folder and nothing else: the socket, the
/// lock and the log are made from it (no separator, no way up).
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.contains("..")
}

#[cfg(test)]
#[path = "../tests/unit/options.rs"]
mod tests;
