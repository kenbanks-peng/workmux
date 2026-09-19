"""Session lifecycle probes. Each case uses a private server and repository."""

import json
import sys

from server import wait_until


def snapshot(f):
    return f.server.request("session.snapshot")["snapshot"]


def workspace(f, label):
    matches = [w for w in snapshot(f)["workspaces"] if w["label"] == label]
    assert len(matches) == 1, matches
    return matches[0]


def absent(f, label):
    return all(w["label"] != label for w in snapshot(f)["workspaces"])


def session(f):
    for name, args in [("flag", ["--session"]), ("mode", ["--mode", "session"])]:
        f.run("add", name, *args, "--background")
        target = workspace(f, f"wm-{name}")
        assert snapshot(f)["focused_workspace_id"] == f.parent["workspace_id"]
        f.run("open", name)
        assert snapshot(f)["focused_workspace_id"] == target["workspace_id"]
        f.run("close", name)
        wait_until(lambda name=name: absent(f, f"wm-{name}"))
        f.run("open", name)
        workspace(f, f"wm-{name}")
        f.run("remove", name, "--force")
        wait_until(lambda name=name: absent(f, f"wm-{name}"))
    f.config({"mode": "session", "panes": [{}]})
    f.run("add", "configured", "--background", "--target-name", "custom")
    workspace(f, "wm-custom")
    f.run("add", "headless", "--headless")
    f.run("open", "headless", "--session")
    workspace(f, "wm-headless")
    before = snapshot(f)
    assert f.run("open", "configured", "--new", ok=False).returncode != 0
    assert (
        f.run("add", "invalid", "--parent-session", "parent", ok=False).returncode != 0
    )
    assert snapshot(f)["tabs"] == before["tabs"]
    f.run("rename", "configured", "renamed")
    renamed = workspace(f, "wm-renamed")
    assert absent(f, "wm-custom")
    f.run("open", "renamed")
    assert snapshot(f)["focused_workspace_id"] == renamed["workspace_id"]
    f.run("close", "renamed")
    wait_until(lambda: absent(f, "wm-renamed"))
    f.run("open", "renamed")
    workspace(f, "wm-renamed")
    f.run("add", "batch", "--count", "2", "--background")
    workspace(f, "wm-batch-1")
    workspace(f, "wm-batch-2")
    print(
        "PASS session: flag, mode, config, explicit target, open, close, remove, rename, invalid options"
    )


def session_layout(f):
    f.config(
        {
            "mode": "session",
            "windows": [
                {"name": "editor", "panes": [{}]},
                {
                    "name": "tests",
                    "panes": [{}, {"split": "horizontal", "focus": True, "zoom": True}],
                },
            ],
        }
    )
    f.run("add", "layout", "--background")
    target = workspace(f, "wm-layout")
    state = snapshot(f)
    assert state["focused_workspace_id"] == f.parent["workspace_id"]
    tabs = [t for t in state["tabs"] if t["workspace_id"] == target["workspace_id"]]
    assert {t["label"] for t in tabs} == {"editor", "tests"}, tabs
    assert (
        len([p for p in state["panes"] if p["workspace_id"] == target["workspace_id"]])
        == 3
    )
    f.run("close", "layout")
    wait_until(lambda: absent(f, "wm-layout"))
    f.run("open", "layout")
    state = snapshot(f)
    focused = next(t for t in state["tabs"] if t["tab_id"] == state["focused_tab_id"])
    assert focused["label"] == "tests", state
    # Convert both ways through the shared workflows. Use an explicit destination
    # so an external command never relies on the last focused workspace.
    f.config({"panes": [{}]})
    f.run("open", "layout", "--mode", "window", "--parent-session", "parent")
    assert absent(f, "wm-layout")
    state = snapshot(f)
    tab = next(t for t in state["tabs"] if t["label"] == "wm-layout")
    assert tab["workspace_id"] == f.parent["workspace_id"]
    f.run("open", "layout", "--session")
    workspace(f, "wm-layout")
    assert all(t["tab_id"] != tab["tab_id"] for t in snapshot(f)["tabs"])
    print(
        "PASS session layout: multiple tabs, splits, background focus, selected tab, mode conversion"
    )


