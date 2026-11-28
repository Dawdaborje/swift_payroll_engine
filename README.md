# Swift Payroll Engine

A payroll calculation engine in Rust. A **salary structure** (earnings, deductions, employer contributions,
working figures) goes in with a list of people; **payslips** come out as reproducible decimal amounts, rounded
using the structure's settings (two digits by default).

* **Exact decimals.** Every amount is a decimal number; rates are read as written (`0.15` is fifteen
  hundredths). Nothing goes through binary floating point, so `0.1 + 0.2` is `0.3` and a payslip can be
  reproduced years later. A structure can set `"precision": "exact"` to make any result that cannot be held
  exactly an error instead of rounding it far below a cent.
* **Formulas in a small CEL-style language** ([`swift_payroll_expr`](./crates/swift_payroll_expr)): conditions,
  `min`/`max`/`round`, `pct`, `prorate`, and `bracket(x, table)` for progressive tax. No loops, no side effects.
* **Components find their own order.** A formula names other components and adds up tagged groups with
  `total("tag")`; the engine sorts them by what they need, and refuses a loop, an unknown name, or a typo
  **before** any payroll runs. Unknown JSON fields are errors too, so `"wehn"` never silently replaces `"when"`.
* **Strict inputs.** A missing input is an error, never zero; an unknown input is an error, never ignored; an
  input can declare a `type` (`number`, `text`, `bool`) that is checked before anything is calculated.
* **Pay held at zero, in a stated order.** With `"negative_net": "zero"`, deductions are taken by `priority`
  (tax before loans, loans before voluntary items) until pay runs out; what could not be taken is reported as
  each line's `shortfall` and the payslip's total `shortfall`.
* **Explainable.** `--explain` shows each line's source and the names its formula or condition read.
* **Rule packs.** Tax and contribution rules can live as data in the companion
  [swift_tax_rules](https://github.com/borjes-tech/swift_tax_rules) repository, with source information, a
  `verified` flag and test cases, and plug into a structure after checks on currency and validity dates.
* **Parallel when you want it** (`parallel` feature, Rayon); off for WebAssembly.

## Scope

The engine calculates **one pay period for one person, with no memory of other periods**. It does not:

* know pay dates, calendars or pay frequencies;
* keep year-to-date figures or compute cumulative tax (pass year-to-date amounts in as inputs);
* re-run past periods for retroactive pay or arrears (run again with corrected inputs and compare);
* handle several currencies in one structure;
* ship any country's rules (rule packs live in a separate repository, and most are marked unverified).

The host gives it every figure a period needs and keeps the payslips. **It is not legal or tax advice**: whoever
configures a structure is responsible for it matching the law that applies.

## Try it

```bash
cargo test --workspace                       # expression language, engine, golden payslips, rule packs
cargo run -- --input example_data/demo_run.json --explain
cargo run -- check-rules ../swift_tax_rules/countries   # with swift_tax_rules cloned next to this repository
```

The demo intentionally leaves E-003's required `base_salary` out. The CLI reports that employee's error
after calculating the other employees, then exits with status 2 to signal that one result failed.

Kani proof harnesses and their current coverage are documented in [kani/README.md](./kani/README.md).

## Crates

| Crate | What it is |
|---|---|
| [`swift_payroll_expr`](./crates/swift_payroll_expr) | The expression language (exact decimals, CEL syntax). |
| [`swift_payroll_core`](./crates/swift_payroll_core) | Structures, compiling, calculating, rule packs. |
| [`swift_payroll_engine`](./swift_payroll_engine) | The command line. |

```text
crates/swift_payroll_expr/     the expression language
crates/swift_payroll_core/     the payroll kernel (golden payslips in tests/golden/)
swift_payroll_engine/          command line, plus the stress and wasm_bench examples
example_data/                  a worked run and the stress data set
docs/                          the data format and the design
kani/                          formal verification notes
bindings/                      Python and JS/TS packages (swift-payroll-engine), see bindings/README.md
```

Until 0.3 the library crates were called `sp_expr` and `sp_engine`.

## Calculation model

For each person, the components run in dependency order. Each amount is rounded by its own rule (default
two digits, half away from zero); then

```
gross      = sum of earnings
deductions = sum of deductions taken from pay
net        = gross - deductions          (a negative net is an error, held at zero, or allowed: the structure says)
employer   = sum of employer contributions   (on top of pay, not in net)
shortfall  = deductions that could not be taken because net was held at zero
```

`info` components are working figures (gross, taxable pay) shown on the payslip and added to nothing.

See [docs/data-format.md](./docs/data-format.md) and [docs/design.md](./docs/design.md). Changes between
versions are listed in [CHANGELOG.md](./CHANGELOG.md).

## Contributing and security

See [CONTRIBUTING.md](./CONTRIBUTING.md). Report vulnerabilities privately as described in
[SECURITY.md](./SECURITY.md).

## License

[MIT](./LICENSE)
