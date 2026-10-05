"""Offline release integrity, Formula and deferred-publication controls."""
import hashlib
import importlib.util
import io
import json
import os
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import release


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


publish = load_script("publish-release")
tap_update = load_script("update-homebrew-tap")


def binary(target):
    if target.endswith("darwin"):
        data = bytearray(64)
        data[:4] = b"\xcf\xfa\xed\xfe"
        struct.pack_into("<I", data, 4, 0x100000C if target.startswith("aarch64") else 0x1000007)
        struct.pack_into("<I", data, 12, 2)
    else:
        data = bytearray(64)
        data[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<HH", data, 16, 2, 183 if target.startswith("aarch64") else 62)
    return bytes(data)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.dist = Path(self.temporary.name)

    def archive(self, target, change=None, extra=None):
        version = "0.0.27"
        name = f"j-jump-{version}-{target}"
        data = binary(target)
        sha = hashlib.sha256(data).hexdigest()
        manifest = {"schema_version": 1, "version": version, "target": target, "source_sha": "a" * 40,
                    "binary_sha256": sha, "signing": "unsigned", "dirty_source": False}
        manifest.update(change or {})
        files = {"jjump": data, "binary.sha256": (sha + "\n").encode(), "manifest.json": json.dumps(manifest).encode(),
                 "LICENSE.md": b"MIT License\n", "licenses/j-jump/LICENSE.md": b"MIT License\n",
                 "dependencies.json": json.dumps({"schema_version": 2, "target": target}).encode()}
        path = self.dist / (name + ".tar.gz")
        with tarfile.open(path, "w:gz") as archive:
            for relative, content in files.items():
                info = tarfile.TarInfo(name + "/" + relative)
                info.size = len(content)
                info.mode = 0o755 if relative == "jjump" else 0o644
                archive.addfile(info, io.BytesIO(content))
            alias = tarfile.TarInfo(name + "/j-jump")
            alias.type = tarfile.SYMTYPE
            alias.linkname = "jjump"
            archive.addfile(alias)
            if extra:
                archive.addfile(extra)
        path.with_name(path.name + ".sha256").write_text(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n")
        return path

    def bundle(self):
        for target in release.TARGETS:
            self.archive(target)
        return release.verify_bundle(self.dist)

    def test_valid_bundle_binds_version_source_targets_and_hashes(self):
        bundle = self.bundle()
        self.assertEqual("v0.0.27", bundle["tag"])
        self.assertEqual(set(release.TARGETS), {asset["target"] for asset in bundle["assets"]})
        for item in bundle["assets"]:
            self.assertEqual(item["sha256"], hashlib.sha256((self.dist / item["filename"]).read_bytes()).hexdigest())
        with self.assertRaisesRegex(ValueError, "requested source"):
            release.verify_bundle(self.dist, source_sha="b" * 40)

    def test_missing_target_and_mixed_commit_are_rejected(self):
        self.archive(release.TARGETS[0])
        with self.assertRaisesRegex(ValueError, "exactly"):
            release.verify_bundle(self.dist)
        for target in release.TARGETS[1:]:
            self.archive(target)
        self.archive(release.TARGETS[-1], {"source_sha": "b" * 40})
        with self.assertRaisesRegex(ValueError, "mixed"):
            release.verify_bundle(self.dist)

    def test_tampered_archive_and_manifest_are_rejected(self):
        path = self.archive(release.TARGETS[0])
        path.write_bytes(path.read_bytes() + b"tampered")
        with self.assertRaisesRegex(ValueError, "checksum"):
            release.verify_archive(path)
        for field, value, message in (("dirty_source", True, "clean"), ("binary_sha256", "b" * 64, "binary checksum"),
                                      ("target", release.TARGETS[1], "disagree"), ("version", "0.0.28", "disagree")):
            with self.subTest(field=field):
                path = self.archive(release.TARGETS[0], {field: value})
                with self.assertRaisesRegex(ValueError, message):
                    release.verify_archive(path)

    def test_unsafe_and_duplicate_members_are_rejected(self):
        for name, message in (("../escape", "unsafe"), ("j-jump-0.0.27-aarch64-apple-darwin/./jjump", "unsafe"),
                              ("j-jump-0.0.27-aarch64-apple-darwin/jjump", "duplicate")):
            with self.subTest(name=name):
                extra = tarfile.TarInfo(name)
                path = self.archive(release.TARGETS[0], extra=extra)
                with self.assertRaisesRegex(ValueError, message):
                    release.verify_archive(path)
        link = tarfile.TarInfo("j-jump-0.0.27-aarch64-apple-darwin/escape")
        link.type, link.linkname = tarfile.SYMTYPE, "../../outside"
        path = self.archive(release.TARGETS[0], extra=link)
        with self.assertRaisesRegex(ValueError, "symlink"):
            release.verify_archive(path)

    def test_wrong_cpu_and_dynamic_linux_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "architecture"):
            release.check_binary(binary(release.TARGETS[0]), release.TARGETS[1])
        data = bytearray(binary(release.TARGETS[2])) + bytearray(56)
        struct.pack_into("<Q", data, 32, 64)
        struct.pack_into("<HH", data, 54, 56, 1)
        struct.pack_into("<I", data, 64, 3)
        with self.assertRaisesRegex(ValueError, "interpreter"):
            release.check_binary(bytes(data), release.TARGETS[2])

    def test_binary_build_path_disclosure_and_portable_paths(self):
        for prefix in release.BUILD_PREFIXES + (b"C:\\Users\\",):
            with self.subTest(prefix=prefix), self.assertRaisesRegex(ValueError, "build-machine path"):
                release.check_binary_disclosure(binary(release.TARGETS[0]) + prefix + b"invented-owner/source.rs")
        release.check_binary_disclosure(binary(release.TARGETS[0]) + b"/build/cargo/registry/src/library.rs")
        with self.assertRaisesRegex(ValueError, "build-machine path"):
            release.check_binary_disclosure(b"/srv/fictional/source.rs", ["/srv/fictional"])

    def test_local_archive_ownership_and_extended_metadata_are_rejected(self):
        for field, value in (("uid", 501), ("gid", 20), ("uname", "invented-owner"), ("gname", "invented-group"),
                             ("pax_headers", {"SCHILY.xattr.user.fixture": "host attribute"})):
            with self.subTest(field=field):
                member = tarfile.TarInfo("j-jump-0.0.27-aarch64-apple-darwin/metadata-fixture")
                setattr(member, field, value)
                path = self.archive(release.TARGETS[0], extra=member)
                with self.assertRaisesRegex(ValueError, "ownership|extended metadata"):
                    release.verify_archive(path)

    def test_release_archive_payload_disclosure_is_rejected(self):
        for relative in ("jjump", "LICENSE.md", "dependencies.json"):
            with self.subTest(member=relative):
                path = self.archive(release.TARGETS[0])
                with tarfile.open(path) as archive:
                    entries = [(member, archive.extractfile(member).read() if member.isfile() else None)
                               for member in archive.getmembers()]
                with tarfile.open(path, "w:gz") as archive:
                    for member, content in entries:
                        if member.name.endswith("/" + relative):
                            content += release.BUILD_PREFIXES[0] + b"invented-owner/source.rs"
                            member.size = len(content)
                        archive.addfile(member, io.BytesIO(content) if content is not None else None)
                path.with_name(path.name + ".sha256").write_text(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n")
                with self.assertRaisesRegex(ValueError, "build-machine path"):
                    release.verify_archive(path)

    def test_formula_is_ruby_valid_and_has_exact_assets_and_no_runtime_toolchain(self):
        bundle = self.bundle()
        content = release.formula(bundle, "example/j-jump-releases")
        for item in bundle["assets"]:
            self.assertIn(item["sha256"], content)
            self.assertIn("/v0.0.27/" + item["filename"], content)
        self.assertIn('bin.install_symlink "jjump" => "j-jump"', content)
        self.assertIn('pkgshare.install "LICENSE.md", "licenses"', content)
        for shell in ("bash", "zsh", "fish"):
            self.assertIn("jjump init " + shell, content)
        self.assertNotIn('depends_on "rust"', content)
        self.assertNotIn("install.sh", content)
        self.assertNotIn("post_install", content)
        path = self.dist / "j-jump.rb"
        path.write_text(content)
        if shutil.which("ruby"):
            subprocess.run(["ruby", "-c", str(path)], check=True, capture_output=True)
        else:
            self.skipTest("Ruby syntax is checked separately when Ruby is available")

    def test_formula_refuses_repository_injection_and_partial_bundle(self):
        bundle = self.bundle()
        for repository in ('example/repo"; system("bad")', "../repo", "https://github.com/example/repo", "example/repo/extra"):
            with self.assertRaises(ValueError):
                release.formula(bundle, repository)
        bundle["assets"].pop()
        with self.assertRaisesRegex(ValueError, "four"):
            release.formula(bundle, "example/repo")

    def test_source_checks_dirty_tag_and_cargo_version(self):
        repo = self.dist / "source"
        repo.mkdir()
        (repo / "VERSION").write_text("0.0.27\n")
        (repo / "Cargo.toml").write_text('[package]\nversion="0.0.27"\n')
        for arguments in (("init", "-b", "main"), ("add", "."),
                          ("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "fixture"),
                          ("tag", "v0.0.27")):
            subprocess.run(["git", "-C", str(repo), *arguments], check=True, capture_output=True)
        self.assertEqual("0.0.27", release.source_identity(repo, "v0.0.27")["version"])
        with self.assertRaisesRegex(ValueError, "tag"):
            release.source_identity(repo, "v0.0.28")
        (repo / ".cargo").mkdir()
        (repo / ".cargo/config.toml").write_text('[build]\nrustc-wrapper="fixture"\n')
        with self.assertRaisesRegex(ValueError, "clean"):
            release.source_identity(repo)
        shutil.rmtree(repo / ".cargo")
        (repo / "Cargo.toml").write_text('[package]\nversion="0.0.28"\n')
        with self.assertRaisesRegex(ValueError, "disagree"):
            release.source_identity(repo)

    def test_publish_and_tap_cannot_run_without_activation(self):
        with patch.dict("os.environ", {}, clear=True), patch.object(publish.subprocess, "check_output") as network:
            with self.assertRaisesRegex(ValueError, "deferred"):
                publish.publish(self.dist, self.dist, "example/repo", "v0.0.27")
            network.assert_not_called()
        with patch.dict("os.environ", {}, clear=True), patch.object(tap_update, "git") as network:
            with self.assertRaisesRegex(ValueError, "deferred"):
                tap_update.update(self.dist, self.dist, "example/repo")
            network.assert_not_called()

    def test_missing_tap_token_stops_preflight_before_publication(self):
        workflow = json.loads((ROOT / ".github/workflows/release.yml").read_text())
        step = next(item for item in workflow["jobs"]["preflight"]["steps"] if "run" in item)
        self.assertEqual("${{ secrets.J_JUMP_DISTRIBUTION_TOKEN != '' }}", step["env"]["TAP_TOKEN_AVAILABLE"])
        env = dict(os.environ, REQUEST_PUBLISH="true", REQUEST_TAP="true", PUBLISH_ENABLED="true",
                   TAP_TOKEN_AVAILABLE="false", RELEASE_TAG="v0.0.27")
        with tempfile.TemporaryDirectory() as empty:
            result = subprocess.run(["bash", "-c", step["run"]], cwd=empty, env=env, capture_output=True, text=True)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("scoped J_JUMP_DISTRIBUTION_TOKEN secret before publication", result.stderr)
        self.assertNotIn("not a git repository", result.stderr)

    def test_private_release_and_existing_assets_cannot_be_published(self):
        identity = {"version": "0.0.27", "source_sha": "a" * 40}
        bundle = self.bundle()
        with patch.dict("os.environ", {"J_JUMP_PUBLISH_ENABLED": "true"}), patch.object(publish, "source_identity", return_value=identity), \
             patch.object(publish, "verify_bundle", return_value=bundle), patch.object(publish.subprocess, "check_output", return_value='{"private":true}'), \
             patch.object(publish.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "public release"):
                publish.publish(self.dist, self.dist, "example/repo", "v0.0.27")
            run.assert_not_called()
        with patch.dict("os.environ", {"J_JUMP_PUBLISH_ENABLED": "true"}), patch.object(publish, "source_identity", return_value=identity), \
             patch.object(publish, "verify_bundle", return_value=bundle), patch.object(publish.subprocess, "check_output", return_value='{"private":false}'), \
             patch.object(publish.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            with self.assertRaisesRegex(ValueError, "already exists"):
                publish.publish(self.dist, self.dist, "example/repo", "v0.0.27")
            self.assertEqual(1, run.call_count)

    def test_publication_rechecks_draft_before_public_visibility(self):
        bundle = self.bundle()
        identity = {"version": "0.0.27", "source_sha": "a" * 40}
        commands = []
        def run(command, **kwargs):
            commands.append(command)
            operation = command[2]
            if operation == "download":
                destination = Path(command[command.index("--dir") + 1])
                destination.mkdir()
                for path in self.dist.iterdir():
                    if path.is_file():
                        shutil.copy2(path, destination / path.name)
            return subprocess.CompletedProcess(command, 1 if operation == "view" else 0)
        with patch.dict("os.environ", {"J_JUMP_PUBLISH_ENABLED": "true"}), \
             patch.object(publish, "source_identity", return_value=identity), \
             patch.object(publish.subprocess, "check_output", return_value='{"private":false}'), \
             patch.object(publish.subprocess, "run", side_effect=run), \
             patch.object(publish, "verify_public_assets") as public:
            publish.publish(self.dist, self.dist, "example/repo", "v0.0.27")
        self.assertEqual(["view", "create", "download", "edit"], [command[2] for command in commands])
        self.assertIn("--draft", commands[1])
        self.assertIn("--draft=false", commands[-1])
        self.assertEqual(bundle, json.loads((self.dist / "release-manifest.json").read_text()))
        public.assert_called_once_with(bundle, "example/repo")

    def test_tap_rejects_wrong_repo_dirty_checkout_and_symlink(self):
        self.bundle()
        cases = ((["other"], "clean main"), (["main", " M flomo.rb"], "clean main"),
                 (["main", "", "https://github.com/example/other.git"], "approved"))
        for answers, reason in cases:
            with patch.dict("os.environ", {"J_JUMP_PUBLISH_ENABLED": "true"}), patch.object(tap_update, "git", side_effect=answers), \
                 patch.object(tap_update.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, reason):
                    tap_update.update(self.dist, self.dist, "example/repo")
                run.assert_not_called()
        (self.dist / "j-jump.rb").symlink_to(self.dist / "outside")
        with patch.dict("os.environ", {"J_JUMP_PUBLISH_ENABLED": "true"}), \
             patch.object(tap_update, "git", side_effect=["main", "", "https://github.com/mreasonyang/homebrew-taps.git"]):
            with self.assertRaisesRegex(ValueError, "regular"):
                tap_update.update(self.dist, self.dist, "example/repo")

    def test_tap_update_is_scoped_and_reads_back_without_real_network(self):
        bundle = self.bundle()
        (self.dist / "flomo.rb").write_text("existing formula\n")
        with patch.dict("os.environ", {"J_JUMP_PUBLISH_ENABLED": "true"}), \
             patch.object(tap_update, "git", side_effect=["main", "", "https://github.com/mreasonyang/homebrew-taps.git", "j-jump.rb", "b" * 40 + "\trefs/heads/main", "b" * 40]), \
             patch.object(tap_update.subprocess, "run") as run, patch.object(tap_update, "verify_public_assets") as public:
            tap_update.update(self.dist, self.dist, "example/repo")
        self.assertEqual("existing formula\n", (self.dist / "flomo.rb").read_text())
        self.assertEqual(release.formula(bundle, "example/repo"), (self.dist / "j-jump.rb").read_text())
        public.assert_called_once_with(bundle, "example/repo")
        calls = [call.args[0] for call in run.call_args_list]
        self.assertIn(["git", "-C", str(self.dist), "push", "origin", "HEAD:refs/heads/main"], calls)
        self.assertFalse((self.dist / "Formula").exists())

    def test_anonymous_asset_verification_checks_size_hash_and_no_authentication(self):
        bundle = self.bundle()
        def response(url, timeout):
            self.assertIsInstance(url, str)  # No Request object with secret headers.
            return io.BytesIO((self.dist / url.rsplit("/", 1)[1]).read_bytes())
        with patch.object(release.urllib.request, "urlopen", side_effect=response) as network:
            release.verify_public_assets(bundle, "example/repo")
            self.assertEqual(4, network.call_count)
        with patch.object(release.urllib.request, "urlopen", return_value=io.BytesIO(b"corrupt")):
            with self.assertRaisesRegex(ValueError, "checksum/size"):
                release.verify_public_assets(bundle, "example/repo")

    def test_unavailable_public_assets_stop_tap_before_any_change(self):
        self.bundle()
        with patch.dict("os.environ", {"J_JUMP_PUBLISH_ENABLED": "true"}), \
             patch.object(tap_update, "git", side_effect=["main", "", "https://github.com/mreasonyang/homebrew-taps.git"]), \
             patch.object(tap_update, "verify_public_assets", side_effect=OSError("unavailable")), \
             patch.object(tap_update.subprocess, "run") as run:
            with self.assertRaises(OSError):
                tap_update.update(self.dist, self.dist, "example/repo")
            run.assert_not_called()
            self.assertFalse((self.dist / "j-jump.rb").exists())


if __name__ == "__main__":
    unittest.main()
