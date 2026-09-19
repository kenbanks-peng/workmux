"""Terminal capture acceptance on a private live Herdr server.

Run with --test-binary PATH from cargo test --no-run. No inherited endpoint is
used. All panes, shell processes, HOME/XDG paths and the server are disposable.
"""

import argparse
import json
import shlex
import subprocess
from pathlib import Path

from server import HerdrServer, wait_until


def capture_case(binary, name, payload, expected_cases):
    server = HerdrServer()
    try:
        server.start()
        pane = server.request("workspace.create", label=name, focus=True)["root_pane"]
        # Resize only this fixture's private PTY. Wait past the server's
        # initial 120-column layout so output cannot race UI attachment.
        server.attach().resize(40, 180)
        wait_until(
            lambda: any(
                item["pane_id"] == pane["pane_id"] and item["rect"]["width"] >= 140
                for item in server.request("pane.layout", pane_id=pane["pane_id"])[
                    "layout"
                ]["panes"]
            )
        )
        source = server.root / "output.bin"
        ready = server.root / "ready"
        source.write_bytes(payload)
        # No prompt or command echo after the payload. Keep the terminal alive.
        command = (
            f"stty -echo; cat {shlex.quote(str(source))}; "
            f"touch {shlex.quote(str(ready))}; sleep 120"
        )
        server.request(
            "pane.send_input", pane_id=pane["pane_id"], text=command, keys=["enter"]
        )
        wait_until(ready.exists)
        # The shell marker can precede PTY processing. Wait for the final output
        # marker through the real server before the adapter reads any history.
        wait_until(
            lambda: (
                "CAPTURE-END"
                in server.request(
                    "pane.read", pane_id=pane["pane_id"], source="recent", lines=1000
                )["read"]["text"]
            )
        )
        manifest = server.root / "cases.json"
        manifest.write_text(
            json.dumps(
                [
                    {
                        "name": f"{name}-{lines}",
                        "terminal_id": pane["terminal_id"],
                        "lines": lines,
                        "expected": expected,
                    }
                    for lines, expected in expected_cases
                ]
            )
        )
        result = subprocess.run(
            [
                str(binary),
                "--exact",
                "multiplexer::herdr::capture_tests::isolated_capture_output",
                "--ignored",
                "--nocapture",
            ],
            env={
                **server.env,
                "WORKMUX_CAPTURE_SOCKET": str(server.socket_path),
                "WORKMUX_CAPTURE_MANIFEST": str(manifest),
            },
            cwd=server.root,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )
        assert result.returncode == 0, result.stdout + result.stderr
        assert "1 passed" in result.stdout, result.stdout
        print(f"PASS live {name}: {len(expected_cases)} exact captures", flush=True)
    finally:
        server.close()


def history(binary):
    rows = [f"history-{i:04d}" for i in range(2200)] + ["CAPTURE-END"]
    capture_case(
        binary,
        "history-above-1000",
        ("\n".join(rows) + "\n").encode(),
        [(n, "\n".join(rows[-n:])) for n in (1000, 1001, 1024, 2000)],
    )


def large_output(binary):
    # More than 1 MiB of shell output, with unique row numbers. Verify both
    # recent-read truncation and multi-batch history truncation byte for byte.
    rows = [f"large-{i:05d}-" + "x" * 108 for i in range(9000)] + ["CAPTURE-END"]
    payload = ("\n".join(rows) + "\n").encode()
    assert len(payload) > 1024 * 1024
    capture_case(
        binary,
        "large-output-truncation",
        payload,
        [(n, "\n".join(rows[-n:])) for n in (1, 10, 1001, 4096)],
    )


def sequences(binary):
    # SGR, CR + erase-line, backspace, cursor-left overwrite, OSC title,
    # OSC-8 hyperlink, tab expansion, and UTF-8 all pass through the real PTY.
    payload = (
        "\x1b[31mred\x1b[0m\n"
        "discard this\r\x1b[2Kkept\n"
        "abc\bZ\n"
        "abcdef\x1b[3DXYZ\n"
        "\x1b]0;capture-owned-title\x07title-safe\n"
        "\x1b]8;;https://example.invalid\x1b\\link\x1b]8;;\x1b\\\n"
        "a\tb\n"
        "界🙂 café\n"
    )
    expected_rows = [
        "red",
        "kept",
        "abZ",
        "abcXYZ",
        "title-safe",
        "link",
        "a       b",
        "界🙂 café",
    ] * 200 + ["CAPTURE-END"]
    capture_case(
        binary,
        "terminal-sequences",
        (payload * 200 + "CAPTURE-END\n").encode(),
        [(n, "\n".join(expected_rows[-n:])) for n in (9, 1001)],
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", required=True, type=Path)
    parser.add_argument(
        "--case", choices=("history", "large", "sequences", "all"), default="all"
    )
    args = parser.parse_args()
    binary = args.test_binary.resolve(strict=True)
    for name, test in [
        ("history", history),
        ("large", large_output),
        ("sequences", sequences),
    ]:
        if args.case in (name, "all"):
            test(binary)


if __name__ == "__main__":
    main()
