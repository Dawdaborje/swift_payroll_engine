//! Property tests for `Rounding`, over the whole range of the number type, against an integer oracle that
//! rounds the mantissa by hand. A failure is shrunk by proptest to its smallest case.
//!
//! More cases: `PROPTEST_CASES=100000 cargo test --release -p swift_payroll_core --test properties`.

use proptest::prelude::*;
use swift_payroll_core::{Decimal, Mode, Rounding};

const MANTISSA_LIMIT: u128 = 1 << 96;

/// Any decimal, mantissa sizes spread evenly from 0 to 96 bits, any scale, zero often.
fn decimal() -> impl Strategy<Value = Decimal> {
    let sized = (0u32..=96, any::<u128>(), any::<bool>(), 0u32..=28).prop_map(|(bits, raw, negative, scale)| {
        let m = if bits == 0 { 0 } else { (raw >> (128 - bits)) as i128 };
        Decimal::from_i128_with_scale(if negative { -m } else { m }, scale)
    });
    prop_oneof![8 => sized, 1 => (0u32..=28).prop_map(|s| Decimal::new(0, s)), 1 => Just(Decimal::MAX), 1 => Just(Decimal::MIN)]
}

fn mode() -> impl Strategy<Value = Mode> {
    prop_oneof![Just(Mode::HalfUp), Just(Mode::HalfEven), Just(Mode::Down), Just(Mode::Up)]
}

/// `x` rounded to `digits` places, worked out on its integer mantissa; `None` when a decimal cannot keep
/// that many places.
fn oracle(x: Decimal, digits: u32, mode: Mode) -> Option<Decimal> {
    let (m, s) = (x.mantissa(), x.scale());
    let rounded = if s <= digits {
        m.checked_mul(10i128.pow(digits - s))?
    } else {
        let unit = 10i128.pow(s - digits);
        let (q, rest) = (m / unit, (m % unit).abs());
        let away = match mode {
            Mode::Down => false,
            Mode::Up => rest != 0,
            Mode::HalfUp => rest * 2 >= unit,
            Mode::HalfEven => rest * 2 > unit || (rest * 2 == unit && q % 2 != 0),
            _ => unreachable!("a new rounding mode needs a line here"),
        };
        if away {
            q + m.signum()
        } else {
            q
        }
    };
    (rounded.unsigned_abs() < MANTISSA_LIMIT).then(|| Decimal::from_i128_with_scale(rounded, digits))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(5000))]

    /// `checked_apply` is the hand-rounded value, with exactly `digits` places, or `None` only when no decimal
    /// can keep that many places.
    #[test]
    fn rounding_matches_the_integer_oracle(x in decimal(), digits in 0..=Rounding::MAX_DIGITS, mode in mode()) {
        let rounding = Rounding::new(digits, mode).unwrap();
        let got = rounding.checked_apply(x);
        match oracle(x, digits, mode) {
            Some(want) => {
                let got = got.ok_or_else(|| TestCaseError::fail(format!("{x} to {digits} places ({mode:?}) refused, want {want}")))?;
                prop_assert_eq!(got, want, "{} to {} places ({:?})", x, digits, mode);
                prop_assert_eq!(got.scale(), digits, "{} to {} places gave {}", x, digits, got);
                prop_assert!(!(got.is_zero() && got.is_sign_negative()), "{} rounded to a negative zero", x);
            }
            None => prop_assert_eq!(got, None, "{} cannot keep {} places", x, digits),
        }
    }

    /// Rounding is idempotent: a rounded amount rounds to itself.
    #[test]
    fn rounding_twice_changes_nothing(x in decimal(), digits in 0..=Rounding::MAX_DIGITS, mode in mode()) {
        let rounding = Rounding::new(digits, mode).unwrap();
        if let Some(once) = rounding.checked_apply(x) {
            prop_assert_eq!(rounding.checked_apply(once), Some(once));
        }
    }

    /// Rounding keeps order: `x <= y` gives `round(x) <= round(y)`, so a larger amount never pays less.
    #[test]
    fn rounding_keeps_order(x in decimal(), y in decimal(), digits in 0..=Rounding::MAX_DIGITS, mode in mode()) {
        let rounding = Rounding::new(digits, mode).unwrap();
        let (low, high) = if x <= y { (x, y) } else { (y, x) };
        if let (Some(a), Some(b)) = (rounding.checked_apply(low), rounding.checked_apply(high)) {
            prop_assert!(a <= b, "{} <= {} but they round to {} > {}", low, high, a, b);
        }
    }
}
