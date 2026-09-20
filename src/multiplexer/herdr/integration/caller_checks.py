"""Live caller regression tests. All targets and both UI clients are disposable.

Run: CARGO_BUILD_JOBS=1 python3 src/multiplexer/herdr/integration/caller_checks.py
An optional first argument selects an already-built Rust test executable.
"""

import json
import os
import shlex
import subprocess
import sys
from pathlib import Path

from run import build
from server import HerdrServer, wait_until

TEST = "multiplexer::herdr::caller_tests::isolated_caller_discovery"


def run(binary):
    server = HerdrServer()
    try:
        server.start()
        caller = server.request("workspace.create", label="caller", focus=True)[
            "root_pane"
        ]
        server.request("tab.rename", tab_id=caller["tab_id"], label="origin")
        other = server.request(
            "pane.split", pane_id=caller["pane_id"], direction="right", focus=False
        )["pane"]
        # Use a real removed address, not only an invented invalid ID.
        stale = server.request("workspace.create", label="stale", focus=False)[
            "root_pane"
        ]
        server.request("workspace.close", workspace_id=stale["workspace_id"])
        first = server.attach()
        wait_until(lambda: b"caller" in first.output)
        second = server.attach()
        wait_until(lambda: b"caller" in second.output)
        command = [str(binary), "--exact", TEST, "--ignored", "--nocapture"]
        hints = {
            "missing": {},
            "stale": {
                "HERDR_PANE_ID": stale["pane_id"],
                "HERDR_ACTIVE_PANE_ID": stale["pane_id"],
            },
            "conflicting": {
                "HERDR_PANE_ID": other["pane_id"],
                "HERDR_ACTIVE_PANE_ID": other["pane_id"],
                "WORKMUX_PANE_ID": "not-a-valid-workmux-target",
            },
        }
        for case, values in hints.items():
            base = {
                "HERDR_SOCKET_PATH": str(server.socket_path),
                "WORKMUX_CALLER_SOCKET": str(server.socket_path),
                **values,
            }
            # An external process must not acquire identity from hints or focus.
            result = subprocess.run(
                command,
                env={**server.env, **base, "WORKMUX_CALLER_EXPECTED": "null"},
                capture_output=True,
                check=False,
                text=True,
                timeout=45,
            )
            assert result.returncode == 0, result.stdout + result.stderr
            assert "CALLER_DISCOVERY_PASSED" in result.stdout, result.stdout
            print(f"PASS external/{case}", flush=True)

            output = server.root / f"{case}.output"
            exit_file = server.root / f"{case}.exit"
            sync = server.root / f"{case}-sync"
            env = {
                **base,
                "WORKMUX_CALLER_EXPECTED": json.dumps(caller),
                "WORKMUX_CALLER_SYNC": str(sync),
            }
            # env -i removes inherited native IDs and user shell settings.
            shell = shlex.join(
                ["/usr/bin/env", "-i", "PATH=/usr/bin:/bin"]
                + [f"{k}={v}" for k, v in env.items()]
                + command
            )
            shell += f" > {shlex.quote(str(output))} 2>&1; printf '%s' $? > {shlex.quote(str(exit_file))}"
            server.request("pane.focus", pane_id=caller["pane_id"])
            server.request(
                "pane.send_input", pane_id=caller["pane_id"], text=shell, keys=["enter"]
            )
            wait_until(
                lambda sync=sync, exit_file=exit_file: (
                    sync.with_suffix(".ready").exists() or exit_file.exists()
                ),
                timeout=45,
            )
            assert not exit_file.exists(), output.read_text()
            # End on the non-caller pane as well as testing transitions.
            moves = [(b"\x02l", other), (b"\x02h", caller)] * 3 + [(b"\x02l", other)]
            for phase, (keys, target) in enumerate(moves):
                second.send(keys)
                wait_until(
                    lambda target=target: (
                        server.request("session.snapshot")["snapshot"][
                            "focused_pane_id"
                        ]
                        == target["pane_id"]
                    )
                )
                sync.with_suffix(f".phase-{phase}").touch()
                wait_until(
                    lambda sync=sync, phase=phase, exit_file=exit_file: (
                        sync.with_suffix(f".ack-{phase}").exists() or exit_file.exists()
                    )
                )
                assert not exit_file.exists(), output.read_text()
            sync.with_suffix(".done").touch()
            wait_until(
                lambda exit_file=exit_file: (
                    exit_file.exists() and exit_file.read_text()
                ),
                timeout=45,
            )
            assert exit_file.read_text() == "0", output.read_text()
            assert "CALLER_DISCOVERY_PASSED" in output.read_text(), output.read_text()
            count = next(
                line
                for line in output.read_text().splitlines()
                if "CALLER_FOCUS_CHECKS=" in line
            )
            print(
                f"PASS pane/{case}: 7 second-client focus changes; {count}", flush=True
            )
    finally:
        server.close()


if __name__ == "__main__":
    os.environ["CARGO_BUILD_JOBS"] = "1"
    run(Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else build())
