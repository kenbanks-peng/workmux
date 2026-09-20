"""Check adapter keys at a real terminal receiver on a disposable Herdr server.

Run: CARGO_BUILD_JOBS=1 python3 src/multiplexer/herdr/integration/input_keys_checks.py
No inherited Herdr socket, HOME, or workspace is used.
"""

import json
import os
import shlex
import subprocess
import sys
from pathlib import Path

from run import ROOT, env_for
from server import HerdrServer, wait_until

TEST = "multiplexer::herdr::input_keys_tests::isolated_input_key_receipt"


def build_test():
    result = subprocess.run(
        ["cargo", "test", "--no-run", "--message-format=json"],
        cwd=ROOT,
        env={**os.environ, "CARGO_BUILD_JOBS": "1"},
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get("executable") and item.get("profile", {}).get("test"):
            return Path(item["executable"])
    raise RuntimeError("Rust test binary not found")


def main():
    binary = build_test()
    server = HerdrServer()
    try:
        snapshot = server.start()["snapshot"]
        print(f"Live Herdr {snapshot['version']}, protocol {snapshot['protocol']}", flush=True)
        pane = server.request(
            "workspace.create", label="input-keys-receiver", focus=True
        )["root_pane"]
        ready = server.root / "receiver-ready"
        received = server.root / "received"
        receiver = server.root / "receiver.py"
        receiver.write_text(
            "import os, tty\n"
            "tty.setraw(0)\n"
            # Select normal cursor keys, so Up must be ESC [ A, not ESC O A.
            "os.write(1, b'\\x1b[?1l')\n"
            f"open({str(received)!r}, 'wb').close()\n"
            f"open({str(ready)!r}, 'w').write('ready')\n"
            "while True:\n"
            "    data = os.read(0, 4096)\n"
            "    if not data: break\n"
            f"    with open({str(received)!r}, 'ab') as output: output.write(data)\n"
        )
        server.request(
            "pane.send_input",
            pane_id=pane["pane_id"],
            text=shlex.join([sys.executable, str(receiver)]),
            keys=["enter"],
        )
        wait_until(ready.exists)
        # Send to a background target. Key delivery must not steal focus.
        server.request("workspace.create", label="input-keys-guard", focus=True)
        result = subprocess.run(
            [str(binary), "--exact", TEST, "--ignored", "--nocapture"],
            cwd=server.root,
            env={
                **env_for(server),
                "WORKMUX_HERDR_INPUT_KEYS_SOCKET": str(server.socket_path),
                "WORKMUX_HERDR_INPUT_KEYS_TERMINAL": pane["terminal_id"],
                "WORKMUX_HERDR_INPUT_KEYS_RECEIVED": str(received),
            },
            capture_output=True,
            text=True,
            timeout=90,
            check=False,
        )
        print(result.stdout, end="", flush=True)
        assert result.returncode == 0, result.stdout + result.stderr
        assert "1 passed" in result.stdout, result.stdout
        assert "HERDR_INPUT_KEYS_RECEIPT_PASSED" in result.stdout, result.stdout
        # Independent full-stream oracle catches missing/reordered/extra bytes.
        assert received.read_bytes() == b" \x7f\x03\x04\x1a\r\x1b\t\x1b[A"
    finally:
        server.close()


if __name__ == "__main__":
    main()
