//! Property tests for `+ - * /` over the *whole* range of the number type: every mantissa size up to 96 bits
//! and every scale up to 28, where the fuzz oracles (amounts up to ±1000.00) never go. Each answer is checked
//! against the exact `i128` oracle; a failure is shrunk by proptest to its smallest case.
//!
//! More cases: `PROPTEST_CASES=100000 cargo test --release -p swift_payroll_expr --test properties`.

use std::collections::BTreeMap;

use proptest::prelude::*;
use rust_decimal::Decimal;
use swift_payroll_expr::{compile, ErrorKind, MapEnv, Precision, Value};

mod oracle;
use oracle::Exact;

/// Any decimal, with mantissa sizes spread evenly from 0 to `max_bits` bits so both small amounts and the
/// edge of the number type are common, and zero (of any scale) often.
fn decimal(max_bits: u32) -> impl Strategy<Value = Decimal> {
    let sized = (0..=max_bits, any::<u128>(), any::<bool>(), 0u32..=28).prop_map(|(bits, raw, negative, scale)| {
        let magnitude = if bits == 0 { 0 } else { raw >> (128 - bits) };
        let m = magnitude as i128;
        Decimal::from_i128_with_scale(if negative { -m } else { m }, scale)
    });
    prop_oneof![
        8 => sized,
        1 => (0u32..=28).prop_map(|scale| Decimal::new(0, scale)),
        1 => Just(Decimal::MAX),
        1 => Just(Decimal::MIN),
    ]
}

fn eval(op: &str, x: Decimal, y: Decimal, precision: Precision) -> Result<Decimal, ErrorKind> {
    let names = BTreeMap::from([("x".to_string(), Value::Num(x)), ("y".to_string(), Value::Num(y))]);
    match compile(&format!("x {op} y")).unwrap().eval_with(&MapEnv(names), precision) {
        Ok(Value::Num(n)) => Ok(n),
        Ok(other) => panic!("x {op} y gave a {}", other.type_name()),
        Err(e) => Err(e.kind),
    }
}

/// The oracle's answer for `x op y`: `Some(Some(v))` when a decimal can hold it exactly, `Some(None)` when
/// it cannot, `None` when the oracle itself cannot tell (its i128 ran out).
fn oracle(op: &str, x: Decimal, y: Decimal) -> Option<Option<Decimal>> {
    let (a, b) = (Exact::of(x), Exact::of(y));
    let exact = match op {
        "+" => a.add(b)?,
        "-" => a.add(b.neg())?,
        "*" => a.mul(b)?,
        _ => unreachable!(),
    };
    Some(exact.reduced().map(Exact::decimal))
}

