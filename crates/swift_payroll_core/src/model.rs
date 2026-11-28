//! The data a payroll is made of: the salary structure going in, the payslips coming out.
//!
//! Every type that is read from JSON refuses a field it does not know (`"wehn"` for `"when"` is an error,
//! not a silently ignored key), so a typo can never quietly change what somebody is paid.

use std::collections::BTreeMap;

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use swift_payroll_expr::Precision;

/// How an amount is rounded to its digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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

/// How many decimal digits an amount keeps, and how it is rounded to them. At most
/// [`Rounding::MAX_DIGITS`] digits: a `Rounding` with more cannot be made or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RoundingSpec")]
pub struct Rounding {
    digits: u32,
    mode: Mode,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoundingSpec {
    #[serde(default = "two")]
    digits: u32,
    #[serde(default)]
    mode: Mode,
}

fn two() -> u32 {
    2
}

impl TryFrom<RoundingSpec> for Rounding {
    type Error = String;

    fn try_from(spec: RoundingSpec) -> Result<Self, String> {
        Rounding::new(spec.digits, spec.mode).ok_or_else(|| format!("rounding keeps at most {} digits, not {}", Rounding::MAX_DIGITS, spec.digits))
    }
}

impl Default for Rounding {
    fn default() -> Self {
        Self { digits: 2, mode: Mode::HalfUp }
    }
}

impl Rounding {
    /// The most digits an amount may keep.
    pub const MAX_DIGITS: u32 = 8;

    /// A rounding to `digits` decimal places, or `None` above [`MAX_DIGITS`](Self::MAX_DIGITS).
    pub const fn new(digits: u32, mode: Mode) -> Option<Self> {
        if digits <= Self::MAX_DIGITS {
            Some(Self { digits, mode })
        } else {
            None
        }
    }

    /// The decimal places kept.
    pub const fn digits(&self) -> u32 {
        self.digits
    }

    /// How an amount is rounded to them.
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Round an amount to exactly [`digits`](Self::digits) places (`5` becomes `5.00`), or `None` when it is
    /// too large to keep that many places (above about 7.9 * 10^26 with 2 digits). Zero is `0.00`, never `-0.00`.
    pub fn checked_apply(&self, amount: Decimal) -> Option<Decimal> {
        let rounded = self.apply(amount);
        (rounded.scale() == self.digits).then_some(rounded)
    }

    /// Round an amount to [`digits`](Self::digits) places (`5` becomes `5.00`). An amount too large for that
    /// many places keeps as many as fit; [`checked_apply`](Self::checked_apply) refuses it instead. Zero is
    /// always shown as `0.00`, never `-0.00`.
    pub fn apply(&self, amount: Decimal) -> Decimal {
        let strategy = match self.mode {
            Mode::HalfUp => RoundingStrategy::MidpointAwayFromZero,
            Mode::HalfEven => RoundingStrategy::MidpointNearestEven,
            Mode::Down => RoundingStrategy::ToZero,
            Mode::Up => RoundingStrategy::AwayFromZero,
        };
        let mut rounded = amount.round_dp_with_strategy(self.digits, strategy);
        rounded.rescale(self.digits);
        if rounded.is_zero() {
            rounded.set_sign_positive(true);
        }
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

    /// No digit count admitted by `Rounding::new` makes `apply` panic, for any mode.
    #[kani::proof]
    fn every_admitted_rounding_applies_without_panicking() {
        let digits: u32 = kani::any();
        let mode = match kani::any::<u8>() % 4 {
            0 => Mode::HalfUp,
            1 => Mode::HalfEven,
            2 => Mode::Down,
            _ => Mode::Up,
        };
        if let Some(rounding) = Rounding::new(digits, mode) {
            assert!(rounding.digits() <= Rounding::MAX_DIGITS);
            let _ = rounding.apply(Decimal::new(2675, 3));
        } else {
            assert!(digits > Rounding::MAX_DIGITS);
        }
    }
}

/// What a component is on the payslip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
    /// The tag every component of this kind has, for `total("...")`.
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Earning => "earning",
            Kind::Deduction => "deduction",
            Kind::Employer => "employer",
            Kind::Info => "info",
        }
    }
}

/// What happens when deductions come to more than gross pay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NegativeNet {
    /// The calculation fails for that person, so nobody is paid a negative amount unseen.
    #[default]
    Error,
    /// Deductions are taken in order of [`Component::priority`] until pay is used up; what could not be
    /// taken is each line's `shortfall`, net pay is zero, and gross − deductions = net still holds.
    Zero,
    /// Negative net pay is allowed (a recovery of an overpayment).
    Allow,
}

