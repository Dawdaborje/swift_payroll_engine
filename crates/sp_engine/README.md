# sp_engine

Payroll calculation engine: gross salary, deduction evaluation, and net pay.

## Core types

- `EmployeeContext` — employee identity, base salary, allowances, deductions
- `EmpDeduction` — fixed amount or compiled DSL rule (`PayrollRuleContext`)
- `CalculationContext` — batch of employees; `calculate()` runs in parallel via Rayon
- `CalculationResult` — `employee_id`, `gross_salary`, `net_salary`, `deductions`

## Example

```rust
use sp_engine::calculator::CalculationContext;

let ctx = CalculationContext { emp_contexts: employees };
let results = ctx.calculate()?;
```

Fixture loading and JSON parsing live in the `swift_payroll` crate (`fixture` module).

See [technical docs](../../docs/technical/README.md) for the full calculation flow.