/// What both precisions promise for `+ - *`, given the oracle's answer.
fn check(op: &str, x: Decimal, y: Decimal) -> Result<(), TestCaseError> {
    let exact = eval(op, x, y, Precision::Exact);
    let round = eval(op, x, y, Precision::Round);
    // A zero is never negative, whatever the signs and scales that made it.
    for result in [&exact, &round].into_iter().flatten() {
        prop_assert!(!(result.is_zero() && result.is_sign_negative()), "{x} {op} {y} gave a negative zero");
    }
    // An exact answer is also what rounding gives: rounding only ever happens when it has to.
    if let Ok(e) = exact {
        prop_assert_eq!(round, Ok(e), "{} {} {}: round and exact disagree", x, op, y);
    }
    match oracle(op, x, y) {
        None => {}
        Some(Some(want)) => prop_assert_eq!(exact, Ok(want), "{} {} {} (exact)", x, op, y),
        Some(None) => prop_assert!(
            matches!(exact, Err(ErrorKind::Precision | ErrorKind::Overflow)),
            "{x} {op} {y}: no decimal holds the answer, so exact must refuse it, not give {exact:?}"
        ),
    }
    // Rounding never fails with anything but a number too large or a non-zero answer rounded to zero.
    if let Err(kind) = round {
        prop_assert!(matches!(kind, ErrorKind::Overflow | ErrorKind::Precision), "{x} {op} {y} (round): {kind:?}");
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn addition_is_exact_or_refused(x in decimal(96), y in decimal(96)) {
        check("+", x, y)?;
    }

    #[test]
    fn subtraction_is_exact_or_refused(x in decimal(96), y in decimal(96)) {
        check("-", x, y)?;
    }

    // Mantissas up to 63 bits keep the product inside the oracle's i128, and two of them still make a
    // product of up to 38 digits: well past the 28 a decimal holds.
    #[test]
    fn multiplication_is_exact_or_refused(x in decimal(63), y in decimal(63)) {
        check("*", x, y)?;
    }

    #[test]
    fn exact_addition_commutes_and_undoes(x in decimal(96), y in decimal(96)) {
        let sum = eval("+", x, y, Precision::Exact);
        prop_assert_eq!(sum, eval("+", y, x, Precision::Exact));
        if let Ok(s) = sum {
            // x + y - y gives back x: nothing was lost on the way.
            prop_assert_eq!(eval("-", s, y, Precision::Exact), Ok(x), "({} + {}) - {}", x, y, y);
        }
    }

    #[test]
    fn exact_multiplication_commutes(x in decimal(96), y in decimal(96)) {
        prop_assert_eq!(eval("*", x, y, Precision::Exact), eval("*", y, x, Precision::Exact));
    }

    /// Exact division gives the quotient only when it has a finite decimal; rounded division is then the
    /// same number, and otherwise off by less than one unit in its last digit.
    #[test]
    fn division_is_exact_or_refused(x in decimal(63), y in decimal(63)) {
        let exact = eval("/", x, y, Precision::Exact);
        let round = eval("/", x, y, Precision::Round);
        if y.is_zero() {
            prop_assert_eq!(exact, Err(ErrorKind::DivisionByZero));
            prop_assert_eq!(round, Err(ErrorKind::DivisionByZero));
            return Ok(());
        }
        for result in [&exact, &round].into_iter().flatten() {
            prop_assert!(!(result.is_zero() && result.is_sign_negative()), "{x} / {y} gave a negative zero");
        }
        if let Ok(q) = exact {
            // The quotient times the divisor is the dividend, exactly.
            let back = Exact::of(q).mul(Exact::of(y)).and_then(|p| p.add(Exact::of(x).neg()));
            if let Some(diff) = back {
                prop_assert_eq!(diff.m, 0, "{} / {} = {} does not multiply back", x, y, q);
            }
            prop_assert_eq!(round, Ok(q), "{} / {}: round and exact disagree", x, y);
        }
        match Exact::of(x).div(Exact::of(y)).map(Exact::reduced) {
            None => {}
            Some(Some(want)) => prop_assert_eq!(exact, Ok(want.decimal()), "{} / {} (exact)", x, y),
            Some(None) => prop_assert!(matches!(exact, Err(ErrorKind::Precision | ErrorKind::Overflow)), "{x} / {y}: {exact:?}"),
        }
        if let Ok(q) = round {
            // |q*y - x| <= |y| * 10^-scale(q), worked out in the oracle so the check itself rounds nothing.
            let (qe, ye) = (Exact::of(q), Exact::of(y));
            if let Some(diff) = qe.mul(ye).and_then(|qy| qy.add(Exact::of(x).neg())) {
                if let Some(bound) = 10i128.checked_pow(diff.s - ye.s - qe.s).and_then(|p| ye.m.abs().checked_mul(p)) {
                    prop_assert!(diff.m.abs() <= bound, "{x} / {y} = {q} is {} away", diff.decimal());
                }
            }
        }
    }

    /// `-x` is never a negative zero either.
    #[test]
    fn negation_has_no_negative_zero(x in decimal(96)) {
        let names = BTreeMap::from([("x".to_string(), Value::Num(x))]);
        let Ok(Value::Num(n)) = compile("-x").unwrap().eval(&MapEnv(names)) else { panic!("-x failed") };
        prop_assert_eq!(n, -x);
        prop_assert!(!(n.is_zero() && n.is_sign_negative()), "-{} gave a negative zero", x);
    }
}
