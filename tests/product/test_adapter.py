import hashlib
import os
import pathlib
import platform
import json
import shutil
import socket
import struct
import subprocess
import tempfile
import time
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path(os.environ.get("JJ_TEST_BIN", ROOT / "target/debug/jjump")).resolve()


class AdapterLifecycle(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name).resolve()
        self.home = self.root / "home"
        self.home.mkdir()
        self.env = dict(os.environ, HOME=str(self.home), J_JUMP_HOME=str(self.root / "state"))
        self.env.pop("TYPESAFE_API_KEY", None)
        self.env.pop("J_JUMP_CONFIG", None)
        self.env.pop("J_JUMP_OFFLINE", None)
        proxy_keys = ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy",
                      "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy", "REQUEST_METHOD")
        for key in proxy_keys:
            self.env.pop(key, None)
        config_path = self.root / "state/config/config.json"
        profile = (os.fsencode(config_path) + b"\0" +
                   os.fsencode(self.root / "state/cache") + b"\0adapter-v4" +
                   b"".join(key.encode() + b"\0\0" for key in proxy_keys))
        digest = hashlib.sha256(profile).hexdigest()[:24]
        base = pathlib.Path("/private/tmp" if platform.system() == "Darwin" else "/tmp")
        self.runtime = base / f"jj-{os.geteuid()}-{digest}"
        self.run_cli("config", "set", "semantic", "on")
        self.process = None

    def tearDown(self):
        if self.process and self.process.poll() is None:
            self.run_cli("adapter", "stop")
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=2)
        if self.process:
            self.process.stderr.close()
        if self.runtime.exists():
            shutil.rmtree(self.runtime)
        self.temp.cleanup()

    def run_cli(self, *args, code=0):
        p = subprocess.run([str(BIN), *args], env=self.env, cwd=self.home,
                           capture_output=True, timeout=5)
        self.assertEqual(p.returncode, code, (args, p.stdout, p.stderr))
        return p

    def start(self):
        self.process = subprocess.Popen([str(BIN), "adapter-serve"], env=self.env,
                                        cwd=self.home, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.PIPE)
        self.assert_running()

    def assert_running(self):
        until = time.monotonic() + 2
        while time.monotonic() < until:
            status = self.run_cli("adapter", "status").stdout
            if status == b"adapter: running\n":
                return
            time.sleep(.02)
        detail = self.process.stderr.read() if self.process.poll() is not None else b"still running"
        self.fail(f"private adapter did not become ready: {self.process.poll()} {detail!r}")

    def test_lifecycle_private_socket_and_semantic_off(self):
        self.assertEqual(self.run_cli("adapter", "status").stdout, b"adapter: stopped\n")
        self.start()
        sockets = list(self.runtime.glob("jev.sock"))
        self.assertEqual(len(sockets), 1)
        self.assertEqual(sockets[0].stat().st_mode & 0o777, 0o600)
        self.assertEqual(sockets[0].parent.stat().st_mode & 0o777, 0o700)
        self.run_cli("config", "set", "semantic", "off")
        self.process.wait(timeout=2)
        self.assertEqual(self.run_cli("adapter", "status").stdout, b"adapter: stopped\n")

    def test_restart_is_lazy_and_socket_substitution_rejected(self):
        self.start()
        self.run_cli("adapter", "restart")
        self.process.wait(timeout=2)
        self.assertEqual(self.run_cli("adapter", "status").stdout, b"adapter: stopped\n")
        socket = self.runtime / "jev.sock"
        if socket and socket.exists():
            socket.unlink()
        socket.symlink_to(self.root / "state/config/config.json")
        self.run_cli("adapter", "status", code=7)

    def test_large_framed_request_is_read_before_safe_rejection(self):
        self.start()
        request_id = "0" * 32
        frame = json.dumps({
            "kind": "Send", "version": 4, "id": request_id,
            "fingerprint": "deliberately-wrong", "credential_epoch": "absent",
            "remaining_ms": 1000,
            "deadline_tick_ns": time.clock_gettime_ns(time.CLOCK_MONOTONIC) + 1_000_000_000,
            "key": "synthetic-test-key", "payload": "x" * 12_000,
        }).encode()
        self.assertGreater(len(frame), 8_192)
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(2)
            client.connect(str(self.runtime / "jev.sock"))
            client.sendall(struct.pack(">I", len(frame)) + frame)
            size = struct.unpack(">I", self.recv_exact(client, 4))[0]
            reply = json.loads(self.recv_exact(client, size))
        self.assertEqual(reply["id"], request_id)
        self.assertEqual(reply["status"], "fingerprint")
        self.assertEqual(reply["body"], "")
        self.assert_running()

    @staticmethod
    def recv_exact(client, count):
        data = b""
        while len(data) < count:
            part = client.recv(count - len(data))
            if not part:
                raise AssertionError("adapter closed before complete frame")
            data += part
        return data

    def test_local_navigation_and_preview_do_not_launch_adapter(self):
        target = self.home / "alpha"
        target.mkdir()
        p = subprocess.run([str(BIN), "record", "--", str(target)], env=dict(self.env, PWD=str(target)),
                           cwd=target, capture_output=True, timeout=5)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(self.run_cli("query", "alpha").stdout, (str(target) + "\n").encode())
        self.run_cli("preview", "alpha")
        self.assertEqual(self.run_cli("adapter", "status").stdout, b"adapter: stopped\n")

    def test_changed_proxy_environment_does_not_reuse_old_adapter(self):
        self.start()
        self.env["HTTPS_PROXY"] = "http://127.0.0.1:19091"
        self.assertEqual(self.run_cli("adapter", "status").stdout, b"adapter: stopped\n")
        self.env.pop("HTTPS_PROXY")
        self.assert_running()


if __name__ == "__main__":
    unittest.main()
