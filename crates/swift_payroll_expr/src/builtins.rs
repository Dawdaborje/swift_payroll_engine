//! The functions every expression has.
//!
//! | Function | What it gives |
//! |---|---|
//! | `min(a, b, ...)`, `max(a, b, ...)` | The smallest or largest number (also of one list: `max([1, 2])`). |
//! | `abs(x)`, `floor(x)`, `ceil(x)`, `trunc(x)` | The usual. |
//! | `round(x)`, `round(x, digits)` | Rounded half away from zero (`digits` from 0 to 10, default 0). |
//! | `round_even(x, digits)` | Rounded half to even. |
//! | `clamp(x, low, high)` | `x` held between `low` and `high`. |
//! | `between(x, low, high)` | Whether `low <= x <= high`. |
//! | `pct(x, p)` | `p` percent of `x`: `pct(2500, 5)` is `125`. |
//! | `prorate(amount, part, whole)` | `amount * part / whole`; `whole` above zero, `part` from 0 to `whole`. |
//! | `bracket(x, table)` | Tax by progressive bands (below). Not rounded. |
//! | `rate_at(x, table)` | The rate of the band `x` falls in (an upper limit belongs to its own band). |
//! | `sum(list)` or `sum(a, b, ...)` | The sum. |
//! | `size(x)` | Items of a list or map, or characters of a string. |
//! | `coalesce(a, b, ...)` | The first value that is not `null`. |
//! | `dec(text)` | Text read as an exact decimal; a number passes through. |
//! | `string(x)` | A value as text. |
//!
//! A bracket `table` is `[[upper, rate], ...]` with upper limits going up; the last `upper` may be `null`
//! for "no limit". Band one covers 0 up to the first upper limit, the next up to its own, and so on; each is
//! taxed at its rate (a fraction: `0.15`). An amount above a table without an open top band is an error,
//! never a silent cap. [`check_bracket_table`] checks a table before it is used.
//!
//! The arithmetic of `sum`, `pct`, `prorate` and `bracket` follows the [`Precision`] of the evaluation.

use rust_decimal::{Decimal, RoundingStrategy};

use crate::arith::{self, Precision};
use crate::error::{Error, ErrorKind, Result};
use crate::eval::num;
use crate::value::Value;

fn arity(name: &str, args: &[&Value], wanted: std::ops::RangeInclusive<usize>) -> Result<()> {
    if wanted.contains(&args.len()) {
        Ok(())
    } else if wanted.start() == wanted.end() {
        Err(Error::new(ErrorKind::Arity, format!("{name} takes {} arguments, not {}", wanted.start(), args.len())))
    } else {
        Err(Error::new(ErrorKind::Arity, format!("{name} takes {} to {} arguments, not {}", wanted.start(), wanted.end(), args.len())))
    }
}

fn numbers(args: &[&Value]) -> Result<Vec<Decimal>> {
    match args {
        [Value::List(items)] => items.iter().map(num).collect(),
        _ => args.iter().map(|v| num(v)).collect(),
    }
}

fn digits(value: Option<&Value>) -> Result<u32> {
    let wrong = || Error::invalid("digits is a whole number from 0 to 10");
    match value {
        None => Ok(0),
        Some(v) => {
            let n = num(v)?;
            if n.fract().is_zero() && n >= Decimal::ZERO && n <= Decimal::from(10) {
                u32::try_from(i64::try_from(n).map_err(|_| wrong())?).map_err(|_| wrong())
            } else {
                Err(wrong())
            }
        }
    }
}

/// One band of a bracket table.
#[derive(Clone, Copy)]
struct Band {
    upper: Option<Decimal>,
    rate: Decimal,
}

