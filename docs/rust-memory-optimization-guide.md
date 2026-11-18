# A Guide to Reducing RAM Use in Rust (Windows)

If your application uses more than 400 MB of RAM on Windows, there are several things to look
at: the build profile, the memory allocator, the data structures and the dependencies used.

The recommendations below can reduce the memory footprint considerably.

---

## 1. The Release Profile (`Cargo.toml`)

By default, Cargo optimizes for speed (`opt-level = 3`), which can grow the binary and the RAM
it uses. This can be changed in `Cargo.toml`:

```toml
[profile.release]
# Optimize for size ("s" keeps a good speed, "z" shrinks the most)
opt-level = "s"

# Link-Time Optimization: removes dead code and optimizes calls across crates
lto = true

# Cuts the code units from 16 to 1 (better LLVM optimization)
codegen-units = 1

# Removes the debugging symbols
strip = true

# No unwinding on panic
panic = "abort"
```

---

## 2. Changing the Memory Allocator (`mimalloc`)

On Windows, the operating system's allocator can fragment memory and keep allocated pages for
longer. Replacing it with **mimalloc** (developed by Microsoft) performs clearly better on
Windows.

### Step 1: Add the dependency to `Cargo.toml`
```toml
[dependencies]
mimalloc = "*"
```

### Step 2: Set the global allocator in `src/main.rs`
```rust
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    println!("The application runs with mimalloc!");
}
```

---

## 3. Data Structures and Allocations

Frequent heap allocations (`Vec`, `String`, `Box`) raise memory use and fragmentation.

### Practical advice:
* **Prefer the stack to the heap:** use fixed-size arrays (`[u8; N]`) instead of `Vec<u8>`
  when the size is known at compile time.
* **Use smaller numeric types:** `u8` or `u16` instead of `usize` or `i64` where the range of
  values allows it.
* **Avoid automatic reallocations:** with `Vec` or `HashMap`, set the initial capacity:
  ```rust
  // Bad (several reallocations on the heap)
  let mut vec = Vec::new();

  // GOOD (allocates exactly the room needed from the start)
  let mut vec = Vec::with_capacity(1000);
  ```
* **Clone less:** use references (`&str`, `&[T]`) or *copy-on-write* types
  (`std::borrow::Cow`) instead of `.clone()`.

---

## 4. Auditing the Dependencies (Crates)

Sometimes high RAM use comes from third-party libraries that bring in features nobody uses.

1. Look at the dependency tree:
   ```bash
   cargo tree
   ```
2. **Pick only the features needed:**
   don't import a whole package when you need only a few of its modules.

   *Example (Tokio):*
   ```toml
   # Bad (pulls in the whole framework)
   tokio = { version = "1", features = ["full"] }

   # GOOD (only the runtime and the network)
   tokio = { version = "1", features = ["rt-multi-thread", "net", "macros"] }
   ```

---

## 5. Profiling Memory on Windows

To find exactly where the large allocations come from:

* **Visual Studio Performance Profiler:**
  1. Open the project built in `Release` mode.
  2. Go to **Debug** -> **Performance Profiler** (`Alt + F2`).
  3. Check **Memory Usage** and start the application.
  4. Look through the memory snapshots for unusual allocations.

* **The `memory-stats` crate:**
  the current RAM use can be watched from the code itself:
  ```rust
  use memory_stats::memory_stats;

  if let Some(usage) = memory_stats() {
      println!("Physical RAM used: {} MB", usage.physical_mem / 1024 / 1024);
      println!("Virtual RAM used: {} MB", usage.virtual_mem / 1024 / 1024);
  }
  ```
