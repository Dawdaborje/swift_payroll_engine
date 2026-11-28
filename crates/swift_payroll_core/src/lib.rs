//! A payroll calculation kernel.
//!
//! A [`Structure`] lists the *components* of a payslip (earnings, deductions, employer contributions and
//! working figures). A component is a fixed amount, a value taken from the person's inputs, or a formula in
//! the decimal expression language of [`swift_payroll_expr`]. Formulas name each other and add up tagged
//! groups with `total("tag")`; the engine finds the order, refuses a loop, an unknown name or a broken
//! table when the structure is *compiled* (before any payroll runs), and then calculates a person at a
//! time: each line rounded by its own rule, totals exact sums of the rounded lines, nothing floating.
//!
//! ```
//! use swift_payroll_core::{Compiled, Input, Structure};
//!
//! let structure: Structure = serde_json::from_str(r#"{
//!   "id": "demo", "version": "1", "currency": "GMD",
//!   "inputs": [{ "name": "base_salary", "type": "number" }],
//!   "components": [
//!     { "id": "base", "label": "Base", "kind": "earning", "input": "base_salary" },
//!     { "id": "pension", "label": "Pension", "kind": "deduction", "formula": "pct(total(\"earning\"), 5)" }
//!   ]
//! }"#)?;
//! let compiled = Compiled::new(&structure)?;
//! let input: Input = serde_json::from_str(r#"{ "employee_id": "e1", "variables": { "base_salary": "1000" } }"#)?;
//! let slip = compiled.run(&input)?;
//! assert_eq!(slip.net.to_string(), "950.00");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # What this crate is, and is not
//!
//! It calculates **one pay period for one person, with no memory of other periods**. It does not know pay
//! dates or calendars, keep year-to-date figures, compute cumulative tax, re-run past periods for
//! retroactive pay, or ship any country's rules. A host gives it every figure a period needs (days worked,
//! year-to-date pay, the rule packs in force on the pay date) as inputs and constants, and keeps the
//! payslips. It is not legal or tax advice: whoever configures a structure is responsible for it matching
//! the law that applies.
//!
//! # Numbers
//!
//! Amounts are [`Decimal`]s: 28 significant digits, at most 28 decimal places. With the structure's
//! `precision` at `round` (the default) a result that needs more digits, such as a division, is rounded far
//! below a cent; with `exact` it is an error. See [`swift_payroll_expr`]'s crate documentation.
//!
//! JSON numbers are read through an `f64`, so exactly up to 15 significant digits, unless the `exact-json`
//! feature is on. Decimal text (`"120000.50"`) is always read exactly where a number is expected.
//!
//! # Features
//!
//! * `parallel` (default): [`Compiled::run_all`] and [`Compiled::for_each`] use Rayon. Turn it off for
//!   WebAssembly or single-threaded hosts.
//! * `exact-json`: JSON numbers keep the exact text they were written in (`serde_json/arbitrary_precision`).
//!   It changes `serde_json` for the whole program it is built into, so it is off by default.

#![warn(missing_docs)]

mod compile;
mod convert;
mod error;
mod model;
mod rules;
mod run;

pub use compile::{Compiled, Warning};
pub use convert::{value_from_json, value_to_json};
pub use error::{ComponentErrorKind, EngineError, Total};
pub use model::{
    Calculation, Component, Detail, Explanation, Input, InputSpec, InputType, Kind, Line, Mode, NegativeNet, Payslip, Rounding, Structure, Summary,
};
pub use rules::{PackChecks, PackInfo, RuleError, RuleErrorKind, RulePack, RuleTest};
/// The number type of every amount, so a caller can add them up without converting each one to text.
pub use rust_decimal::Decimal;
/// The expression language, so a host uses the same version the engine does.
pub use swift_payroll_expr;
pub use swift_payroll_expr::Precision;

#[cfg(test)]
mod tests;
