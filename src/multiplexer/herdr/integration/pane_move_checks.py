"""Live pane move regressions on a disposable Herdr server (no workmux build).

Run directly with Python. No inherited socket or user terminal is used.
These tests cover native protocol behavior, not automatic adapter recovery.
"""

import json
import shlex
import socket
import unittest

from server import HerdrServer, wait_until


class PaneMoves(unittest.TestCase):
    def setUp(self):
        self.server = HerdrServer()
        self.addCleanup(self.server.close)
        self.server.start()
        self.sequence = 0

    def snapshot(self):
        return self.server.request("session.snapshot")["snapshot"]

    def workspace(self, label):
        return self.server.request("workspace.create", label=label, focus=False)[
            "root_pane"
        ]

    def split(self, pane, direction="right"):
        return self.server.request(
            "pane.split",
            target_pane_id=pane["pane_id"],
            direction=direction,
            focus=False,
        )["pane"]

    def locate(self, pane):
        matches = [
            p
            for p in self.snapshot()["panes"]
            if p["terminal_id"] == pane["terminal_id"]
        ]
        self.assertEqual(len(matches), 1, matches)
        return matches[0]

    def shell_state(self, pane, initialize=False):
        """Prove the same shell still accepts input and retains private state."""
        self.sequence += 1
        output = self.server.root / f"shell-{self.sequence}"
        command = "MOVE_TOKEN=retained; " if initialize else ""
        command += f'printf \'%s:%s\' "$$" "$MOVE_TOKEN" > {shlex.quote(str(output))}'
        self.server.request(
            "pane.send_input",
            pane_id=self.locate(pane)["pane_id"],
            text=command,
            keys=["enter"],
        )
        wait_until(lambda: output.exists() and output.read_text().endswith(":retained"))
        return output.read_text()

    def params(self, source, destination, direction="down"):
        return {
            "pane_id": source["pane_id"],
            "destination": {
                "type": "tab",
                "tab_id": destination["tab_id"],
                "target_pane_id": destination["pane_id"],
                "split": direction,
                "ratio": 0.4,
            },
            "focus": False,
        }

    def move(self, source, destination, direction="down"):
        return self.server.request(
            "pane.move", **self.params(source, destination, direction)
        )

    def topology(self):
        """Ignore focus and output revisions; compare terminal placement and layout."""
        snapshot = self.snapshot()
        return {
            "panes": sorted(
                (p["terminal_id"], p["workspace_id"], p["tab_id"], p["pane_id"])
                for p in snapshot["panes"]
            ),
            "layouts": snapshot["layouts"],
        }

    def assert_alive(self, states):
        for pane, state in states:
            self.assertEqual(self.shell_state(pane), state)

    def test_move_between_split_tabs_preserves_all_shells(self):
        source = self.workspace("source")
        source_peer = self.split(source)
        destination = self.workspace("destination")
        destination_peer = self.split(destination, "down")
        panes = [source, source_peer, destination, destination_peer]
        states = [(p, self.shell_state(p, initialize=True)) for p in panes]
        self.assertEqual(destination_peer["tab_id"], destination["tab_id"])
        result = self.move(source, destination_peer)["move_result"]
        self.assertTrue(result["changed"])
        wait_until(lambda: self.locate(source)["tab_id"] == destination["tab_id"])
        moved = self.locate(source)
        self.assertEqual(moved["tab_id"], destination["tab_id"])
        self.assertEqual(moved["workspace_id"], destination["workspace_id"])
        self.assertEqual(self.locate(source_peer)["tab_id"], source_peer["tab_id"])
        self.assertEqual(len(self.snapshot()["panes"]), 4)
        self.assert_alive(states)

    def test_same_tab_move_is_explicit_noop_and_preserves_nested_split(self):
        root = self.workspace("nested")
        right = self.split(root)
        lower = self.split(right, "down")
        states = [
            (p, self.shell_state(p, initialize=True)) for p in [root, right, lower]
        ]
        before = self.topology()
        result = self.move(lower, root, "right")["move_result"]
        self.assertFalse(result["changed"])
        self.assertEqual(result["reason"], "same_tab")
        self.assertEqual(self.topology(), before)
        self.assertEqual(len(self.snapshot()["panes"]), 3)
        self.assert_alive(states)

    def test_nested_split_relocation_through_staging_tab(self):
        root = self.workspace("nested")
        right = self.split(root)
        lower = self.split(right, "down")
        staging = self.workspace("staging")
        states = [
            (p, self.shell_state(p, initialize=True))
            for p in [root, right, lower, staging]
        ]
        before = self.topology()["layouts"]
        self.move(lower, staging)
        wait_until(lambda: self.locate(lower)["tab_id"] == staging["tab_id"])
        self.assert_alive(states)
        # Resume from current identity after the intermediate move. A direct
        # same-tab move is a native no-op, so relocation needs a staging tab.
        self.move(self.locate(lower), self.locate(root), "down")
        wait_until(lambda: self.locate(lower)["tab_id"] == root["tab_id"])
        self.assertNotEqual(self.topology()["layouts"], before)
        layout = next(
            item
            for item in self.snapshot()["layouts"]
            if item["tab_id"] == root["tab_id"]
        )
        rects = {item["pane_id"]: item["rect"] for item in layout["panes"]}
        root_rect = rects[self.locate(root)["pane_id"]]
        lower_rect = rects[self.locate(lower)["pane_id"]]
        right_rect = rects[self.locate(right)["pane_id"]]
        self.assertEqual(lower_rect["x"], root_rect["x"])
        self.assertGreater(lower_rect["y"], root_rect["y"])
        self.assertGreater(right_rect["x"], root_rect["x"])
        self.assertEqual(len(self.snapshot()["panes"]), 4)
        self.assert_alive(states)

    def test_destination_tab_disappears_after_resolution(self):
        source = self.workspace("source")
        peer = self.split(source)
        destination = self.workspace("destination")
        states = [(p, self.shell_state(p, initialize=True)) for p in [source, peer]]
        self.server.request("tab.close", tab_id=destination["tab_id"])
        before = self.topology()
        with self.assertRaises(RuntimeError):
            self.move(source, destination)
        self.assertEqual(self.topology(), before)
        self.assert_alive(states)

    def test_source_disappears_after_resolution(self):
        source = self.workspace("source")
        peer = self.split(source)
        destination = self.workspace("destination")
        states = [
            (p, self.shell_state(p, initialize=True)) for p in [peer, destination]
        ]
        self.server.request("pane.close", pane_id=source["pane_id"])
        before = self.topology()
        with self.assertRaises(RuntimeError):
            self.move(source, destination)
        self.assertEqual(self.topology(), before)
        self.assert_alive(states)

    def test_destination_disappears_after_resolution_then_retry(self):
        source = self.workspace("source")
        destination = self.workspace("destination")
        peer = self.split(destination)
        states = [(p, self.shell_state(p, initialize=True)) for p in [source, peer]]
        self.server.request("pane.close", pane_id=destination["pane_id"])
        before = self.topology()
        with self.assertRaises(RuntimeError):
            self.move(source, destination)
        self.assertEqual(self.topology(), before)
        self.assert_alive(states)
        self.move(self.locate(source), self.locate(peer))
        wait_until(lambda: self.locate(source)["tab_id"] == peer["tab_id"])
        self.assertEqual(self.locate(source)["tab_id"], peer["tab_id"])
        self.assert_alive(states)

    def test_lost_response_reconciles_by_terminal_identity(self):
        source = self.workspace("source")
        source_peer = self.split(source)
        destination = self.workspace("destination")
        states = [
            (p, self.shell_state(p, initialize=True))
            for p in [source, source_peer, destination]
        ]
        # Stop sending after submission, without reading the response. The real
        # server processes the move; a fresh connection must discover its result.
        # Keep the socket open until commit so the fault is deterministic: this
        # covers a lost acknowledgement, not interruption inside the server.
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(8)
            connection.connect(str(self.server.socket_path))
            connection.sendall(
                json.dumps(
                    {
                        "id": "unacknowledged-move",
                        "method": "pane.move",
                        "params": self.params(source, destination),
                    }
                ).encode()
                + b"\n"
            )
            connection.shutdown(socket.SHUT_WR)
            wait_until(lambda: self.locate(source)["tab_id"] == destination["tab_id"])
        moved = self.locate(source)
        self.assertNotEqual(moved["pane_id"], source["pane_id"])
        self.assertEqual(len(self.snapshot()["panes"]), 3)
        self.assert_alive(states)
        # Use the observed current address, not a blind replay of the old move.
        self.move(moved, self.locate(source_peer))
        wait_until(lambda: self.locate(source)["tab_id"] == source_peer["tab_id"])
        self.assertEqual(self.locate(source)["tab_id"], source_peer["tab_id"])
        self.assert_alive(states)


if __name__ == "__main__":
    unittest.main(verbosity=2)
