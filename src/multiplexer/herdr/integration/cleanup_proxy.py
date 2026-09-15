"""Check container cleanup with a pane inserted at the close barrier.

Insert a real foreign terminal immediately before the first close request.
The adapter sees a stable proxy endpoint; all mutations reach a private server.
"""

import json
import socket
import threading


class CleanupProxy:
    def __init__(self, server, insertion):
        self.server = server
        self.insertion = insertion
        self.socket_path = server.root / "cleanup-proxy.sock"
        self.methods = []
        self.foreign = None
        self.shell_pid = None
        self.errors = []
        self.stopping = threading.Event()
        self.listener = socket.socket(socket.AF_UNIX)
        self.listener.bind(str(self.socket_path))
        self.listener.listen()
        self.listener.settimeout(0.1)
        self.thread = threading.Thread(target=self.forward)

    def __getattr__(self, name):
        return getattr(self.server, name)

    def forward(self):
        try:
            while not self.stopping.is_set():
                try:
                    client, _ = self.listener.accept()
                except TimeoutError:
                    continue
                with client:
                    client.settimeout(8)
                    with client.makefile("rb") as reader:
                        request = json.loads(reader.readline(8 * 1024 * 1024))
                    method = request["method"]
                    if (self.server.root / "arm-cleanup").exists() and method in {
                        "pane.close",
                        "tab.close",
                        "workspace.close",
                    }:
                        self.methods.append(method)
                        if self.foreign is None:
                            state = self.server.request("session.snapshot")["snapshot"]
                            owned = next(
                                p
                                for p in state["panes"]
                                if p["workspace_id"]
                                == next(
                                    w["workspace_id"]
                                    for w in state["workspaces"]
                                    if w["label"] == "cleanup-race"
                                )
                            )
                            if self.insertion == "same-tab":
                                self.foreign = self.server.request(
                                    "pane.split",
                                    target_pane_id=owned["pane_id"],
                                    direction="right",
                                    focus=False,
                                )["pane"]
                            else:
                                result = self.server.request(
                                    "layout.apply",
                                    workspace_id=owned["workspace_id"],
                                    tab_label="foreign",
                                    focus=False,
                                    root={"type": "pane", "cwd": str(self.server.root)},
                                )
                                tab = result.get("tab", result.get("layout"))["tab_id"]
                                self.foreign = next(
                                    p
                                    for p in self.server.request("session.snapshot")[
                                        "snapshot"
                                    ]["panes"]
                                    if p["tab_id"] == tab
                                )
                            self.shell_pid = self.server.request(
                                "pane.process_info", pane_id=self.foreign["pane_id"]
                            )["process_info"]["shell_pid"]
                    with socket.socket(socket.AF_UNIX) as upstream:
                        upstream.settimeout(8)
                        upstream.connect(str(self.server.socket_path))
                        upstream.sendall(json.dumps(request).encode() + b"\n")
                        with upstream.makefile("rb") as reader:
                            client.sendall(reader.readline(8 * 1024 * 1024))
        except (
            OSError,
            ValueError,
            KeyError,
            TypeError,
            StopIteration,
            RuntimeError,
        ) as error:
            self.errors.append(error)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.stopping.set()
        self.thread.join(10)
        self.listener.close()
        assert not self.thread.is_alive()
        assert not self.errors, self.errors

    def verify(self):
        assert self.methods in (["tab.close"], ["workspace.close"]), self.methods
        state = self.server.request("session.snapshot")["snapshot"]
        assert not any(
            p["terminal_id"] == self.foreign["terminal_id"] for p in state["panes"]
        )
        if self.methods == ["tab.close"]:
            assert not any(t["tab_id"] == self.foreign["tab_id"] for t in state["tabs"])
        else:
            assert not any(
                w["workspace_id"] == self.foreign["workspace_id"]
                for w in state["workspaces"]
            )
