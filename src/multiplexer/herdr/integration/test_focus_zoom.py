"""Live zoom regression. Each test owns a private Herdr server and PTYs.

Run: python3 -m unittest discover -s src/multiplexer/herdr/integration \
    -p test_focus_zoom.py -v
No inherited socket, workspace, tab, or client is used.
"""

import shlex
import unittest

from server import HerdrServer, wait_until


class FocusZoomTests(unittest.TestCase):
    def setUp(self):
        self.server = HerdrServer()
        self.addCleanup(self.server.close)
        self.server.start()
        self.left = self.server.request(
            "workspace.create", label="focus-zoom", focus=True
        )["root_pane"]
        self.right = self.server.request(
            "pane.split", pane_id=self.left["pane_id"], direction="right", focus=False
        )["pane"]
        for name, pane in (("left", self.left), ("right", self.right)):
            ready = self.server.root / f"{name}-ready"
            self.input(
                pane, f"export FOCUS_TARGET={name}; touch {shlex.quote(str(ready))}"
            )
            wait_until(ready.exists)

    def input(self, pane, command):
        self.server.request(
            "pane.send_input", pane_id=pane["pane_id"], text=command, keys=["enter"]
        )

    def layout(self):
        return self.server.request("pane.layout", pane_id=self.left["pane_id"])[
            "layout"
        ]

    def attach(self):
        client = self.server.attach()
        wait_until(lambda: b"focus-zoom" in client.output)
        return client

    def test_resize_while_zoomed_preserves_target_and_shell(self):
        client = self.attach()
        before = self.server.request("session.snapshot")["snapshot"]["panes"]
        identities = {p["pane_id"]: p["terminal_id"] for p in before}

        def shell_size(label):
            output = self.server.root / f"size-{label}"
            self.input(self.right, f"stty size > {shlex.quote(str(output))}")
            wait_until(lambda: output.exists() and output.read_text().strip())
            return tuple(map(int, output.read_text().split()))

        split_size = shell_size("split")
        self.server.request("pane.zoom", pane_id=self.right["pane_id"], mode="on")
        wait_until(lambda: self.layout()["zoomed"])
        # Account for the native sidebar and pane borders. Check real terminal
        # size deltas, not an assumed full-screen size or just layout metadata.
        counter = iter(range(1000))
        zoom_size = wait_until(
            lambda: (
                size
                if (size := shell_size(f"zoom-{next(counter)}"))[1] > split_size[1]
                else None
            )
        )
        initial_area = self.layout()["area"]
        for index, (rows, columns) in enumerate(((30, 100), (50, 160), (24, 80))):
            client.resize(rows, columns)
            layout = wait_until(
                lambda rows=rows, columns=columns: (
                    value
                    if (value := self.layout())["area"]["width"]
                    == initial_area["width"] + columns - 140
                    and value["area"]["height"] == initial_area["height"] + rows - 40
                    else None
                )
            )
            self.assertTrue(layout["zoomed"])
            self.assertEqual(layout["focused_pane_id"], self.right["pane_id"])
            self.assertEqual(
                shell_size(index),
                (zoom_size[0] + rows - 40, zoom_size[1] + columns - 140),
            )
            self.assertIsNone(client.process.poll())

        self.server.request("pane.zoom", pane_id=self.right["pane_id"], mode="off")
        layout = wait_until(
            lambda: value if not (value := self.layout())["zoomed"] else None
        )
        self.assertEqual({p["pane_id"] for p in layout["panes"]}, set(identities))
        for pane in layout["panes"]:
            self.assertGreater(pane["rect"]["width"], 0)
            self.assertLess(pane["rect"]["width"], layout["area"]["width"])
        self.assertLess(shell_size("unzoom")[1], zoom_size[1] - 60)
        after = self.server.request("session.snapshot")["snapshot"]["panes"]
        self.assertEqual({p["pane_id"]: p["terminal_id"] for p in after}, identities)
        # Both original shells must remain usable after unzoom, not just listed.
        for name, pane in (("left", self.left), ("right", self.right)):
            output = self.server.root / f"survived-{name}"
            self.input(
                pane, f"printf '%s' \"$FOCUS_TARGET\" > {shlex.quote(str(output))}"
            )
            wait_until(lambda output=output: output.exists() and output.read_text())
            self.assertEqual(output.read_text(), name)


if __name__ == "__main__":
    unittest.main()
