# Technical documentation

## Architecture

Swift Payroll Engine is a Cargo workspace with three main crates:

```text
┌─────────────────────────────────────────────────────────┐
│  swift_payroll_engine                                          │
│  CLI · fixture loading · cohort expansion · tests/bench │
└──────────────────────────┬──────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────┐
│  sp_engine                                              │
│  CalculationContext · gross / deductions / net          │
└──────────────────────────┬──────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────┐
│  sp_dsl                                                 │
│  PayrollRuleContext · CEL / Rhai compile + evaluate     │
└─────────────────────────────────────────────────────────┘
```

### Calculation flow

1. Load JSON → `RawEmployee` → `EmployeeContext` (compile DSL rules at this step).
2. `CalculationContext::calculate()` maps each employee in parallel.
3. Per employee: compute gross → evaluate each deduction → sum deductions → net.

Rounding: gross, each deduction, and net are rounded to **2 decimal places** (`MidpointAwayFromZero`).

### Library API (Rust)

```rust
use sp_engine::calculator::CalculationContext;
use sp_engine::models::emp_context::EmployeeContext;
// build EmployeeContext values (or use swift_payroll_engine::fixture helpers)

let ctx = CalculationContext { emp_contexts: employees };
let results = ctx.calculate()?;           // parallel
let results = ctx.calculate_sequential()?; // single-threaded baseline
```

For large synthetic cohorts without materializing a full `Vec<EmployeeContext>`:

```rust
CalculationContext::calculate_stream_count(n, |i| cohort.employee_at(i))?;
```

See `swift_payroll_engine::cohort::ProfileCohort` and `expand_profiles`.

## Rule engines

| Engine | `DslType` | Status | Notes |
|--------|-----------|--------|-------|
| CEL | `DslType::CEL` | **Implemented** | `gross` variable; integers auto-suffixed with `.0` |
| Rhai | `DslType::Rhai` | **Implemented** | `gross` variable in script scope |
| Fixed | — | **Implemented** | Static `amount`, no DSL |
| Odoo HRMS | `DslType::OdooHRMS` | Planned | Returns `UnsupportedDsl` |
| Frappe / ERPNext | `DslType::FrappeHRMS` | Planned | Returns `UnsupportedDsl` |

Invalid expressions fail at **load time** via `PayrollRuleContext::new` / `EmpDeduction::build_from_dsl`. Runtime evaluation errors (e.g. division by zero) surface as `EngineError::DslEvaluation`.

Input JSON schema: [Data format](../data-format.md).

## Compile cache

DSL rules are not re-parsed on every deduction evaluation.

1. **Global `RuleCache`** — deduplicates compiled rule sources by `(dsl_type, expression)` when rules are first built.
2. **Per-thread caches** — hold CEL `Program` and Rhai `AST` instances. Rhai AST is not `Sync`, so it cannot live in a global `HashMap`.

This means the first evaluation of a unique expression on a thread pays a compile cost; subsequent evaluations on that thread reuse the cached artifact.

## Parallel calculation

`CalculationContext::calculate` uses Rayon (`par_iter` over `emp_contexts`). `calculate_sequential` remains for benchmark baselines and debugging.

Streaming APIs (`calculate_stream`, `calculate_stream_count`) generate employees on the fly — used by throughput benchmarks so a 1.5M-employee run does not require a 1.5M-entry JSON file in the repo.

## Tests and fixtures

Fixtures: `example_data/{basic,intermediate,advanced}/`. Each test file is a JSON array; entries with `results` are checked against engine output.

| Test module | What it covers |
|-------------|----------------|
| `basic` | All JSON files in `example_data/basic/` |
| `intermediate` | All JSON files in `example_data/intermediate/` |
| `advanced` | All JSON files in `example_data/advanced/` + CEL/Rhai parity |
| `errors` | Invalid CEL, runtime eval failure, negative salary |
| `stress` | `cohort_500.json` (≤500 employees), streaming vs materialized |

**Policy:** integration tests stay at **≤500 employees**. Million-scale throughput is measured by Criterion / `bench_once`, not unit tests.

```bash
cargo test -p swift_payroll_engine
just run-tests
just bench
just bench-once
just bench-once 1000
```

Benchmark inputs: `example_data/benchmark/profiles.json` (tier templates with 10–100 tax rules) expanded by `swift_payroll_engine::cohort::expand_profiles` and `ProfileCohort::employee_at`. Details: [benchmark README](../../example_data/benchmark/README.md).

## CLI

```bash
cargo run -p swift_payroll_engine --bin swift_payroll_engine --release -- --input example_data/basic/001_basic.json
```

| Flag | Description |
|------|-------------|
| `-i`, `--input FILE` | JSON employee array to calculate |
| `-s`, `--serve ADDR` | Reserved for HTTP mode (not implemented; default `0.0.0.0:8080`) |

## Odoo integration

Use an **external adapter** — do not embed Odoo inside Rust.

```text
Odoo HR  --JSON-RPC/XML-RPC-->  odoo_adapter  -->  sp_engine (JSON)
Odoo payslip lines  <--  adapter  <--  CalculationResult
```

**Phase 1:** A Python Odoo module (or sidecar) maps `hr.employee` / payslip inputs to the JSON shape in `example_data/`, calls the engine (PyO3 bindings or CLI/HTTP), and writes payslip lines back.

**Phase 2 (optional):** Translate Odoo salary-rule Python expressions into CEL/Rhai at import time. Do not reimplement Odoo's Python eval in Rust.

## ERPNext / Frappe integration

Same adapter pattern: a Frappe app or REST client maps Salary Structure / Salary Slip ↔ `EmployeeContext` / `CalculationResult`. Prefer exporting formulas as CEL/Rhai rather than native Frappe Python eval.

## Python bindings (PyO3)

Planned crate: `bindings/swift_payroll_engine_py` as a `cdylib` built with [maturin](https://github.com/PyO3/maturin).

Target API:

```python
results = swift_payroll_engine.calculate(employees: list[dict]) -> list[dict]
```

- Python `>=3.9`
- Odoo/ERPNext adapters can call this in-process without HTTP

Scaffolding exists under `bindings/`; the native extension is **not implemented** yet.

## JavaScript / TypeScript

**Short-term (planned):** HTTP API (`POST /v1/calculate`) in `swift_payroll_engine` (`server.rs` / `--serve`) so any JS client can post the same JSON as fixtures.

**Later:** `napi-rs` Node addon or `wasm-pack` for browser/edge, reusing the same JSON shapes.

## Error types

| Error | When |
|-------|------|
| `EmpDeductionError::UnknownEngine` | Unsupported `engine` in JSON |
| `EmpDeductionError::CompileRule` | CEL/Rhai parse failure at load |
| `EngineError::InvalidInput` | Negative salary or allowance |
| `EngineError::DslEvaluation` | Runtime DSL failure for a specific employee/rule |
