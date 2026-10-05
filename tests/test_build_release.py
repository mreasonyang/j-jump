"""Release remapping must preserve arguments and hide build roots."""
import importlib.util
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import release

spec = importlib.util.spec_from_file_location("build_release", ROOT / "scripts/build-release.py")
build_release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_release)


class BuildReleaseTests(unittest.TestCase):
    def test_encoded_options_and_native_target_options_survive(self):
        home = "/" + "home" + "/alpha"
        root = Path(home) / "project with spaces"
        original = {"HOME": home, "CARGO_ENCODED_RUSTFLAGS": "-C\x1flink-arg=argument with spaces",
                    "RUSTFLAGS": "ignored by encoded precedence", "CFLAGS": "-DORIGINAL=1",
                    "CFLAGS_aarch64_unknown_linux_musl": "-mno-outline-atomics"}
        actual, roots = build_release.build_environment(root, original)
        flags = actual["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
        self.assertEqual(["-C", "link-arg=argument with spaces"], flags[:2])
        self.assertIn(f"--remap-path-prefix={root}=/build/j-jump", flags)
        self.assertIn(str(root), roots)
        self.assertEqual("-mno-outline-atomics", actual["CFLAGS_aarch64_unknown_linux_musl"])
        self.assertIn("-DORIGINAL=1", shlex.split(actual["CFLAGS"]))
        self.assertIn(f"-ffile-prefix-map={root}=/build/j-jump", shlex.split(actual["CFLAGS"]))
        self.assertNotIn("CC_SHELL_ESCAPED_FLAGS", original)

    def test_plain_options_external_roots_and_root_guard(self):
        original = {"HOME": "/fixture/home", "RUSTFLAGS": "-C opt-level=2", "CARGO_HOME": "/fixture/cache",
                    "RUSTUP_HOME": "/fixture/toolchain", "CARGO_TARGET_DIR": "/fixture/output"}
        actual, roots = build_release.build_environment(Path("/fixture/project"), original)
        self.assertEqual(["-C", "opt-level=2"], actual["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")[:2])
        for path in ("/fixture/cache", "/fixture/toolchain", "/fixture/output"):
            self.assertIn(path, roots)
        nested, nested_roots = build_release.build_environment(Path("/fixture/project"),
                                                              {"HOME": "/fixture", "CARGO_TARGET_DIR": "relative target"})
        self.assertIn("/fixture/project/relative target", nested_roots)
        self.assertEqual("--remap-path-prefix=/fixture/project/relative target=/build/target",
                         nested["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")[-1])
        with self.assertRaisesRegex(ValueError, "filesystem root"):
            build_release.build_environment(Path("/fixture/project"), {"HOME": "/"})

    def test_rust_and_native_file_macros_with_space_paths(self):
        if not shutil.which("rustc") or not shutil.which("cc"):
            self.skipTest("native Rust/C compiler unavailable")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve() / "project with spaces"
            root.mkdir()
            env, _ = build_release.build_environment(root, os.environ)
            rust = root / "fixture.rs"
            rust.write_text('fn main() { println!("{}", file!()); }\n')
            rust_binary = root / "rust-fixture"
            subprocess.run(["rustc", str(rust), "-C", "strip=symbols", "-o", str(rust_binary), *env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")],
                           check=True, capture_output=True, timeout=60)
            self.assertEqual(b"/build/j-jump/fixture.rs\n", subprocess.check_output([str(rust_binary)]))
            release.check_binary_disclosure(rust_binary.read_bytes(), [str(root)])
            native = root / "fixture.c"
            native.write_text('#include <stdio.h>\nint main(void) { puts(__FILE__); return 0; }\n')
            native_binary = root / "native-fixture"
            subprocess.run(["cc", str(native), "-o", str(native_binary), *shlex.split(env["CFLAGS"])],
                           check=True, capture_output=True, timeout=60)
            self.assertEqual(b"/build/j-jump/fixture.c\n", subprocess.check_output([str(native_binary)]))
            release.check_binary_disclosure(native_binary.read_bytes(), [str(root)])
