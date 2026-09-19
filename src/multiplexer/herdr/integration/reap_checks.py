"""Failure-path checks for agent reaping on a private Herdr server."""

import json
import sys
from pathlib import Path

from run import ROOT
from server import wait_until

BINARY = ROOT / "target/debug/workmux"


def reap_unresponsive(f):
    ready = f.server.root / "stubborn-ready"
    interrupted = f.server.root / "stubborn-interrupted"
    eof = f.server.root / "stubborn-eof"
    stub = f.server.root / "claude"
    stub.write_text(
        f"#!{sys.executable}\nimport os, signal, subprocess, time\n"
        f"signal.signal(signal.SIGINT, lambda *_: open({str(interrupted)!r}, 'w').write('interrupt'))\n"
        f"subprocess.run([{str(BINARY)!r}, 'register-agent'], check=True)\n"
        f"subprocess.run([{str(BINARY)!r}, 'set-window-status', 'working'], check=True)\n"
        f"open({str(ready)!r}, 'w').write('ready')\n"
        "while True:\n"
        "    if not os.read(0, 4096):\n"
        f"        open({str(eof)!r}, 'w').write('eof')\n"
        "        time.sleep(0.05)\n"
    )
    stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    pane = f.add()
    wait_until(lambda: ready.exists())
    # Control time only in this private fixture, as in the successful reap tests.
    files = list(Path(f.env["XDG_STATE_HOME"]).glob("workmux/agents/*.json"))
    assert len(files) == 1, files
    state = json.loads(files[0].read_text())
    state["updated_ts"] -= 7200
    files[0].write_text(json.dumps(state))
    before = json.loads(f.run("status", "--json").stdout)["agents"]
    assert len(before) == 1 and before[0]["status"] == "working", before

    result = f.run("reap-agents", "--hours", "1")
    assert "Would exit" in result.stdout, result
    assert not interrupted.exists() and not eof.exists()

    result = f.run("reap-agents", "--hours", "1", "--force", ok=False)
    assert result.returncode != 0, result
    assert "agent did not exit after C-c and C-d" in result.stdout, result
    assert "failed to exit 1 agent(s)" in result.stderr, result
    assert "Exited " not in result.stdout, result
    assert interrupted.read_text() == "interrupt"
    assert eof.read_text() == "eof"
    after = json.loads(f.run("status", "--json").stdout)["agents"]
    assert len(after) == 1, after
    for key in ("pane_id", "status", "updated_ts", "worktree"):
        assert after[0][key] == before[0][key], (before, after)

    snapshot = f.server.request("session.snapshot")["snapshot"]
    for original in (pane, f.parent):
        live = next(p for p in snapshot["panes"] if p["pane_id"] == original["pane_id"])
        assert live["terminal_id"] == original["terminal_id"], live
    code, output = f.native(f.parent, "printf 'UNRELATED_SHELL_ALIVE'")
    assert code == 0 and output == "UNRELATED_SHELL_ALIVE", output
    print(
        "PASS reap-unresponsive: dry-run sends no input; Ctrl-C/EOF refusal reports failure, "
        "retains agent state, and preserves the unrelated shell"
    )
