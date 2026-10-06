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
