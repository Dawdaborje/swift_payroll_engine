# swift_payroll_engine

The command line for the Swift payroll engine: a salary structure and a list of people go in, payslips come
out as exact decimal amounts. The calculation itself lives in the
[`swift_payroll_core`](https://crates.io/crates/swift_payroll_core) library.

## Install

```sh
cargo install swift_payroll_engine
```

## Usage

```text
swift_payroll_engine --input <run.json> [--explain]
swift_payroll_engine --structure <structure.json> --employees <people.ndjson|-> [--summary] [--explain] [--window N]
swift_payroll_engine check-rules <dir>
```

* `--input run.json` reads one file holding `{ "structure": {...}, "employees": [...] }` and prints a JSON
  array with one payslip or error per person, in input order.
* `--structure` with `--employees` streams people from newline-delimited JSON (one person per line; `-` reads
  standard input). At most `--window` people (default 4096) are held in memory at once. Each output line is a
  payslip, only the totals with `--summary`, or an error record. A malformed line or a failing person is
  reported and processing continues.
* `--explain` adds, to every line, the formula, input or fixed amount it came from and the names it read.
* `check-rules <dir>` loads every rule pack (`.cel` expression plus `.json` description) under a directory and
  runs each pack's own tests. A pack with no tests fails; a `.cel` file without a description is reported as
  pending.

Warnings about a structure (an input or constant that nothing reads) are printed to standard error.

### Exit status

| Status | Meaning |
|---|---|
| 0 | Every person was calculated (or every rule pack passed). |
| 2 | At least one person, input line or rule pack failed; the others were still processed. |
| 1 | A fatal error: a bad argument, an unreadable file, or a structure that does not compile. |

## Example

```sh
swift_payroll_engine --input example_data/demo_run.json --explain
```

```json
[
  { "employee_id": "E-001", "lines": [ ... ], "gross": "62000.00", "deductions": "7200.00", "net": "54800.00", ... },
  { "employee_id": "E-002", ... "net": "116100.46", ... },
  { "employee_id": "E-003", "error": "E-003: `base_salary` is missing" }
]
```

The command line reads JSON numbers exactly as written (it enables the core crate's `exact-json` feature) and
calculates people in parallel.

See the [data format](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/docs/data-format.md) for the
structure, input and payslip JSON, and the
[project README](https://github.com/Dawdaborje/swift_payroll_engine#readme) for an overview.

## License

MIT. See [LICENSE](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/LICENSE).
