//! Rule packs: a tax or contribution rule kept as data, outside any program.
//!
//! A pack is a JSON description (country, validity dates, where the rule comes from, whether someone has
//! checked it against the law, its tables, and its own test cases) and one expression in the language of
//! `swift_payroll_expr`. The rules live in a repository of their own, versioned by date, and every pack
//! carries tests that run whenever the pack is loaded.
//!
//! A pack becomes part of a salary structure with [`RulePack::as_component`]. That refuses a pack nobody
//! has checked against the law, a pack in another currency, or a pack not in force on the pay date, unless
//! the [`PackChecks`] given say otherwise.

use std::collections::BTreeMap;
use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use swift_payroll_expr::{compile, Expr, MapEnv, Value};

use crate::convert::value_from_json;
use crate::model::{Component, Kind, Rounding, Structure};

/// What kind of thing is wrong with a rule pack or its use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RuleErrorKind {
    /// The JSON description cannot be read, or a field in it is wrong.
    Description,
    /// The expression is empty, does not compile, or reads a name the pack does not declare.
    Expression,
    /// An input is missing, unknown, or not a value the rule can use.
    Input,
    /// Evaluating the rule failed, or it gave something other than a number.
    Evaluation,
    /// A test's `expect` is not a number.
    Test,
    /// The pack has not been checked against the law (`verified: false`).
    Unverified,
    /// The pack's currency is not the structure's.
    Currency,
    /// The pack is not in force on the pay date.
    NotInForce,
}

/// What is wrong with a rule pack or its use.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RuleError {
    /// The pack's id, when it is known.
    pub pack: Option<String>,
    /// The kind of failure.
    pub kind: RuleErrorKind,
    /// What is wrong.
    pub message: String,
}

impl RuleError {
    fn new(pack: &str, kind: RuleErrorKind, message: impl Into<String>) -> Self {
        Self { pack: Some(pack.to_string()), kind, message: message.into() }
    }
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.pack {
            Some(pack) => write!(f, "{pack}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for RuleError {}

/// One of a pack's own test cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct RuleTest {
    /// What it checks.
    pub name: String,
    /// A value for each of the pack's inputs.
    pub inputs: BTreeMap<String, Json>,
    /// The expected result, as text.
    pub expect: String,
    /// Where `expect` comes from: `authority` (printed by the tax authority), `secondary` (a third party's
    /// example) or `calculated` (worked out from the source's rates by the pack's author).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Where in the source the value is found, or how it differs from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

/// A pack's JSON description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PackInfo {
    /// A JSON Schema reference for editors; not used by the engine.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// `country.rule.year`, for example `gm.paye.2025`: lower-case letters, digits, `.`, `-` and `_`,
    /// starting with a letter.
    pub id: String,
    /// The country's ISO code, lower case.
    pub country: String,
    /// `income_tax`, `social_security`, `levy`, `vat`...
    pub kind: String,
    /// The currency of the amounts the rule reads and gives.
    pub currency: String,
    /// The first day it applies, `YYYY-MM-DD`.
    pub valid_from: String,
    /// The last day it applies, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    /// The law or notice it follows (a citation or a link).
    pub source: String,
    /// Saved copies of what `source` cites, relative to the rules repository's `sources/` folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_files: Vec<String>,
    /// True only once somebody has checked the figures against that source.
    #[serde(default)]
    pub verified: bool,
    /// The names the expression reads besides the constants.
    pub inputs: Vec<String>,
    /// Its tables and rates.
    #[serde(default)]
    pub constants: BTreeMap<String, Json>,
    /// How its result is rounded.
    #[serde(default)]
    pub rounding: Rounding,
    /// Its own test cases.
    #[serde(default)]
    pub tests: Vec<RuleTest>,
}

/// What [`RulePack::as_component`] must check before a pack goes into a structure.
///
/// ```
/// use swift_payroll_core::PackChecks;
/// // The pay date is required: without `.on(...)` every pack is refused.
/// let checks = PackChecks::new().currency("GMD").on("2025-06-30");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackChecks {
    /// The structure's currency: the pack's must be the same.
    pub currency: Option<String>,
    /// The pay date (`YYYY-MM-DD`): the pack must be in force on it.
    pub pay_date: Option<String>,
    /// Use the pack even though nobody has checked it against the law.
    pub allow_unverified: bool,
    /// Use the pack without a pay date to check it against.
    pub any_date: bool,
}

impl PackChecks {
    /// No currency to check; an unverified pack is refused, and so is every pack until a pay date is given
    /// with [`on`](Self::on) (a tax table from last year must never run unseen).
    pub fn new() -> Self {
        Self::default()
    }

    /// The checks a structure implies: its currency.
    pub fn for_structure(structure: &Structure) -> Self {
        Self::new().currency(structure.currency.clone())
    }

    /// The pack's currency must be this one.
    pub fn currency(mut self, currency: impl Into<String>) -> Self {
        self.currency = Some(currency.into());
        self
    }

    /// The pack must be in force on this day (`YYYY-MM-DD`).
    pub fn on(mut self, pay_date: impl Into<String>) -> Self {
        self.pay_date = Some(pay_date.into());
        self
    }

