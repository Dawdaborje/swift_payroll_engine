# Data format

JSON Schemas for these formats are in [`schemas/`](../schemas): a structure, one person's input, a whole run, and
one command-line result. Editors can use them for completion and checking; the engine checks more than a schema
can (see [schemas/README.md](../schemas/README.md)).

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
fails, status 1 for a fatal file, argument or structure error, and status 0 if all results succeed. Warnings
about the structure (an input or constant that nothing reads) go to standard error and do not change the status.

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
configurations. Money can be written as a number (`120000.50`) or as decimal text (`"120000.50"`) for an input
declared `"type": "number"` (an `any` input keeps text as text, so a formula such as `salary * 2` would fail on it);
text is read exactly in every configuration, while a library host without the `exact-json` feature reads JSON numbers through
an `f64` (exact up to 15 significant digits). The engine rejects an input variable that the structure did not
declare. A required variable without a value or default is an error, not zero.

Every object in a structure, run and rule pack rejects fields it does not know, so a misspelt key is an error
rather than being ignored.

## Structure

| Field | Meaning |
|---|---|
| `$schema` | Optional. A JSON Schema reference for editors, such as `schemas/structure.schema.json`; the engine ignores it. |
| `id`, `version`, `currency` | Identify the structure; the version and currency are copied onto each payslip. |
| `rounding` | Defaults to `{ "digits": 2, "mode": "half_up" }`. `digits` can be 0 through 8. Modes: `half_up` (ties away from zero), `half_even` (ties to even), `down` (towards zero), and `up` (away from zero). |
| `precision` | `round` (default) rounds a result that needs more than 28 digits, such as `1 / 3`, far below a cent; `exact` makes such a result an error. In both modes a product or quotient of non-zero numbers that would round to zero is an error. |
| `currency` | Copied onto each payslip. For a currency without two decimal places (JPY, KRW, … with none; BHD, KWD, … with three), a rounding to another number of places is a compile warning. |
| `negative_net` | `error` (default) rejects negative net pay; `zero` holds net at 0, taking deductions in `priority` order and reporting what could not be taken as `shortfall`; `allow` returns negative net pay. |
| `inputs` | For example, `[{ "name": "base_salary", "type": "number" }, { "name": "transport", "default": 0 }]`. See below. |
| `constants` | JSON values such as tables and rates that formulas read by name: `{ "paye_table": [[288000, 0], [null, 0.2]] }`. |
| `components` | Payslip lines. See below. A structure must have at least one component. |

### Inputs

| Field | Meaning |
|---|---|
| `name` | The name formulas and `input` components use. |
| `default` | Optional. Used when a person has no value; without one the input is required. |
| `type` | Optional: `any` (default), `number` (a JSON number or decimal text such as `"120000.50"`), `text`, or `bool`. A person's value, and the default, are checked against it before anything is calculated. An `input` component may only read an input whose type is `any` or `number`. |

### Names

Input, constant, and component names share one namespace. A name starts with an ASCII lowercase letter or `_`,
then uses only ASCII lowercase letters, digits, and `_`. The reserved expression words `true`, `false`, `null`,
and `in` cannot be used as names. Names must be unique across all three groups.

## Component

| Field | Meaning |
|---|---|
| `id`, `label` | `id` is the name formulas use; `label` is shown on the payslip. |
| `kind` | `earning`, `deduction`, `employer`, or `info`. |
| exactly one of `formula`, `amount`, `input` | Calculate an expression, use a fixed amount (a JSON number, or decimal text such as `"1500.50"`), or read a declared input. The result must be numeric. |
| `when` | A boolean condition. If false, the line is zero and `applied` is false. |
| `tags` | Additional groups for `total("tag")`. The component's `kind` is always an implicit tag. A false `when` contributes zero. |
| `round` | Overrides the structure's rounding for this line, using the same `{ "digits", "mode" }` shape. |
| `allow_negative` | Allows a negative amount for earning, deduction, or employer lines. Info figures may be negative. Defaults to false. |
| `priority` | For a deduction, a whole number from 0 up: when `negative_net` is `zero`, lower numbers are taken from pay first. Deductions without a priority come after all those with one, in the order the structure lists them. It has no effect under the other policies. |

Lines are displayed in the order written in the structure; formulas are evaluated in dependency order. `total`
accepts a literal tag string and sums the rounded amounts of applied lines carrying that tag. Gross includes
earnings, deductions are totaled separately, and employer contributions are on top of pay. Info lines do not
contribute to those three totals.

### Holding net pay at zero

