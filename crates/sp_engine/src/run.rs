use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use sp_expr::{Env, Value};

use crate::compile::Compiled;
use crate::convert::value_from_json;
use crate::error::EngineError;
use crate::model::{Explain, Input, Kind, Line, NegativeNet, Payslip};

struct RunEnv<'a> {
    vars: &'a BTreeMap<String, Value>,
    constants: &'a BTreeMap<String, Value>,
    results: &'a BTreeMap<String, Decimal>,
    lines: &'a [Line],
    reads: RefCell<BTreeSet<String>>,
}

impl Env for RunEnv<'_> {
    fn get(&self, name: &str) -> Option<Value> {
        let value = self.results.get(name).map(|n| Value::Num(*n)).or_else(|| self.vars.get(name).cloned()).or_else(|| self.constants.get(name).cloned());
        if value.is_some() {
            self.reads.borrow_mut().insert(name.to_string());
        }
        value
    }

    fn call(&self, name: &str, args: &[Value]) -> Option<sp_expr::Result<Value>> {
        if name != "total" {
            return None;
        }
        Some(match args {
            [Value::Str(tag)] => {
                let sum = self.lines.iter().filter(|l| l.applied && l.tags.contains(tag)).fold(Decimal::ZERO, |sum, l| sum + l.amount);
                Ok(Value::Num(sum))
            }
            _ => Err(sp_expr::Error::new("total takes the tag, written out: total(\"earning\")")),
        })
    }
}

impl Compiled {
    fn variables(&self, input: &Input) -> Result<BTreeMap<String, Value>, EngineError> {
        let fail = |message: String| EngineError::Input { employee: input.employee_id.clone(), message };
        if let Some(unknown) = input.variables.keys().find(|k| !self.inputs.iter().any(|(name, _)| name == *k)) {
            return Err(fail(format!("`{unknown}` is not an input of the salary structure `{}`", self.structure.id)));
        }
        let mut vars = BTreeMap::new();
        for (name, default) in &self.inputs {
            let value = match input.variables.get(name) {
                Some(json) => value_from_json(json).map_err(|e| fail(format!("`{name}`: {e}")))?,
                None => default.clone().ok_or_else(|| fail(format!("`{name}` is missing")))?,
            };
            vars.insert(name.clone(), value);
        }
        Ok(vars)
    }

    /// Calculate one person. With `explain`, each line carries its formula and the names it read.
    pub fn run(&self, input: &Input, explain: bool) -> Result<Payslip, EngineError> {
        let employee = input.employee_id.clone();
        let vars = self.variables(input)?;
        let mut results: BTreeMap<String, Decimal> = BTreeMap::new();
        let mut lines: Vec<Line> = Vec::with_capacity(self.items.len());
        let mut by_index: Vec<Option<Line>> = vec![None; self.items.len()];

        for &index in &self.order {
            let item = &self.items[index];
            let id = &item.component.id;
            let fail = |message: String| EngineError::Component { employee: employee.clone(), component: id.clone(), message };
            let env = RunEnv { vars: &vars, constants: &self.constants, results: &results, lines: &lines, reads: RefCell::new(BTreeSet::new()) };

            let applied = match &item.when {
                Some(when) => match when.eval(&env).map_err(|e| fail(format!("when: {e}")))? {
                    Value::Bool(b) => b,
                    other => return Err(fail(format!("when gives true or false, not a {}", other.type_name()))),
                },
                None => true,
            };
            let amount = if !applied {
                Decimal::ZERO
            } else {
                let raw = if let Some(formula) = &item.formula {
                    match formula.eval(&env).map_err(|e| fail(e.to_string()))? {
                        Value::Num(n) => n,
                        other => return Err(fail(format!("a formula gives a number, not a {}", other.type_name()))),
                    }
                } else if let Some(fixed) = item.fixed {
                    fixed
                } else {
                    let name = item.component.input.as_deref().unwrap_or_default();
                    match vars.get(name) {
                        Some(Value::Num(n)) => *n,
                        Some(other) => return Err(fail(format!("the input `{name}` is a {}, not a number", other.type_name()))),
                        None => return Err(fail(format!("the input `{name}` is missing"))),
                    }
                };
                let rounded = item.rounding.apply(raw);
                if rounded.is_sign_negative() && !rounded.is_zero() && !item.component.allow_negative && item.component.kind != Kind::Info {
                    return Err(fail(format!("it came to {rounded}: an amount is not negative (set allow_negative if it should be)")));
                }
                rounded
            };
            let reads: Vec<String> = env.reads.borrow().iter().cloned().collect();
            let line = Line {
                id: id.clone(),
                label: item.component.label.clone(),
                kind: item.component.kind,
                amount: if applied { amount } else { item.rounding.apply(Decimal::ZERO) },
                applied,
                tags: item.tags.clone(),
                explain: explain.then(|| Explain {
                    source: item.formula.as_ref().map(|f| f.source().to_string()).or_else(|| item.component.input.as_ref().map(|i| format!("input {i}"))).unwrap_or_else(|| "fixed amount".to_string()),
                    reads,
                }),
            };
            results.insert(id.clone(), line.amount);
            lines.push(line.clone());
            by_index[index] = Some(line);
        }

        // Lines are shown as the structure lists them, not in calculation order.
        let shown: Vec<Line> = by_index.into_iter().flatten().collect();
        let rounding = self.structure.rounding;
        let total = |kind: Kind| shown.iter().filter(|l| l.kind == kind).fold(Decimal::ZERO, |sum, l| sum + l.amount);
        let gross = total(Kind::Earning);
        let deductions = total(Kind::Deduction);
        let mut net = gross - deductions;
        let mut shortfall = Decimal::ZERO;
        if net.is_sign_negative() && !net.is_zero() {
            match self.structure.negative_net {
                NegativeNet::Error => return Err(EngineError::NegativeNet { employee, net: rounding.apply(net).to_string() }),
                NegativeNet::Zero => {
                    shortfall = -net;
                    net = Decimal::ZERO;
                }
                NegativeNet::Allow => {}
            }
        }
        let fix = |d: Decimal| rounding.apply(d);
        Ok(Payslip {
            employee_id: input.employee_id.clone(),
            structure: self.structure.id.clone(),
            structure_version: self.structure.version.clone(),
            currency: self.structure.currency.clone(),
            gross: fix(gross),
            deductions: fix(deductions),
            net: fix(net),
            employer_cost: fix(total(Kind::Employer)),
            shortfall: fix(shortfall),
            lines: shown,
        })
    }

    /// Calculate many people; in parallel when the `parallel` feature is on. A person who fails does
    /// not stop the others: each answer is its own result, in the order given.
    pub fn run_all(&self, inputs: &[Input], explain: bool) -> Vec<Result<Payslip, EngineError>> {
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            inputs.par_iter().map(|i| self.run(i, explain)).collect()
        }
        #[cfg(not(feature = "parallel"))]
        {
            inputs.iter().map(|i| self.run(i, explain)).collect()
        }
    }
}
