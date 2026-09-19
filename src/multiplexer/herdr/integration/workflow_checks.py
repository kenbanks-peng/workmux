"""CLI workflow checks against real Git repositories and private Herdr servers."""

import json
import shlex
import subprocess
import sys
from pathlib import Path


def git(f, cwd, *args):
    result = subprocess.run(
        ["git", *args],
        cwd=cwd,
        env=f.env,
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    return result.stdout.strip()


def commit(f, cwd, content):
    (cwd / "change.txt").write_text(content + "\n")
    git(f, cwd, "add", "change.txt")
    git(f, cwd, "commit", "-m", content)
    return git(f, cwd, "rev-parse", "HEAD")


def terminals(f):
    return {
        (p["tab_id"], p["terminal_id"])
        for p in f.server.request("session.snapshot")["snapshot"]["panes"]
    }


def conflict_fixture(f):
    commit(f, f.repo, "base")
    pane = f.add()
    worktree = Path(f.run("path", "feature").stdout.strip())
    feature_head = commit(f, worktree, "feature")
    main_head = commit(f, f.repo, "main")
    return pane, worktree, feature_head, main_head


def merge_hooks(f):
    marker = f.server.root / "merge-hooks.json"
    allow = f.server.root / "allow-merge"
    script = f.server.root / "merge-hook.py"
    script.write_text(
        "import json, os\nfrom pathlib import Path\n"
        f"Path({str(marker)!r}).write_text(json.dumps({{\n"
        "  'cwd': os.getcwd(),\n"
        "  **{k: os.environ[k] for k in [\n"
        "    'WM_BRANCH_NAME', 'WM_TARGET_BRANCH', 'WM_WORKTREE_PATH',\n"
        "    'WM_PROJECT_ROOT', 'WM_HANDLE', 'WORKMUX_HANDLE']}\n"
        "}))\n"
        f"raise SystemExit(0 if Path({str(allow)!r}).exists() else 23)\n"
    )
    f.config({"panes": [{}], "pre_merge": [shlex.join([sys.executable, str(script)])]})
    git(f, f.repo, "add", ".workmux.yaml")
    git(f, f.repo, "commit", "-m", "configure merge hook")
    for name, bypass in (
        ("checked", None),
        ("skip-hooks", "--no-hooks"),
        ("skip-verify", "--no-verify"),
    ):
        pane = f.add(name)
        worktree = Path(f.run("path", name).stdout.strip())
        feature_head = commit(f, worktree, name)
        main_head = git(f, f.repo, "rev-parse", "main")
        before = terminals(f)
        if bypass is None:
            result = f.run("merge", name, ok=False)
            assert (
                result.returncode != 0 and "Pre-merge hook failed" in result.stderr
            ), result
            data = json.loads(marker.read_text())
            assert Path(data["cwd"]).resolve() == worktree.resolve(), data
            assert Path(data["WM_WORKTREE_PATH"]).resolve() == worktree.resolve(), data
            assert Path(data["WM_PROJECT_ROOT"]).resolve() == f.repo.resolve(), data
            assert (
                data["WM_BRANCH_NAME"]
                == data["WM_HANDLE"]
                == data["WORKMUX_HANDLE"]
                == name
            ), data
            assert data["WM_TARGET_BRANCH"] == "main", data
            assert git(f, f.repo, "rev-parse", "main") == main_head
            assert git(f, worktree, "rev-parse", "HEAD") == feature_head
            assert terminals(f) == before
            marker.unlink()
            allow.touch()
            f.run("merge", name)
            assert marker.exists(), "Successful retry must run the hook"
            marker.unlink()
            allow.unlink()
        else:
            f.run("merge", name, bypass)
            assert not marker.exists(), "Bypass must not run the failing hook"
        git(f, f.repo, "merge-base", "--is-ancestor", feature_head, "main")
        assert not worktree.exists()
        assert not git(f, f.repo, "branch", "--list", name)
        assert terminals(f) == before - {(pane["tab_id"], pane["terminal_id"])}
    print(
        "PASS merge-hooks: correct cwd and hook variables; failure preserves branch and terminals; successful retry, --no-hooks and --no-verify merge and clean up"
    )


def rebase_conflicts(f):
    _, worktree, feature_head, main_head = conflict_fixture(f)
    before = terminals(f)
    result = f.run("rebase", "feature", ok=False)
    assert result.returncode != 0, result
    assert "git rebase --continue" in result.stderr, result.stderr
    assert "git rebase --abort" in result.stderr, result.stderr
    assert git(f, worktree, "diff", "--name-only", "--diff-filter=U") == "change.txt"
    assert git(f, f.repo, "rev-parse", "main") == main_head
    assert not git(f, f.repo, "status", "--porcelain")
    assert terminals(f) == before
    git(f, worktree, "rebase", "--abort")
    assert git(f, worktree, "rev-parse", "HEAD") == feature_head
    assert not git(f, worktree, "status", "--porcelain")
    # Retry, resolve the conflict, and finish without losing the terminal.
    result = f.run("rebase", "feature", ok=False)
    assert result.returncode != 0, result
    (worktree / "change.txt").write_text("resolved\n")
    git(f, worktree, "add", "change.txt")
    git(f, worktree, "-c", "core.editor=true", "rebase", "--continue")
    f.run("rebase", "feature")
    assert git(f, worktree, "merge-base", "HEAD", "main") == main_head
    assert git(f, worktree, "branch", "--show-current") == "feature"
    assert (worktree / "change.txt").read_text() == "resolved\n"
    assert not git(f, worktree, "status", "--porcelain")
    assert git(f, f.repo, "rev-parse", "main") == main_head
    assert terminals(f) == before
    assert Path(f.run("path", "feature").stdout.strip()) == worktree
    print(
        "PASS rebase-conflicts: conflict remains available for resolution; abort restores HEAD; continue and retry succeed while main and live terminals remain unchanged"
    )


def merge_squash(f):
    pane = f.add()
    worktree = Path(f.run("path", "feature").stdout.strip())
    commit(f, worktree, "first change")
    commit(f, worktree, "second change")
    main_head = git(f, f.repo, "rev-parse", "HEAD")
    before = terminals(f)
    editor = f.server.root / "commit-editor"
    editor.write_text('#!/bin/sh\nprintf "squashed feature\\n" > "$1"\n')
    editor.chmod(0o700)
    f.run("merge", "feature", "--squash", env={**f.env, "GIT_EDITOR": str(editor)})
    assert git(f, f.repo, "rev-list", "--count", f"{main_head}..HEAD") == "1"
    assert git(f, f.repo, "rev-parse", "HEAD^") == main_head
    assert git(f, f.repo, "show", "-s", "--format=%s") == "squashed feature"
    assert (f.repo / "change.txt").read_text() == "second change\n"
    assert not git(f, f.repo, "status", "--porcelain")
    assert not worktree.exists()
    assert not git(f, f.repo, "branch", "--list", "feature")
    assert terminals(f) == before - {(pane["tab_id"], pane["terminal_id"])}
    print(
        "PASS merge-squash: two commits become one with the editor message and final content; only the feature branch, worktree and tab are removed"
    )


def merge_conflicts(f):
    pane, worktree, feature_head, main_head = conflict_fixture(f)
    before = terminals(f)
    for options in ((), ("--squash",)):
        result = f.run("merge", "feature", *options, ok=False)
        assert result.returncode != 0, result
        assert "Target worktree kept clean" in result.stderr, result.stderr
        assert git(f, f.repo, "rev-parse", "HEAD") == main_head
        assert git(f, worktree, "rev-parse", "HEAD") == feature_head
        assert not git(f, f.repo, "status", "--porcelain")
        assert not git(f, worktree, "status", "--porcelain")
        assert terminals(f) == before
        assert Path(f.run("path", "feature").stdout.strip()) == worktree
    # Resolve in the feature worktree, then retry the public command.
    git(f, worktree, "merge", "-s", "ours", "main", "-m", "resolve conflict")
    f.run("merge", "feature")
    assert (f.repo / "change.txt").read_text() == "feature\n"
    assert not worktree.exists()
    assert not git(f, f.repo, "branch", "--list", "feature")
    assert terminals(f) == before - {(pane["tab_id"], pane["terminal_id"])}
    print(
        "PASS merge-conflicts: default and squash refusal preserve both branches, clean worktrees and live terminals; resolved retry cleans up only the feature"
    )
