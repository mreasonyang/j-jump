"""Offline release-download selection, integrity and real installer lifecycle.

Set JJ_DOWNLOAD_TEST_ARCHIVE to a native clean-source archive for the optional
real-binary case. Other CPU/OS cases prove selection only, using synthetic files.
"""
import hashlib
import http.server
import io
import json
import os
import shlex
import shutil
import signal
import ssl
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = (ROOT / "VERSION").read_text().strip()
REPOSITORY = "example/j-jump"

CURL_FIXTURE = r'''import json, os, pathlib, shutil, sys, time
args = sys.argv[1:]
url = args[-1]
with open(os.environ["DOWNLOAD_LOG"], "a") as log:
    log.write(json.dumps(args) + "\n")
kind = "latest" if url.endswith("/latest") else "sidecar" if url.endswith(".sha256") else "archive"
if os.environ.get("DOWNLOAD_PAUSE") == kind:
    pathlib.Path(os.environ["DOWNLOAD_READY"]).touch()
    time.sleep(30)
if os.environ.get("DOWNLOAD_FAILURE") == kind:
    print("synthetic HTTP/download failure", file=sys.stderr)
    sys.exit(22)
if kind == "latest":
    print(os.environ["DOWNLOAD_LATEST"], end="")
else:
    source = json.loads(os.environ["DOWNLOAD_FILES"]).get(url)
    if source is None:
        print("synthetic missing release asset", file=sys.stderr)
        sys.exit(22)
    if pathlib.Path(source).stat().st_size > int(args[args.index("--max-filesize")+1]):
        sys.exit(63)
    shutil.copyfile(source, args[args.index("--output")+1])
'''


class DownloadInstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.tmp = self.root / "tmp"
        self.tmp.mkdir()
        self.shims = self.root / "shims"
        self.shims.mkdir()
        self.prefix = self.root / "prefix with spaces"
        self.files = {}
        self.log = self.root / "curl.jsonl"
        self.env = dict(os.environ, HOME=str(self.home), TMPDIR=str(self.tmp),
                        PATH=str(self.shims) + os.pathsep + os.environ["PATH"],
                        DOWNLOAD_LOG=str(self.log), DOWNLOAD_SYSTEM="Darwin",
                        DOWNLOAD_MACHINE="arm64", DOWNLOAD_MACOS="15.0",
                        DOWNLOAD_LATEST=f"https://github.com/{REPOSITORY}/releases/tag/v{VERSION}",
                        DOWNLOAD_FILES="{}", DOWNLOAD_READY=str(self.root / "ready"))
        for key in ("DOWNLOAD_FAILURE", "DOWNLOAD_PAUSE"):
            self.env.pop(key, None)
        self.shim("curl", f"#!{sys.executable}\n" + CURL_FIXTURE)
        self.shim("uname", '#!/bin/sh\ncase "$1" in -s) printf "%s\\n" "$DOWNLOAD_SYSTEM";; -m) printf "%s\\n" "$DOWNLOAD_MACHINE";; *) exit 2;; esac\n')
        self.shim("sw_vers", '#!/bin/sh\nprintf "%s\\n" "$DOWNLOAD_MACOS"\n')
        self.archive("aarch64-apple-darwin")

    def shim(self, name, text):
        path = self.shims / name
        path.write_text(text)
        path.chmod(0o755)

    def register(self, archive, version, target):
        name = f"j-jump-{version}-{target}.tar.gz"
        url = f"https://github.com/{REPOSITORY}/releases/download/v{version}/{name}"
        sidecar = self.root / (name + ".sha256")
        sidecar.write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + "  " + name + "\n")
        self.files[url] = str(archive)
        self.files[url + ".sha256"] = str(sidecar)
        self.env["DOWNLOAD_FILES"] = json.dumps(self.files)
        return archive, sidecar

    def archive(self, target, version=VERSION, change=None, payload=None):
        name = f"j-jump-{version}-{target}"
        binary = payload or f"#!/bin/sh\nprintf '%s\\n' 'jjump {version}'\n".encode()
        contents = {
            "jjump": binary,
            "install.sh": (ROOT / "packaging/install.sh").read_bytes(),
            "README.md": b"Archive fixture\n",
            "LICENSE.md": b"MIT\n",
            "binary.sha256": (hashlib.sha256(binary).hexdigest() + "\n").encode(),
            "manifest.json": json.dumps(dict(version=version, target=target)).encode(),
            "dependencies.json": b"[]\n",
            "licenses/fixture-1.0.0+build/LICENSE": b"MIT\n",
        }
        members = []
        for path in (name, name + "/licenses", name + "/licenses/fixture-1.0.0+build"):
            member = tarfile.TarInfo(path)
            member.type = tarfile.DIRTYPE
            member.mode = 0o755
            members.append((member, None))
        for path, data in contents.items():
            member = tarfile.TarInfo(name + "/" + path)
            member.size = len(data)
            member.mode = 0o755 if path in ("jjump", "install.sh") else 0o644
            members.append((member, data))
        alias = tarfile.TarInfo(name + "/j-jump")
        alias.type = tarfile.SYMTYPE
        alias.linkname = "jjump"
        members.append((alias, None))
        if change:
            change(members, name)
        archive = self.root / (name + ".tar.gz")
        with tarfile.open(archive, "w:gz") as tar:
            for member, data in members:
                tar.addfile(member, None if data is None else io.BytesIO(data))
        return self.register(archive, version, target)

    def run_installer(self, *arguments, latest=False, default_prefix=False, env=None):
        args = ["sh", str(ROOT / "install.sh"), "--repository", REPOSITORY]
        if not default_prefix:
            args.extend(["--prefix", str(self.prefix)])
        if not latest:
            args.extend(["--version", VERSION])
        args.extend(arguments)
        return subprocess.run(args, env=env or self.env, capture_output=True, text=True, timeout=15)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def clean(self):
        self.assertEqual([], list(self.tmp.iterdir()))

    def refused(self, result, diagnostic):
        self.assertNotEqual(0, result.returncode, result.stdout + result.stderr)
        self.assertIn(diagnostic, result.stderr)
        self.assertFalse(self.prefix.exists())
        self.clean()

    def test_four_targets_and_machine_synonyms(self):
        cases = [("Darwin", "arm64", "aarch64-apple-darwin"),
                 ("Darwin", "x86_64", "x86_64-apple-darwin"),
                 ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
                 ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
                 ("Linux", "arm64", "aarch64-unknown-linux-musl"),
                 ("Linux", "amd64", "x86_64-unknown-linux-musl")]
        for system, machine, target in cases:
            with self.subTest(system=system, machine=machine):
                self.archive(target)
                self.env.update(DOWNLOAD_SYSTEM=system, DOWNLOAD_MACHINE=machine)
                result = self.run_installer()
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn(target, result.stdout)
                self.assertIn(f"j-jump-{VERSION}-{target}.tar.gz", self.calls()[-1][-1])
                self.assertEqual("jjump", os.readlink(self.prefix / "bin/j-jump"))
                self.clean()
                shutil.rmtree(self.prefix)

    def test_latest_discovery_and_https_constraints(self):
        result = self.run_installer(latest=True)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(3, len(self.calls()))
        self.assertTrue(self.calls()[0][-1].endswith("/releases/latest"))
        for args in self.calls():
            self.assertEqual("-q", args[0])
            for flag, value in (("--proto", "=https"), ("--proto-redir", "=https"),
                                ("--connect-timeout", "10"), ("--max-time", "120"), ("--max-redirs", "5")):
                self.assertEqual(value, args[args.index(flag)+1])
            self.assertIn("--fail", args)
        self.assertEqual("1024", self.calls()[1][self.calls()[1].index("--max-filesize")+1])
        self.assertEqual("134217728", self.calls()[2][self.calls()[2].index("--max-filesize")+1])
        self.clean()

    def test_pinned_v_version_and_default_prefix(self):
        result = self.run_installer("--version", "v" + VERSION, default_prefix=True)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue((self.home / ".local/bin/jjump").is_file())
        self.assertEqual(2, len(self.calls()))
        self.clean()

    def test_shasum_on_the_default_macos_system_path(self):
        if not Path("/usr/bin/shasum").exists():
            self.skipTest("macOS system shasum is unavailable")
        env = dict(self.env, PATH=str(self.shims) + ":/usr/bin:/bin")
        result = self.run_installer(env=env)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue((self.prefix / "bin/jjump").is_file())
        self.clean()

    def test_explicit_new_version_replacement(self):
        self.assertEqual(0, self.run_installer().returncode)
        self.archive("aarch64-apple-darwin", version="0.1.0")
        upgraded = self.run_installer("--version", "0.1.0", "--replace")
        self.assertEqual(0, upgraded.returncode, upgraded.stderr)
        self.assertIn("/download/v0.1.0/j-jump-0.1.0-aarch64-apple-darwin.tar.gz", self.calls()[-1][-1])
        self.assertEqual("jjump 0.1.0", subprocess.check_output([str(self.prefix / "bin/jjump")], text=True).strip())
        self.clean()

    def test_help_without_detection_or_download(self):
        self.env["DOWNLOAD_SYSTEM"] = "Windows_NT"
        result = self.run_installer("--help")
        self.assertEqual(0, result.returncode)
        self.assertIn("Usage:", result.stdout)
        self.assertEqual([], self.calls())

    def test_invalid_arguments_before_network(self):
        for args in (("--version", "1.2"), ("--version", "01.2.3"), ("--version", "1.2.3;touch marker"),
                     ("--repository", "example/../evil"), ("--repository", "https://example/repo"),
                     ("--prefix", "relative"), ("--unknown",), ("--version",)):
            with self.subTest(args=args):
                result = self.run_installer(*args)
                self.assertNotEqual(0, result.returncode)
                self.assertEqual([], self.calls())
                self.assertFalse(self.prefix.exists())
                self.clean()

    def test_unsupported_system_cpu_and_old_macos(self):
        for changes, message in ((dict(DOWNLOAD_SYSTEM="Windows_NT"), "Unsupported operating system"),
                                 (dict(DOWNLOAD_MACHINE="riscv64"), "Unsupported CPU"),
                                 (dict(DOWNLOAD_MACHINE="i686"), "Unsupported CPU"),
                                 (dict(DOWNLOAD_MACOS="14.7"), "macOS 15"),
                                 (dict(DOWNLOAD_MACOS="unknown"), "Cannot parse")):
            with self.subTest(changes=changes):
                self.refused(self.run_installer(env=dict(self.env, **changes)), message)
                self.assertEqual([], self.calls())

    def test_invalid_latest_redirects(self):
        for redirect in ("https://example.com/releases/tag/v" + VERSION,
                         f"https://github.com/other/repo/releases/tag/v{VERSION}",
                         f"https://github.com/{REPOSITORY}/releases/latest",
                         f"https://github.com/{REPOSITORY}/releases/tag/v1.2.3-beta"):
            with self.subTest(redirect=redirect):
                self.refused(self.run_installer(latest=True, env=dict(self.env, DOWNLOAD_LATEST=redirect)), "Latest release")

    def test_missing_release_and_download_errors(self):
        for kind in ("latest", "sidecar", "archive"):
            with self.subTest(kind=kind):
                self.refused(self.run_installer(latest=kind == "latest", env=dict(self.env, DOWNLOAD_FAILURE=kind)), "Cannot")

    def test_checksum_mismatch(self):
        archive, _ = self.archive("aarch64-apple-darwin")
        with archive.open("ab") as output:
            output.write(b"changed")
        self.refused(self.run_installer(), "SHA256 mismatch")

    def test_malformed_checksum_sidecars(self):
        _, sidecar = self.archive("aarch64-apple-darwin")
        original = sidecar.read_text()
        for content in (original.replace("  j-jump-", "  ../j-jump-"), original + original,
                        "x" * 64 + original[64:], original.upper(), "", "a" * 1025):
            with self.subTest(content=content[:30]):
                sidecar.write_text(content)
                self.assertNotEqual(0, self.run_installer().returncode)
                self.assertFalse(self.prefix.exists())
                self.clean()

    def test_unsafe_archive_members_and_links(self):
        def extra(path, kind=tarfile.REGTYPE, link=""):
            def change(members, root):
                member = tarfile.TarInfo(path.replace("ROOT", root))
                member.type = kind
                member.linkname = link
                members.append((member, None))
            return change
        def duplicate(members, root):
            members.append(members[3])
        def remove(members, root):
            members[:] = [item for item in members if not item[0].name.endswith("/install.sh")]
        def alias(members, root):
            members[-1][0].linkname = "/tmp/foreign"
        attacks = [extra("../escape"), extra("/absolute"), extra("ROOT/licenses/../escape"),
                   extra("ROOT/unexpected"), extra("ROOT/licenses/redirect", tarfile.SYMTYPE, "/tmp"),
                   extra("ROOT/licenses/hardlink", tarfile.LNKTYPE, "ROOT/jjump"),
                   extra("ROOT/licenses/pipe", tarfile.FIFOTYPE), duplicate, remove, alias]
        for change in attacks:
            with self.subTest(change=change):
                self.archive("aarch64-apple-darwin", change=change)
                result = self.run_installer()
                self.assertNotEqual(0, result.returncode, result.stdout)
                self.assertFalse(self.prefix.exists())
                self.assertFalse((self.root / "escape").exists())
                self.clean()

    def test_invalid_binary_hash_delegated_to_archive_installer(self):
        def change(members, root):
            for index, (member, data) in enumerate(members):
                if member.name.endswith("/binary.sha256"):
                    members[index] = (member, b"0" * 64 + b"\n")
        self.archive("aarch64-apple-darwin", change=change)
        self.refused(self.run_installer(), "Package checksum mismatch")

    def test_install_repeat_and_explicit_replacement_preserve_state(self):
        state = self.home / ".j-jump/visits.db"
        state.parent.mkdir()
        state.write_bytes(b"private visit fixture")
        shellrc = self.home / ".zshrc"
        shellrc.write_text("# user startup fixture\n")
        first = self.run_installer()
        self.assertEqual(0, first.returncode, first.stderr)
        old = (self.prefix / "bin/jjump").read_bytes()
        repeat = self.run_installer()
        self.assertEqual(0, repeat.returncode, repeat.stderr)
        self.assertIn("Same archive already installed", repeat.stdout)
        self.archive("aarch64-apple-darwin", payload=b"#!/bin/sh\necho replacement-fixture\n")
        refused = self.run_installer()
        self.assertNotEqual(0, refused.returncode)
        self.assertIn("--replace", refused.stderr)
        self.assertEqual(old, (self.prefix / "bin/jjump").read_bytes())
        replaced = self.run_installer("--replace")
        self.assertEqual(0, replaced.returncode, replaced.stderr)
        self.assertNotEqual(old, (self.prefix / "bin/jjump").read_bytes())
        self.assertEqual(b"private visit fixture", state.read_bytes())
        self.assertEqual("# user startup fixture\n", shellrc.read_text())
        self.assertFalse((self.prefix / "bin/jjump.previous").exists())
        self.clean()

    def test_foreign_and_modified_executables_are_retained(self):
        (self.prefix / "bin").mkdir(parents=True)
        binary = self.prefix / "bin/jjump"
        binary.write_bytes(b"foreign executable")
        result = self.run_installer("--replace")
        self.assertNotEqual(0, result.returncode)
        self.assertEqual(b"foreign executable", binary.read_bytes())
        binary.unlink()
        self.assertEqual(0, self.run_installer().returncode)
        binary.write_bytes(b"modified owned executable")
        result = self.run_installer("--replace")
        self.assertNotEqual(0, result.returncode)
        self.assertEqual(b"modified owned executable", binary.read_bytes())
        self.clean()

    def test_symlinked_prefix_components_are_refused_before_download(self):
        destination = self.root / "foreign"
        destination.mkdir()
        sentinel = destination / "sentinel"
        sentinel.write_bytes(b"keep")
        for component in (self.prefix, self.prefix / "bin", self.prefix / "share",
                          self.prefix / "share/j-jump-install"):
            with self.subTest(component=component):
                component.parent.mkdir(parents=True, exist_ok=True)
                component.symlink_to(destination, target_is_directory=True)
                result = self.run_installer("--replace")
                self.assertNotEqual(0, result.returncode)
                self.assertIn("symbolic link", result.stderr)
                self.assertEqual([], self.calls())
                self.assertEqual([sentinel], list(destination.iterdir()))
                component.unlink()
                if self.prefix.exists():
                    shutil.rmtree(self.prefix)
                self.clean()

    def test_interrupted_download_cleans_temporary_files(self):
        env = dict(self.env, DOWNLOAD_PAUSE="archive")
        process = subprocess.Popen(["sh", str(ROOT / "install.sh"), "--repository", REPOSITORY,
                                    "--version", VERSION, "--prefix", str(self.prefix)],
                                   env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            deadline = time.monotonic() + 5
            while not Path(env["DOWNLOAD_READY"]).exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(Path(env["DOWNLOAD_READY"]).exists())
            os.killpg(process.pid, signal.SIGTERM)
            process.communicate(timeout=5)
            self.assertNotEqual(0, process.returncode)
            self.assertFalse(self.prefix.exists())
            self.clean()
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate()

    @unittest.skipUnless(shutil.which("curl") and shutil.which("openssl"), "curl and openssl required for local TLS fixture")
    def test_real_curl_https_download_and_http_redirect_refusal(self):
        if os.environ.get("JJ_DOWNLOAD_TEST_ARCHIVE"):
            self.register(Path(os.environ["JJ_DOWNLOAD_TEST_ARCHIVE"]), VERSION, "aarch64-apple-darwin")
        cert = self.root / "fixture.crt"
        key = self.root / "fixture.key"
        configuration = self.root / "openssl.cnf"
        configuration.write_text("[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n"
                                 "[dn]\nCN=github.com\n[ext]\nsubjectAltName=DNS:github.com\n")
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                        "-config", str(configuration), "-keyout", str(key), "-out", str(cert)],
                       check=True, capture_output=True, timeout=20)
        fixture = self
        requests = []
        downgrade = [False]
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_HEAD(self):
                requests.append(self.path)
                if self.path.endswith("/latest"):
                    self.send_response(302)
                    self.send_header("Location", "http://127.0.0.1/rejected" if downgrade[0] else fixture.env["DOWNLOAD_LATEST"])
                else:
                    self.send_response(200)
                self.end_headers()

            def do_GET(self):
                requests.append(self.path)
                source = fixture.files.get("https://github.com" + self.path)
                self.send_response(200 if source else 404)
                data = Path(source).read_bytes() if source else b"missing"
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(cert, key)
        server.socket = context.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        real_curl = shutil.which("curl")
        self.shim("curl", f'#!/bin/sh\nexec {shlex.quote(real_curl)} "$@" --cacert {shlex.quote(str(cert))} '
                          f"--noproxy '*' --connect-to github.com:443:127.0.0.1:{server.server_port}\n")
        try:
            success = self.run_installer(latest=True)
            self.assertEqual(0, success.returncode, success.stderr)
            self.assertEqual(4, len(requests))
            self.assertTrue((self.prefix / "bin/jjump").is_file())
            self.clean()
            shutil.rmtree(self.prefix)
            downgrade[0] = True
            self.refused(self.run_installer(latest=True), "Cannot discover")
            self.assertEqual(5, len(requests))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    @unittest.skipUnless(os.environ.get("JJ_DOWNLOAD_TEST_ARCHIVE"), "opt-in native exact archive")
    def test_native_exact_archive_lifecycle_and_shell_init(self):
        archive = Path(os.environ["JJ_DOWNLOAD_TEST_ARCHIVE"]).resolve()
        with tarfile.open(archive) as tar:
            manifest = json.load(tar.extractfile(next(m for m in tar if m.name.endswith("/manifest.json"))))
        self.assertEqual(VERSION, manifest["version"])
        self.assertEqual("aarch64-apple-darwin", manifest["target"])
        self.assertFalse(manifest["dirty_source"])
        self.register(archive, VERSION, manifest["target"])
        env = dict(self.env, J_JUMP_HOME=str(self.home / "isolated-profile"), J_JUMP_SEMANTIC="disabled")
        first = self.run_installer(env=env)
        self.assertEqual(0, first.returncode, first.stderr)
        binary = self.prefix / "bin/jjump"
        self.assertEqual(f"jjump {VERSION}", subprocess.check_output([str(binary), "--version"], env=env, text=True).strip())
        for shell in ("bash", "zsh", "fish"):
            executable = shutil.which(shell)
            self.assertIsNotNone(executable, shell)
            command = (f'eval "$( {shlex.quote(str(binary))} init {shell})"; j --offline -- {shlex.quote(str(self.home))}; pwd'
                       if shell != "fish" else
                       f'{shlex.quote(str(binary))} init fish | source; j --offline -- {shlex.quote(str(self.home))}; pwd')
            shellenv = dict(env, PATH=str(self.prefix / "bin") + os.pathsep + env["PATH"], ZDOTDIR=str(self.home))
            shellenv.pop("BASH_ENV", None)
            flags = {"bash": ["--noprofile", "--norc"], "zsh": ["-f"], "fish": ["--no-config"]}[shell]
            check = subprocess.run([executable, *flags, "-c", command], env=shellenv, capture_output=True, text=True, timeout=15)
            self.assertEqual(0, check.returncode, check.stderr)
            self.assertEqual(self.home.resolve(), Path(check.stdout.strip()).resolve())
        self.assertEqual(0, self.run_installer(env=env).returncode)
        self.assertEqual(0, self.run_installer("--replace", env=env).returncode)
        extracted = self.root / "uninstall"
        extracted.mkdir()
        subprocess.run(["tar", "-xzf", str(archive), "-C", str(extracted)], check=True, capture_output=True)
        installer = next(extracted.glob("*/install.sh"))
        removed = subprocess.run(["sh", str(installer), "--prefix", str(self.prefix), "--uninstall"], env=env, capture_output=True, timeout=15)
        self.assertEqual(0, removed.returncode, removed.stderr)
        self.assertFalse(binary.exists())
        self.assertFalse((self.prefix / "bin/j-jump").is_symlink())
        self.clean()


if __name__ == "__main__":
    unittest.main()
