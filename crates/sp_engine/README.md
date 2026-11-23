# sp_engine

Payroll calculation: a salary structure of components with exact-decimal formulas, evaluated in
dependency order. Formulas are written in the expression language of [`sp_expr`](../sp_expr).

## Core types

- `Structure`: the salary structure (inputs, constants, components, rounding, negative-net policy).
- `Component`: one payslip line, of kind `earning`, `deduction`, `employer` or `info`. Its value is a
  `formula`, a fixed `amount`, or an `input`.
- `Compiled`: a validated structure. `Compiled::new` finds the calculation order and rejects a loop, an
  unknown name or a typo **before** any payroll runs. `calculation_order()` shows the order.
- `Input`: one person (`employee_id` and `variables`).
- `Payslip`: the result, with `lines`, `gross`, `deductions`, `net`, `employer_cost` and `shortfall`.
  With `explain` on, each line carries its formula and the names it read.
- `RulePack`: a tax or contribution rule kept as data (a `.cel` expression plus a `.json` description with
  its own tests). `as_component` plugs it into a structure.
- `EngineError`: compile and calculation errors.

## Example

```rust
use sp_engine::{Compiled, Input, Structure};

let structure: Structure = serde_json::from_str(r#"{
  "id": "demo", "version": "1", "currency": "GMD",
  "inputs": [{ "name": "base_salary" }],
  "components": [
    { "id": "base", "label": "Base", "kind": "earning", "input": "base_salary" },
    { "id": "pension", "label": "Pension", "kind": "deduction", "formula": "pct(total(\"earning\"), 5)" }
  ]
}"#)?;
let compiled = Compiled::new(&structure)?;

let input: Input = serde_json::from_str(r#"{ "employee_id": "e1", "variables": { "base_salary": 1000 } }"#)?;
let slip = compiled.run(&input, false)?;
assert_eq!(slip.net.to_string(), "950.00");

// Many people: one result per person, in order.
let results = compiled.run_all(&[input], false);
```

## Features

- `parallel` (default): `run_all` uses Rayon. Turn it off for WebAssembly.

See [data-format.md](../../docs/data-format.md) for the JSON shapes and [design.md](../../docs/design.md)
for why it is built this way.
