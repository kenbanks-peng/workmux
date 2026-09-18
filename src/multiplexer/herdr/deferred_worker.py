"""Embedded deferred worker. Keep identity formats aligned with client/identity.rs.

Run by absolute Python path with -I: no project imports, Python environment,
workmux configuration, or installed helper file. The caller supplies only data.
"""

import ctypes
import json
import os
import socket
import struct
import sys
import time
from pathlib import Path

TIMEOUT = 8
MAX_RESPONSE = 8 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


class BsdInfo(ctypes.Structure):
    # Darwin's proc_bsdinfo from <sys/proc_info.h>.
    _fields_ = (
        [
            (name, ctypes.c_uint32)
            for name in (
                "flags",
                "status",
                "xstatus",
                "pid",
                "ppid",
                "uid",
                "gid",
                "ruid",
                "rgid",
                "svuid",
                "svgid",
                "reserved",
            )
        ]
        + [
            ("comm", ctypes.c_char * 16),
            ("name", ctypes.c_char * 32),
        ]
        + [
            (name, ctypes.c_uint32)
            for name in ("nfiles", "pgid", "pjobc", "e_tdev", "e_tpgid")
        ]
        + [
            ("nice", ctypes.c_int32),
            ("start_sec", ctypes.c_uint64),
            ("start_usec", ctypes.c_uint64),
        ]
    )


def process_start(pid):
    require(type(pid) is int and 1 < pid <= 2147483647, "Invalid Herdr process PID")
    if sys.platform == "darwin":
        info = BsdInfo()
        libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
        proc_pidinfo = libc.proc_pidinfo
        proc_pidinfo.argtypes = [
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_uint64,
            ctypes.c_void_p,
            ctypes.c_int,
        ]
        proc_pidinfo.restype = ctypes.c_int
        size = ctypes.sizeof(info)
        require(
            proc_pidinfo(pid, 3, 0, ctypes.byref(info), size) == size,
            "Cannot verify Herdr process lifetime",
        )
        require(
            info.uid == os.geteuid() and info.status != 5,
            "Herdr process is foreign or exited",
        )
        return f"{info.start_sec}-{info.start_usec}"
    if sys.platform == "linux":
        path = Path(f"/proc/{pid}")
        require(
            path.stat().st_uid == os.geteuid(), "Herdr process belongs to another user"
        )
        fields = (path / "stat").read_text().rsplit(")", 1)[1].split()
        require(fields[0] not in ("Z", "X"), "Herdr process exited")
        boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
        return f"{fields[19]}-{boot}"
    raise ValueError("Herdr process verification requires macOS or Linux")


def connection_identity(stream, endpoint):
    metadata = os.stat(endpoint)
    if sys.platform == "darwin":
        # SOL_LOCAL = 0, LOCAL_PEERPID = 2.
        pid = struct.unpack("=i", stream.getsockopt(0, 2, 4))[0]
    elif sys.platform == "linux":
        pid, uid, _ = struct.unpack(
            "=iII", stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
        )
        require(uid == os.geteuid(), "Herdr server belongs to another user")
    else:
        raise ValueError("Herdr peer verification requires macOS or Linux")
    return f"{metadata.st_dev}-{metadata.st_ino}-{pid}-{process_start(pid)}"


