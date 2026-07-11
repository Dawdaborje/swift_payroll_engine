# Bindings

Language bindings for Swift Payroll Engine.

## Python (`bindings/swift_payroll_py`)

**Status:** scaffolding only — native extension not implemented.

Planned stack:

- [PyO3](https://pyo3.rs/) + [maturin](https://github.com/PyO3/maturin)
- Target API: `calculate(employees: list[dict]) -> list[dict]`
- Python `>=3.9`

When implemented:

```bash
cd bindings/swift_payroll_py
maturin develop --release
```

The JSON input/output shape matches [docs/data-format.md](../docs/data-format.md).

## JavaScript / TypeScript

Planned via HTTP API (`POST /v1/calculate`) or later `napi-rs` / `wasm-pack`. See [technical docs](../docs/technical/README.md#javascript--typescript).
