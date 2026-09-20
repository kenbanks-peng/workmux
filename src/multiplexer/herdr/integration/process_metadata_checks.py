"""Process metadata checks on a disposable backend instance, never the main server."""

import json
import os
import subprocess
from pathlib import Path

from run import env_for
from server import HerdrServer

ROOT = Path(__file__).resolve().parents[4]
TEST = (
    "multiplexer::herdr::platform_tests::process_metadata_tests::"
    "isolated_process_exit_and_replacement"
)


def main():
    result = subprocess.run(
        ["cargo", "test", "--no-run", "--message-format=json"],
        cwd=ROOT,
        env={**os.environ, "CARGO_BUILD_JOBS": "1"},
        check=True,
        capture_output=True,
        text=True,
    )
    binaries = [
        Path(item["executable"])
        for line in result.stdout.splitlines()
        if (item := json.loads(line)).get("executable")
        and item.get("profile", {}).get("test")
    ]
    assert len(binaries) == 1, binaries
    server = HerdrServer()
    try:
        snapshot = server.start()["snapshot"]
        print(f"Herdr {snapshot['version']}, protocol {snapshot['protocol']}", flush=True)
        result = subprocess.run(
            [str(binaries[0]), "--exact", TEST, "--ignored", "--nocapture"],
            cwd=server.root,
            env={
                **env_for(server),
                "WORKMUX_HERDR_PROCESS_METADATA_SOCKET": str(server.socket_path),
            },
            capture_output=True,
            text=True,
            check=False,
            timeout=45,
        )
        print(result.stdout, end="")
        print(result.stderr, end="")
        result.check_returncode()
        assert "1 passed; 0 failed; 0 ignored" in result.stdout, result.stdout
    finally:
        server.close()


if __name__ == "__main__":
    main()