def session_recovery(f):
    # Normal registration, not a synthetic state file or a status workaround.
    from support_checks import BINARY, recovery_files

    ready = f.server.root / "session-agent.json"
    stub = f.server.root / "claude"
    stub.write_text(
        f"#!{sys.executable}\nimport json, os, subprocess, time\n"
        f"subprocess.run([{str(BINARY)!r}, 'register-agent'], check=True)\n"
        f"with open({str(ready)!r}, 'w') as output: json.dump(dict(os.environ), output)\n"
        "time.sleep(120)\n"
    )
    stub.chmod(0o700)
    config = {
        "mode": "session",
        "agent": str(stub),
        "agents": {"claude": str(stub)},
        "panes": [{"command": "<agent>"}],
    }
    f.config(config)
    f.run("add", "feature", "--background")
    wait_until(lambda: ready.exists() and ready.read_text())
    original = json.loads(ready.read_text())
    assert recovery_files(f), "Ordinary agent registration did not persist state"
    f.run("close", "feature")
    wait_until(lambda: absent(f, "wm-feature"))
    ready.unlink()
    assert "Restored 1" in f.run("resurrect").stdout
    wait_until(lambda: ready.exists() and ready.read_text())
    old_key = json.loads(ready.read_text())["WORKMUX_STATUS_PANE_ID"]
    assert old_key != original["WORKMUX_STATUS_PANE_ID"]
    f.server.stop()
    restored = f.server.start()["snapshot"]
    workspace(f, "wm-feature")
    retained = {p["terminal_id"] for p in restored["panes"]}
    # Occupied suffixes must not be adopted, either.
    f.server.request(
        "workspace.create", label="wm-feature-2", cwd=str(f.repo), focus=False
    )
    before = snapshot(f)
    recovery = recovery_files(f)
    assert "would restore 1" in f.run("resurrect", "--dry-run").stdout
    assert snapshot(f)["tabs"] == before["tabs"]
    assert recovery_files(f) == recovery
    ready.unlink()
    assert "Restored 1" in f.run("resurrect").stdout
    wait_until(lambda: ready.exists() and ready.read_text())
    environment = json.loads(ready.read_text())
    key = environment["WORKMUX_STATUS_PANE_ID"]
    assert key.split("~")[0] != old_key.split("~")[0]
    target = workspace(f, "wm-feature-3")
    state = snapshot(f)
    pane = next(p for p in state["panes"] if key.endswith("~" + p["terminal_id"]))
    assert pane["workspace_id"] == target["workspace_id"]
    assert retained <= {p["terminal_id"] for p in state["panes"]}
    for _ in range(2):
        assert "skipping (already open)" in f.run("resurrect").stdout
        assert snapshot(f)["tabs"] == state["tabs"]
    f.run("set-window-status", "working", env=environment)
    agents = json.loads(f.run("status", "--json").stdout)["agents"]
    assert any(a["status"] == "working" for a in agents), agents
    f.run("open", "feature")
    assert snapshot(f)["focused_workspace_id"] == target["workspace_id"]
    f.run("close", "feature")
    wait_until(lambda: absent(f, "wm-feature-3"))
    workspace(f, "wm-feature")
    workspace(f, "wm-feature-2")
    assert retained <= {p["terminal_id"] for p in snapshot(f)["panes"]}
    # Failure after a restart must retain recovery data and the partial target.
    f.config({**config, "panes": [{}, {"split": "horizontal", "percentage": 5}]})
    recovery = recovery_files(f)
    result = f.run("resurrect", ok=False)
    assert result.returncode != 0 and "Failed to setup panes" in result.stderr, result
    assert recovery_files(f) == recovery
    assert "skipping (already open)" in f.run("resurrect").stdout
    assert recovery_files(f) == recovery
    f.run("close", "feature")
    f.config(config)
    assert "Restored 1" in f.run("resurrect").stdout
    # Rename must use the persisted recovery target, not the preserved original.
    f.run("rename", "feature", "recovered")
    renamed = workspace(f, "wm-recovered")
    f.run("open", "recovered")
    assert snapshot(f)["focused_workspace_id"] == renamed["workspace_id"]
    f.run("close", "recovered")
    wait_until(lambda: absent(f, "wm-recovered"))
    assert retained <= {p["terminal_id"] for p in snapshot(f)["panes"]}
    print(
        "PASS session recovery: closed space, restart, safe suffix, fresh agent, repeat open, cleanup, partial failure retry, rename"
    )


def session_navigation(f):
    f.run("add", "child", "--session", "--background")
    target = workspace(f, "wm-child")
    pane = next(
        p for p in snapshot(f)["panes"] if p["workspace_id"] == target["workspace_id"]
    )
    # Run from inside the target: deferred cleanup must outlive this terminal.
    code, output = f.native(pane, f.wm("close", "child"))
    assert code == 0, output
    wait_until(lambda: absent(f, "wm-child"))
    assert snapshot(f)["focused_workspace_id"] == f.parent["workspace_id"]
    f.run("open", "child")
    target = workspace(f, "wm-child")
    pane = next(
        p for p in snapshot(f)["panes"] if p["workspace_id"] == target["workspace_id"]
    )
    # An empty feature is sufficient to exercise merge cleanup and navigation.
    code, output = f.native(pane, f.wm("merge", "child"))
    assert code == 0, output
    wait_until(lambda: absent(f, "wm-child"))
    assert snapshot(f)["focused_workspace_id"] == f.parent["workspace_id"]
    f.run("add", "removed", "--session")
    target = workspace(f, "wm-removed")
    assert snapshot(f)["focused_workspace_id"] == target["workspace_id"]
    pane = next(
        p for p in snapshot(f)["panes"] if p["workspace_id"] == target["workspace_id"]
    )
    code, output = f.native(pane, f.wm("remove", "removed", "--force"))
    assert code == 0, output
    wait_until(lambda: absent(f, "wm-removed"))
    assert snapshot(f)["focused_workspace_id"] == f.parent["workspace_id"]
    print(
        "PASS session navigation: foreground add, in-pane close/merge/remove, deferred cleanup, parent retained"
    )
