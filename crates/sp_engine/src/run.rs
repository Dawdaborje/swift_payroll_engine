use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeSet;

use rust_decimal::Decimal;
use sp_expr::{Env, Value};

use crate::compile::{Compiled, Slot};
use crate::convert::value_from_json;
use crate::error::EngineError;
use crate::model::{Explain, Input, Kind, Line, NegativeNet, Payslip, Summary};

/// What a formula can see while one person is calculated. Names were turned into positions when the
/// structure was compiled, so a lookup is one hash of a short name and an index, and nothing is copied
/// except the value itself.
struct RunEnv<'a> {
    compiled: &'a Compiled,
    vars: &'a [Value],
    amounts: &'a [Decimal],
    done: &'a [bool],
    /// Running total of the applied lines of each tag, in calculation order.
    tag_sums: &'a [Decimal],
    /// A tag whose running total no longer fits the number type.
    tag_overflow: &'a [bool],
    /// With `explain`: the positions of the names read (a name is a `Slot`, kept as a small number).
    reads: Option<RefCell<BTreeSet<usize>>>,
}

impl Env for RunEnv<'_> {
    fn get(&self, name: &str) -> Option<Value> {
        self.get_ref(name).map(Cow::into_owned)
    }

    fn get_ref(&self, name: &str) -> Option<Cow<'_, Value>> {
        let slot = *self.compiled.slots.get(name)?;
        let (key, value) = match slot {
            Slot::Item(i) if self.done[i] => (i, Cow::Owned(Value::Num(self.amounts[i]))),
            Slot::Item(_) => return None,
            // Inputs and constants (a table of bands is read by every formula for every person) are lent, not copied.
            Slot::Input(i) => (self.compiled.items.len() + i, Cow::Borrowed(&self.vars[i])),
            Slot::Constant(i) => (self.compiled.items.len() + self.compiled.inputs.len() + i, Cow::Borrowed(&self.compiled.constants[i])),
        };
        if let Some(reads) = &self.reads {
            reads.borrow_mut().insert(key);
        }
        Some(value)
    }

    fn call(&self, name: &str, args: &[&Value]) -> Option<sp_expr::Result<Value>> {
        if name != "total" {
            return None;
        }
        Some(match args {
            [Value::Str(tag)] => match self.compiled.tag_names.get(tag) {
                Some(t) if self.tag_overflow[*t] => Err(sp_expr::Error::new("the total is too large")),
                Some(t) => Ok(Value::Num(self.tag_sums[*t])),
                None => Ok(Value::Num(Decimal::ZERO)),
            },
            _ => Err(sp_expr::Error::new("total takes the tag, written out: total(\"earning\")")),
        })
    }
}

/// One person, calculated: an amount for every component (by position in the structure) and the totals.
struct Calculated {
    amounts: Vec<Decimal>,
    applied: Vec<bool>,
    reads: Vec<Vec<String>>,
    summary: Summary,
}

impl Compiled {
    fn variables(&self, input: &Input) -> Result<Vec<Value>, EngineError> {
        let fail = |message: String| EngineError::Input { employee: input.employee_id.clone(), message };
        for key in input.variables.keys() {
            if !matches!(self.slots.get(key.as_str()), Some(Slot::Input(_))) {
                return Err(fail(format!("`{key}` is not an input of the salary structure `{}`", self.structure.id)));
            }
        }
        let mut vars = Vec::with_capacity(self.inputs.len());
        for (name, default) in &self.inputs {
            let value = match input.variables.get(name) {
                Some(json) => value_from_json(json).map_err(|e| fail(format!("`{name}`: {e}")))?,
                None => default.clone().ok_or_else(|| fail(format!("`{name}` is missing")))?,
            };
            vars.push(value);
        }
        Ok(vars)
    }

