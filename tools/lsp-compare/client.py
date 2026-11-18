"""The comparison's probes asked of a PHP language server, as an editor asks them (stdio,
JSON-RPC): its answers and how long each took, beside the built-in engine's.

    python3 -I client.py <work dir> intelephense|phpactor

Reads <work dir>/probes.json (probes.py), writes <work dir>/answers-<server>.json. The
servers are configured as the app describes them
(crates/modules/languages/php/data/language-servers.toml) and installed by run.sh in
<work dir>/servers; their caches and their HOME are under <work dir> (nothing of the user's
is read or written). Each server is stopped when its project is done.
"""

import json
import os
import queue
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path
from urllib.parse import quote, unquote, urlparse

# How long a project's first index may take, a request, and the quiet that ends the wait
# for a file's diagnostics.
INDEX_TIMEOUT = 600
REQUEST_TIMEOUT = 30
DIAGNOSTICS_QUIET = 4
DIAGNOSTICS_TIMEOUT = 30

EXCLUDE = [
    "**/.git/**", "**/.svn/**", "**/.hg/**", "**/CVS/**", "**/.DS_Store/**",
    "**/node_modules/**", "**/bower_components/**", "**/vendor/**/{Tests,tests}/**",
    "**/.history/**", "**/vendor/**/vendor/**", "**/storage/framework/views/**",
    "**/var/cache/**",
]


def uri_of(path):
    return "file://" + quote(str(path))


def path_of(uri):
    return Path(unquote(urlparse(uri).path))


def server_setup(name, work, project):
    """The command, its environment, its initialization options and its settings."""
    servers = work / "servers"
    cache = work / "cache" / name / project
    cache.mkdir(parents=True, exist_ok=True)
    home = work / "home"
    home.mkdir(parents=True, exist_ok=True)
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_CACHE_HOME": str(cache),
        "XDG_DATA_HOME": str(home / ".local" / "share"),
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
    }
    if name == "intelephense":
        program = servers / "intelephense" / "node_modules" / "intelephense" / "lib" / "intelephense.js"
        command = ["node", "--max-old-space-size=3072", str(program), "--stdio"]
        options = {
            "storagePath": str(cache),
            "globalStoragePath": str(work / "cache" / name / "global"),
            "clearCache": True,
        }
        settings = {
            "intelephense": {
                "telemetry": {"enabled": False},
                "format": {"enable": True},
                "files": {"maxSize": 5000000, "exclude": EXCLUDE},
            }
        }
        return command, env, options, settings
    if name == "phpactor":
        command = [
            os.environ.get("PHP", "php"), "-n",
            "-d", "memory_limit=3072M",
            "-d", "display_errors=stderr",
            "-d", "error_reporting=E_ALL & ~E_DEPRECATED",
            str(servers / "phpactor" / "phpactor.phar"), "language-server",
        ]
        env["BOX_REQUIREMENT_CHECKER"] = "0"
        options = {
            "indexer.enabled_watchers": ["lsp"],
            "indexer.parallel_workers": 2,
            "language_server.diagnostic_outsource": False,
            "language_server.code_action_outsource": False,
            "language_server.diagnostic_providers": ["code-action"],
            "language_server.diagnostic_ignore_codes": [
                "worse.missing_member",
                "worse.unused_import",
                "worse.docblock_missing_return_type",
                "worse.docblock_missing_param",
                "worse.docblock_missing_extends_tag",
                "worse.docblock_missing_implements_tag",
                "worse.missing_return_type",
            ],
            "language_server_configuration.auto_config": False,
        }
        return command, env, options, {}
    raise SystemExit(f"unknown server {name}")


