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
* A fingerprint of the structure on each payslip: `structure_version` is whatever the author wrote, so an edit
  without a new version looks like the old one. A host that must prove which rules paid someone should store a
  hash of the structure's JSON with each run.
* Paying someone twice across runs: `run_all` and `for_each` refuse a repeated `employee_id` within one call; the
  host keeps track of who was paid in which period.
* A net pay floor above zero (protected earnings, garnishment limits such as 25% of disposable pay): write the
  limit into the deduction's formula, for example `min(order_amount, pct(disposable, 25))`, from earnings and
  inputs.
* Grossing up (the gross that leaves a wanted net): the language has no loops, so a gross-up through tax bands
  is solved by the host; a flat rate can be written directly (`net / (1 - rate)`).
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
