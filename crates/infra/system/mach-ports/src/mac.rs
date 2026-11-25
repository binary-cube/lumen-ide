//! The Mach calls (from the system's C library), and the one message we send: a port
//! descriptor and two numbers.

#![allow(unsafe_code)]

use std::ffi::{CString, c_char};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

type Port = u32;
type KernReturn = i32;

/// The Mach message we send: a port and two numbers (`#pragma pack(4)`, as the system's).
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
struct Message {
    header: Header,
    descriptors: u32,
    port: PortDescriptor,
    first: u64,
    second: u64,
}

/// As received: the message, then the audit trailer the receive asked for.
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
struct Received {
    message: Message,
    trailer: AuditTrailer,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Header {
    bits: u32,
    size: u32,
    remote_port: Port,
    local_port: Port,
    voucher_port: Port,
    id: i32,
}

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct PortDescriptor {
    name: Port,
    pad1: u32,
    pad2: u16,
    disposition: u8,
    kind: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AuditTrailer {
    kind: u32,
    size: u32,
    sequence: u32,
    sender: [u32; 2],
    /// The audit token ([`AUDIT_PID`], [`AUDIT_PID_VERSION`]).
    audit: [u32; 8],
}

/// The audit token's word holding the sender's process id.
const AUDIT_PID: usize = 5;
/// The audit token's word holding the process id's version (a process id reused by a later
/// process has another).
const AUDIT_PID_VERSION: usize = 7;
/// `MACH_MSG_TRAILER_FORMAT_0`: the only trailer format there is.
const TRAILER_FORMAT_0: u32 = 0;

/// Who a message must come from: a process, and the version of its id when it's known
/// (read from the sender's audit token where the two met, e.g. a socket's
/// `LOCAL_PEERTOKEN`) — so a process that took a dead sender's id is refused too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub pid: u32,
    /// `None`: only the process id is compared (its version unknown).
    pub pid_version: Option<u32>,
}

impl Peer {
    /// The process an audit token names (a socket's `LOCAL_PEERTOKEN`, a message's audit
    /// trailer), with its id's version.
    pub fn from_audit_token(audit: &[u32; 8]) -> Self {
        Self { pid: audit[AUDIT_PID], pid_version: Some(audit[AUDIT_PID_VERSION]) }
    }

    /// Whether the audit token `audit` is this peer's.
    fn sent(&self, audit: &[u32; 8]) -> bool {
        audit[AUDIT_PID] == self.pid
            && self.pid_version.is_none_or(|version| audit[AUDIT_PID_VERSION] == version)
    }
}

const KERN_SUCCESS: KernReturn = 0;
const MACH_SEND_MSG: i32 = 0x0000_0001;
const MACH_RCV_MSG: i32 = 0x0000_0002;
const MACH_SEND_TIMEOUT: i32 = 0x0000_0010;
const MACH_RCV_TIMEOUT: i32 = 0x0000_0100;
/// `MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT)`.
const MACH_RCV_TRAILER_AUDIT: i32 = 3 << 24;
const MACH_RCV_TIMED_OUT: KernReturn = 0x1000_4003;
/// The receive right is gone (the inbox can't receive any more).
const MACH_RCV_INVALID_NAME: KernReturn = 0x1000_4002;
const MACH_RCV_PORT_DIED: KernReturn = 0x1000_4009;
/// `MACH_PORT_RIGHT_RECEIVE`.
const RIGHT_RECEIVE: u32 = 1;
const MACH_MSGH_BITS_COMPLEX: u32 = 0x8000_0000;
const MACH_MSG_TYPE_COPY_SEND: u32 = 19;
const MACH_MSG_TYPE_MOVE_SEND: u8 = 17;
const MACH_MSG_PORT_DESCRIPTOR: u8 = 0;
/// The message's id (ours: anything else is dropped).
const MESSAGE_ID: i32 = 0x7765_6266;
/// A wake's id ([`Waker`]): a message with nothing in it, from this process only.
const WAKE_ID: i32 = 0x7765_6277;
/// `MACH_MSG_TYPE_MAKE_SEND`: a send right made from our receive right.
const MAKE_SEND: u32 = 20;
/// How long a send waits for room in the receiver's queue.
const SEND_WAIT: Duration = Duration::from_millis(100);

unsafe extern "C" {
    static bootstrap_port: Port;
    static mach_task_self_: Port;
    fn bootstrap_check_in(bp: Port, name: *const c_char, service: *mut Port) -> KernReturn;
    fn bootstrap_look_up(bp: Port, name: *const c_char, service: *mut Port) -> KernReturn;
    fn mach_msg(
        message: *mut Header,
        option: i32,
        send_size: u32,
        receive_size: u32,
        receive_name: Port,
        timeout: u32,
        notify: Port,
    ) -> KernReturn;
    fn mach_port_deallocate(task: Port, name: Port) -> KernReturn;
    fn mach_port_mod_refs(task: Port, name: Port, right: u32, delta: i32) -> KernReturn;
    fn mach_port_insert_right(task: Port, name: Port, right: Port, kind: u32) -> KernReturn;
    fn mach_msg_destroy(message: *mut Header);
}

fn task() -> Port {
    // SAFETY: the process's own task port, set by the system before main.
    unsafe { mach_task_self_ }
}

fn failed(call: &str, code: KernReturn) -> io::Error {
    io::Error::other(format!("{call} failed ({code:#x})"))
}

fn millis(wait: Duration) -> u32 {
    u32::try_from(wait.as_millis()).unwrap_or(u32::MAX)
}

/// A send right of this process (let go when dropped).
#[derive(Debug)]
pub struct Right(Port);

impl Right {
    /// A right this process holds (taken over: let go when this is dropped).
    pub fn adopt(port: u32) -> Self {
        Self(port)
    }

