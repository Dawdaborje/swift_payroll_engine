use std::collections::BTreeMap;

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Half away from zero: 2.5 is 3.
    #[default]
    HalfUp,
    /// Half to even: 2.5 is 2.
    HalfEven,
    /// Towards zero.
    Down,
    /// Away from zero.
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rounding {
    #[serde(default = "two")]
    pub digits: u32,
    #[serde(default)]
    pub mode: Mode,
}

fn two() -> u32 {
    2
}

impl Default for Rounding {
    fn default() -> Self {
        Self { digits: 2, mode: Mode::HalfUp }
    }
}

impl Rounding {
    pub fn apply(&self, amount: Decimal) -> Decimal {
        let strategy = match self.mode {
            Mode::HalfUp => RoundingStrategy::MidpointAwayFromZero,
            Mode::HalfEven => RoundingStrategy::MidpointNearestEven,
            Mode::Down => RoundingStrategy::ToZero,
            Mode::Up => RoundingStrategy::AwayFromZero,
        };
        let mut rounded = amount.round_dp_with_strategy(self.digits, strategy);
        rounded.rescale(self.digits);
        rounded
    }
}

#[cfg(kani)]
mod verification {
    use super::{Mode, Rounding};
    use rust_decimal::Decimal;

    /// A representative integer amount is unchanged by zero-digit rounding for each supported mode.
    #[kani::proof]
    fn representative_integer_survives_zero_digit_rounding() {
        let amount = Decimal::from(1234_i64);
        for mode in [Mode::HalfUp, Mode::HalfEven, Mode::Down, Mode::Up] {
            let rounded = Rounding { digits: 0, mode }.apply(amount);
            assert_eq!(rounded, amount);
        }
    }

    /// Checks positive and negative half-cent behavior against the engine's documented rounding modes.
    #[kani::proof]
    fn half_cent_rounding_matches_declared_modes() {
        let positive = Decimal::new(2675, 3);
        let negative = Decimal::new(-2675, 3);
        let half_up = Rounding { digits: 2, mode: Mode::HalfUp };
        let half_even = Rounding { digits: 2, mode: Mode::HalfEven };
        let down = Rounding { digits: 2, mode: Mode::Down };
        let up = Rounding { digits: 2, mode: Mode::Up };
        assert_eq!(half_up.apply(positive), Decimal::new(268, 2));
        assert_eq!(half_up.apply(negative), Decimal::new(-268, 2));
        assert_eq!(half_even.apply(positive), Decimal::new(268, 2));
        assert_eq!(half_even.apply(negative), Decimal::new(-268, 2));
        assert_eq!(down.apply(positive), Decimal::new(267, 2));
        assert_eq!(down.apply(negative), Decimal::new(-267, 2));
        assert_eq!(up.apply(positive), Decimal::new(268, 2));
        assert_eq!(up.apply(negative), Decimal::new(-268, 2));
    }

    /// Checks rounding at the one-trillion boundary, including carry into a new integer digit.
    #[kani::proof]
    fn trillion_boundary_rounding_has_no_lost_carry() {
        let positive = Decimal::new(999_999_999_999_995, 3);
        let negative = Decimal::new(-999_999_999_999_995, 3);
        let trillion = Decimal::from(1_000_000_000_000_i64);
        let just_below = Decimal::new(99_999_999_999_999, 2);

        assert_eq!(Rounding { digits: 2, mode: Mode::HalfUp }.apply(positive), trillion);
        assert_eq!(Rounding { digits: 2, mode: Mode::HalfUp }.apply(negative), -trillion);
        assert_eq!(Rounding { digits: 2, mode: Mode::Down }.apply(positive), just_below);
        assert_eq!(Rounding { digits: 2, mode: Mode::Up }.apply(positive), trillion);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Added to pay.
    Earning,
    /// Taken from pay.
    Deduction,
    /// Paid by the employer on top of pay (it is not in net pay).
    Employer,
    /// A working figure (taxable income, gross): shown, never added.
    Info,
}

impl Kind {
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Earning => "earning",
            Kind::Deduction => "deduction",
            Kind::Employer => "employer",
            Kind::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NegativeNet {
    /// The calculation fails for that person, so nobody is paid a negative amount unseen.
    #[default]
    Error,
    /// Deductions are allowed to take pay to zero and no further (the shortfall is shown on the payslip).
    Zero,
    /// Negative net pay is allowed (a recovery of an overpayment).
    Allow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub label: String,
    pub kind: Kind,
    /// A formula in the expression language.
    #[serde(default)]
    pub formula: Option<String>,
    /// A fixed amount (a JSON number).
    #[serde(default)]
    pub amount: Option<Json>,
    /// The name of an input holding the amount.
    #[serde(default)]
    pub input: Option<String>,
    /// A condition: the component counts only when it is true.
    #[serde(default)]
    pub when: Option<String>,
    /// Groups it belongs to, for `sum("tag")`. Its kind is always one of its tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Its own rounding, instead of the structure's.
    #[serde(default)]
    pub round: Option<Rounding>,
    /// A negative result is an error unless this is set.
    #[serde(default)]
    pub allow_negative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSpec {
    pub name: String,
    /// Used when the person has none; without one the input is required.
    #[serde(default)]
    pub default: Option<Json>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Structure {
    pub id: String,
    pub version: String,
    pub currency: String,
    #[serde(default)]
    pub rounding: Rounding,
    #[serde(default)]
    pub negative_net: NegativeNet,
    /// The names a person's inputs may have, and which are optional.
    #[serde(default)]
    pub inputs: Vec<InputSpec>,
    /// Fixed figures and tables (bands, rates) that formulas read by name.
    #[serde(default)]
    pub constants: BTreeMap<String, Json>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Input {
    pub employee_id: String,
    #[serde(default)]
    pub variables: BTreeMap<String, Json>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub id: String,
    pub label: String,
    pub kind: Kind,
    pub amount: Decimal,
    /// False when the component's condition was not met (the amount is zero).
    pub applied: bool,
    pub tags: Vec<String>,
    /// With `explain`: the formula and the names it read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<Explain>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Explain {
    pub source: String,
    pub reads: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Payslip {
    pub employee_id: String,
    pub structure: String,
    pub structure_version: String,
    pub currency: String,
    pub lines: Vec<Line>,
    pub gross: Decimal,
    pub deductions: Decimal,
    pub net: Decimal,
    pub employer_cost: Decimal,
    /// What deductions could not be taken because net pay was held at zero.
    pub shortfall: Decimal,
}

/// A person's totals without the lines: what [`Compiled::summarize`](crate::Compiled::summarize) returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub employee_id: String,
    pub gross: Decimal,
    pub deductions: Decimal,
    pub net: Decimal,
    pub employer_cost: Decimal,
    /// What deductions could not be taken because net pay was held at zero.
    pub shortfall: Decimal,
}

/// A person's totals and every component's amount (see [`Compiled::calculate_amounts`](crate::Compiled::calculate_amounts)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calculation {
    pub summary: Summary,
    /// One amount per component, in the order of `Structure::components`.
    pub amounts: Vec<Decimal>,
    /// False where the component's condition was not met (its amount is zero).
    pub applied: Vec<bool>,
}
