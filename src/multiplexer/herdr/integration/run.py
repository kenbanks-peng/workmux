"""Run the adapter against private servers. Never connect to a user's session."""

import json
import os
import shlex
import subprocess
from contextlib import ExitStack
from pathlib import Path

from cleanup_proxy import CleanupProxy
from server import HerdrServer, wait_until

ROOT = Path(__file__).resolve().parents[4]


def build():
    subprocess.run(["cargo", "build", "--quiet"], cwd=ROOT, check=True)
    result = subprocess.run(
        ["cargo", "test", "--no-run", "--message-format=json"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get("executable") and item.get("profile", {}).get("test"):
            return Path(item["executable"])
    raise RuntimeError("Rust test binary not found")


def own(stack):
    server = HerdrServer()
    stack.callback(server.close)
    server.start()
    return server


def env_for(server):
    return {
        **server.env,
        "PATH": os.environ["PATH"],
        "HERDR_SOCKET_PATH": str(server.socket_path),
        "WORKMUX_BACKEND": "herdr",
        "WORKMUX_SKIP_UPDATE_CHECK": "1",
        "WORKMUX_HERDR_TEST_EXECUTABLE": str(ROOT / "target/debug/workmux"),
    }


def probe(binary, server, name, variables):
    module = "tests"
    if name == "isolated_deferred_cleanup":
        module = "deferred_tests"
    elif name in {"isolated_cleanup_race", "isolated_launch_ownership"}:
        module = "cleanup_tests"
    result = subprocess.run(
        [
            str(binary),
            "--exact",
            f"multiplexer::herdr::{module}::{name}",
            "--ignored",
            "--nocapture",
        ],
        env={**env_for(server), **variables},
        cwd=server.root,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    assert "1 passed" in result.stdout, result.stdout
    if name == "isolated_deferred_cleanup":
        wait_until(
            lambda: all(
                tab["label"] != "owned"
                for tab in server.request("session.snapshot")["snapshot"]["tabs"]
            )
        )
    print(f"PASS {name}", flush=True)


def caller_probe(binary, server):
    created = server.request("workspace.create", label="caller", focus=True)
    pane = created["root_pane"]
    server.request("tab.rename", tab_id=pane["tab_id"], label="root")
    output, exit_file = server.root / "caller-output", server.root / "caller-exit"
    command = shlex.join(
        [
            "env",
            "WORKMUX_BACKEND=herdr",
            "WORKMUX_HERDR_CALLER_PROBE=1",
            f"HERDR_SOCKET_PATH={server.socket_path}",
            str(binary),
            "--exact",
            "multiplexer::herdr::tests::isolated_current_context",
            "--ignored",
            "--nocapture",
        ]
    )
    command += f" > {shlex.quote(str(output))} 2>&1; printf '%s' $? > {shlex.quote(str(exit_file))}"
    server.request(
        "pane.send_input", pane_id=pane["pane_id"], text=command, keys=["enter"]
    )
    wait_until(lambda: exit_file.exists() and exit_file.read_text(), timeout=45)
    assert exit_file.read_text() == "0", output.read_text()
    assert "HERDR_CURRENT_CONTEXT_PASSED" in output.read_text(), output.read_text()
    print("PASS native caller resolution and window placement", flush=True)


def restart_probe(binary, server):
    output = server.root / "restart-output"
    with (
        output.open("w") as log,
        subprocess.Popen(
            [
                str(binary),
                "--exact",
                "multiplexer::herdr::tests::isolated_server_replacement",
                "--ignored",
                "--nocapture",
            ],
            env={
                **env_for(server),
                "WORKMUX_HERDR_RESTART_SOCKET": str(server.socket_path),
            },
            cwd=server.root,
            stdout=log,
            stderr=subprocess.STDOUT,
        ) as process,
    ):
        try:
            wait_until(
                lambda: (
                    (server.root / "restart-ready").exists()
                    or process.poll() is not None
                )
            )
            assert process.poll() is None, output.read_text()
            server.stop()
            server.start()
            (server.root / "restart-continue").touch()
            assert process.wait(timeout=45) == 0, output.read_text()
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            if server.artifact_dir:
                server.artifact_dir.mkdir(parents=True, exist_ok=True)
                (server.artifact_dir / "restart-output.log").write_text(
                    output.read_text()
                )
    assert "HERDR_REPLACEMENT_REFUSAL_PASSED" in output.read_text(), output.read_text()
    print(
        "PASS restart: missing/stale parents, fresh ownership, and old tab/workspace cleanup refusal",
        flush=True,
    )


def cli_smoke(server):
    binary = ROOT / "target/debug/workmux"
    repo = server.root / "repo"
    repo.mkdir()
    env = env_for(server)
    for args in (
        ["init", "-b", "main"],
        ["config", "user.name", "Test"],
        ["config", "user.email", "test@example.invalid"],
    ):
        subprocess.run(
            ["git", *args], cwd=repo, env=env, check=True, capture_output=True
        )
    # Slow shell initialization must complete before the command runs.
    (Path(env["HOME"]) / ".profile").write_text("sleep 0.1\nexport LAUNCH_ENV=ready\n")
    marker = server.root / "launched"
    (repo / ".workmux.yaml").write_text(
        json.dumps(
            {
                "panes": [{"command": f"printf '%s' \"$LAUNCH_ENV\" > {marker}"}],
            }
        )
    )
    subprocess.run(
        ["git", "add", "."], cwd=repo, env=env, check=True, capture_output=True
    )
    subprocess.run(
        ["git", "commit", "-m", "fixture"],
        cwd=repo,
        env=env,
        check=True,
        capture_output=True,
    )
    server.request("workspace.create", label="parent", cwd=str(repo), focus=True)

    def run(*args, success=True):
        result = subprocess.run(
            [str(binary), *args],
            cwd=repo,
            env=env,
            capture_output=True,
            text=True,
            timeout=45,
            check=False,
        )
        if success and result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
        return result

    run("add", "feature", "--parent-session", "parent", "--background")
    wait_until(lambda: marker.exists() and marker.read_text() == "ready")
    run("list")
    run("close", "feature")
    run("open", "feature", "--parent-session", "parent")
    run("remove", "feature", "--force")
    result = run("add", "session-rejected", "--session", success=False)
    assert result.returncode != 0 and "only supported with tmux" in result.stderr, (
        result
    )
    print(
        "PASS CLI add/list/close/open/remove and unchanged session restriction",
        flush=True,
    )


def main():
    binary = build()
    with ExitStack() as stack:
        server, other = own(stack), own(stack)
        probe(
            binary,
            server,
            "isolated_core_operations",
            {
                "WORKMUX_HERDR_CORE_SOCKET": str(server.socket_path),
                "WORKMUX_HERDR_OTHER_SOCKET": str(other.socket_path),
            },
        )
    with ExitStack() as stack:
        server = own(stack)
        for override, signals, expected in [
            ("herdr", {"TMUX": "/not-a-server,1,1"}, "herdr"),
            ("tmux", {}, "tmux"),
            ("", {"TMUX": "/not-a-server,1,1"}, "tmux"),
            ("", {"WEZTERM_PANE": "1"}, "wezterm"),
            ("", {"ZELLIJ_PANE_ID": "1"}, "zellij"),
            ("", {"KITTY_WINDOW_ID": "1"}, "kitty"),
            ("", {}, "herdr"),
        ]:
            probe(
                binary,
                server,
                "isolated_selection",
                {
                    "WORKMUX_BACKEND": override,
                    "WORKMUX_HERDR_EXPECT_BACKEND": expected,
                    **signals,
                },
            )
    for name, variable in [
        ("isolated_split_size_limits", "WORKMUX_HERDR_SIZE_SOCKET"),
        ("isolated_persisted_identity_contract", "WORKMUX_HERDR_IDENTITY_SOCKET"),
        ("isolated_deferred_cleanup", "WORKMUX_HERDR_DEFERRED_SOCKET"),
    ]:
        with ExitStack() as stack:
            server = own(stack)
            server.request("workspace.create", label="client-view", focus=True)
            client = server.attach()
            wait_until(lambda client=client: client.output)
            probe(binary, server, name, {variable: str(server.socket_path)})
    with ExitStack() as stack:
        probe(binary, own(stack), "isolated_launch_ownership", {})
    for mode in [
        "immediate-tab",
        "immediate-workspace",
        "deferred-tab",
        "deferred-workspace",
    ]:
        for insertion in (
            ["same-tab", "new-tab"] if mode.endswith("workspace") else ["same-tab"]
        ):
            with ExitStack() as stack:
                server = own(stack)
                with CleanupProxy(server, insertion) as proxy:
                    probe(
                        binary,
                        proxy,
                        "isolated_cleanup_race",
                        {"WORKMUX_HERDR_CLEANUP_MODE": mode},
                    )
                    proxy.verify()
                    print(
                        f"PASS {mode} cleanup includes {insertion} insertion",
                        flush=True,
                    )
    with ExitStack() as stack:
        restart_probe(binary, own(stack))
    with ExitStack() as stack:
        caller_probe(binary, own(stack))
    with ExitStack() as stack:
        cli_smoke(own(stack))


if __name__ == "__main__":
    main()