    /// Accept a pack without checking that it is in force on a pay date (for a pack's own tests and
    /// demonstrations, never for real pay).
    pub fn any_date(mut self) -> Self {
        self.any_date = true;
        self
    }

    /// Accept a pack whose `verified` is false (for tests and demonstrations, never for real pay).
    pub fn allow_unverified(mut self) -> Self {
        self.allow_unverified = true;
        self
    }
}

/// A rule pack, read and checked.
#[derive(Debug, Clone)]
pub struct RulePack {
    /// Its description.
    pub info: PackInfo,
    expression: Expr,
    constants: BTreeMap<String, Value>,
}

fn valid_pack_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

impl RulePack {
    /// Read a pack from its JSON description and the text of its expression.
    pub fn parse(description: &str, expression: &str) -> Result<Self, RuleError> {
        use RuleErrorKind::{Description, Expression};
        let info: PackInfo =
            serde_json::from_str(description).map_err(|e| RuleError { pack: None, kind: Description, message: format!("the description: {e}") })?;
        let id = info.id.clone();
        let fail = |kind: RuleErrorKind, message: String| RuleError::new(&id, kind, message);
        if !valid_pack_id(&info.id) {
            return Err(fail(Description, "the id is lower-case letters, digits, `.`, `-` and `_`, starting with a letter".to_string()));
        }
        if !valid_date(&info.valid_from) {
            return Err(fail(Description, "`valid_from` must be a real YYYY-MM-DD date".to_string()));
        }
        if let Some(valid_to) = &info.valid_to {
            if !valid_date(valid_to) {
                return Err(fail(Description, "`valid_to` must be a real YYYY-MM-DD date".to_string()));
            }
            if valid_to < &info.valid_from {
                return Err(fail(Description, "`valid_to` must not be before `valid_from`".to_string()));
            }
        }
        let mut input_names = std::collections::BTreeSet::new();
        for name in &info.inputs {
            if !input_names.insert(name) {
                return Err(fail(Description, format!("input `{name}` is declared more than once")));
            }
            if info.constants.contains_key(name) {
                return Err(fail(Description, format!("`{name}` cannot be both an input and a constant")));
            }
        }
        if expression.lines().all(|l| l.trim().is_empty() || l.trim_start().starts_with("//")) {
            return Err(fail(Expression, "the expression is empty".to_string()));
        }
        let expr = compile(expression).map_err(|e| fail(Expression, e.to_string()))?;
        let mut constants = BTreeMap::new();
        for (name, json) in &info.constants {
            constants.insert(name.clone(), value_from_json(json).map_err(|e| fail(Description, format!("constant `{name}`: {e}")))?);
        }
        for name in expr.identifiers() {
            if !constants.contains_key(&name) && !info.inputs.contains(&name) {
                return Err(fail(Expression, format!("the expression uses `{name}`, which is neither an input nor a constant")));
            }
        }
        Ok(Self { info, expression: expr, constants })
    }

    /// Whether the rule is in force on a day (`YYYY-MM-DD`; text order is date order).
    pub fn in_force_on(&self, day: &str) -> bool {
        valid_date(day) && self.info.valid_from.as_str() <= day && self.info.valid_to.as_deref().is_none_or(|to| day <= to)
    }

    /// Evaluate the rule on its own, with a value for each of its inputs.
    pub fn evaluate(&self, inputs: &BTreeMap<String, Json>) -> Result<Decimal, RuleError> {
        let fail = |kind: RuleErrorKind, message: String| RuleError::new(&self.info.id, kind, message);
        let mut env = self.constants.clone();
        for name in &self.info.inputs {
            let json = inputs.get(name).ok_or_else(|| fail(RuleErrorKind::Input, format!("the input `{name}` is missing")))?;
            env.insert(name.clone(), value_from_json(json).map_err(|e| fail(RuleErrorKind::Input, format!("`{name}`: {e}")))?);
        }
        if let Some(extra) = inputs.keys().find(|k| !self.info.inputs.contains(k)) {
            return Err(fail(RuleErrorKind::Input, format!("`{extra}` is not an input of this rule")));
        }
        match self.expression.eval(&MapEnv(env)).map_err(|e| fail(RuleErrorKind::Evaluation, e.to_string()))? {
            Value::Num(n) => self
                .info
                .rounding
                .checked_apply(n)
                .ok_or_else(|| fail(RuleErrorKind::Evaluation, format!("it came to {n}, too large to keep {} decimal places", self.info.rounding.digits()))),
            other => Err(fail(RuleErrorKind::Evaluation, format!("the rule gives a number, not a {}", other.type_name()))),
        }
    }

    /// Run the pack's own tests: each name with `Ok` or what was wrong.
    pub fn run_tests(&self) -> Vec<(String, Result<(), RuleError>)> {
        self.info
            .tests
            .iter()
            .map(|test| {
                let outcome = self.evaluate(&test.inputs).and_then(|got| {
                    let want = Decimal::from_str_exact(&test.expect)
                        .map_err(|_| RuleError::new(&self.info.id, RuleErrorKind::Test, format!("expect `{}` is not a number", test.expect)))?;
                    if got == want {
                        Ok(())
                    } else {
                        Err(RuleError::new(&self.info.id, RuleErrorKind::Test, format!("expected {want}, got {got}")))
                    }
                });
                (test.name.clone(), outcome)
            })
            .collect()
    }

