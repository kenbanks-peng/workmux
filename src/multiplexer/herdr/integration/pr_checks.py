"""PR/MR checkout on private Herdr servers, with local Git and fake forge CLIs.

Build workmux first, then run: python3 src/multiplexer/herdr/integration/pr_checks.py
No network service or user multiplexer session is used.
"""

import json
import subprocess
import sys
from pathlib import Path

from server import HerdrServer, wait_until
from support_checks import Fixture


def checkout(f, forge, session_mode, url_reference):
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

    remote = f.server.root / "remote.git"
    git("init", "--bare", str(remote))
    repository = f"https://{forge}.com/testowner/testrepo"
    git("remote", "add", "origin", repository + ".git")
    git("config", f"url.{remote}.insteadOf", repository + ".git")
    git("push", "-u", "origin", "main")
    git("checkout", "-b", "review-source")
    (f.repo / "review.txt").write_text("Review change\n")
    git("add", "review.txt")
    git("commit", "-m", "Review change")
    expected = git("rev-parse", "HEAD")
    ref = (
        "refs/heads/review-source"
        if forge == "github"
        else "refs/merge-requests/123/head"
    )
    git("push", "origin", f"HEAD:{ref}")
    git("checkout", "main")
    git("branch", "-D", "review-source")

    fake_bin = f.server.root / "fake-bin"
    fake_bin.mkdir()
    log = f.server.root / "forge-calls.jsonl"
    if forge == "github":
        executable = "gh"
        arguments = [
            "pr",
            "view",
            "123",
            "--json",
            "headRefName,baseRefName,headRepositoryOwner,state,isDraft,title,author",
        ]
        response = {
            "headRefName": "review-source",
            "baseRefName": "main",
            "headRepositoryOwner": {"login": "testowner"},
            "state": "OPEN",
            "isDraft": False,
            "title": "Review change",
            "author": {"login": "tester"},
        }
        reference = repository + "/pull/123"
    else:
        executable = "glab"
        arguments = ["mr", "view", "123", "--output", "json"]
        response = {
            "iid": 123,
            "web_url": repository + "/-/merge_requests/123",
            "source_branch": "review-source",
            "target_branch": "main",
            "source_project_id": 100,
            "target_project_id": 100,
            "state": "opened",
            "draft": False,
            "title": "Review change",
            "author": {"username": "tester"},
        }
        reference = response["web_url"]
    script = fake_bin / executable
    script.write_text(
        f"#!{sys.executable}\nimport json, sys\n"
        f"with open({str(log)!r}, 'a') as log: log.write(json.dumps(sys.argv[1:]) + '\\n')\n"
        f"assert sys.argv[1:] == {arguments!r}, sys.argv\n"
        f"print({json.dumps(response)!r})\n"
    )
    script.chmod(0o700)
    f.env["PATH"] = str(fake_bin) + ":/usr/bin:/bin"
    options = ["--session"] if session_mode else ["--parent-session", "parent"]
    f.run(
        "add", "--pr", reference if url_reference else "123", *options, "--background"
    )
    worktree = Path(f.run("path", "review-source").stdout.strip())
    assert git("rev-parse", "HEAD", cwd=worktree) == expected
    assert (worktree / "review.txt").read_text() == "Review change\n"
    assert git("config", "--get", "branch.review-source.workmux-base") == "origin/main"
    assert (
        git("diff", "--name-only", "origin/main...HEAD", cwd=worktree) == "review.txt"
    )

    snapshot = f.server.request("session.snapshot")["snapshot"]
    if session_mode:
        workspace = next(
            w for w in snapshot["workspaces"] if w["label"] == "wm-review-source"
        )
        tab = next(
            t
            for t in snapshot["tabs"]
            if t["workspace_id"] == workspace["workspace_id"]
        )
    else:
        tab = next(t for t in snapshot["tabs"] if t["label"] == "wm-review-source")
        assert tab["workspace_id"] == f.parent["workspace_id"]
    pane = next(p for p in snapshot["panes"] if p["tab_id"] == tab["tab_id"])
    code, output = f.native(pane, "{ pwd; git rev-parse HEAD; }")
    assert code == 0 and str(worktree) in output and expected in output, output

    # A repeated checkout must not allocate a second window or workspace.
    before = {p["pane_id"] for p in snapshot["panes"]}
    result = f.run("add", "--pr", reference, *options, "--background", ok=False)
    assert result.returncode != 0, result.stdout + result.stderr
    after = f.server.request("session.snapshot")["snapshot"]
    assert {p["pane_id"] for p in after["panes"]} == before
    assert [json.loads(line) for line in log.read_text().splitlines()] == [
        arguments,
        arguments,
    ]
    f.run("remove", "review-source", "--force")
    wait_until(lambda: not worktree.exists())
    assert git("branch", "--list", "review-source") == ""
    print(
        f"PASS {forge} checkout: {'session' if session_mode else 'window'}, "
        f"{'URL' if url_reference else 'number'}, commit/base/pane/duplicate/cleanup",
        flush=True,
    )


def main():
    for forge in ("github", "gitlab"):
        for session_mode in (False, True):
            for url_reference in (False, True):
                server = HerdrServer()
                try:
                    server.start()
                    checkout(Fixture(server), forge, session_mode, url_reference)
                finally:
                    server.close()


if __name__ == "__main__":
    main()
