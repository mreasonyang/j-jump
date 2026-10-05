# Half-life default study

This directory contains a reproducible offline half-life evaluation tool. It introduces no runtime setting, data collection or store migration. The executable assumptions and thresholds are defined in `protocol.json`. Python3.9 standard library, the repository Rust toolchain and already-cached Cargo dependencies are sufficient.

## Sequence

1. Run `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s research/half-life-0031 -p 'test_*.py'`. These use small fixtures and never generate the formal holdout.
2. Freeze the implementation commit and SHA256 of protocol, evaluator, tests and production bridge before inspecting outcomes.
3. Run `PYTHONDONTWRITEBYTECODE=1 python3 research/half-life-0031/verify_production.py --build-dir /tmp/jjump-0031-bridge --out <new-parity.json>`. This builds/links the current production library offline and compares every calibration event plus numerical boundaries. The harness mirrors the recorder transition and calls the real engine functions; it does not exercise SQLite or shell hooks.
4. Write a checks JSON with `passed: true`, the evaluator and protocol SHA256 values, and an `evidence` list. Each entry has a path, SHA256 and `result: "pass"` for independently reviewed unit and production-parity evidence. Paths are relative to that checks file unless absolute. The evaluator verifies these bindings; the coordinator owns semantic review of the evidence.
5. Run `evaluate.py calibration --out <new-calibration.json>`. Outputs include all candidate/control scores, per-trace counts, event exclusions and a digest of every generated trace. Independently repeat calibration to another file and compare exact output bytes.
6. Run `evaluate.py freeze --calibration <calibration.json> --checks <checks.json> --out <new-selection.json>`. Save the frozen selection outside Git before the first holdout generation.
7. Run `evaluate.py holdout --decision <selection.json> --out <new-holdout.json>` exactly once for that frozen decision. This evaluates only the chosen challenger, incumbent and fixed controls, and applies the prespecified gates. An exclusive claim file records the attempt even if execution fails. Preserve it with any failure evidence; do not remove it to retune or retry silently.

## Boundaries

- `Ranker` is a study reference. `production_bridge.rs` binds H7 to the real engine; other half-lives are hypothetical policies.
- `scored`/`exclusion` are generated before any ranker runs. Never set a cold target as scored in a hand-written fixture.
- `generate_traces("holdout")` is blocked by default; the holdout command opens it only after verifying frozen decision and evidence. This is an accidental-use guard, not an adversarial access-control system.
- A positive parameter result would require a separate product claim for a forward migration. Aggregated seven-day history cannot be losslessly reconstructed at a new half-life.

## Current production bridge

The source-linking bridge uses mandatory weights and the current implementation ref (`--runtime-ref`, default HEAD). Each run writes a new report with source identities; it does not certify SQLite, shell behavior or genuine-user quality.
