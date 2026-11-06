# example_data

| File | What it is |
|---|---|
| `demo_run.json` | A small payroll for the CLI: `swift_payroll_engine --input example_data/demo_run.json` |
| `stress.rs` | Scale harness (a cargo example): generates demo tax rules in CEL and employees, runs them, checks every answer |
| `stress_structure_10k.json` | A structure with 10,000 demo tax rules (about 10,056 components, 100 band tables) made by `stress.rs gen` |
| `stress_structure_10k_employees.ndjson` | 1,000 demo employees (2 in 1,000 are deliberately broken) |

The rules are illustrative demo figures, not any country's law.

```sh
# Stream the sample through the CLI (one payslip total per line; add --explain for full payslips):
swift_payroll_engine --structure example_data/stress_structure_10k.json \
    --employees example_data/stress_structure_10k_employees.ndjson --summary

# Regenerate the files:
cargo run --release -p swift_payroll_engine --example stress -- gen --rules 10000 --employees 1000 \
    --out example_data/stress_structure_10k.json

# One million employees x 10,000 rules, no input file needed (people are made from their index):
just stress-1m
```

## What `stress run` checks

* **Invariants** on a sample of full payslips: one line per component, totals equal the sum of their lines,
  net = gross - deductions, every amount at the structure's rounding.
* **An oracle**: each of the 10 rule templates is also written as plain Rust (`Oracle::expect`); the engine's
  amount for every component must equal it to the cent. `--oracle-every 1` checks everybody, `--full` also
  builds everybody's complete payslip.
* **Fault isolation**: about 1 in 1,000 employees is broken on purpose (missing input, text for a number, an
  unknown variable, an amount at the limit of the number type, a negative salary...). Each must fail alone.
  A person the engine refuses for negative net pay must really have a negative net by the oracle's sums.
* **Determinism**: the checksum of all net pay is the same for any `--threads`.

## Measuring one plugin batch in WebAssembly

`wasm_bench.rs` does what the aether `hr_payroll` plugin does in one `calculate_batch` call (parse the structure,
compile it, calculate people, turn their lines into JSON) so it can be run under `wasmtime`, which counts fuel the
way the aether kernel does (a call gets 50,000,000):

```sh
CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo build --release --target wasm32-wasip2 -p swift_payroll_engine --example wasm_bench
wasmtime run -W fuel=50000000 --dir . target/wasm32-wasip2/release/examples/wasm_bench.wasm \
    structure.json people.ndjson plugin 20      # modes: plugin, text, lines, compact, summary, compile, read
```

`odoo_style_baseline.py` runs the same structure and people the way Odoo's `hr_payroll` evaluates a payslip
(rules in sequence, Python code per rule against a dict, float amounts), with everything Odoo adds around it
left out, so it is a floor for Odoo, not an estimate.
