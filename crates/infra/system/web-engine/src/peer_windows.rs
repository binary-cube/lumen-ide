//! Windows: whether a process runs as the same user as this one (its token's user's SID,
//! compared with ours) — the web engine's pipe answered by another user's program is never
//! trusted; a thread's blocked read or write on a pipe cancelled (a pipe takes no timeout);
//! and the one wrapper of a handle of ours ([`OwnedHandle`]: a token, a process, a frame's
//! texture duplicated into this process).

#![allow(unsafe_code)]

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// A handle of ours, closed when dropped (none: `0`). Its value as the protocol carries it
/// (a frame's texture's handle, duplicated into this process by the host).
#[derive(Debug)]
pub struct OwnedHandle(pub u64);

impl OwnedHandle {
    /// A handle this process just opened, taken over.
    fn adopt(handle: HANDLE) -> Self {
        Self(handle as u64)
    }

    fn raw(&self) -> HANDLE {
        self.0 as HANDLE
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: a handle of this process, held only by this value, closed once.
            unsafe { CloseHandle(self.raw()) };
        }
    }
}

/// The token's user (a `TOKEN_USER` and the SID after it), in words aligned for it.
fn user_of(process: HANDLE) -> Option<Vec<u64>> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: a process handle with query access; `token` receives a handle we close.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return None;
    }
    let token = OwnedHandle::adopt(token);
    let mut needed = 0u32;
    // SAFETY: asked for the size first (the call fails, saying it).
    unsafe {
        GetTokenInformation(token.raw(), TokenUser, std::ptr::null_mut(), 0, &raw mut needed)
    };
    // Words, not bytes: the `TOKEN_USER` at its start is read in place (8-byte aligned).
    let words = (needed as usize).div_ceil(std::mem::size_of::<u64>());
    let mut buffer = vec![0u64; words];
    let room = u32::try_from(words * std::mem::size_of::<u64>()).ok()?;
    // SAFETY: a buffer of at least the size asked for.
    let ok = unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            room,
            &raw mut needed,
        )
    };
    let whole = needed as usize >= std::mem::size_of::<TOKEN_USER>();
    (ok != 0 && whole).then_some(buffer)
}

/// Whether process `pid` runs as this process's user (`false` when it can't be told).
pub fn same_user(pid: u32) -> bool {
    // SAFETY: opening a process by its id with query access only, closed when dropped.
    let theirs =
        OwnedHandle::adopt(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
    if theirs.0 == 0 {
        return false;
    }
    // SAFETY: the pseudo handle of this process (never closed).
    let (Some(ours), Some(theirs)) =
        (user_of(unsafe { GetCurrentProcess() }), user_of(theirs.raw()))
    else {
        return false;
    };
    let sid = |buffer: &Vec<u64>| {
        // SAFETY: `buffer` holds a `TOKEN_USER` at its start (checked by its size), aligned
        // for it (words of 8 bytes), its SID after it.
        unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    };
    // SAFETY: two SIDs living in their buffers for the call.
    unsafe { EqualSid(sid(&ours), sid(&theirs)) != 0 }
}

/// How a thread's blocked call is cancelled: tried `pause` apart until the thread ended,
/// for `limit` at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancel {
    /// The pause between two tries (the thread may not be in its call yet when tried, or
    /// enter another after).
    pub pause: std::time::Duration,
    /// The longest it's tried.
    pub limit: std::time::Duration,
}

/// `threads`' blocked reads or writes cancelled (`CancelSynchronousIo`), tried as `cancel`
/// says until every one ended (all of them at each try: one that stays doesn't hold back
/// the others).
pub fn cancel_blocked(threads: &[std::thread::JoinHandle<()>], cancel: Cancel) {
    use std::os::windows::io::AsRawHandle as _;
    let Cancel { pause, limit } = cancel;
    let since = std::time::Instant::now();
    while since.elapsed() < limit {
        let mut running = threads.iter().filter(|thread| !thread.is_finished()).peekable();
        if running.peek().is_none() {
            return;
        }
        for thread in running {
            // SAFETY: the thread's own handle, held by `thread` for the call; a thread not in
            // a blocking call is left as it is.
            unsafe { windows_sys::Win32::System::IO::CancelSynchronousIo(thread.as_raw_handle()) };
        }
        std::thread::sleep(pause);
    }
}
