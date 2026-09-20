"""Focused live adapter checks. Only test-owned servers and PTYs are changed."""

import argparse
import os
import subprocess
from pathlib import Path

from server import HerdrServer, wait_until


def run(binary):
    server = HerdrServer()
    process = None
    try:
        server.start()
        output = server.root / "dimensions.log"
        with output.open("w") as log:
            process = subprocess.Popen(
                [
                    str(binary.resolve()),
                    "--exact",
                    "multiplexer::herdr::dimensions_tests::isolated_dimensions_nested_and_resize",
                    "--ignored",
                    "--nocapture",
                ],
                cwd=server.root,
                env={
                    **server.env,
                    "PATH": os.environ["PATH"],
                    "WORKMUX_HERDR_DIMENSIONS_SOCKET": str(server.socket_path),
                },
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            client = None
            for stage, rows, columns in [
                ("attach", 40, 140),
                ("shrink", 24, 40),
                ("boundary", 8, 8),
                ("minimum", 4, 4),
            ]:
                wait_until(
                    lambda stage=stage: (
                        (server.root / f"{stage}-ready").exists()
                        or process.poll() is not None
                    ),
                    timeout=30,
                )
                assert process.poll() is None, output.read_text()
                if client is None:
                    client = server.attach()
                client.resize(rows, columns)

                def resized(stage=stage, columns=columns):
                    snapshot = server.request("session.snapshot")["snapshot"]
                    pane = snapshot["panes"][0]
                    layout = server.request("pane.layout", pane_id=pane["pane_id"])[
                        "layout"
                    ]
                    # The sidebar can reduce available width on large screens.
                    widths = [p["rect"]["width"] for p in layout["panes"]]
                    if stage == "boundary":
                        return sorted(widths) == [2, 2, 4, 4]
                    return (
                        max(widths) <= 2
                        if stage == "minimum"
                        else (
                            80 < max(widths) <= columns
                            if stage == "attach"
                            else 0 < max(widths) <= columns
                        )
                    )

                try:
                    wait_until(resized)
                except TimeoutError as error:
                    raise RuntimeError(
                        f"{stage}: {server.request('session.snapshot')}\n{output.read_text()}"
                    ) from error
                (server.root / f"{stage}-continue").touch()
            assert process.wait(timeout=30) == 0, output.read_text()
        text = output.read_text()
        assert "1 passed" in text and "HERDR_DIMENSIONS_PASSED" in text, text
        print(text, end="")
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        server.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("test_binary", type=Path)
    run(parser.parse_args().test_binary)
