# The UI thread only draws and answers

Part of the project's rules: [index](project-conventions.md). Related: [writing code](code.md), [UI components](ui-components.md), [the log](logging.md).

- **Nothing heavy on the UI thread**: no disk, git, network, process or parsing of anything
  big. Work goes to a worker (`kernel::work::run` with a named pool, `host.spawn`,
  `ui_components::deferred::Deferred`, a module's `Pending<T>`), and comes back by a poll;
  the view's paint waker wakes the app when it's ready. A click never reads a file.
- **Nothing per frame that could be cached**: parse once per version, read states once per
  change (a generation), build strings once, follow wakers once. Frames only when something
  changed; an animation asks for the next frame only while it moves (`frame_deadline`,
  `is_animating`).
- **Exists only while used**: views, engines, previews are made when first needed and let go
  when closed or hidden long enough; nothing loaded before it's needed. Memory has budgets
  (e.g. response bodies per project), the oldest let go to disk.
- **Measure before choosing**, and write the numbers in the commit or the plan.
- **Never I/O, waking or calling out under a lock**: a lock only moves data in or out;
  writing to a process goes through a thread of its own fed by a channel, a wake happens
  after the guard is dropped. Saves of one file go one at a time (the latest state taken
  inside the worker), through a temporary file renamed over it.
- **Cheap checks first in what runs every frame or poll**: compare a generation before
  cloning a list; return when a queue is empty before looking at anything else.
- **Reading another program's output** (an agent's pipes, its session files): bounded
  lines, lossy UTF-8, one bad line skipped, never the end of the stream; titles from it
  tidied to one line and capped; anything typed into a terminal stripped of control
  characters.

## Measuring what the app spends (plan 91)

- **A part that does work reports it to the probes** (`kernel::probe`), with static names
  of our code: `count(owner, what, n)` (things done), `level(owner, what, value)` (what it
  holds), `time(owner, what)` (a guard around the work), `trigger(owner, cause)` (a frame
  asked for). Nothing else changes: a profile recording keeps it and its view lists it. Off,
  a probe is one atomic load (~3.5 ns): put them where the work is, not in a hot inner loop.
- **A module's richer figures** (what it knows as a whole: requests waiting, an answer's
  time) are a `contracts::profiling::ProfileSource` it contributes: told when a recording
  starts and ends (what it keeps only for it begins and is let go there), read once a
  second from memory only, its readings typed (`Count`, `Bytes`, `Micros`, `Level`) with
  static names. The app registers them; the recorder lists them and names no module.
- **What a part moves over the network or the disk** goes in its Task Manager row
  (`workbench_model::NetIo`, `DiskIo` on its `ResourceUse`): the profile reads those too.
- **No event per measurement**: a frame, a poll, a byte happen thousands of times a second;
  the probes add numbers, the recorder takes them at its own pace.
- Names and values only, never what or where (`privacy.md`).
