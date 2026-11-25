//! macOS: who is at the other end of a local socket, as the system tells it — the process
//! and its id's version (`LOCAL_PEERTOKEN`'s audit token). The socket's own credentials
//! carry no process there, so the host's word for its process is checked against this.

#![allow(unsafe_code)]

use std::os::fd::AsRawFd as _;

use interprocess::local_socket::Stream;

/// The process at the other end of `stream`, and its id's version (Mach messages it sends
/// carry the same audit token).
pub fn peer(stream: &Stream) -> std::io::Result<mach_ports::Peer> {
    let Stream::UdSocket(socket) = stream;
    let fd = socket.inner().as_raw_fd();
    let mut token = [0u32; 8];
    let mut size = libc::socklen_t::try_from(std::mem::size_of_val(&token)).unwrap_or(0);
    // SAFETY: a socket of ours; `token` has room for the audit token asked for, `size` says
    // how much.
    let done = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    if done != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if size as usize != std::mem::size_of_val(&token) {
        return Err(std::io::Error::other("the peer's audit token isn't whole"));
    }
    Ok(mach_ports::Peer::from_audit_token(&token))
}
