//! This process's threads: each one's name and the CPU time it has used so far. Sampled
//! twice, the difference over the time between says how busy each one is — and, the
//! threads being named after what they do (`indexer`, `syntax`, `git-log`…), which part
//! of the app is.
//!
//! macOS: the task's Mach threads (`thread_info`), named through pthreads. Linux:
//! `/proc/self/task`. Windows: a Toolhelp snapshot of the process's threads,
//! `GetThreadTimes` and `GetThreadDescription`.
//!
//! The programs the app started are measured with their descendants ([`ProcessTrees`]).

use std::time::Duration;

mod children;
mod first_word;

pub use children::{ProcessTrees, TreeUse};

/// A thread, as last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadTime {
    /// The OS's id for it (stable while it lives).
    pub id: u64,
    /// Its name (`""` if it has none).
    pub name: String,
    /// User + system CPU time used since it started.
    pub cpu: Duration,
}

/// This process's threads now (empty where it can't be read).
pub fn threads() -> Vec<ThreadTime> {
    platform::threads()
}

/// What this process has read and written so far (bytes), as the system counts it — to
/// compare with what the app's parts report. macOS: the disk (`proc_pid_rusage`); Linux:
/// the storage layer (`/proc/self/io`); Windows: every I/O (`GetProcessIoCounters`: the
/// network's too). `None` where it can't be read.
pub fn disk_io() -> Option<(u64, u64)> {
    io::disk_io()
}

/// A program's name from the first word of its command (`/usr/bin/git` → `git`, a login
/// shell's `-zsh` → `zsh`, `node.exe` → `node`): its file's name, [tidied](tidy_name).
/// `None`: nothing left, or a title the program wrote over it (`sshd: user@pts/3`: its
/// folder isn't one).
pub fn invoked_name(word: &str) -> Option<String> {
    let (folder, file) = word.rsplit_once(['/', '\\']).unwrap_or(("", word));
    if folder.contains(": ") {
        return None;
    }
    let file = file.trim_start_matches('-');
    tidy_name(strip_exe(file))
}

