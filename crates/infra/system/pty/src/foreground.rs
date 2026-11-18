//! Who has the terminal now (see `terminal_session::history::Foreground`) and the program's
//! name: asked when a program wants the history erased, for the tab's title while a command
//! runs (on the UI thread, as the screen changes) and by the Task Manager — so cheap to ask.
//!
//! Unix: the PTY's foreground process group (`tcgetpgrp` on its master side, one system
//! call): the shell's own (a session leader: its group is its pid), or a command's, whose
//! leader's start time says how long it has been running (macOS: `proc_pidinfo`; Linux:
//! `/proc`), asked once per process; its name read again at most every half second.
//! Windows (ConPTY has no process groups): the shell's children, the youngest one's age and
//! name (none: the shell itself), from one look at every process kept for half a second.

use std::time::{Duration, Instant};

use terminal_session::history::Foreground;

/// How long a look at the system answers for: Windows' look at every process costs a few
/// milliseconds, and a program's name (which an `exec` changes) is read again no sooner.
const KEPT: Duration = Duration::from_millis(500);

/// What a look at the system found: who has the terminal and, for a program, its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) foreground: Foreground,
    pub(crate) name: Option<String>,
}

impl Default for Seen {
    fn default() -> Self {
        Self { foreground: Foreground::Unknown, name: None }
    }
}

/// Asks the system who has the terminal and the program's name, in one look: the
/// foreground and its name are asked together (the tab's title, the Task Manager), so both
/// answer from what one look found.
pub(crate) struct Probe {
    look: Box<dyn FnMut(&mut Seen) + Send>,
    seen: Seen,
}

impl Probe {
    /// A probe that tells only who has the terminal (no name).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(mut ask: impl FnMut() -> Foreground + Send + 'static) -> Self {
        Self::looking(move |seen| *seen = Seen { foreground: ask(), name: None })
    }

    /// A probe whose `look` brings what it last found up to date.
    pub(crate) fn looking(look: impl FnMut(&mut Seen) + Send + 'static) -> Self {
        Self { look: Box::new(look), seen: Seen::default() }
    }

    fn look(&mut self) -> &Seen {
        (self.look)(&mut self.seen);
        &self.seen
    }

    pub(crate) fn foreground(&mut self) -> Foreground {
        self.look().foreground
    }

    /// The program in the foreground's name (`None`: the shell's, or not told).
    pub(crate) fn program(&mut self) -> Option<String> {
        self.look().name.clone()
    }
}

/// A look kept for [`KEPT`]: the system asked again only after that, a program's age
/// growing meanwhile (Windows: who has the terminal is known only by looking at every
/// process).
#[cfg(any(windows, test))]
#[derive(Debug, Default)]
pub(crate) struct Kept {
    at: Option<Instant>,
    foreground: Option<Foreground>,
}

#[cfg(any(windows, test))]
impl Kept {
    /// `seen` as of `now`: as found by the last look while it's fresh, else by `ask`.
    pub(crate) fn look(&mut self, now: Instant, seen: &mut Seen, ask: impl FnOnce() -> Seen) {
        if let (Some(at), Some(foreground)) = (self.at, self.foreground) {
            let since = now.saturating_duration_since(at);
            if since < KEPT {
                seen.foreground = match foreground {
                    Foreground::Program(age) => Foreground::Program(age.saturating_add(since)),
                    other => other,
                };
                return;
            }
        }
        *seen = ask();
        self.at = Some(now);
        self.foreground = Some(seen.foreground);
    }
}

/// A program known by its process: when it started (asked once: it doesn't change) and its
/// name — asked every time in its first [`KEPT`] (a forked child bears the shell's name
/// until it `exec`s), then again at most every [`KEPT`] (a program may `exec` another).
#[cfg(unix)]
#[derive(Debug, Default)]
pub(crate) struct Known {
    pid: u32,
    started: Option<Instant>,
    name: Option<String>,
    named: Option<Instant>,
}

#[cfg(unix)]
impl Known {
    /// Process `pid` as of `now`: its age (`None`: not told) and its name.
    pub(crate) fn look(
        &mut self,
        pid: u32,
        now: Instant,
        age: impl FnOnce(u32) -> Option<Duration>,
        name: impl FnOnce(u32) -> Option<String>,
    ) -> (Option<Duration>, &Option<String>) {
        if self.pid != pid {
            *self = Self { pid, ..Self::default() };
        }
        if self.started.is_none() {
            self.started = age(pid).and_then(|age| now.checked_sub(age));
        }
        let young = self.started.is_none_or(|s| now.saturating_duration_since(s) < KEPT);
        if young || self.named.is_none_or(|at| now.saturating_duration_since(at) >= KEPT) {
            self.name = name(pid);
            self.named = Some(now);
        }
        (self.started.map(|s| now.saturating_duration_since(s)), &self.name)
    }
}

