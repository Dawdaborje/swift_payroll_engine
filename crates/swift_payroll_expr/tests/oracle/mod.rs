//! An exact decimal oracle on `i128`, shared by the fuzz and property tests. It never rounds: an operation
//! whose answer `i128` cannot hold gives `None`, and the test skips that case.

#![allow(dead_code)] // each test file uses part of it

use rust_decimal::Decimal;

/// An exact decimal `m / 10^s` the oracle works with.
#[derive(Clone, Copy, Debug)]
pub struct Exact {
    pub m: i128,
    pub s: u32,
}

const MANTISSA_LIMIT: u128 = 1 << 96;

impl Exact {
    pub fn of(d: Decimal) -> Self {
        Exact { m: d.mantissa(), s: d.scale() }
    }
    /// Whether a `Decimal` can hold it (after dropping trailing zeros, as the engine may).
    pub fn reduced(mut self) -> Option<Self> {
        while self.s > 0 && self.m % 10 == 0 {
            self.m /= 10;
            self.s -= 1;
        }
        (self.s <= 28 && self.m.unsigned_abs() < MANTISSA_LIMIT).then_some(self)
    }
    pub fn decimal(self) -> Decimal {
        Decimal::from_i128_with_scale(self.m, self.s)
    }
    fn scaled(self, s: u32) -> Option<i128> {
        self.m.checked_mul(10i128.checked_pow(s - self.s)?)
    }
    pub fn add(self, o: Self) -> Option<Self> {
        let s = self.s.max(o.s);
        Some(Exact { m: self.scaled(s)?.checked_add(o.scaled(s)?)?, s })
    }
    pub fn neg(self) -> Self {
        Exact { m: -self.m, s: self.s }
    }
    pub fn mul(self, o: Self) -> Option<Self> {
        Some(Exact { m: self.m.checked_mul(o.m)?, s: self.s + o.s })
    }
    /// `self / o` as a finite decimal, or `None` when it has none (a factor other than 2 and 5 is left
    /// in the denominator) or the oracle's i128 runs out.
    pub fn div(self, o: Self) -> Option<Self> {
        let gcd = |mut a: i128, mut b: i128| {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a.abs()
        };
        // self.m/10^self.s / (o.m/10^o.s) = self.m * 10^o.s / (o.m * 10^self.s)
        let mut num = self.m.checked_mul(10i128.checked_pow(o.s)?)?;
        let mut den = o.m.checked_mul(10i128.checked_pow(self.s)?)?;
        if den < 0 {
            (num, den) = (-num, -den);
        }
        let g = gcd(num, den).max(1);
        (num, den) = (num / g, den / g);
        let (mut twos, mut fives, mut rest) = (0u32, 0u32, den);
        while rest % 2 == 0 {
            rest /= 2;
            twos += 1;
        }
        while rest % 5 == 0 {
            rest /= 5;
            fives += 1;
        }
        if rest != 1 {
            return None;
        }
        let k = twos.max(fives);
        let m = num.checked_mul(2i128.checked_pow(k - twos)?)?.checked_mul(5i128.checked_pow(k - fives)?)?;
        Some(Exact { m, s: k })
    }
}
