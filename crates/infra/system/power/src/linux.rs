//! Linux: a systemd inhibitor (`systemd-inhibit --what=sleep --mode=block`; `sleep:idle`
//! when the screen is kept on too: the session isn't idle, nothing blanks or locks), held by
//! a child process that only waits; stopped when the hold is dropped. Without systemd: not
//! supported (said once).

use std::process::{Child, Command, Stdio};

use power::{Keep, PowerError, SleepHold};

/// The waiting child holding the inhibitor: stopped when dropped.
#[derive(Debug)]
struct Inhibitor(Child);

impl SleepHold for Inhibitor {}

impl Drop for Inhibitor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) fn hold(reason: &str, keep: Keep) -> Result<Box<dyn SleepHold>, PowerError> {
    let what = if keep.screen { "--what=sleep:idle" } else { "--what=sleep" };
    let child = Command::new("systemd-inhibit")
        .args([what, "--mode=block", "--who=editor"])
        .arg(format!("--why={reason}"))
        .args(["sleep", "infinity"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PowerError::Unsupported("systemd-inhibit".into()),
            _ => PowerError::Failed(format!("systemd-inhibit: {e}")),
        })?;
    Ok(Box::new(Inhibitor(child)))
}
