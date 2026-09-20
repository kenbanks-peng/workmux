"""Live label probes on a disposable server; run this file with Python 3.

These test native protocol state, not text layout in an attached terminal UI.
No inherited socket or target is used.
"""

import json
import unittest

from server import HerdrServer


class LiveLabelTests(unittest.TestCase):
    def setUp(self):
        self.server = HerdrServer()
        self.addCleanup(self.server.close)
        self.server.start()
        created = self.server.request(
            "workspace.create",
            label="label-probe",
            cwd=str(self.server.root),
            focus=False,
        )
        self.ids = {
            "workspace": created["workspace"]["workspace_id"],
            "tab": created["tab"]["tab_id"],
            "pane": created["root_pane"]["pane_id"],
        }

    def row(self, kind):
        collection = {"workspace": "workspaces", "tab": "tabs", "pane": "panes"}[kind]
        rows = self.server.request("session.snapshot")["snapshot"][collection]
        return next(row for row in rows if row[kind + "_id"] == self.ids[kind])

    def rename(self, kind, label):
        return self.server.request(
            kind + ".rename", **{kind + "_id": self.ids[kind], "label": label}
        )

    def test_session_empty_and_long_unicode_labels(self):
        for label in ["", "界🙂" * 4096]:
            with self.subTest(characters=len(label)):
                self.rename("workspace", label)
                self.assertEqual(self.row("workspace")["label"], label)

    def test_window_empty_and_long_unicode_labels(self):
        for label in ["", "界🙂" * 4096]:
            with self.subTest(characters=len(label)):
                self.rename("tab", label)
                self.assertEqual(self.row("tab")["label"], label)

    def test_pane_empty_clears_label_and_long_labels_are_not_truncated(self):
        for label in ["界🙂" * 4096, "x" * 65536, "x" * 262144]:
            with self.subTest(utf8_bytes=len(label.encode())):
                self.rename("pane", label)
                self.assertEqual(self.row("pane")["label"], label)
        self.rename("pane", "")
        self.assertIsNone(self.row("pane").get("label"))
        self.rename("pane", "restored")
        self.assertEqual(self.row("pane")["label"], "restored")

    def test_oversized_label_requests_leave_each_label_unchanged(self):
        # At 1 MiB, Herdr 0.9.0 disconnects or stops reading the request.
        # A bounded transport failure is not a label-validation error.
        # Verify state and recovery on a fresh connection in either case.
        for kind in ["workspace", "tab", "pane"]:
            with self.subTest(kind=kind):
                self.rename(kind, "before-rejection")
                with self.assertRaises(
                    (
                        json.JSONDecodeError,
                        ConnectionResetError,
                        BrokenPipeError,
                        TimeoutError,
                    )
                ):
                    self.rename(kind, "x" * 1048576)
                self.assertEqual(self.row(kind)["label"], "before-rejection")
                self.rename(kind, "after-rejection")
                self.assertEqual(self.row(kind)["label"], "after-rejection")


if __name__ == "__main__":
    unittest.main(verbosity=2)
