#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
current=$(git -C "$repo_root" config --get core.hooksPath || true)

if [ -n "$current" ] && [ "$current" != ".githooks" ]; then
    printf '%s\n' "refusing to replace existing core.hooksPath: $current" >&2
    exit 1
fi

git -C "$repo_root" config core.hooksPath .githooks
chmod +x "$repo_root/.githooks/pre-push"
printf '%s\n' "installed repository hooks: .githooks"