    /// Check a pack's use against `checks`: verified (unless allowed otherwise), same currency, in force on
    /// the pay date.
    pub fn check(&self, checks: &PackChecks) -> Result<(), RuleError> {
        let fail = |kind: RuleErrorKind, message: String| RuleError::new(&self.info.id, kind, message);
        if !self.info.verified && !checks.allow_unverified {
            return Err(fail(
                RuleErrorKind::Unverified,
                "nobody has checked this pack against its source (`verified` is false); use PackChecks::allow_unverified only for tests and demonstrations"
                    .to_string(),
            ));
        }
        if let Some(currency) = &checks.currency {
            if !currency.eq_ignore_ascii_case(&self.info.currency) {
                return Err(fail(RuleErrorKind::Currency, format!("the pack is in {}, the structure in {currency}", self.info.currency)));
            }
        }
        match &checks.pay_date {
            None if checks.any_date => {}
            None => {
                return Err(fail(
                    RuleErrorKind::NotInForce,
                    "no pay date to check the pack against: give one with PackChecks::on (PackChecks::any_date is only for tests)".to_string(),
                ))
            }
            Some(day) => {
                if !valid_date(day) {
                    return Err(fail(RuleErrorKind::NotInForce, format!("`{day}` is not a real YYYY-MM-DD date")));
                }
                if !self.in_force_on(day) {
                    let until = self.info.valid_to.as_deref().map_or(String::new(), |to| format!(" to {to}"));
                    return Err(fail(RuleErrorKind::NotInForce, format!("the pack is in force from {}{until}, not on {day}", self.info.valid_from)));
                }
            }
        }
        Ok(())
    }

    /// The pack as part of a salary structure: its tables become constants, the component reads them.
    /// `arguments` maps each of the rule's inputs to a formula in the structure, such as
    /// `taxable`, `periods_per_year`. The pack's constants are named `<id with . and - as _>_<name>`.
    /// The pack is first [`check`](Self::check)ed against `checks`.
    pub fn as_component(
        &self,
        id: &str,
        label: &str,
        kind: Kind,
        arguments: &BTreeMap<String, String>,
        checks: &PackChecks,
    ) -> Result<(BTreeMap<String, Json>, Component), RuleError> {
        self.check(checks)?;
        if let Some(extra) = arguments.keys().find(|name| !self.info.inputs.contains(name)) {
            return Err(RuleError::new(&self.info.id, RuleErrorKind::Input, format!("`{extra}` is not an input of this rule")));
        }
        let prefix = self.info.id.replace(['.', '-'], "_");
        let mut constants = BTreeMap::new();
        let mut replacements: BTreeMap<&str, String> = BTreeMap::new();
        for name in self.constants.keys() {
            let renamed = format!("{prefix}_{name}");
            constants.insert(renamed.clone(), self.info.constants.get(name).cloned().unwrap_or(Json::Null));
            replacements.insert(name, renamed);
        }
        for name in &self.info.inputs {
            let argument =
                arguments.get(name).ok_or_else(|| RuleError::new(&self.info.id, RuleErrorKind::Input, format!("give the input `{name}` a formula")))?;
            replacements.insert(name, format!("({argument})"));
        }
        let source = substitute(self.expression.source(), &replacements);
        Ok((constants, Component::formula(id, label, kind, source).rounding(self.info.rounding)))
    }
}

fn valid_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' || !bytes.iter().enumerate().all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()) {
        return false;
    }
    let year = date[0..4].parse::<u32>().ok().unwrap_or(0);
    let month = date[5..7].parse::<u32>().ok().unwrap_or(0);
    let day = date[8..10].parse::<u32>().ok().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && day >= 1 && day <= max_day
}

/// Replace names in an expression, all at once and only where the language reads a name: not inside a
/// string, not in a `//` comment, not a function being called (`min(`), and not a key after a `.`. One
/// pass, so a replacement is never itself rewritten by another. Text other than names is copied as it is,
/// whole characters at a time.
fn substitute(text: &str, replacements: &BTreeMap<&str, String>) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut after_dot = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            out.push_str(&text[start..i]);
            after_dot = false;
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            out.push_str(&text[start..i]);
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))) {
                i += 1;
            }
            out.push_str(&text[start..i]);
            after_dot = false;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            let is_call = text[i..].trim_start().starts_with('(');
            match replacements.get(word) {
                Some(with) if !after_dot && !is_call => out.push_str(with),
                _ => out.push_str(word),
            }
            after_dot = false;
        } else {
            // A whole character, so text outside the ASCII range is copied, not mangled byte by byte.
            let ch = text[i..].chars().next().unwrap_or(c as char);
            out.push(ch);
            if !ch.is_whitespace() {
                after_dot = ch == '.';
            }
            i += ch.len_utf8();
        }
    }
    out
}
