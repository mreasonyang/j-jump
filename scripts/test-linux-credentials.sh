#!/bin/sh
# Explicit opt-in Linux acceptance in a disposable Secret Service session.
set -eu
cd "$(dirname "$0")/.."
project=$(pwd -P)
binary=${JJ_TEST_BIN:-$project/target/debug/jjump}
case "$binary" in /*) ;; *) binary=$project/$binary;; esac
[ "$(uname -s)" = Linux ] || { echo 'Linux Secret Service acceptance only' >&2; exit 2; }
for tool in dbus-run-session gnome-keyring-daemon secret-tool python3; do
    command -v "$tool" >/dev/null || { echo "Required test tool: $tool" >&2; exit 2; }
done
[ -x "$binary" ] || { echo 'Build jjump or set JJ_TEST_BIN first' >&2; exit 2; }
root=$(mktemp -d /tmp/jjump-credential-test.XXXXXX)
trap 'rm -rf -- "$root"' EXIT HUP INT TERM
export HOME="$root" XDG_DATA_HOME="$root/data" XDG_CONFIG_HOME="$root/config"
export XDG_CACHE_HOME="$root/cache" XDG_RUNTIME_DIR="$root/runtime"
export JJ_TEST_BIN="$binary" JJ_ISOLATED_CREDENTIAL_TEST="$root" J_JUMP_LANG=en
unset TYPESAFE_API_KEY CLOUDFLARE_AUTH_TOKEN CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID
unset J_JUMP_HOME J_JUMP_CONFIG DBUS_SESSION_BUS_ADDRESS
mkdir -m 700 "$XDG_RUNTIME_DIR"
dbus-run-session -- sh -eu -c '
    printf %s synthetic-disposable-keyring-password | gnome-keyring-daemon --unlock --components=secrets > "$XDG_RUNTIME_DIR/daemon.env"
    python3 "$1/tests/linux_credentials.py"
' sh "$project"
