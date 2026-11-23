# Deferred actions (`ui_components::deferred`)

**For:** a click whose action needs the disk or git (opening a comparison, reading a
file): the view prepares the action on a worker and gives it to the app when it's ready,
so a click never reads a file on the UI thread.

**Where it's used:** Git's changes and conflicts views, the local history, the remote's
browser.

## Anatomy

```
 click ──▶ start("git-output", job) ──▶ worker runs job ──▶ wake()
                                                              │
 paint / poll ◀── poll() → Some(ViewerAction) ◀───────────────┘
```

One action waited for at a time: a new `start` replaces it (the last click wins).

## Options

| Option | Default | Effect |
|---|---|---|
| `Deferred::default()` | idle | nothing waited for |
| `set_waker(&ctx.waker)` | none | how the app is woken when the job ends (the view's paint has it) |
| `start(thread, job)` | — | `job` (returns `Option<ViewerAction>`) on `kernel::work`'s named pool |
| `is_busy()` | `false` | a job is running (a spinner, a disabled button) |
| `poll()` | — | the action once ready, else `None` |

## States and interaction

- Idle → busy at `start` → idle again at the `poll` that takes the answer.
- A job that ends without an answer (it panicked) leaves it idle with nothing to do.
- An older job's answer is dropped when a newer one started.

## Accessibility

None: it isn't drawn. The view shows it's busy with the shared loader if the wait shows.

## Cost

- One channel and one job per click; nothing per frame but a `try_recv` in `poll`.
- The waker wakes the app once, when the job ends.

## Example

```rust
fn compare(&mut self, i: usize) -> Option<ViewerAction> {
    let (repo, path) = (self.repo.clone()?, self.path(i)?);
    self.opening.start("git-output", move || {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        let input = compare_with(repo.as_ref(), &path, &current, &Revision::Head, "HEAD")?;
        Some(ViewerAction::Compare(Box::new(input), format!("{name} ↔ HEAD")))
    });
    None
}
// in paint: self.opening.set_waker(&ctx.waker);
// in poll:  if let Some(action) = self.opening.poll() { self.pending = Some(action) }
```

## Don't

- Read a file or run git in a click handler: start a job.
- Start it on an unnamed thread: name the pool (`git-output`), so a profile says whose it is.
- Forget `set_waker`: without it the answer waits for the next frame something else asks for.
