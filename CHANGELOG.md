# Changelog

All notable changes to this project are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0, a minor version (0.x) may break the API
or the data format; each such break is listed under **Changed** or **Removed**.

The three crates (`swift_payroll_expr`, `swift_payroll_core`, `swift_payroll_engine`) share one version and are
released together.

## [Unreleased]

### Added

- Python bindings (`swift-payroll-engine` on PyPI, `import swift_payroll_engine`, PyO3 + maturin, one abi3
  wheel for Python 3.9+) and JS/TS bindings (`swift-payroll-engine` on npm, WebAssembly for Node). Both expose
  `Payroll(structure)` with `run`, `run_all` / `runAll` and `warnings`; neither is published yet.

### Changed

- `Compiled::run_all` and `for_each` refuse an `employee_id` seen earlier in the same call (an
  `EngineError::Input`), even when the first one failed, and every run refuses an empty `employee_id`. Before,
  only the CLI checked, and `--input` mode paid an empty id.

### Fixed

- Gross, deductions and employer cost are rounded before net is taken from them, so `gross - deductions = net`
  holds on every payslip. A line that kept more digits than the structure could give, for example, gross
  `100.01`, deductions `0.00` and net `100.00`.
- The CLI reports a closed output pipe (`| head`) as an error instead of panicking.
- `check-rules` no longer follows symlinked folders, which could loop forever.
- A list position above 2^32 in WebAssembly said "not negative"; it now says there is no such item.
- `bracket` and `rate_at` check their table in the same pass that reads it, not twice per call.
- The JS binding says what it expected when given `undefined` or a function, instead of a Rust unwrap message.
  The Python binding releases the GIL while it calculates.
- Under `precision: "exact"`, totals (`total(...)`, gross, deductions, employer cost, net) are exact or an
  error too; they were rounded silently past 28 digits.
- An amount too large to keep its rounding's decimal places is an error (`ComponentErrorKind::TooLarge`, or
  `EngineError::TooLarge` for a total) instead of a payslip with fewer places. New: `Rounding::checked_apply`.
- `0.00 - 0.00` and `-x` for a zero `x` in a formula are `0.00`, not `-0.00` (found by the new property tests).
- A repeated `employee_id` is caught when the copies differ only by surrounding spaces.
- An explained line lists the tags it totals, as `total("tag")`, among the names it read.
- Python: a `Decimal` input reaches the engine as a number, so formulas can use it (it was sent as text,
  and `salary * 2` failed on an `any` input).

## [0.3.0] - unreleased

The first version prepared for publication on crates.io.

### Changed (breaking)

- **Crates renamed**: `sp_expr` is now `swift_payroll_expr` and `sp_engine` is now `swift_payroll_core`. The
  command line package keeps the name `swift_payroll_engine`. Update `Cargo.toml` and every `use` path.
- `Compiled::run`, `run_all` and `for_each` no longer take an `explain: bool`. Use `run` / `run_explained` /
  `run_with(input, Detail)` for one person, and pass `Detail::Lines` or `Detail::Explained` to `run_all` and
  `for_each`.
- `EngineError` is now a structured enum: `Structure { component, message }`, `Input { employee, input, .. }`,
  `Component { employee, component, kind: ComponentErrorKind, .. }`, `NegativeNet { employee, net }` and
  `TooLarge { employee, total: Total }`, with `EngineError::employee()`. Error messages are unchanged in spirit
  but code that matched the old string variants must be updated.
- `swift_payroll_expr::Error` now has a `kind: ErrorKind` (`Syntax`, `TooDeep`, `Undefined`, `Type`, `Overflow`,
  `DivisionByZero`, `Precision`, `Arity`, `InvalidArgument`, `UnknownFunction`, `Host`). `Error::new` and
  `Error::at` take the kind as their first argument.
- `RulePack::parse`, `evaluate`, `run_tests` and `as_component` return a structured `RuleError` (with a
  `RuleErrorKind`) instead of `String`. `as_component` takes a `&PackChecks` and refuses, by default, a pack
  that is unverified, in another currency, or not in force on the pay date.
- Rule-pack parsing is stricter: the `id` must be lower-case letters, digits, `.`, `-` and `_` starting with a
  letter; `valid_from` / `valid_to` must be real dates in order; an input may not be declared twice or also be a
  constant; the expression may not be empty. All 36 packs in `swift_tax_rules` pass the new checks.
