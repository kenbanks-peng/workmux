"""Exercise diff actions through the real dashboard on a private server."""

import shlex
import subprocess
import sys
from pathlib import Path

from server import wait_until


def diff_actions(f):
    received = f.server.root / "received"
    ready = f.server.root / "input-ready"
    stub = f.server.root / "claude"
    stub.write_text(
        f"#!{sys.executable}\n"
        "import os, subprocess, tty\n"
        f"subprocess.run({[shlex.split(f.wm())[0], 'register-agent']!r}, check=True)\n"
        "tty.setraw(0)\n"
        f"open({str(ready)!r}, 'w').write('ready')\n"
        "while True:\n"
        "    data = os.read(0, 4096)\n"
        "    if not data: break\n"
        f"    with open({str(received)!r}, 'ab') as output: output.write(data)\n"
    )
    stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    source = f.repo / "example.txt"
    source.write_text("before\n")
    subprocess.run(
        ["git", "add", "."], cwd=f.repo, env=f.env, check=True, capture_output=True
    )
    subprocess.run(
        ["git", "commit", "-m", "diff fixture"],
        cwd=f.repo,
        env=f.env,
        check=True,
        capture_output=True,
    )
    pane = f.add()
    wait_until(lambda: ready.exists())
    worktree = Path(f.run("path", "feature").stdout.strip())
    (worktree / "example.txt").write_text("after\n")
    f.server.request("pane.focus", pane_id=f.parent["pane_id"])
    f.server.attach()
    done = f.server.root / "diff-dashboard-exit"
    f.server.request(
        "pane.send_input",
        pane_id=f.parent["pane_id"],
        text=f.wm("dashboard") + f"; printf '%s' $? > {shlex.quote(str(done))}",
        keys=["enter"],
    )

    def screen():
        return f.server.request(
            "pane.read", pane_id=f.parent["pane_id"], source="visible"
        )["read"]["text"]

    def key(value):
        f.server.request("pane.send_keys", pane_id=f.parent["pane_id"], keys=[value])

    def text(value):
        for character in value:
            key("space" if character == " " else character)

    def data():
        return received.read_bytes() if received.exists() else b""

    def open_diff():
        key("d")
        wait_until(lambda: "example.txt" in screen() and "after" in screen())

    wait_until(lambda: "feature" in screen(), timeout=15)
    open_diff()
    key("a")
    key("o")
    wait_until(lambda: "Send" in screen())
    text("Please check this change")
    wait_until(lambda: "Please check this change" in screen())
    key("enter")
    try:
        wait_until(
            lambda: b"Please check this change" in data() and data().endswith(b"\r")
        )
    except TimeoutError:
        print("Received:", repr(data()), "Screen:", screen())
        raise
    message = data()
    assert b"example.txt:1" in message and b"```diff" in message, message
    assert b"-before" in message and b"+after" in message, message
    assert "after" in screen() and not done.exists()
    key("q")  # Leave patch mode, not the diff.
    for action, expected in (
        ("c", b"Commit staged changes with a descriptive message"),
        ("m", b"!workmux merge"),
    ):
        before = len(data())
        key(action)
        wait_until(
            lambda expected=expected, before=before: (
                expected in data()[before:] and data()[before:].endswith(b"\r")
            )
        )
        assert data()[before:] == expected + b"\r", data()[before:]
        wait_until(lambda: "feature" in screen() and "Commit changes" not in screen())
        assert not done.exists(), "Diff action exited the dashboard"
        if action == "c":
            open_diff()
    key("q")
    wait_until(lambda: done.exists() and done.read_text() == "0")
    assert (worktree / "example.txt").read_text() == "after\n"
    snapshot = f.server.request("session.snapshot")["snapshot"]
    assert {pane["terminal_id"], f.parent["terminal_id"]} <= {
        p["terminal_id"] for p in snapshot["panes"]
    }
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=worktree,
        env=f.env,
        check=True,
        capture_output=True,
        text=True,
    )
    assert status.stdout == " M example.txt\n", status.stdout
    print(
        "PASS diff actions: Send delivers file, line, hunk and comment; commit and merge deliver exact prompts plus Enter; diff closes, dashboard stays open and exits 0 (input stub, not real agent execution)"
    )
