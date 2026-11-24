# Bindings

Language bindings for Swift Payroll Engine.

## Python (`bindings/swift_payroll_py`)

**Status:** placeholder only — the native extension is not implemented, and the current Python project metadata
does not define a build backend or installable module. There is no supported Python API or Python-version range.

Possible future stack and API shape:

- [PyO3](https://pyo3.rs/) + [maturin](https://github.com/PyO3/maturin)
- A function taking a structure and employee records and returning one result per employee. The Python API has not been specified.

The intended JSON input/output shape is described in [docs/data-format.md](../docs/data-format.md). The PyO3
and maturin workflow above is a proposal, not a command that works in this checkout.

## JavaScript / TypeScript

Planned via `napi-rs` / `wasm-pack`.
