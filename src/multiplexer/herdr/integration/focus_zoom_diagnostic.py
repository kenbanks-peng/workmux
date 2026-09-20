"""Explicit live focus diagnostic; excluded from unittest test_*.py discovery.

Each invocation owns a disposable server. No focus request or input is retried.
Exit 1 means a wrong target or timeout, not a supported two-client result.
"""

import argparse
import json
import shlex
import time
from pathlib import Path

import test_focus_zoom
from server import wait_until


def diagnose(clients, settle_seconds, trace, inspect_snapshots=False):
    fixture = test_focus_zoom.FocusZoomTests()
    evidence = {
        "clients": clients,
        "settle_seconds": settle_seconds,
        "inspect_snapshots": inspect_snapshots,
        "requests": [],
    }
    started = time.monotonic()

    def state():
        snapshot = fixture.server.request("session.snapshot")["snapshot"]
        return {
            "seconds": round(time.monotonic() - started, 4),
            "focus": {
                key: snapshot[key]
                for key in ("focused_workspace_id", "focused_tab_id", "focused_pane_id")
            },
            "client_output_bytes": [len(c.output) for c in fixture.server.clients],
        }

    try:
        fixture.setUp()
        attached = [fixture.attach() for _ in range(clients)]
        # Establish the initial right target by keyboard, as in the original
        # failing test. Use the same UI for the one-client control.
        attached[-1].send(b"\x02l")
        wait_until(
            lambda: fixture.layout()["focused_pane_id"] == fixture.right["pane_id"]
        )
        evidence["initial_pane"] = fixture.right["pane_id"]
        for index, (key, target, expected) in enumerate(
            (
                (b"h", fixture.left, "left"),
                (b"l", fixture.right, "right"),
                (b"h", fixture.left, "left"),
                (b"l", fixture.right, "right"),
            )
        ):
            client_index = index % clients
            client = attached[client_index]
            entry = {
                "index": index,
                "client": client_index,
                "expected": expected,
                "target": target["pane_id"],
                "sent_at": round(time.monotonic() - started, 4),
            }
            evidence["requests"].append(entry)
            client.send(b"\x02" + key)

            def selected(target=target, entry=entry):
                layout = fixture.layout()
                entry["last_observed_pane"] = layout["focused_pane_id"]
                return layout["focused_pane_id"] == target["pane_id"]

            wait_until(selected)
            entry["observed_at"] = round(time.monotonic() - started, 4)
            # An explicit experiment variable, not a retry or a passing-test fix.
            if settle_seconds:
                time.sleep(settle_seconds)
            if inspect_snapshots:
                entry["before_input"] = state()
            output = fixture.server.root / f"focus-diagnostic-{index}"
            client.send(
                f"printf '%s' \"$FOCUS_TARGET\" > {shlex.quote(str(output))}\r".encode()
            )
            wait_until(lambda output=output: output.exists() and output.read_text())
            entry["received"] = output.read_text()
            entry["after_input"] = state()
            assert entry["received"] == expected, (
                f"request {index}: expected {expected}, received {entry['received']}"
            )
            assert all(c.process.poll() is None for c in attached)
        evidence["result"] = "passed"
        return 0
    except (AssertionError, OSError, RuntimeError, ValueError) as error:
        evidence["result"] = "failed"
        evidence["error"] = f"{type(error).__name__}: {error}"
        if getattr(fixture, "server", None) and fixture.server.process:
            evidence["failure_state"] = state()
        return 1
    finally:
        fixture.doCleanups()
        trace.parent.mkdir(parents=True, exist_ok=True)
        trace.write_text(json.dumps(evidence, indent=2) + "\n")
        print(f"{evidence.get('result', 'interrupted')}: {trace}")
        if "error" in evidence:
            print(evidence["error"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--clients", type=int, choices=(1, 2), default=2)
    parser.add_argument("--settle-seconds", type=float, choices=(0, 0.25), default=0)
    parser.add_argument("--trace", type=Path, required=True)
    parser.add_argument(
        "--inspect-snapshots",
        action="store_true",
        help="Add a snapshot round trip before input; changes timing",
    )
    args = parser.parse_args()
    return diagnose(
        args.clients, args.settle_seconds, args.trace, args.inspect_snapshots
    )


if __name__ == "__main__":
    raise SystemExit(main())
