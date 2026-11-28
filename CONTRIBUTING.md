# Contributing

Thank you for helping. This is a payroll engine: a change that moves any payslip by a cent is a bug unless it
is intended, documented and reviewed. Most of what follows exists to catch that.

## Getting started

You need Rust 1.85 or newer. [`just`](https://github.com/casey/just) is optional.

```sh
cargo test --workspace          # all tests, including golden payslips and a quick fuzz pass
cargo run -- --input example_data/demo_run.json --explain
```

To check rule packs, clone [swift_tax_rules](https://github.com/borjes-tech/swift_tax_rules) next to this
repository and run `cargo run -- check-rules ../swift_tax_rules/countries`.

## Before opening a pull request

CI runs all of these; running them locally saves a round trip:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features
cargo test --workspace --locked
cargo test -p swift_payroll_core --no-default-features
cargo test -p swift_payroll_core --features exact-json
cargo build -p swift_payroll_core --no-default-features --target wasm32-unknown-unknown
```

Longer checks, worth running when you touch arithmetic, rounding, ordering or the calculation loop:

```sh
just fuzz                       # property tests and fuzzers at 2,000,000 iterations
just stress                     # 100,000 employees x 10,000 rules against an independent oracle
cargo kani --workspace          # formal proofs (see kani/README.md)
```

## Rules for changes

* **Golden payslips.** `crates/swift_payroll_core/tests/golden/` holds whole payslips worked out by hand. If your
  change makes one fail, do not re-record it until you have worked out, by hand, which figure is right, and say
  so in the pull request. Add a golden case for any new calculation behaviour.
* **No floating point** in any path that touches an amount. Use `Decimal` and checked arithmetic; an overflow is
  an error, never a wrap, a cap or a panic.
* **Strictness stays.** Unknown fields, unknown inputs and missing inputs are errors. Do not add a silent default.
* **Errors over guesses.** If a structure is ambiguous, refuse it at compile time with a message that names the
  component.
* **Document the data format.** Any new JSON field goes in [docs/data-format.md](./docs/data-format.md) and in
  the `CHANGELOG.md` `[Unreleased]` section. Breaking changes are listed under **Changed (breaking)**.
* **Public API.** Public items need doc comments (`missing_docs` is on). Public structs and enums are
  `#[non_exhaustive]` so fields and variants can be added without a breaking release.
* **Formatting.** `rustfmt.toml` sets a 160-column width; let `cargo fmt` decide.

## Rule packs

Country rules do not belong in this repository; send them to
[swift_tax_rules](https://github.com/borjes-tech/swift_tax_rules). A pack is only marked `verified: true` by
someone who has checked every figure against the cited source.

## Security

Do not report vulnerabilities in public issues; see [SECURITY.md](./SECURITY.md).

## License

By contributing you agree that your contribution is licensed under the [MIT license](./LICENSE).
