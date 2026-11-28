# Fuzz targets

Coverage-guided fuzzing with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer, nightly Rust).
This crate is its own workspace and is never published.

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run expr -- -max_total_time=300
cargo +nightly fuzz run structure -- -max_total_time=300
cargo +nightly fuzz run rule_pack -- -max_total_time=300
```

| Target | Input | What must hold |
|---|---|---|
| `expr` | Any text, as an expression | Compiling and evaluating never panic, overflow the stack or hang; the same answer every time, in both precision modes. |
| `structure` | Any bytes, as a run (`{ "structure", "employees" }`) | Reading, compiling and calculating never panic; every payslip keeps gross = earnings, net = gross − deductions, deductions + shortfall = the deduction lines, and agrees with `summarize`. |
| `rule_pack` | A description, a NUL byte, an expression | Parsing a pack, running its tests and making it a component never panic. |

`corpus/` holds one seed per target. A crash is saved under `artifacts/<target>/`; turn it into a regular test
in the crate's `tests/` before fixing it. The seeded fuzzers in `crates/*/tests/fuzz.rs` cover the same ground
in ordinary `cargo test` runs.