/// Serde for [`Precision`], which lives in the expression crate (that has no serde).
mod precision_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use swift_payroll_expr::Precision;

    pub(super) fn serialize<S: Serializer>(precision: &Precision, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match precision {
            Precision::Exact => "exact",
            _ => "round",
        })
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Precision, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "round" => Ok(Precision::Round),
            "exact" => Ok(Precision::Exact),
            other => Err(serde::de::Error::unknown_variant(other, &["round", "exact"])),
        }
    }

    pub(super) fn is_default(precision: &Precision) -> bool {
        *precision == Precision::Round
    }
}

/// One line of the payslip, and how its amount is found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Component {
    /// The name formulas use for it.
    pub id: String,
    /// What the payslip shows.
    pub label: String,
    /// Earning, deduction, employer contribution or working figure.
    pub kind: Kind,
    /// A formula in the expression language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// A fixed amount: a JSON number, or decimal text (`"1500.50"`), which is always read exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Json>,
    /// The name of an input holding the amount.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// A condition: the component counts only when it is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// Groups it belongs to, for `total("tag")`. Its kind is always one of its tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Its own rounding, instead of the structure's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<Rounding>,
    /// A negative result is an error unless this is set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_negative: bool,
    /// For a deduction, when net pay is held at zero ([`NegativeNet::Zero`]): lower numbers are taken from
    /// pay first (statutory tax and social security before loans, loans before voluntary items).
    /// Deductions without a priority come after all those with one, in the order the structure lists them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
}

impl Component {
    fn new(id: impl Into<String>, label: impl Into<String>, kind: Kind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind,
            formula: None,
            amount: None,
            input: None,
            when: None,
            tags: Vec::new(),
            round: None,
            allow_negative: false,
            priority: None,
        }
    }

    /// A component calculated by a formula.
    pub fn formula(id: impl Into<String>, label: impl Into<String>, kind: Kind, formula: impl Into<String>) -> Self {
        Self { formula: Some(formula.into()), ..Self::new(id, label, kind) }
    }

    /// A component with a fixed amount.
    pub fn fixed(id: impl Into<String>, label: impl Into<String>, kind: Kind, amount: Decimal) -> Self {
        Self { amount: Some(Json::String(amount.to_string())), ..Self::new(id, label, kind) }
    }

    /// A component whose amount is one of the person's inputs.
    pub fn input(id: impl Into<String>, label: impl Into<String>, kind: Kind, input: impl Into<String>) -> Self {
        Self { input: Some(input.into()), ..Self::new(id, label, kind) }
    }

    /// Count the component only when `condition` is true.
    pub fn when(mut self, condition: impl Into<String>) -> Self {
        self.when = Some(condition.into());
        self
    }

    /// Add it to a group for `total("tag")`.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Round it its own way.
    pub fn rounding(mut self, rounding: Rounding) -> Self {
        self.round = Some(rounding);
        self
    }

    /// Allow a negative amount.
    pub fn allow_negative(mut self) -> Self {
        self.allow_negative = true;
        self
    }

    /// Its place when deductions are cut to keep net pay at zero (see [`Component::priority`]).
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = Some(priority);
        self
    }
}

/// What type an input must have. Checked when a person is read, before anything is calculated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InputType {
    /// Anything JSON can hold (the default).
    #[default]
    Any,
    /// A number: a JSON number, or decimal text such as `"120000.50"` (read exactly).
    Number,
    /// Text.
    Text,
    /// `true` or `false`.
    Bool,
}

impl InputType {
    fn is_any(&self) -> bool {
        *self == InputType::Any
    }
}

/// A name a person's inputs may have.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct InputSpec {
    /// The name formulas use for it.
    pub name: String,
    /// Used when the person has none; without one the input is required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Json>,
    /// The type it must have.
    #[serde(default, rename = "type", skip_serializing_if = "InputType::is_any")]
    pub input_type: InputType,
}

impl InputSpec {
    /// An input every person must have.
    pub fn required(name: impl Into<String>) -> Self {
        Self { name: name.into(), default: None, input_type: InputType::Any }
    }

    /// An input with a value for people who have none.
    pub fn optional(name: impl Into<String>, default: Json) -> Self {
        Self { name: name.into(), default: Some(default), input_type: InputType::Any }
    }

    /// Require a type.
    pub fn typed(mut self, input_type: InputType) -> Self {
        self.input_type = input_type;
        self
    }
}

