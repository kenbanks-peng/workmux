"""Protocol 22 focus capability checks. Passing does not mean acknowledgement works."""

import json
import select
import socket
import time

from server import wait_until


class FocusEvents:
    """Read the supported focus subscription on a test-owned server only."""

    def __init__(self, server):
        self.server = server
        self.connection = socket.socket(socket.AF_UNIX)
        self.buffer = b""
        self.evidence = []

    def __enter__(self):
        try:
            self.connection.settimeout(3)
            self.connection.connect(str(self.server.socket_path))
            self.connection.sendall(
                json.dumps(
                    {
                        "id": "focus-probe",
                        "method": "events.subscribe",
                        "params": {
                            "subscriptions": [
                                {"type": f"{kind}.focused"}
                                for kind in ("pane", "tab", "workspace")
                            ]
                        },
                    }
                ).encode()
                + b"\n"
            )
            assert self.read(1) == [
                {"id": "focus-probe", "result": {"type": "subscription_started"}}
            ]
            return self
        except BaseException:
            self.connection.close()
            raise

    def read(self, duration=0.3):
        messages = []
        deadline = time.monotonic() + duration
        while (remaining := deadline - time.monotonic()) > 0:
            if not select.select([self.connection], [], [], remaining)[0]:
                break
            data = self.connection.recv(65536)
            assert data, "Focus subscription closed before the probe finished"
            self.buffer += data
            while b"\n" in self.buffer:
                line, self.buffer = self.buffer.split(b"\n", 1)
                messages.append(json.loads(line))
        assert not self.buffer, "Incomplete focus event"
        return messages

    def record(self, phase, snapshot, events):
        self.evidence.append({"phase": phase, "snapshot": snapshot, "events": events})

    def __exit__(self, *_):
        self.connection.close()
        if self.server.artifact_dir:
            self.server.artifact_dir.mkdir(parents=True, exist_ok=True)
            (self.server.artifact_dir / "focus-protocol.json").write_text(
                json.dumps(self.evidence, indent=2) + "\n"
            )


def focus_fields(snapshot):
    return {
        key: snapshot[key]
        for key in ("focused_pane_id", "focused_tab_id", "focused_workspace_id")
    } | {
        kind: [(item[id_key], item["focused"]) for item in snapshot[kind]]
        for kind, id_key in (
            ("panes", "pane_id"),
            ("tabs", "tab_id"),
            ("workspaces", "workspace_id"),
        )
    }


def focus_protocol(f):
    """Show why neither events nor snapshots prove attached-client visibility."""
    server = f.server
    schema = json.loads(server.cli("api", "schema", "--json"))
    assert schema["protocol"] == 22
    snapshot_properties = schema["schemas"]["success_response"]["$defs"][
        "SessionSnapshot"
    ]["properties"]
    assert set(snapshot_properties) == {
        "version",
        "protocol",
        "focused_workspace_id",
        "focused_tab_id",
        "focused_pane_id",
        "workspaces",
        "tabs",
        "panes",
        "layouts",
        "agents",
    }, "Reassess focus support if the snapshot contract changes"
    left = f.parent
    right = server.request(
        "pane.split", pane_id=left["pane_id"], direction="right", focus=False
    )["pane"]

    def snapshot():
        return server.request("session.snapshot")["snapshot"]

    def selected(pane):
        value = snapshot()
        return value if value["focused_pane_id"] == pane["pane_id"] else None

    with FocusEvents(server) as subscription:
        # There is no UI process yet, but API focus still emits all three events.
        assert not server.clients
        server.request("pane.focus", pane_id=right["pane_id"])
        detached_events = subscription.read()
        expected = [
            {
                "event": "pane_focused",
                "data": {
                    "type": "pane_focused",
                    "pane_id": right["pane_id"],
                    "workspace_id": right["workspace_id"],
                },
            },
            {
                "event": "tab_focused",
                "data": {
                    "type": "tab_focused",
                    "tab_id": right["tab_id"],
                    "workspace_id": right["workspace_id"],
                },
            },
            {
                "event": "workspace_focused",
                "data": {
                    "type": "workspace_focused",
                    "workspace_id": right["workspace_id"],
                },
            },
        ]
        assert detached_events == expected, detached_events
        detached = snapshot()
        subscription.record("API focus without UI", detached, detached_events)

        first = server.attach()
        wait_until(lambda: b"parent" in first.output)
        first.send(b"\x02h")  # Default Ctrl-B h: manual focus left.
        manual = wait_until(lambda: selected(left))
        events = subscription.read()
        subscription.record("first client manual left", manual, events)
        assert events == [], events

        # The same event payload occurs with a real UI; it has no caller identity.
        server.request("pane.focus", pane_id=right["pane_id"])
        events = subscription.read()
        subscription.record("API focus with UI", snapshot(), events)
        assert events == detached_events, events

        second = server.attach()
        wait_until(lambda: b"parent" in second.output)
        second.send(b"\x02h")
        manual = wait_until(lambda: selected(left))
        events = subscription.read()
        subscription.record("second client manual left", manual, events)
        assert events == [], events
        first.send(b"\x02l")
        manual = wait_until(lambda: selected(right))
        events = subscription.read()
        subscription.record("first client manual right", manual, events)
        assert events == [], events

        before_detach = focus_fields(manual)
        first.close()
        second.close()
        detached_again = snapshot()
        events = subscription.read()
        subscription.record("both clients detached", detached_again, events)
        assert focus_fields(detached_again) == before_detach
        assert events == [], events

        # Prove the subscription is still live after the negative observations.
        server.request("pane.focus", pane_id=left["pane_id"])
        events = subscription.read()
        subscription.record("API focus after detach", snapshot(), events)
        assert [event["event"] for event in events] == [
            "pane_focused",
            "tab_focused",
            "workspace_focused",
        ], events

    print(
        "BLOCKED focus acknowledgement (protocol capability check passed): "
        "API focus events also occur without a UI and have no client identity; "
        "manual focus from two clients changes snapshots but emits no focus events; "
        "snapshot focus remains set after both clients detach"
    )
