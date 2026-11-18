# The Editor's MCP Server

The editor has an [MCP](https://modelcontextprotocol.io) (Model Context Protocol) server.
Through it, the agents on your computer (Claude Code now, others later) can use the editor:
find out which file you have open and what you selected, open a file at a line, read a tab's
text including its unsaved changes, or see the project's Git status.

The server runs only on your computer (`127.0.0.1`), asks for a token on every request and is
**off until you turn it on**.

## Starting it

1. **Settings › Tools › MCP Server**: turn the switch on.
2. The page shows the address it listens on, for example `http://127.0.0.1:52817/mcp`.
3. Press **Configure the agents**. Each installed agent is configured with the server: for
   Claude Code, an `editor` entry is added at the user level, valid in every project.
4. Start (or restart) an agent session. In Claude Code, `/mcp` shows the `editor` server as
   connected.

## The port

- **Empty (the default)**: on the first start the system picks a free port, and the editor
  keeps it. The configured agent still finds it after you restart the editor.
- **A number between 1024 and 65535**: the server listens there. Useful if you want a fixed
  port, known in advance.
- If the port kept is taken by another program at startup, the server listens on another one,
  and the page tells you to configure the agents again.
- After you change the port by hand, press **Configure the agents** again.

## The token

Every request must carry the `Authorization: Bearer <token>` header. Without it, the server
answers `401`.

- The token has 256 bits and is made on the first start.
- It is kept in the **system's keychain** (macOS Keychain, Windows Credential Manager, Secret
  Service on Linux), under the `<app-id>.mcp` service. It is **not** in the settings file.
- It appears nowhere in the interface and is never written to the log.
- The agent gets it through a helper command (`headersHelper` in Claude Code): on every
  connection, Claude runs the editor with the `--mcp-headers` argument. The editor reads the
  token from the keychain, writes the header and exits at once. This way the token is neither
  on a process's command line (visible with `ps`) nor in the agent's configuration.

## Configuring it by hand

The **Configure the agents** button runs, for Claude Code:

```sh
claude mcp remove --scope user editor
claude mcp add-json --scope user editor \
  '{"type":"http","url":"http://127.0.0.1:<port>/mcp","headersHelper":"'\''<the editor'\''s path>'\'' --mcp-headers"}'
```

For another agent that speaks MCP over HTTP (Streamable HTTP):

- **the address**: `http://127.0.0.1:<port>/mcp`, the `POST` method, a JSON-RPC 2.0 body;
- **the header**: `Authorization: Bearer <token>`. If the agent can run a command for its
  headers, use `<the editor's path> --mcp-headers`, which writes
  `{"Authorization":"Bearer …"}`. Otherwise the token has to be taken out of the keychain and
  put in the agent's configuration (less safe).

## The tools

| Tool | What it does | Kind |
|---|---|---|
| `open_file` | Opens a project file in the editor, at a line if one is given (`path`, `line`). | read |
| `get_selection` | The file you're in, the cursor's line and the selected text. | read |
| `get_open_files` | The files open in tabs (relative to the project). | read |
| `get_file_text` | An open file's text as the editor holds it, with the unsaved changes. | read |
| `git_status` | The branch and the changed files, each with its state. | read |
| `git_file_at_head` | A file's text as it is in the last commit (`HEAD`). | read |

The full list, with what the running modules offer, is shown on the settings page.

### Permissions

- **Tools that only read** answer at once.
- **Tools that write or run something** ask you in the editor's window: *Allow once*,
  *Allow in this session* or *Refuse*. If you close the question, the request is refused.
- A call waits at most 30 seconds (5 minutes if it asks you). After that the agent learns that
  the editor didn't answer in time, and the call is **abandoned**: it doesn't run even if you
  allow it later.

### Which window answers

- A call about a file reaches the window of the project that holds the file.
- A call without a file (or with a relative path) reaches the window you last worked in.

## Security

- **It listens only on `127.0.0.1`**: no other computer reaches it.
- **The token** is compared in constant time. A request without a token is refused after its
  headers, before its body is read.
- **Checks against web pages**: the `Host` header must be exactly the local address (this
  stops *DNS rebinding*), and `Origin`, if sent, must be local.
- **Limits**:
  - headers of at most 16 KB and a body of at most 4 MB;
  - at most 8 connections at once;
  - a request must arrive whole within 10 seconds;
  - the Git tools' answers are limited (2,000 files, 2 MB per file).
- **Paths** given by the agent stay in the project: `..` or a path outside the project is
  refused.

## When it doesn't work

| What you see | What to do |
|---|---|
| The page says the server didn't start | The chosen port is taken or below 1024: pick another, or leave the field empty. |
| "the earlier port was taken: configure the agents again" | Press **Configure the agents**. |
| `/mcp` in Claude Code shows `editor` as failed | The editor must be running, with the server on. Then configure again (after the app was moved, the helper command's path changed). |
| `401` in the agent's log | The token is no longer in the keychain (deleted by hand): turn the server off and on, then configure again. |
| "no project is open in the editor" | Open a project: the tools work on the open project. |
| "the editor didn't answer in time" | The window was busy, or the permission question went unanswered. |
