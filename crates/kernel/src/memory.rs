//! Giving freed memory back to the system. After a large buffer is freed (a decoded image,
//! a purged cache), the allocator may keep its pages for reuse: they still count in the
//! process's footprint. This asks it to return what it can.

/// The app's own allocator's way of returning its free pages (mimalloc's), when it has one:
/// the system's calls below don't reach them.
static ALLOCATOR_RELEASE: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Sets how the global allocator returns its free pages (once, at startup).
pub fn set_allocator_release(release: fn()) {
    let _ = ALLOCATOR_RELEASE.set(release);
}

/// Returns the allocator's free pages to the system (the app's allocator's, then macOS',
/// Linux glibc's, Windows' process heap; a no-op elsewhere).
/// Cheap enough to call after freeing something large, not every frame.
pub fn release_free() {
    if let Some(release) = ALLOCATOR_RELEASE.get() {
        release();
    }
    #[cfg(target_os = "macos")]
    {
        #[allow(unsafe_code)]
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: a libSystem function; a null zone means every zone and a goal of 0 means
        // "as much as possible". It touches only the allocator's own free lists.
        #[allow(unsafe_code)]
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
    #[cfg(windows)]
    {
        #[allow(unsafe_code)]
        unsafe extern "system" {
            fn GetProcessHeap() -> *mut std::ffi::c_void;
            fn HeapCompact(heap: *mut std::ffi::c_void, flags: u32) -> usize;
        }
        // SAFETY: kernel32 functions; the process heap (the one Rust allocates from) always
        // exists, and compacting only coalesces and decommits its free blocks.
        #[allow(unsafe_code)]
        unsafe {
            HeapCompact(GetProcessHeap(), 0);
        }
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        #[allow(unsafe_code)]
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: a glibc function that only releases free heap memory.
        #[allow(unsafe_code)]
        unsafe {
            malloc_trim(0);
        }
    }
}

/// The heap a vector's buffer takes (bytes; not what its items point to).
pub fn vec_bytes<T>(items: &Vec<T>) -> u64 {
    (items.capacity() * std::mem::size_of::<T>()) as u64
}

/// The heap a hash map's table takes (bytes; its keys and values inline, a control byte
/// each — not what they point to).
pub fn map_bytes<K, V, S>(map: &std::collections::HashMap<K, V, S>) -> u64 {
    (map.capacity() * (std::mem::size_of::<(K, V)>() + 1)) as u64
}

/// A global allocator counting the heap's live bytes (allocated and not freed yet) around
/// another one: what the app's data holds, apart from what the allocator keeps for reuse
/// and from the code, the GPU, the system's libraries. Installed as `#[global_allocator]`,
/// it makes [`live_heap`] answer. Cheap enough to stay on: each thread counts on a slot of
/// its own (no two threads fighting for one counter), the slots summed when asked.
#[derive(Debug)]
pub struct Counting<A>(pub A);

/// How many slots the threads count on (a thread's is its number modulo this).
const SLOTS: usize = 64;

/// A slot, alone on its cache line.
#[repr(align(128))]
struct Slot(std::sync::atomic::AtomicIsize);

static LIVE: [Slot; SLOTS] = [const { Slot(std::sync::atomic::AtomicIsize::new(0)) }; SLOTS];
static COUNTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The next thread's slot.
static NEXT_SLOT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

std::thread_local! {
    /// This thread's slot (`usize::MAX`: not given yet). No destructor: the allocator may
    /// use it while the thread ends.
    static THREAD_SLOT: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// `bytes` more (or fewer) live, on this thread's slot.
fn count(bytes: isize) {
    use std::sync::atomic::Ordering::Relaxed;
    let slot = THREAD_SLOT
        .try_with(|slot| {
            if slot.get() == usize::MAX {
                slot.set(NEXT_SLOT.fetch_add(1, Relaxed) % SLOTS);
            }
            slot.get()
        })
        .unwrap_or(0);
    LIVE[slot].0.fetch_add(bytes, Relaxed);
}

/// The heap's live bytes (`None`: no [`Counting`] allocator installed).
pub fn live_heap() -> Option<usize> {
    use std::sync::atomic::Ordering::Relaxed;
    if !COUNTED.load(Relaxed) {
        return None;
    }
    let sum: isize = LIVE.iter().map(|slot| slot.0.load(Relaxed)).sum();
    Some(sum.max(0) as usize)
}

/// `size` as counted (a block's size always fits).
fn signed(size: usize) -> isize {
    isize::try_from(size).unwrap_or(isize::MAX)
}

/// The allocator counts: told once (a read first, a write only the first time — a write at
/// each allocation, on every thread, fought over one cache line: 8 threads took 25 times
/// longer).
#[inline]
fn counted() {
    if !COUNTED.load(std::sync::atomic::Ordering::Relaxed) {
        COUNTED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

// SAFETY: every call goes to the wrapped allocator unchanged; only the sizes are counted.
#[allow(unsafe_code)]
unsafe impl<A: std::alloc::GlobalAlloc> std::alloc::GlobalAlloc for Counting<A> {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: the caller's layout, passed on.
        let ptr = unsafe { self.0.alloc(layout) };
        if !ptr.is_null() {
            count(signed(layout.size()));
            counted();
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: as above.
        let ptr = unsafe { self.0.alloc_zeroed(layout) };
        if !ptr.is_null() {
            count(signed(layout.size()));
            counted();
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: a pointer this allocator gave, with its layout.
        unsafe { self.0.dealloc(ptr, layout) };
        count(-signed(layout.size()));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new: usize) -> *mut u8 {
        // SAFETY: a pointer this allocator gave, with its layout, and the size asked.
        let moved = unsafe { self.0.realloc(ptr, layout, new) };
        if !moved.is_null() {
            count(signed(new) - signed(layout.size()));
        }
        moved
    }
}
