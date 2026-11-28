# Security policy

## Supported versions

Until 1.0, only the latest released minor version receives fixes.

| Version | Supported |
|---|---|
| 0.3.x | Yes |
| < 0.3 | No |

## Reporting a vulnerability

Please **do not open a public issue** for a security problem. Report it privately through GitHub's
[private vulnerability reporting](https://github.com/Dawdaborje/swift_payroll_engine/security/advisories/new)
for this repository.

Include what you can of:

* the affected crate and version (`swift_payroll_expr`, `swift_payroll_core` or `swift_payroll_engine`);
* a structure, expression, rule pack or input that shows the problem (the smallest you can make);
* what happens and what you expected.

You should get an acknowledgement within a week. Once a fix is ready it is released, a GitHub security advisory
is published, and the issue is reported to the [RustSec advisory database](https://rustsec.org/) when it affects
a published crate. Reporters are credited unless they prefer not to be.

## What counts

The engine computes money, so a **silently wrong amount** is treated as a security issue, not only a crash. In
scope:

* a payslip amount that differs from what the structure and the documented rules say, without an error;
* a panic, stack overflow, unbounded memory use or hang caused by an expression, structure, rule pack or input
  (the expression language is meant to fail cleanly on hostile input);
* one person's input affecting another person's payslip;
* an input, field or rule-pack check that can be bypassed (an unknown field or input accepted, an unverified or
  out-of-date pack accepted without `PackChecks` allowing it).

Out of scope:

* the figures in rule packs being wrong for a country's law (report those to
  [swift_tax_rules](https://github.com/borjes-tech/swift_tax_rules)); most packs are marked `verified: false`;
* slow compilation of a structure deliberately written to be huge. `Compiled::new` can be quadratic in the number
  of names; a host compiling structures from people it does not trust should limit their size;
* vulnerabilities in dependencies that are already in the RustSec database (CI runs `cargo audit`).
