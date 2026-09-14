"""Isolated Herdr 0.9.0 primitive probes, not a workmux backend fixture."""

import fcntl
import json
import os
import pty
import select
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import termios
import threading
import time
from pathlib import Path

HERDR_VERSION = "0.9.0"
HERDR_PROTOCOL = 22


def wait_until(check, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.03)
    raise TimeoutError(f"Herdr probe timed out: {check}")


class HerdrClient:
    """Run the real UI binary in a test-owned PTY and drain terminal output."""

    def __init__(self, server):
        self.master, slave = pty.openpty()
        self.output = bytearray()
        self.closed = False
        try:
            self.resize(40, 140)
            self.process = subprocess.Popen(
                [server.binary, "--session", "probe"],
                stdin=slave,
                stdout=slave,
                stderr=slave,
                env=server.env,
                cwd=server.root,
                start_new_session=True,
            )
        except BaseException:
            os.close(self.master)
            raise
        finally:
            os.close(slave)
        self.stopping = threading.Event()
        self.reader = threading.Thread(target=self._drain, daemon=True)
        self.reader.start()

    def _drain(self):
        while not self.stopping.is_set():
            if not select.select([self.master], [], [], 0.05)[0]:
                continue
            try:
                data = os.read(self.master, 65536)
                if not data:
                    return
                self.output.extend(data)
                del self.output[: max(0, len(self.output) - 2 * 1024 * 1024)]
                # Terminal capability queries, not synthetic Herdr clients.
                for query, response in [
                    (b"\x1b[6n", b"\x1b[1;1R"),
                    (b"\x1b[c", b"\x1b[?1;2c"),
                ]:
                    if query in data:
                        os.write(self.master, response)
            except OSError:
                return

    def resize(self, rows, columns):
        fcntl.ioctl(
            self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0)
        )
        # Popen starts a new session without acquiring a controlling terminal.
        # Deliver the same resize signal that a terminal host would send.
        if hasattr(self, "process") and self.process.poll() is None:
            self.process.send_signal(signal.SIGWINCH)

    def send(self, data):
        assert self.process.poll() is None, bytes(self.output).decode(errors="replace")
        while data:
            if not select.select([], [self.master], [], 8)[1]:
                raise TimeoutError("Herdr PTY input timed out")
            written = os.write(self.master, data[:4096])
            data = data[written:]

    def close(self):
        if self.closed:
            return
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)
        self.stopping.set()
        self.reader.join(timeout=2)
        os.close(self.master)
        self.closed = True


