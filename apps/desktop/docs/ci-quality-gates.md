# CI Quality Gates

This document defines the deterministic baseline gates used in CI for Story 1.2.

## Mandatory checks

- `bun run check`
- `bun run test`
- `bun run test:smoke:report`
- `bun run bench:run benchmarks/latest.json benchmarks/smoke-report.json`
- `bun run bench:gate`
- `bun run build`
- `cargo build --release`
- `bun run ci:repro:prepare`

## Thresholds

- Startup p95: `< 1000ms`
- Steady-state memory: `< 50MB`
- Search p95: `< 300ms`

`bench:run` reads smoke-test timing from `benchmarks/smoke-report.json`, writes
`benchmarks/latest.json`, and `bench:gate` enforces these thresholds.

## Hardware profile assumption

- macOS Apple Silicon class
- Windows modern mid-range x86 class

## Failure diagnostics

If a gate fails:

1. Inspect workflow logs for the exact failing command.
2. Inspect `benchmarks/smoke-report.json` and `benchmarks/latest.json` in CI artifacts.
3. Re-run locally with explicit values when debugging:
   - `bun run test:smoke:report`
   - `bun run bench:run benchmarks/latest.json benchmarks/smoke-report.json`
   - `bun run bench:gate`
4. Update implementation before adjusting thresholds.