class Server:
    def __init__(self, name, work, project, root, log):
        command, env, self.options, self.settings = server_setup(name, work, project)
        self.name = name
        self.root = root
        self.log = log
        self.process = subprocess.Popen(
            command, cwd=root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=log, start_new_session=True,
        )
        self.next_id = 0
        self.pending = {}
        self.lock = threading.Lock()
        self.diagnostics = {}
        self.diagnostics_at = 0.0
        self.progress = {}
        self.indexing_ended = threading.Event()
        self.versions = {}
        threading.Thread(target=self.read, daemon=True).start()

    # ─── The wire ───

    def send(self, message):
        message["jsonrpc"] = "2.0"
        body = json.dumps(message).encode("utf-8")
        with self.lock:
            self.process.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
            self.process.stdin.flush()

    def read(self):
        out = self.process.stdout
        while True:
            length = None
            while True:
                line = out.readline()
                if not line:
                    return
                line = line.strip()
                if not line:
                    break
                if line.lower().startswith(b"content-length:"):
                    length = int(line.split(b":")[1])
            if length is None:
                continue
            message = json.loads(out.read(length))
            self.received(message)

    def received(self, message):
        method = message.get("method")
        if method is None:
            waiting = self.pending.pop(message.get("id"), None)
            if waiting is not None:
                waiting.put(message)
            return
        params = message.get("params") or {}
        if "id" in message:
            self.answer(message["id"], method, params)
        elif method == "textDocument/publishDiagnostics":
            self.diagnostics[params["uri"]] = params.get("diagnostics", [])
            self.diagnostics_at = time.monotonic()
        elif method == "$/progress":
            kind = (params.get("value") or {}).get("kind")
            self.progress[str(params.get("token"))] = kind
        elif method == "indexingEnded":
            self.indexing_ended.set()
        elif method in ("window/logMessage", "window/showMessage"):
            # intelephense says its first index is done in its log.
            if "Indexing finished" in params.get("message", ""):
                self.indexing_ended.set()
            self.log.write(f"[{method}] {params.get('message', '')}\n".encode("utf-8"))
            self.log.flush()

    def answer(self, id, method, params):
        result = None
        if method == "workspace/configuration":
            result = []
            for item in params.get("items", []):
                value = self.settings
                for key in (item.get("section") or "").split("."):
                    if key:
                        value = value.get(key, {}) if isinstance(value, dict) else {}
                result.append(value if value != {} else None)
        elif method == "workspace/workspaceFolders":
            result = [{"uri": uri_of(self.root), "name": self.root.name}]
        self.send({"id": id, "result": result})

    def request(self, method, params, timeout=REQUEST_TIMEOUT):
        """The answer (`result`, or `{"error": …}`) and how long it took (ms)."""
        self.next_id += 1
        id = self.next_id
        waiting = queue.Queue()
        self.pending[id] = waiting
        started = time.perf_counter()
        self.send({"id": id, "method": method, "params": params})
        try:
            message = waiting.get(timeout=timeout)
        except queue.Empty:
            self.pending.pop(id, None)
            return {"error": "timeout"}, timeout * 1000.0
        took = (time.perf_counter() - started) * 1000.0
        if "error" in message:
            return {"error": message["error"]}, took
        return message.get("result"), took

    def notify(self, method, params):
        self.send({"method": method, "params": params})

    # ─── The session ───

    def initialize(self):
        capabilities = {
            "textDocument": {
                "synchronization": {"didSave": True, "dynamicRegistration": False},
                "completion": {
                    "completionItem": {"snippetSupport": True, "documentationFormat": ["markdown", "plaintext"]},
                    "contextSupport": True,
                },
                "hover": {"contentFormat": ["markdown", "plaintext"]},
                "signatureHelp": {
                    "signatureInformation": {
                        "documentationFormat": ["markdown", "plaintext"],
                        "parameterInformation": {"labelOffsetSupport": True},
                        "activeParameterSupport": True,
                    }
                },
                "definition": {"linkSupport": True},
                "references": {},
                "rename": {"prepareSupport": False},
                "publishDiagnostics": {"relatedInformation": False},
            },
            "workspace": {
                "configuration": True,
                "workspaceFolders": True,
                "didChangeWatchedFiles": {"dynamicRegistration": True},
                "applyEdit": False,
            },
            "window": {"workDoneProgress": True},
        }
        result, took = self.request("initialize", {
            "processId": os.getpid(),
            "rootUri": uri_of(self.root),
            "rootPath": str(self.root),
            "workspaceFolders": [{"uri": uri_of(self.root), "name": self.root.name}],
            "capabilities": capabilities,
            "initializationOptions": self.options,
            "clientInfo": {"name": "lsp-compare"},
        }, timeout=120)
        if not isinstance(result, dict) or "capabilities" not in result:
            raise SystemExit(f"{self.name}: initialize failed: {result}")
        self.notify("initialized", {})
        if self.settings:
            self.notify("workspace/didChangeConfiguration", {"settings": self.settings})
        return took

    def wait_indexed(self):
        """Until the first index is done (intelephense says `indexingEnded`; phpactor ends
        its progress): seconds."""
        started = time.monotonic()
        while time.monotonic() - started < INDEX_TIMEOUT:
            if self.name == "intelephense" and self.indexing_ended.is_set():
                break
            if self.name == "phpactor" and self.progress and all(
                kind == "end" for kind in self.progress.values()
            ):
                break
            if self.process.poll() is not None:
                raise SystemExit(f"{self.name} exited while indexing")
            time.sleep(0.05)
        took = time.monotonic() - started
        time.sleep(1)
        return took

    def open(self, path, text):
        uri = uri_of(path)
        if uri not in self.versions:
            self.versions[uri] = 1
            self.notify("textDocument/didOpen", {"textDocument": {
                "uri": uri, "languageId": "php", "version": 1, "text": text,
            }})
        return uri

    def change(self, uri, text):
        self.versions[uri] += 1
        self.notify("textDocument/didChange", {
            "textDocument": {"uri": uri, "version": self.versions[uri]},
            "contentChanges": [{"text": text}],
        })

    def stop(self):
        try:
            self.request("shutdown", None, timeout=10)
            self.notify("exit", None)
            self.process.wait(timeout=10)
        except Exception:
            pass
        try:
            os.killpg(self.process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


# ─── The answers, in the same shape as the built-in engine's ───


def relative(root, uri):
    path = os.path.realpath(path_of(uri))
    base = os.path.realpath(root)
    return os.path.relpath(path, base) if path.startswith(base + os.sep) else path


def hover_text(contents):
    if contents is None:
        return None
    if isinstance(contents, str):
        return contents
    if isinstance(contents, dict):
        return contents.get("value", "")
    return "\n".join(hover_text(c) or "" for c in contents)


def shape(server, category, result, text, line):
    root = server.root
    if isinstance(result, dict) and "error" in result and len(result) == 1:
        return None
    if category == "completion":
        items = result.get("items", []) if isinstance(result, dict) else (result or [])
        out = []
        for item in items:
            edit = item.get("textEdit") or {}
            out.append([item.get("label", ""), item.get("filterText"),
                        item.get("insertText") or edit.get("newText")])
        return out
    if category == "definition":
        if result is None:
            return []
        locations = result if isinstance(result, list) else [result]
        out = []
        for l in locations:
            if "targetUri" in l:
                selection = l.get("targetSelectionRange") or l["targetRange"]
                out.append([relative(root, l["targetUri"]), l["targetRange"]["start"]["line"],
                            l["targetRange"]["end"]["line"], selection["start"]["line"]])
            else:
                r = l["range"]
                out.append([relative(root, l["uri"]), r["start"]["line"], r["end"]["line"],
                            r["start"]["line"]])
        return out
    if category == "hover":
        return hover_text(result.get("contents")) if isinstance(result, dict) else None
    if category == "signature":
        if not isinstance(result, dict) or not result.get("signatures"):
            return None
        signature = result["signatures"][result.get("activeSignature") or 0]
        active = signature.get("activeParameter", result.get("activeParameter"))
        return {"label": signature.get("label", ""), "active": active}
    if category == "references":
        return sorted({(relative(root, l["uri"]), l["range"]["start"]["line"]) for l in result or []})
    if category == "rename":
        if not isinstance(result, dict):
            return []
        places = set()
        for uri, edits in (result.get("changes") or {}).items():
            for e in edits:
                places.add((relative(root, uri), e["range"]["start"]["line"]))
        for change in result.get("documentChanges") or []:
            if "textDocument" in change:
                for e in change.get("edits", []):
                    places.add((relative(root, change["textDocument"]["uri"]), e["range"]["start"]["line"]))
        return sorted(places)
    raise SystemExit(f"unknown category {category}")


def params_for(category, uri, p, text):
    position = {"line": p["line"], "character": p["character"]}
    params = {"textDocument": {"uri": uri}, "position": position}
    if category == "completion":
        params["context"] = {"triggerKind": 1}
    elif category == "references":
        params["context"] = {"includeDeclaration": True}
    elif category == "rename":
        params["newName"] = "renamedName"
    return params


METHODS = {
    "completion": "textDocument/completion",
    "definition": "textDocument/definition",
    "hover": "textDocument/hover",
    "signature": "textDocument/signatureHelp",
    "references": "textDocument/references",
    "rename": "textDocument/rename",
}


def run(work, name):
    data = json.loads((work / "probes.json").read_text(encoding="utf-8"))
    answers, projects = {}, {}
    logs = work / "logs"
    logs.mkdir(exist_ok=True)
    for project, root in data["projects"].items():
        root = Path(os.path.realpath(root))
        probes = [p for p in data["probes"] if p["project"] == project]
        if not probes:
            continue
        with open(logs / f"{name}-{project}.log", "wb") as log:
            server = Server(name, work, project, root, log)
            try:
                ready = server.initialize()
                indexed = server.wait_indexed()
                projects[project] = {"initialize_ms": ready, "index_seconds": indexed}
                print(f"{name} {project}: indexed in {indexed:.1f} s", flush=True)
                for p in probes:
                    if p["category"] in ("diagnostics", "noise"):
                        continue
                    path = root / p["file"]
                    original = path.read_text(encoding="utf-8")
                    uri = server.open(path, original)
                    asked = p.get("text", original)
                    if asked != original:
                        server.change(uri, asked)
                    times, result = [], None
                    for _ in range(4):
                        result, took = server.request(METHODS[p["category"]],
                                                      params_for(p["category"], uri, p, asked))
                        times.append(took)
                    answers[p["id"]] = {
                        "result": shape(server, p["category"], result, asked, p.get("line")),
                        "raw_error": result.get("error") if isinstance(result, dict) and "error" in result else None,
                        "ms": times,
                    }
                    if asked != original:
                        server.change(uri, original)
                # The diagnostics: the files asked opened (as they are on disk), then the
                # last ones published for them once they're quiet.
                files = sorted({p["file"] for p in probes if p["category"] in ("diagnostics", "noise")})
                uris = {}
                for file in files:
                    # One file at a time (a server may check only the last one it was given),
                    # until its diagnostics are quiet.
                    path = root / file
                    text = path.read_text(encoding="utf-8")
                    uri = server.open(path, text)
                    if server.versions[uri] > 1:
                        server.change(uri, text)
                    server.diagnostics.pop(uri, None)
                    uris[file] = uri
                    started = time.monotonic()
                    server.diagnostics_at = started
                    while time.monotonic() - started < DIAGNOSTICS_TIMEOUT:
                        quiet = time.monotonic() - server.diagnostics_at
                        if uri in server.diagnostics and quiet > DIAGNOSTICS_QUIET:
                            break
                        time.sleep(0.1)
                for p in probes:
                    if p["category"] not in ("diagnostics", "noise"):
                        continue
                    found = server.diagnostics.get(uris[p["file"]], [])
                    severity = {1: "error", 2: "warning", 3: "info", 4: "hint"}
                    answers[p["id"]] = {"result": [
                        {
                            "line": d["range"]["start"]["line"],
                            "message": f"{d.get('message', '')} {d.get('code', '')}".strip(),
                            "severity": severity.get(d.get("severity", 1), "error"),
                        }
                        for d in found
                    ], "ms": []}
            finally:
                server.stop()
    out = {"engine": name, "projects": projects, "answers": answers}
    (work / f"answers-{name}.json").write_text(json.dumps(out, indent=1), encoding="utf-8")


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(), sys.argv[2])
