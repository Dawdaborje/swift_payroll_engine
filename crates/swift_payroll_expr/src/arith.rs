//! Checked decimal arithmetic, with a choice of what happens when a result cannot be held exactly.
//!
//! [`rust_decimal::Decimal`] keeps a 96-bit integer and a scale of at most 28 decimal places. A result that
//! needs more (`1 / 3`, or the product of two numbers with 20 decimals each) is rounded to fit, without
//! saying so. [`Precision::Round`] keeps that behaviour (it is what most payroll formulas want: a division
//! is rounded far below a cent) but refuses the one silent loss that is never wanted, a product or quotient
//! of non-zero numbers that rounds to zero. [`Precision::Exact`] refuses every result it cannot keep exactly.

use rust_decimal::Decimal;

use crate::error::{Error, ErrorKind, Result};

/// What arithmetic does with a result the number type cannot hold exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Precision {
    /// Round the result to the 28 significant digits the number type holds (half away from zero at the last
    /// digit kept, as `rust_decimal` does). A product or quotient of non-zero numbers that would round to
    /// zero is an error ([`ErrorKind::Precision`]) instead of a silent zero.
    #[default]
    Round,
    /// Every `+ - * /` result, and those of `sum`, `pct`, `prorate` and `bracket`, is exact or an error
    /// ([`ErrorKind::Precision`]). `1 / 3` is an error; `1 / 4` is `0.25`.
    Exact,
}

impl Precision {
    /// `x + y` under this precision: what a `+` in a formula gives, so a host's totals follow the same rule.
    pub fn add(self, x: Decimal, y: Decimal) -> Result<Decimal> {
        add(x, y, self)
    }

    /// `x - y` under this precision.
    pub fn sub(self, x: Decimal, y: Decimal) -> Result<Decimal> {
        sub(x, y, self)
    }
}

fn too_small() -> Error {
    Error::new(ErrorKind::Precision, "the result is too small to hold: it would be rounded to zero")
}

fn inexact() -> Error {
    Error::new(ErrorKind::Precision, "the result cannot be held exactly (it needs more than 28 decimal places or 28 digits)")
}

pub(crate) fn add(x: Decimal, y: Decimal, precision: Precision) -> Result<Decimal> {
    match precision {
        Precision::Round => x.checked_add(y).ok_or_else(Error::overflow),
        Precision::Exact => exact_add(x, y),
    }
}

pub(crate) fn sub(x: Decimal, y: Decimal, precision: Precision) -> Result<Decimal> {
    match precision {
        // Not `x + -y`: that makes `0.00 - 0.00` a negative zero.
        Precision::Round => x.checked_sub(y).ok_or_else(Error::overflow),
        Precision::Exact => exact_add(x, -y),
    }
}

pub(crate) fn mul(x: Decimal, y: Decimal, precision: Precision) -> Result<Decimal> {
    match precision {
        Precision::Round => {
            let product = x.checked_mul(y).ok_or_else(Error::overflow)?;
            if product.is_zero() && !x.is_zero() && !y.is_zero() {
                return Err(too_small());
            }
            Ok(product)
        }
        Precision::Exact => exact_mul(x, y),
    }
}

pub(crate) fn div(x: Decimal, y: Decimal, precision: Precision) -> Result<Decimal> {
    if y.is_zero() {
        return Err(Error::new(ErrorKind::DivisionByZero, "division by zero"));
    }
    let quotient = x.checked_div(y).ok_or_else(Error::overflow)?;
    if quotient.is_zero() && !x.is_zero() {
        return Err(too_small());
    }
    if precision == Precision::Exact {
        // The quotient is exact exactly when multiplying it back gives the dividend, with nothing rounded.
        match exact_mul(quotient, y) {
            Ok(back) if back == x => {}
            _ => return Err(inexact()),
        }
    }
    Ok(quotient)
}

pub(crate) fn rem(x: Decimal, y: Decimal) -> Result<Decimal> {
    if y.is_zero() {
        return Err(Error::new(ErrorKind::DivisionByZero, "division by zero"));
    }
    x.checked_rem(y).ok_or_else(Error::overflow)
}

// ---- exact arithmetic on 256-bit magnitudes
//
// A decimal is `mantissa * 10^-scale` with |mantissa| < 2^96 and scale <= 28. Products of two mantissas
// (< 2^192) and mantissas aligned to a common scale (< 2^96 * 10^28 < 2^190) fit in 256 bits, so every
// result is first computed exactly, then made to fit or refused.

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct U256([u64; 4]); // most significant limb first, so the derived ordering is numeric

