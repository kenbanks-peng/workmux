"""Programmatic focus must not acknowledge agent status.

Run directly after `CARGO_BUILD_JOBS=1 cargo build`. Each unittest owns a
private live Herdr server, HOME, repository, and (when needed) UI PTYs.
No inherited server is used. This tests false acknowledgement prevention,
not recognition of real user acknowledgement on protocol 22.
"""

import json
import time
import unittest

from focus_checks import FocusEvents
from server import HerdrServer, wait_until
from support_checks import Fixture, assert_status, launch_status_agent


class ProgrammaticFocusAcknowledgement(unittest.TestCase):
    def setUp(self):
        self.server = HerdrServer()
        self.addCleanup(self.server.close)
        self.server.start()
        self.fixture = Fixture(self.server)
        self.pane, self.environment = launch_status_agent(self.fixture)
        self.away = self.server.request("workspace.create", label="away", focus=False)[
            "root_pane"
        ]

    def attach_clients(self, count):
        clients = [self.server.attach() for _ in range(count)]
        for client in clients:
            wait_until(lambda client=client: b"parent" in client.output)
            self.assertIsNone(client.process.poll())
        return clients

    def assert_status_remains(self, state):
        # Observe for a bounded interval, rather than checking only before a
        # queued focus event could be handled. Check both native and stored state.
        deadline = time.monotonic() + 0.5
        while True:
            assert_status(self.fixture, self.pane, state)
            if time.monotonic() >= deadline:
                break
            time.sleep(0.05)

    def check_programmatic_focus(self):
        pane = self.pane
        operations = {
            "pane.focus": lambda: self.server.request(
                "pane.focus", pane_id=pane["pane_id"]
            ),
            "tab.focus": lambda: self.server.request(
                "tab.focus", tab_id=pane["tab_id"]
            ),
            "workspace.focus": lambda: self.server.request(
                "workspace.focus", workspace_id=pane["workspace_id"]
            ),
            "workmux open": lambda: self.fixture.run("open", "feature"),
        }
        with FocusEvents(self.server) as subscription:
            for state in ("waiting", "done", "working"):
                for name, focus in operations.items():
                    with self.subTest(state=state, operation=name):
                        self.server.request("pane.focus", pane_id=pane["pane_id"])
                        self.fixture.run(
                            "set-window-status", state, env=self.environment
                        )
                        # A status update while already focused is not consent.
                        self.assert_status_remains(state)
                        self.server.request(
                            "workspace.focus", workspace_id=self.away["workspace_id"]
                        )
                        snapshot = self.server.request("session.snapshot")["snapshot"]
                        self.assertEqual(
                            snapshot["focused_workspace_id"], self.away["workspace_id"]
                        )
                        subscription.read()
                        focus()
                        snapshot = wait_until(
                            lambda: self.focused_snapshot(pane["pane_id"])
                        )
                        events = subscription.read()
                        # Prove focus really happened, including event delivery.
                        self.assertTrue(events, f"No focus event for {name}")
                        self.assertTrue(
                            all(
                                event.get("event")
                                in {"pane_focused", "tab_focused", "workspace_focused"}
                                for event in events
                            ),
                            events,
                        )
                        subscription.record(f"{state}: {name}", snapshot, events)
                        self.assert_status_remains(state)
                        # Repeated focus on the same target is not consent either.
                        focus()
                        self.assert_status_remains(state)

        # Positive control: explicit clear removes Workmux status. Native Herdr
        # can fall back to its detected agent status after authority is cleared;
        # that fallback is not an acknowledgement of Workmux status.
        self.fixture.run("set-window-status", "clear", env=self.environment)
        agents = json.loads(self.fixture.run("status", "--json").stdout)["agents"]
        self.assertEqual(len(agents), 1, agents)
        self.assertEqual(agents[0]["status"], "-", agents)

    def focused_snapshot(self, pane_id):
        snapshot = self.server.request("session.snapshot")["snapshot"]
        return snapshot if snapshot["focused_pane_id"] == pane_id else None

    def test_without_ui(self):
        self.check_programmatic_focus()

    def test_with_one_ui(self):
        self.attach_clients(1)
        self.check_programmatic_focus()

    def test_with_two_uis(self):
        self.attach_clients(2)
        self.check_programmatic_focus()

    def test_after_all_uis_detach(self):
        for client in self.attach_clients(2):
            client.close()
            self.assertIsNotNone(client.process.poll())
        self.check_programmatic_focus()


if __name__ == "__main__":
    unittest.main(verbosity=2)
