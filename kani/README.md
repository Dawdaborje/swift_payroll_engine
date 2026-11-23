# Kani verification

This workspace is Rust, so Kani can model-check selected parts of it. Kani proves assertions in
proof harnesses for all inputs admitted by the harness. It does not establish that the payroll
rules match a country's law, that a JSON rule pack is authoritative, or that an unbounded workload
has been checked.

## Install and run

On supported Linux and macOS targets, install Kani using its official installer:

```sh
cargo install --locked kani-verifier
cargo kani setup
cargo kani --workspace --default-unwind 32
```

The first two commands install Kani and its verifier components. `cargo kani --workspace` checks
the proof harnesses in workspace crates. The command sets a loop unwinding bound of 32; proofs
that need a different bound should set one on the command line and document why.

## What an accuracy proof needs

Start with a concrete contract, for example: for salaries in a specified range and a named rule
pack version, the net amount equals gross minus the legally specified deductions, rounded using the
structure's declared mode. Encode the contract independently of the implementation, bound the
symbolic inputs to the supported domain, and assert the relation in a `#[kani::proof]` harness.
Then cover boundary cases such as tax-band edges, rounding midpoints, zero values, and maximum
supported amounts. Keep separate proofs for structural safety properties (no overflow or panic)
and business properties (the resulting amounts match the contract).

Kani will report a counterexample when a property fails and a successful result only for the
property and input domain in that harness. Existing unit tests and fuzzing remain useful evidence,
but are not substitutes for those proofs. Review proof coverage whenever engine behavior or rule
semantics change.

## Current proof inventory

The selected business bound is a salary input of up to **1,000,000,000,000** (one trillion), in
the structure's currency. The engine's default output rounding is two digits; structures may request
up to eight digits. These are proof targets, not new runtime validation limits.

The current harnesses are examples and boundary checks over the production implementation. They
are not a universal proof of the payroll engine. In particular, Kani 0.64.0 was not able to finish
symbolic `rust_decimal` addition or a full `Compiled::run` payroll in a practical time in this
workspace, so the passing decimal harnesses use concrete values. The harnesses prove their exact
assertions; they do not generalize from those examples to every decimal amount.
Kani also reports unsupported dependency constructs during compilation; the passing harnesses do
not reach them, but future proofs must check whether a new path makes them reachable.

| Area | Property to prove | Current coverage |
| --- | --- | --- |
| Rounding | Every mode matches its rule for all supported amounts and digit counts; tie, sign, zero and scale boundaries are correct. | Concrete integer, half-cent, and one-trillion carry checks. |
| Expression arithmetic | Add, subtract, multiply, divide and remainder are exact within the declared decimal domain; overflow and zero divisors return errors. | No Kani proof of evaluator arithmetic yet; a concrete evaluator addition exceeded practical run time. |
| Built-in functions | `pct`, `prorate`, `bracket`, `rate_at`, `sum`, `round`, `clamp`, etc. match their specifications, including boundaries and malformed inputs. | `pct` at 1,000 and one trillion; zero-whole `prorate` rejection. |
| Expression language | Parsing precedence, literals, indexing, type errors, short-circuiting and evaluation agree with the language specification; parsing/evaluation terminate within limits. | No Kani harness yet. |
| Structure compilation | Names are unique and valid; every reference exists; dependency order respects edges; cycles and invalid tag totals are rejected. | No Kani harness yet. |
| Input handling | Exact JSON number conversion, required/default input behavior, type checking and rejection of unknown keys are correct. | No Kani harness yet. |
| Payroll totals | Rounded line amounts are counted once in the right kind/tag; gross, deductions, employer cost, net and shortfall satisfy their definitions without overflow. | Existing examples are unit tests only; full-run Kani harness did not finish. |
| Policy branches | Conditions, negative line handling and all negative-net policies behave as specified. | No Kani harness yet. |
| Output APIs | `run`, `summarize`, `calculate_amounts`, `run_all` and `for_each` agree; sequential and parallel results preserve order and independence. | No Kani harness yet. |
| Rule packs | Pack inputs and constants are constrained correctly; conversion to components preserves rule semantics and rounding. | No Kani harness yet. |
| External rule correctness | Rates, bands, dates and source citations match the applicable law. | Cannot be proved by engine code alone; requires authoritative source data and review. |

The current command completes with six harnesses verified and no failed assertions. Kani warns about
unsupported constructs in dependencies; none are reachable in these six harness paths.

The largest next step for useful universal proofs is to introduce a small arithmetic boundary that
Kani can model without expanding all of `rust_decimal` internals. Then prove that boundary against an
independent integer minor-unit specification for the one-trillion domain and separately validate
the conversion to and from `Decimal`.
