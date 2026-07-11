# Technical Documentation

## Architecture

Swift Payroll Engine is a Rust workspace:

- `sp_dsl` — rule evaluation (CEL, Rhai; Odoo/Frappe stubs)
- `sp_engine` — gross / deduction / net calculation
- `swift_payroll` — CLI, fixture loading, integration tests

Calculation flow: load employees → gross = base + allowances → evaluate each deduction (fixed amount or DSL on `gross`) → net = gross − deductions.

## Example data and tests

Fixtures live under `example_data/{basic,intermediate,advanced}/`. Each employee entry includes a golden `results` object:

```json
"results": {
  "gross_salary": "22000.00",
  "net_salary": "18000.00",
  "deductions": [{ "id": "PAYE", "amount": "2000.00" }]
}
```

Dynamic deductions use `engine: "cel"` or `engine: "rhai"` with an `expression`. Fixed lines use `engine: "fixed"` with `amount`.

**Tests stay ≤500 employees.** Throughput at 1M is a Criterion bench, not a unit test.

```bash
cargo test -p swift_payroll
just bench        # Criterion (parallel + sequential baselines; includes 1M)
just bench-once           # default 1_500_000
just bench-once 1000      # scale up from here
```

Benchmark inputs: `example_data/benchmark/profiles.json` (tier templates with 10–100 tax rules) expanded in-memory by `swift_payroll::cohort::expand_profiles`. See that folder’s README for why 1M JSON is not checked in.
## Parallel calculation

`CalculationContext::calculate` uses Rayon (`par_iter` over employees). `calculate_sequential` remains for bench baselines.

## Compile cache

DSL rules are **not** re-parsed on every deduction eval:

- **Global `RuleCache`** deduplicates rule sources by `(dsl_type, expression)` when employees are loaded.
- **Per-thread caches** hold compiled CEL `Program` and Rhai `AST` (Rhai AST is not `Sync`, so it cannot live in a global `HashMap`).

Invalid expressions fail at **load time** via `PayrollRuleContext::new` / `EmpDeduction::build_from_dsl`.

```bash
just bench-once 10000
```
## Odoo integration

Prefer an **external adapter**, not embedding Odoo inside Rust.

```text
Odoo HR  --JSON-RPC/XML-RPC-->  odoo_adapter  -->  sp_engine (JSON)
Odoo payslip lines  <--  adapter  <--  CalculationResult
```

**Phase 1:** A Python Odoo module (or sidecar) maps `hr.employee` / payslip inputs to the engine JSON shape used in `example_data/`, calls the engine (via PyO3 bindings or CLI/HTTP), and writes payslip lines back.

**Phase 2 (optional):** Translate Odoo salary-rule Python expressions into CEL/Rhai at import time. Do not reimplement Odoo’s Python eval in Rust. `DslType::OdooHRMS` currently returns `UnsupportedDsl`.

## ERPNext / Frappe integration

Same adapter pattern: a Frappe app or REST client maps Salary Structure / Salary Slip ↔ `EmployeeContext` / `CalculationResult`. Prefer exporting formulas as CEL/Rhai. `DslType::FrappeHRMS` is unsupported until a translator exists.

## Python bindings (PyO3)

Planned crate under `bindings/swift_payroll_py` as a `cdylib` built with maturin:

- Expose `calculate(employees: list[dict]) -> list[dict]` wrapping `CalculationContext`
- Target Python `>=3.9`
- Odoo/ERPNext adapters can call this in-process without HTTP

Scaffolding exists under `bindings/`; the native extension is not implemented yet.

## JavaScript / TypeScript bindings

**Short-term:** HTTP API (`POST /v1/calculate`) in `swift_payroll` (`server.rs` / `--serve`) so any JS client can post the same JSON as fixtures.

**Later:** `napi-rs` Node addon or `wasm-pack` for browser/edge, reusing the same JSON shapes.
