//! The first word of a program's command (`argv[0]`), read on its own: nothing after it is
//! kept, its arguments may hold secrets. macOS: `KERN_PROCARGS2` (the whole block read into a
//! buffer of ours, wiped once the word is copied out); Linux: `/proc/<pid>/cmdline` up to its
//! first NUL. Windows has no such word apart from the program's file (its name says it).

/// `argv[0]` of program `pid`, as text (lossy); `None` when it can't be read or is empty.
pub fn first_word(pid: u32) -> Option<String> {
    platform::first_word(pid).filter(|w| !w.is_empty())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    /// At most this much of the word is read (a title a program wrote for itself is longer).
    const MAX: usize = 4096;

    pub fn first_word(pid: u32) -> Option<String> {
        let pid = libc::c_int::try_from(pid).ok()?;
        let mut max: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `max` is an int and its size is said; nothing is written to the system.
        let read = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut max as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let max = usize::try_from(max).ok().filter(|m| read == 0 && *m > 4)?;
        let mut buffer = vec![0u8; max];
        let mut size = buffer.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        // SAFETY: the buffer is ours and its length is said; the call writes at most that.
        let read = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buffer.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let word = (read == 0).then(|| word(&buffer[..size.min(buffer.len())])).flatten();
        // The arguments and the environment read with it don't stay in memory.
        buffer.fill(0);
        std::hint::black_box(&buffer);
        word
    }

    /// The word in a `KERN_PROCARGS2` block: its count of arguments, the program's path, the
    /// NULs padding it, then `argv[0]`. An empty `argv[0]` can't be told from that padding,
    /// so a word that reads as an argument (`--token=…`, a URL) is not taken for it.
    pub(super) fn word(block: &[u8]) -> Option<String> {
        let argc = i32::from_ne_bytes(block.get(..4)?.try_into().ok()?);
        if argc < 1 {
            return None;
        }
        let rest = &block[4..];
        let rest = &rest[rest.iter().position(|&b| b == 0)?..];
        let rest = &rest[rest.iter().position(|&b| b != 0)?..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len()).min(MAX);
        let word = String::from_utf8_lossy(&rest[..end]).into_owned();
        let argument = word.starts_with("--") || word.contains(['=', '@']) || word.contains("://");
        (!argument).then_some(word)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io::{BufRead, Read};

    pub fn first_word(pid: u32) -> Option<String> {
        let file = std::fs::File::open(format!("/proc/{pid}/cmdline")).ok()?;
        let mut word = Vec::new();
        std::io::BufReader::new(file.take(4096)).read_until(0, &mut word).ok()?;
        if word.last() == Some(&0) {
            word.pop();
        }
        Some(String::from_utf8_lossy(&word).into_owned())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    pub fn first_word(_pid: u32) -> Option<String> {
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "../tests/unit/first_word.rs"]
mod tests;
