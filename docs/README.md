# Documentation

## Guides

- **[Technical overview](./technical/README.md)** — workspace architecture, calculation flow, rule caching, parallel execution, testing, benchmarks, and integration patterns (Odoo, ERPNext, Python, JavaScript).
- **[Data format](./data-format.md)** — JSON schema for employee fixtures: fields, deduction engines, and golden `results`.

## Example data

Fixtures live under [`example_data/`](../example_data/):

| Directory | Purpose |
|-----------|---------|
| `basic/` | Small cohorts with mixed CEL, Rhai, and fixed deductions |
| `intermediate/` | Edge cases (mixed engines, bracket boundaries) |
| `advanced/` | Progressive tax tiers, CEL/Rhai parity checks |
| `benchmark/` | Profile templates and a 500-employee stress file |

See [benchmark README](../example_data/benchmark/README.md) for how 1M+ employees are generated in memory.

## Crate docs

- [`sp_dsl`](../crates/sp_dsl/README.md) — DSL compilation and evaluation
- [`sp_engine`](../crates/sp_engine/README.md) — payroll calculation engine
- [`bindings`](../bindings/README.md) — language bindings (Python scaffolding)

## Commands

```bash
cargo test -p swift_payroll_engine          # integration + unit tests
just run-tests                       # same, with --nocapture
just bench                           # Criterion throughput suite
just bench-once [N]                  # one-shot bench (default N = 1_500_000)
```
