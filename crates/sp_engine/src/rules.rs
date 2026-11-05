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
    /// Where `expect` comes from: `authority` (printed by the tax authority), `secondary` (a third party's
    /// example) or `calculated` (worked out from the source's rates by the pack's author).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Where in the source the value is found, or how it differs from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
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
    /// Saved copies of what `source` cites, relative to the rules repository's `sources/` folder.
    #[serde(default)]
    pub source_files: Vec<String>,
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
        let mut replacements: BTreeMap<&str, String> = BTreeMap::new();
        for name in self.constants.keys() {
            let renamed = format!("{prefix}_{name}");
            constants.insert(renamed.clone(), self.info.constants.get(name).cloned().unwrap_or(Json::Null));
            replacements.insert(name, renamed);
        }
        for name in &self.info.inputs {
            let argument = arguments.get(name).ok_or_else(|| format!("{}: give the input `{name}` a formula", self.info.id))?;
            replacements.insert(name, format!("({argument})"));
        }
        let source = substitute(self.expression.source(), &replacements);
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

/// Replace names in an expression, all at once and only where the language reads a name: not inside a
/// string, not in a `//` comment, not a function being called (`min(`), and not a key after a `.`. One
/// pass, so a replacement is never itself rewritten by another.
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
            out.push(c as char);
            if !c.is_ascii_whitespace() {
                after_dot = c == b'.';
            }
            i += 1;
        }
    }
    out
}
