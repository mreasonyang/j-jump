#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

python3 "$repo_root/scripts/verify_workflow.py" "$@"
git -C "$repo_root" diff --check
