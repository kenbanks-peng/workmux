"""Opt-in live Pi test. Uses existing Pi auth and makes paid model requests.

Run after cargo build:
    python3 src/multiplexer/herdr/integration/pi_diff_checks.py

Only auth.json is shared with the user's Pi configuration (normal token refresh
can update it). All other configuration, sessions, repositories and panes are
private. Only the repository's Workmux status extension is loaded. No user
plugins, skills, context files or remote Git repositories are used.
"""

import json
import os
import shlex
import shutil
import subprocess
from pathlib import Path

from server import HerdrServer, wait_until
from support_checks import BINARY, ROOT, Fixture


def run(f):
    pi = shutil.which("pi")
    assert pi, "Pi is required"
    original_config = Path(
        os.environ.get("PI_CODING_AGENT_DIR", str(Path.home() / ".pi/agent"))
    )
    auth = original_config / "auth.json"
    assert auth.is_file(), "Pi auth.json is required"
    config = f.server.root / "pi-config"
    config.mkdir(mode=0o700)
    (config / "auth.json").symlink_to(auth.resolve())
    (config / "settings.json").write_text(json.dumps({"enableInstallTelemetry": False}))
    default_model = (
        os.environ.get("PI_PROVIDER", "openai-codex")
        + "/"
        + os.environ.get("PI_MODEL", "gpt-6-astra")
    )
    model = os.environ.get("WORKMUX_TEST_PI_MODEL", default_model)
    wrapper = f.server.root / "pi"
    path = str(BINARY.parent) + os.pathsep + os.environ["PATH"]
    wrapper.write_text(
        "#!/bin/sh\n"
        + "export PATH="
        + shlex.quote(path)
        + "\n"
        + "export PI_CODING_AGENT_DIR="
        + shlex.quote(str(config))
        + "\n"
        + "exec "
        + shlex.join(
            [
                pi,
                "--offline",
                "--no-extensions",
                "--extension",
                str(ROOT / "resources/pi/extensions/workmux-status.ts"),
                "--no-skills",
                "--no-prompt-templates",
                "--no-context-files",
                "--no-themes",
                "--no-approve",
                "--model",
                model,
                "--thinking",
                "low",
            ]
        )
        + "\n"
    )
    wrapper.chmod(0o700)
    f.config({"agent": str(wrapper), "panes": [{"command": "<agent>"}]})

    def git(*args, cwd=None):
        return subprocess.run(
            ["git", *args],
            cwd=cwd or f.repo,
            env=f.env,
            capture_output=True,
            text=True,
            check=True,
            timeout=15,
        ).stdout.strip()

    (f.repo / "example.txt").write_text("before\n")
    git("add", ".")
    git("commit", "-m", "live Pi fixture")
    initial = git("rev-parse", "HEAD")
    pane = f.add()
    worktree = Path(f.run("path", "feature").stdout.strip())
    source = worktree / "example.txt"
    source.write_text("after\n")
    f.server.attach()

    def screen(target=None):
        return f.server.request(
            "pane.read", pane_id=(target or f.parent)["pane_id"], source="visible"
        )["read"]["text"]

    def key(value):
        f.server.request("pane.send_keys", pane_id=f.parent["pane_id"], keys=[value])

    def wait(check, label, timeout=120):
        try:
            wait_until(check, timeout=timeout)
        except TimeoutError:
            print("FAILED:", label, flush=True)
            for target in (f.parent, pane):
                try:
                    print(screen(target), flush=True)
                except RuntimeError:
                    pass
            raise

    wait(lambda: model.split("/")[-1] in screen(pane), "Pi startup", 40)
    f.server.request("pane.focus", pane_id=f.parent["pane_id"])
    done = f.server.root / "dashboard-exit"
    f.server.request(
        "pane.send_input",
        pane_id=f.parent["pane_id"],
        text=f.wm("dashboard") + f"; printf '%s' $? > {shlex.quote(str(done))}",
        keys=["enter"],
    )
    wait(lambda: "feature" in screen(), "dashboard", 15)
    key("d")
    wait(lambda: "example.txt" in screen() and "after" in screen(), "WIP diff", 15)
    key("a")
    key("o")
    wait(lambda: "Send" in screen(), "comment editor", 15)
    comment = "Set example.txt to reviewed followed by a newline. Do not commit."
    for character in comment:
        key("space" if character == " " else character)
    key("enter")
    wait(lambda: source.read_text() == "reviewed\n", "Pi acts on diff comment")
    assert git("rev-parse", "HEAD", cwd=worktree) == initial
    print("PASS Send: Pi changed example.txt as requested; no commit", flush=True)

    # The default commit prompt asks for staged changes. Stage through patch mode.
    key("q")
    key("q")
    key("d")
    wait(lambda: "reviewed" in screen(), "updated WIP diff", 15)
    key("a")
    key("y")
    wait(
        lambda: git("diff", "--cached", "--name-only", cwd=worktree) == "example.txt",
        "stage hunk",
        15,
    )
    key("c")
    wait(lambda: git("rev-parse", "HEAD", cwd=worktree) != initial, "Pi creates commit")
    commit = git("rev-parse", "HEAD", cwd=worktree)
    assert git("show", "HEAD:example.txt", cwd=worktree) == "reviewed"
    assert git("status", "--porcelain", cwd=worktree) == ""
    assert git("rev-list", "--count", f"{initial}..HEAD", cwd=worktree) == "1"
    print(
        "PASS Commit:",
        commit,
        git("log", "-1", "--format=%s", cwd=worktree),
        flush=True,
    )

    key("d")
    wait(lambda: "No uncommitted changes" in screen(), "empty WIP diff", 15)
    key("tab")
    wait(lambda: "Review:" in screen() and "reviewed" in screen(), "branch diff", 15)
    key("m")
    wait(
        lambda: git("show", "main:example.txt") == "reviewed",
        "Pi runs default merge command",
    )
    wait(lambda: not worktree.exists(), "merge worktree cleanup", 40)
    wait(
        lambda: all(
            p["terminal_id"] != pane["terminal_id"]
            for p in f.server.request("session.snapshot")["snapshot"]["panes"]
        ),
        "merge terminal cleanup",
        40,
    )
    assert git("branch", "--list", "feature") == ""
    assert git("merge-base", "--is-ancestor", commit, "main") == ""
    assert git("status", "--porcelain") == ""
    key("q")
    wait(lambda: done.exists() and done.read_text() == "0", "dashboard exit", 15)
    print(
        "PASS Merge: Pi ran !workmux merge; main contains the commit; branch, worktree and terminal removed; dashboard exits 0",
        flush=True,
    )
    print("Pi model:", model, flush=True)


if __name__ == "__main__":
    server = HerdrServer()
    try:
        server.start()
        run(Fixture(server))
    finally:
        server.close()
