# Design

## Why exact decimals and an expression language of our own

0.1 evaluated rules with an off-the-shelf CEL and Rhai, both of which compute with `f64`. Payroll must give
the same cent today and in five years, on any machine, and an auditor must be able to recompute it by hand.
Neither engine has a decimal type; wrapping one meant converting at every edge and rewriting numbers in the
rule text. `sp_expr` is about 700 lines, has the CEL syntax a rule needs, reads literals exactly, and gives
the engine one place that decides what a number is.

## Why components, not a gross and a list of deductions

A real structure has working figures (taxable pay is gross less pre-tax deductions), taxes that depend on
them, contributions that depend on a base, and conditions (a loan only while it runs). Odoo models this as
rule categories and Python rules ordered by `sequence`; ERPNext as earning and deduction rows whose formulas
may name other rows. Both depend on the order the rows are written in, and a wrong order silently gives a
wrong figure. Here the order is computed from what each component names, so it cannot be wrong, and a loop
or a typo stops the structure from loading.

## Things deliberately not done (yet)

* Year-to-date and cumulative tax: the caller passes year-to-date figures as inputs.
* Proration by days: use `prorate(amount, days_worked, days_in_period)` with inputs from attendance and leave.
* Several currencies in one structure.
* Retroactive recalculation and arrears: a new run with corrected inputs; comparing runs is the caller's job.
* Rhai (or any other `f64` language).
* A service wrapper and Python bindings.

## Testing

`cargo test` runs the language, the engine, and the rule packs in `example_data/`. Every rule pack carries
tests, and `check-rules` runs them for a whole folder, so a rule repository can be checked in CI.
