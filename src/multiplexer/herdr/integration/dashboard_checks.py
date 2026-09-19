"""Dashboard keyboard checks against real terminals on a private Herdr server."""

import json
import shlex
import sys
from pathlib import Path

from run import ROOT
from server import wait_until
from workflow_checks import git

BINARY = ROOT / "target/debug/workmux"


class Dashboard:
    def __init__(self, f):
        self.f = f
        self.done = f.server.root / "actions-dashboard-exit"

    def __enter__(self):
        f = self.f
        self.done.unlink(missing_ok=True)
        f.server.request("pane.focus", pane_id=f.parent["pane_id"])
        if not f.server.clients:
            f.server.attach()
        f.server.request(
            "pane.send_input",
            pane_id=f.parent["pane_id"],
            text=f.wm("dashboard")
            + f"; printf '%s' $? > {shlex.quote(str(self.done))}",
            keys=["enter"],
        )
        self.wait("Worktrees")
        return self

    def screen(self):
        return self.f.server.request(
            "pane.read", pane_id=self.f.parent["pane_id"], source="visible"
        )["read"]["text"]

    def wait(self, text):
        try:
            wait_until(lambda: text in self.screen(), timeout=15)
        except TimeoutError:
            print("Dashboard screen:", self.screen())
            raise

    def key(self, value):
        self.f.server.request(
            "pane.send_keys", pane_id=self.f.parent["pane_id"], keys=[value]
        )

    def text(self, value):
        for character in value:
            self.key("space" if character == " " else character)

    def filter(self, value):
        self.key("/")
        self.text(value)
        self.key("enter")
        self.wait(value)

    def exited(self):
        try:
            wait_until(lambda: self.done.exists() and self.done.read_text() == "0")
        except TimeoutError:
            print("Dashboard exit screen:", self.screen())
            raise

    def __exit__(self, kind, error, traceback):
        if kind:
            print("Dashboard screen:", self.screen())
        elif not self.done.exists():
            self.key("q")
            self.exited()


def snapshot(f):
    return f.server.request("session.snapshot")["snapshot"]


def assert_preserved(f, *panes):
    current = snapshot(f)
    for pane in panes:
        live = next(p for p in current["panes"] if p["pane_id"] == pane["pane_id"])
        assert live["terminal_id"] == pane["terminal_id"], live


def input_agent(f):
    ready = f.server.root / "dashboard-agent-ready"
    received = f.server.root / "dashboard-agent-input"
    stub = f.server.root / "claude"
    stub.write_text(
        f"#!{sys.executable}\nimport os, subprocess, tty\n"
        f"subprocess.run([{str(BINARY)!r}, 'register-agent'], check=True)\n"
        f"subprocess.run([{str(BINARY)!r}, 'set-window-status', 'working'], check=True)\n"
        "tty.setraw(0)\n"
        f"open({str(ready)!r}, 'w').write('ready')\n"
        "while True:\n"
        "    data = os.read(0, 4096)\n"
        "    if not data: break\n"
        f"    with open({str(received)!r}, 'ab') as output: output.write(data)\n"
    )
    stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    pane = f.add()
    wait_until(lambda: ready.exists())
    return pane, received


def dashboard_input(f):
    guard = f.add("guard")
    pane, received = input_agent(f)
    with Dashboard(f) as ui:
        ui.wait("feature")
        ui.key("i")
        ui.wait("INPUT MODE")
        ui.text("Hello from dashboard")
        ui.key("backspace")
        ui.key("tab")
        ui.key("enter")
        expected = b"Hello from dashboard\x7f\t\r"
        try:
            wait_until(lambda: received.exists() and received.read_bytes() == expected)
        except TimeoutError:
            print("Agent input:", received.read_bytes() if received.exists() else b"")
            raise
        ui.key("escape")
        wait_until(lambda: "INPUT MODE" not in ui.screen())
        ui.key("q")
        ui.exited()
        assert received.read_bytes() == expected, received.read_bytes()
    assert_preserved(f, pane, guard, f.parent)
    assert snapshot(f)["focused_tab_id"] == f.parent["tab_id"]
    code, output = f.native(guard, "printf 'GUARD_ALIVE'")
    assert code == 0 and output == "GUARD_ALIVE", output
    print(
        "PASS dashboard-input: exact text and Enter reach the agent; Escape/q stay local; focus and unrelated terminals survive"
    )


