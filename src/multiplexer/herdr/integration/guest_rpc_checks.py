"""Guest CLI -> real TCP supervisor -> private live Herdr regression tests.

Run after `CARGO_BUILD_JOBS=1 cargo build`:
  python3 src/multiplexer/herdr/integration/guest_rpc_checks.py -v

The container launcher is controlled: it captures the supervisor's guest
variables instead of starting a container. This does NOT test a VM/container,
its networking, or its filesystem boundary. No Herdr responses are mocked.
"""

import json
import shlex
import subprocess
import sys
import unittest
from pathlib import Path

from server import HerdrServer, wait_until
from support_checks import BINARY, Fixture


class GuestRpcTests(unittest.TestCase):
    def setUp(self):
        self.server = HerdrServer()
        self.addCleanup(self.server.close)
        # Set PATH before server start so its terminal children use only our
        # controlled runtime, never a real Docker daemon.
        tools = self.server.root / "bin"
        tools.mkdir()
        self.server.env["PATH"] = f"{tools}:/usr/bin:/bin"
        self.ready = self.server.root / "guest.json"
        self.release = self.server.root / "release"
        runtime = tools / "docker"
        runtime.write_text(
            f"#!{sys.executable}\n"
            "import json, pathlib, sys, time\n"
            "args = sys.argv[1:]\n"
            "if args[:2] == ['image', 'inspect'] or args[:1] == ['stop']:\n"
            "    sys.exit(0)\n"
            "assert args[0] == 'run', args\n"
            "values = {}\n"
            "for i, arg in enumerate(args[:-1]):\n"
            "    if arg in ('-e', '--env') and '=' in args[i+1]:\n"
            "        key, value = args[i+1].split('=', 1)\n"
            "        if key.startswith('WM_'): values[key] = value\n"
            f"ready = pathlib.Path({str(self.ready)!r})\n"
            "ready.with_suffix('.pending').write_text(json.dumps(values))\n"
            "ready.with_suffix('.pending').replace(ready)\n"
            "deadline = time.monotonic() + 90\n"
            f"while not pathlib.Path({str(self.release)!r}).exists():\n"
            "    assert time.monotonic() < deadline, 'test did not release launcher'\n"
            "    time.sleep(.03)\n"
        )
        runtime.chmod(0o700)
        config = Path(self.server.env["XDG_CONFIG_HOME"]) / "workmux/config.yaml"
        config.parent.mkdir()
        config.write_text(
            json.dumps(
                {
                    "sandbox": {
                        "backend": "container",
                        "runtime": "docker",
                        "image": "rpc-test-local",
                        "rpc_host": "127.0.0.1",
                        "host_commands": ["sh"],
                        "toolchain": "off",
                        "dangerously_allow_unsandboxed_host_exec": True,
                    }
                }
            )
        )
        self.server.start()
        self.f = Fixture(self.server)
        self.supervised = self.server.root / "supervised"
        subprocess.run(
            ["git", "worktree", "add", "-b", "supervised", str(self.supervised)],
            cwd=self.f.repo,
            env=self.f.env,
            check=True,
            capture_output=True,
        )
        self.log = self.server.root / "supervisor.log"
        self.done = self.server.root / "supervisor.exit"
        command = self.f.wm("sandbox", "run", str(self.supervised), "--", "true")
        # Keep the supervisor in the terminal's foreground process tree.
        # A detached shell would lose the verified Herdr caller ancestry.
        self.server.request(
            "pane.send_input",
            pane_id=self.f.parent["pane_id"],
            text=f"{command} > {shlex.quote(str(self.log))} 2>&1; "
            f"printf '%s' $? > {shlex.quote(str(self.done))}",
            keys=["enter"],
        )
        self.addCleanup(self.stop_supervisor)
        wait_until(lambda: self.ready.exists() or self.done.exists(), timeout=20)
        self.assertTrue(self.ready.exists(), self.log.read_text())
        values = json.loads(self.ready.read_text())
        self.assertEqual(values["WM_SANDBOX_GUEST"], "1")
        self.assertEqual(values["WM_RPC_HOST"], "127.0.0.1")
        # No host Herdr identity or endpoint reaches the guest CLI. Its only
        # route to the test server is through the authenticated supervisor.
        self.guest_env = {**self.server.env, **values}

    def stop_supervisor(self):
        self.release.touch()
        wait_until(lambda: self.done.exists() and self.done.read_text(), timeout=10)
        self.assertEqual(self.done.read_text(), "0", self.log.read_text())
        code, output = self.f.native(self.f.parent, "printf parent-alive")
        self.assertEqual((code, output), (0, "parent-alive"))

    def guest(self, *args, code=0, env=None):
        result = subprocess.run(
            [str(BINARY), *args],
            cwd=self.supervised,
            env=env or self.guest_env,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        return result

    def snapshot(self):
        return self.server.request("session.snapshot")["snapshot"]

    def spawn(self, name):
        self.guest("add", name, "--background")
        worktree = Path(self.f.run("path", name).stdout.strip())
        self.assertTrue(worktree.is_dir())
        snapshot = self.snapshot()
        tabs = [t for t in snapshot["tabs"] if t["label"] == f"wm-{name}"]
        self.assertEqual(len(tabs), 1, snapshot)
        self.assertEqual(tabs[0]["workspace_id"], self.f.parent["workspace_id"])
        self.assertEqual(snapshot["focused_tab_id"], self.f.parent["tab_id"])
        return worktree, tabs[0]["tab_id"]

    def test_spawn(self):
        worktree, tab = self.spawn("rpc-spawn")
        pane = next(p for p in self.snapshot()["panes"] if p["tab_id"] == tab)
        code, output = self.f.native(pane, "(pwd; printf rpc-shell-alive)")
        self.assertEqual(code, 0, output)
        self.assertEqual(Path(output.splitlines()[0]).resolve(), worktree.resolve())
        self.assertIn("rpc-shell-alive", output)
        # Duplicate requests must fail rather than allocate another tab.
        self.guest("add", "rpc-spawn", "--background", code=1)
        self.assertEqual(
            sum(t["label"] == "wm-rpc-spawn" for t in self.snapshot()["tabs"]), 1
        )

    def test_exec(self):
        result = self.guest(
            "host-exec",
            "sh",
            "-c",
            "printf 'rpc-out:%s' \"$PWD\"; printf rpc-err >&2; exit 7",
            code=7,
        )
        self.assertEqual(result.stdout, f"rpc-out:{self.supervised.resolve()}")
        self.assertEqual(result.stderr, "rpc-err")
        self.guest("host-exec", "touch", str(self.f.repo / "forbidden"), code=127)
        self.assertFalse((self.f.repo / "forbidden").exists())

    def test_merge(self):
        worktree, tab = self.spawn("rpc-merge")
        (worktree / "merged.txt").write_text("guest RPC commit\n")
        for args in (("add", "merged.txt"), ("commit", "-m", "RPC change")):
            subprocess.run(
                ["git", *args],
                cwd=worktree,
                env=self.f.env,
                check=True,
                capture_output=True,
            )
        self.guest("merge", "rpc-merge", "--into", "main")
        self.assertEqual((self.f.repo / "merged.txt").read_text(), "guest RPC commit\n")
        self.assertFalse(worktree.exists())
        self.assertNotIn(tab, [t["tab_id"] for t in self.snapshot()["tabs"]])
        self.assertIn(
            self.f.parent["terminal_id"],
            [p["terminal_id"] for p in self.snapshot()["panes"]],
        )

    def test_close(self):
        worktree, tab = self.spawn("rpc-close")
        self.guest("close", "rpc-close")
        self.assertNotIn(tab, [t["tab_id"] for t in self.snapshot()["tabs"]])
        self.assertTrue(worktree.is_dir(), "close must retain the worktree")
        self.assertIn(
            self.f.parent["terminal_id"],
            [p["terminal_id"] for p in self.snapshot()["panes"]],
        )

    def test_bad_token_cannot_spawn(self):
        before = self.snapshot()
        self.guest(
            "add",
            "unauthorized",
            "--background",
            code=1,
            env={**self.guest_env, "WM_RPC_TOKEN": "wrong-token"},
        )
        self.assertEqual(self.snapshot()["tabs"], before["tabs"])
        result = self.f.run("path", "unauthorized", ok=False)
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