impl U256 {
    fn from_u128(n: u128) -> Self {
        U256([0, 0, (n >> 64) as u64, n as u64])
    }

    fn is_zero(&self) -> bool {
        self.0 == [0; 4]
    }

    /// The product, when it fits in 256 bits (always, for the bounded values used here).
    fn mul(&self, other: &U256) -> Option<U256> {
        let a = self.limbs_le();
        let b = other.limbs_le();
        let mut out = [0u64; 8];
        for i in 0..4 {
            let mut carry: u128 = 0;
            for j in 0..4 {
                let cell = out[i + j] as u128 + (a[i] as u128) * (b[j] as u128) + carry;
                out[i + j] = cell as u64;
                carry = cell >> 64;
            }
            out[i + 4] = carry as u64;
        }
        if out[4..].iter().any(|limb| *limb != 0) {
            return None;
        }
        Some(U256([out[3], out[2], out[1], out[0]]))
    }

    fn add(&self, other: &U256) -> Option<U256> {
        let (a, b) = (self.limbs_le(), other.limbs_le());
        let mut out = [0u64; 4];
        let mut carry = 0u128;
        for i in 0..4 {
            let cell = a[i] as u128 + b[i] as u128 + carry;
            out[i] = cell as u64;
            carry = cell >> 64;
        }
        (carry == 0).then_some(U256([out[3], out[2], out[1], out[0]]))
    }

    /// `self - other`, for `self >= other`.
    fn sub(&self, other: &U256) -> U256 {
        let (a, b) = (self.limbs_le(), other.limbs_le());
        let mut out = [0u64; 4];
        let mut borrow = 0i128;
        for i in 0..4 {
            let mut cell = a[i] as i128 - b[i] as i128 - borrow;
            borrow = 0;
            if cell < 0 {
                cell += 1 << 64;
                borrow = 1;
            }
            out[i] = cell as u64;
        }
        U256([out[3], out[2], out[1], out[0]])
    }

    /// The quotient and remainder of a division by a small number.
    fn div_small(&self, divisor: u64) -> (U256, u64) {
        let mut out = [0u64; 4];
        let mut rest: u128 = 0;
        for (i, limb) in self.0.iter().enumerate() {
            let cell = (rest << 64) | *limb as u128;
            out[i] = (cell / divisor as u128) as u64;
            rest = cell % divisor as u128;
        }
        (U256(out), rest as u64)
    }

    fn limbs_le(&self) -> [u64; 4] {
        [self.0[3], self.0[2], self.0[1], self.0[0]]
    }

    /// The value, when it is below 2^96 (what a decimal's mantissa can hold).
    fn to_mantissa(self) -> Option<u128> {
        (self.0[0] == 0 && self.0[1] == 0 && self.0[2] >> 32 == 0).then(|| ((self.0[2] as u128) << 64) | self.0[3] as u128)
    }
}

const MAX_SCALE: u32 = 28;

fn pow10(n: u32) -> U256 {
    U256::from_u128(10u128.pow(n))
}

/// Make `±magnitude * 10^-scale` a decimal, dropping only trailing zeros, or say why it cannot be one.
fn fit(negative: bool, mut magnitude: U256, mut scale: u32) -> Result<Decimal> {
    loop {
        let fits = scale <= MAX_SCALE && magnitude.to_mantissa().is_some();
        if fits {
            break;
        }
        let (quotient, rest) = magnitude.div_small(10);
        if scale == 0 {
            return Err(Error::overflow());
        }
        if rest != 0 {
            return Err(inexact_or_overflow(magnitude, scale));
        }
        magnitude = quotient;
        scale -= 1;
    }
    let mantissa = magnitude.to_mantissa().ok_or_else(Error::overflow)? as i128;
    Ok(Decimal::from_i128_with_scale(if negative { -mantissa } else { mantissa }, scale))
}

/// A magnitude too large for the mantissa: an overflow when its whole-number part alone does not fit,
/// otherwise a loss of decimal places.
fn inexact_or_overflow(magnitude: U256, scale: u32) -> Error {
    let mut whole = magnitude;
    for _ in 0..scale {
        whole = whole.div_small(10).0;
    }
    if whole.to_mantissa().is_none() {
        Error::overflow()
    } else {
        inexact()
    }
}

fn parts(d: Decimal) -> (bool, U256, u32) {
    let mantissa = d.mantissa();
    (mantissa < 0, U256::from_u128(mantissa.unsigned_abs()), d.scale())
}

