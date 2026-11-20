# Privacy: the user's data stays theirs

Part of the project's rules: [index](project-conventions.md). Related: [the log](logging.md),
[modules](modules.md), [writing code](code.md).

- **No telemetry, ever.** Nothing about how the editor is used leaves the machine by itself:
  no metrics, no "anonymous usage", no crash upload. What goes out is only what the user
  connects to themselves (and the update check, which they can turn off).
- **Anything recorded about the app is anonymous by construction** (the log's bug-report
  archive, a profile recording, a diagnostic):
  - only what is the app's own (its threads, modules, views, commands, by the names written
    in our code: `indexer`, `log-viewer`, `tools.updates`) and the system's basics that
    explain performance (OS and version, architecture, cores, memory, GPU and driver, the
    screens' scale, the display session: Wayland, X11, macOS, Windows);
  - **never** a path, a file or folder name, a project, a branch, a database, a host, a
    connection, a URL, any content (code, text, results, requests), a user or machine name,
    an id of the hardware, the environment's variables, a log message (it may quote a
    path), a program's arguments (a program started by the app is named by its kind only:
    `terminal`, `agent`, `server`);
  - names reported to probes are `&'static str` from our code (`kernel::probe`): a name
    made at run time can carry the user's data and is refused by the type.
- **Allowlisted, tested:** a recorded format has a test listing every field it may hold (a
  new one fails it until it's added on purpose), and a test that records a real session in
  a project with "secret" names and paths, then looks for any piece of them in the output.
- **Consent first, every time:** a recording starts only after the user agreed in its own
  window, which says plainly what is and isn't kept; it shows while it runs (a visible
  indicator), stops by itself after a time the user picks, stays on the machine, and is
  shared only by the user (a file they send).
- **Secrets are never written anywhere** (the log's rules), and what the user can delete is
  deleted for real (the recordings' tab, the log's folder).
