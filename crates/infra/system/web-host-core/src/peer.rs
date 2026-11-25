//! Who is at the other end of a client's connection, as the system tells it, and how the
//! host cuts a connection from its side.
//!
//! - The user: on Unix the peer's effective user must be the host's (the socket's folder is
//!   the user's private one, but a folder left open must not let another user in); on
//!   Windows the pipe is open to its owner only (`channel::owner_only`).
//! - The process: where the system gives the peer's process (Linux, macOS, Windows), the
//!   one an app says it is in its greeting must be it — the host duplicates its frames'
//!   handles into that process.

#![allow(unsafe_code)]

use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::StreamCommon as _;

/// Whether the peer runs as the host's user.
pub(crate) fn same_user(stream: &Stream) -> bool {
    #[cfg(unix)]
    {
        let Ok(creds) = stream.peer_creds() else { return false };
        // SAFETY: `geteuid` has no preconditions and never fails.
        let ours = unsafe { libc::geteuid() };
        creds.euid() == Some(ours)
    }
    #[cfg(windows)]
    {
        // The pipe's security descriptor lets its owner (and the system) only.
        let _ = stream;
        true
    }
}

/// The peer's process, as the system tells it (`None`: it doesn't, here).
pub(crate) fn pid(stream: &Stream) -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd as _;
        let Stream::UdSocket(socket) = stream;
        let fd = socket.inner().as_raw_fd();
        let mut pid: libc::pid_t = 0;
        let mut length = libc::socklen_t::try_from(std::mem::size_of::<libc::pid_t>()).ok()?;
        // SAFETY: `LOCAL_PEERPID` writes the peer's `pid_t` into `pid`, `length` bytes at most;
        // the socket is borrowed for the call.
        let code = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&raw mut pid).cast(),
                &raw mut length,
            )
        };
        (code == 0).then(|| u32::try_from(pid).ok()).flatten()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // The peer's process id as the system gives it (`u32` on Windows, `pid_t` elsewhere).
        #[allow(clippy::useless_conversion)]
        stream.peer_creds().ok()?.pid().and_then(|pid| u32::try_from(pid).ok())
    }
}

/// The connection cut from the host's side: its reads and writes end at once, on both ends
/// (the app sees it closed, the client's reader thread wakes).
pub(crate) fn cut(stream: &Stream) {
    #[cfg(unix)]
    {
        let Stream::UdSocket(socket) = stream;
        let _ = socket.inner().shutdown(std::net::Shutdown::Both);
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsHandle as _, AsRawHandle as _};
        let Stream::NamedPipe(pipe) = stream;
        let handle = pipe.inner().as_handle().as_raw_handle();
        // SAFETY: the pipe's server handle, borrowed for the call: disconnecting it closes the
        // client's end and fails what's pending on ours.
        unsafe { windows_sys::Win32::System::Pipes::DisconnectNamedPipe(handle) };
    }
}