pub(crate) fn exact_mul(x: Decimal, y: Decimal) -> Result<Decimal> {
    let (nx, mx, sx) = parts(x);
    let (ny, my, sy) = parts(y);
    let magnitude = mx.mul(&my).ok_or_else(Error::overflow)?;
    if magnitude.is_zero() {
        return Ok(Decimal::ZERO);
    }
    fit(nx != ny, magnitude, sx + sy)
}

pub(crate) fn exact_add(x: Decimal, y: Decimal) -> Result<Decimal> {
    let (nx, mx, sx) = parts(x);
    let (ny, my, sy) = parts(y);
    let scale = sx.max(sy);
    let ax = mx.mul(&pow10(scale - sx)).ok_or_else(Error::overflow)?;
    let ay = my.mul(&pow10(scale - sy)).ok_or_else(Error::overflow)?;
    let (negative, magnitude) = if nx == ny {
        (nx, ax.add(&ay).ok_or_else(Error::overflow)?)
    } else if ax >= ay {
        (nx, ax.sub(&ay))
    } else {
        (ny, ay.sub(&ax))
    };
    if magnitude.is_zero() {
        return Ok(Decimal::new(0, scale));
    }
    fit(negative, magnitude, scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> Decimal {
        Decimal::from_str_exact(text).unwrap()
    }

    #[test]
    fn exact_results_keep_their_natural_scale() {
        assert_eq!(exact_mul(d("1.50"), d("2")).unwrap().to_string(), "3.00");
        assert_eq!(exact_mul(d("1000"), d("7.5")).unwrap().to_string(), "7500.0");
        assert_eq!(exact_add(d("0.1"), d("0.20")).unwrap().to_string(), "0.30");
        assert_eq!(exact_add(d("-5.5"), d("2")).unwrap().to_string(), "-3.5");
        assert_eq!(exact_add(d("5"), d("-5.00")).unwrap().to_string(), "0.00");
    }

    #[test]
    fn exact_arithmetic_refuses_what_it_cannot_hold() {
        let tiny = d("0.00000000000000123456789");
        assert_eq!(exact_mul(tiny, d("0.0000000000000012345")).unwrap_err().kind, ErrorKind::Precision);
        assert_eq!(div(d("1"), d("3"), Precision::Exact).unwrap_err().kind, ErrorKind::Precision);
        assert_eq!(div(d("1"), d("4"), Precision::Exact).unwrap(), d("0.25"));
        let max = Decimal::MAX;
        assert_eq!(exact_add(max, d("1")).unwrap_err().kind, ErrorKind::Overflow);
        assert_eq!(exact_add(max, d("0.5")).unwrap_err().kind, ErrorKind::Precision);
        assert_eq!(exact_mul(max, d("2")).unwrap_err().kind, ErrorKind::Overflow);
        // MAX * 0.1 only moves the decimal point: exact. Times 0.3 needs 30 digits: not.
        let tenth = exact_mul(max, d("0.1")).unwrap();
        assert_eq!(tenth.to_string(), "7922816251426433759354395033.5");
        assert_eq!(exact_mul(tenth, d("0.3")).unwrap_err().kind, ErrorKind::Precision);
        assert_eq!(exact_mul(max, d("1.5")).unwrap_err().kind, ErrorKind::Overflow);
    }

    #[test]
    fn trailing_zeros_are_dropped_only_when_needed() {
        // 28 + 2 decimal places, but the last two are zeros: exact.
        let a = d("0.1000000000000000000000000000");
        assert_eq!(exact_mul(a, d("0.10")).unwrap(), d("0.01"));
    }

    #[test]
    fn exact_agrees_with_rust_decimal_whenever_it_succeeds() {
        let values = [
            "0",
            "1",
            "-1",
            "0.01",
            "2.5",
            "123456.789",
            "-0.004",
            "79228162514264337593543950335",
            "0.0000000000000000000000000001",
            "12345678901234567890.123456789",
            "3.3333333333333333333333333333",
        ];
        for a in values {
            for b in values {
                let (x, y) = (d(a), d(b));
                if let Ok(exact) = exact_mul(x, y) {
                    assert_eq!(Some(exact), x.checked_mul(y), "{a} * {b}");
                }
                if let Ok(exact) = exact_add(x, y) {
                    assert_eq!(Some(exact), x.checked_add(y), "{a} + {b}");
                }
            }
        }
    }

    #[test]
    fn rounding_mode_refuses_a_silent_zero() {
        let tiny = d("0.00000000000000123456789");
        assert_eq!(mul(tiny, d("0.0000000000000012345"), Precision::Round).unwrap_err().kind, ErrorKind::Precision);
        assert_eq!(div(d("1"), d("3"), Precision::Round).unwrap().to_string(), "0.3333333333333333333333333333");
    }
}
