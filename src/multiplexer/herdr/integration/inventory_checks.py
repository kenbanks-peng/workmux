"""Live inventory primitives on disposable servers; no inherited socket is used.

Run: python3 src/multiplexer/herdr/integration/inventory_checks.py
Use --adapter-test-binary PATH to also run the workmux adapter test.
The other three tests check native Herdr API behavior only.
"""

import argparse
import concurrent.futures
import subprocess
import threading
import unittest
from pathlib import Path

from server import HerdrServer

ADAPTER_TEST_BINARY = None


class InventoryChecks(unittest.TestCase):
    def setUp(self):
        self.server = HerdrServer()
        self.addCleanup(self.server.close)
        self.server.start()
        self.parent = self.server.request(
            "workspace.create", label="parent", cwd=str(self.server.root), focus=False
        )["root_pane"]

    def test_live_adapter_same_name_creation(self):
        if ADAPTER_TEST_BINARY is None:
            self.skipTest("use --adapter-test-binary for adapter evidence")
        env = self.server.env | {
            "WORKMUX_HERDR_INVENTORY_SOCKET": str(self.server.socket_path),
        }
        result = subprocess.run(
            [
                str(ADAPTER_TEST_BINARY),
                "multiplexer::herdr::inventory_tests::isolated_simultaneous_same_name_creation",
                "--exact",
                "--ignored",
                "--nocapture",
            ],
            cwd=self.server.root,
            env=env,
            capture_output=True,
            text=True,
            timeout=90,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("HERDR_INVENTORY_CONCURRENT_PASSED", result.stdout)
        print(result.stdout, end="")

    def snapshot(self):
        return self.server.request("session.snapshot")["snapshot"]

    def create_tab(self, label):
        return self.server.request(
            "layout.apply",
            workspace_id=self.parent["workspace_id"],
            tab_label=label,
            focus=False,
            root={
                "type": "pane",
                "cwd": str(self.server.root),
                "command": ["/bin/sh"],
            },
        )

    def simultaneous(self, create):
        # Both callers start together. Native request execution can be serial.
        barrier = threading.Barrier(2, timeout=8)

        def run():
            barrier.wait()
            return create()

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            futures = [pool.submit(run) for _ in range(2)]
            return [future.result(timeout=15) for future in futures]

    def test_simultaneous_same_name_workspaces_keep_distinct_ids(self):
        before = self.snapshot()
        results = self.simultaneous(
            lambda: self.server.request(
                "workspace.create", label="same", cwd=str(self.server.root), focus=False
            )
        )
        ids = {result["root_pane"]["workspace_id"] for result in results}
        self.assertEqual(len(ids), 2)
        after = self.snapshot()
        self.assertEqual(
            {w["workspace_id"] for w in after["workspaces"] if w["label"] == "same"},
            ids,
        )
        self.assertEqual(len(after["workspaces"]), len(before["workspaces"]) + 2)
        self.assertEqual(after["focused_tab_id"], before["focused_tab_id"])

    def test_simultaneous_same_name_tabs_keep_distinct_ids(self):
        before = self.snapshot()
        self.simultaneous(lambda: self.create_tab("same"))
        after = self.snapshot()
        tabs = [t for t in after["tabs"] if t["label"] == "same"]
        self.assertEqual(len(tabs), 2)
        self.assertEqual(len({t["tab_id"] for t in tabs}), 2)
        self.assertTrue(
            all(t["workspace_id"] == self.parent["workspace_id"] for t in tabs)
        )
        self.assertEqual(len(after["tabs"]), len(before["tabs"]) + 2)
        self.assertEqual(after["focused_tab_id"], before["focused_tab_id"])

    def test_parent_removed_between_discovery_and_allocation(self):
        self.assertIn(
            self.parent["workspace_id"],
            {w["workspace_id"] for w in self.snapshot()["workspaces"]},
        )
        self.server.request("workspace.close", workspace_id=self.parent["workspace_id"])
        replacement = self.server.request(
            "workspace.create", label="parent", cwd=str(self.server.root), focus=False
        )["root_pane"]
        self.assertNotEqual(replacement["workspace_id"], self.parent["workspace_id"])
        before = self.snapshot()
        with self.assertRaises(RuntimeError) as error:
            self.create_tab("must-not-exist")
        self.assertIn("workspace", str(error.exception).lower())
        after = self.snapshot()
        self.assertEqual(after["tabs"], before["tabs"])
        self.assertEqual(after["panes"], before["panes"])
        self.assertEqual(after["focused_tab_id"], before["focused_tab_id"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter-test-binary", type=Path)
    args, remaining = parser.parse_known_args()
    ADAPTER_TEST_BINARY = (
        args.adapter_test_binary.resolve() if args.adapter_test_binary else None
    )
    unittest.main(argv=[__file__, *remaining], verbosity=2)
