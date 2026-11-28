//! A small expression language with CEL-style syntax and decimal arithmetic.
//!
//! Payroll and tax rules are written once as text and evaluated for many people. Money must never go
//! through binary floating point, so every number here is a [`rust_decimal::Decimal`], literals are read
//! exactly as written (`0.15` is fifteen hundredths), and the language has no loops and no side
//! effects: an expression ends, and the same inputs always give the same answer.
//!
//! ```
//! use std::collections::BTreeMap;
//! use swift_payroll_expr::{compile, MapEnv, Value};
//!
//! let expr = compile("bracket(pay * 12, [[288000, 0], [576000, 0.10], [null, 0.20]]) / 12")?;
//! let env = MapEnv(BTreeMap::from([("pay".to_string(), Value::from(59500))]));
//! assert_eq!(expr.eval(&env)?.to_string(), "4700.00");
//! # Ok::<(), swift_payroll_expr::Error>(())
//! ```
//!
//! # Syntax
//!
//! The syntax is the part of CEL a rule needs: literals (numbers, strings, `true`/`false`/`null`, lists,
//! maps), names, `a.b` and `a[i]`, calls, unary `-` and `!`, `* / %`, `+ -`, comparisons and `in`, `&&`,
//! `||` and `c ? a : b`. `//` starts a comment. It is not full CEL: there are no macros (`has`, `map`,
//! `all`), no separate integer and double types and no timestamps. See [`builtins`] for the functions.
//!
//! # Precision
//!
//! A number holds up to 28 significant digits and up to 28 decimal places. Addition, subtraction and
//! remainder of amounts like money are exact. A result that needs more digits, such as `1 / 3`, is
//! rounded to 28 digits with [`Precision::Round`] (the default), and is an error with
//! [`Precision::Exact`] (see [`Expr::eval_with`]). In both modes a product or quotient of non-zero numbers
//! that would round to zero is an error, never a silent zero, and a number too large for the type is an
//! error, never a wrapped or capped value.
//!
//! # Limits
//!
//! Expressions are nested at most 64 levels deep and evaluation never recurses without bound, so a hostile
//! expression can fail but cannot overflow the stack.

#![warn(missing_docs)]

mod arith;
mod ast;
pub mod builtins;
mod error;
mod eval;
mod parse;
mod value;

pub use arith::Precision;
pub use ast::Expr;
pub use error::{Error, ErrorKind, Result};
pub use eval::{Env, MapEnv};
pub use value::Value;

/// Read an expression from text.
pub fn compile(source: &str) -> Result<Expr> {
    parse::parse(source)
}

#[cfg(test)]
mod tests;
