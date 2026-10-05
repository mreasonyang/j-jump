#!/bin/sh
# Native source and exact-archive acceptance; never dispatches or publishes.
set -eu
cd "$(dirname "$0")/.."
target=${1:?pass a native release target}
tag=${2:-}
if [ -n "$tag" ]; then
    python3 scripts/release.py check-source --tag "$tag"
else
    python3 scripts/release.py check-source
fi
./scripts/install-hooks.sh
./scripts/test-product.sh
case "$target" in
    aarch64-unknown-linux-musl)
        export CC=musl-gcc HOST_CC=cc
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc
        export CFLAGS_aarch64_unknown_linux_musl=-mno-outline-atomics
        ;;
    x86_64-unknown-linux-musl)
        export CC=musl-gcc HOST_CC=cc
        host=$(rustc -vV | sed -n 's/^host: //p')
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$(rustc --print sysroot)/lib/rustlib/$host/bin/rust-lld"
        ;;
    aarch64-apple-darwin|x86_64-apple-darwin)
        export MACOSX_DEPLOYMENT_TARGET=15.0
        ;;
    *) printf '%s\n' 'Unsupported native release target' >&2; exit 2;;
esac
rustup target add "$target"
python3 scripts/build-release.py --target "$target"
python3 scripts/package.py --release --binary "target/$target/release/jjump" --target "$target"
version=$(cat VERSION)
source_sha=$(git rev-parse HEAD)
python3 scripts/release.py verify --dist dist --targets "$target" --version "$version" --source-sha "$source_sha"
scratch=$(mktemp -d)
package=$scratch/j-jump-$version-$target
prefix=$scratch/prefix
cleanup() {
    if [ -f "$prefix/share/j-jump-install/installed.sha256" ]; then
        "$package/install.sh" --prefix "$prefix" --uninstall
    fi
    rm -rf -- "$scratch"
}
trap cleanup EXIT HUP INT TERM
tar -xzf "dist/j-jump-$version-$target.tar.gz" -C "$scratch"
"$package/install.sh" --prefix "$prefix"
JJ_TEST_BIN="$prefix/bin/jjump" python3 -m unittest discover -s tests/product -v
JJ_DOWNLOAD_TEST_ARCHIVE="$PWD/dist/j-jump-$version-$target.tar.gz" \
    python3 -m unittest discover -s tests -p test_download_installer.py -v
"$package/install.sh" --prefix "$prefix" --replace
"$package/install.sh" --prefix "$prefix" --uninstall
[ ! -e "$prefix/bin/jjump" ] && [ ! -L "$prefix/bin/j-jump" ]
