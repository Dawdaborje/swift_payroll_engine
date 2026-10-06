//! A small expression language with CEL syntax and exact decimal arithmetic.
//!
//! Payroll and tax rules are written once as text and evaluated for many people. Money must never go
//! through binary floating point, so every number here is a [`rust_decimal::Decimal`], literals are read
//! exactly as written (`0.15` is fifteen hundredths), and the language has no loops and no side
//! effects: an expression ends, and the same inputs always give the same answer.
//!
//! The syntax is the part of CEL a rule needs: literals (numbers, strings, `true`/`false`/`null`, lists,
//! maps), names, `a.b` and `a[i]`, calls, unary `-` and `!`, `* / %`, `+ -`, comparisons and `in`, `&&`,
//! `||` and `c ? a : b`. `//` starts a comment. See [`builtins`] for the functions.

mod ast;
mod builtins;
mod error;
mod eval;
mod parse;
mod value;

pub use ast::Expr;
pub use error::{Error, Result};
pub use eval::{Env, MapEnv};
pub use value::Value;

/// Read an expression from text.
pub fn compile(source: &str) -> Result<Expr> {
    parse::parse(source)
}

#[cfg(test)]
mod tests;