class Client:
    def __init__(self, endpoint, boot):
        require(os.path.isabs(endpoint), "Herdr socket path must be absolute")
        self.endpoint = endpoint
        self.boot = boot

    def request(self, method, params):
        deadline = time.monotonic() + TIMEOUT

        def remaining():
            seconds = deadline - time.monotonic()
            require(seconds > 0, "Herdr request timed out")
            return seconds

        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(remaining())
            stream.connect(self.endpoint)
            require(
                connection_identity(stream, self.endpoint) == self.boot,
                "Herdr server lifetime changed; refusing to reuse live targets",
            )
            stream.settimeout(remaining())
            stream.sendall(
                (
                    json.dumps(
                        {
                            "id": "workmux",
                            "method": method,
                            "params": params,
                        }
                    )
                    + "\n"
                ).encode()
            )
            response = bytearray()
            while True:
                stream.settimeout(remaining())
                chunk = stream.recv(8192)
                require(chunk, "Incomplete Herdr response")
                response.extend(chunk)
                require(len(response) <= MAX_RESPONSE, "Oversized Herdr response")
                if b"\n" in chunk:
                    break
        value = json.loads(response)
        require(isinstance(value, dict), "Malformed Herdr response")
        require(value.get("id") == "workmux", "Mismatched Herdr response ID")
        require("error" not in value, f"Herdr {method}: {value.get('error')}")
        require(
            isinstance(value.get("result"), dict), "Herdr response has no object result"
        )
        return value["result"]

    def snapshot(self):
        snapshot = self.request("session.snapshot", {}).get("snapshot")
        require(isinstance(snapshot, dict), "Malformed Herdr snapshot")
        require(
            snapshot.get("version") == "0.9.0" and snapshot.get("protocol") == 22,
            "Unsupported Herdr server; expected 0.9.0 protocol 22",
        )
        for group, fields in (
            ("workspaces", ("workspace_id", "label")),
            ("tabs", ("tab_id", "workspace_id", "label")),
            ("panes", ("pane_id", "terminal_id", "tab_id", "workspace_id", "cwd")),
        ):
            items = snapshot.get(group)
            require(isinstance(items, list), "Malformed Herdr snapshot")
            for item in items:
                require(
                    isinstance(item, dict)
                    and all(isinstance(item.get(field), str) for field in fields),
                    "Malformed Herdr snapshot",
                )
                if group == "panes":
                    require(
                        type(item.get("focused")) is bool, "Malformed Herdr snapshot"
                    )
        return snapshot


def run(payload):
    operation = json.loads(payload)
    require(isinstance(operation, dict), "Invalid Herdr deferred operation")
    for field in ("endpoint", "boot", "target", "action"):
        require(
            isinstance(operation.get(field), str) and operation[field],
            "Invalid Herdr deferred operation",
        )
    action = operation["action"]
    require(
        action in ("close-tab", "close-workspace", "focus-tab", "focus-workspace"),
        "Invalid Herdr deferred operation",
    )
    terminals = operation.get("terminals")
    require(isinstance(terminals, dict), "Invalid Herdr deferred operation")
    for identity in terminals.values():
        require(
            isinstance(identity, dict)
            and type(identity.get("pid")) is int
            and 1 < identity["pid"] <= 2147483647
            and isinstance(identity.get("start"), str),
            "Invalid Herdr deferred operation",
        )
    workspace = action.endswith("workspace")
    close = action.startswith("close-")
    target = operation["target"]
    key = "workspace_id" if workspace else "tab_id"
    group = "workspaces" if workspace else "tabs"
    client = Client(operation["endpoint"], operation["boot"])
    snapshot = client.snapshot()
    require(
        any(item[key] == target for item in snapshot[group]),
        "Herdr target no longer exists",
    )
    if close:
        require(workspace or terminals, "Herdr cleanup target has no owned terminals")

        def verify_contents(current):
            live = {p["terminal_id"] for p in current["panes"] if p[key] == target}
            require(
                terminals.keys() <= live,
                "Herdr target contents changed; refusing deferred close",
            )

        verify_contents(snapshot)
        for pane in snapshot["panes"]:
            if pane[key] != target or pane["terminal_id"] not in terminals:
                continue
            identity = terminals[pane["terminal_id"]]
            process = client.request("pane.process_info", {"pane_id": pane["pane_id"]})
            try:
                live = process_start(identity["pid"]) == identity["start"]
            except (OSError, ValueError, IndexError):
                live = False
            require(
                process.get("process_info", {}).get("shell_pid") == identity["pid"]
                and live,
                "Herdr terminal process changed",
            )
        verify_contents(client.snapshot())
    # Close the container, including extra panes. Never follow a moved terminal.
    method = ("workspace" if workspace else "tab") + (".close" if close else ".focus")
    client.request(method, {key: target})


if __name__ == "__main__":
    try:
        require(len(sys.argv) == 2, "Expected one Herdr operation payload")
        run(sys.argv[1])
    except (OSError, ValueError, TypeError, KeyError, IndexError) as error:
        print(f"Herdr deferred operation failed: {error}", file=sys.stderr)
        sys.exit(1)