/// The bands of a table, checked one by one as they are read, without building a list: a table read by
/// every formula for every person is never copied.
fn bands(table: &Value) -> Result<impl Iterator<Item = Result<Band>> + '_> {
    let Value::List(rows) = table else { return Err(Error::invalid("a bracket table is a list of [upper, rate]")) };
    if rows.is_empty() {
        return Err(Error::invalid("a bracket table has at least one band"));
    }
    let count = rows.len();
    let mut previous: Option<Decimal> = None;
    Ok(rows.iter().enumerate().map(move |(index, row)| {
        let Value::List(pair) = row else { return Err(Error::invalid("a bracket table is a list of [upper, rate]")) };
        let [upper, rate] = pair.as_slice() else { return Err(Error::invalid("each band is [upper, rate]")) };
        let upper = match upper {
            Value::Null => None,
            other => Some(num(other)?),
        };
        if upper.is_none() && index + 1 != count {
            return Err(Error::invalid("only the last band may have no upper limit"));
        }
        if let (Some(before), Some(this)) = (previous, upper) {
            if this <= before {
                return Err(Error::invalid("band limits go up"));
            }
        }
        if upper.is_some_and(|u| u.is_sign_negative() && !u.is_zero()) {
            return Err(Error::invalid("a band limit is not negative"));
        }
        previous = upper;
        let rate = num(rate)?;
        if rate.is_sign_negative() && !rate.is_zero() {
            return Err(Error::invalid("a rate is not negative"));
        }
        Ok(Band { upper, rate })
    }))
}

/// Check that a value is a valid bracket table for `bracket` and `rate_at`: a non-empty list of
/// `[upper, rate]` with upper limits going up, no negative rate, and `null` (no limit) only on the last band.
pub fn check_bracket_table(table: &Value) -> Result<()> {
    for band in bands(table)? {
        band?;
    }
    Ok(())
}

fn bracket(x: Decimal, table: &Value, precision: Precision) -> Result<Decimal> {
    // One pass over every band, checking each, even after `x` has stopped: a broken table is always an error.
    let mut tax = Decimal::ZERO;
    let mut lower = Decimal::ZERO;
    let mut done = x <= Decimal::ZERO;
    for band in bands(table)? {
        let band = band?;
        if done {
            continue;
        }
        let top = band.upper.map_or(x, |u| u.min(x));
        if top > lower {
            let part = arith::mul(arith::sub(top, lower, precision)?, band.rate, precision)?;
            tax = arith::add(tax, part, precision)?;
        }
        match band.upper {
            Some(upper) if x > upper => lower = upper,
            _ => done = true,
        }
    }
    if done {
        Ok(tax)
    } else {
        Err(Error::invalid("the amount is above the top of the table: end it with a band whose upper limit is null"))
    }
}

fn rate_at(x: Decimal, table: &Value) -> Result<Decimal> {
    // One pass, checking every band (as in `bracket`).
    let mut rate = None;
    for band in bands(table)? {
        let band = band?;
        if rate.is_none() && band.upper.is_none_or(|u| x <= u) {
            rate = Some(band.rate);
        }
    }
    rate.ok_or_else(|| Error::invalid("the amount is above the top of the table"))
}

