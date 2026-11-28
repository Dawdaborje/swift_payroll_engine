# swift_payroll_core

Payroll calculation: a salary structure of components with exact-decimal formulas, evaluated in
dependency order. Formulas are written in the expression language of
[`swift_payroll_expr`](https://crates.io/crates/swift_payroll_expr).

## Core types

- `Structure`: the salary structure (inputs, constants, components, rounding, precision, negative-net policy).
- `Component`: one payslip line, of kind `earning`, `deduction`, `employer` or `info`. Its value is a
  `formula`, a fixed `amount`, or an `input`. A deduction can have a `priority` for when net pay is held at zero.
- `InputSpec`: a declared input, with an optional default and an optional `type` (`number`, `text`, `bool`).
- `Compiled`: a validated structure. `Compiled::new` finds the calculation order and rejects a loop, an
  unknown name or a typo **before** any payroll runs. `calculation_order()` shows the order and `warnings()`
  lists inputs or constants nothing reads.
- `Input`: one person (`employee_id` and `variables`).
- `Payslip`: the result, with `lines`, `gross`, `deductions`, `net`, `employer_cost` and `shortfall`.
  With `Detail::Explained`, each line carries its source (formula, input, or fixed amount) and the names read by
  its formula or condition.
- `RulePack`: a tax or contribution rule kept as data (a `.cel` expression plus a `.json` description with
  its own tests). `as_component` plugs it into a structure after checking it against `PackChecks` (verified,
  same currency, in force on the pay date).
- `EngineError`, `RuleError`: structured errors with a kind, the person and component at fault, and a message.

## Example

```rust
use swift_payroll_core::{Compiled, Detail, Input, Structure};

let structure: Structure = serde_json::from_str(r#"{
  "id": "demo", "version": "1", "currency": "GMD",
  "inputs": [{ "name": "base_salary", "type": "number" }],
  "components": [
    { "id": "base", "label": "Base", "kind": "earning", "input": "base_salary" },
    { "id": "pension", "label": "Pension", "kind": "deduction", "formula": "pct(total(\"earning\"), 5)" }
  ]
}"#).unwrap();
let compiled = Compiled::new(&structure).unwrap();

let input: Input = serde_json::from_str(r#"{ "employee_id": "e1", "variables": { "base_salary": "1000" } }"#).unwrap();
let slip = compiled.run(&input).unwrap();
assert_eq!(slip.net.to_string(), "950.00");

// Many people: one result per person, in order.
let results = compiled.run_all(&[input], Detail::Lines);
```

Other entry points: `run_explained`, `run_with`, `summarize` (totals only), `calculate_amounts` (raw amounts,
no strings) and `for_each` (streams people through a bounded window).

## Scope

The crate calculates **one pay period for one person, with no memory of other periods**. It does not know pay
dates or calendars, keep year-to-date figures, compute cumulative tax, re-run past periods, or ship any
country's rules. The host passes every figure a period needs as inputs and constants and keeps the payslips.
It is not legal or tax advice.

## Features

- `parallel` (default): `run_all` and `for_each` use Rayon. Turn it off for WebAssembly or single-threaded hosts.
- `exact-json` (off by default): enables `serde_json/arbitrary_precision`, so JSON numbers keep their written
  value instead of passing through `f64`. It changes `serde_json` for the whole build it is part of, which is
  why it is opt-in. Without it, numbers are exact up to 15 significant digits; decimal text (`"120000.50"`) is
  always read exactly.

See the [data format](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/docs/data-format.md) for the
JSON and CLI formats, the [design notes](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/docs/design.md)
for why it is built this way, and
[Kani verification](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/kani/README.md) for the current
proof coverage.

## License

MIT. See [LICENSE](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/LICENSE).
