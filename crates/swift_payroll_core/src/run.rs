use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};

use rust_decimal::Decimal;
use swift_payroll_expr::{Env, ErrorKind, Value};

use crate::compile::{typed, Compiled, Slot};
use crate::convert::{number_from_text, value_from_json};
use crate::error::{ComponentErrorKind, EngineError, Total};
use crate::model::{Calculation, Detail, Explanation, Input, Kind, Line, NegativeNet, Payslip, Summary};

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
    /// With explanations: the positions of the names read (a name is a `Slot`, kept as a small number).
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

    fn call(&self, name: &str, args: &[&Value]) -> Option<swift_payroll_expr::Result<Value>> {
        if name != "total" {
            return None;
        }
        Some(match args {
            [Value::Str(tag)] => match self.compiled.tag_names.get(tag) {
                Some(&t) => {
                    if let Some(reads) = &self.reads {
                        reads.borrow_mut().insert(self.compiled.tag_key(t));
                    }
                    if self.tag_overflow[t] {
                        Err(swift_payroll_expr::Error::new(ErrorKind::Overflow, "the total is too large"))
                    } else {
                        Ok(Value::Num(self.tag_sums[t]))
                    }
                }
                None => Ok(Value::Num(Decimal::ZERO)),
            },
            _ => Err(swift_payroll_expr::Error::new(ErrorKind::Arity, "total takes the tag, written out: total(\"earning\")")),
        })
    }
}

/// One person, calculated: an amount for every component (by position in the structure) and the totals.
struct Calculated {
    amounts: Vec<Decimal>,
    applied: Vec<bool>,
    shortfalls: Vec<Decimal>,
    reads: Vec<Vec<String>>,
    summary: Summary,
}

