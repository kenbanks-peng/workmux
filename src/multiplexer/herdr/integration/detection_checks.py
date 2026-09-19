"""Selection through real loopback SSH and two disposable live Herdr instances.

Usage: python3 detection_checks.py --test-binary /absolute/path/to/workmux-test
Build the Rust test binary first with CARGO_BUILD_JOBS=1 cargo test --no-run.
No existing SSH daemon, Herdr instance, or global configuration is used.
Failure to start/authenticate SSH is an error, not a passing or skipped test.
"""

import argparse
import getpass
import shlex
import shutil
import socket
import subprocess
import tempfile
from contextlib import ExitStack
from pathlib import Path

from server import HerdrServer, wait_until

PROBE = "multiplexer::herdr::remote_detection_tests::isolated_remote_selection"
CONFLICTS = {
    "WORKMUX_BACKEND": "tmux",
    "TMUX": "/test-owned/stale-tmux,1,0",
    "WEZTERM_PANE": "42",
    "ZELLIJ": "1",
    "KITTY_WINDOW_ID": "43",
    "HERDR_ACTIVE_PANE_ID": "stale-inherited-pane",
}


class PrivateSsh:
    """A loopback-only daemon with temporary keys and no password authentication."""

    def __init__(self, stack, root):
        tools = {name: shutil.which(name) for name in ("ssh", "sshd", "ssh-keygen")}
        if not all(tools.values()):
            raise RuntimeError(f"SSH tools are required: {tools}")
        self.root = root
        self.env = {"PATH": "/usr/bin:/bin", "HOME": str(root)}
        for key in ("host", "client"):
            subprocess.run(
                [
                    tools["ssh-keygen"],
                    "-q",
                    "-t",
                    "ed25519",
                    "-N",
                    "",
                    "-f",
                    str(root / key),
                ],
                env=self.env,
                check=True,
                capture_output=True,
                timeout=10,
            )
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        config = root / "sshd_config"
        config.write_text(
            f"Port {port}\nListenAddress 127.0.0.1\n"
            f"HostKey {root / 'host'}\nPidFile {root / 'sshd.pid'}\n"
            f"AuthorizedKeysFile {root / 'client.pub'}\nAllowUsers {getpass.getuser()}\n"
            "StrictModes no\nUsePAM no\nPasswordAuthentication no\n"
            "KbdInteractiveAuthentication no\nPubkeyAuthentication yes\n"
            "PermitRootLogin no\nPermitUserRC no\nPermitUserEnvironment no\n"
            "AllowTcpForwarding no\nX11Forwarding no\n"
            "AcceptEnv WORKMUX_BACKEND TMUX WEZTERM_PANE ZELLIJ KITTY_WINDOW_ID "
            "HERDR_ACTIVE_PANE_ID HERDR_SOCKET_PATH\n"
        )
        log = stack.enter_context((root / "sshd.log").open("w+"))
        self.process = subprocess.Popen(
            [tools["sshd"], "-D", "-e", "-f", str(config)],
            env=self.env,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=log,
        )
        stack.callback(self.close)
        public = (root / "host.pub").read_text().split()
        known_hosts = root / "known_hosts"
        known_hosts.write_text(f"[127.0.0.1]:{port} {public[0]} {public[1]}\n")
        self.command = [
            tools["ssh"],
            "-F",
            "/dev/null",
            "-p",
            str(port),
            "-i",
            str(root / "client"),
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "IdentityAgent=none",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            f"UserKnownHostsFile={known_hosts}",
            "-o",
            "GlobalKnownHostsFile=/dev/null",
            f"{getpass.getuser()}@127.0.0.1",
        ]

        def ready():
            if self.process.poll() is not None:
                raise RuntimeError((root / "sshd.log").read_text())
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                    return True
            except OSError:
                return False

        wait_until(ready)
        check = self.run("true", {})
        if check.returncode:
            raise RuntimeError(
                "Private SSH authentication failed:\n"
                + check.stderr
                + (root / "sshd.log").read_text()
            )

    def run(self, command, inherited):
        # SendEnv/AcceptEnv crosses the real SSH transport. The Rust probe checks
        # each received conflict before it applies the requested selection.
        send = [arg for name in inherited for arg in ("-o", f"SendEnv={name}")]
        return subprocess.run(
            [*self.command[:-1], *send, self.command[-1], command],
            env={**self.env, **inherited},
            capture_output=True,
            text=True,
            timeout=45,
            check=False,
        )

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", required=True, type=Path)
    args = parser.parse_args()
    binary = args.test_binary.resolve(strict=True)
    with ExitStack() as stack:
        root = Path(
            stack.enter_context(
                tempfile.TemporaryDirectory(prefix="wm-ssh-", dir="/tmp")
            )
        )
        ssh = PrivateSsh(stack, root)
        servers = []
        for _ in range(2):
            server = HerdrServer()
            stack.callback(server.close)
            server.start()
            servers.append(server)
        selected, inherited = servers
        before = inherited.request("session.snapshot")
        for case in ("override", "explicit", "missing"):
            command = shlex.join(
                [
                    "env",
                    f"HOME={root}",
                    "WORKMUX_DETECTION_REQUIRE_SSH=1",
                    f"WORKMUX_DETECTION_CASE={case}",
                    f"WORKMUX_DETECTION_SELECTED={selected.socket_path}",
                    str(binary),
                    "--exact",
                    PROBE,
                    "--ignored",
                    "--nocapture",
                ]
            )
            result = ssh.run(
                command,
                {
                    **CONFLICTS,
                    "HERDR_SOCKET_PATH": str(inherited.socket_path),
                },
            )
            if (
                result.returncode
                or f"REMOTE_SELECTION_PASSED {case}" not in result.stdout
            ):
                raise RuntimeError(result.stdout + result.stderr)
            print(f"PASS live Herdr / loopback SSH: {case}", flush=True)
        assert inherited.request("session.snapshot") == before, (
            "inherited instance changed"
        )
        print("3 passed (live Herdr, real loopback SSH; no terminal UI operations)")


if __name__ == "__main__":
    main()
