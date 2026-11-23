//! The components' tests' allocator: the system's, counting what one thread allocates while
//! it's asked to, so a test can say a frame drawn again allocates nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// How many allocations `work` made on this thread.
pub fn during(work: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    work();
    COUNTING.with(|on| on.set(false));
    ALLOCATIONS.with(Cell::get)
}

fn counted() {
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

/// The system's allocator, counting.
struct Counting;

// SAFETY: every call goes to the system's allocator as it came; counting only reads and
// writes this thread's own cells (const-initialized: touching them allocates nothing).
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        counted();
        // SAFETY: the caller's layout, handed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        counted();
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: a pointer this allocator gave, with its layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        counted();
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The counter counts: one allocation is seen as one.
#[test]
fn the_counter_counts() {
    let mut kept = Vec::new();
    assert_eq!(during(|| kept = Vec::<u8>::with_capacity(16)), 1);
    assert_eq!(during(|| kept.clear()), 0);
}
