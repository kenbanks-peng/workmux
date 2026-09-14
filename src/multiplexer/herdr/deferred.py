"""Private deferred operations. Embedded by the Rust adapter; requires Python 3.

No workmux CLI entry point or shared workflow change is required. Each socket
connection verifies the endpoint and server lifetime before it sends a request.
"""

import ctypes
import json
import os
import socket
import struct
import sys
import time
from pathlib import Path


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def process_start(pid):
    require(pid > 1, "Invalid process PID")
    if sys.platform == "darwin":
        # Darwin proc_bsdinfo: 12 u32, 48 name bytes, 6 u32, 2 u64.
        data = ctypes.create_string_buffer(136)
        lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        require(
            lib.proc_pidinfo(pid, 3, 0, data, len(data)) == len(data),
            "Cannot verify process lifetime",
        )
        fields = struct.unpack("=12I48s6I2Q", data.raw)
        require(
            fields[5] == os.geteuid() and fields[1] != 5, "Foreign or exited process"
        )
        return f"{fields[-2]}-{fields[-1]}"
    require(sys.platform.startswith("linux"), "Herdr requires macOS or Linux")
    path = Path(f"/proc/{pid}")
    require(path.stat().st_uid == os.geteuid(), "Foreign process")
    fields = (path / "stat").read_text().rsplit(")", 1)[1].split()
    require(fields[0] not in ("Z", "X"), "Exited process")
    boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    return f"{fields[19]}-{boot}"


class Client:
    def __init__(self, endpoint, boot):
        self.endpoint = endpoint
        self.boot = boot

    def request(self, method, **params):
        deadline = time.monotonic() + 8
        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(8)
            stream.connect(self.endpoint)
            if sys.platform == "darwin":
                pid = stream.getsockopt(0, 2)  # LOCAL_PEERPID
            else:
                pid, uid, _ = struct.unpack(
                    "3i", stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
                )
                require(uid == os.geteuid(), "Foreign Herdr server")
            stat = os.stat(self.endpoint)
            identity = f"{stat.st_dev}-{stat.st_ino}-{pid}-{process_start(pid)}"
            require(identity == self.boot, "Herdr server lifetime changed")
            stream.settimeout(max(0.001, deadline - time.monotonic()))
            stream.sendall(
                (
                    json.dumps({"id": "workmux", "method": method, "params": params})
                    + "\n"
                ).encode()
            )
            data = bytearray()
            while b"\n" not in data:
                require(time.monotonic() < deadline, "Herdr request timed out")
                stream.settimeout(max(0.001, deadline - time.monotonic()))
                chunk = stream.recv(65536)
                require(chunk, "Herdr response ended early")
                data.extend(chunk)
                require(len(data) <= 8 * 1024 * 1024, "Herdr response too large")
            response = json.loads(data.split(b"\n", 1)[0])
            require(response.get("id") == "workmux", "Mismatched response ID")
            require("error" not in response, str(response.get("error")))
            return response["result"]

    def snapshot(self):
        snapshot = self.request("session.snapshot")["snapshot"]
        require(
            (snapshot["version"], snapshot["protocol"]) == ("0.9.0", 22),
            "Unsupported Herdr protocol",
        )
        return snapshot


def run(payload):
    time.sleep(payload.get("delay", 0))
    client = Client(payload["endpoint"], payload["boot"])
    snapshot = client.snapshot()
    action = payload["action"]
    kind = "workspace" if action.endswith("workspace") else "tab"
    target = payload["target"]
    require(
        any(item[f"{kind}_id"] == target for item in snapshot[f"{kind}s"]),
        "Herdr target no longer exists",
    )
    if action.startswith("close-"):
        expected = payload["terminals"]
        panes = [p for p in snapshot["panes"] if p[f"{kind}_id"] == target]
        require(
            {p["terminal_id"] for p in panes} == set(expected),
            "Herdr target contents changed; refusing deferred close",
        )
        for pane in panes:
            process = client.request("pane.process_info", pane_id=pane["pane_id"])[
                "process_info"
            ]
            shell = expected[pane["terminal_id"]]
            require(
                process["shell_pid"] == shell["pid"]
                and process_start(shell["pid"]) == shell["start"],
                "Herdr terminal process changed",
            )
        # Recheck topology after process queries; do not adopt newly moved panes.
        live = [p for p in client.snapshot()["panes"] if p[f"{kind}_id"] == target]
        require(
            {p["terminal_id"] for p in live} == set(expected),
            "Herdr target contents changed",
        )
        # Never close a container: foreign panes can arrive after the check.
        # Resolve each captured terminal again because native moves change IDs.
        for terminal, shell in expected.items():
            pane = next(
                (p for p in client.snapshot()["panes"] if p["terminal_id"] == terminal),
                None,
            )
            if pane is None:
                continue
            process = client.request("pane.process_info", pane_id=pane["pane_id"])[
                "process_info"
            ]
            require(
                process["shell_pid"] == shell["pid"]
                and process_start(shell["pid"]) == shell["start"],
                "Herdr terminal process changed",
            )
            client.request("pane.close", pane_id=pane["pane_id"])
        remaining = client.snapshot()
        require(
            not any(item[f"{kind}_id"] == target for item in remaining[f"{kind}s"]),
            "Herdr cleanup preserved unexpected occupants; target container remains",
        )
    else:
        require(action in ("focus-tab", "focus-workspace"), "Invalid deferred action")
        client.request(f"{kind}.focus", **{f"{kind}_id": target})


if __name__ == "__main__":
    try:
        run(json.loads(sys.argv[1]))
    except (OSError, RuntimeError, ValueError, KeyError, TypeError) as error:
        print(f"workmux Herdr deferred operation: {error}", file=sys.stderr)
        sys.exit(1)