pub(crate) fn call(name: &str, args: &[&Value], precision: Precision) -> Result<Value> {
    let number = |d: Decimal| Ok(Value::Num(d));
    match name {
        "min" | "max" => {
            let list = numbers(args)?;
            let pick = if name == "min" { list.iter().min() } else { list.iter().max() };
            pick.map(|d| Value::Num(*d)).ok_or_else(|| Error::new(ErrorKind::Arity, format!("{name} of nothing")))
        }
        "abs" => {
            arity(name, args, 1..=1)?;
            number(num(args[0])?.abs())
        }
        "floor" => {
            arity(name, args, 1..=1)?;
            number(num(args[0])?.floor())
        }
        "ceil" => {
            arity(name, args, 1..=1)?;
            number(num(args[0])?.ceil())
        }
        "trunc" => {
            arity(name, args, 1..=1)?;
            number(num(args[0])?.trunc())
        }
        "round" | "round_even" => {
            arity(name, args, 1..=2)?;
            let strategy = if name == "round" { RoundingStrategy::MidpointAwayFromZero } else { RoundingStrategy::MidpointNearestEven };
            number(num(args[0])?.round_dp_with_strategy(digits(args.get(1).copied())?, strategy))
        }
        "clamp" => {
            arity(name, args, 3..=3)?;
            let (x, low, high) = (num(args[0])?, num(args[1])?, num(args[2])?);
            if low > high {
                return Err(Error::invalid("the low end is above the high end"));
            }
            number(x.max(low).min(high))
        }
        "between" => {
            arity(name, args, 3..=3)?;
            let (x, low, high) = (num(args[0])?, num(args[1])?, num(args[2])?);
            Ok(Value::Bool(low <= x && x <= high))
        }
        "pct" => {
            arity(name, args, 2..=2)?;
            let product = arith::mul(num(args[0])?, num(args[1])?, precision)?;
            number(arith::div(product, Decimal::ONE_HUNDRED, precision)?)
        }
        "prorate" => {
            arity(name, args, 3..=3)?;
            let whole = num(args[2])?;
            if whole <= Decimal::ZERO {
                return Err(Error::invalid("the whole is above zero"));
            }
            // 31 days of a 30-day period would pay more than the full amount: an error, not a bonus.
            let part = num(args[1])?;
            if part.is_sign_negative() && !part.is_zero() || part > whole {
                return Err(Error::invalid("the part is from zero to the whole (use amount * part / whole to go outside it)"));
            }
            let product = arith::mul(num(args[0])?, part, precision)?;
            number(arith::div(product, whole, precision)?)
        }
        "bracket" => {
            arity(name, args, 2..=2)?;
            number(bracket(num(args[0])?, args[1], precision)?)
        }
        "rate_at" => {
            arity(name, args, 2..=2)?;
            number(rate_at(num(args[0])?, args[1])?)
        }
        "sum" => number(numbers(args)?.into_iter().try_fold(Decimal::ZERO, |sum, d| arith::add(sum, d, precision))?),
        "size" => {
            arity(name, args, 1..=1)?;
            let n = match args[0] {
                Value::List(items) => items.len(),
                Value::Map(map) => map.len(),
                Value::Str(text) => text.chars().count(),
                other => return Err(Error::kind_of_type(format!("no size for a {}", other.type_name()))),
            };
            Ok(Value::from(n as i64))
        }
        "coalesce" => Ok(args.iter().find(|v| !matches!(***v, Value::Null)).map_or(Value::Null, |v| (*v).clone())),
        "dec" => {
            arity(name, args, 1..=1)?;
            match args[0] {
                Value::Str(text) => Decimal::from_str_exact(text.trim()).map(Value::Num).map_err(|_| Error::invalid(format!("`{text}` is not a number"))),
                Value::Num(n) => Ok(Value::Num(*n)),
                other => Err(Error::kind_of_type(format!("cannot read a {} as a number", other.type_name()))),
            }
        }
        "string" => {
            arity(name, args, 1..=1)?;
            Ok(Value::Str(match args[0] {
                Value::Str(s) => s.clone(),
                other => other.to_string(),
            }))
        }
        other => Err(Error::new(ErrorKind::UnknownFunction, format!("there is no function `{other}`"))),
    }
}

#[cfg(kani)]
mod verification {
    use super::call;
    use crate::{Precision, Value};
    use rust_decimal::Decimal;

    /// Confirms the percentage helper uses percent units and does not introduce binary floating point.
    #[kani::proof]
    fn percent_helper_uses_percent_units() {
        let salary = Value::Num(Decimal::from(1000));
        let rate = Value::Num(Decimal::from(5));
        assert_eq!(call("pct", &[&salary, &rate], Precision::Round), Ok(Value::Num(Decimal::from(50))));
    }

    /// Checks a percentage calculation at the selected one-trillion amount bound.
    #[kani::proof]
    fn percent_helper_handles_one_trillion() {
        let salary = Value::Num(Decimal::from(1_000_000_000_000_i64));
        let rate = Value::Num(Decimal::from(5));
        assert_eq!(call("pct", &[&salary, &rate], Precision::Round), Ok(Value::Num(Decimal::from(50_000_000_000_i64))));
    }

    /// A zero denominator for prorate is reported as an error instead of panicking or returning a number.
    #[kani::proof]
    fn prorate_rejects_a_zero_whole() {
        let amount = Value::Num(Decimal::from(100));
        let part = Value::Num(Decimal::from(1));
        let whole = Value::Num(Decimal::ZERO);
        assert!(call("prorate", &[&amount, &part, &whole], Precision::Round).is_err());
    }
}
