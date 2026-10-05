#!/bin/sh
set -eu
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq git zsh fish fzf python3 pkg-config musl-tools > /tmp/jj-apt.log
# Test the complete committed checkout, including package tooling and licenses.
# No user configuration or uncommitted working-tree content is copied.
git -c safe.directory=/source clone --no-hardlinks /source /build/j-jump
cd /build/j-jump
cargo test --locked --quiet
cargo build --locked --release
cargo build --locked --example setup-credentials-fixture
JJ_TEST_BIN=/build/j-jump/target/release/jjump python3 -m unittest discover -s tests/product -v
cp target/release/jjump /output/j-jump-linux-$(uname -m)

if [ "${JJ_BUILD_MUSL:-0}" = 1 ]; then
  triple=$(uname -m)-unknown-linux-musl
  rustup target add "$triple"
  case "$triple" in
    aarch64-*) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc; export CFLAGS_aarch64_unknown_linux_musl=-mno-outline-atomics;;
    x86_64-*) host=$(rustc -vV | sed -n "s/^host: //p"); export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$(rustc --print sysroot)/lib/rustlib/$host/bin/rust-lld";;
  esac
  CC=musl-gcc HOST_CC=cc cargo build --locked --release --target "$triple"
  JJ_TEST_BIN="/build/j-jump/target/$triple/release/jjump" python3 -m unittest discover -s tests/product -v
  cp "target/$triple/release/jjump" "/output/j-jump-$triple"
fi