/// A name another program chose, made safe to show: one line, at most [`NAME_CHARS`]
/// characters, without control characters or the invisible ones that could disguise it
/// (bidi overrides and isolates, zero-width marks). `None`: nothing left.
pub fn tidy_name(name: &str) -> Option<String> {
    let invisible = |c: char| {
        c.is_control()
            || matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{FEFF}')
            || ('\u{200B}'..='\u{200F}').contains(&c)
            || ('\u{202A}'..='\u{202E}').contains(&c)
            || ('\u{2060}'..='\u{2069}').contains(&c)
    };
    let name: String = name.chars().filter(|c| !invisible(*c)).take(NAME_CHARS).collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The longest name shown.
pub const NAME_CHARS: usize = 40;

/// A file's name without `.exe` (any case).
fn strip_exe(file: &str) -> &str {
    let cut = file.len().saturating_sub(4);
    match (file.get(..cut), file.get(cut..)) {
        (Some(stem), Some(ext)) if ext.eq_ignore_ascii_case(".exe") && !stem.is_empty() => stem,
        _ => file,
    }
}

/// The name of program `pid` (`git`, `zsh`, `php-cs-fixer`), as the system knows it: macOS
/// `proc_name`, Linux `/proc/<pid>/comm`, Windows `QueryFullProcessImageNameW` (its file's
/// name, without `.exe`). `None` when it can't be read (ended, not ours to look at).
/// Tidied as [`tidy_name`] (Linux's can be set by the program itself).
pub fn process_name(pid: u32) -> Option<String> {
    names::process_name(pid).as_deref().and_then(tidy_name)
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod names {
    pub fn process_name(pid: u32) -> Option<String> {
        let pid = libc::pid_t::try_from(pid).ok()?;
        // `proc_name` writes at most 2 × MAXCOMLEN (16) bytes and a terminator.
        let mut buffer = [0u8; 64];
        // SAFETY: the buffer is ours and its length is said; the call writes at most that.
        let len = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        let len = usize::try_from(len).ok().filter(|l| *l > 0)?;
        Some(String::from_utf8_lossy(&buffer[..len.min(buffer.len())]).into_owned())
    }
}

#[cfg(target_os = "linux")]
mod names {
    pub fn process_name(pid: u32) -> Option<String> {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(comm.trim_end_matches('\n').to_owned())
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod names {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };

    pub fn process_name(pid: u32) -> Option<String> {
        // SAFETY: a handle to the process (limited to asking about it), closed below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        // SAFETY: the buffer is ours, its length in characters given; the call writes the
        // path's length back.
        let done = unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
        };
        unsafe { CloseHandle(handle) };
        if done == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..(len as usize).min(buffer.len())]);
        let file = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        Some(super::strip_exe(file).to_owned())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod names {
    pub fn process_name(_pid: u32) -> Option<String> {
        None
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod io {
    pub fn disk_io() -> Option<(u64, u64)> {
        // SAFETY: `info` is a zeroed `rusage_info_v2`, the structure the V2 flavor fills;
        // `getpid` names this process.
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        let buffer = (&mut info as *mut libc::rusage_info_v2).cast::<libc::c_void>();
        let done =
            unsafe { libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V2, buffer.cast()) };
        (done == 0).then_some((info.ri_diskio_bytesread, info.ri_diskio_byteswritten))
    }
}

#[cfg(target_os = "linux")]
mod io {
    pub fn disk_io() -> Option<(u64, u64)> {
        let text = std::fs::read_to_string("/proc/self/io").ok()?;
        let field = |name: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(name))
                .and_then(|v| v.trim().parse::<u64>().ok())
        };
        Some((field("read_bytes:")?, field("write_bytes:")?))
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod io {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessIoCounters, IO_COUNTERS,
    };

    pub fn disk_io() -> Option<(u64, u64)> {
        // SAFETY: the current process's pseudo-handle needs no closing; `counters` is an
        // owned, zeroed `IO_COUNTERS` the call fills.
        let mut counters: IO_COUNTERS = unsafe { std::mem::zeroed() };
        let done = unsafe { GetProcessIoCounters(GetCurrentProcess(), &mut counters) };
        (done != 0).then_some((counters.ReadTransferCount, counters.WriteTransferCount))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod io {
    pub fn disk_io() -> Option<(u64, u64)> {
        None
    }
}

/// What this process holds of the memory as its own (bytes): what Activity Monitor shows
/// on macOS (`phys_footprint`: its private pages, the GPU's memory it asked for, the
/// compressed ones; not the system's shared libraries), Windows' private bytes, Linux's
/// anonymous resident pages. Not RSS: that counts the pages shared with every other app.
pub fn footprint() -> Option<u64> {
    footprint_of(std::process::id())
}

/// [`footprint`] of process `pid` (one of the user's own: the programs this one started).
/// `None`: it ended, or the system doesn't say (then RSS is the fallback).
pub fn footprint_of(pid: u32) -> Option<u64> {
    own::footprint(pid)
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod own {
    pub fn footprint(pid: u32) -> Option<u64> {
        let pid = libc::pid_t::try_from(pid).ok()?;
        // SAFETY: as `io::disk_io`: a zeroed `rusage_info_v2` the V2 flavor fills; another
        // process of the same user is read without its task port (as Activity Monitor).
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        let buffer = (&mut info as *mut libc::rusage_info_v2).cast::<libc::c_void>();
        let done = unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, buffer.cast()) };
        (done == 0).then_some(info.ri_phys_footprint)
    }
}

#[cfg(target_os = "linux")]
mod own {
    pub fn footprint(pid: u32) -> Option<u64> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let kb = |name: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(name))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        };
        Some(kb("RssAnon:")?.saturating_add(kb("RssShmem:").unwrap_or(0)).saturating_mul(1024))
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod own {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    pub fn footprint(pid: u32) -> Option<u64> {
        let own = pid == std::process::id();
        // SAFETY: the current process's pseudo-handle needs no closing; another's is opened
        // for reading its counters only and closed below.
        let handle = unsafe {
            if own {
                GetCurrentProcess()
            } else {
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
            }
        };
        if handle.is_null() {
            return None;
        }
        // SAFETY: `counters` is an owned, zeroed `PROCESS_MEMORY_COUNTERS_EX` of the size
        // given, which the call fills; `handle` is valid (checked above).
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        counters.cb = size;
        let done = unsafe {
            K32GetProcessMemoryInfo(
                handle,
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX)
                    .cast::<PROCESS_MEMORY_COUNTERS>(),
                size,
            )
        };
        if !own {
            // SAFETY: a handle this function opened, closed once.
            unsafe { CloseHandle(handle) };
        }
        (done != 0).then_some(counters.PrivateUsage as u64)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod own {
    pub fn footprint(_pid: u32) -> Option<u64> {
        None
    }
}

// The system calls (and only they) are unsafe here; each says why it is sound.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    use super::ThreadTime;
    use std::time::Duration;

    unsafe extern "C" {
        // Not in `libc`: releases a reference to a Mach port.
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }

    #[allow(deprecated)] // libc points to the mach2 crate; these calls are stable.
    pub fn threads() -> Vec<ThreadTime> {
        let mut list: libc::thread_act_array_t = std::ptr::null_mut();
        let mut count: libc::mach_msg_type_number_t = 0;
        // SAFETY: `task_threads` fills `list` with `count` thread ports of this task; both
        // the ports and the array are released below.
        let task = unsafe { libc::mach_task_self() };
        if unsafe { libc::task_threads(task, &mut list, &mut count) } != libc::KERN_SUCCESS {
            return Vec::new();
        }
        let ports = unsafe { std::slice::from_raw_parts(list, count as usize) };
        let mut out = Vec::with_capacity(ports.len());
        for &port in ports {
            let mut info: libc::thread_basic_info = unsafe { std::mem::zeroed() };
            let mut size = (std::mem::size_of::<libc::thread_basic_info>()
                / std::mem::size_of::<libc::natural_t>())
                as libc::mach_msg_type_number_t;
            // SAFETY: `info` is a `thread_basic_info` and `size` its size in `natural_t`s.
            let read = unsafe {
                libc::thread_info(
                    port,
                    libc::THREAD_BASIC_INFO as libc::thread_flavor_t,
                    (&mut info as *mut libc::thread_basic_info).cast(),
                    &mut size,
                )
            };
            if read == libc::KERN_SUCCESS {
                let time = |t: libc::time_value_t| {
                    Duration::from_secs(t.seconds as u64)
                        + Duration::from_micros(t.microseconds as u64)
                };
                out.push(ThreadTime {
                    id: u64::from(port),
                    name: name_of(port),
                    cpu: time(info.user_time) + time(info.system_time),
                });
            }
            // SAFETY: each port from `task_threads` holds a reference to release.
            unsafe { mach_port_deallocate(task, port) };
        }
        // SAFETY: the array `task_threads` allocated in this task's address space.
        unsafe {
            libc::vm_deallocate(
                task,
                list as libc::vm_address_t,
                (count as usize * std::mem::size_of::<libc::thread_act_t>()) as libc::vm_size_t,
            )
        };
        out
    }

    /// The pthread name of Mach thread `port`.
    fn name_of(port: libc::thread_act_t) -> String {
        // SAFETY: a thread of this task; the name is written into `buf` (NUL-terminated).
        let thread = unsafe { libc::pthread_from_mach_thread_np(port) };
        if thread == 0 as libc::pthread_t {
            return String::new();
        }
        let mut buf = [0 as libc::c_char; 64];
        if unsafe { libc::pthread_getname_np(thread, buf.as_mut_ptr(), buf.len()) } != 0 {
            return String::new();
        }
        // SAFETY: NUL-terminated by `pthread_getname_np`.
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod platform {
    use super::ThreadTime;
    use std::time::Duration;

    pub fn threads() -> Vec<ThreadTime> {
        // SAFETY: a plain query of the clock's ticks per second.
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
        let Ok(tasks) = std::fs::read_dir("/proc/self/task") else { return Vec::new() };
        tasks
            .flatten()
            .filter_map(|task| {
                let id: u64 = task.file_name().to_str()?.parse().ok()?;
                let stat = std::fs::read_to_string(task.path().join("stat")).ok()?;
                // The name sits in parentheses (and may hold spaces): fields follow it.
                let (head, rest) = stat.rsplit_once(')')?;
                let name = head.split_once('(').map(|(_, n)| n.to_owned()).unwrap_or_default();
                let fields: Vec<&str> = rest.split_whitespace().collect();
                // utime and stime: the 14th and 15th fields, the 12th and 13th after ')'.
                let used: u64 =
                    fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
                Some(ThreadTime { id, name, cpu: Duration::from_micros(used * 1_000_000 / ticks) })
            })
            .collect()
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use super::ThreadTime;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE, LocalFree};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, GetThreadDescription, GetThreadTimes, OpenThread,
        THREAD_QUERY_LIMITED_INFORMATION,
    };

    pub fn threads() -> Vec<ThreadTime> {
        // SAFETY: a snapshot of the system's threads, closed below; entries are plain data.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let me = unsafe { GetCurrentProcessId() };
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut out = Vec::new();
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == me
                && let Some(thread) = read(entry.th32ThreadID)
            {
                out.push(thread);
            }
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        out
    }

    fn read(id: u32) -> Option<ThreadTime> {
        // SAFETY: a handle to a thread of this process, closed below.
        let handle = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, id) };
        if handle.is_null() {
            return None;
        }
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let timed =
            unsafe { GetThreadTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
        let mut name = String::new();
        let mut text: windows_sys::core::PWSTR = std::ptr::null_mut();
        // SAFETY: the description is allocated by the system and freed with LocalFree.
        if unsafe { GetThreadDescription(handle, &mut text) } >= 0 && !text.is_null() {
            let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
            name = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
            unsafe { LocalFree(text.cast()) };
        }
        unsafe { CloseHandle(handle) };
        if timed == 0 {
            return None;
        }
        // FILETIME: 100 ns units.
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        Some(ThreadTime {
            id: u64::from(id),
            name,
            cpu: Duration::from_nanos((ticks(kernel) + ticks(user)) * 100),
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod platform {
    pub fn threads() -> Vec<super::ThreadTime> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
