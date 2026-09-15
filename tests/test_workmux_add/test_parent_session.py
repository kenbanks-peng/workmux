"""Shared option checks for explicit multi-worktree parents."""

from pathlib import Path

import pytest

from ..conftest import (
    TmuxEnvironment,
    run_workmux_command,
    write_workmux_config,
)


@pytest.mark.tmux_only
def test_add_count_uses_explicit_parent(
    mux_server: TmuxEnvironment, workmux_exe_path: Path, mux_repo_path: Path
):
    write_workmux_config(mux_repo_path, panes=[{}])
    mux_server.tmux(["new-session", "-d", "-s", "WalkingMate"])
    run_workmux_command(
        mux_server,
        workmux_exe_path,
        mux_repo_path,
        "add multi --count 2 --parent-session WalkingMate --background",
    )
    windows = mux_server.tmux(
        ["list-windows", "-t", "WalkingMate:", "-F", "#{window_name}"]
    ).stdout.splitlines()
    assert {"wm-multi-1", "wm-multi-2"} <= set(windows)
    for name in ("multi-1", "multi-2"):
        parent = mux_server.run_command(
            ["git", "config", "--get", f"workmux.worktree.{name}.window-session"],
            cwd=mux_repo_path,
        )
        assert parent.stdout.strip() == "WalkingMate"


@pytest.mark.parametrize(
    ("options", "stdin", "error"),
    [
        ("--count 2 --target-name shared", None, "--target-name cannot be used"),
        (
            "--agent claude --agent codex --target-name shared",
            None,
            "--target-name cannot be used",
        ),
        (
            "--foreach item:red,blue --target-name shared",
            None,
            "--target-name cannot be used",
        ),
        ("--target-name shared", "red\nblue\n", "--target-name cannot be used"),
        ("--count 2 --name shared", None, "--name cannot be used"),
        ("--count 2 --agent claude --agent codex", None, "--count can only be used"),
        ("--foreach item:red,blue", "red\nblue\n", "Cannot use --foreach when piping"),
        ("--count 2 --session", None, "--parent-session requires window mode"),
        ("--count 2 --headless", None, "--headless"),
    ],
)
def test_add_parent_keeps_other_option_checks(
    mux_server, workmux_exe_path: Path, mux_repo_path: Path, options, stdin, error
):
    write_workmux_config(mux_repo_path, panes=[{}])
    result = run_workmux_command(
        mux_server,
        workmux_exe_path,
        mux_repo_path,
        f"add rejected --parent-session parent --no-pane-cmds {options}",
        stdin_input=stdin,
        expect_fail=True,
    )
    assert error in result.stderr
    branches = mux_server.run_command(
        ["git", "branch", "--list", "rejected*"], cwd=mux_repo_path
    )
    assert not branches.stdout.strip()


def test_add_multi_parent_still_validates_parent_name(
    mux_server, workmux_exe_path: Path, mux_repo_path: Path
):
    write_workmux_config(mux_repo_path, panes=[{}])
    result = run_workmux_command(
        mux_server,
        workmux_exe_path,
        mux_repo_path,
        "add rejected --count 2 --parent-session invalid:parent",
        expect_fail=True,
    )
    assert "Parent session cannot contain ':'" in result.stderr
