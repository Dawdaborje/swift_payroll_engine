# Data format

## A run

```json
{ "structure": { ... }, "employees": [ { "employee_id": "E-001", "variables": { "base_salary": 50000 } } ] }
```

The output is a list with one payslip, or `{ "employee_id": .., "error": .. }`, per person, in the order given.
Numbers in JSON are read exactly as written; write money as numbers (`120000.50`), not as strings.

## Structure

| Field | Meaning |
|---|---|
| `id`, `version`, `currency` | Identify the structure; the version is copied onto every payslip. |
| `rounding` | `{ "digits": 2, "mode": "half_up" }`; modes `half_up`, `half_even`, `down`, `up`. |
| `negative_net` | `error` (default), `zero` (hold net at 0 and report the `shortfall`), `allow`. |
| `inputs` | `[{ "name": "base_salary" }, { "name": "transport", "default": 0 }]`. Without a default an input is required. |
| `constants` | Tables and rates formulas read by name: `{ "paye_table": [[288000, 0], [null, 0.2]] }`. |
| `components` | See below. |

Names (inputs, constants, components) share one set: lower-case letters, digits and `_`.

## Component

| Field | Meaning |
|---|---|
| `id`, `label` | The name formulas use, and what the payslip shows. |
| `kind` | `earning`, `deduction`, `employer`, `info`. |
| exactly one of `formula`, `amount`, `input` | An expression; a fixed number; or the name of an input. |
| `when` | A condition; when false the line is zero and `applied` is false. |
| `tags` | Groups for `total("tag")`. The kind is always a tag too. |
| `round` | This line's own rounding. |
| `allow_negative` | A negative result is an error unless set. |

## Expressions

Numbers, strings (`'a'`), `true`/`false`/`null`, lists `[1, 2]`, maps `{'a': 1}`, `a.b`, `a[i]`, calls,
`! - * / % + -`, `< <= > >= == != in`, `&&`, `||`, `c ? a : b`, and `//` comments.

`min max abs floor ceil trunc round(x[,digits]) round_even clamp between pct(x,p) prorate(a,part,whole)
bracket(x,table) rate_at(x,table) sum(list) size coalesce dec string`, and in a structure
`total("tag")`, the exact sum of the lines carrying a tag.

`bracket(x, [[upper, rate], ...])`: tax by progressive bands; the first band runs from 0 to its upper limit;
the last upper may be `null` for no limit; without it an amount above the table is an error.

## Payslip

`employee_id`, `structure`, `structure_version`, `currency`, `lines[]` (`id`, `label`, `kind`, `amount`,
`applied`, `tags`, and with `--explain` `explain.source` and `explain.reads`), `gross`, `deductions`, `net`,
`employer_cost`, `shortfall`. Amounts are text with the structure's digits (`"54800.00"`).

## Rule packs

A `.cel` expression and a `.json` description side by side; see `../swift_tax_rules`.
`RulePack::as_component` turns one into a component and the constants it needs.