/// The probe of the PTY whose master side is `fd`, the shell `shell`: its foreground group
/// asked each time (one system call), the program's start and name kept for it.
#[cfg(unix)]
pub(crate) fn probe(fd: std::os::fd::RawFd, shell: Option<u32>) -> Probe {
    let mut known = Known::default();
    Probe::looking(move |seen| {
        // SAFETY: a plain query on the PTY's descriptor, kept open by the PTY this probe
        // lives with (the same struct: dropped together).
        #[allow(unsafe_code)]
        let group = unsafe { libc::tcgetpgrp(fd) };
        if group <= 0 {
            *seen = Seen::default();
            return;
        }
        let group = group as u32;
        if Some(group) == shell {
            *seen = Seen { foreground: Foreground::Shell, name: None };
            return;
        }
        let (age, name) = known.look(group, Instant::now(), age_of, name_of);
        seen.foreground = age.map_or(Foreground::Unknown, Foreground::Program);
        seen.name.clone_from(name);
    })
}

/// Process `pid`'s name.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn name_of(pid: u32) -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is ours and of the size given; the call fills it.
    let got = unsafe {
        libc::proc_name(pid as libc::c_int, buffer.as_mut_ptr().cast(), buffer.len() as u32)
    };
    let got = usize::try_from(got).ok().filter(|n| *n > 0)?;
    Some(String::from_utf8_lossy(&buffer[..got.min(buffer.len())]).into_owned())
}

/// Process `pid`'s name (`/proc/<pid>/comm`).
#[cfg(target_os = "linux")]
fn name_of(pid: u32) -> Option<String> {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn name_of(_pid: u32) -> Option<String> {
    None
}

/// How long process `pid` has been running.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn age_of(pid: u32) -> Option<Duration> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is ours and of the size given; the call fills it.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::from_mut(&mut info).cast(),
            size,
        )
    };
    if got != size {
        return None;
    }
    let started = std::time::UNIX_EPOCH
        + Duration::from_secs(info.pbi_start_tvsec)
        + Duration::from_micros(info.pbi_start_tvusec);
    Some(std::time::SystemTime::now().duration_since(started).unwrap_or_default())
}

/// How long process `pid` has been running: its start (`/proc/<pid>/stat`, in clock ticks
/// since boot) against the system's uptime.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn age_of(pid: u32) -> Option<Duration> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // After the command's name (which may hold spaces and parentheses): the state is field
    // 3, the start time field 22.
    let rest = &stat[stat.rfind(')')? + 1..];
    let ticks: u64 = rest.split_whitespace().nth(19)?.parse().ok()?;
    // SAFETY: a plain query of a system constant.
    let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if per_second <= 0 {
        return None;
    }
    let uptime = std::fs::read_to_string("/proc/uptime").ok()?;
    let uptime: f64 = uptime.split_whitespace().next()?.parse().ok()?;
    let started = ticks as f64 / per_second as f64;
    Some(Duration::from_secs_f64((uptime - started).max(0.0)))
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn age_of(_pid: u32) -> Option<Duration> {
    None
}

/// The probe of the ConPTY whose shell is `shell`: one look at every process tells both
/// the shell's youngest child and its name, kept for [`KEPT`].
#[cfg(windows)]
pub(crate) fn probe(shell: Option<u32>) -> Probe {
    let shell_started = shell.and_then(windows::started);
    let mut kept = Kept::default();
    Probe::looking(move |seen| {
        let Some(shell) = shell else {
            *seen = Seen::default();
            return;
        };
        kept.look(Instant::now(), seen, || match windows::youngest_child(shell, shell_started) {
            None => Seen::default(),
            Some(None) => Seen { foreground: Foreground::Shell, name: None },
            Some(Some(child)) => {
                Seen { foreground: Foreground::Program(child.age), name: Some(child.name) }
            }
        });
    })
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn ticks(t: FILETIME) -> u64 {
        (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
    }

    /// When process `pid` started (100 ns ticks since 1601).
    pub(super) fn started(pid: u32) -> Option<u64> {
        // SAFETY: a handle only to ask about the process, closed below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: the four out-parameters are ours.
        let done =
            unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
        unsafe { CloseHandle(handle) };
        (done != 0).then(|| ticks(created))
    }

    fn now() -> u64 {
        let mut t = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        // SAFETY: the out-parameter is ours.
        unsafe { GetSystemTimeAsFileTime(&mut t) };
        ticks(t)
    }

    /// A program the shell started: its executable's name and how long it has run.
    pub(super) struct Child {
        pub(super) name: String,
        pub(super) age: Duration,
    }

    /// The shell's youngest child (`Some(None)`: none, the shell itself; `None`: the
    /// system didn't say). A child started before the shell is another's whose parent's
    /// pid was reused: not counted.
    pub(super) fn youngest_child(shell: u32, shell_started: Option<u64>) -> Option<Option<Child>> {
        // SAFETY: a snapshot of the processes, closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut children = Vec::new();
        // SAFETY: the entry is ours, its size set.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while more {
            if entry.th32ParentProcessID == shell && entry.th32ProcessID != shell {
                let end = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                children.push((entry.th32ProcessID, name));
            }
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        let now = now();
        let youngest = children
            .into_iter()
            .filter_map(|(pid, name)| Some((started(pid)?, name)))
            .filter(|(s, _)| shell_started.is_none_or(|shell| *s >= shell))
            .max_by_key(|(s, _)| *s)
            .map(|(started, name)| Child {
                name,
                age: Duration::from_nanos(now.saturating_sub(started) * 100),
            });
        Some(youngest)
    }
}

#[cfg(test)]
#[path = "../tests/unit/foreground.rs"]
mod tests;
