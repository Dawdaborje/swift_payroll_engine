//! The functions every expression has.
//!
//! * `min(a, b, ...)`, `max(a, b, ...)` (or one list), `abs(x)`, `floor(x)`, `ceil(x)`, `trunc(x)`
//! * `round(x)`, `round(x, digits)`: half away from zero; `round_even(x, digits)`: half to even
//! * `clamp(x, low, high)`, `between(x, low, high)` (both ends included)
//! * `pct(x, p)`: `p` percent of `x`; `prorate(amount, part, whole)`: `amount * part / whole`
//! * `bracket(x, table)`: tax by progressive bands. `table` is `[[upper, rate], ...]` in ascending order, the
//!   last `upper` may be `null` for "no limit". Band one covers 0 up to the first upper limit, the next
//!   covers up to its own, and so on; each is taxed at its rate (a fraction: `0.15`). Not rounded.
//! * `rate_at(x, table)`: the rate of the band `x` falls in (the upper limit belongs to its band)
//! * `sum(list)`, `size(x)`, `coalesce(a, b, ...)`, `dec(text)`, `string(x)`

use rust_decimal::{Decimal, RoundingStrategy};

use crate::error::{Error, Result};
use crate::eval::num;
use crate::value::Value;

fn arity(name: &str, args: &[Value], wanted: std::ops::RangeInclusive<usize>) -> Result<()> {
    if wanted.contains(&args.len()) {
        Ok(())
    } else {
        Err(Error::new(format!("{name} takes {} to {} arguments, not {}", wanted.start(), wanted.end(), args.len())))
    }
}

fn numbers(args: &[Value]) -> Result<Vec<Decimal>> {
    match args {
        [Value::List(items)] => items.iter().map(num).collect(),
        _ => args.iter().map(num).collect(),
    }
}

fn digits(value: Option<&Value>) -> Result<u32> {
    match value {
        None => Ok(0),
        Some(v) => {
            let n = num(v)?;
            if n.fract().is_zero() && n >= Decimal::ZERO && n <= Decimal::from(10) {
                u32::try_from(i64::try_from(n).map_err(|_| Error::new("digits is a whole number from 0 to 10"))?).map_err(|_| Error::new("digits is a whole number from 0 to 10"))
            } else {
                Err(Error::new("digits is a whole number from 0 to 10"))
            }
        }
    }
}

struct Band {
    upper: Option<Decimal>,
    rate: Decimal,
}

fn bands(table: &Value) -> Result<Vec<Band>> {
    let Value::List(rows) = table else { return Err(Error::new("a bracket table is a list of [upper, rate]")) };
    let mut out: Vec<Band> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let Value::List(pair) = row else { return Err(Error::new("a bracket table is a list of [upper, rate]")) };
        let [upper, rate] = pair.as_slice() else { return Err(Error::new("each band is [upper, rate]")) };
        let upper = match upper {
            Value::Null => None,
            other => Some(num(other)?),
        };
        if upper.is_none() && index + 1 != rows.len() {
            return Err(Error::new("only the last band may have no upper limit"));
        }
        if let (Some(previous), Some(this)) = (out.last().and_then(|b| b.upper), upper) {
            if this <= previous {
                return Err(Error::new("band limits go up"));
            }
        }
        let rate = num(rate)?;
        if rate < Decimal::ZERO {
            return Err(Error::new("a rate is not negative"));
        }
        out.push(Band { upper, rate });
    }
    if out.is_empty() {
        return Err(Error::new("a bracket table has at least one band"));
    }
    Ok(out)
}

fn bracket(x: Decimal, table: &Value) -> Result<Decimal> {
    let bands = bands(table)?;
    if x <= Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }
    let mut tax = Decimal::ZERO;
    let mut lower = Decimal::ZERO;
    for band in &bands {
        let top = band.upper.map_or(x, |u| u.min(x));
        if top > lower {
            tax = tax.checked_add((top - lower).checked_mul(band.rate).ok_or_else(|| Error::new("the number is too large"))?).ok_or_else(|| Error::new("the number is too large"))?;
        }
        match band.upper {
            Some(upper) if x > upper => lower = upper,
            _ => return Ok(tax),
        }
    }
    Err(Error::new("the amount is above the top of the table: end it with a band whose upper limit is null"))
}