    fn calculate(&self, input: &Input, explain: bool) -> Result<Calculated, EngineError> {
        let employee = &input.employee_id;
        let vars = self.variables(input)?;
        let count = self.items.len();
        let mut amounts = vec![Decimal::ZERO; count];
        let mut applied_flags = vec![false; count];
        let mut done = vec![false; count];
        let mut reads: Vec<Vec<String>> = if explain { vec![Vec::new(); count] } else { Vec::new() };
        let mut tag_sums = vec![Decimal::ZERO; self.tag_names.len()];
        let mut tag_overflow = vec![false; self.tag_names.len()];

        for &index in &self.order {
            let item = &self.items[index];
            let id = &item.component.id;
            let fail = |message: String| EngineError::Component { employee: employee.clone(), component: id.clone(), message };
            let env = RunEnv { compiled: self, vars: &vars, amounts: &amounts, done: &done, tag_sums: &tag_sums, tag_overflow: &tag_overflow, reads: explain.then(|| RefCell::new(BTreeSet::new())) };

            let applied = match &item.when {
                Some(when) => match when.eval(&env).map_err(|e| fail(format!("when: {e}")))? {
                    Value::Bool(b) => b,
                    other => return Err(fail(format!("when gives true or false, not a {}", other.type_name()))),
                },
                None => true,
            };
            let amount = if !applied {
                item.rounding.apply(Decimal::ZERO)
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
                    let position = self.inputs.iter().position(|(n, _)| n == name);
                    match position.map(|p| &vars[p]) {
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
            if explain {
                let seen = env.reads.as_ref().map(|r| r.borrow().clone()).unwrap_or_default();
                let mut names: Vec<String> = seen.into_iter().map(|key| self.name_at(key)).collect();
                names.sort();
                reads[index] = names;
            }
            amounts[index] = amount;
            applied_flags[index] = applied;
            done[index] = true;
            if applied {
                for &tag in &self.item_tags[index] {
                    match tag_sums[tag].checked_add(amount) {
                        Some(sum) => tag_sums[tag] = sum,
                        None => tag_overflow[tag] = true,
                    }
                }
            }
        }

        let too_large = |what: &str| EngineError::Input { employee: employee.clone(), message: format!("the {what} is too large to hold") };
        // Totals add the lines in the order the structure lists them.
        let total = |kind: Kind| self.items.iter().enumerate().filter(|(_, i)| i.component.kind == kind).try_fold(Decimal::ZERO, |sum, (n, _)| sum.checked_add(amounts[n]));
        let gross = total(Kind::Earning).ok_or_else(|| too_large("gross pay"))?;
        let deductions = total(Kind::Deduction).ok_or_else(|| too_large("deductions"))?;
        let employer = total(Kind::Employer).ok_or_else(|| too_large("employer cost"))?;
        let mut net = gross.checked_sub(deductions).ok_or_else(|| too_large("net pay"))?;
        let mut shortfall = Decimal::ZERO;
        let rounding = self.structure.rounding;
        if net.is_sign_negative() && !net.is_zero() {
            match self.structure.negative_net {
                NegativeNet::Error => return Err(EngineError::NegativeNet { employee: employee.clone(), net: rounding.apply(net).to_string() }),
                NegativeNet::Zero => {
                    shortfall = -net;
                    net = Decimal::ZERO;
                }
                NegativeNet::Allow => {}
            }
        }
        let fix = |d: Decimal| rounding.apply(d);
        let summary = Summary {
            employee_id: employee.clone(),
            gross: fix(gross),
            deductions: fix(deductions),
            net: fix(net),
            employer_cost: fix(employer),
            shortfall: fix(shortfall),
        };
        Ok(Calculated { amounts, applied: applied_flags, reads, summary })
    }

    /// The name of a component, input or constant from the numbering `RunEnv` uses for reads.
    fn name_at(&self, key: usize) -> String {
        let items = self.items.len();
        if key < items {
            self.items[key].component.id.clone()
        } else if key < items + self.inputs.len() {
            self.inputs[key - items].0.clone()
        } else {
            let wanted = key - items - self.inputs.len();
            self.slots.iter().find(|(_, s)| matches!(s, Slot::Constant(i) if *i == wanted)).map(|(n, _)| n.clone()).unwrap_or_default()
        }
    }

    /// Calculate one person. With `explain`, each line carries its formula and the names it read.
    pub fn run(&self, input: &Input, explain: bool) -> Result<Payslip, EngineError> {
        let done = self.calculate(input, explain)?;
        let mut reads = done.reads;
        // Lines are shown as the structure lists them, not in calculation order.
        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| Line {
                id: item.component.id.clone(),
                label: item.component.label.clone(),
                kind: item.component.kind,
                amount: done.amounts[index],
                applied: done.applied[index],
                tags: item.tags.clone(),
                explain: explain.then(|| Explain {
                    source: item.formula.as_ref().map(|f| f.source().to_string()).or_else(|| item.component.input.as_ref().map(|i| format!("input {i}"))).unwrap_or_else(|| "fixed amount".to_string()),
                    reads: std::mem::take(&mut reads[index]),
                }),
            })
            .collect();
        let s = done.summary;
        Ok(Payslip {
            employee_id: s.employee_id,
            structure: self.structure.id.clone(),
            structure_version: self.structure.version.clone(),
            currency: self.structure.currency.clone(),
            lines,
            gross: s.gross,
            deductions: s.deductions,
            net: s.net,
            employer_cost: s.employer_cost,
            shortfall: s.shortfall,
        })
    }

    /// Calculate one person and keep only the totals. The same numbers as [`run`](Self::run), without
    /// building the payslip's lines: for a payroll of very many people, where the lines of each person are
    /// not wanted in memory all at once (calculate them again with `run` for the few who need a payslip).
    pub fn summarize(&self, input: &Input) -> Result<Summary, EngineError> {
        Ok(self.calculate(input, false)?.summary)
    }

    /// Calculate many people; in parallel when the `parallel` feature is on. A person who fails does
    /// not stop the others: each answer is its own result, in the order given.
    ///
    /// Every payslip stays in memory until the call returns (one line per component per person). For
    /// more than a few thousand people use [`for_each`](Self::for_each) or [`summarize`](Self::summarize).
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

    /// Calculate people and hand each answer to `sink`, in the order given, without holding more than
    /// `window` of them in memory at once. A person who fails does not stop the others.
    pub fn for_each<'a, I, F>(&self, inputs: I, explain: bool, window: usize, mut sink: F)
    where
        I: IntoIterator<Item = &'a Input>,
        F: FnMut(&Input, Result<Payslip, EngineError>),
    {
        let window = window.max(1);
        let mut iter = inputs.into_iter();
        loop {
            let batch: Vec<&Input> = iter.by_ref().take(window).collect();
            if batch.is_empty() {
                return;
            }
            #[cfg(feature = "parallel")]
            let results: Vec<Result<Payslip, EngineError>> = {
                use rayon::prelude::*;
                batch.par_iter().map(|i| self.run(i, explain)).collect()
            };
            #[cfg(not(feature = "parallel"))]
            let results: Vec<Result<Payslip, EngineError>> = batch.iter().map(|i| self.run(i, explain)).collect();
            for (input, result) in batch.into_iter().zip(results) {
                sink(input, result);
            }
        }
    }
}