impl Compiled {
    fn variables(&self, input: &Input) -> Result<Vec<Value>, EngineError> {
        let fail = |name: &str, message: String| EngineError::Input { employee: input.employee_id.clone(), input: Some(name.to_string()), message };
        if input.employee_id.trim().is_empty() {
            return Err(EngineError::Input { employee: input.employee_id.clone(), input: None, message: "employee_id is empty".to_string() });
        }
        for key in input.variables.keys() {
            if !matches!(self.slots.get(key.as_str()), Some(Slot::Input(_))) {
                return Err(fail(key, format!("`{key}` is not an input of the salary structure `{}`", self.structure.id)));
            }
        }
        let mut vars = Vec::with_capacity(self.inputs.len());
        for slot in &self.inputs {
            let name = slot.name.as_str();
            let value = match input.variables.get(name) {
                Some(json) => {
                    let value = value_from_json(json).map_err(|e| fail(name, format!("`{name}`: {e}")))?;
                    typed(value, slot.input_type).map_err(|e| fail(name, format!("`{name}` {e}")))?
                }
                None => slot.default.clone().ok_or_else(|| fail(name, format!("`{name}` is missing")))?,
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
            let fail = |kind: ComponentErrorKind, message: String| EngineError::Component { employee: employee.clone(), component: id.clone(), kind, message };
            let env = RunEnv {
                compiled: self,
                vars: &vars,
                amounts: &amounts,
                done: &done,
                tag_sums: &tag_sums,
                tag_overflow: &tag_overflow,
                reads: explain.then(|| RefCell::new(BTreeSet::new())),
            };

            let applied = match &item.when {
                Some(when) => match when.eval_with(&env, self.precision).map_err(|e| fail(ComponentErrorKind::Expression(e.kind), format!("when: {e}")))? {
                    Value::Bool(b) => b,
                    other => return Err(fail(ComponentErrorKind::NotACondition, format!("when gives true or false, not a {}", other.type_name()))),
                },
                None => true,
            };
            let amount = if !applied {
                item.rounding.apply(Decimal::ZERO)
            } else {
                let raw = if let Some(formula) = &item.formula {
                    match formula.eval_with(&env, self.precision).map_err(|e| fail(ComponentErrorKind::Expression(e.kind), e.to_string()))? {
                        Value::Num(n) => n,
                        other => return Err(fail(ComponentErrorKind::NotANumber, format!("a formula gives a number, not a {}", other.type_name()))),
                    }
                } else if let Some(fixed) = item.fixed {
                    fixed
                } else {
                    let position = item.input.unwrap_or_default();
                    let name = item.component.input.as_deref().unwrap_or_default();
                    match vars.get(position) {
                        Some(Value::Num(n)) => *n,
                        // Decimal text is a number given exactly (see `InputType::Number`).
                        Some(Value::Str(text)) => {
                            number_from_text(text).map_err(|e| fail(ComponentErrorKind::InputNotANumber, format!("the input `{name}`: {e}")))?
                        }
                        Some(other) => {
                            return Err(fail(ComponentErrorKind::InputNotANumber, format!("the input `{name}` is a {}, not a number", other.type_name())))
                        }
                        None => return Err(fail(ComponentErrorKind::InputNotANumber, format!("the input `{name}` is missing"))),
                    }
                };
                let rounded = item.rounding.checked_apply(raw).ok_or_else(|| {
                    fail(ComponentErrorKind::TooLarge, format!("it came to {raw}, too large to keep {} decimal places", item.rounding.digits()))
                })?;
                if rounded.is_sign_negative() && !rounded.is_zero() && !item.component.allow_negative && item.component.kind != Kind::Info {
                    return Err(fail(
                        ComponentErrorKind::Negative,
                        format!("it came to {rounded}: an amount is not negative (set allow_negative if it should be)"),
                    ));
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
                    // Under `Precision::Exact` a sum that would be rounded is refused, as a `+` in a formula is.
                    match self.precision.add(tag_sums[tag], amount) {
                        Ok(sum) => tag_sums[tag] = sum,
                        Err(_) => tag_overflow[tag] = true,
                    }
                }
            }
        }

        let too_large = |total: Total| EngineError::TooLarge { employee: employee.clone(), total };
        // All totals follow the structure's precision: under `Exact` a sum that would be rounded is refused.
        let add = |x: Decimal, y: Decimal, t: Total| self.precision.add(x, y).map_err(|_| too_large(t));
        let sub = |x: Decimal, y: Decimal, t: Total| self.precision.sub(x, y).map_err(|_| too_large(t));
        // Totals add the lines in the order the structure lists them.
        let total = |kind: Kind| {
            self.items
                .iter()
                .enumerate()
                .filter(|(_, i)| i.component.kind == kind)
                .try_fold(Decimal::ZERO, |sum, (n, _)| self.precision.add(sum, amounts[n]).ok())
        };
        let rounding = self.structure.rounding;
        let fix = |d: Option<Decimal>, t: Total| d.and_then(|d| rounding.checked_apply(d)).ok_or_else(|| too_large(t));
        // Totals are rounded before net is taken from them, so a payslip's gross - deductions is its net even
        // when a line keeps more digits than the structure does.
        let gross = fix(total(Kind::Earning), Total::Gross)?;
        let mut deductions = fix(total(Kind::Deduction), Total::Deductions)?;
        let employer = fix(total(Kind::Employer), Total::EmployerCost)?;
        let mut net = sub(gross, deductions, Total::Net)?;
        let mut shortfalls: Vec<Decimal> = self.items.iter().map(|i| i.rounding.apply(Decimal::ZERO)).collect();
        let mut shortfall = Decimal::ZERO;
        if net.is_sign_negative() && !net.is_zero() {
            match self.structure.negative_net {
                NegativeNet::Zero if !gross.is_sign_negative() || gross.is_zero() => {
                    // A negative deduction (a refund) gives money back, so all of those count first; then
                    // deductions are taken in priority order while pay lasts, and the rest of each is its
                    // shortfall.
                    let mut available = gross;
                    deductions = Decimal::ZERO;
                    let refunds = self.deduction_order.iter().filter(|i| amounts[**i].is_sign_negative());
                    let charges = self.deduction_order.iter().filter(|i| !amounts[**i].is_sign_negative());
                    for &index in refunds.chain(charges) {
                        let amount = amounts[index];
                        let taken = if amount <= available { amount } else { available.max(Decimal::ZERO) };
                        available = sub(available, taken, Total::Net)?;
                        deductions = add(deductions, taken, Total::Deductions)?;
                        shortfalls[index] = sub(amount, taken, Total::Deductions)?;
                        shortfall = add(shortfall, shortfalls[index], Total::Deductions)?;
                    }
                    deductions = fix(Some(deductions), Total::Deductions)?;
                    net = sub(gross, deductions, Total::Net)?;
                }
                NegativeNet::Allow => {}
                // `error`, or `zero` when gross pay itself is negative (no cut of deductions can bring net to zero).
                _ => return Err(EngineError::NegativeNet { employee: employee.clone(), net }),
            }
        }
        let summary =
            Summary { employee_id: employee.clone(), gross, deductions, net, employer_cost: employer, shortfall: fix(Some(shortfall), Total::Deductions)? };
        Ok(Calculated { amounts, applied: applied_flags, shortfalls, reads, summary })
    }

    /// The name of a component, input or constant from the numbering `RunEnv` uses for reads.
    fn name_at(&self, key: usize) -> String {
        let items = self.items.len();
        if key < items {
            self.items[key].component.id.clone()
        } else if key < items + self.inputs.len() {
            self.inputs[key - items].name.clone()
        } else if key < self.tag_key(0) {
            self.constant_names[key - items - self.inputs.len()].clone()
        } else {
            // Only explained payslips get here, so finding the tag's name by its number is fine.
            let tag = key - self.tag_key(0);
            let name = self.tag_names.iter().find(|(_, &t)| t == tag).map(|(name, _)| name.as_str()).unwrap_or_default();
            format!("total(\"{name}\")")
        }
    }

    /// The read key of a tag totalled with `total`: after every component, input and constant.
    fn tag_key(&self, tag: usize) -> usize {
        self.items.len() + self.inputs.len() + self.constants.len() + tag
    }

    fn payslip(&self, input: &Input, explain: bool) -> Result<Payslip, EngineError> {
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
                shortfall: done.shortfalls[index],
                explain: explain.then(|| Explanation {
                    source: item
                        .formula
                        .as_ref()
                        .map(|f| f.source().to_string())
                        .or_else(|| item.component.input.as_ref().map(|i| format!("input {i}")))
                        .unwrap_or_else(|| "fixed amount".to_string()),
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

    /// Calculate one person's payslip.
    pub fn run(&self, input: &Input) -> Result<Payslip, EngineError> {
        self.payslip(input, false)
    }

    /// Calculate one person's payslip, each line carrying its formula and the names it read.
    pub fn run_explained(&self, input: &Input) -> Result<Payslip, EngineError> {
        self.payslip(input, true)
    }

    /// Calculate one person with the given [`Detail`].
    pub fn run_with(&self, input: &Input, detail: Detail) -> Result<Payslip, EngineError> {
        self.payslip(input, detail == Detail::Explained)
    }

    /// Calculate one person and return the totals with every component's amount, in the order the structure
    /// lists its components (`structure().components`), and whether its condition was met. No labels, tags or
    /// ids are copied: a payslip that records which structure it was calculated with can show them from the
    /// structure, so this is the cheapest way to keep the lines of very many people.
    pub fn calculate_amounts(&self, input: &Input) -> Result<Calculation, EngineError> {
        let done = self.calculate(input, false)?;
        Ok(Calculation { summary: done.summary, amounts: done.amounts, applied: done.applied, shortfalls: done.shortfalls })
    }

    /// Calculate one person and keep only the totals. The same numbers as [`run`](Self::run), without
    /// building the payslip's lines: for a payroll of very many people, where the lines of each person are
    /// not wanted in memory all at once (calculate them again with `run` for the few who need a payslip).
    pub fn summarize(&self, input: &Input) -> Result<Summary, EngineError> {
        Ok(self.calculate(input, false)?.summary)
    }

    /// Calculate many people; in parallel when the `parallel` feature is on. A person who fails does
    /// not stop the others: each answer is its own result, in the order given. An `employee_id` seen
    /// earlier in the same call is refused, never paid twice ([`EngineError::Input`]).
    ///
    /// Every payslip stays in memory until the call returns (one line per component per person). For
    /// more than a few thousand people use [`for_each`](Self::for_each) or [`summarize`](Self::summarize).
    ///
    /// With `parallel`, the work runs on Rayon's current thread pool: to choose the number of threads, call
    /// this inside `rayon::ThreadPoolBuilder::new().num_threads(n).build()?.install(|| ...)`.
    pub fn run_all(&self, inputs: &[Input], detail: Detail) -> Vec<Result<Payslip, EngineError>> {
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            let repeated = repeated(inputs.iter(), &mut HashSet::new());
            inputs.par_iter().zip(repeated).map(|(i, again)| self.run_once(i, again, detail)).collect()
        }
        #[cfg(not(feature = "parallel"))]
        {
            let repeated = repeated(inputs.iter(), &mut HashSet::new());
            inputs.iter().zip(repeated).map(|(i, again)| self.run_once(i, again, detail)).collect()
        }
    }

    fn run_once(&self, input: &Input, repeated: bool, detail: Detail) -> Result<Payslip, EngineError> {
        if repeated {
            return Err(EngineError::Input {
                employee: input.employee_id.clone(),
                input: None,
                message: "this employee_id appears more than once in the run".to_string(),
            });
        }
        self.run_with(input, detail)
    }

    /// Calculate people and hand each answer to `sink`, in the order given, without holding more than
    /// `window` of them in memory at once. A person who fails does not stop the others, and a repeated
    /// `employee_id` is refused as in [`run_all`](Self::run_all). Runs on Rayon's current thread pool with
    /// the `parallel` feature.
    pub fn for_each<'a, I, F>(&self, inputs: I, detail: Detail, window: usize, mut sink: F)
    where
        I: IntoIterator<Item = &'a Input>,
        F: FnMut(&Input, Result<Payslip, EngineError>),
    {
        let window = window.max(1);
        let mut iter = inputs.into_iter();
        let mut seen = HashSet::new();
        loop {
            let batch: Vec<&Input> = iter.by_ref().take(window).collect();
            if batch.is_empty() {
                return;
            }
            let again = repeated(batch.iter().copied(), &mut seen);
            #[cfg(feature = "parallel")]
            let results: Vec<Result<Payslip, EngineError>> = {
                use rayon::prelude::*;
                batch.par_iter().zip(again).map(|(i, again)| self.run_once(i, again, detail)).collect()
            };
            #[cfg(not(feature = "parallel"))]
            let results: Vec<Result<Payslip, EngineError>> = batch.iter().zip(again).map(|(i, again)| self.run_once(i, again, detail)).collect();
            for (input, result) in batch.into_iter().zip(results) {
                sink(input, result);
            }
        }
    }
}

/// For each input, whether its `employee_id` is in `seen` already (adding the new ones).
fn repeated<'a>(inputs: impl Iterator<Item = &'a Input>, seen: &mut HashSet<&'a str>) -> Vec<bool> {
    // Trimmed, as the empty-id check is: `"e1"` and `" e1"` are the same person.
    inputs.map(|i| !seen.insert(i.employee_id.trim())).collect()
}
