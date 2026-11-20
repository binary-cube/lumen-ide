# Roadmap

What's planned next for the editor, in the order it's expected to come. More entries arrive
soon.

## PHP

- **A debugger and a profiler for PHP projects.** Step through code with breakpoints,
  watches and the call stack while a request or a script runs, and see where the time and
  the memory go in a run, without leaving the editor.

## The app

- **Profiling the app, with your consent.** Record a few minutes of what the editor itself
  spends (CPU per thread, memory, GPU, frames and why it woke up), anonymous by design and
  stopped by itself after the time you choose. A tab lists your recordings, shows them as a
  timeline and a flame graph, and gives a small JSON file you can attach to a bug report.

- **Logs for a bug report in one click.** Help → "Collect Logs for a Bug Report…" makes an
  archive of the last days' logs, with the paths made anonymous (your home folder and name
  left out), what the app found stuck (if anything) and which version and system it ran on.
  You see what goes in before it's saved, and nothing is ever sent by itself.
