"""Repeatable support-table probes on private Herdr servers.

Build workmux first. Run one named case, or run all cases with no argument.
These tests use temporary repositories and never contact a user's server.
They assert both working paths and known restrictions in STATUS.html.
When a restriction is fixed, update its assertion and table entry together.
"""

import json
import shlex
import subprocess
import sys
import time
from pathlib import Path

from dashboard_checks import (
    dashboard_input,
    dashboard_kill,
    dashboard_sweep,
    dashboard_worktrees,
)
from diff_checks import diff_actions, patch_split
from focus_checks import focus_protocol
from reap_checks import reap_unresponsive
from run import ROOT, env_for
from server import HerdrServer, wait_until
from session_checks import session, session_layout, session_navigation, session_recovery
from workflow_checks import (
    merge_conflicts,
    merge_hooks,
    merge_squash,
    rebase_conflicts,
    session_merge_conflicts,
    session_merge_squash,
    session_rebase_conflicts,
)

BINARY = ROOT / "target/debug/workmux"


class Fixture:
    def __init__(self, server):
        self.server = server
        self.env = env_for(server)
        self.repo = server.root / "repo"
        self.repo.mkdir()
        self.counter = 0
        self.session_mode = False
        self.config({"panes": [{}]})
        for args in (
            ["init", "-b", "main"],
            ["config", "user.name", "Test"],
            ["config", "user.email", "test@example.invalid"],
            ["add", "."],
            ["commit", "-m", "fixture"],
        ):
            subprocess.run(
                ["git", *args],
                cwd=self.repo,
                env=self.env,
                check=True,
                capture_output=True,
            )
        self.parent = server.request(
            "workspace.create",
            label="parent",
            cwd=str(self.repo),
            focus=True,
        )["root_pane"]

    def config(self, value):
        (self.repo / ".workmux.yaml").write_text(
            json.dumps({"nerdfont": False, **value})
        )

    def run(self, *args, ok=True, env=None, input=None):
        result = subprocess.run(
            [str(BINARY), *args],
            cwd=self.repo,
            env=env or self.env,
            input=input,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        if ok:
            assert result.returncode == 0, result.stdout + result.stderr
        return result

    def native(self, pane, command):
        self.counter += 1
        output = self.server.root / f"output-{self.counter}"
        code = self.server.root / f"code-{self.counter}"
        command = (
            f"{command} > {shlex.quote(str(output))} 2>&1; "
            f"printf '%s' $? > {shlex.quote(str(code))}"
        )
        self.server.request(
            "pane.send_input",
            pane_id=pane["pane_id"],
            text=shlex.join(["/bin/sh", "-c", command]),
            keys=["enter"],
        )
        try:
            wait_until(lambda: code.exists() and code.read_text(), timeout=35)
        except TimeoutError:
            if output.exists():
                print(output.read_text())
            raise
        return int(code.read_text()), output.read_text()

    def wm(self, *args):
        return shlex.join([str(BINARY), *args])

    def add(self, name="feature"):
        options = ["--session"] if self.session_mode else ["--parent-session", "parent"]
        self.run("add", name, *options, "--background")
        snapshot = self.server.request("session.snapshot")["snapshot"]
        if self.session_mode:
            workspace = next(
                w for w in snapshot["workspaces"] if w["label"] == f"wm-{name}"
            )
            tab = next(
                t
                for t in snapshot["tabs"]
                if t["workspace_id"] == workspace["workspace_id"]
            )
        else:
            tab = next(t for t in snapshot["tabs"] if t["label"] == f"wm-{name}")
        return next(p for p in snapshot["panes"] if p["tab_id"] == tab["tab_id"])


def sidebar(f):
    result = f.run("sidebar", ok=False)
    assert result.returncode != 0 and "Sidebar requires tmux" in result.stderr
    print("PASS sidebar: explicit tmux-only rejection")


def launch_status_agent(f, name="feature", hold=True):
    """Use normal agent launch variables and the real registration hook."""
    ready = f.server.root / f"{name}-environment.json"
    stub = f.server.root / "claude"
    stub.write_text(
        f"#!{sys.executable}\nimport json, os, subprocess, time\n"
        f"subprocess.run([{str(BINARY)!r}, 'register-agent'], check=True)\n"
        f"with open({str(ready)!r}, 'w') as output: json.dump(dict(os.environ), output)\n"
        f"time.sleep({120 if hold else 0})\n"
    )
    stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    pane = f.add(name)
    wait_until(lambda: ready.exists() and ready.read_text())
    environment = json.loads(ready.read_text())
    assert environment["WORKMUX_STATUS_BACKEND"] == "herdr", environment
    assert environment["WORKMUX_STATUS_INSTANCE"] == str(f.server.socket_path.resolve())
    assert environment["WORKMUX_STATUS_PANE_ID"].endswith("~" + pane["terminal_id"])
    # Hooks can run outside the terminal, without native variables or ancestry.
    environment = {
        key: value
        for key, value in environment.items()
        if not key.startswith("HERDR_") and key != "WORKMUX_BACKEND"
    }
    return pane, environment


def assert_status(f, pane, state):
    agents = json.loads(f.run("status", "--json").stdout)["agents"]
    assert len(agents) == 1 and agents[0]["status"] == state, agents
    report = next(
        p
        for p in f.server.request("session.snapshot")["snapshot"]["panes"]
        if p["terminal_id"] == pane["terminal_id"]
    )
    assert (
        report.get("agent_status")
        == {
            "working": "working",
            "waiting": "blocked",
            # Protocol 22 reports Workmux completion as idle, not a new state.
            "done": "idle",
            "-": "unknown",
        }[state]
    ), report


def status(f):
    pane, environment = launch_status_agent(f)
    assert_status(f, pane, "-")
    for state in ("working", "waiting", "done", "clear"):
        f.run("set-window-status", state, env=environment)
        assert_status(f, pane, "-" if state == "clear" else state)
    print(
        "PASS status: ordinary agent launch registers; detached hooks without native variables update working/waiting/done/clear in JSON and native reports"
    )


def status_targets(f):
    pane, environment = launch_status_agent(f)
    key = environment["WORKMUX_STATUS_PANE_ID"]
    boot, _ = key.split("~", 1)
    f.run("set-window-status", "working", env=environment)
    # A move across workspaces changes the pane address, not the terminal identity.
    destination = f.server.request(
        "workspace.create", label="moved", cwd=str(f.repo), focus=False
    )["root_pane"]
    f.server.request(
        "pane.move",
        pane_id=pane["pane_id"],
        destination={
            "type": "tab",
            "tab_id": destination["tab_id"],
            "target_pane_id": destination["pane_id"],
            "split": "right",
        },
        focus=False,
    )
    moved = next(
        p
        for p in f.server.request("session.snapshot")["snapshot"]["panes"]
        if p["terminal_id"] == pane["terminal_id"]
    )
    assert moved["pane_id"] != pane["pane_id"], moved
    f.run("register-agent", env=environment)
    assert_status(f, moved, "-")
    f.run("set-window-status", "waiting", env=environment)
    assert_status(f, moved, "waiting")

    def reports(server):
        return {
            p["terminal_id"]: p.get("agent_status")
            for p in server.request("session.snapshot")["snapshot"]["panes"]
        }

    def reject(target, servers):
        # Test native fallback suppression as well as detached-hook rejection.
        before = [reports(server) for server in servers]
        state_dir = Path(f.env["XDG_STATE_HOME"]) / "workmux/agents"
        records = {p.name: p.read_bytes() for p in state_dir.glob("*.json")}
        for args in (
            ("register-agent",),
            ("set-window-status", "working"),
            ("set-window-status", "clear"),
        ):
            f.run(*args, env=target)
            command = shlex.join(
                [
                    "env",
                    *(
                        f"{k}={v}"
                        for k, v in target.items()
                        if k.startswith("WORKMUX_STATUS_")
                    ),
                    str(BINARY),
                    *args,
                ]
            )
            code, output = f.native(f.parent, command)
            assert code == 0, output  # Hooks are best-effort; check effects below.
            after = [reports(server) for server in servers]
            assert after == before, (before, after, args)
            assert {p.name: p.read_bytes() for p in state_dir.glob("*.json")} == records

    for invalid_key in (
        pane["terminal_id"],
        moved["pane_id"],
        "stale~" + pane["terminal_id"],
        boot + "~missing-terminal",
        "",
    ):
        reject({**environment, "WORKMUX_STATUS_PANE_ID": invalid_key}, [f.server])
    for endpoint in ("relative.sock", str(f.server.root / "missing.sock"), ""):
        reject({**environment, "WORKMUX_STATUS_INSTANCE": endpoint}, [f.server])
    partial = dict(environment)
    del partial["WORKMUX_STATUS_PANE_ID"]
    reject(partial, [f.server])

    other = HerdrServer()
    try:
        other.start()
        other_fixture = Fixture(other)
        other_pane, other_environment = launch_status_agent(other_fixture)
        other_fixture.run("set-window-status", "done", env=other_environment)
        reject(
            {**environment, "WORKMUX_STATUS_INSTANCE": str(other.socket_path)},
            [f.server, other],
        )
        reject(
            {
                **environment,
                "WORKMUX_STATUS_PANE_ID": other_environment["WORKMUX_STATUS_PANE_ID"],
            },
            [f.server, other],
        )
        assert_status(other_fixture, other_pane, "done")
    finally:
        other.close()

    f.server.request("pane.close", pane_id=moved["pane_id"])
    reject(environment, [f.server])
    f.server.stop()
    f.server.start()
    snapshot = f.server.request("session.snapshot")["snapshot"]
    parents = [w for w in snapshot["workspaces"] if w["label"] == "parent"]
    if parents:
        assert len(parents) == 1, parents
        f.parent = next(
            p
            for p in snapshot["panes"]
            if p["workspace_id"] == parents[0]["workspace_id"]
        )
    else:
        f.parent = f.server.request(
            "workspace.create", label="parent", cwd=str(f.repo), focus=True
        )["root_pane"]
    replacement, new_environment = launch_status_agent(f, "replacement")
    new_key = new_environment["WORKMUX_STATUS_PANE_ID"]
    assert new_key.split("~", 1)[0] != boot
    reject(environment, [f.server])
    # Even an existing terminal cannot be used with the old server lifetime.
    reject(
        {
            **environment,
            "WORKMUX_STATUS_PANE_ID": boot + "~" + replacement["terminal_id"],
        },
        [f.server],
    )
    f.run("set-window-status", "working", env=new_environment)
    assert_status(f, replacement, "working")
    print(
        "PASS status targets: moved terminal registration and hooks; invalid, partial, closed, foreign-endpoint and stale-lifetime targets leave native reports and state unchanged"
    )


def focus(f):
    # This is a restriction check until protocol 22 can prove UI visibility.
    # Do not count it as a passing acknowledgement acceptance test.
    pane, environment = launch_status_agent(f)
    client = f.server.attach()
    wait_until(lambda: b"parent" in client.output)
    f.server.request("pane.focus", pane_id=pane["pane_id"])
    for state in ("waiting", "done", "working"):
        f.run("set-window-status", state, env=environment)
        assert_status(f, pane, state)  # Already-focused updates also remain.
        f.server.request("pane.focus", pane_id=f.parent["pane_id"])
        f.run("open", "feature")
        wait_until(
            lambda: (
                f.server.request("session.snapshot")["snapshot"]["focused_pane_id"]
                == pane["pane_id"]
            )
        )
        time.sleep(0.3)
        assert_status(f, pane, state)
    print(
        "BLOCKED focus acknowledgement (restriction check passed): "
        "waiting/done remain for already-focused updates and after Workmux open; "
        "working remains; native reports and Workmux tracking agree"
    )


def wait(f):
    pane, environment = launch_status_agent(f)
    f.run("set-window-status", "working", env=environment)
    assert_status(f, pane, "working")
    result = f.run("wait", "feature", "--status", "done", "--timeout", "1", ok=False)
    assert result.returncode == 1 and "Timeout" in result.stderr, result
    with subprocess.Popen(
        [str(BINARY), "wait", "feature", "--status", "done", "--timeout", "10"],
        cwd=f.repo,
        env=f.env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    ) as process:
        time.sleep(0.3)
        assert process.poll() is None
        f.run("set-window-status", "done", env=environment)
        assert_status(f, pane, "done")
        stdout, stderr = process.communicate(timeout=15)
        assert process.returncode == 0 and "done" in stderr, stdout + stderr
    print(
        "PASS wait: timeout exit 1, then blocked wait released by working-to-done transition"
    )


def run_commands(f):
    pane, environment = launch_status_agent(f)
    f.run("set-window-status", "working", env=environment)
    assert_status(f, pane, "working")
    result = f.run(
        "run", "feature", "--", "sh", "-c", "printf tracked-output; exit 7", ok=False
    )
    assert result.returncode == 7 and "tracked-output" in result.stdout, result
    marker = f.server.root / "background-result"
    start = time.monotonic()
    f.run(
        "run",
        "feature",
        "--background",
        "--",
        "sh",
        "-c",
        f"sleep 2; printf background > {marker}",
    )
    assert time.monotonic() - start < 2 and not marker.exists()
    wait_until(lambda: marker.exists() and marker.read_text() == "background")
    print("PASS run: output, exit 7, and background return before command completion")


def hooks(f):
    marker = f.server.root / "hooks"
    f.config(
        {
            "panes": [{}],
            "post_create": [f"printf create >> {marker}"],
            "pre_remove": [f"printf remove >> {marker}; test -f {marker}.allow"],
        }
    )
    f.add()
    assert marker.read_text() == "create"
    result = f.run("remove", "feature", "--force", ok=False)
    assert result.returncode != 0 and marker.read_text() == "createremove", result
    assert Path(f.run("path", "feature").stdout.strip()).exists()
    worktree = Path(f.run("path", "feature").stdout.strip())
    Path(f"{marker}.allow").touch()
    f.run("remove", "feature", "--force")
    assert marker.read_text() == "createremoveremove" and not worktree.exists()
    print(
        "PASS hooks: post-create runs; pre-remove failure blocks removal; successful pre-remove allows removal"
    )


def multi(f):
    marker = f.server.root / "launches"
    stub = f.server.root / "claude"
    stub.write_text(
        "#!/bin/sh\n"
        + f.wm("register-agent")
        + "\n"
        + f.wm("set-window-status", "working")
        + "\n"
        + f"printf '%s\\n' \"$PWD\" >> {marker}\nsleep 120\n"
    )
    stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    other = f.server.request(
        "workspace.create", label="other", cwd=str(f.repo), focus=True
    )["root_pane"]
    result = f.run(
        "add", "multi", "--count", "2", "--parent-session", "parent",
        "--background", ok=False,
    )
    assert result.returncode != 0, result
    assert "cannot be used with multi-worktree generation" in result.stderr, result
    code, output = f.native(f.parent, f.wm("add", "multi", "--count", "2", "--background"))
    assert code == 0, output
    snapshot = f.server.request("session.snapshot")["snapshot"]
    tabs = [t for t in snapshot["tabs"] if t["label"].startswith("wm-multi")]
    assert {t["label"] for t in tabs} == {"wm-multi-1", "wm-multi-2"}, tabs
    assert all(t["workspace_id"] == f.parent["workspace_id"] for t in tabs)
    assert snapshot["focused_tab_id"] == other["tab_id"], snapshot
    wait_until(lambda: marker.exists() and len(marker.read_text().splitlines()) == 2)
    assert len(set(marker.read_text().splitlines())) == 2
    agents = json.loads(f.run("status", "--json").stdout)["agents"]
    assert len(agents) == 2 and all(a["status"] == "working" for a in agents), agents
    reports = f.server.request("session.snapshot")["snapshot"]["panes"]
    for agent in agents:
        report = next(
            p for p in reports if agent["pane_id"].endswith("~" + p["terminal_id"])
        )
        assert report.get("agent_status") == "working", report
    print(
        "PASS multi: external --count with explicit parent is refused; native-pane launches retain background focus, register and report working in separate worktrees"
    )


def multi_session_generation(f):
    multi_generation(f, session_mode=True)


def multi_generation(f, session_mode=False):
    stub = f.server.root / "claude"
    stub.write_text(
        "#!/bin/sh\n"
        + f.wm("register-agent")
        + "\n"
        + f.wm("set-window-status", "working")
        + "\nsleep 120\n"
    )
    stub.chmod(0o700)
    second_stub = f.server.root / "codex"
    second_stub.write_text(stub.read_text())
    second_stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    other = f.server.request(
        "workspace.create", label="other", cwd=str(f.repo), focus=True
    )["root_pane"]
    prompt = f.server.root / "prompt.md"
    prompt.write_text("---\nforeach:\n  item: [red, blue]\n---\nDo {{ item }}\n")
    cases = (
        (
            "agents",
            ["--agent", str(stub), "--agent", str(second_stub)],
            None,
            {"agents-claude", "agents-codex"},
        ),
        ("matrix", ["--foreach", "item:red,blue"], None, {"matrix-red", "matrix-blue"}),
        ("stdin", [], "red\nblue\n", {"stdin-red", "stdin-blue"}),
        (
            "frontmatter",
            ["--prompt-file", str(prompt)],
            None,
            {"frontmatter-red", "frontmatter-blue"},
        ),
    )
    expected = set()
    for base, args, stdin, names in cases:
        if session_mode:
            f.run("add", base, *args, "--session", "--background", input=stdin)
        else:
            before = f.server.request("session.snapshot")["snapshot"]
            worktrees_before = f.run("list").stdout
            result = f.run(
                "add", base, *args, "--parent-session", "parent", "--background",
                input=stdin, ok=False,
            )
            if base == "frontmatter":
                # Frontmatter expansion occurs after the explicit-target guard.
                assert result.returncode == 0, result.stdout + result.stderr
            else:
                assert result.returncode != 0, result
                assert "cannot be used with multi-worktree generation" in result.stderr, result
                after = f.server.request("session.snapshot")["snapshot"]
                assert after["tabs"] == before["tabs"]
                assert f.run("list").stdout == worktrees_before
                command = f.wm("add", base, *args, "--background")
                if stdin is not None:
                    command = f"printf %s {shlex.quote(stdin)} | {command}"
                code, output = f.native(f.parent, command)
                assert code == 0, output
        expected |= names

        def working(expected=expected):
            agents = json.loads(f.run("status", "--json").stdout)["agents"]
            return {
                a["worktree"] for a in agents if a["status"] == "working"
            } == expected

        wait_until(working)
        snapshot = f.server.request("session.snapshot")["snapshot"]
        if session_mode:
            workspaces = [w for w in snapshot["workspaces"] if w["label"].startswith("wm-")]
            assert {w["label"] for w in workspaces} == {"wm-" + n for n in expected}, workspaces
            workspace_ids = {w["workspace_id"] for w in workspaces}
            tabs = [t for t in snapshot["tabs"] if t["workspace_id"] in workspace_ids]
            assert len(tabs) == len(expected), tabs
            assert {t["workspace_id"] for t in tabs} == workspace_ids, tabs
        else:
            tabs = [t for t in snapshot["tabs"] if t["label"].startswith("wm-")]
            assert {t["label"] for t in tabs} == {"wm-" + n for n in expected}, tabs
            assert all(t["workspace_id"] == f.parent["workspace_id"] for t in tabs)
        assert snapshot["focused_tab_id"] == other["tab_id"], snapshot
        assert snapshot["focused_workspace_id"] == other["workspace_id"], snapshot
        reports = [
            p for p in snapshot["panes"] if p["tab_id"] in {t["tab_id"] for t in tabs}
        ]
        assert len(reports) == len(expected) and all(
            p.get("agent_status") == "working" for p in reports
        ), reports
    print(
        f"PASS multi-generation ({'session' if session_mode else 'window'}): "
        "multiple --agent, --foreach, stdin and frontmatter; correct placement, "
        "unique names, background focus and normal working status; "
        + ("external session calls" if session_mode else "explicit parent refused for agent/foreach/stdin; native calls and external frontmatter pass")
    )


def multi_failure(f):
    duplicate = f.server.request(
        "workspace.create", label="parent", cwd=str(f.repo), focus=True
    )["root_pane"]
    before = f.server.request("session.snapshot")["snapshot"]
    result = f.run(
        "add", "ambiguous", "--count", "2", "--parent-session", "parent", ok=False
    )
    assert result.returncode != 0 and "cannot be used with multi-worktree generation" in result.stderr, result
    after = f.server.request("session.snapshot")["snapshot"]
    assert after["tabs"] == before["tabs"] and after["panes"] == before["panes"]
    f.server.request(
        "workspace.rename", workspace_id=duplicate["workspace_id"], label="other"
    )
    # Inherited IDs and arbitrary focus still cannot replace a verified caller.
    result = f.run(
        "add",
        "implicit",
        "--count",
        "2",
        ok=False,
        env={**f.env, "HERDR_PANE_ID": f.parent["pane_id"]},
    )
    assert result.returncode != 0 and "No verified Herdr caller" in result.stderr, (
        result
    )
    # A user tab with a generated name must survive the normal collision suffix.
    foreign = f.server.request(
        "tab.create",
        workspace_id=duplicate["workspace_id"],
        label="wm-collision-1",
        cwd=str(f.repo),
        focus=False,
    )["root_pane"]
    code, output = f.native(
        f.parent, f.wm("add", "collision", "--count", "2", "--background")
    )
    assert code == 0, output
    snapshot = f.server.request("session.snapshot")["snapshot"]
    owned = [
        t for t in snapshot["tabs"] if t["workspace_id"] == f.parent["workspace_id"]
    ]
    assert {t["label"] for t in owned} >= {"wm-collision-1-repo", "wm-collision-2"}, (
        owned
    )
    for name in ("collision-1-repo", "collision-2"):
        f.run("remove", name, "--force")
    assert any(
        p["terminal_id"] == foreign["terminal_id"]
        for p in f.server.request("session.snapshot")["snapshot"]["panes"]
    )
    # Duplicate generated names fail, rather than sharing one target or replacing it.
    code, output = f.native(
        f.parent, f.wm("add", "duplicate", "--foreach", "item:same,same", "--background")
    )
    assert code != 0 and "already exists" in output, output
    snapshot = f.server.request("session.snapshot")["snapshot"]
    assert len([t for t in snapshot["tabs"] if t["label"] == "wm-duplicate-same"]) == 1
    f.run("remove", "duplicate-same", "--force")
    # Fail the second launch after its tab allocation; keep both usable layouts.
    hook = f.server.root / "second-layout.sh"
    hook.write_text(
        '#!/bin/sh\ncase "$PWD" in *partial-1) '
        'printf \'%s\' \'{"panes":[{}, {"split":"horizontal", "percentage":5}]}\' '
        f"> {shlex.quote(str(f.repo / '.workmux.yaml'))};; esac\n"
    )
    hook.chmod(0o700)
    f.config({"panes": [{}], "post_create": [str(hook)]})
    code, output = f.native(
        f.parent, f.wm("add", "partial", "--count", "3", "--background")
    )
    assert code != 0 and "limits each split to 10–90%" in output, output
    snapshot = f.server.request("session.snapshot")["snapshot"]
    tabs = [t for t in snapshot["tabs"] if t["label"].startswith("wm-partial")]
    assert {t["label"] for t in tabs} == {"wm-partial-1", "wm-partial-2"}, tabs
    assert all(t["workspace_id"] == f.parent["workspace_id"] for t in tabs)
    assert snapshot["focused_tab_id"] == duplicate["tab_id"], snapshot
    for name in ("partial-1", "partial-2"):
        assert Path(f.run("path", name).stdout.strip()).is_dir()
        f.run("remove", name, "--force")
    snapshot = f.server.request("session.snapshot")["snapshot"]
    assert {p["terminal_id"] for p in snapshot["panes"]} == {
        f.parent["terminal_id"],
        duplicate["terminal_id"],
        foreign["terminal_id"],
    }, snapshot
    print(
        "PASS multi-failure: explicit parent refused without tabs; no inherited-ID fallback; native-pane "
        "collision suffix, duplicate names and partial launch preserve user layouts; owned cleanup"
    )


def multi_open(f):
    for name in ("first", "last"):
        f.run("add", name, "--headless")
    other = f.server.request(
        "workspace.create", label="parent", cwd=str(f.repo), focus=True
    )["root_pane"]
    before = f.server.request("session.snapshot")["snapshot"]
    result = f.run("open", "first", "last", "--parent-session", "parent", ok=False)
    assert result.returncode != 0, result
    assert "cannot be used when opening multiple worktrees" in result.stderr, result
    after = f.server.request("session.snapshot")["snapshot"]
    assert after["tabs"] == before["tabs"] and after["panes"] == before["panes"]
    f.server.request(
        "workspace.rename", workspace_id=other["workspace_id"], label="other"
    )
    code, output = f.native(f.parent, f.wm("open", "first", "last"))
    assert code == 0, output
    snapshot = f.server.request("session.snapshot")["snapshot"]
    tabs = [t for t in snapshot["tabs"] if t["label"] in {"wm-first", "wm-last"}]
    assert len(tabs) == 2, tabs
    assert all(t["workspace_id"] == f.parent["workspace_id"] for t in tabs)
    assert snapshot["focused_tab_id"] == next(
        t["tab_id"] for t in tabs if t["label"] == "wm-last"
    )
    # Open continues after one failure, unlike add, which stops on failure.
    for name in ("first", "last"):
        f.run("close", name)
    code, output = f.native(f.parent, f.wm("open", "first", "missing", "last"))
    assert code != 0 and "Failed to open 1 of 3" in output, output
    snapshot = f.server.request("session.snapshot")["snapshot"]
    tabs = [t for t in snapshot["tabs"] if t["label"] in {"wm-first", "wm-last"}]
    assert len(tabs) == 2 and all(
        t["workspace_id"] == f.parent["workspace_id"] for t in tabs
    ), snapshot
    # Repeated names switch to the owned target; --new makes distinct targets.
    code, output = f.native(f.parent, f.wm("open", "first", "first"))
    assert code == 0, output
    assert len(f.server.request("session.snapshot")["snapshot"]["tabs"]) == len(
        snapshot["tabs"]
    )
    code, output = f.native(f.parent, f.wm("open", "first", "first", "--new"))
    assert code == 0, output
    snapshot = f.server.request("session.snapshot")["snapshot"]
    tabs = [t for t in snapshot["tabs"] if t["label"].startswith("wm-first")]
    assert {t["label"] for t in tabs} == {"wm-first", "wm-first-2", "wm-first-3"}
    assert all(t["workspace_id"] == f.parent["workspace_id"] for t in tabs)
    for name in ("first", "last"):
        f.run("remove", name, "--force")
    snapshot = f.server.request("session.snapshot")["snapshot"]
    assert {p["terminal_id"] for p in snapshot["panes"]} == {
        f.parent["terminal_id"],
        other["terminal_id"],
    }, snapshot
    print(
        "PASS multi-open: external explicit parent refused; native-pane placement, foreground focus, partial failure, "
        "repeated names, --new suffixes and owned cleanup preserve user panes"
    )


def continue_fork(f):
    for agent, expected in (
        ("claude", ["--continue"]),
        ("codex", ["resume", "--last"]),
        ("pi", ["--continue"]),
    ):
        marker = f.server.root / f"{agent}-args"
        stub = f.server.root / agent
        stub.write_text(
            "#!/bin/sh\n"
            + f.wm("register-agent")
            + "\n"
            + f.wm("set-window-status", "working")
            + "\n"
            + f"printf '%s\\n' \"$@\" > {marker}\n"
        )
        stub.chmod(0o700)
        f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
        f.run(
            "add",
            f"resume-{agent}",
            "--continue",
            "--parent-session",
            "parent",
            "--background",
        )
        wait_until(lambda marker=marker: marker.exists() and marker.read_text())
        assert marker.read_text().splitlines() == expected, marker.read_text()
        agents = json.loads(f.run("status", "--json").stdout)["agents"]
        resumed = [a for a in agents if a["worktree"] == f"resume-{agent}"]
        assert len(resumed) == 1 and resumed[0]["status"] == "working", agents
    f.config(
        {"agent": str(f.server.root / "claude"), "panes": [{"command": "<agent>"}]}
    )
    encoded = "".join(
        c if c.isalnum() or c == "-" else "-" for c in str(f.repo.resolve())
    )
    source = Path(f.env["HOME"]) / ".claude/projects" / encoded / "test-session.jsonl"
    source.parent.mkdir(parents=True)
    source.write_text('{"type":"message"}\n')
    marker = f.server.root / "claude-args"
    marker.unlink()
    f.run(
        "add",
        "forked",
        "--fork=test-session",
        "--parent-session",
        "parent",
        "--background",
    )
    wait_until(lambda: marker.exists() and marker.read_text())
    assert "--resume" in marker.read_text(), marker.read_text()
    copies = list(source.parent.parent.glob("*forked/*.jsonl"))
    assert len(copies) == 1 and copies[0].read_text() == source.read_text(), copies
    agents = json.loads(f.run("status", "--json").stdout)["agents"]
    forked = [a for a in agents if a["worktree"] == "forked"]
    assert len(forked) == 1 and forked[0]["status"] == "working", agents
    print(
        "PASS continue/fork: Claude/Codex/pi resume flags; Claude conversation copy and --resume launch (stubs, no real agent session)"
    )


def dashboard(f):
    pane, environment = launch_status_agent(f, hold=False)
    f.run("set-window-status", "done", env=environment)
    assert_status(f, pane, "done")
    f.server.request("pane.focus", pane_id=f.parent["pane_id"])
    f.server.attach()
    done = f.server.root / "dashboard-exit"
    command = f.wm("dashboard") + f"; printf '%s' $? > {done}"
    f.server.request(
        "pane.send_input", pane_id=f.parent["pane_id"], text=command, keys=["enter"]
    )

    def screen():
        return f.server.request(
            "pane.read", pane_id=f.parent["pane_id"], source="visible"
        )["read"]["text"]

    wait_until(lambda: "feature" in screen() and "✅" in screen(), timeout=15)
    f.server.request("pane.send_keys", pane_id=f.parent["pane_id"], keys=["enter"])
    wait_until(lambda: done.exists() and done.read_text() == "0")
    snapshot = f.server.request("session.snapshot")["snapshot"]
    assert snapshot["focused_tab_id"] == pane["tab_id"], snapshot
    print(
        "PASS dashboard: renders tracked agent and done status; Enter exits and focuses agent"
    )
    return pane


def navigation(f):
    pane = dashboard(f)
    code, output = f.native(pane, f.wm("last-agent"))
    assert code == 0, output
    assert (
        f.server.request("session.snapshot")["snapshot"]["focused_tab_id"]
        == f.parent["tab_id"]
    )
    code, output = f.native(f.parent, f.wm("last-agent"))
    assert code == 0, output
    assert (
        f.server.request("session.snapshot")["snapshot"]["focused_tab_id"]
        == pane["tab_id"]
    )
    code, output = f.native(pane, f.wm("last-agent"))
    assert code == 0, output
    f.run("close", "feature")
    code, output = f.native(f.parent, f.wm("last-agent"))
    assert code == 0 and "no longer exists" in output, output
    print(
        "PASS last-agent: dashboard creates history; two-way toggle and closed-target handling"
    )


def recovery_files(f):
    state = Path(f.env["XDG_STATE_HOME"]) / "workmux"
    return {
        path: path.read_bytes()
        for directory in ("agents", "agent-recovery")
        for path in (state / directory).rglob("*.json")
    }


def resurrect(f):
    pane, environment = launch_status_agent(f)
    # Resolve the recorded agent kind to the private stub on later restores.
    f.config(
        {
            "agent": str(f.server.root / "claude"),
            "agents": {"claude": str(f.server.root / "claude")},
            "panes": [{"command": "<agent>"}],
        }
    )
    ready = f.server.root / "feature-environment.json"
    original_key = environment["WORKMUX_STATUS_PANE_ID"]
    f.server.request("tab.close", tab_id=pane["tab_id"])
    ready.unlink()
    result = f.run("resurrect", "--dry-run")
    assert "would restore 1" in result.stdout, result
    assert "Restored 1" in f.run("resurrect").stdout
    wait_until(lambda: ready.exists() and ready.read_text())
    old_key = json.loads(ready.read_text())["WORKMUX_STATUS_PANE_ID"]
    assert old_key != original_key
    print("PASS resurrect: closed tab restored from ordinary agent registration")

    f.server.stop()
    restarted = f.server.start()["snapshot"]
    # Herdr restores workspace labels. The original probe made two parents.
    parents = [w for w in restarted["workspaces"] if w["label"] == "parent"]
    assert len(parents) == 1, restarted
    replacement = f.server.request(
        "workspace.create", label="parent", cwd=str(f.repo), focus=True
    )["root_pane"]
    before = f.server.request("session.snapshot")["snapshot"]
    recovery = recovery_files(f)
    assert recovery
    result = f.run("resurrect", ok=False)
    print(result.stdout + result.stderr, flush=True)
    assert result.returncode != 0, result
    assert "Failed to create window in session" in result.stderr, result
    # The shared command prints only the outer error. Adapter unit tests check
    # the detailed ambiguity error; here check no allocation or recovery loss.
    after = f.server.request("session.snapshot")["snapshot"]
    assert after["tabs"] == before["tabs"]
    assert recovery_files(f) == recovery
    assert "would restore 1" in f.run("resurrect", "--dry-run").stdout
    # Keep the native restored layout: the replacement is only a destination.
    f.server.request(
        "workspace.rename", workspace_id=parents[0]["workspace_id"], label="retained"
    )
    ready.unlink()
    assert "Restored 1" in f.run("resurrect").stdout
    wait_until(lambda: ready.exists() and ready.read_text())
    new_environment = json.loads(ready.read_text())
    new_key = new_environment["WORKMUX_STATUS_PANE_ID"]
    assert new_key.split("~", 1)[0] != old_key.split("~", 1)[0]
    snapshot = f.server.request("session.snapshot")["snapshot"]
    pane = next(
        p for p in snapshot["panes"] if new_key.endswith("~" + p["terminal_id"])
    )
    assert pane["workspace_id"] == replacement["workspace_id"], snapshot
    assert pane["terminal_id"] not in {p["terminal_id"] for p in restarted["panes"]}
    assert {p["terminal_id"] for p in restarted["panes"]} <= {
        p["terminal_id"] for p in snapshot["panes"]
    }, snapshot
    assert all(old_key.encode() not in data for data in recovery_files(f).values())
    for _ in range(2):
        result = f.run("resurrect")
        assert (
            "skipping (already open)" in result.stdout
            and "Nothing to restore" in result.stdout
        )
        assert (
            f.server.request("session.snapshot")["snapshot"]["tabs"] == snapshot["tabs"]
        )
    f.run("set-window-status", "working", env=new_environment)
    assert_status(f, pane, "working")
    print(
        "PASS restart: unique replacement parent, fresh identity, retained native panes, repeat restore, and status"
    )


def resurrect_failure(f):
    pane = f.add()
    code, output = f.native(pane, f.wm("set-window-status", "done"))
    assert code == 0, output
    f.server.request("tab.close", tab_id=pane["tab_id"])
    recovery = recovery_files(f)
    assert recovery
    # Fail after allocation: Herdr cannot preserve a split outside 10--90%.
    f.config({"panes": [{}, {"split": "horizontal", "percentage": 5}]})
    result = f.run("resurrect", ok=False)
    assert result.returncode != 0 and "Failed to setup panes" in result.stderr, result
    assert recovery_files(f) == recovery
    # As with normal setup, a partial layout can retain a usable shell. Repeat
    # restore must not overwrite it or consume the failed attempt's recovery.
    partial = f.server.request("session.snapshot")["snapshot"]
    assert any(t["label"] == "wm-feature" for t in partial["tabs"])
    assert "skipping (already open)" in f.run("resurrect").stdout
    assert recovery_files(f) == recovery
    assert f.server.request("session.snapshot")["snapshot"]["tabs"] == partial["tabs"]
    f.run("close", "feature")
    assert recovery_files(f) == recovery
    assert "would restore 1" in f.run("resurrect", "--dry-run").stdout
    # The shared setup contract creates a missing named parent. It does not
    # choose the focused workspace. Preserve the existing fixture workspace.
    f.server.request(
        "workspace.rename", workspace_id=f.parent["workspace_id"], label="retained"
    )
    f.config({"panes": [{}]})
    assert "Restored 1" in f.run("resurrect").stdout
    snapshot = f.server.request("session.snapshot")["snapshot"]
    parent = next(w for w in snapshot["workspaces"] if w["label"] == "parent")
    assert parent["workspace_id"] != f.parent["workspace_id"]
    assert any(
        t["label"] == "wm-feature" and t["workspace_id"] == parent["workspace_id"]
        for t in snapshot["tabs"]
    )
    assert any(p["terminal_id"] == f.parent["terminal_id"] for p in snapshot["panes"])
    assert all(
        pane["terminal_id"].encode() not in data for data in recovery_files(f).values()
    )
    print(
        "PASS resurrect failure: partial layout is not replaced, recovery retained, close and retry creates missing named parent"
    )


def reap(f):
    ready = f.server.root / "ready"
    stopped = f.server.root / "stopped"
    stub = f.server.root / "claude"
    stub.write_text(
        f"#!{sys.executable}\nimport os, signal, subprocess, time\n"
        f"subprocess.run([{str(BINARY)!r}, 'register-agent'], check=True)\n"
        f"subprocess.run([{str(BINARY)!r}, 'set-window-status', 'working'], check=True)\n"
        f"open({str(ready)!r}, 'w').write('ready')\n"
        "try:\n    while True: time.sleep(1)\n"
        f"except KeyboardInterrupt: open({str(stopped)!r}, 'w').write('interrupt')\n"
    )
    stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    f.add()
    wait_until(lambda: ready.exists())
    # Age only this private fixture's state; do not wait an hour in a test.
    files = list(Path(f.env["XDG_STATE_HOME"]).glob("workmux/agents/*.json"))
    assert len(files) == 1, files
    state = json.loads(files[0].read_text())
    state["updated_ts"] -= 7200
    files[0].write_text(json.dumps(state))
    result = f.run("reap-agents", "--hours", "1")
    assert "Would exit" in result.stdout and not stopped.exists(), result
    result = f.run("reap-agents", "--hours", "1", "--force")
    assert "Exited" in result.stdout and stopped.read_text() == "interrupt", result
    assert not json.loads(f.run("status", "--json").stdout)["agents"]
    print(
        "PASS reap: dry-run leaves stub alive; force sends Ctrl-C, observes exit, removes tracked state"
    )


def reap_eof(f):
    ready = f.server.root / "eof-ready"
    interrupted = f.server.root / "interrupted"
    stopped = f.server.root / "eof-stopped"
    stub = f.server.root / "claude"
    stub.write_text(
        f"#!{sys.executable}\nimport os, signal, subprocess\n"
        f"signal.signal(signal.SIGINT, lambda *_: open({str(interrupted)!r}, 'w').write('interrupt'))\n"
        f"subprocess.run([{str(BINARY)!r}, 'register-agent'], check=True)\n"
        f"subprocess.run([{str(BINARY)!r}, 'set-window-status', 'working'], check=True)\n"
        f"open({str(ready)!r}, 'w').write('ready')\n"
        "while os.read(0, 4096): pass\n"
        f"open({str(stopped)!r}, 'w').write('eof')\n"
    )
    stub.chmod(0o700)
    f.config({"agent": str(stub), "panes": [{"command": "<agent>"}]})
    pane = f.add()
    wait_until(lambda: ready.exists())
    files = list(Path(f.env["XDG_STATE_HOME"]).glob("workmux/agents/*.json"))
    assert len(files) == 1, files
    state = json.loads(files[0].read_text())
    state["updated_ts"] -= 7200
    files[0].write_text(json.dumps(state))
    result = f.run("reap-agents", "--hours", "1")
    assert "Would exit" in result.stdout
    assert not interrupted.exists() and not stopped.exists()
    result = f.run("reap-agents", "--hours", "1", "--force")
    assert "Exited" in result.stdout, result
    assert interrupted.read_text() == "interrupt"
    assert stopped.read_text() == "eof"
    assert not files[0].exists()
    assert not json.loads(f.run("status", "--json").stdout)["agents"]
    snapshot = f.server.request("session.snapshot")["snapshot"]
    assert {pane["terminal_id"], f.parent["terminal_id"]} <= {
        p["terminal_id"] for p in snapshot["panes"]
    }
    code, output = f.native(pane, "printf shell-alive")
    assert code == 0 and output == "shell-alive", output
    print(
        "PASS reap-eof: dry-run sends no signal; Ctrl-C is received but does not exit; Ctrl-D exits the agent, removes state and preserves the shell"
    )


def popup(f):
    marker = f.server.root / "popup-result"
    output = f.server.root / "popup-output"
    script = f.server.root / "popup.sh"
    script.write_text(
        "#!/bin/sh\n"
        + f.wm("add", "from-popup", "--background")
        + f" > {output} 2>&1\nprintf '%s' $? > {marker}.implicit\n"
        + f.wm(
            "add", "explicit-popup", "--background", "--parent-session", "popup-parent"
        )
        + f" >> {output} 2>&1\nprintf '%s' $? > {marker}\n"
    )
    script.chmod(0o700)
    f.server.stop()
    with f.server.config_path.open("a") as config:
        config.write(
            '\n[[keys.command]]\nkey = "ctrl+g"\ntype = "popup"\ncommand = '
            + json.dumps(str(script))
            + "\n"
        )
    f.server.start()
    f.server.request(
        "workspace.create", label="popup-parent", cwd=str(f.repo), focus=True
    )
    client = f.server.attach()
    wait_until(lambda: client.output)
    time.sleep(0.5)
    client.send(b"\x07")
    wait_until(lambda: marker.exists() and marker.read_text(), timeout=30)
    assert (
        Path(f"{marker}.implicit").read_text() != "0"
        and "No verified Herdr caller" in output.read_text()
    ), output.read_text()
    assert marker.read_text() == "0", output.read_text()
    snapshot = f.server.request("session.snapshot")["snapshot"]
    tab = next(t for t in snapshot["tabs"] if t["label"] == "wm-explicit-popup")
    parent = next(w for w in snapshot["workspaces"] if w["label"] == "popup-parent")
    assert tab["workspace_id"] == parent["workspace_id"]
    print(
        "PASS popup limitation: script launch needs explicit --parent-session; explicit workspace creates correct tab"
    )


CASES = {
    "session": session,
    "session-layout": session_layout,
    "session-recovery": session_recovery,
    "session-navigation": session_navigation,
    "sidebar": sidebar,
    "status": status,
    "status-targets": status_targets,
    "focus": focus,
    "focus-protocol": focus_protocol,
    "wait": wait,
    "run": run_commands,
    "hooks": hooks,
    "merge-conflicts": merge_conflicts,
    "merge-squash": merge_squash,
    "rebase-conflicts": rebase_conflicts,
    "merge-hooks": merge_hooks,
    "session-merge-conflicts": session_merge_conflicts,
    "session-merge-squash": session_merge_squash,
    "session-rebase-conflicts": session_rebase_conflicts,
    "patch-split": patch_split,
    "reap-eof": reap_eof,
    "reap-unresponsive": reap_unresponsive,
    "multi": multi,
    "multi-open": multi_open,
    "multi-generation": multi_generation,
    "multi-session-generation": multi_session_generation,
    "multi-failure": multi_failure,
    "continue-fork": continue_fork,
    "dashboard": dashboard,
    "dashboard-input": dashboard_input,
    "dashboard-kill": dashboard_kill,
    "dashboard-worktrees": dashboard_worktrees,
    "dashboard-sweep": dashboard_sweep,
    "diff-actions": diff_actions,
    "navigation": navigation,
    "resurrect": resurrect,
    "resurrect-failure": resurrect_failure,
    "reap": reap,
    "popup": popup,
}


def main():
    for name in sys.argv[1:] or CASES:
        server = HerdrServer()
        try:
            server.start()
            CASES[name](Fixture(server))
        finally:
            server.close()


if __name__ == "__main__":
    main()
