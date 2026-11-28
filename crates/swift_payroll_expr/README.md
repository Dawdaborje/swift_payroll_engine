# swift_payroll_expr

A small expression language with CEL-style syntax and **exact decimal arithmetic**, written for payroll and
tax rules. It is the formula language of [`swift_payroll_core`](https://crates.io/crates/swift_payroll_core),
and can be used on its own.

* Every number is a [`rust_decimal::Decimal`](https://docs.rs/rust_decimal). Literals are read exactly as
  written (`0.15` is fifteen hundredths), so `0.1 + 0.2 == 0.3`. Nothing goes through binary floating point.
* No loops, no assignment, no side effects: an expression always ends, and the same inputs always give the
  same answer.
* Nesting is limited to 64 levels and evaluation never recurses without bound, so a hostile expression can
  fail but cannot overflow the stack.
* Errors carry an `ErrorKind` (syntax, undefined name, type, overflow, division by zero, precision, ...) and,
  where known, the byte offset in the source.

## Example

```rust
use std::collections::BTreeMap;
use swift_payroll_expr::{compile, MapEnv, Value};

// Progressive annual tax on a monthly salary, back to a monthly amount.
let expr = compile("bracket(pay * 12, [[288000, 0], [576000, 0.10], [null, 0.20]]) / 12").unwrap();
let env = MapEnv(BTreeMap::from([("pay".to_string(), Value::from(59500))]));
assert_eq!(expr.eval(&env).unwrap().to_string(), "4700.00");
```

A host can supply its own names and functions by implementing the `Env` trait instead of using `MapEnv`.
`Expr::identifiers()` lists the names an expression reads, which lets a host check references and order
calculations before evaluating anything.

## Syntax

The part of CEL a rule needs: numbers, strings, `true`/`false`/`null`, lists (`[1, 2]`), maps with string keys
(`{'a': 1}`), names, `a.b` and `a[i]`, function calls, unary `-` and `!`, `* / %`, `+ -`, comparisons, `in`,
`&&`, `||`, `cond ? a : b`, and `//` comments.

It is **not** full CEL: there are no macros (`has`, `map`, `all`), no separate integer and double types and no
timestamps.

## Built-in functions

`min`, `max`, `abs`, `floor`, `ceil`, `trunc`, `round`, `round_even`, `clamp`, `between`, `pct`, `prorate`,
`bracket` (progressive bands), `rate_at`, `sum`, `size`, `coalesce`, `dec` and `string`. Their exact behaviour
is documented in the [`builtins`](https://docs.rs/swift_payroll_expr/latest/swift_payroll_expr/builtins/)
module and in the engine's
[data format](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/docs/data-format.md#expressions).

## Precision

A number holds up to 28 significant digits and up to 28 decimal places. Addition, subtraction and remainder of
money-like amounts are exact. For a result that needs more digits, such as `1 / 3`, `Expr::eval_with` takes a
`Precision`:

* `Precision::Round` (the default, used by `Expr::eval`): the result is rounded to 28 digits.
* `Precision::Exact`: the result is an error (`ErrorKind::Precision`). `1 / 3` fails; `1 / 4` is `0.25`.

In both modes a product or quotient of non-zero numbers that would round to zero is an error, never a silent
zero, and a number too large for the type is an error, never a wrapped or capped value.

## License

MIT. See [LICENSE](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/LICENSE).