/// A salary structure: the components of a payslip and everything they read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Structure {
    /// A JSON Schema reference for editors (`schemas/structure.schema.json`); not used by the engine.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Which structure this is; copied onto each payslip.
    pub id: String,
    /// Which version of it; copied onto each payslip.
    pub version: String,
    /// The currency of every amount; copied onto each payslip.
    pub currency: String,
    /// How lines and totals are rounded (two digits, half up, by default).
    #[serde(default)]
    pub rounding: Rounding,
    /// Whether arithmetic that cannot be held exactly is rounded (default) or an error.
    #[serde(default, with = "precision_serde", skip_serializing_if = "precision_serde::is_default")]
    pub precision: Precision,
    /// What happens when deductions come to more than gross pay.
    #[serde(default)]
    pub negative_net: NegativeNet,
    /// The names a person's inputs may have, and which are optional.
    #[serde(default)]
    pub inputs: Vec<InputSpec>,
    /// Fixed figures and tables (bands, rates) that formulas read by name.
    #[serde(default)]
    pub constants: BTreeMap<String, Json>,
    /// The payslip's lines, in the order they are shown.
    pub components: Vec<Component>,
}

impl Structure {
    /// A structure with default rounding, precision and negative-net policy, and no inputs or constants.
    pub fn new(id: impl Into<String>, version: impl Into<String>, currency: impl Into<String>, components: Vec<Component>) -> Self {
        Self {
            schema: None,
            id: id.into(),
            version: version.into(),
            currency: currency.into(),
            rounding: Rounding::default(),
            precision: Precision::Round,
            negative_net: NegativeNet::default(),
            inputs: Vec::new(),
            constants: BTreeMap::new(),
            components,
        }
    }
}

/// One person's inputs for one pay period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Input {
    /// Who it is; copied onto the payslip and into every error about them.
    pub employee_id: String,
    /// A value for each input of the structure the person has.
    #[serde(default)]
    pub variables: BTreeMap<String, Json>,
}

impl Input {
    /// A person and their values.
    pub fn new(employee_id: impl Into<String>, variables: BTreeMap<String, Json>) -> Self {
        Self { employee_id: employee_id.into(), variables }
    }
}

/// One line of a payslip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Line {
    /// The component's id.
    pub id: String,
    /// The component's label.
    pub label: String,
    /// The component's kind.
    pub kind: Kind,
    /// What the component came to, rounded by its own rule.
    pub amount: Decimal,
    /// False when the component's condition was not met (the amount is zero).
    pub applied: bool,
    /// The component's tags, its kind first.
    pub tags: Vec<String>,
    /// For a deduction cut to keep net pay at zero: the part of `amount` that could not be taken.
    #[serde(default, skip_serializing_if = "Decimal::is_zero")]
    pub shortfall: Decimal,
    /// With explanations on: the formula and the names it read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explain: Option<Explanation>,
}

/// Where a line's amount came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Explanation {
    /// The formula, `input <name>`, or `fixed amount`.
    pub source: String,
    /// The names its formula and condition read, sorted.
    pub reads: Vec<String>,
}

/// A person's payslip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Payslip {
    /// Who it is for.
    pub employee_id: String,
    /// The structure's id.
    pub structure: String,
    /// The structure's version.
    pub structure_version: String,
    /// The structure's currency.
    pub currency: String,
    /// One line per component, in the order the structure lists them.
    pub lines: Vec<Line>,
    /// The sum of the earnings.
    pub gross: Decimal,
    /// The deductions taken from pay (each line's amount less its shortfall).
    pub deductions: Decimal,
    /// `gross - deductions`.
    pub net: Decimal,
    /// The sum of the employer contributions.
    pub employer_cost: Decimal,
    /// What deductions could not be taken because net pay was held at zero.
    pub shortfall: Decimal,
}

/// A person's totals without the lines: what [`Compiled::summarize`](crate::Compiled::summarize) returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Summary {
    /// Who it is for.
    pub employee_id: String,
    /// The sum of the earnings.
    pub gross: Decimal,
    /// The deductions taken from pay.
    pub deductions: Decimal,
    /// `gross - deductions`.
    pub net: Decimal,
    /// The sum of the employer contributions.
    pub employer_cost: Decimal,
    /// What deductions could not be taken because net pay was held at zero.
    pub shortfall: Decimal,
}

/// A person's totals and every component's amount (see [`Compiled::calculate_amounts`](crate::Compiled::calculate_amounts)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Calculation {
    /// The totals.
    pub summary: Summary,
    /// One amount per component, in the order of `Structure::components`.
    pub amounts: Vec<Decimal>,
    /// False where the component's condition was not met (its amount is zero).
    pub applied: Vec<bool>,
    /// Per component: what could not be taken to keep net pay at zero (zero everywhere else).
    pub shortfalls: Vec<Decimal>,
}

/// How much detail [`Compiled::run_all`](crate::Compiled::run_all) and
/// [`Compiled::for_each`](crate::Compiled::for_each) put on each payslip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Detail {
    /// Lines with their amounts.
    #[default]
    Lines,
    /// Lines with an [`Explanation`] each.
    Explained,
}
