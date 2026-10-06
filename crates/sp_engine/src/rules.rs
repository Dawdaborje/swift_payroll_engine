//! Rule packs: a tax or contribution rule kept as data, outside any program.
//!
//! A pack is a JSON description (country, validity dates, where the rule comes from, whether someone has
//! checked it against the law, its tables, and its own test cases) and one expression in the language of
//! `sp_expr`. Odoo keeps such rules as XML records inside modules; here they live in a repository of their
//! own, versioned by date, and every pack carries tests that run whenever the pack is loaded.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use sp_expr::{compile, Expr, MapEnv, Value};

use crate::convert::value_from_json;
use crate::model::{Component, Kind, Rounding};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleTest {
    pub name: String,
    pub inputs: BTreeMap<String, Json>,
    /// The expected result, as text.
    pub expect: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInfo {
    pub id: String,
    pub country: String,
    /// `income_tax`, `social_security`, `levy`, `vat`...
    pub kind: String,
    pub currency: String,
    pub valid_from: String,
    #[serde(default)]
    pub valid_to: Option<String>,
    /// The law or notice it follows (a citation or a link).
    pub source: String,
    /// True only once somebody has checked the figures against that source.
    #[serde(default)]
    pub verified: bool,
    /// The names the expression reads besides the constants.
    pub inputs: Vec<String>,
    #[serde(default)]
    pub constants: BTreeMap<String, Json>,
    #[serde(default)]
    pub rounding: Rounding,
    #[serde(default)]
    pub tests: Vec<RuleTest>,
}

pub struct RulePack {
    pub info: PackInfo,
    expression: Expr,
    constants: BTreeMap<String, Value>,
}

impl RulePack {
    /// Read a pack from its JSON description and the text of its expression.
    pub fn parse(description: &str, expression: &str) -> Result<Self, String> {
        let info: PackInfo = serde_json::from_str(description).map_err(|e| format!("the description: {e}"))?;
        if expression.lines().all(|l| l.trim().is_empty() || l.trim_start().starts_with("//")) {
            return Err(format!("{}: the expression is empty", info.id));
        }
        let expr = compile(expression).map_err(|e| format!("{}: {e}", info.id))?;
        let mut constants = BTreeMap::new();
        for (name, json) in &info.constants {
            constants.insert(name.clone(), value_from_json(json).map_err(|e| format!("{}: constant `{name}`: {e}", info.id))?);
        }
        for name in expr.identifiers() {
            if !constants.contains_key(&name) && !info.inputs.contains(&name) {
                return Err(format!("{}: the expression uses `{name}`, which is neither an input nor a constant", info.id));
            }
        }
        Ok(Self { info, expression: expr, constants })
    }

    /// Whether the rule is in force on a day (`YYYY-MM-DD`; text order is date order).
    pub fn in_force_on(&self, day: &str) -> bool {
        self.info.valid_from.as_str() <= day && self.info.valid_to.as_deref().is_none_or(|to| day <= to)
    }

    pub fn evaluate(&self, inputs: &BTreeMap<String, Json>) -> Result<Decimal, String> {
        let mut env = self.constants.clone();
        for name in &self.info.inputs {
            let json = inputs.get(name).ok_or_else(|| format!("{}: the input `{name}` is missing", self.info.id))?;
            env.insert(name.clone(), value_from_json(json).map_err(|e| format!("{}: `{name}`: {e}", self.info.id))?);
        }
        if let Some(extra) = inputs.keys().find(|k| !self.info.inputs.contains(k)) {
            return Err(format!("{}: `{extra}` is not an input of this rule", self.info.id));
        }
        match self.expression.eval(&MapEnv(env)).map_err(|e| format!("{}: {e}", self.info.id))? {
            Value::Num(n) => Ok(self.info.rounding.apply(n)),
            other => Err(format!("{}: the rule gives a number, not a {}", self.info.id, other.type_name())),
        }
    }

    /// Run the pack's own tests: each name with `Ok` or what was wrong.
    pub fn run_tests(&self) -> Vec<(String, Result<(), String>)> {
        self.info
            .tests
            .iter()
            .map(|test| {
                let outcome = self.evaluate(&test.inputs).and_then(|got| {
                    let want = Decimal::from_str_exact(&test.expect).map_err(|_| format!("expect `{}` is not a number", test.expect))?;
                    if got == want { Ok(()) } else { Err(format!("expected {want}, got {got}")) }
                });
                (test.name.clone(), outcome)
            })
            .collect()
    }

    /// The pack as part of a salary structure: its tables become constants, the component reads them.
    /// `arguments` maps each of the rule's inputs to a formula in the structure, such as
    /// `taxable`, `periods_per_year`. The pack's constants are named `<id with dots as _>_<name>`.
    pub fn as_component(&self, id: &str, label: &str, kind: Kind, arguments: &BTreeMap<String, String>) -> Result<(BTreeMap<String, Json>, Component), String> {
        let prefix = self.info.id.replace(['.', '-'], "_");
        let mut constants = BTreeMap::new();
        let mut source = self.expression.source().to_string();
        for name in self.constants.keys() {
            let renamed = format!("{prefix}_{name}");
            constants.insert(renamed.clone(), self.info.constants.get(name).cloned().unwrap_or(Json::Null));
            source = replace_word(&source, name, &renamed);
        }
        for name in &self.info.inputs {
            let argument = arguments.get(name).ok_or_else(|| format!("{}: give the input `{name}` a formula", self.info.id))?;
            source = replace_word(&source, name, &format!("({argument})"));
        }
        Ok((constants, Component {
            id: id.to_string(),
            label: label.to_string(),
            kind,
            formula: Some(source),
            amount: None,
            input: None,
            when: None,
            tags: Vec::new(),
            round: Some(self.info.rounding),
            allow_negative: false,
        }))
    }
}

/// Replace whole words only, so `table` does not change `tables`.
fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(found) = rest.find(word) {
        let before_ok = rest[..found].chars().next_back().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        let end = found + word.len();
        let after_ok = rest[end..].chars().next().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        out.push_str(&rest[..found]);
        out.push_str(if before_ok && after_ok { with } else { word });
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}
