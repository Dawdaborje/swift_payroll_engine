# Data format

Employee payroll input is a **JSON array** of employee objects. The CLI and integration tests use the same shape as the files under `example_data/`.

## Employee object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `employee_id` | string | yes | Unique identifier |
| `full_name` | string | yes | Display name |
| `base_salary` | number | yes | Base pay (must be ≥ 0) |
| `allowances` | object | yes | `transport`, `housing`, `meal` (numbers, must be ≥ 0) |
| `deductions` | array | yes | Deduction rules (see below) |
| `pay_period` | string | no | e.g. `"2024-06"` |
| `results` | object | no* | Golden expected output for tests |

\* Required in `basic/`, `intermediate/`, and `advanced/` fixtures. Omitted in benchmark files that only stress calculation, not assertions.

Additional metadata fields (`tax_identification_number`, `currency`, etc.) may appear in fixtures but are **not** read by the engine today.

## Allowances

```json
"allowances": {
  "transport": 1500.0,
  "housing": 2000.0,
  "meal": 500.0
}
```

Gross salary = `base_salary` + `transport` + `housing` + `meal`.

## Deductions

Each deduction has an `engine` that selects how the amount is determined.

### Fixed (`engine: "fixed"`)

```json
{
  "id": "LOAN_001",
  "label": "Loan Repayment",
  "type": "fixed",
  "engine": "fixed",
  "amount": 800.0
}
```

### CEL (`engine: "cel"`)

[Common Expression Language](https://github.com/google/cel-spec). The variable `gross` (float) is in scope. Integer literals in expressions are normalized to floats at compile time.

```json
{
  "id": "PAYE",
  "label": "Income Tax (PAYE)",
  "type": "tax",
  "engine": "cel",
  "expression": "gross <= 10000 ? 0.0 : (gross - 10000) * 0.15"
}
```

### Rhai (`engine: "rhai"`)

[Rhai](https://rhai.rs/) script. The variable `gross` (float) is in scope.

```json
{
  "id": "SSHFC",
  "label": "Social Security (SSHFC)",
  "type": "tax",
  "engine": "rhai",
  "expression": "gross * 0.05"
}
```

| Field | `fixed` | `cel` / `rhai` |
|-------|---------|----------------|
| `id` | yes | yes |
| `label` | yes | yes |
| `engine` | `"fixed"` | `"cel"` or `"rhai"` |
| `amount` | yes | — |
| `expression` | — | yes (non-empty) |

Unknown `engine` values fail at load time with `UnknownEngine`.

## Golden results (tests)

Test fixtures include a `results` object so integration tests can assert exact amounts:

```json
"results": {
  "gross_salary": "22000.00",
  "net_salary": "18000.00",
  "deductions": [
    { "id": "PAYE", "amount": "2000.00" },
    { "id": "SSHFC", "amount": "1100.00" }
  ]
}
```

Amounts are compared as `Decimal` values after engine rounding (2 decimal places, midpoint away from zero).

## Minimal example

```json
[
  {
    "employee_id": "EMP-001",
    "full_name": "Jane Doe",
    "base_salary": 18000.0,
    "allowances": { "transport": 0.0, "housing": 0.0, "meal": 0.0 },
    "deductions": [
      {
        "id": "TAX",
        "label": "Income Tax",
        "engine": "cel",
        "expression": "gross * 0.10"
      }
    ],
    "results": {
      "gross_salary": "18000.00",
      "net_salary": "16200.00",
      "deductions": [{ "id": "TAX", "amount": "1800.00" }]
    }
  }
]
```

Run:

```bash
cargo run -p swift_payroll --bin swift_payroll --release -- --input path/to/employees.json
```
