//! The CPU time the current thread has used: what a piece of work costs is the difference
//! before and after it, on the thread doing it (another thread's work never counts, unlike
//! a wall clock). For the Task Manager's per-project CPU, measured by the modules.

use std::time::Duration;

/// The CPU time (user + system) the current thread has used so far; `None` where the
/// system can't say.
pub fn thread_time() -> Option<Duration> {
    imp::thread_time()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod imp {
    use std::ffi::{c_int, c_long};
    use std::time::Duration;

    #[repr(C)]
    struct Timespec {
        tv_sec: c_long,
        tv_nsec: c_long,
    }

    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn clock_gettime(clock: c_int, time: *mut Timespec) -> c_int;
    }

    #[cfg(target_os = "macos")]
    const CLOCK_THREAD_CPUTIME_ID: c_int = 16;
    #[cfg(target_os = "linux")]
    const CLOCK_THREAD_CPUTIME_ID: c_int = 3;

    pub fn thread_time() -> Option<Duration> {
        let mut time = Timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: a libc function writing only into `time`, a valid, owned `timespec`.
        #[allow(unsafe_code)]
        let done = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut time) };
        (done == 0).then(|| Duration::new(time.tv_sec.max(0) as u64, time.tv_nsec.max(0) as u32))
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::time::Duration;

    #[repr(C)]
    #[derive(Default)]
    struct Filetime {
        low: u32,
        high: u32,
    }

    impl Filetime {
        /// In units of 100 ns.
        fn ticks(&self) -> u64 {
            (u64::from(self.high) << 32) | u64::from(self.low)
        }
    }

    #[allow(unsafe_code)]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut c_void;
        fn GetThreadTimes(
            thread: *mut c_void,
            creation: *mut Filetime,
            exit: *mut Filetime,
            kernel: *mut Filetime,
            user: *mut Filetime,
        ) -> i32;
    }

    pub fn thread_time() -> Option<Duration> {
        let (mut creation, mut exit, mut kernel, mut user) = Default::default();
        // SAFETY: kernel32 functions; the current thread's pseudo-handle needs no closing,
        // and the four out-parameters are valid, owned `FILETIME`s.
        #[allow(unsafe_code)]
        let done = unsafe {
            GetThreadTimes(GetCurrentThread(), &mut creation, &mut exit, &mut kernel, &mut user)
        };
        let _ = (creation, exit);
        (done != 0).then(|| Duration::from_nanos((kernel.ticks() + user.ticks()) * 100))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    pub fn thread_time() -> Option<std::time::Duration> {
        None
    }
}

#[cfg(test)]
#[path = "../tests/unit/cpu.rs"]
mod tests;
