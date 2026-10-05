#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
cargo build --locked --example setup-credentials-fixture
python3 -m unittest discover -s tests/product -v
python3 -m unittest discover -s research/semantic-pilot -p 'test_*.py'
python3 -m unittest discover -s research/half-life-0031 -p 'test_*.py'
python3 -m unittest discover -s tests -p 'test_*.py'
./scripts/verify-workflow.sh --local
git diff --check