    /// The right's name in this process (still owned by this value).
    pub fn port(&self) -> u32 {
        self.0
    }
}

impl Drop for Right {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: a right of this task, let go once.
            unsafe { mach_port_deallocate(task(), self.0) };
        }
    }
}

/// A port received with its two numbers.
#[derive(Debug)]
pub struct Delivery {
    pub first: u64,
    pub second: u64,
    pub right: Right,
}

/// The receiving end: a service of this process.
#[derive(Debug)]
pub struct Inbox {
    port: Port,
    name: String,
    /// A wake sent and not received yet (one at a time: wakes never fill the queue).
    woken: Arc<AtomicBool>,
}

/// Ends an [`Inbox::receive`] waiting on another thread (it returns `Ok(None)`): a message
/// with nothing in it, sent to the inbox's own port. Wakes sent before the receive took the
/// last are one.
#[derive(Debug)]
pub struct Waker {
    right: Right,
    woken: Arc<AtomicBool>,
}

impl Waker {
    /// The receive waiting woken (or the next one, if none waits now).
    pub fn wake(&self) {
        if self.woken.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut header = Header {
            bits: MACH_MSG_TYPE_COPY_SEND,
            size: u32::try_from(std::mem::size_of::<Header>()).unwrap_or(0),
            remote_port: self.right.0,
            local_port: 0,
            voucher_port: 0,
            id: WAKE_ID,
        };
        let size = header.size;
        // SAFETY: a message of `size` bytes (a header only, no rights) to our own port.
        let code = unsafe {
            mach_msg(&raw mut header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, size, 0, 0, 0, 0)
        };
        // Not sent (its queue full: the receive returns anyway, or the inbox gone): the next
        // wake tries again.
        if code != KERN_SUCCESS {
            self.woken.store(false, Ordering::Release);
        }
    }
}

impl Inbox {
    /// A service checked in under `name` (unique: the process's id and a random part).
    pub fn check_in(name: &str) -> io::Result<Self> {
        let c = CString::new(name).map_err(io::Error::other)?;
        let mut port: Port = 0;
        // SAFETY: the bootstrap port is the process's; `port` receives the new right.
        let code = unsafe { bootstrap_check_in(bootstrap_port, c.as_ptr(), &raw mut port) };
        if code != KERN_SUCCESS {
            return Err(failed("bootstrap_check_in", code));
        }
        Ok(Self { port, name: name.to_owned(), woken: Arc::default() })
    }