With `"negative_net": "zero"`, every line is first calculated in full. If the deductions come to more than gross
pay, they are taken from pay one at a time, in `priority` order, until pay is used up. A deduction that could be
taken only in part, or not at all, keeps its full `amount` on the payslip and reports the part not taken as its
`shortfall`. A negative deduction (a refund with `allow_negative`) gives money back, so all refunds count first,
whatever their priority. The payslip's `deductions` is what was actually taken, `net` is zero, `shortfall` is the
sum of the lines' shortfalls, and `gross - deductions = net` still holds. If gross pay itself is negative, no cut
of deductions can bring net to zero and the person's calculation is an error.

Because deductions are cut only after every formula has run, a structure with `zero` may not read a deduction:
no formula or condition may name a deduction or total a tag that holds one. Otherwise taxable pay would subtract
a pension contribution that was never taken (and tax would be under-withheld), or an employer match would be paid
on it. Such a structure is refused when it is compiled; work the figure out from inputs and earnings, or use
`error` and let the host decide what to take.

## Expressions

The language supports numeric and string literals, `true`/`false`/`null`, lists (`[1, 2]`), maps with string
keys (`{'a': 1}`), names, member access (`a.b`), indexing (`a[i]`), function calls, parentheses, unary `!` and
`-`, arithmetic `* / % + -`, comparisons `< <= > >= == !=`, membership `in`, boolean `&&` and `||`, conditional
expressions (`condition ? yes : no`), and `//` comments. There are no loop or assignment expressions.

`==`, `!=` and `in` between values of different kinds are errors, as `<` is: `"3" == 3` does not quietly give
`false` (which would drop a component paid by condition when an input arrives as text). `null` compares with
anything. Declaring an input's `type` catches the same mistake earlier, when the person is read.

Built-in functions:

| Function | Behavior |
|---|---|
| `min(a, b, ...)`, `max(a, b, ...)` | Select the least or greatest number; a single list is also accepted. |
| `abs(x)`, `floor(x)`, `ceil(x)`, `trunc(x)` | Absolute value and integer rounding operations. |
| `round(x[, digits])` | Round half away from zero; `digits` is an integer from 0 to 10 and defaults to 0. |
| `round_even(x[, digits])` | Round ties to even; `digits` has the same range and default as `round`. |
| `clamp(x, low, high)`, `between(x, low, high)` | Clamp to inclusive bounds, or check whether `x` is within them. `clamp` errors if `low > high`. |
| `pct(x, p)` | Return `p` percent of `x`; for example, `pct(1000, 5)` is 50. |
| `prorate(amount, part, whole)` | Return `amount * part / whole`; `whole` must be above zero and `part` from 0 to `whole` (31 days of a 30-day period is an error, not extra pay). |
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
`amount`, `applied`, `tags`, `shortfall` when it is not zero, and, with `--explain`, `explain.source` and
`explain.reads`), plus `gross`, `deductions`, `net`, `employer_cost`, and `shortfall`. Amounts are serialized as decimal strings. Each line
uses its own rounding scale; summary totals use the structure's scale (for example, `"54800.00"`).
`explain.source` is the formula, input name, or fixed-amount marker; `explain.reads` lists names read by the
line's formula or condition.

## Rule packs

A rule pack consists of a `.cel` expression and a matching `.json` description in the companion
[`swift_tax_rules`](https://github.com/borjes-tech/swift_tax_rules) repository.

| Field | Meaning |
|---|---|
| `$schema` | Optional, ignored by the engine. |
| `id` | `country.rule.year`, for example `gm.paye.2025`: lower-case letters, digits, `.`, `-` and `_`, starting with a letter. |
| `country`, `kind`, `currency` | The ISO country code, the kind of rule (`income_tax`, `social_security`, `levy`, ...), and the currency of its amounts. |
| `valid_from`, `valid_to` | Real `YYYY-MM-DD` dates; `valid_to` is optional and may not be before `valid_from`. |
| `source`, `source_files` | The law or notice it follows, and saved copies of it in the rules repository's `sources/` folder. |
| `verified` | True only once somebody has checked the figures against the source. Defaults to false. |
| `inputs` | The names the expression reads besides the constants. No duplicates, and no name that is also a constant. |
| `constants`, `rounding` | Its tables and rates, and how its result is rounded. |
| `tests` | Test cases: `name`, `inputs`, `expect` (decimal text), and optionally `origin` (`authority`, `secondary` or `calculated`) and `source_ref`. |

The expression must not be empty and may only read declared inputs and constants. `RulePack::as_component` turns
a pack into a component and the constants it needs, after `PackChecks`: an unverified pack is refused unless
explicitly allowed (for tests and demonstrations only), the pack's currency must match the structure's, and the
pack must be in force on the pay date given with `PackChecks::on`. Without a pay date every pack is refused
(`NotInForce`), so last year's table never runs unseen; `PackChecks::any_date` skips the date for tests only.

`cargo run -- check-rules <dir>` checks packs under a directory; a description with no tests is reported as a
failure, while a `.cel` file without a description is reported as pending.