class HerdrServer:
    """Own all HOME, XDG, config, socket, server, client and pane resources.

    The short root avoids AF_UNIX path limits on macOS. No inherited mux,
    agent, shell startup or session environment can select a user's server.
    """

    def __init__(self, config=""):
        self.binary = shutil.which("herdr")
        if not self.binary:
            raise FileNotFoundError("Herdr is required; primitive gate remains pending")
        self.temp = tempfile.TemporaryDirectory(prefix="wm-herdr-", dir="/tmp")
        self.root = Path(self.temp.name)
        self.env = {
            "PATH": "/usr/bin:/bin",
            "TERM": "xterm-256color",
            "LANG": "en_US.UTF-8",
            "SHELL": "/bin/sh",
        }
        for name in [
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
            "XDG_DATA_HOME",
            "XDG_RUNTIME_DIR",
            "TMPDIR",
        ]:
            path = self.root / name.lower()
            path.mkdir(mode=0o700)
            self.env[name] = str(path)
        self.config_path = self.root / "config.toml"
        self.config_path.write_text(
            'onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\nshell_mode = "non_login"\n'
            "[update]\nversion_check = false\nmanifest_check = false\n" + config
        )
        self.env["HERDR_CONFIG_PATH"] = str(self.config_path)
        self.socket_path = (
            Path(self.env["XDG_CONFIG_HOME"]) / "herdr/sessions/probe/herdr.sock"
        )
        self.process = None
        self.clients = []
        self.log = None
        self.requests = []
        self.closed = False
        log_dir = os.environ.get("WORKMUX_HERDR_LOG_DIR")
        self.artifact_dir = Path(log_dir) / self.root.name if log_dir else None

    def cli(self, *args):
        return subprocess.run(
            [self.binary, "--session", "probe", *args],
            env=self.env,
            cwd=self.root,
            capture_output=True,
            text=True,
            check=True,
            timeout=10,
        ).stdout

    def start(self):
        if self.closed or self.process is not None:
            raise RuntimeError("Herdr fixture is closed or already started")
        version = self.cli("--version").strip()
        if version != f"herdr {HERDR_VERSION}":
            raise RuntimeError(f"Unsupported Herdr client: {version}")
        self.log = (self.root / "server.log").open("ab")
        try:
            self.process = subprocess.Popen(
                [self.binary, "--session", "probe", "server"],
                env=self.env,
                cwd=self.root,
                stdin=subprocess.DEVNULL,
                stdout=self.log,
                stderr=self.log,
                start_new_session=True,
            )
            wait_until(
                lambda: self.socket_path.exists() or self.process.poll() is not None
            )
            assert self.process.poll() is None, (self.root / "server.log").read_text()
            snapshot = self.request("session.snapshot")
            actual = (snapshot["snapshot"]["version"], snapshot["snapshot"]["protocol"])
            if actual != (HERDR_VERSION, HERDR_PROTOCOL):
                raise RuntimeError(f"Unsupported Herdr server: {actual}")
            return snapshot
        except BaseException:
            self.stop()
            raise

    def request(self, method, **params):
        request_id = str(len(self.requests))
        with socket.socket(socket.AF_UNIX) as conn:
            conn.settimeout(8)
            conn.connect(str(self.socket_path))
            conn.sendall(
                json.dumps(
                    {"id": request_id, "method": method, "params": params}
                ).encode()
                + b"\n"
            )
            with conn.makefile("rb") as reader:
                response = json.loads(reader.readline(8 * 1024 * 1024))
        self.requests.append((method, params, response))
        assert response["id"] == request_id, response
        if "error" in response:
            raise RuntimeError(response)
        return response["result"]

    def attach(self):
        client = HerdrClient(self)
        self.clients.append(client)
        return client

    def stop(self):
        client_outputs = []
        for client in self.clients:
            client.close()
            client_outputs.append(bytes(client.output))
        self.clients.clear()
        if self.process is not None:
            if self.process.poll() is None:
                try:
                    self.request("server.stop")
                    self.process.wait(timeout=5)
                except (
                    OSError,
                    RuntimeError,
                    ValueError,
                    AssertionError,
                    subprocess.TimeoutExpired,
                ):
                    self.process.terminate()
                    try:
                        self.process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        self.process.kill()
                        self.process.wait(timeout=3)
            self.process = None
        if self.log is not None:
            self.log.close()
            self.log = None
        # Optional log failures must not prevent owned process cleanup.
        if self.artifact_dir and client_outputs:
            self.artifact_dir.mkdir(parents=True, exist_ok=True)
            for index, output in enumerate(client_outputs):
                (self.artifact_dir / f"client-{index}.pty").write_bytes(output)

    def close(self):
        if self.closed:
            return
        try:
            self.stop()
            if self.artifact_dir:
                self.artifact_dir.mkdir(parents=True, exist_ok=True)
                for log_path in [
                    self.root / "server.log",
                    self.socket_path.parent / "herdr-server.log",
                ]:
                    if log_path.exists():
                        shutil.copyfile(log_path, self.artifact_dir / log_path.name)
                (self.artifact_dir / "requests.json").write_text(
                    json.dumps(self.requests, indent=2) + "\n"
                )
        finally:
            self.temp.cleanup()
            self.closed = True