def dashboard_kill(f):
    guard = f.add("guard")
    pane, _ = input_agent(f)
    worktree = Path(f.run("path", "feature").stdout.strip())
    marker = worktree / "untracked.txt"
    marker.write_text("preserve me\n")
    with Dashboard(f) as ui:
        ui.wait("feature")
        ui.key("X")
        ui.wait("Kill working agent?")
        ui.key("n")
        wait_until(lambda: "Kill working agent?" not in ui.screen())
        assert_preserved(f, pane, guard, f.parent)
        assert len(json.loads(f.run("status", "--json").stdout)["agents"]) == 1
        ui.key("X")
        ui.wait("Kill working agent?")
        ui.key("y")
        wait_until(
            lambda: all(p["pane_id"] != pane["pane_id"] for p in snapshot(f)["panes"])
        )
        wait_until(lambda: not json.loads(f.run("status", "--json").stdout)["agents"])
        assert not ui.done.exists(), "Kill exited the dashboard"
    assert_preserved(f, guard, f.parent)
    assert snapshot(f)["focused_tab_id"] == f.parent["tab_id"]
    assert Path(f.run("path", "feature").stdout.strip()) == worktree
    assert marker.read_text() == "preserve me\n"
    print(
        "PASS dashboard-kill: cancel preserves the agent; confirm removes only its pane and tracked status, not its worktree or files"
    )


def dashboard_worktrees(f):
    guard = f.add("guard")
    with Dashboard(f) as ui:
        ui.key("tab")
        ui.wait("guard")
        ui.key("a")
        ui.wait("Add Worktree")
        ui.text("dashboard-created")
        ui.key("enter")
        wait_until(
            lambda: any(
                t["label"] == "wm-dashboard-created" for t in snapshot(f)["tabs"]
            ),
            timeout=20,
        )
        wait_until(lambda: "Add Worktree" not in ui.screen())
        worktree = Path(f.run("path", "dashboard-created").stdout.strip())
        assert git(f, worktree, "branch", "--show-current") == "dashboard-created"
        assert snapshot(f)["focused_tab_id"] == f.parent["tab_id"]
        tab = next(
            t for t in snapshot(f)["tabs"] if t["label"] == "wm-dashboard-created"
        )
        original = next(p for p in snapshot(f)["panes"] if p["tab_id"] == tab["tab_id"])
        marker = worktree / "untracked.txt"
        marker.write_text("keep on close\n")
        ui.filter("dashboard-created")
        ui.wait("● active")
        ui.key("c")
        wait_until(
            lambda: all(t["tab_id"] != tab["tab_id"] for t in snapshot(f)["tabs"])
        )
        assert marker.read_text() == "keep on close\n"
        assert git(f, worktree, "branch", "--show-current") == "dashboard-created"
        assert_preserved(f, guard, f.parent)
        assert not ui.done.exists(), "Close exited the dashboard"
        ui.key("enter")
        ui.exited()
    current = snapshot(f)
    reopened = next(t for t in current["tabs"] if t["label"] == "wm-dashboard-created")
    assert reopened["workspace_id"] == f.parent["workspace_id"]
    assert current["focused_tab_id"] == reopened["tab_id"]
    replacement = next(p for p in current["panes"] if p["tab_id"] == reopened["tab_id"])
    assert replacement["terminal_id"] != original["terminal_id"]
    assert marker.read_text() == "keep on close\n"
    assert_preserved(f, guard, f.parent)
    print(
        "PASS dashboard-worktrees: add creates in the caller workspace without focus change; close preserves files/branch; Enter reopens and focuses a new terminal"
    )


def dashboard_sweep(f):
    clean = f.add("clean")
    dirty = f.add("dirty")
    clean_path = Path(f.run("path", "clean").stdout.strip())
    dirty_path = Path(f.run("path", "dirty").stdout.strip())
    marker = dirty_path / "untracked.txt"
    marker.write_text("do not sweep\n")
    with Dashboard(f) as ui:
        ui.key("tab")
        ui.wait("dirty")
        # Wait for the asynchronous Git fetch before opening the candidate list.
        ui.wait("* +1")
        ui.key("R")
        ui.wait("[x] clean")
        ui.wait("[ ] dirty")
        ui.wait("remove (1)")
        ui.key("escape")
        wait_until(lambda: "remove (1)" not in ui.screen())
        assert clean_path.is_dir() and marker.read_text() == "do not sweep\n"
        assert_preserved(f, clean, dirty, f.parent)
        ui.key("R")
        ui.wait("remove (1)")
        ui.key("enter")
        wait_until(lambda: not clean_path.exists(), timeout=20)
        wait_until(
            lambda: all(t["tab_id"] != clean["tab_id"] for t in snapshot(f)["tabs"])
        )
        ui.wait("Sweep complete")
        assert not ui.done.exists(), "Sweep exited the dashboard"
    assert git(f, f.repo, "branch", "--list", "clean") == ""
    assert git(f, dirty_path, "branch", "--show-current") == "dirty"
    assert marker.read_text() == "do not sweep\n"
    assert_preserved(f, dirty, f.parent)
    assert snapshot(f)["focused_tab_id"] == f.parent["tab_id"]
    print(
        "PASS dashboard-sweep: cancel changes nothing; confirm removes the clean merged worktree/branch/tab and preserves the dirty worktree and caller"
    )
