# Swift Payroll Engine

A payroll calculation engine in Rust. A **salary structure** (earnings, deductions, employer contributions,
working figures) goes in with a list of people; **payslips** come out, exact to the cent, the same every
time.

* **Exact decimals.** Every amount is a decimal number; rates are read as written (`0.15` is fifteen
  hundredths). Nothing goes through binary floating point, so `0.1 + 0.2` is `0.3` and a payslip can be
  reproduced years later.
* **Formulas in a small CEL-style language** (`crates/sp_expr`): conditions, `min`/`max`/`round`,
  `pct`, `prorate`, and `bracket(x, table)` for progressive tax. No loops, no side effects.
* **Components find their own order.** A formula names other components and adds up tagged groups with
  `total("tag")`; the engine sorts them by what they need, and refuses a loop, an unknown name, or a typo
  **before** any payroll runs.
* **Strict inputs.** A missing input is an error, never zero; an unknown input is an error, never ignored.
* **Explainable.** `--explain` shows, for each line, the formula and the names it read.
* **Rule packs.** Tax and contribution rules live as data in [swift_tax_rules](../swift_tax_rules), each
  with its source, a `verified` flag and its own tests, and plug into a structure.
* **Parallel when you want it** (`parallel` feature, Rayon); off for WebAssembly.

## Try it

```bash
cargo test                                  # engine, expression language, rule packs
cargo run -- --input example_data/demo_run.json --explain
cargo run -- check-rules ../swift_tax_rules/countries
```

## Layout

```text
crates/sp_expr/       the expression language (exact decimals, CEL syntax)
crates/sp_engine/     structures, compiling, calculating, rule packs
swift_payroll_engine/ command line
example_data/         a worked run
docs/                 the data format and the design
```

## Calculation model

For each person, the components run in dependency order. Each amount is rounded by its own rule (default
two digits, half away from zero); then

```
gross      = sum of earnings
deductions = sum of deductions
net        = gross - deductions          (a negative net is an error, held at zero, or allowed: the structure says)
employer   = sum of employer contributions   (on top of pay, not in net)
```

`info` components are working figures (gross, taxable pay) shown on the payslip and added to nothing.

See [docs/data-format.md](./docs/data-format.md) and [docs/design.md](./docs/design.md).

## What changed from 0.1

0.1 ran every rule with `f64` (and rewrote whole numbers in the expression text with a regex to make CEL
accept them), let a rule see only `gross`, had no way for one deduction to use another, and did not build
(two packages shared a name). 0.2 replaces the rule engines with the exact-decimal language, so Rhai rules
are no longer accepted; the HTTP server and Python bindings of 0.1 were stubs and are gone until they are
needed.

## License

[MIT](./LICENSE)