    /// What wakes a [`Self::receive`] waiting on another thread.
    pub fn waker(&self) -> io::Result<Waker> {
        // SAFETY: a send right made from our own receive right, let go by the `Right`.
        let code = unsafe { mach_port_insert_right(task(), self.port, self.port, MAKE_SEND) };
        if code != KERN_SUCCESS {
            return Err(failed("mach_port_insert_right", code));
        }
        Ok(Waker { right: Right(self.port), woken: Arc::clone(&self.woken) })
    }

    /// The service's name (told to the sender).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The next port sent by `sender` (at most `wait` in all, `None`: as long as it takes;
    /// `Ok(None)` if none came in time, or a [`Waker`] woke it). One from anyone else, or not
    /// of our shape, is dropped and its rights let go; dropped ones never stretch the wait (a
    /// flood of them returns at the deadline).
    pub fn receive(&self, sender: Peer, wait: Option<Duration>) -> io::Result<Option<Delivery>> {
        let deadline = wait.map(|wait| Instant::now() + wait);
        loop {
            let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            let timed = if left.is_some() { MACH_RCV_TIMEOUT } else { 0 };
            let mut buffer = Received {
                message: Message {
                    header: Header::default(),
                    descriptors: 0,
                    port: PortDescriptor::default(),
                    first: 0,
                    second: 0,
                },
                trailer: AuditTrailer::default(),
            };
            let size = u32::try_from(std::mem::size_of::<Received>()).unwrap_or(0);
            // SAFETY: a receive into a buffer of `size` bytes, on our own receive right.
            let code = unsafe {
                mach_msg(
                    (&raw mut buffer).cast::<Header>(),
                    MACH_RCV_MSG | timed | MACH_RCV_TRAILER_AUDIT,
                    0,
                    size,
                    self.port,
                    left.map_or(0, millis),
                    0,
                )
            };
            match code {
                KERN_SUCCESS => {}
                MACH_RCV_TIMED_OUT => return Ok(None),
                // The right itself is gone: nothing more will come.
                MACH_RCV_INVALID_NAME | MACH_RCV_PORT_DIED => {
                    return Err(failed("mach_msg (receive)", code));
                }
                // One message that couldn't be taken (too large, broken: the system dropped
                // it): the next one, within what's left of the wait.
                _ if left.is_some_and(|l| l.is_zero()) => return Ok(None),
                _ => continue,
            }
            match self.woke(&mut buffer) {
                Some(true) => return Ok(None),
                // A wake's shape from anyone else: dropped, any right in its header let go.
                Some(false) => destroy(&mut buffer),
                None => {
                    if let Some(delivery) = ours(&mut buffer, sender) {
                        return Ok(Some(delivery));
                    }
                }
            }
            if left.is_some_and(|l| l.is_zero()) {
                return Ok(None);
            }
        }
    }
}

impl Inbox {
    /// Whether the message just received into `buffer` is a wake (`None`: it isn't one),
    /// and from this process (the next wake is sent again from then on).
    fn woke(&self, buffer: &mut Received) -> Option<bool> {
        let header = buffer.message.header;
        let empty = std::mem::size_of::<Header>();
        let shaped = header.id == WAKE_ID
            && header.bits & MACH_MSGH_BITS_COMPLEX == 0
            && header.size as usize == empty;
        if !shaped {
            return None;
        }
        // The trailer follows the message, here right after its header.
        let whole = empty + std::mem::size_of::<AuditTrailer>() <= std::mem::size_of::<Received>();
        // SAFETY: `buffer` holds `size_of::<Received>()` bytes, the trailer read within them.
        let trailer = whole.then(|| unsafe {
            (&raw const *buffer).cast::<u8>().add(empty).cast::<AuditTrailer>().read_unaligned()
        });
        let me = Peer { pid: std::process::id(), pid_version: None };
        let ours = trailer.is_some_and(|t| {
            t.kind == TRAILER_FORMAT_0
                && t.size as usize >= std::mem::size_of::<AuditTrailer>()
                && me.sent(&t.audit)
        });
        if ours {
            self.woken.store(false, Ordering::Release);
        }
        Some(ours)
    }
}

