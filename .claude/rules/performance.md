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
- **Nothing moves unseen**: a part not drawn now (hidden, a figure the selection lacks, a
  page scrolled away) starts no transition and says it isn't animating (`Chart::forget`
  when it stops being drawn); `is_animating` is true only while something visibly changes.
  The app paces an animation's frames by the display: a frame that showed nothing new
  waits a display's frame (`ANIMATION_FLOOR`) before the next, so a part wrongly saying
  it moves costs ~120 frames a second at worst, never an unpaced loop. A test holds it
  where a view updates on a clock (the Task Manager's ticking detail).
- **Moving is not waiting**: `is_animating` means something visibly changes every frame
  (a transition, a drag scrolling). Work going on (a worker, a measure, a git operation, a
  program's first output) is `is_loading` (a loader's pace); a pause to wait out (typing,
  a save after a change) is a `frame_deadline` (one frame then); a result from a worker
  wakes the window (its waker), never frames asked until it's here. A worker gone without
  its answer (`Disconnected`) ends the wait.
- **What asks for frames is walked in one place** (`Workbench::each_frame_asker`, only
  what's on screen); a new part is added there by name. A part saying it moves while
  frames show nothing is named in the log (`UnseenMotion`) and by the snapshots
  (`moving: …`; a scene meant to move says `MOVES=1`).
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