fn rate_at(x: Decimal, table: &Value) -> Result<Decimal> {
    for band in bands(table)? {
        if band.upper.is_none_or(|u| x <= u) {
            return Ok(band.rate);
        }
    }
    Err(Error::new("the amount is above the top of the table"))
}

pub(crate) fn call(name: &str, args: &[Value]) -> Result<Value> {
    let number = |d: Decimal| Ok(Value::Num(d));
    match name {
        "min" | "max" => {
            let list = numbers(args)?;
            let pick = if name == "min" { list.iter().min() } else { list.iter().max() };
            pick.map(|d| Value::Num(*d)).ok_or_else(|| Error::new(format!("{name} of nothing")))
        }
        "abs" => {
            arity(name, args, 1..=1)?;
            number(num(&args[0])?.abs())
        }
        "floor" => {
            arity(name, args, 1..=1)?;
            number(num(&args[0])?.floor())
        }
        "ceil" => {
            arity(name, args, 1..=1)?;
            number(num(&args[0])?.ceil())
        }
        "trunc" => {
            arity(name, args, 1..=1)?;
            number(num(&args[0])?.trunc())
        }
        "round" | "round_even" => {
            arity(name, args, 1..=2)?;
            let strategy = if name == "round" { RoundingStrategy::MidpointAwayFromZero } else { RoundingStrategy::MidpointNearestEven };
            number(num(&args[0])?.round_dp_with_strategy(digits(args.get(1))?, strategy))
        }
        "clamp" => {
            arity(name, args, 3..=3)?;
            let (x, low, high) = (num(&args[0])?, num(&args[1])?, num(&args[2])?);
            if low > high {
                return Err(Error::new("the low end is above the high end"));
            }
            number(x.max(low).min(high))
        }
        "between" => {
            arity(name, args, 3..=3)?;
            let (x, low, high) = (num(&args[0])?, num(&args[1])?, num(&args[2])?);
            Ok(Value::Bool(low <= x && x <= high))
        }
        "pct" => {
            arity(name, args, 2..=2)?;
            let product = num(&args[0])?.checked_mul(num(&args[1])?).ok_or_else(|| Error::new("the number is too large"))?;
            number(product / Decimal::from(100))
        }
        "prorate" => {
            arity(name, args, 3..=3)?;
            let whole = num(&args[2])?;
            if whole <= Decimal::ZERO {
                return Err(Error::new("the whole is above zero"));
            }
            let product = num(&args[0])?.checked_mul(num(&args[1])?).ok_or_else(|| Error::new("the number is too large"))?;
            number(product / whole)
        }
        "bracket" => {
            arity(name, args, 2..=2)?;
            number(bracket(num(&args[0])?, &args[1])?)
        }
        "rate_at" => {
            arity(name, args, 2..=2)?;
            number(rate_at(num(&args[0])?, &args[1])?)
        }
        "sum" => number(numbers(args)?.into_iter().try_fold(Decimal::ZERO, |sum, d| sum.checked_add(d)).ok_or_else(|| Error::new("the number is too large"))?),
        "size" => {
            arity(name, args, 1..=1)?;
            let n = match &args[0] {
                Value::List(items) => items.len(),
                Value::Map(map) => map.len(),
                Value::Str(text) => text.chars().count(),
                other => return Err(Error::new(format!("no size for a {}", other.type_name()))),
            };
            Ok(Value::from(n as i64))
        }
        "coalesce" => Ok(args.iter().find(|v| !matches!(v, Value::Null)).cloned().unwrap_or(Value::Null)),
        "dec" => {
            arity(name, args, 1..=1)?;
            match &args[0] {
                Value::Str(text) => Decimal::from_str_exact(text.trim()).map(Value::Num).map_err(|_| Error::new(format!("`{text}` is not a number"))),
                Value::Num(n) => Ok(Value::Num(*n)),
                other => Err(Error::new(format!("cannot read a {} as a number", other.type_name()))),
            }
        }
        "string" => {
            arity(name, args, 1..=1)?;
            Ok(Value::Str(match &args[0] {
                Value::Str(s) => s.clone(),
                other => other.to_string(),
            }))
        }
        other => Err(Error::new(format!("there is no function `{other}`"))),
    }
}