/// A message just received into `buffer` and refused: destroyed whole (the rights in its
/// header and its body, and its memory, freed).
fn destroy(buffer: &mut Received) {
    // SAFETY: a message just received into `buffer`, destroyed once (nothing of it kept).
    unsafe { mach_msg_destroy((&raw mut *buffer).cast::<Header>()) };
}

/// The message just received into `buffer`, if it's ours and from `sender`; anything else
/// destroyed whole (its rights and memory freed), never guessed at.
fn ours(buffer: &mut Received, sender: Peer) -> Option<Delivery> {
    let message = buffer.message;
    let header = message.header;
    let port = message.port;
    // Exactly ours: one port descriptor carrying a send right, at our size.
    let shaped = header.id == MESSAGE_ID
        && header.bits & MACH_MSGH_BITS_COMPLEX != 0
        && header.size as usize == std::mem::size_of::<Message>()
        && message.descriptors == 1
        && port.kind == MACH_MSG_PORT_DESCRIPTOR
        && port.disposition == MACH_MSG_TYPE_MOVE_SEND;
    if !shaped {
        destroy(buffer);
        return None;
    }
    // The trailer follows the message (our size, already 4-aligned): the audit trailer asked
    // for, whole, or the sender can't be told (destroyed: its port's right and its header's
    // let go).
    let trailer = buffer.trailer;
    let audited = trailer.kind == TRAILER_FORMAT_0
        && trailer.size as usize >= std::mem::size_of::<AuditTrailer>();
    if !audited || !sender.sent(&trailer.audit) {
        destroy(buffer);
        return None;
    }
    Some(Delivery { first: message.first, second: message.second, right: Right(port.name) })
}

impl Drop for Inbox {
    fn drop(&mut self) {
        // SAFETY: our receive right, let go once (`mach_port_deallocate` frees only send
        // rights): the service goes with it.
        unsafe { mach_port_mod_refs(task(), self.port, RIGHT_RECEIVE, -1) };
    }
}

/// The sending end: another process's service.
#[derive(Debug)]
pub struct Outbox {
    service: Right,
}

impl Outbox {
    /// The service checked in under `name` (by another process).
    pub fn look_up(name: &str) -> io::Result<Self> {
        let c = CString::new(name).map_err(io::Error::other)?;
        let mut port: Port = 0;
        // SAFETY: the bootstrap port is the process's; `port` receives a send right.
        let code = unsafe { bootstrap_look_up(bootstrap_port, c.as_ptr(), &raw mut port) };
        if code != KERN_SUCCESS {
            return Err(failed("bootstrap_look_up", code));
        }
        Ok(Self { service: Right(port) })
    }

    /// `right` sent (moved: the receiver holds it now) with two numbers.
    pub fn send(&self, right: Right, first: u64, second: u64) -> io::Result<()> {
        let mut message = Message {
            header: Header {
                bits: MACH_MSG_TYPE_COPY_SEND | MACH_MSGH_BITS_COMPLEX,
                size: u32::try_from(std::mem::size_of::<Message>()).unwrap_or(0),
                remote_port: self.service.0,
                local_port: 0,
                voucher_port: 0,
                id: MESSAGE_ID,
            },
            descriptors: 1,
            port: PortDescriptor {
                name: right.0,
                pad1: 0,
                pad2: 0,
                disposition: MACH_MSG_TYPE_MOVE_SEND,
                kind: MACH_MSG_PORT_DESCRIPTOR,
            },
            first,
            second,
        };
        let size = message.header.size;
        // SAFETY: a message of `size` bytes to the service's send right; the port's right
        // moves with it (not let go here).
        let code = unsafe {
            mach_msg(
                (&raw mut message).cast::<Header>(),
                MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                size,
                0,
                0,
                millis(SEND_WAIT),
                0,
            )
        };
        if code != KERN_SUCCESS {
            // Not sent: the right is still ours, let go with `right`.
            return Err(failed("mach_msg (send)", code));
        }
        std::mem::forget(right);
        Ok(())
    }
}
