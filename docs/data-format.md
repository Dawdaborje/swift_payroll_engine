# Data format

## CLI modes

For a JSON file containing a structure and all employees:

```sh
cargo run -- --input run.json [--explain]
```

For a separate structure and an employee file in newline-delimited JSON (one `Input` object per line):

```sh
cargo run -- --structure structure.json --employees people.ndjson [--summary] [--explain] [--window N]
```

In streaming mode, use `--employees -` to read employee records from standard input. `--window` controls the
number of people processed as a batch and defaults to 4096. `--summary` emits totals without payslip lines; it
only applies to streaming mode. Blank employee lines are skipped. Malformed records and payroll errors are
reported as output records, and processing continues. The CLI exits with status 2 if any employee or record
fails, status 1 for a fatal file, argument or structure error, and status 0 if all results succeed.

Batch mode prints a JSON array in the input order. Streaming mode prints one JSON result per line. In either
mode, an employee error looks like:

```json
{ "employee_id": "E-001", "error": "E-001: `base_salary` is missing" }
```

A malformed line in streaming mode is reported with its one-based line number:

```json
{ "line": 4, "error": "..." }
```

## A run

```json
{
  "structure": {
    "id": "demo", "version": "1", "currency": "GMD",
    "inputs": [{ "name": "base_salary" }],
    "components": [
      { "id": "base", "label": "Base salary", "kind": "earning", "input": "base_salary" }
    ]
  },
  "employees": [{ "employee_id": "E-001", "variables": { "base_salary": 50000 } }]
}
```

`employees` is a list of inputs. JSON numbers are read exactly as written in the default engine and CLI
configurations; write money as numbers (for example, `120000.50`), not strings. The engine rejects an input
variable that the structure did not declare. A required variable without a value or default is an error, not
zero.

## Structure

| Field | Meaning |
|---|---|
| `id`, `version`, `currency` | Identify the structure; the version and currency are copied onto each payslip. |
| `rounding` | Defaults to `{ "digits": 2, "mode": "half_up" }`. `digits` can be 0 through 8. Modes: `half_up` (ties away from zero), `half_even` (ties to even), `down` (towards zero), and `up` (away from zero). |
| `negative_net` | `error` (default) rejects negative net pay; `zero` holds net at 0 and reports the unpaid deduction as `shortfall`; `allow` returns negative net pay. |
| `inputs` | For example, `[{ "name": "base_salary" }, { "name": "transport", "default": 0 }]`. Without a default, an input is required. |
| `constants` | JSON values such as tables and rates that formulas read by name: `{ "paye_table": [[288000, 0], [null, 0.2]] }`. |
| `components` | Payslip lines. See below. A structure must have at least one component. |

Input, constant, and component names share one namespace. A name starts with an ASCII lowercase letter or `_`,
then uses only ASCII lowercase letters, digits, and `_`. The reserved expression words `true`, `false`, `null`,
and `in` cannot be used as names. Names must be unique across all three groups.

## Component

| Field | Meaning |
|---|---|
| `id`, `label` | `id` is the name formulas use; `label` is shown on the payslip. |
| `kind` | `earning`, `deduction`, `employer`, or `info`. |
| exactly one of `formula`, `amount`, `input` | Calculate an expression, use a fixed numeric amount, or read a declared input. The result must be numeric. |
| `when` | A boolean condition. If false, the line is zero and `applied` is false. |
| `tags` | Additional groups for `total("tag")`. The component's `kind` is always an implicit tag. A false `when` contributes zero. |
| `round` | Overrides the structure's rounding for this line, using the same `{ "digits", "mode" }` shape. |
| `allow_negative` | Allows a negative amount for earning, deduction, or employer lines. Info figures may be negative. Defaults to false. |

Lines are displayed in the order written in the structure; formulas are evaluated in dependency order. `total`
accepts a literal tag string and sums the rounded amounts of applied lines carrying that tag. Gross includes
earnings, deductions are totaled separately, and employer contributions are on top of pay. Info lines do not
contribute to those three totals.

## Expressions

The language supports numeric and string literals, `true`/`false`/`null`, lists (`[1, 2]`), maps with string
keys (`{'a': 1}`), names, member access (`a.b`), indexing (`a[i]`), function calls, parentheses, unary `!` and
`-`, arithmetic `* / % + -`, comparisons `< <= > >= == !=`, membership `in`, boolean `&&` and `||`, conditional
expressions (`condition ? yes : no`), and `//` comments. There are no loop or assignment expressions.

Built-in functions:

| Function | Behavior |
|---|---|
| `min(a, b, ...)`, `max(a, b, ...)` | Select the least or greatest number; a single list is also accepted. |
| `abs(x)`, `floor(x)`, `ceil(x)`, `trunc(x)` | Absolute value and integer rounding operations. |
| `round(x[, digits])` | Round half away from zero; `digits` is an integer from 0 to 10 and defaults to 0. |
| `round_even(x[, digits])` | Round ties to even; `digits` has the same range and default as `round`. |
| `clamp(x, low, high)`, `between(x, low, high)` | Clamp to inclusive bounds, or check whether `x` is within them. `clamp` errors if `low > high`. |
| `pct(x, p)` | Return `p` percent of `x`; for example, `pct(1000, 5)` is 50. |
| `prorate(amount, part, whole)` | Return `amount * part / whole`; `whole` must be above zero. |
| `bracket(x, table)` | Progressive bands. See the rules below. |
| `rate_at(x, table)` | Return the rate for the band containing `x`; an upper bound belongs to its band. |
| `sum(list)` | Sum a list of numbers. Multiple number arguments are also accepted. |
| `size(x)` | Count list or map entries, or Unicode characters in a string. |
| `coalesce(a, b, ...)` | Return the first non-`null` value, or `null` if there is none. |
| `dec(text)` | Parse a numeric string as a decimal; numeric values pass through unchanged. |
| `string(x)` | Convert a value to text. |

`bracket(x, [[upper, rate], ...])` taxes progressively from zero. Upper limits must strictly increase, rates
cannot be negative, and only the last upper limit may be `null` (no limit). Rates are fractions, so `0.15` is
15%. An amount above the last finite band is an error. Amounts at or below zero produce zero tax.

In a salary structure, `total("tag")` is an engine-provided function. Use a literal tag string so the compiler
can order the calculation and reject unknown tags or dependency cycles before payroll runs.

## Payslip

A payslip has `employee_id`, `structure`, `structure_version`, `currency`, `lines[]` (`id`, `label`, `kind`,
`amount`, `applied`, `tags`, and, with `--explain`, `explain.source` and `explain.reads`), plus `gross`,
`deductions`, `net`, `employer_cost`, and `shortfall`. Amounts are serialized as decimal strings. Each line
uses its own rounding scale; summary totals use the structure's scale (for example, `"54800.00"`).
`explain.source` is the formula, input name, or fixed-amount marker; `explain.reads` lists names read by the
line's formula or condition.

## Rule packs

A rule pack consists of a `.cel` expression and a matching `.json` description in the companion
[`swift_tax_rules`](../../swift_tax_rules) repository. `RulePack::as_component` turns a pack into a component and
the constants it needs. `cargo run -- check-rules <dir>` checks packs under a directory; a description with no
tests is reported as a failure, while a `.cel` file without a description is reported as pending.
