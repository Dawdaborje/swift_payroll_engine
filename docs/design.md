# Design

## Why exact decimals and an expression language of our own

0.1 evaluated rules with off-the-shelf CEL and Rhai engines that compute with `f64`. Payroll needs reproducible
decimal results, and an auditor should be able to recompute them by hand. Wrapping binary-float engines in a
decimal type would require conversions at their boundaries and changes to rule expressions. `swift_payroll_expr` provides
the CEL-style syntax this engine needs, reads literals exactly, and gives the engine one place to define number
and expression behavior.

## Why components, not a gross and a list of deductions

A real structure has working figures (taxable pay is gross less pre-tax deductions), taxes that depend on
them, contributions that depend on a base, and conditions (a loan only while it runs). Here the engine computes
component order from formula references and tag totals. It rejects cycles, unknown references and invalid
structures before calculating a person's pay.

## Things deliberately not done (yet)

* Year-to-date and cumulative tax: the caller passes year-to-date figures as inputs.
* Proration by days: use `prorate(amount, days_worked, days_in_period)` with inputs from attendance and leave.
* Several currencies in one structure.
* Retroactive recalculation and arrears: a new run with corrected inputs; comparing runs is the caller's job.
* Rhai (or any other `f64` language).
* Python bindings.

## Testing

* `cargo test --workspace` runs the expression and engine unit tests, the golden payslips
  (`crates/swift_payroll_core/tests/golden/`, worked out by hand and compared digit by digit), rule-pack cases,
  and a quick pass of the seeded fuzzers in each crate's `tests/fuzz.rs` (`just fuzz` runs them at scale).
* `fuzz/` holds coverage-guided cargo-fuzz targets for expressions, whole runs and rule packs.
* The `stress` example checks 100,000 or more employees against an independent plain-Rust oracle.
* `schemas/check.py` validates examples, golden payslips and CLI output against the JSON Schemas.
* `cargo run -- check-rules <dir>` scans a rule directory for `.cel` files and matching `.json` descriptions,
  then runs each described pack's test cases.
* See [Kani verification](../kani/README.md) for formal harnesses and their current coverage.
