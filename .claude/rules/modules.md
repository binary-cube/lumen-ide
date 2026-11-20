# How modules talk

Part of the project's rules: [index](project-conventions.md). Related: [writing code](code.md), [UI components](ui-components.md), [modals](modals.md).

Modules don't know each other: no module depends on another's crate, reads its files or
names its commands, views or windows. Removing one changes nothing in the others.
They meet only through:

1. **Contribution** (`ctx.contribute`): "I offer something", known at start (a viewer
   provider, a tool window, a command, a menu, an assist, a service, a reporter).
2. **Event** (the project's own bus: `ProjectContext.events` / `ProjectView.events`):
   "it happened", any listeners, no answer. Each project has its own bus.
3. **Request** (`contracts`, `Answers::of`, `host.ask`; a view: `ViewerAction::Ask`):
   "do it / tell me", at most one answers; `None` means nobody does — handle it.
4. **Host** (`WorkbenchHost`, `ViewerAction::WithHost`, `AppRequest`): what only the app can
   do (windows, tabs, clipboard, pickers, opening a browser, the system's web engine).

What needs a partner is offered only while there is one (`workbench_model::presence`).
Per-project state is `PerProject<T>`. A view gets its project through `set_data`
(`ProjectView`); a panel's work after a picked file comes back through the host.
Modules build their UI from `ui-components` (never `ui-workbench`).

Made concrete while building the agents (plan 79):

- **Registries the app fills.** A neutral domain crate holds a contract and its registry
  (`agents::AgentProvider`, `mcp::McpTool`); modules contribute, the app registers what
  was contributed at startup. A consumer lists the registry and never names a provider
  (the MCP server names no tool's module; the sessions panel names no agent).
- **A tool window that needs something** says so with `ToolWindowProvider::available`
  (e.g. only while some agent is installed): read from memory, never the disk.
- **Requests between modules are small, named contracts** in `contracts` (`RunInTerminal`,
  `RenameTerminal`, `TerminalRunning`, `RenameSession`, `OpenAgentChat`): the asker handles
  `None` / `false` (nobody answers, the tab is gone: let it go).
- **Identity by key, never by title**: a tab another module opened is found by its key
  (`contracts::terminal::tab_key`) or a UUID, never by its name or a name read from a file.
- **A reply that may come later** is a value that answers once and says "refused" if
  dropped (`mcp::Reply`); whoever waits may give up, and then nothing runs late.
- **An agent is a folder of `modules/ai/agents/`** on `agents::AgentProvider`; what agents
  read alike on disk (finding the program, bounded listings and reads, values kept by
  stamp) is `agent-files`, never copied into each; its own formats stay in its module and
  are checked read-only on a real install before they're written. What the panel keeps of
  a session (favorites, marks, folders) is keyed by agent + id, so a new agent needs
  nothing there.
- **Static tables per project**: an entry kept for a project (its wake, its notifier, its
  favorites) is removed when it closes (`kernel::Subscription::on_end` returned from
  `ProjectService::attach`); never "the last window's" handle kept for all.
