//! Payroll calculation.
//!
//! A [`Structure`] lists the *components* of a payslip (earnings, deductions, employer contributions and
//! working figures). A component is a fixed amount, a value taken from the person's inputs, or a formula in
//! the exact-decimal expression language of `sp_expr`. Formulas name each other and add up tagged groups
//! with `sum("tag")`; the engine finds the order, refuses a loop or an unknown name when the structure is
//! *compiled* (before any payroll runs), and then calculates a person at a time: each line rounded by its
//! own rule, totals exact sums of the rounded lines, nothing floating.
//!
//! ```
//! use sp_engine::{Compiled, Input, Structure};
//!
//! let structure: Structure = serde_json::from_str(r#"{
//!   "id": "demo", "version": "1", "currency": "GMD",
//!   "inputs": [{ "name": "base_salary" }],
//!   "components": [
//!     { "id": "base", "label": "Base", "kind": "earning", "input": "base_salary" },
//!     { "id": "pension", "label": "Pension", "kind": "deduction", "formula": "pct(total(\"earning\"), 5)" }
//!   ]
//! }"#).unwrap();
//! let compiled = Compiled::new(&structure).unwrap();
//! let input: Input = serde_json::from_str(r#"{ "employee_id": "e1", "variables": { "base_salary": 1000 } }"#).unwrap();
//! let slip = compiled.run(&input, false).unwrap();
//! assert_eq!(slip.net.to_string(), "950.00");
//! ```

mod compile;
mod convert;
mod error;
mod model;
mod rules;
mod run;

pub use compile::Compiled;
pub use convert::{value_from_json, value_to_json};
pub use error::EngineError;
pub use rules::{PackInfo, RulePack, RuleTest};
pub use model::{Component, InputSpec, Input, Kind, Line, Mode, NegativeNet, Payslip, Rounding, Structure};

#[cfg(test)]
mod tests;
