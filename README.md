# Swift Payroll Engine

A high-performance, framework-agnostic payroll computation engine written in Rust. It replaces slow, per-run expression evaluation with **compiled rule execution** and **parallel employee processing**, while keeping a simple JSON input/output contract that any HR system can adopt.

## Features

- **Compiled DSL rules** — CEL and Rhai expressions are parsed once at load time and cached per thread; invalid rules fail before payroll runs.
- **Parallel calculation** — employees are processed with Rayon (`par_iter`); a sequential path exists for benchmarks.
- **Decimal-safe money** — amounts use `rust_decimal` with banker's rounding to two decimal places.
- **Golden fixture tests** — `example_data/` JSON files include expected `results` for regression testing.
- **Throughput benchmarks** — Criterion suite and a `bench_once` binary scale to 1.5M+ employees without checking in huge JSON files.

## Quick start

**Requirements:** Rust 1.85+ (edition 2024), [just](https://github.com/casey/just) (optional).

```bash
# Run integration tests (basic, intermediate, advanced, stress)
cargo test -p swift_payroll_engine
# or
just run-tests

# Calculate payroll from a fixture file
cargo run -p swift_payroll_engine --bin swift_payroll_engine --release -- \
  --input example_data/basic/001_basic.json

# Benchmarks
just bench              # Criterion suite (parallel + sequential baselines)
just bench-once         # single run, default 1_500_000 employees
just bench-once 10000   # smaller smoke run
```

## Project layout

```text
swift_payroll_engine/
├── crates/
│   ├── sp_dsl/          # Rule DSL: CEL + Rhai compilation and evaluation
│   └── sp_engine/       # Gross / deduction / net calculation
├── swift_payroll_engine/       # CLI, fixture loading, integration tests, benches
├── example_data/        # Fixtures and benchmark profiles
├── bindings/            # Python bindings scaffolding (PyO3 / maturin)
└── docs/                # Documentation
```

## Calculation model

For each employee:

1. **Gross** = `base_salary` + sum of allowances
2. **Deductions** — fixed amounts, or DSL expressions evaluated with `gross` in scope
3. **Net** = gross − total deductions (rounded to 2 dp)

Deduction engines: `fixed`, `cel`, `rhai`. See [Data format](./docs/data-format.md) for the JSON schema.

## Documentation

| Topic | Location |
|-------|----------|
| Docs index | [docs/README.md](./docs/README.md) |
| Architecture, caching, integrations | [docs/technical/README.md](./docs/technical/README.md) |
| JSON input schema | [docs/data-format.md](./docs/data-format.md) |
| Benchmark data | [example_data/benchmark/README.md](./example_data/benchmark/README.md) |

## Status

| Component | Status |
|-----------|--------|
| CEL + Rhai deduction rules | Implemented |
| CLI (`--input`) | Implemented |
| Parallel `CalculationContext` | Implemented |
| HTTP API (`--serve`) | Planned (`server.rs` stub) |
| Python bindings (PyO3) | Scaffolding only |
| Odoo / ERPNext native DSL | Planned (use external adapters) |

## License

[MIT](./LICENSE)