- Every structure, input and rule-pack object rejects unknown JSON fields.
- `Rounding`'s fields are private: build one with `Rounding::new(digits, mode)` (at most `Rounding::MAX_DIGITS`,
  8) and read it with `digits()` and `mode()`. A rounding with more digits cannot be read from JSON.
- `Explain` is renamed `Explanation`. Public structs and enums are `#[non_exhaustive]`; build them with their
  constructors (`Structure::new`, `Component::formula` / `fixed` / `input`, `InputSpec::required` / `optional`,
  `Input::new`).
- `exact-json` is no longer a default feature of `swift_payroll_core`, because it changes `serde_json` for the
  whole program. The command line still enables it. Library hosts that need JSON numbers above 15 significant
  digits should enable it or pass amounts as decimal text.
- A bracket table with a negative band limit is now refused.
- The minimum supported Rust version (1.85) is now declared and tested in CI.

### Added

- `precision` on a structure: `round` (default) or `exact`, which makes any arithmetic result that cannot be
  held exactly an error (`swift_payroll_expr::Precision`, `Expr::eval_with`). In both modes a non-zero product or
  quotient that would round to zero is an error instead of a silent zero.
- `priority` on a deduction, used when `negative_net` is `zero`: deductions are taken from pay in priority
  order, refunds first, and each line reports the part it could not take as `shortfall`.
- `type` on an input (`any`, `number`, `text`, `bool`), checked before anything is calculated, including the
  default.
- Fixed `amount`s and number inputs may be written as decimal text (`"1500.50"`), always read exactly.
- `Compiled::warnings()` reports inputs and constants nothing reads; the command line prints them to standard
  error. `Compiled::structure()` returns the structure it was compiled from.
- An optional `$schema` field on structures and rule packs, for editors.
- `PackChecks`, `RulePack::check` and `RuleError` / `RuleErrorKind`.
- `swift_payroll_core` re-exports `Precision` and the `swift_payroll_expr` crate, so hosts use the same version.
- Command line: `--help`, `--version`, a usage message on bad arguments, and an error record (instead of a
  silently empty object) when a payslip cannot be serialized.
- Golden payslip tests (`crates/swift_payroll_core/tests/golden/`) that fail on any change to any digit.
- More Kani harnesses (every admitted rounding applies without panicking) and a larger fuzz suite.
- CI on Linux, macOS and Windows, MSRV, feature combinations, a WebAssembly build, `cargo package`,
  `cargo audit`, the CLI demo, a small stress run against an independent oracle, and Kani.
- READMEs for each crate, `CHANGELOG.md`, `CONTRIBUTING.md` and `SECURITY.md`.
- JSON Schemas for structures, inputs, runs and command-line results (`schemas/`), checked in CI against every
  example and golden payslip.
- cargo-fuzz targets for expressions, whole runs and rule packs (`fuzz/`), run for a minute each in CI.
- A `cargo-semver-checks` CI job, which starts checking once the crates are on crates.io.

### Fixed

- A total too large for the number type is reported as its own error, `EngineError::TooLarge`, naming the total.
- Zero is always rounded to `0.00`, never `-0.00`.

### Moved

- The `stress` and `wasm_bench` programs are now cargo examples of the `swift_payroll_engine` package
  (`cargo run --release -p swift_payroll_engine --example stress`).

## [0.2.0] - 2026-10-06

### Changed

- Replaced the `f64` CEL and Rhai rule engines with `sp_expr`, an exact-decimal expression language with
  CEL-style syntax. Rhai rules are no longer accepted.
- A salary structure is a list of components (earnings, deductions, employer contributions, working figures)
  whose formulas may read each other and `total("tag")`; the engine orders them and rejects cycles, unknown names
  and typos before any payroll runs.
- Missing inputs are errors, never zero; unknown inputs are errors, never ignored.

### Added

- Rule packs (`RulePack`) and the `check-rules` command.
- Streaming NDJSON mode with `--structure`, `--employees`, `--summary` and `--window`.
- `--explain`, `calculate_amounts` and `summarize`.
- The `parallel` (Rayon) feature.

### Fixed

- The workspace builds again (two packages previously shared a name).

## 0.1.0

Initial prototype: rules evaluated with `f64` through CEL and Rhai, with only `gross` visible to a rule.

[Unreleased]: https://github.com/Dawdaborje/swift_payroll_engine/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/Dawdaborje/swift_payroll_engine/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Dawdaborje/swift_payroll_engine/releases/tag/v0.2.0
